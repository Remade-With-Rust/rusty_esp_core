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
///
/// The three backend refusals are kept apart because they have three
/// different fixes, which is the distinction rusty_alloc 2.0.1 added after
/// this seam collapsed them into one and had to guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// [`Region::give`] was called twice on this region. It takes its memory
    /// once; a second call is refused rather than aliasing the first.
    AlreadyGiven,
    /// Smaller than the backend's own bookkeeping page. Raise the budget.
    TooSmall,
    /// Too small to hold one segment **at the active geometry**, so the
    /// allocator above could never serve anything. Almost always one missing
    /// flag: without `--cfg ra_small_profile` a segment is 32 MiB.
    Geometry,
    /// Another region is already registered with the backend. There is one
    /// heap per program, and something else claimed it.
    AlreadyRegistered,
    /// The backend refused for a reason this seam does not recognise, which
    /// means it grew a code we have not mapped.
    Refused,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::AlreadyGiven => f.write_str("the heap region was already given"),
            Error::TooSmall => f.write_str("the heap is smaller than the allocator's own page"),
            Error::Geometry => f.write_str(
                "the heap cannot hold one segment at this geometry; set --cfg ra_small_profile",
            ),
            Error::AlreadyRegistered => f.write_str("a heap region is already registered"),
            Error::Refused => f.write_str("the allocator refused the region"),
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
/// The heap region, re-exported from the allocator rather than built here.
///
/// This seam carried its own container until 2.0.4 shipped one. Both of this
/// crate's attempts at it were wrong in ways the allocator's own type is not,
/// and both were found on the board rather than in review:
///
/// 1. A plain `[u8; N]` has alignment 1, so the linker placed it anywhere and
///    a region sized to exactly `good_region_size` served one segment fewer
///    than its size implied -- a startup panic (2026-09-09).
/// 2. Adding `#[repr(align(65536))]` fixed that and cost **60,952 bytes of
///    stack**, because the old sizing rule was `k * SEGMENT_SIZE + FIXED_PAGE`
///    and aligning a size that is not a whole number of segments rounds it up
///    to the next one. `free=0` was verified and the section table was not,
///    which is the exact check this crate's own `.stack` identity exists to
///    force.
///
/// `prim::fixed::Region` is aligned by construction, checks at compile time
/// that `N` is whole segments, asserts `size_of::<Region<N>>() == N` so it
/// cannot be padded, and returns the usable bytes from `give`. There is no
/// version of this worth maintaining separately.
#[cfg(not(any(unix, windows, target_arch = "wasm32")))]
pub use rusty_alloc::prim::fixed::Region;

/// The largest region no bigger than `budget` that the 64 KiB granule strands
/// nothing of, and the smallest region serving at least `usable` bytes.
///
/// Re-exported from 2.0.3, which added them after this consumer measured a
/// 220 KiB region losing 24,576 bytes to the granule -- three times the
/// allocator's whole code cost at the time. A firmware should size its heap
/// with one of these rather than a round number.
#[cfg(not(any(unix, windows, target_arch = "wasm32")))]
pub use rusty_alloc::prim::fixed::{good_region_size, region_for};

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

    /// The sizing arithmetic, checked against the number this consumer
    /// measured on the board before the API existed: a 220 KiB budget must
    /// come back as 200,704 -- three 64 KiB segments plus the 4 KiB page --
    /// and that region must strand nothing.
    #[test]
    #[cfg(not(any(unix, windows, target_arch = "wasm32")))]
    fn a_good_region_strands_nothing() {
        const BUDGET: usize = 220 * 1024;
        assert_eq!(good_region_size(BUDGET), 200_704);
        assert!(good_region_size(BUDGET) <= BUDGET);
        assert_eq!(
            rusty_alloc::prim::fixed::usable_bytes(0, good_region_size(BUDGET)),
            196_608,
            "a good region is all segments and one page"
        );
    }

    #[test]
    fn the_error_messages_say_what_went_wrong() {
        assert_eq!(
            Error::AlreadyGiven.to_string(),
            "the heap region was already given"
        );
        assert!(Error::Refused.to_string().contains("refused"));
        // The geometry refusal has to name the flag. It is the one failure a
        // firmware author cannot diagnose from the symptom -- on 2.0.0 it was
        // a clean build that failed every allocation on the board.
        assert!(
            Error::Geometry.to_string().contains("ra_small_profile"),
            "{}",
            Error::Geometry
        );
        assert!(Error::TooSmall.to_string().contains("smaller"));
        assert!(
            Error::AlreadyRegistered
                .to_string()
                .contains("already registered")
        );
        // All five are distinct, or a caller cannot act on them.
        let all = [
            Error::AlreadyGiven,
            Error::TooSmall,
            Error::Geometry,
            Error::AlreadyRegistered,
            Error::Refused,
        ];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b);
                assert_ne!(a.to_string(), b.to_string());
            }
        }
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
