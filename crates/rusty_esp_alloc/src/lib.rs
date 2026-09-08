//! The allocator seam: `rusty_alloc` as the family's global allocator.
//!
//! Every deliverable in Janus declares its global allocator through this
//! crate and never names `rusty_alloc` itself. That is the house rule, and it
//! buys one thing: the exact pin, the profile and the startup shape are
//! decided here once instead of in every firmware and every CLI.
//!
//! **A library must never declare a global allocator.** A program may define
//! exactly one, so a library that declares it forces the choice on every
//! consumer and makes two such libraries impossible to link together. This
//! crate does not declare one either — it hands out the type and the region,
//! and the deliverable's own `main.rs` does the declaring.
//!
//! # A hosted deliverable
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOC: rusty_esp_alloc::Alloc = rusty_esp_alloc::Alloc;
//! ```
//!
//! # A firmware
//!
//! A chip has no operating system to ask for memory: it has a region the
//! linker gave it. [`Region`] is that region, handed over once at startup.
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOC: rusty_esp_alloc::Alloc = rusty_esp_alloc::Alloc;
//! static HEAP: rusty_esp_alloc::Region<{ 96 * 1024 }> = rusty_esp_alloc::Region::new();
//!
//! # fn main() {
//! HEAP.give().expect("the heap is given once");
//! # }
//! ```
//!
//! # Which allocator a firmware should actually use
//!
//! Not this one, necessarily, and the honest answer is in rusty_alloc's own
//! README: on a XIAO ESP32-S3 it is **2.0x to 3.7x faster** than `esp-alloc`
//! per allocate/free pair, and it needs **68 KiB where `esp-alloc` needs 8**,
//! because a size-class page allocator's floor is `classes touched x page
//! size` rather than `bytes live`. That is structural, not a missing
//! optimisation.
//!
//! So: reach for this when a firmware **churns** — allocates and frees
//! repeatedly at varied sizes, which is what fragments a free list — and has
//! the RAM. Reach for `esp-alloc` when the budget is tight. A firmware that
//! allocates a handful of buffers at startup and never allocates again gains
//! nothing here and pays the whole floor, and several of ours are exactly
//! that shape.
//!
//! The reason to adopt it anyway, where the RAM allows, is the safety
//! posture rather than the speed: **a double free aborts** instead of putting
//! one block on a free list twice and handing identical memory to two owners.
//! Treat that abort as a bug to fix, never a check to disable.
//!
//! # The single-thread invariant a firmware must assert
//!
//! rusty_alloc'''s `no_std` profile assumes **one thread**: its cell'''s `Sync`,
//! the fixed backend'''s constant thread id and spin lock, and the split
//! 64-bit atomics in its options all depend on it. The crate will not build
//! without the caller saying so, which is the right way round -- it refuses
//! rather than assuming.
//!
//! A firmware opts in through its own `.cargo/config.toml`:
//!
//! ```toml
//! [build]
//! rustflags = ["--cfg", "ra_single_threaded"]
//! ```
//!
//! **Only assert that if it is true.** On an ESP32-S3 it holds for a Track B
//! firmware that runs `esp_hal::main` on one core and never starts the app
//! core. It stops holding the moment a second core is started **or an
//! interrupt handler allocates** -- an interrupt is another context even on
//! one core. A firmware that needs either wants `esp-alloc`, or rusty_alloc
//! with the `std` profile if a target ever offers one.

#![cfg_attr(not(feature = "std"), no_std)]
// The region is a `static` whose bytes are handed out as `&'static mut` once.
// There is no safe way to express that, and the whole point of this crate is
// that the unsafe lives here rather than in every firmware.
#![allow(unsafe_code)]

/// The global allocator type. A deliverable writes
/// `#[global_allocator] static A: Alloc = Alloc;` and nothing else.
pub use rusty_alloc_api::RustyAlloc as Alloc;

/// The pinned allocator version, so a firmware can print what it is running
/// rather than what its manifest said.
pub use rusty_alloc_api::VERSION;

/// What can go wrong handing over a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// [`Region::give`] was called twice. The allocator takes its memory once.
    AlreadyGiven,
    /// The backend refused: the region is smaller than one page of its
    /// bookkeeping, or another region is already registered.
    Refused,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::AlreadyGiven => f.write_str("the heap region was already given"),
            Error::Refused => f.write_str("the allocator refused the region (too small?)"),
        }
    }
}

/// A chip's heap: `N` bytes of static storage, handed to the allocator once.
///
/// Only exists where there is no operating system to allocate from. On a
/// hosted target the allocator asks the OS and a deliverable declares
/// [`Alloc`] with no region at all.
///
/// `N` is the whole budget, and rusty_alloc's floor is set by how many size
/// classes the program touches rather than how many bytes it holds — see this
/// module's note on which allocator to use. Below about 68 KiB on an ESP32-S3
/// a real workload starts being refused, so a firmware that cannot spare that
/// should stay on `esp-alloc`.
#[cfg(not(any(unix, windows, target_arch = "wasm32")))]
pub struct Region<const N: usize> {
    cell: core::cell::UnsafeCell<[u8; N]>,
    given: core::sync::atomic::AtomicBool,
}

// SAFETY: the bytes are handed out exactly once, and `given` is what enforces
// it: the first `give` swaps it true and every later call is refused without
// touching the cell. Nothing else in this crate reads or writes `cell`, so
// there is never a second reference to alias the `&'static mut` that call
// produced.
#[cfg(not(any(unix, windows, target_arch = "wasm32")))]
unsafe impl<const N: usize> Sync for Region<N> {}

#[cfg(not(any(unix, windows, target_arch = "wasm32")))]
impl<const N: usize> Region<N> {
    /// Reserve `N` bytes. `const`, so this is a `static` and the bytes are in
    /// the image's BSS rather than on anybody's stack.
    #[must_use]
    pub const fn new() -> Self {
        Region {
            cell: core::cell::UnsafeCell::new([0; N]),
            given: core::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Hand the region to the allocator. Call once, before the first
    /// allocation; a second call is refused rather than aliasing the first.
    ///
    /// # Errors
    /// [`Error::AlreadyGiven`] on a second call, [`Error::Refused`] if the
    /// backend will not take it.
    pub fn give(&'static self) -> Result<(), Error> {
        use core::sync::atomic::Ordering;
        if self.given.swap(true, Ordering::SeqCst) {
            return Err(Error::AlreadyGiven);
        }
        // SAFETY: the swap above succeeded, so this is the first and only
        // call, and no other code in this crate touches `cell`. `self` is
        // `&'static`, so the bytes live for the program and the `&'static mut`
        // this produces is the only reference to them.
        let bytes: &'static mut [u8] = unsafe { &mut *self.cell.get() };
        rusty_alloc::prim::fixed::init_region(bytes).map_err(|_| Error::Refused)
    }

    /// The region's size in bytes, for a firmware that wants to report its own
    /// budget beside what the allocator says it is using.
    #[must_use]
    pub const fn len(&self) -> usize {
        N
    }

    /// Whether the region is empty. Present because clippy asks for it beside
    /// [`Region::len`]; a zero-length region would be refused anyway.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        N == 0
    }
}

#[cfg(not(any(unix, windows, target_arch = "wasm32")))]
impl<const N: usize> Default for Region<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// What the allocator has done with the region: `(used, free, total)` bytes.
///
/// The counterpart to `esp_alloc::HEAP.stats()`, and the reason a firmware can
/// report its own footprint after a swap instead of guessing. A fixed-region
/// allocator that cannot say how much of its region is out is unmeasurable on
/// exactly the deployment it exists for.
#[cfg(not(any(unix, windows, target_arch = "wasm32")))]
#[must_use]
pub fn occupancy() -> (usize, usize, usize) {
    rusty_alloc::prim::fixed::region_stats()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_error_messages_say_what_went_wrong() {
        assert_eq!(
            Error::AlreadyGiven.to_string(),
            "the heap region was already given"
        );
        assert!(Error::Refused.to_string().contains("refused"));
    }

    #[test]
    fn the_pinned_version_is_the_one_the_manifest_names() {
        // The seam exists to hold one pin; if the dependency moves under it,
        // this is where that is noticed rather than in a firmware.
        assert!(
            VERSION.starts_with("2.0."),
            "expected rusty_alloc 2.0.x, got {VERSION}"
        );
    }
}
