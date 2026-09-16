# rusty_esp_alloc

[![Remade With Rust](https://img.shields.io/badge/Remade%20With-Rust-000?logo=rust&logoColor=fff)](https://github.com/remade-with-rust) [![By Mata Network](https://img.shields.io/badge/by-Mata%20Network-5b2be0)](https://www.mata.network) [![crates.io](https://img.shields.io/crates/v/rusty_esp_alloc.svg)](https://crates.io/crates/rusty_esp_alloc) [![docs.rs](https://docs.rs/rusty_esp_alloc/badge.svg)](https://docs.rs/rusty_esp_alloc) [![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](https://github.com/Remade-With-Rust/rusty_esp_core/blob/main/LICENSE-MIT)

The allocator seam: [`rusty_alloc`](https://crates.io/crates/rusty_alloc) as the global allocator, with the fixed region a microcontroller needs. One crate, one pin, one decision — declared in a deliverable and never in a library.

Adopting it was one crate and one line, and the first build would have failed **on the board without ever failing to compile**: the allocator's segment was 32 MiB by default and the region was 220 KiB, and nothing checked. That became an upstream report, and five releases in one week followed, each measured on the chip before it was believed:

| | first release | fifth |
|---|---|---|
| flash it costs | 16.6 KB | **3.4 KB** |
| stack it takes | 3,092 B | **4 B** |
| region stranded by the granule | 24,576 B | **0** |

The middle row is the one a size table cannot show: memory on the part is fixed, so its sections must sum to a constant, and when they did not the missing bytes were the linker's padding — belonging to no section at all.

## Where the evidence is

This crate is part of [`rusty_esp_core`](https://crates.io/crates/rusty_esp_core). The
hardware results, the method lines and the open defects live in that package's
[README](https://github.com/Remade-With-Rust/rusty_esp_core#readme) and in
[`docs/LEDGER.md`](https://github.com/Remade-With-Rust/rusty_esp_core/blob/main/docs/LEDGER.md), where no number
appears without the run that produced it.

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
| **2 — the surfaces** | [`rusty_esp_arduino`](https://crates.io/crates/rusty_esp_arduino) — the sketch facade · [`espino`](https://crates.io/crates/espino) — the maker's CLI |

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
