# rusty_esp_core — the ledger

Every number this package claims, with the run that produced it. A row
without a method is not a number.

## C0–C2 on the host (the standing gate, re-run 2026-09-04)

Laptop, Windows 11, stable Rust, `CARGO_TARGET_DIR=C:/janus-d`, this
package's own workspace — core has no sibling, so no `[patch]` is in play.

| gate | result |
|---|---|
| `cargo test --workspace` — the unit tests across the modules the README lists (frames and planes, PCM blocks, `Micros`, `Error`, the manifest's canonical encoding and its parse-and-re-encode round trip, `Chip::parse`, `MediaPacket`, `FrameMut`, `WallOffset`) | **26 pass** |
| `tests/no_panic.rs` — `Manifest::parse`, `Chip::parse`, `WallOffset::from_exchange` / `to_wall` under random inputs from an LCG (the same corpus on every machine) and mutations of valid encodings, all under `catch_unwind` | **3 pass**. One finding on the first run (2026-09-02), fixed the same day: `from_exchange` subtracted in `i64` and overflowed for clock readings near `u64::MAX`, caught at 100 000 triples including `0` and `u64::MAX`; it subtracts in `i128` now |
| `rusty_esp_core-esp` unit | 0 tests — the seam traits only; nothing to run on a host |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo deny check` | advisories, bans, licenses, sources: all ok |
| `cargo check -p rusty_esp_core --no-default-features` and `--features alloc` on `riscv32imac-unknown-none-elf` and `riscv32imafc-unknown-none-elf` | **4 of 4 pass** — and in CI, where core is the one green workflow in the family: it has no private sibling to fetch |
| `xtensa-esp32s3-none-elf`, both rungs, through `janus check --xtensa` | pass in the fleet run of 2026-09-03 (18 Xtensa checks across all ten packages, 289 s warm); local only — the hosted runners have no esp toolchain |

No speed number: nothing in core is a kernel (the kernels the packages
shared moved to `rusty_esp_dsp`, C3), and the one path with a cost worth
measuring — the manifest's canonical encoding under a signature — gets its
number when a chip signs one (the M1 row in the umbrella's
`hardware-verify.md`).

Not run: anything on a chip. `Micros`, `Clock`, `Rng` and `Kv` are traits
here; their first numbers belong to the `-esp` backends that implement them
(`rusty_esp_mid`'s `EspRng` and `EspNvsKv`) on a board.

## The `Rng` seam's real source: a megabyte from an ESP32-S3 (2026-09-06)

The seam has always been fed by `esp_hal`'s generator on a chip and by a
deterministic generator in tests, and nothing had ever judged the chip's
output. `rusty_esp_dsp/firmware/xiao-s3-probe --features rngdump` emits
**1 048 576 bytes** from the hardware generator on a Seeed XIAO ESP32-S3
Sense, as 16 384 hex lines over the USB serial link.

The source matters and is named in the code: `TrngSource::new(peripherals.RNG,
peripherals.ADC1)`. Without that, `RNG` on an ESP32 is a pseudo-random
register and only the ADC-backed path is a true generator; the firmware
takes `Trng::try_new()` and refuses to dump if it is unavailable.

`ent` is not installed on this machine, so its five statistics were
implemented in `ent.py`. **A reimplementation is weaker evidence than the
tool**, so the script runs a **control arm** over the same number of bytes
from the operating system's own generator: if the implementation were
wrong, both columns would be wrong the same way, and the chip's column only
means something beside a known-good one.

| statistic | ideal | **the chip** | control (`os.urandom`) |
|---|---:|---:|---:|
| entropy, bits/byte | 8.000000 | **7.999828** | 7.999837 |
| chi-square, 255 dof | ~255 | **249.7** | 237.6 |
| arithmetic mean | 127.5 | **127.6303** | 127.3892 |
| Monte Carlo pi error | 0 % | **0.0195 %** | 0.3462 % |
| serial correlation | 0 | **+0.001096** | +0.000932 |

**The chip is indistinguishable from the control on every statistic**, and
on two of the five it is nearer the ideal — which is what a fair coin looks
like, not evidence of superiority. Chi-square at 249.7 sits close to the
centre of its distribution; a generator with structure would fail here
first and loudly.

This is a smoke test, not a certification: five statistics over one
megabyte from one part at one temperature. It is enough to say the seam is
fed real entropy on this hardware, and not enough to say anything about the
generator's design.
