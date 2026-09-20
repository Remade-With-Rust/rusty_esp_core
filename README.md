### In The Wild with 62 Active Installs

FREE RAG Converter Online -- <a href="https://RAGconverter.com">RAGconverter.com</a>

# rusty_esp_core

[![Remade With Rust](https://img.shields.io/badge/Remade%20With-Rust-000?logo=rust&logoColor=fff)](https://github.com/remade-with-rust) [![By Mata Network](https://img.shields.io/badge/by-Mata%20Network-5b2be0)](https://www.mata.network) [![crates.io](https://img.shields.io/crates/v/rusty_esp_core.svg)](https://crates.io/crates/rusty_esp_core) [![docs.rs](https://docs.rs/rusty_esp_core/badge.svg)](https://docs.rs/rusty_esp_core) [![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](https://github.com/Remade-With-Rust/rusty_esp_core/blob/main/LICENSE-MIT)

The shared vocabulary of the **Janus** ESP32 family: borrowed media frames, PCM
blocks, monotonic timestamps, one `Copy` error, a signed capability manifest
with an honesty rule, and the three hardware seams — clock, entropy, key-value
store — that every backend fills. No drivers, no allocator, no product types,
no C, no FFI. `forbid(unsafe)`. `no_std` by default; `alloc` and `std` are
features.

* **Frames are views, not owners.** `Frame<'a>` is a validated window onto
  memory the caller already has — a DMA ring, a static arena, a host `Vec`. The
  FFmpeg-shaped `Vec<Vec<u8>>`-per-frame model costs one-plus-N heap
  allocations per frame on a part with 512 KB of SRAM. Sources write into
  caller buffers; encoders read views; a camera frame reaches the network with
  no conversion and no copy.
* **One error, and it crosses every boundary.** A single `Copy` enum with no
  `String` in it, so a `no_std` driver and a `std` service return the same type.
* **Capabilities that can be refused.** A `Manifest` names a model, a firmware,
  a chip and what the device declares it can do — each with a status and the
  crate that backs it — in a canonical line encoding that
  [`rusty_esp_mid`](https://crates.io/crates/rusty_esp_mid) signs. A device
  that claims a capability it does not have is making a signed, attributable
  statement.
* **Seams, not implementations.** `Clock`, `Rng` and `Kv` are traits here. The
  backends live in `rusty_esp_core-esp` and in the function packages, so this
  crate compiles for a microcontroller and for a laptop test with the same
  source.

## What has run on hardware

This crate is Layer 0, so it does not run alone — it runs inside every profile
the family has verified on silicon. Five of seven have, on two boards. What
was measured *here*, directly:

| what | measured |
|---|---|
| the entropy seam's real source | **1,048,576 bytes** from an ESP32-S3's hardware generator, judged against five standard statistics **with the operating system's own generator as a control arm** — entropy 7.999828 bits/byte against the control's 7.999837, chi-square 249.7 against 237.6, and nearer the ideal than the control on two of the five |
| the manifest on a chip | signed and carried by every verified profile; a device's identity has survived six whole-image reflashes and three flashes from another session |
| the types under `no_std` | `riscv32imac`, `riscv32imafc` and `xtensa-esp32s3-none-elf`, with and without `alloc` |

The entropy result is a **smoke test, not a certification**: five statistics,
one megabyte, one part, one temperature. The control arm is the point — a
wrong implementation would have been wrong in both columns.

Every number, with the run that produced it:
[`docs/LEDGER.md`](https://github.com/Remade-With-Rust/rusty_esp_core/blob/main/docs/LEDGER.md).
This README makes no claim that is not backed by a test in the repository.

## Using it

```rust
use rusty_esp_core::prelude::*;

// A validated view over memory the caller already owns. No copy, no alloc.
let geometry = Geometry::new(320, 240, PixelFormat::Rgb565)?;
let frame = Frame::packed(geometry, clock.now(), seq, &dma_buffer[..])?;

// What this device says it can do -- and what it admits it cannot.
let manifest = Manifest {
    model: "acme/doorbell-2",
    firmware: "1.4.0",
    chip: Chip::Esp32S3,
    declared: &[
        Declared::available(Capability::ImageJpeg, "rusty_esp_image"),
        Declared::planned(Capability::IrohRelay),
    ],
};
let n = manifest.encode(&mut buf)?;   // deterministic bytes: sign them
```

| module | what it holds |
|---|---|
| `frame` | `Frame<'a>` and `FrameMut<'a>`, `Geometry`, `PixelFormat` (RGB565, YUYV, JPEG and more), `Plane`, `Planes` |
| `pcm` | `PcmBlock<'a>` — interleaved and frame-aligned by construction |
| `time` | `Micros` — monotonic microseconds since boot, a value rather than an atomic |
| `error` | `Error` — one `Copy` enum that crosses `no_std` boundaries |
| `capability` | `Manifest`, `Chip`, `Declared`, and the canonical encoding a signature covers |
| `hal` | the `Clock`, `Rng` and `Kv` traits; under `std`, working host implementations for tests |

## Two tracks, one vocabulary

Everything in the family is built for both, and this crate is what makes that
possible: the same types compile for a supervised chip and a bare one.

| track | what it is | this crate |
|---|---|---|
| **A** | `std` on ESP-IDF — Wi-Fi, sockets, threads, the mesh | `--features std` |
| **B** | `no_std` on `esp-hal` — no operating system, no heap unless you ask | default, or `--features alloc` |

```sh
cargo test --workspace
cargo check -p rusty_esp_core --no-default-features --target riscv32imac-unknown-none-elf
cargo check -p rusty_esp_core --no-default-features --features alloc --target riscv32imac-unknown-none-elf
```

## Part of Janus

**Janus** rebuilds the Espressif ESP32 and Arduino application portfolio as
independent, memory-safe Rust packages — so a hardware maker can ship a device
that the [MATA](https://www.mata.network) home computer discovers, catalogs honestly, adopts
under its own identity, and pays for. Ten packages, three layers, and the
dependency direction never reverses.

| layer | packages |
|---|---|
| **0 — the vocabulary** | [`rusty_esp_core`](https://crates.io/crates/rusty_esp_core) · [`rusty_esp_dsp`](https://crates.io/crates/rusty_esp_dsp) |
| **1 — the functions** | [`rusty_esp_image`](https://crates.io/crates/rusty_esp_image) · [`rusty_esp_video`](https://crates.io/crates/rusty_esp_video) · [`rusty_esp_audio`](https://crates.io/crates/rusty_esp_audio) · [`rusty_esp_signal`](https://crates.io/crates/rusty_esp_signal) · [`rusty_esp_mid`](https://crates.io/crates/rusty_esp_mid) · [`rusty_esp_iroh`](https://crates.io/crates/rusty_esp_iroh) |
| **2 — the surfaces** | [`rusty_esp_arduino`](https://crates.io/crates/rusty_esp_arduino) — the sketch facade · `espino` — the maker's CLI (not published) |

Every package is host-verified against an external oracle and keeps a ledger
in which no number appears without the run that produced it. **Five of seven
device profiles have now run their kill tests on real silicon**, three of them
over a Wi-Fi network the board hosts itself.

Also check out the rest of [Remade With Rust](https://github.com/remade-with-rust) — including
[`rusty_alloc`](https://crates.io/crates/rusty_alloc), the pure-Rust rebuild of
mimalloc that these firmwares run on, and
[`rusty_jpeg`](https://crates.io/crates/rusty_jpeg), the JPEG engine behind the
camera path — and our sister project
[remade_ffmpeg_rs](https://github.com/Remade-With-Rust/remade_ffmpeg_rs), a ground-up Rust rebuild of FFmpeg.

## About Mata Network

[Mata Network](https://www.mata.network) builds sovereign, self-hostable infrastructure.
**Remade With Rust** is our open-source home for the permissively-licensed
building blocks that work depends on.

## License

MIT OR Apache-2.0, at your option. See [LICENSE-MIT](https://github.com/Remade-With-Rust/rusty_esp_core/blob/main/LICENSE-MIT)
and [LICENSE-APACHE](https://github.com/Remade-With-Rust/rusty_esp_core/blob/main/LICENSE-APACHE).
