#![no_std]
#![no_main]
// `rsil`/`wsr.ps` are the interrupt mask, and inline asm is still unstable on
// this architecture. The `esp` toolchain is nightly-based, so this is the same
// gate `rusty_esp_dsp`'s PIE kernels and Kairos's own cells use.
#![cfg_attr(target_arch = "xtensa", feature(asm_experimental_arch))]
//! Two tasks **scheduled by the Kairos kernel**, on a XIAO ESP32-S3, reached
//! through the Janus seam.
//!
//! # What is new here
//!
//! Kairos's own `xiao-s3-switch` proves the Xtensa context switch on silicon
//! and is explicit about its limit: *"there is no tick preemption here and no
//! priority scheduler — `Kernel::switch_context` is not in this loop."*
//!
//! This cell puts it in the loop. Every switch below is **decided by
//! `Kernel::switch_context`** — the fixed-priority scheduler choosing from
//! its own ready lists — and only then enacted by the port. And the kernel
//! and the port are reached through [`rusty_esp_rtos`], never named
//! directly, so what passes here is the *seam*, which is Janus's half of K5.
//!
//! # Why the tasks yield from three calls deep
//!
//! Xtensa keeps a 64-entry physical register file behind a rotating window.
//! A task several calls deep has several live windows in that file, and a
//! switch that mishandles them corrupts the *outer* frames — far from the
//! cause, and invisible to a probe that only ever yields from the task body.
//! So each worker yields from `level_one` -> `level_two` -> `level_three`,
//! and every frame re-checks a witness it held across the switch.
//!
//! # The shape
//!
//! `main` becomes a kernel task too, at priority 1. The two workers sit at
//! priority 2, so while either is ready `main` cannot run. Each worker
//! ping-pongs `LAPS` times through the kernel, then **suspends itself**;
//! when both have, `main` is the only ready task, wakes, and reports.
//!
//! Cooperative on purpose: no tick, no timer peripheral. The claim is that
//! the kernel *chooses* and the port *enacts*, which needs no preemption to
//! demonstrate and far less machinery to get wrong.
//!
//! # PASS
//!
//! Both workers reach `LAPS`, every witness word survives every resumption,
//! the kernel reports a real swap for each hand-off, and `main` regains the
//! CPU — which it can only do if the scheduler actually parked both workers.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};

use esp_backtrace as _;
use esp_println::println;

use rusty_esp_rtos::core_types::config::Config;
use rusty_esp_rtos::core_types::handle::TaskHandle;
use rusty_esp_rtos::core_types::hooks::NoTickHook;
use rusty_esp_rtos::core_types::tick::Bits32;
use rusty_esp_rtos::core_types::trace::{Event, Trace};
use rusty_esp_rtos::kernel::{Kernel, items_for, lists_for};
use rusty_esp_rtos::port::{
    Context, XtensaPort, clear_switch_request, enable_switching, new_task_context, switch_context,
    yield_now,
};

esp_bootloader_esp_idf::esp_app_desc!();

/// Hand-offs each worker must complete.
const LAPS: u32 = 50;
/// Words of witness on each worker's own stack.
const WITNESS: usize = 64;
/// Bytes of stack per worker.
const STACK: usize = 8 * 1024;
/// Workers, plus `main`.
const SLOTS: usize = 3;
/// `main`'s slot index.
const MAIN: usize = 2;

// ------------------------------------------------------------ the kernel --

/// Four priorities: idle at 0, the timer daemon at 1, the workers at 2 and
/// `main` at 3.
///
/// `main` is HIGHEST on purpose, and the reason cost a board run. The kernel
/// creates `Tmr Svc` inside `start_scheduler`, unconditionally, at
/// `TIMER_TASK_PRIORITY` — and `create_task` makes the highest-priority task
/// current. With the daemon above `main` the kernel believed a task with no
/// stack was running: `current_idx=4` while the CPU was on `main`'s stack,
/// so every switch was declined as `from == to` and both workers starved at
/// `laps 0/0`. Putting `main` on top keeps the kernel's view and the CPU's
/// in agreement from the first instruction.
#[derive(Debug, Clone, Copy, Default)]
pub struct S3Config;

impl Config for S3Config {
    type Tick = Bits32;
    const TICK_RATE_HZ: u32 = 1000;
    const MAX_PRIORITIES: u8 = 4;
    const MINIMAL_STACK_SIZE: usize = 128;
    const MAX_TASK_NAME_LEN: usize = 8;
    const TIMER_TASK_PRIORITY: u8 = 1;
    const TIMER_TASK_STACK_DEPTH: usize = 128;
    const TIMER_QUEUE_LENGTH: usize = 2;
    const NOTIFICATION_ARRAY_ENTRIES: usize = 1;
}

/// This cell asserts scheduling, not a trace.
#[derive(Debug, Default)]
struct NoTrace;
impl Trace for NoTrace {
    fn event(&mut self, _tick: u64, _event: Event<'_>) {}
}

const TASKS: usize = 6;
const QUEUES: usize = 2;
const QSLOTS: usize = 8;
const TIMERS: usize = 1;
const GROUPS: usize = 1;

type K = Kernel<
    S3Config,
    XtensaPort,
    NoTrace,
    NoTickHook,
    TASKS,
    { items_for(TASKS, TIMERS) },
    { lists_for(S3Config::MAX_PRIORITIES, QUEUES, GROUPS) },
    QUEUES,
    QSLOTS,
    1,
    8,
    TIMERS,
    GROUPS,
>;

struct KernelCell(UnsafeCell<Option<K>>);
// SAFETY: one core. Every access is either from task context with interrupts
// masked, or from the `Software0` handler, which runs with interrupts already
// masked and no task on the CPU.
unsafe impl Sync for KernelCell {}
static KERNEL: KernelCell = KernelCell(UnsafeCell::new(None));

/// Borrow the kernel with interrupts masked.
///
/// The closure must not yield: the lock IS the interrupt mask, and holding
/// it across a switch would run another task with interrupts off.
fn with_kernel<R>(f: impl FnOnce(&mut K) -> Option<R>) -> Option<R> {
    let saved = mask();
    // SAFETY: interrupts are masked and there is one core.
    let out = unsafe {
        match (*KERNEL.0.get()).as_mut() {
            Some(k) => f(k),
            None => None,
        }
    };
    unmask(saved);
    out
}

/// Raise INTLEVEL and return the previous `PS`.
#[inline(always)]
fn mask() -> u32 {
    let ps: u32;
    // SAFETY: reads PS and raises INTLEVEL; touches no memory.
    unsafe {
        core::arch::asm!("rsil {0}, 3", out(reg) ps, options(nostack));
    }
    ps
}

/// Restore `PS` — the whole register, not a level. Setting INTLEVEL back to
/// a guessed 0 would enable interrupts that were masked before the call.
#[inline(always)]
fn unmask(ps: u32) {
    // SAFETY: `ps` is a state this core was already in.
    unsafe {
        core::arch::asm!("wsr.ps {0}", "rsync", in(reg) ps, options(nostack));
    }
}

// ------------------------------------------------------------- the slots --

/// One `Context` per task that owns a stack. The idle task and the timer
/// daemon have none, and a switch naming them is declined.
struct Slots(UnsafeCell<[Option<(TaskHandle, *mut Context)>; SLOTS]>);
// SAFETY: written once during boot, before switching is enabled; read only
// from the switch handler afterwards.
unsafe impl Sync for Slots {}
static SLOTS_TAB: Slots = Slots(UnsafeCell::new([None; SLOTS]));

fn register(idx: usize, handle: TaskHandle, ctx: *mut Context) {
    // SAFETY: boot only, before `enable_switching`.
    unsafe {
        (*SLOTS_TAB.0.get())[idx] = Some((handle, ctx));
    }
}

fn context_of(handle: TaskHandle) -> Option<*mut Context> {
    // SAFETY: the table is immutable after boot.
    unsafe {
        (*SLOTS_TAB.0.get())
            .iter()
            .flatten()
            .find(|(h, _)| *h == handle)
            .map(|(_, c)| *c)
    }
}

static STARTED: AtomicU32 = AtomicU32::new(0);
static ENTRIES: AtomicU32 = AtomicU32::new(0);
static SWAPS: AtomicU32 = AtomicU32::new(0);
static SAME: AtomicU32 = AtomicU32::new(0);
static NO_CTX: AtomicU32 = AtomicU32::new(0);
static FAULTS: AtomicU32 = AtomicU32::new(0);
static LAPS_A: AtomicU32 = AtomicU32::new(0);
static LAPS_B: AtomicU32 = AtomicU32::new(0);
/// Workers that have finished their laps.
static DONE: AtomicU32 = AtomicU32::new(0);
/// `main`'s handle, so the last worker out can resume it.
///
/// A cell rather than an index: `TaskHandle` is generational and has no
/// `from_index`, which is the right call by the kernel — an index alone
/// cannot distinguish a live task from a dead one that held the same slot.
struct MainCell(UnsafeCell<Option<TaskHandle>>);
// SAFETY: written once at boot before switching is enabled, read afterwards.
unsafe impl Sync for MainCell {}
static MAIN_H: MainCell = MainCell(UnsafeCell::new(None));

// ---------------------------------------------------------- the switch --

/// `Software0`: the one place a switch is decided AND enacted.
///
/// `XtensaPort::COMMITS_SWITCH` is true, so `Kernel::port_yield` raises this
/// exception and leaves `current` alone — the kernel does not move `current`
/// while task code is still on the CPU. Deciding and enacting together here
/// is what keeps the two from drifting apart.
#[esp_hal::ram]
#[unsafe(export_name = "Software0")]
fn switching_interrupt(trap_frame: &mut Context) {
    ENTRIES.fetch_add(1, Ordering::Relaxed);
    clear_switch_request();
    if STARTED.load(Ordering::Acquire) == 0 {
        return;
    }

    let moved = {
        // SAFETY: an interrupt handler on a single core: interrupts are
        // already masked, so this borrow is exclusive.
        let k = unsafe { (*KERNEL.0.get()).as_mut() };
        match k {
            None => None,
            Some(k) => {
                let from = k.current();
                k.switch_context();
                let to = k.current();
                if from == to {
                    SAME.fetch_add(1, Ordering::Relaxed);
                    None
                } else {
                    Some((from, to))
                }
            }
        }
    };

    let Some((from, to)) = moved else { return };
    let (Some(out), Some(into)) = (context_of(from), context_of(to)) else {
        NO_CTX.fetch_add(1, Ordering::Relaxed);
        return;
    };
    SWAPS.fetch_add(1, Ordering::Relaxed);
    // SAFETY: both pointers name live statics, and `trap_frame` is the frame
    // this handler was handed.
    unsafe { switch_context(Some(out), into, trap_frame) };
}

// ------------------------------------------------------------- the work --

/// Yield, then prove the frame survived it.
fn level_three(seed: u32, laps: &AtomicU32) {
    let witness = seed ^ 0x3333_3333;
    yield_now();
    if core::hint::black_box(witness) != seed ^ 0x3333_3333 {
        FAULTS.fetch_add(1, Ordering::Relaxed);
    }
    laps.fetch_add(1, Ordering::Relaxed);
}

fn level_two(seed: u32, laps: &AtomicU32) {
    let witness = seed ^ 0x2222_2222;
    level_three(seed, laps);
    if core::hint::black_box(witness) != seed ^ 0x2222_2222 {
        FAULTS.fetch_add(1, Ordering::Relaxed);
    }
}

fn level_one(seed: u32, laps: &AtomicU32) {
    let witness = seed ^ 0x1111_1111;
    level_two(seed, laps);
    if core::hint::black_box(witness) != seed ^ 0x1111_1111 {
        FAULTS.fetch_add(1, Ordering::Relaxed);
    }
}

/// A worker body. `seed` distinguishes the two; `laps` is its counter.
fn worker(seed: u32, laps: &'static AtomicU32) -> ! {
    // The witness array lives on THIS task's own stack, which is what a
    // mishandled register file corrupts.
    let mut stack_witness = [0u32; WITNESS];
    for (i, w) in stack_witness.iter_mut().enumerate() {
        *w = seed.wrapping_mul(i as u32 + 1);
    }

    while laps.load(Ordering::Relaxed) < LAPS {
        level_one(seed, laps);
        for (i, w) in stack_witness.iter().enumerate() {
            if *w != seed.wrapping_mul(i as u32 + 1) {
                FAULTS.fetch_add(1, Ordering::Relaxed);
                break;
            }
        }
    }

    // Stand down, and the LAST one out wakes `main`. If the scheduler does
    // not honour either half, `main` never prints and the cell fails by
    // silence rather than by a wrong number.
    let done = DONE.fetch_add(1, Ordering::AcqRel) + 1;
    let _ = with_kernel(|k: &mut K| {
        if done >= 2 {
            // SAFETY: written at boot, immutable since.
            if let Some(h) = unsafe { *MAIN_H.0.get() } {
                let _ = k.resume(h);
            }
        }
        k.suspend(None).ok()
    });
    loop {
        yield_now();
    }
}

/// The C-ABI trampoline `new_task_context` starts a task through.
extern "C" fn task_entry(task_fn: usize, param: usize) {
    // SAFETY: `task_fn` is the `fn(u32, &'static AtomicU32) -> !` this
    // firmware passed to `new_task_context`, and `param` is its seed.
    let f: fn(u32, &'static AtomicU32) -> ! = unsafe { core::mem::transmute(task_fn) };
    let laps: &'static AtomicU32 = if param == 0xA { &LAPS_A } else { &LAPS_B };
    f(param as u32, laps);
}

/// A worker's stack, 16-byte aligned because `new_task_context` masks the
/// top down to that and would otherwise hand back less room than asked for.
///
/// The field is never READ through the struct — it is addressed as memory —
/// which is exactly what the lint says and not a defect.
#[repr(align(16))]
#[allow(dead_code)]
struct Stack([u8; STACK]);
static mut STACK_A: Stack = Stack([0; STACK]);
static mut STACK_B: Stack = Stack([0; STACK]);
static mut CTX: [Context; SLOTS] = [Context::new(), Context::new(), Context::new()];

// ------------------------------------------------------------------ main --

#[esp_hal::main]
fn main() -> ! {
    let _p = esp_hal::init(esp_hal::Config::default());
    println!("== JANUS KAIROS TASKS xiao-s3 ==");
    println!(
        "KT seam_port={} scheduler={}",
        rusty_esp_rtos::port_name().unwrap_or("none"),
        rusty_esp_rtos::compat::HAS_SCHEDULER
    );

    let kernel = match K::new(XtensaPort::new(), NoTrace) {
        Ok(k) => k,
        Err(_) => {
            println!("RESULT: FAIL -- kernel would not build");
            loop {}
        }
    };
    // SAFETY: boot, before any task or interrupt can reach it.
    unsafe {
        *KERNEL.0.get() = Some(kernel);
    }

    // `main` is a task too, below the workers, so that yielding hands them
    // the CPU and suspending them hands it back.
    let Some(main_h) = with_kernel(|k: &mut K| k.create_task("main", 3).ok()) else {
        println!("RESULT: FAIL -- no main task");
        loop {}
    };
    // SAFETY: single-threaded boot; nothing else names these yet.
    unsafe {
        register(MAIN, main_h, &raw mut CTX[MAIN]);
    }
    // SAFETY: boot, before any task or interrupt can read it.
    unsafe {
        *MAIN_H.0.get() = Some(main_h);
    }

    for (idx, (name, seed, stack)) in [
        ("wa", 0xAusize, &raw mut STACK_A),
        ("wb", 0xBusize, &raw mut STACK_B),
    ]
    .into_iter()
    .enumerate()
    {
        // SAFETY: the stack is `STACK` bytes and lives for the program; one
        // past its end is `base + STACK`.
        let ctx = unsafe {
            new_task_context(
                task_entry,
                worker as *const () as usize,
                seed,
                (stack as *mut u8).add(STACK),
            )
        };
        // SAFETY: boot only.
        unsafe {
            CTX[idx] = ctx;
        }
        let Some(h) = with_kernel(|k: &mut K| k.create_task(name, 2).ok()) else {
            println!("RESULT: FAIL -- no worker {name}");
            loop {}
        };
        // SAFETY: boot only, before switching is enabled.
        unsafe {
            register(idx, h, &raw mut CTX[idx]);
        }
    }

    if with_kernel(|k: &mut K| k.start_scheduler().ok()).is_none() {
        println!("RESULT: FAIL -- scheduler would not start");
        loop {}
    }
    enable_switching();
    STARTED.store(1, Ordering::Release);

    // What the kernel thinks, before anything yields. If `current` is not
    // `main`, or the workers are not ready at 2, every conclusion below is
    // about the wrong thing -- so ask rather than assume.
    let cur = with_kernel(|k: &mut K| Some(k.current())).unwrap_or_default();
    println!(
        "KT boot main_idx={} current_idx={} ready1={:?} ready2={:?}",
        main_h.index(),
        cur.index(),
        with_kernel(|k: &mut K| k.ready_len(1).ok()),
        with_kernel(|k: &mut K| k.ready_len(2).ok())
    );

    // Hand the CPU over by BLOCKING, not yielding. `main` outranks the
    // workers, so a yield would pick `main` again; suspending takes it out
    // of the ready lists entirely and leaves the workers as the highest
    // ready pair. The last worker to finish resumes it.
    //
    // This is also the stronger claim: `suspend`/`resume` are scheduler
    // operations, so a pass means the kernel moved tasks between its own
    // ready and suspended lists, not merely that a yield rotated two peers.
    let _ = with_kernel(|k: &mut K| k.suspend(None).ok());

    let a = LAPS_A.load(Ordering::Relaxed);
    let b = LAPS_B.load(Ordering::Relaxed);
    let faults = FAULTS.load(Ordering::Relaxed);
    let swaps = SWAPS.load(Ordering::Relaxed);
    println!("KT laps_a={a} laps_b={b} want={LAPS} faults={faults}");
    println!(
        "KT entries={} swaps={swaps} declined_same={} declined_no_ctx={}",
        ENTRIES.load(Ordering::Relaxed),
        SAME.load(Ordering::Relaxed),
        NO_CTX.load(Ordering::Relaxed)
    );

    if a >= LAPS && b >= LAPS && faults == 0 && swaps > 0 {
        println!(
            "RESULT: PASS -- {a} and {b} hand-offs chosen by Kernel::switch_context, \
             every witness intact, main resumed"
        );
    } else {
        println!("RESULT: FAIL");
    }
    loop {}
}
