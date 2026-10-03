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

## X0 of the killing-C plan: the C census — 2026-09-30

`python tools/c-census.py build && python tools/c-census.py report --ledger` from the umbrella, so sibling crates are the checkouts beside this one: each firmware is linked `--release` with a linker map and `--emit-relocs`, and the two are read together. Every input section the linker kept is charged to the archive the map names for it, one owner per address; every FUNC and OBJECT symbol in the ELF to the archive whose section holds its address; and a mask-ROM routine counts when a kept relocation names it (a linker script defines every ROM symbol whether or not anything calls it). `image B` is code + data as flashed; bss is RAM only. `tools/c-census.py verify` is the gate: the bytes charged equal the bytes the ELF loads, and every symbol charged to a C archive is one `llvm-nm` finds defined in that archive; on an ESP-IDF build the image bytes of every archive also equal what Espressif's own `esp_idf_size` reports from the same map. Two limits: a string table the linker merged is shared by everything that contributed to it, so it is charged where the map puts it (GNU ld) or to the linker row (lld, which names no contributor); and with LTO the Rust side is one object, so its crates are not told apart. Where a firmware reads its network at compile time the build is given placeholders for all of it (`census` / `census-pass`, stream destinations in 192.0.2.0/24): a firmware given no destination compiles its networking out, and the census would measure an image nobody ships.

**What it says.** No C archive is linked (6 bytes of `crti.o`). The
12 mask-ROM routines are memory copies, 64-bit division and the
boot-time cache and clock setup `esp-hal` calls — a subset of the 13 the
identity firmware (`xiao-s3-keys`) calls with no scheduler at all, so the
kernel adds none.

### `xiao-s3-kairos-tasks` — S3, Track B, `perf/aligned-i16-view@46f2b5a`

| origin | objects | symbols | code B | data B | bss B |
|---|---:|---:|---:|---:|---:|
| Rust | 2 | 342 | 64,602 | 11,967 | 17,195 |
| toolchain C runtime (libc, libgcc) | 1 | 2 | 6 | 0 | 0 |
| linker (merged constants, padding, reservations) | 1 | 0 | 887 | 37 | 319,821 |

**C in this image: 2 symbols, 6 B of 77,499 B (0.0%). The blob floor is 0 symbols in 0 archives.** The 2nd-stage bootloader that starts it is espflash 4.6.0's bundled `esp32s3-bootloader.bin`: 21,072 B of C outside this image.

Mask-ROM routines called: 12 — 0 from C, 12 from Rust: `Cache_Resume_DCache`, `Cache_Suspend_DCache`, `__udivdi3`, `esp_rom_regi2c_read`, `ets_delay_us`, `ets_update_cpu_frequency`, `memcpy`, `memset`, `rom_config_data_cache_mode`, `rom_config_instruction_cache_mode`, `rom_i2c_writeReg`, `rtc_get_reset_reason`.

| C archive | origin | symbols | image B | bss B |
|---|---|---:|---:|---:|
| `crti.o` | toolchain | 2 | 6 | 0 |

## X8 of the killing-C plan: the second-stage bootloader in Rust — rollback and refusals proven on the XIAO under both policies, the app at `main` sooner than under the C loader, the eFuse half still the sacrificial board's (2026-10-01)

The plan's X8 row: the loader of §2.2, signed by `espino-sign`, flashed
through `espflash`; the kill test is X7's sequence under it, an image
with one byte flipped refused under secure boot v2, and a boot-to-`main`
time no worse than the C bootloader's on the same image. D1 was taken as
"a new crate in `rusty_esp_core`": `firmware/esp32s3-boot/`, package
`rusty_esp_boot`, its own cargo project like the demos beside it.

### What it is

The program at flash `0x0` that the S3's ROM loads into the C loader's
corner of SRAM (`memory.x` is ESP-IDF's `bootloader.ld` for the S3: code at
`0x403CB800`, data at `0x3FCE2700`, the stack down from `0x3FCE9700`) and
that loads the app, in the C loader's order: the super watchdog's auto-feed
and the flash-boot watchdog protection the ROM arms, the analog resets, the
flash size from its own header into the ROM's chip record, the PLL at
480 MHz and the CPU and bus at 80 MHz from it (the C loader's
`bootloader_clock_configure`, by the same register writes, with the crystal
undivided as the fallback should the calibration not finish) and the flash
at the divider the header names, the partition table at `0x8000`, `otadata` with the rollback rule (`New` gets one try as
`PendingVerify`; a `PendingVerify` found at boot becomes `Aborted` and the
other slot boots; a blank `otadata` gets `otadata[0]` written for the slot
that boots, which is `set_actual_ota_seq` in C), the app partition mapped
through the data cache and walked once (chip id, every segment, RAM
segments copied in by whole words, the XOR checksum, the appended SHA-256
through the ROM's SHA unit), under secure boot v2 the signature sector
through the ROM's own `ets_secure_boot_verify_signature`, the MMU cleared
and the app's rodata and text mapped, the flash clock divider the header
asks for, the jump. A refused image is named on the console with the check
that failed and the next candidate is tried in the C loader's order; with
none left it says so and resets every two seconds.

Below it there is only the ROM. `xtensa-lx-rt` 0.23 gives the reset and
exception vectors, the `esp32s3` PAC 0.35 the dozen registers it touches,
`esp-rom-sys` 0.1.5 the ROM's addresses. No esp-hal: the loader must not
be configuring what the app configures. The trusted keys are the policy:
the eFuse digests when `SECURE_BOOT_EN` is burnt (written, not provable on
the bench), else a digest compiled in through `JANUS_BOOT_KEY_DIGEST`
(`espsecure.py digest_sbv2_public_key` of the key), else open.

`tools/c-census.py` now carries it as `esp32s3-boot` and `--rust-bootloader
FILE` names it in every S3 firmware's sentence:

| | |
|---|---|
| image | 22,400 B open, 22,432 B with the key digest; 22,299 B censused: Rust 17,694 B code + 3,748 B data, the linker's 857 B, bss 3,288 B |
| C | **0 symbols, 0 B** (`-nostartfiles`: the gcc driver's crti.o was six bytes of `_init`/`_fini` nothing calls) |
| mask-ROM routines | 36, all from Rust: the cache MMU and suspend/resume, SPI flash read/write/erase/unlock/clock, the SHA unit, `ets_secure_boot_verify_signature` and `_read_key_digests`, the analog I2C master for the PLL (`rom_i2c_writeReg`, `_Mask`), `crc32_le`, the console, `software_reset`, `memcpy`/`memset`/`memcmp`, `__udivdi3` |
| converter | `esptool.py elf2image` (what ESP-IDF's own `bootloader.bin` is made with). **espflash 4.6.0's `save-image` refuses an ELF with no app descriptor inside a flash segment, and a bootloader has neither** (`AppDescriptorNotPresent`, and `unreachable!("appdesc segment not found")` behind it) |

### The runs, on the XIAO ESP32-S3 Sense

The image under test: X7's cell rebuilt with one line more, `boot (track
B) did … main_ms=…` (the systimer at the first statement of `main`;
`espino make`'s template and the bench project both carry it), 662,912 B,
5 segments, 64,352 B into RAM; the C bootloader for comparison is X7's
ESP-IDF v5.5.1 build with rollback enabled.

**X7's sequence under the Rust loader** (`tools/x7-rollback.ps1 -Bootloader
boot-open.bin`, four times: `rust`, `rust2` with the sink fix below,
`rust3` with the PLL, and `rust-sb` under the secure-boot policy with every
image signed — the good one, the never-valid one and its manifest made
over the signed bytes — so the push, the one try and the fallback were all
verified signatures, 120 ms each):

| step | seen |
|---|---|
| boot 1 (`otadata` erased) | `ota_0 otadata seq 0 state Undefined`, `otadata[0] was blank: ota_0 written as seq 1 Valid`, `Loaded app from partition at offset 0x20000`; the app: `ota slot=Ota0 state=Valid` |
| the push | `200 committed 0a8930d2…f622e`; the app restarts |
| the boot after it | `ota_1 otadata seq 2 state PendingVerify`, loaded `0x400000`; `PROOF: this image will not mark itself valid` |
| the hard reset | `otadata[1] seq 2 was PendingVerify: Aborted, rolling back`, `ota_0 otadata seq 1 state Valid`, loaded `0x20000` |
| one more reset | `ota_0 otadata seq 3 state Valid`, `0x20000` again: the app had re-pointed `otadata` at the slot it runs from |

**ROLLBACK WORKS** under the Rust loader, all four runs. The first attempt
did not: with `otadata` blank the app said `ota data unreadable`, and its
`PUT /update` erased `ota_0` — the slot it was running from. Two findings,
both fixed: the C loader writes `otadata[0]` on a blank table before it
boots an OTA slot (`set_actual_ota_seq`) and `esp-bootloader-esp-idf` 0.6
relies on it — with both entries blank its `next_partition` is `ota_0`
whatever is running (its guard against the booted slot adds two to a
`Factory` index and lands back on `Ota0`); the loader now writes that entry,
and `rusty_esp_iroh-esp`'s `FlashSlots` now takes the running slot from the
cache MMU's word and refuses to erase it (its ledger has the rest).

**The refusals** (`tools/x8-kill-test.ps1`; the loader built with the bench
key's digest, a throwaway RSA-3072 PEM under `F:/jt-x8`; `ota_1`'s first
sector and `otadata` erased so the fallback is visible too):

| image in `ota_0` | the loader said | app |
|---|---|---|
| the good image, unsigned | `ota_0 at 0x20000 refused: signature NoSignature`; and in the second run, with the previous case’s signature sector still in the slot past the image’s end, `signature Rejected(1966311518)`: another image’s signature is as refused as none | — |
| one byte flipped at `0x1000` (`tools/x8-tamper.py --flip`) | `refused: Checksum { stored: 231, computed: 230 }` (the XOR checksum, before the hash gets its turn) | — |
| the byte flipped, checksum and appended SHA-256 repaired (`--repair`) | `refused: signature Rejected(1966311518)` (`0x7533885E`, the ROM's `SB_FAILED`) | — |
| the good image signed (`espino sign --sign-pem`, 667,648 B) | `Loaded app … signature 151` ms | boots, the C2 cell's DID |

and after each refusal `ota_1 at 0x400000 refused: Magic(255)`, `no
bootable app image (2 candidates tried)`, a reset every two seconds. The
open loader on the same three: the flipped byte refused by the checksum,
the repaired one **boots**, the unsigned one boots — the limit the
signature exists for, and the policy line says which loader is which.
The final round, under the PLL loader with the previous case's signature
sector erased before each case: **7 of 7 as planned**: `signature NoSignature`, `Checksum { stored: 61, computed: 60 }`, `signature Rejected(1966311518)`, the signed image booted (`signature 120` ms); under the open loader the flipped byte refused, the repaired one and the unsigned one booted (`docs/runs/x8-kill-test.txt`). A partition table
overwritten with 4 KB of noise: `partition table at 0x8000: Corrupt { index: 0, magic: 34820 }`, a reset every two seconds,
and the board back on its feet once the table was written again.

**The eFuse half is not done.** Everything above checks the signature
exactly as the ROM does under secure boot, with the trusted digest compiled
in instead of burnt. The ROM verifying the loader itself, and the loader
reading the digests from the eFuse (`ets_secure_boot_read_key_digests`,
written, untested), is the sacrificial S3's run, as D1 says; this
loader has never been flashed on a board with `SECURE_BOOT_EN`.

### Time

Both loaders stamp the same clock (the systimer / cycle counter since
reset) and the app stamps its own `main`:

| | ROM → loader | loader's work | jump | app's `main` |
|---|---|---|---|---|
| C (ESP-IDF v5.5.1, rollback build, INFO logging) | 24 ms | 217 ms | `I (241) boot: Loaded app` | `main_ms=` `4078`, `4079`, and `4509` on a first boot after flashing |
| Rust on the crystal (the first build) | 27 ms | table 6 + load 213 = 220 ms | 247 ms since reset | `main_ms=4408`, `4409` |
| **Rust on the PLL, open** | 28 ms | table 6 + load 153 = 160 ms | **188 ms** since reset | `main_ms=` `3393` three times, `3827`–`3832` on a boot that wrote `otadata` |
| Rust on the PLL, signature | 28 ms | 8 + 153 + 120 | 310–334 ms | `main_ms=5344`, see below |

The loader's own share: **188 against 241 ms**, and the app's `main`
**some 690 ms sooner** under the Rust loader. The first build had the app
330 ms later instead, and that was the clock it handed over: the C loader
moves the CPU to 80 MHz on the PLL (`bootloader_clock_configure`), the
first build stayed on the crystal and only dropped the ROM's divider
(20 → 40 MHz), so the app's own pre-`main` — about 3.2 s under the Rust
loader and 3.8 s under the C one, in esp-hal's start-up before the first
line of `main`, not this row's, and with a first-boot-after-flashing
outlier of up to two seconds more — ran its CPU-bound part at half speed
until `esp_hal::init` set 240 MHz. The PLL port is ESP-IDF's
`clk_ll_bbpll_enable`, `clk_ll_bbpll_set_config(480, 40)` with its
calibration and `rtc_clk_cpu_freq_to_pll_mhz(80)` (the LDO slaves for
80 MHz; the voltage step is 240 MHz's alone), by the same register writes
esp-hal's `enable_pll_clk_impl` makes, 50 lines; so the row's third clause
is **met**, with 53 ms to spare on the loader's own share and the app's
start on the clock the C loader gives it.

What the first build measured on the way: the ROM leaves the CPU at 20 MHz
(crystal / 2) and both flash controllers at half the bus clock, 10 MHz,
and the loader took 870 ms to walk the image whether it read through the
ROM's SPI routine or through the cache; undivided crystal and the flash at
40 MHz brought it to 213, and 80 MHz on the PLL (the flash still at 40) to
153. The console's clock source is the crystal (`uart source 3`), so
neither change costs a baud rate.

One observation stays open. Under the signature policy the app's `main`
reads 5,344 ms, 1.95 s later than under the open loader, every time. It
is not the image (the open loader boots the signed image at 3,393) and it
is not the clock (after the ROM's verifier the loader still reads 80 MHz
on the PLL, `sysclk=0xa8400 cpu_per=0xc`); it is something the ROM's
`ets_secure_boot_verify_signature` leaves behind that esp-hal's pre-`main`
pays for. The C loader under real secure boot calls the same ROM routine;
whether it pays the same is the sacrificial board's to show.

### Also found

- `esp_rom_spiflash_read` fails with `1` past the ROM's idea of the chip
  (4 MB) until the loader writes the header's size into
  `rom_spiflash_legacy_data->chip.chip_size`; `esp-storage` reads that
  record for its capacity, which is why the app's identity read and its
  two-slot table failed under the first build.
- `Cache_Suspend_ICache` is exported by the ROM's linker script as
  `rom_Cache_Suspend_ICache` (IDF wraps it); `#[link_name]` finds it.
- The ROM's secure-boot verdict is a full word (`0x3A5A5AA5` / `0x7533885E`),
  against fault injection; the loader compares the recovered digest too.
- Under `$ErrorActionPreference = Stop`, a PowerShell function named
  `Erase` is never called: `erase` is an alias of `Remove-Item` and aliases
  win. And `-match` is single-line: `^` on a joined capture needs `(?m)`.

Records: `docs/runs/x8-*.txt` (the two rollback runs, the refusals, the
stamped boots, the census), `firmware/esp32s3-boot/README.md`.

## X10 addendum: the loader refused padding segments (2026-10-01)

X10's first board run put the DSP probe into `ota_0` under the Rust
bootloader, and the loader refused it:
`ota_0 at 0x20000 refused: SegmentAddress { index: 3, addr: 0, len: 32700 }`,
then booted the C14 image left in `ota_1`. Segment 3 is a padding block:
the image tool inserts one, load address 0, so that a flash-mapped segment
starts on its 64 KB alignment. ESP-IDF's loader (`should_load` in
`esp_image_format.c`) treats every load address below `0x1000_0000` as a
reserved non-loaded block — 0x0 padding, 0x4 an MD5 block — skips it, and
still folds its bytes into the checksum and the SHA-256. The Rust loader's
`classify` knew RAM, DROM and IROM and refused the rest, so it refused
every image that needed padding; every image X8 tested happened not to.

`image.rs` now has `Kind::Skipped` for those addresses: walked for the
checksum and the hash, never copied or mapped. The open-policy loader is
22,304 B (was 22,400), the secure-boot one 22,384 B (was 22,432).

On the XIAO, with both rebuilt loaders: the probe boots from `ota_0`
(549,232 B, five segments, the padding among them); C14 boots as before;
`tools/x8-kill-test.ps1` against the two new loaders gives **7 of 7
refusals as planned** again (signed-good boots, flipped hash and checksum
refused, unsigned and flipped signature refused under the signature
policy and booted under the open one). Records: `docs/runs/x10-padding-refusal.txt`,
`docs/runs/x8-kill-test-x10-recheck.{txt,json}`.

The run records in `docs/runs/` carried the board's page token in the
page-URL line; it is replaced with `<token>` in every one of them.

## X11 of the killing-C plan: no C of ours in any image — the last six bytes, found in this ledger (2026-10-01)

The answer was already written down. X0's census traced the only C in
every radio-free S3 image to `crti.o`: two 3-byte `_init`/`_fini` stubs the
gcc driver adds to an Xtensa link (`killing-c.md` §1.1). X8 linked the
bootloader with `-nostartfiles` and recorded the result above: "0 symbols,
0 B; the gcc driver's crti.o was six bytes of `_init`/`_fini` nothing
calls". X11 takes that to every image we build. RISC-V images link with
rust-lld, which adds no start files; `c6-lora-p2p` was already at 0.

**Why it is safe.** esp-hal's `text.x` keeps `.init` (`KEEP(*(.init))`), so
the stubs' bytes were in every image, but nothing calls them: no call to
`_init` or `_fini` in the probe's or C14's disassembly, xtensa-lx-rt's start
runs `__pre_init`/`__post_init` and never a C runtime's init array, and the
blob archives do not reference them either.

**What changed.** `-C link-arg=-nostartfiles` in the cargo config of the
eight Xtensa Track B firmwares (`xiao-s3-probe`, `xiao-s3-keys`,
`xiao-s3-kairos-tasks`, `xiao-s3-sense-hal-{pdm,pdm-udp,page,capture,ble-provision}`)
and in espino's generator for every Xtensa project (espino's ledger).

**The census** (`tools/c-census.py build | report | verify --work F:/jt-x11`,
every Track B firmware and the four generated S3 camera cells; `verify`
closes on all 17 — every loaded byte charged once, every C symbol defined by
the archive it is charged to):

| firmware | chip | image B | C B | C symbols | of which blob | ROM routines (from Rust) |
|---|---|---:|---:|---:|---:|---:|
| `esp32s3-boot` (the bootloader) | S3 | 22,215 | **0** | 0 | — | 36 (36) |
| `xiao-s3-keys` | S3 | 205,725 | **0** | 0 | — | 19 (19) |
| `xiao-s3-probe` | S3 | 516,445 | **0** | 0 | — | 32 (32) |
| `xiao-s3-kairos-tasks` | S3 | 77,493 | **0** | 0 | — | 12 (12) |
| `xiao-s3-sense-hal-pdm` | S3 | 157,657 | **0** | 0 | — | 34 (34) |
| `xiao-s3-sense-hal-capture` | S3 | 97,285 | **0** | 0 | — | 26 (26) |
| `c6-lora-p2p` | C6 | 120,482 | **0** | 0 | — | 5 (5) |
| `xiao-s3-sense-hal-pdm-udp` | S3 | 516,861 | 320,568 | 1,710 | 1,710 | 185 (22) |
| `xiao-s3-sense-hal-page` | S3 | 541,429 | 316,889 | 1,699 | 1,699 | 196 (30) |
| `xiao-s3-sense-hal-ble-provision` | S3 | 705,565 | 414,606 | 2,426 | 2,426 | 1,008 (22) |
| `C11` | S3 | 587,925 | 316,841 | 1,699 | 1,699 | 199 (34) |
| `C12` | S3 | 643,765 | 316,801 | 1,699 | 1,699 | 202 (50) |
| `C13` | S3 | 799,393 | 414,650 | 2,426 | 2,426 | 1,025 (41) |
| `C14` | S3 | 695,545 | 316,849 | 1,699 | 1,699 | 207 (43) |
| `c6-ble-provision` | C6 | 446,062 | 265,247 | 1,427 | 1,427 | 89 (9) |
| `c6-mesh-node` | C6 | 550,414 | 420,033 | 2,210 | 2,210 | 309 (10) |
| `c6-s1-link` | C6 | 554,680 | 420,033 | 2,210 | 2,210 | 309 (10) |

Every C symbol left in any image we build is a radio-blob symbol (the "C
symbols" and "of which blob" columns are equal on every row). Seven images
— the bootloader, the five bare-metal S3 firmwares and the LoRa node — link
no C at all; with the Rust bootloader beneath them, a board running one of
them runs no C from flash.

**On the XIAO** (`tools/x11-boot-check.py`: each image into `ota_0` under
the Rust bootloader, serial only), nine of nine did their job: the probe ran
to its end with the same 21 checksums as X10 and every one of its 148
kernels within 2 % of X10's clock; the keys firmware loaded the board's
identity and verified 100 of 100 signatures; Kairos' two tasks passed
(103/103 swaps); the PDM firmware and the capture firmware ran their passes
to `== DONE ==`; the station page, the PCM streamer and the BLE provisioner
reached their banners and their sensor or join lines; C14 ran its camera,
its page and its link task. Records: `docs/runs/x11/`.

**What is left on the board, said plainly.** A firmware with Wi-Fi or
Bluetooth up still carries Espressif's radio blob: 316,801–420,033 B of
closed C (1,427–2,426 symbols). The plan's §1.3 and §6 keep it out of scope
— its source is not published and the hardware it drives is not documented
— and its ledgered way round is the one `c6-lora-p2p` proves: a node whose
radio is an external part has no blob. The mask ROM is silicon; X10 decided
per routine which of its libgcc and memory routines we call.

## The optimization campaign after X11: NVS 6–16× faster, the loader's load 12 % shorter, and a data-loss bug fixed (2026-10-01)

The method, the probe kernels and every figure are in rusty_esp_dsp's
ledger ("fifteen deterministic wins"). What changed here:

**`nvs` (X4's reader and writer)**
- W1: CRC-32 by a 256-entry table; W18: slicing-by-4 (four tables, four
  bytes a step). The bitwise CRC is the oracle test (every byte value, 300
  lengths, a 4 KB stream); espino-nvs's fixtures still read and the
  writer's identity image is still byte for byte espino-nvs's.
- W2: the last namespace resolved is cached; W17: entry headers verified
  since the last write are not CRC-checked again. Both are cleared by every
  method that writes (`put_blob`, `remove`); tests: a cached reader answers
  as a fresh one across writes, removals and new namespaces, and a damaged
  header is `Corrupt` on every get before and after cached gets.
- W14: a blob `get` notes each chunk number's first header on its way to
  the index (up to four), so its chunks need no second scan; a chunk not
  noted falls back to the scan. Flash reads for open + get: 100 → 66.
- Refuted: reading entries eight at a time (fewer calls, more bytes; the
  keys firmware's identity load got 180 µs slower on the real flash).

On the probe's identity-shaped partition: a `get` 2,104.7 → 132.1 µs, a
322-byte blob 2,199.9 → 245.1 µs, open + one get (the boot path) 1,912.8
→ 322.3 µs.

**C1: `put_blob` lost the old value when a write did not fit.** Found by
W14's ground-truth test (random writes, rewrites and removals of 1 B–20 KB
blobs on an eight-page partition, every value read back): `put_blob`
called `remove_in` first and then wrote, so a write that ran out of pages
returned `BufferTooSmall` with the old value already erased and orphan
chunks left behind. On a device that is an adoption record update that
fails and forgets the owner. Now as ESP-IDF does it: the new version is
written under the other chunk version (chunk numbers from 0 or from 128;
the index records which), the old version is erased only after the new
index is down, and a write that fails erases the chunks it wrote. A blob
of more than 127 chunks is refused (`InvalidFormat`), which the versioning
needs. A first write still uses version 0, so the fixture comparison with
espino-nvs holds.

**`esp32s3-boot` (X8's loader)**
- W16: the image walk keeps the checksum in a register, runs one loop per
  destination and uses plain loads on the mapped flash: on C14's 697 KB
  image `load` 161 → 142 ms, identical over four boots of each loader;
  `tools/x8-kill-test.ps1` against both rebuilt loaders: 7 of 7 refusals as
  planned. The open loader is 22,336 B, the signature one 22,400 B.
- Refuted: SHA pieces of 2, 8 or 16 KB (149–150 ms against 142 at 4 KB).
- The 6 ms `table` phase is mostly the banner line going out over USB
  serial, not the table read.

Records: rusty_esp_dsp `docs/runs/w/`.

## Round 2: the loader at 80 MHz flash and a 160 MHz load (2026-10-01)

The loader takes the flash clock from its own header and the app runs on
the clock it hands over, so the header's speed is boot time and every
cache miss. ESP-IDF v5.5.1's S3 loader changes only the divider for 80
MHz; this one already maps the `0xF` nibble to divider 1. And R10:
`soc::cpu_160` before the table read and the image walk (one more LDO
slave, no voltage change), `soc::cpu_80` before the hand-over, which is
the same 80 MHz as before.

| the probe's 840 KB image, XIAO | load ms | reset to app ms |
|---|---:|---:|
| X10's `boot-open.bin` (on the bench), 40 MHz | 193 | 228 |
| W16, 40 MHz | 170 | 205 |
| W16 stamped 80 MHz | 111 | 142 |
| W16 + R10, 80 MHz (`F:/jt-w/boot-r10-160.bin`) | **89** | **120** |

85 checksums equal in every run; steady-state kernels unchanged; cold code
faster. The README's recipe now stamps 80 MHz. QIO is the next doubling
and needs the flash's QE bit, which was not touched. The bench was put back
to X10's loader region afterwards (read back byte for byte). dsp's ledger, "Round 2", has the method, every run and the refuted shapes.

## Round 3: the loader's console, its two waits, and the flash it cannot quad (2026-10-01)

All on the XIAO with the probe image or a minimal stamp app; the boards'
sessions are in dsp's ledger, "Round 3".

| change | measured | before | after |
|---|---|---:|---:|
| **B1** the console quiet by default: two short lines (`v0.1.0 open, dio 80 MHz`; `Loaded app from partition at offset 0x20000 (ota_0 seq 1 Valid); load 83 ms, 108 ms since reset`); the old lines behind feature `verbose` | the probe's `main`, after reset | 140.3 ms | 121.2 ms |
| **B14** no wait for the console to drain before the jump (`soc::start`): the last line empties from UART0's FIFO on its own after the jump, the app keeping the 80 MHz bus the divider was set for | a minimal app's `main`, after reset | 44.1 ms | **34.8 ms** |
| **B17** the image walk pipelined: the data cache preloads the next 4 KB piece (`Cache_Start_DCache_Preload`) while the ROM's SHA and the checksum take the current one | the loader's `load`, a 951 KB image | 101 ms | **77 ms** |

**The loader as shipped** (`boot-r3-final`: B1, B14, B17), booted against
round 3's first quiet loader (B1 only) on the same images, two boots each:
the probe image's `load` 110 -> **84 ms** and its `main` 148.3 -> **114.5 ms**;
a minimal app's `main` 44.1 -> **33.1 ms**. On the bench only for those
boots: the board was restored from the session's backup and read back byte
for byte. Installing it on a board is the owner's call.

**How B14 was found.** Feature `jump-stamp` (diagnostics only) packs three
systimer readings, 64 us units, into RTC_CNTL STORE0 for the app to print:
before the last line 34.18 ms, after it 34.50 ms, the jump 43.90 ms --
the line costs 0.32 ms to write and 9.4 ms to drain (about 108 characters
at 115200). Three boots each build, identical to the microsecond; without
the drain the jump is at 34.62 ms and the whole line still arrives.

**Refuted**: the image's SHA-256 through the unit's registers instead of the
ROM's `ets_sha_update`: the load 83 -> 91 ms.

**Quad I/O, not done.** Feature `qio` ports ESP-IDF's
`bootloader_enable_qio_mode` without its write: the ROM's read mode goes to
QIO only when the chip's Quad Enable bit is already set. On the bench XIAO
it is not -- a GigaDevice GD25Q64 (`c8 4017`), status 0x200000, QE (bit 9)
clear -- so the feature leaves the loader in DIO. Setting QE is a
non-volatile status-register write on the board, the owner's call; with it
the loader's reads and the app's cache fills would be twice as wide.
(Feature `flash-diag` prints the id and the ROM's two status words.)

**A correction to X8.** The cell's `main_ms` was read with esp-hal's
`Instant::now()` before `esp_hal::init`, which counts the raw 16 MHz
systimer: every `main_ms` in X8's table is 16 times too large. Divided by
16, the Rust loader's app reaches `main` at about 212 ms (3393) against the
C loader's 255 ms (4078): **43 ms sooner, not 690**, and there is no
"3.2 s pre-`main`" -- the app's own start-up before `main` is about 24 ms.
The loaders' own figures (188 against 241 ms) were the loader's systimer
and stand. The template now reads SYSTIMER unit 0 and divides by 16.

## Round 3 addendum: Quad I/O on the bench, and the loader installed (2026-10-02)

The owner's go. `tools/r3h-loader.py`: ten quiet minutes; the whole 8 MB
read twice, equal (`391ece05902ad192`, `F:/jt-w/r3h-backup-8m.bin`); the
GD25Q64's status 0x200000 -> **0x200200** (`write_flash_status
--non-volatile --bytes 2 0x200`: Quad Enable set, SR1 still 0, SR3
untouched). The identity partition was never written.

| the probe image, two boots each | DIO loader | QIO loader |
|---|---:|---:|
| the loader's `load` | 79 ms | **52 ms** |
| the probe's `main` | 108.3 / 113.3 ms | **81.1 / 81.1 ms** |

Under QIO the probe's full run against round 3's last: 94 checksums equal,
work equal in 243 kernels; hot loops unchanged, code fetched cold from flash
faster (`mid_load_cached` 1.32 -> 1.06 ms, a signature 25.4 -> 24.1 ms).

`qio` is now a default feature (it switches only when QE is set; the chip
stays in DIO otherwise), and the default build is byte for byte the image
installed: the bench XIAO boots `boot-r3-final-qio` (B1, B14, B17, QIO)
under its own firmware, which comes up (`factory`, load 6 ms, 31 ms since
reset); the first 4 MB were put back from the backup and read back.

Not done: C14. Its first boot writes the identity partition, and that write
was refused by the session's permission check; it waits for the owner.

## Round 3 addendum: C14 installed on the bench (2026-10-02)

With the owner's explicit permission (`tools/r3h-c14.py`): ten quiet minutes;
the whole 8 MB read twice, equal (`05433e27c71ddf14`,
`F:/jt-w/r3h-c14-backup-8m.bin`; the first attempt's read failed with
espflash's `corrupt_data` before anything was written, and the script now
reads again); the identity partition held `mid.devkey`, `mid.owner` and
`mid.adopt`, so the board's key was there to load and not mint. C14 (built
by `tools/r3h-build-c14.py` with the hosted network's passphrase from the
gitignored file, the bridge of X9) at ota_0 with its table, otadata erased,
under the round-3 loader with QIO. Two boots, identical:

```
rusty_esp_boot: v0.1.0 open, qio 80 MHz
rusty_esp_boot: Loaded app from partition at offset 0x20000 (ota_0 seq 1 Valid); load 41 ms, 66 ms since reset
janus/cam-page-b: boot (track B) did did:mata:29qcqKb5kMT529GSNgfcUU2gSf4bpd7EWUDEj2Mq7cb9J main_ms=70
janus/cam-page-b: sensor OV3660 configured=Ok(()) fps_cap=15 quality_scale=12
janus/cam-page-b: network hosting janus-cam (wpa2, up to 4 stations)
janus/cam-page-b: link: firmware 0.1.0, bridge 192.168.71.50:7700, 5 declared
```

The DID is the board's on both boots; the cell reaches `main` 70 ms after
reset (X8's C14-era figure, corrected, was about 212). The page token is
redacted in the records. The bench XIAO now runs C14 hosting `janus-cam`;
the laptop did not join it.
