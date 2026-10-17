# firmware/

Per-chip example projects for `rusty_esp_core`. Each directory here is a **separate
cargo project**, excluded from the workspace, because every chip needs its own
target triple, linker script and (for Xtensa parts) its own toolchain. n0's
iroh-on-ESP32 work reached the same conclusion: keep the firmware projects out
of the library workspace so architecture-specific patches never leak into it.

Naming: `<board>-<track>-<demo>/`, for example `xiao-s3-sense-idf-mjpeg/`.

| Track | Generate with | Target |
|---|---|---|
| A (`std`, ESP-IDF) | `cargo generate esp-rs/esp-idf-template` | `xtensa-esp32s3-espidf`, `riscv32imac-esp-espidf` |
| B (`no_std`, esp-hal) | `esp-generate --chip esp32c6 <name>` | `riscv32imac-unknown-none-elf`, `xtensa-esp32s3-none-elf` |

Rules:

- Depend on this repo's crates by **path** (`../../crates/rusty_esp_core`) inside a
  firmware example; depend on siblings by git URL as usual.
- Release profile for a chip: `opt-level = "s"` (or `"z"`), `lto = true`,
  `codegen-units = 1`, `panic = "abort"`.
- A firmware example is not a test. The library's tests run on the host.

## Not a demo: the bootloader

`esp32s3-boot/` is `rusty_esp_boot`, the second-stage bootloader for the
ESP32-S3 (X8 of the killing-C plan): the program at flash `0x0` that the
ROM loads and that loads the app. It breaks the `<board>-<track>-<demo>`
naming because it is neither a board demo nor on a track; it is per chip,
sits under every Track B firmware, and is its own project here for the same
reasons the demos are (its own target, linker script and toolchain). Its
README says how it is built, converted and flashed.
