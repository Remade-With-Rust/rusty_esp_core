//! `rusty_esp_boot`: the Janus second-stage bootloader for the ESP32-S3.
//!
//! What ESP-IDF's C bootloader does, in the same order, with the same ROM
//! underneath: the watchdog and analog-reset housekeeping the ROM leaves to
//! the second stage, the flash size from its own header, the PLL and the
//! 80 MHz CPU clock with the flash at the header's rate, the partition
//! table at `0x8000`, `otadata` with the rollback rule (a `New` image gets
//! one try as `PendingVerify`; one still pending on the next boot is
//! `Aborted` and the previous slot boots), the chosen app image mapped
//! through the cache and verified as it is walked (chip id, XOR checksum,
//! appended SHA-256, and under secure boot v2 its RSA-PSS signature sector
//! through the ROM's own verifier), its RAM segments copied in, its flash
//! segments mapped through the cache MMU, the flash clock divider the app
//! will run with, and the jump to its entry. Nothing of the image runs
//! before every check has passed; a refused image is named on the console
//! and the next candidate is tried, as the C loader tries them.
//!
//! The loader lives in the C bootloader's corner of SRAM (`memory.x`), so
//! every image linked for that one fits under this one unchanged, and it
//! hands over the clock the C one hands over: 80 MHz on the PLL.
#![no_std]
#![no_main]

use core::ptr::addr_of_mut;

// Pulls the ROM's linker scripts into the link.
use esp_rom_sys as _;
use xtensa_lx_rt::entry;

#[macro_use]
mod console;
mod flash;
mod image;
mod rom;
mod secure;
mod sha;
mod soc;
mod table;

use secure::Policy;
use table::{Choice, Part, Table};

/// The partition table, word-aligned because the ROM reads whole words.
static mut TABLE: [u32; table::TABLE_WORDS] = [0; table::TABLE_WORDS];
static mut SHA: sha::Sha = sha::Sha::new();

#[entry]
fn main() -> ! {
    soc::early_init();
    let own = match soc::own_header() {
        Ok(h) => h,
        Err(e) => fatal!("own header at 0x0: {:?}", e),
    };
    soc::set_flash_size(own.size_bytes());
    #[cfg(feature = "flash-diag")]
    {
        let (id, lo, hi, a, b) = soc::flash_status();
        say!("flash id {:#08x} status {:#x} ({}) statushigh {:#x} ({})", id, lo, a, hi, b);
    }
    #[cfg(feature = "jump-stamp")]
    let t_clk = soc::now_us();
    let clocks = soc::clocks_up(own.freq_div());
    // diagnostics only: what the clock change costs, for the jump stamp
    #[cfg(feature = "jump-stamp")]
    // SAFETY: one core, before the jump; nothing else reads it.
    unsafe {
        soc::STAMP_US = [t_clk, soc::now_us()];
    }
    #[cfg(feature = "qio")]
    let qio = soc::qio_if_enabled();
    #[cfg(not(feature = "qio"))]
    let qio = false;
    let t_main = soc::now_us();
    let policy = secure::policy();
    #[cfg(feature = "verbose")]
    {
        let (clk0, ctrl0, clk1) = soc::flash_regs();
        say!(
            "v{} esp32s3, {}; flash {} {} MHz {} MB; cpu {} MHz {} (the ROM's {}), uart source {}, spi0 clk={:#x} ctrl={:#x} spi1 clk={:#x}",
            env!("CARGO_PKG_VERSION"),
            match policy {
                Policy::Open => "open: checksum and appended SHA-256",
                Policy::Compiled(_) => "secure boot v2 in software: the compiled-in key digest",
                Policy::Efuse => "secure boot v2: the eFuse key digests",
            },
            if qio { "qio" } else { own.mode_name() },
            own.speed_mhz(),
            own.size_bytes() >> 20,
            clocks.cpu_after,
            if clocks.pll { "on the PLL" } else { "on the crystal" },
            clocks.cpu_before,
            clocks.uart_source,
            clk0,
            ctrl0,
            clk1
        );
    }
    #[cfg(not(feature = "verbose"))]
    say!(
        "v{} {}, {} {} MHz",
        env!("CARGO_PKG_VERSION"),
        match policy {
            Policy::Open => "open",
            Policy::Compiled(_) => "secure boot (compiled key)",
            Policy::Efuse => "secure boot (eFuse)",
        },
        if qio { "qio" } else { own.mode_name() },
        own.speed_mhz()
    );

    // SAFETY: the statics are used from this one function, on the one core
    // that runs, and the references never escape it.
    let (tbl, sha) = unsafe { (&mut *addr_of_mut!(TABLE), &mut *addr_of_mut!(SHA)) };

    // the table read and the image walk at 160 MHz, back to 80 for the
    // hand-over (R10); on the crystal, as the ROM left it
    if clocks.pll {
        // SAFETY: `clocks_up` put the CPU on the PLL at 80 MHz.
        unsafe { soc::cpu_160() };
    }

    let table = match table::read(tbl) {
        Ok(t) => t,
        Err(e) => fatal!("partition table at {:#x}: {:?}", table::TABLE_OFFSET, e),
    };
    let selected = match table::select(&table) {
        Ok(s) => s,
        Err(e) => fatal!("otadata: {:?}", e),
    };
    let t_table = soc::now_us();

    let mut tried = 0;
    for choice in candidates(&table, selected.choice) {
        let Some(part) = part_of(&table, choice) else { continue };
        tried += 1;
        let t_begin = soc::now_us();
        let view = match flash::map(part.offset, part.len) {
            Ok(v) => v,
            Err(e) => {
                say!("{} at {:#x} refused: map {:?}", choice, part.offset, e);
                continue;
            }
        };
        let img = match image::load(view, part.offset, sha) {
            Ok(img) => img,
            Err(e) => {
                say!("{} at {:#x} refused: {:?}", choice, part.offset, e);
                continue;
            }
        };
        let t_load = soc::now_us();
        if !matches!(policy, Policy::Open) {
            if let Err(e) = secure::verify(view, img.len, &policy, sha) {
                say!("{} at {:#x} refused: signature {:?}", choice, part.offset, e);
                continue;
            }
        }
        let t_verify = soc::now_us();
        #[cfg(feature = "verbose")]
        if choice == selected.choice {
            say!("{} otadata seq {} state {}", choice, selected.seq, table::state_name(selected.state));
        }
        if choice != selected.choice {
            say!("{} is a fallback: the selected slot was refused", choice);
        }
        if let (true, Choice::Ota(index)) = (selected.initial, choice) {
            table::write_initial(&table, index);
        }
        if clocks.pll {
            // SAFETY: on the PLL since `clocks_up`.
            unsafe { soc::cpu_80() };
        }
        #[cfg(feature = "verbose")]
        {
            let (cpu_now, sysclk, cpu_per) = soc::cpu_regs();
            say!(
                "Loaded app from partition at offset {:#x}: {} B, {} segments, {} B into RAM, entry {:#x}; ms: rom {} table {} load {} signature {}, {} since reset; cpu {} MHz sysclk={:#x} cpu_per={:#x}",
                part.offset,
                img.len,
                img.segments,
                img.ram_bytes,
                img.entry,
                t_main / 1000,
                (t_table - t_main) / 1000,
                (t_load - t_begin) / 1000,
                (t_verify - t_load) / 1000,
                soc::now_us() / 1000,
                cpu_now,
                sysclk,
                cpu_per
            );
        }
        // the prefix every runner matches, ESP-IDF's own words; the slot's
        // otadata state and the time since reset in the same line
        #[cfg(not(feature = "verbose"))]
        say!(
            "Loaded app from partition at offset {:#x} ({} seq {} {}); load {} ms, {} ms since reset",
            part.offset,
            choice,
            selected.seq,
            table::state_name(selected.state),
            (t_load - t_begin) / 1000,
            soc::now_us() / 1000
        );
        #[cfg(not(feature = "verbose"))]
        let _ = (t_main, t_table, t_verify);
        // SAFETY: the image passed every check above and its RAM segments
        // are in place; this never returns.
        unsafe { soc::start(img.entry, img.drom, img.irom, own.freq_div()) }
    }
    fatal!("no bootable app image ({} candidates tried)", tried)
}

/// The order the C loader tries partitions in: the selected one, then every
/// OTA slot in table order, then the factory app.
fn candidates(table: &Table, first: Choice) -> impl Iterator<Item = Choice> + '_ {
    core::iter::once(first)
        .chain((0..table.ota_count).map(Choice::Ota))
        .chain(core::iter::once(Choice::Factory))
        .enumerate()
        .filter(move |(i, c)| *i == 0 || *c != first)
        .map(|(_, c)| c)
}

fn part_of(table: &Table, choice: Choice) -> Option<Part> {
    match choice {
        Choice::Factory => table.factory,
        Choice::Ota(i) => table.ota.get(i).copied().flatten(),
    }
}

/// Says why, lets the console drain, waits two seconds and resets, as the
/// C loader's `bootloader_reset` does: a board that cannot boot keeps saying
/// so rather than sitting silent.
pub fn fatal(args: core::fmt::Arguments<'_>) -> ! {
    say!("{}", args);
    // SAFETY: ROM routines with no preconditions.
    unsafe {
        rom::uart_tx_wait_idle(0);
        rom::ets_delay_us(2_000_000);
        rom::software_reset();
    }
    loop {}
}

#[macro_export]
macro_rules! fatal {
    ($($arg:tt)*) => {
        $crate::fatal(format_args!($($arg)*))
    };
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    fatal!("panic: {}", info)
}

/// The ROM loaded `.data` at its own address: there is nothing to copy.
#[no_mangle]
pub extern "C" fn __init_data() -> bool {
    false
}
