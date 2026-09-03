# rusty_esp_core — mission plan

**One sentence:** the shared vocabulary of the Janus ESP family — the handful of
`no_std` types every function package speaks at its boundary, so frames, PCM,
timestamps, errors and capability claims flow between crates with no
conversion and no copy.

Family plan: Janus `docs/plans/janus-mission.md` (§3.4 defines this crate's
role). This crate is **Layer 0**; nothing in the family is beneath it.

Written 2026-09-01. Status: **M0 shipped on the host** — the crate exists with
real types and 19 tests, clippy clean, compiling for `riscv32imac` and
`riscv32imafc` with and without `alloc`.

---

## 1. What it is, what it is not

**Is:** types, one error, three seam traits, one canonical encoding. Every one
of them is justified by a package that needs it at its boundary.

**Is not:** a driver, a HAL, an allocator, a codec, a serialization framework,
a product type. If a type needs `esp-hal`, ESP-IDF, `serde`, or a codec crate
to define it, it does not belong here. If two packages want to share a type
and it is not here, the answer is to add it here — never a fourth edge in the
Layer-1 graph.

## 2. The laws this crate encodes

1. **Borrowed frames.** `Frame<'a>` and `PcmBlock<'a>` are views over memory
   the caller owns — a DMA ring, a static arena, a `Vec` on the host.
   Constructors validate size and alignment so consumers index without
   re-checking and without `unsafe`. Nothing here allocates per frame.
2. **Monotonic time as a value.** `Micros(u64)` since boot. No atomics (32-bit
   targets lack 64-bit atomics), no wall clock (the host maps it once per
   session).
3. **One `Copy` error.** `Error` has no `String`, so it crosses `no_std`
   boundaries and package boundaries without conversion chains. Packages wrap
   it in their own enums for detail; the boundary stays `Error`.
4. **Honest capabilities, canonically encoded.** `Manifest::encode` writes the
   same bytes for the same facts regardless of input order, and
   `Declared::validate` enforces the rule the MATA OEM sidecar's catalog
   enforces: a live claim names its backing crate, a planned claim names none.
   `rusty_esp_mid` signs those bytes.
5. **Three seams, traits only.** `Clock`, `Rng`, `Kv` are the only way a core
   crate touches time, entropy or persistent storage. `Rng` must be a hardware
   TRNG on the chip; the host provides an *insecure, so-named* test generator.
   `Kv` keys are ≤ 15 bytes (ESP-IDF NVS) and the seam does not encrypt.
6. **Feature ladder** `std` ⊃ `alloc` ⊃ core-only, copied from `rusty_zstd`.
   CI proves the two bare-metal rungs on every push.

## 3. The surface as built (0.1.0)

| Module | Items |
|---|---|
| `time` | `Micros` (`from_millis`, `from_secs`, `as_millis`, `since` saturating, `add_micros`) |
| `error` | `Error` (`Unsupported`, `BufferTooSmall { needed }`, `InvalidGeometry`, `InvalidFormat`, `Hardware`, `Timeout`, `Busy`, `Crypto`, `Denied`, `Corrupt`), `Result<T>` |
| `frame` | `PixelFormat` (Jpeg, Gray8, **Rgb565**, Rgb888, Bgr888, Rgba8888, Yuyv422, Yuv420p, Raw8), `Geometry` (`new` with parity rules, `byte_len`, `packed_stride`), `Plane<'a>` (`new`, `row`), `Planes<'a>` (`Packed` / `Planar{y,u,v}`), `Frame<'a>` (`packed` — JPEG must start `FF D8`, `yuv420p`, `coded`, `byte_len`) |
| `pcm` | `SampleFormat` (I16, I24In32, I32, F32), `PcmFormat` (`PCM16_16K_MONO`, `PCM16_48K_STEREO`, `frame_bytes`, `bytes_for_micros`, `micros_for_frames`), `PcmBlock<'a>` (`new` frame-aligned, `frames`, `duration_micros`, `end`, `samples_i16`) |
| `capability` | `Capability` (24 tags, `ALL`, `tag`, `parse`), `Status`, `Declared` (`available`/`preview`/`planned`, `validate`), `Chip` (9 parts, `has_wifi`, `has_pie`), `Manifest` (`has`, `validate`, `encode`, `encoded_len`), `MAX_FIELD_LEN` |
| `hal` | `Clock`, `Rng`, `Kv`, `MAX_KEY_LEN`, `check_key`; under `std`: `host::{SystemClock, MemoryKv, InsecureTestRng}` |
| root | `VERSION`, `FORMAT_VERSION = 1`, `prelude` |

The canonical manifest form:

```text
janus/1
model=acme/doorbell-2
fw=1.4.0
chip=esp32s3
cap=image.jpeg:available:rusty_esp_image
cap=iroh.relay:planned:
cap=mid.device:preview:rusty_esp_mid
```

## 4. Roadmap

| Milestone | Adds | Driven by | Kill test |
|---|---|---|---|
| **C0** (done) | everything in §3 | J0 | 19 host tests; `--no-default-features` and `--features alloc` on both riscv32 targets |
| **C1** ✅ 2026-09-02 | `Manifest::parse` → `ParsedManifest` (owned, `alloc`; strict canonical form, `Unsupported` for a newer version or an unknown tag), `Chip::parse` + `Chip::ALL`; `MediaPacket<'a>` + `Codec` promoted from `rusty_esp_video-core` into `media` (video re-exports them); `FrameMut<'a>` / `PlaneMut` / `PlanesMut` for in-place ops with `as_frame()` | J1 | **passed:** 400 generated manifests (every chip, random declaration subsets and order, random fields) encode → parse → encode byte-identical; 2 000 corruptions of a real manifest never panic and anything accepted re-encodes to itself; version / tag / order / duplicate refusals each named |
| **C2** ✅ 2026-09-02 | `time::WallOffset`: the host's one-shot device→wall mapping from a send / device-reading / receive triple, error = half the round trip, `error_at(device, ppm)` for drift, `better()` keeps the smaller error | J3 | **passed:** `WallOffset::UNKNOWN` maps nothing and reports `error_us() == None` (infinite), never zero; a 40 µs round trip places the device reading mid-way with a 20 µs bound |
| **C3** ✅ 2026-09-02 | `dsp` hoisting rule executed (`rusty_esp_dsp` D0: `downscale2x_*` and the pixel conversions from image, the i16 reductions and PCM conversion from audio, `isqrt` from signal; every move byte-identical to the copy it replaced over a corpus before the copy went) — as planned: the first kernel two `-core` crates both carry (likely `dot_i16`, `sad8x8`, a biquad) moves to `rusty_esp_dsp`, scalar oracle first | J5 | byte-identical PIE vs scalar on S3 |
| **C4** ◐ table 2026-09-02 | 1.0.0: format frozen; `FORMAT_VERSION` bump discipline documented (accept-both reader ships one release before any writer change). **Done:** the family `use-protection-please` table (umbrella mission plan, last section): unsafe inventory (twenty documented blocks in three `-esp` files), the no_std rungs CI checks, the no-panic gates, the oracles, `cargo deny` in CI | J6 | `use-protection-please` table complete — **it is; the freeze and the bump discipline wait for J6** |

## 5. Deliberately absent

- A `Packet` type for *network* frames — that is `rusty_esp_iroh` / `rusty_esp_video`.
- Any `serde` derive. The manifest has its own encoding on purpose (no `alloc`,
  deterministic bytes to sign). Packages that want `serde` add it locally.
- A logging facade. Firmware picks `defmt` (Track B) or `log` (Track A).
- An async trait. The seams are sync on purpose; async wrappers live in the
  `-esp` crates where the executor is known.

## 6. Risks

| Risk | Mitigation |
|---|---|
| `#[non_exhaustive]` enums make downstream `match` need a wildcard arm | documented; wire tags are the stable contract, not variant order |
| A package wants a fourth Layer-1 edge instead of a core type | the DAG rule in the family plan; review blocks the edge |
| Format drift once devices are in the field | `FORMAT_VERSION` in the first line; accept-both-on-read law from the house architecture notes |

## 7. Decision log

| Date | Decision |
|---|---|
| 2026-09-01 | `Frame` borrows; `rff_core::Frame` (`Vec<Vec<u8>>` by value) is not reused. RGB565 and YUYV are first-class. |
| 2026-09-01 | Manifest encoding is line-oriented text, not JSON/postcard: signable without `alloc`, readable by a human on a serial console. |
| 2026-09-01 | No allocator, no `serde`, no async in Layer 0. |
| 2026-09-01 | The host `Rng` is named `InsecureTestRng` so it cannot be mistaken for a key source. |
| 2026-09-02 | **`parse` is strict.** The manifest's canonical form is what the signature covers, so the parser accepts exactly that form and re-encodes to the same bytes; a newer format version or an unknown capability tag is `Unsupported`, not a partial read. A host keeps the bytes it received for the signature check. |
| 2026-09-02 | **An unknown wall offset is infinite, not zero.** `WallOffset::UNKNOWN` answers `None` for every mapping and for its error; only a measured exchange produces numbers, and the number carries its bound. |
| 2026-09-02 | `MediaPacket` moved here the day the mesh and the video transport both needed it; `rusty_esp_video_core::packet` re-exports it so no call site changed. |

## The no-panic gate (host, 2026-09-02)

`tests/no_panic.rs`: every parser this crate exposes takes random bytes from
an LCG and mutations of a valid encoding under `catch_unwind` — `Manifest::parse`
and `ParsedManifest::parse` (30 000 inputs), the three tag parsers (30 000
strings), `WallOffset::from_exchange` and `to_wall` (100 000 triples including
`0` and `u64::MAX`). **One finding, fixed:** `from_exchange` subtracted in `i64`
and overflowed for clock readings near `u64::MAX`; it now subtracts in `i128`
and reports an unrepresentable offset as `InvalidFormat`. The same gate runs in
every function package; each ledger has its row.
