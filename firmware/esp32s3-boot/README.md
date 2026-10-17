# esp32s3-boot: `rusty_esp_boot`, the second-stage bootloader in Rust

X8 of the killing-C plan. The ESP32-S3's ROM loads whatever sits at flash
`0x0` into the loader's corner of SRAM and jumps to it; ESP-IDF puts its C
bootloader there, and `espflash` bundles that one into every image it
writes. This crate is the same program in Rust: the ROM below it, nothing
else, and the app above it unchanged.

What it does, in order, and what the C loader it replaces calls each step:

| step | here | ESP-IDF |
|---|---|---|
| the super watchdog feeds itself, the flash-boot watchdog protection the ROM armed comes off, the clock-glitch reset off | `soc::early_init` | `bootloader_super_wdt_auto_feed`, `bootloader_config_wdt`, `bootloader_ana_*_reset_config` |
| the PLL at 480 MHz and the CPU and bus at 80 MHz from it, the flash controllers at the divider the header names (the crystal undivided should the PLL not calibrate) | `soc::clocks_up` | `bootloader_clock_configure` |
| the flash size from the loader's own header into the ROM's chip record | `soc::set_flash_size` | `bootloader_flash_update_size` |
| the partition table at `0x8000` | `table::read` | `bootloader_utility_load_partition_table` |
| `otadata`, the rollback rule, the blank-table first write | `table::select`, `table::write_initial` | `bootloader_utility_get_selected_boot_partition`, `set_actual_ota_seq` |
| the app partition mapped through the data cache | `flash::map` | `bootloader_mmap` |
| chip id, segments, RAM copies, XOR checksum, appended SHA-256 in one pass | `image::load` | `esp_image_verify` / `process_segments` |
| under secure boot v2, the signature sector through the ROM's verifier | `secure::verify` | `esp_secure_boot_verify_signature` |
| the MMU cleared and the app's rodata and text mapped, the header's flash clock divider, the jump | `soc::start` | `set_cache_and_start_app` |

Secure boot v2's trusted keys are the policy (`secure.rs`): the eFuse key
digests when `SECURE_BOOT_EN` is burnt (the sacrificial board's case, not
provable on the bench XIAO), else a digest compiled in through
`JANUS_BOOT_KEY_DIGEST` (64 hex digits, what `espsecure.py
digest_sbv2_public_key` prints for the maker's key: the same check in
software, on a board whose eFuse is untouched), else open. A refused image
is named on the console with the check that failed, and the next candidate
is tried in the C loader's order; with none left the loader says so and
resets, as the C one does.

## Build, convert, flash

```sh
cargo +esp build --release                       # xtensa-esp32s3-none-elf, build-std core
JANUS_BOOT_KEY_DIGEST=<64 hex> cargo +esp build --release   # the software secure-boot policy

# the flash image: esptool's converter, the one ESP-IDF's own bootloader.bin is made with.
# espflash 4.6.0's `save-image` refuses an ELF with no app descriptor in a flash
# segment, and a bootloader has neither.
esptool.py --chip esp32s3 elf2image --flash_mode dio --flash_freq 80m --flash_size 8MB \
    -o boot.bin target/xtensa-esp32s3-none-elf/release/rusty_esp_boot-esp32s3

espflash write-bin --port COM4 0x0 boot.bin      # or `espflash flash --bootloader boot.bin ...`
```

80 MHz since round 2 (core's ledger): the loader runs the flash at the
header's speed and the app inherits it; on the XIAO 80 MHz loads the image
in 89 ms against 170 at 40, with the walk itself at 160 MHz.

The header's flash mode, speed and size matter: the ROM reads them, and the
loader reads the speed as the flash clock divider it runs with and hands
on, and the size as the chip record's. 8 MB for the XIAO ESP32-S3 Sense's
two-slot table.

## Memory

`memory.x` is the C loader's `bootloader.ld` for the S3: code at
`0x403CB800` (the vectors need a 1 KB-aligned base; the C one starts at
`0x403CB700`), data at `0x3FCE2700`, the stack down from `0x3FCE9700`.
Every app linked for the C loader fits under this one unchanged, and a
segment that would land on the loader is refused. The image is about 21 KB
of a 32 KB region.

## What it is not

- Not a flash-mode changer: the ROM configures DIO or QIO from the header,
  and `bootloader_enable_qio_mode` has no counterpart here.
- Not a flash-encryption loader: the cache decrypts transparently once the
  eFuse is burnt, which is why the image is read through the cache and never
  through the raw SPI path, but the burn itself is the sacrificial board's.
- Not a clock configurator beyond what the C loader is: the PLL and the
  80 MHz the app starts on are its, the console, the peripherals and the
  240 MHz the app runs at are the app's.

## The bench

`tools/x7-rollback.ps1 -Bootloader boot.bin -Label rust` runs X7's
sequence (a signed never-valid image pushed over the page, one boot, a hard
reset, the fallback) under this loader; `tools/x8-kill-test.ps1` feeds the
secure-boot build an unsigned image, one with a flipped byte, and one with
the byte flipped and the checksum and hash repaired, then the signed good
image; `tools/x8-stamp.py` timestamps a boot from the reset. The numbers are
in `docs/LEDGER.md` under X8.
