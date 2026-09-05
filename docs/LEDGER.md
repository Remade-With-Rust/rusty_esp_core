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
