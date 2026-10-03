<!-- rusty_esp_core/docs/LEDGER.md -->
## X0 of the killing-C plan: the C census — 2026-10-01

`python tools/c-census.py build && python tools/c-census.py report --ledger` from the umbrella, so sibling crates are the checkouts beside this one: each firmware is linked `--release` with a linker map and `--emit-relocs`, and the two are read together. Every input section the linker kept is charged to the archive the map names for it, one owner per address; every FUNC and OBJECT symbol in the ELF to the archive whose section holds its address; and a mask-ROM routine counts when a kept relocation names it (a linker script defines every ROM symbol whether or not anything calls it). `image B` is code + data as flashed; bss is RAM only. `tools/c-census.py verify` is the gate: the bytes charged equal the bytes the ELF loads, and every symbol charged to a C archive is one `llvm-nm` finds defined in that archive; on an ESP-IDF build the image bytes of every archive also equal what Espressif's own `esp_idf_size` reports from the same map. Two limits: a string table the linker merged is shared by everything that contributed to it, so it is charged where the map puts it (GNU ld) or to the linker row (lld, which names no contributor); and with LTO the Rust side is one object, so its crates are not told apart. Where a firmware reads its network at compile time the build is given placeholders for all of it (`census` / `census-pass`, stream destinations in 192.0.2.0/24): a firmware given no destination compiles its networking out, and the census would measure an image nobody ships.

### `esp32s3-boot` — S3, Track B, `perf/aligned-i16-view@46f2b5a`

| origin | objects | symbols | code B | data B | bss B |
|---|---:|---:|---:|---:|---:|
| Rust | 1 | 124 | 17,694 | 3,748 | 3,288 |
| linker (merged constants, padding, reservations) | 1 | 0 | 841 | 16 | 0 |

**C in this image: 0 symbols, 0 B of 22,299 B (0.0%). The blob floor is 0 symbols in 0 archives.** This is the 2nd-stage bootloader itself: below it there is only the ROM.

Mask-ROM routines called: 36 — 0 from C, 36 from Rust: `Cache_Dbus_MMU_Set`, `Cache_Enable_DCache`, `Cache_Enable_ICache`, `Cache_Ibus_MMU_Set`, `Cache_Invalidate_DCache_All`, `Cache_Invalidate_ICache_All`, `Cache_Resume_DCache`, `Cache_Resume_ICache`, `Cache_Suspend_DCache`, `__udivdi3`, `crc32_le`, `esp_rom_spiflash_config_clk`, `esp_rom_spiflash_erase_sector`, `esp_rom_spiflash_read`, `esp_rom_spiflash_unlock`, `esp_rom_spiflash_write`, `ets_delay_us`, `ets_get_cpu_frequency`, `ets_secure_boot_read_key_digests`, `ets_secure_boot_verify_signature`, `ets_sha_disable`, `ets_sha_enable`, `ets_sha_finish`, `ets_sha_init`, `ets_sha_update`, `ets_update_cpu_frequency`, `memcmp`, `memcpy`, `memset`, `rom_Cache_Suspend_ICache`, `rom_i2c_writeReg`, `rom_i2c_writeReg_Mask`, `software_reset`, `uart_div_modify`, `uart_tx_one_char`, `uart_tx_wait_idle`.

