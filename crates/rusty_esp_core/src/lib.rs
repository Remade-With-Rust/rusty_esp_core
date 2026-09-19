#![cfg_attr(not(feature = "std"), no_std)]
// `deny`, not `forbid`, for exactly ONE exception: `pcm::as_i16`/`as_i16_mut`,
// six lines that view an aligned `&[u8]` as `&[i16]`. Everything else in this
// crate still rejects `unsafe` at compile time, and the two exceptions carry
// `#[allow(unsafe_code)]` on the function, so `git grep unsafe_code` finds
// every one of them.
//
// Why it is here at all: on a 32-bit core, `i16::from_le_bytes([b[0], b[1]])`
// over a `&[u8]` cannot use a halfword load, because the load needs 2-byte
// alignment the compiler cannot prove a byte slice has. Measured on an
// ESP32-S3, that costs the audio elements 42-59% -- `StereoToMono` spent 56
// of its 72 loop instructions marshalling bytes into i16s and back.
//
// Why not `bytemuck`, which does exactly this and needs no `unsafe` here:
// this crate is the base of nine repos and has ZERO dependencies, which is
// worth more than six auditable lines. Swapping to `bytemuck::try_cast_slice`
// is a three-line change if that trade is ever re-made.
#![deny(unsafe_code)]
//! `rusty_esp_core` — the shared vocabulary of the Janus ESP family.
//!
//! Every Janus package (`rusty_esp_audio`, `rusty_esp_image`, `rusty_esp_video`,
//! `rusty_esp_signal`, `rusty_esp_iroh`, `rusty_esp_mid`) speaks these types at
//! its boundary, so a camera frame from one crate flows into an encoder in
//! another and onto the mesh in a third with no conversion and no copy.
//!
//! What lives here, and the rule that keeps it small:
//!
//! | Module | Holds | Rule |
//! |---|---|---|
//! | [`time`] | [`Micros`], a monotonic device timestamp; [`WallOffset`], the host's device→wall mapping with its error bound | value type, no clock |
//! | [`error`] | [`Error`], one `Copy` error for the family | no `String`, no `alloc` |
//! | [`frame`] | [`Frame`], a **borrowed** image/video frame | planes over caller memory |
//! | [`pcm`] | [`PcmBlock`], a **borrowed** interleaved PCM block | same |
//! | [`capability`] | [`Manifest`], what a device can do, canonically encoded; [`capability::ParsedManifest`] reads it back (`alloc`) | signed by `rusty_esp_mid` |
//! | [`media`] | [`MediaPacket`], a **borrowed** coded packet and its [`Codec`] | the shape every transport frames |
//! | [`hal`] | [`Clock`], [`Rng`], [`Kv`] — the three seams every backend fills | traits only |
//!
//! Nothing here is a driver, an allocator, a codec, or a product type. If a
//! type needs `esp-hal`, ESP-IDF, or a codec crate to define it, it does not
//! belong in this crate.
//!
//! **Why borrowed frames.** The FFmpeg-shaped `Vec<Vec<u8>>`-per-frame model
//! is one-plus-N heap allocations per frame, returned by value. On a chip with
//! 512 KB of SRAM that is the wrong shape at every level: allocation count,
//! ownership, and the trait signatures it forces. Janus frames are views over
//! memory the caller (a DMA ring, a static arena, a `Vec` on the host) already
//! owns. Sources write into caller buffers; encoders read views.
//!
//! Feature ladder: `std` ⊃ `alloc` ⊃ core-only. The crate compiles for
//! `riscv32imac-unknown-none-elf` with `--no-default-features`.

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod capability;
pub mod error;
pub mod frame;
pub mod hal;
pub mod media;
pub mod pcm;
pub mod prelude;
pub mod time;

pub use capability::{Capability, Chip, Declared, Manifest, Status};
pub use error::Error;
pub use frame::{Frame, FrameMut, Geometry, PixelFormat, Plane, PlaneMut, Planes, PlanesMut};
pub use hal::{Clock, Kv, Rng};
pub use media::{Codec, MediaPacket};
pub use pcm::{PcmBlock, PcmFormat, SampleFormat};
pub use time::{Micros, WallOffset};

/// Crate version, for capability manifests and logs.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The wire/format version of every canonical encoding this crate defines.
/// Bump only with an accept-both reader already shipped (see the Janus plan,
/// "changing a format others already read").
pub const FORMAT_VERSION: u8 = 1;
