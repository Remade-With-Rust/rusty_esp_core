# rusty_esp_core

[![crates.io](https://img.shields.io/crates/v/rusty_esp_core.svg)](https://crates.io/crates/rusty_esp_core)
[![docs.rs](https://docs.rs/rusty_esp_core/badge.svg)](https://docs.rs/rusty_esp_core)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The shared vocabulary of the **Janus** ESP family: borrowed media frames, PCM
blocks, monotonic timestamps, one `Copy` error, a signed-capability manifest
with an honesty rule, and the three HAL seams (clock, rng, kv) every backend
fills. No drivers, no allocator, no product types. `forbid(unsafe)`.
`no_std` by default; `alloc` and `std` are features.

Janus rebuilds the Espressif ESP32 and Arduino application portfolio in
memory-safe Rust so hardware makers can ship products that plug straight into
the MATA home computer. This crate is Layer 0; `rusty_esp_audio`, `_image`,
`_video`, `_signal`, `_iroh` and `_mid` all speak these types at their
boundaries, so a camera frame flows into an encoder and onto the mesh with no
conversion and no copy.

- This package's plan: [docs/plans/rusty_esp_core.md](docs/plans/rusty_esp_core.md)
- The family plan: Janus `docs/plans/janus-mission.md` (umbrella repo)

**Claims discipline:** this README makes no performance or capability claim
that is not backed by a test in this repository.

## Status

**0.1.0 — types shipped on the host.** 19 tests; clippy clean; compiles for
`riscv32imac-unknown-none-elf` and `riscv32imafc-unknown-none-elf` with
`--no-default-features` and with `--features alloc`. Nothing here has run on
a chip yet; the `-esp` backend crate holds the seam traits only.

**C1 and C2 done (2026-09-02).** `Manifest::parse` reads the canonical
encoding back into an owned `ParsedManifest` (`alloc`) that re-encodes byte
for byte, `Chip::parse` names every part, `MediaPacket` and `Codec` moved
here from the video package, and `FrameMut` is the in-place twin of `Frame`.
`WallOffset` is the host's one-shot device-to-wall mapping with its error
bound; unmeasured, it says so with an infinite error rather than a zero.
26 tests.

## What is in it

| Module | Types |
|---|---|
| `frame` | `Frame<'a>` — a **view** over caller-owned memory (a DMA ring, a static arena, a host `Vec`); `Geometry`, `PixelFormat` (incl. RGB565, YUYV, JPEG), `Plane`, `Planes` |
| `pcm` | `PcmBlock<'a>` — interleaved, frame-aligned by construction; `PcmFormat`, `SampleFormat` |
| `time` | `Micros` — monotonic `u64` since boot, a value not an atomic |
| `error` | `Error` — one `Copy` enum, no `String`, crosses `no_std` boundaries |
| `capability` | `Manifest` — model, firmware, `Chip`, and `Declared` capabilities with a **status and a backing crate**; canonical line-oriented encoding that `rusty_esp_mid` signs |
| `hal` | `Clock`, `Rng`, `Kv` traits; under `std`: `SystemClock`, `MemoryKv`, `InsecureTestRng` |

```rust
use rusty_esp_core::prelude::*;

let geometry = Geometry::new(320, 240, PixelFormat::Rgb565)?;
let frame = Frame::packed(geometry, clock.now(), seq, &dma_buffer[..])?;   // validated view, no copy

let manifest = Manifest {
    model: "acme/doorbell-2", firmware: "1.4.0", chip: Chip::Esp32S3,
    declared: &[
        Declared::available(Capability::ImageJpeg, "rusty_esp_image"),
        Declared::planned(Capability::IrohRelay),
    ],
};
let n = manifest.encode(&mut buf)?;   // deterministic bytes: sign them
```

Why borrowed frames: the FFmpeg-shaped `Vec<Vec<u8>>`-per-frame model is
one-plus-N heap allocations per frame, returned by value — the wrong shape at
every level for 512 KB of SRAM. Sources write into caller buffers; encoders
read views.

## Layout

```text
crates/rusty_esp_core        the types (this crate)
crates/rusty_esp_core-esp    backends for the three seams: `esp-hal` | `esp-idf`
firmware/                    per-chip example projects, excluded from the workspace
docs/plans/                  the mission plan
```

## Build

```sh
cargo test --workspace
cargo check -p rusty_esp_core --no-default-features --target riscv32imac-unknown-none-elf
cargo check -p rusty_esp_core --no-default-features --features alloc --target riscv32imac-unknown-none-elf
```

## License

MIT OR Apache-2.0, at your option.
