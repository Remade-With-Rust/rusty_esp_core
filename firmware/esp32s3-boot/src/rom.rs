//! The ROM routines the loader calls, at the addresses esp-rom-sys's linker
//! scripts give them: only what ESP-IDF's own bootloader uses below itself.
//! Prototypes from `components/esp_rom/esp32s3/include/esp32s3/rom/`.
#![allow(non_snake_case, dead_code)]

/// The ROM's `SHA_CTX` (`rom/sha.h`).
#[repr(C)]
pub struct ShaCtx {
    pub start: bool,
    pub in_hardware: bool,
    pub ty: u32,
    pub state: [u32; 16],
    pub buffer: [u8; 128],
    pub total_bits: [u32; 4],
}

impl ShaCtx {
    pub const fn zero() -> Self {
        Self {
            start: false,
            in_hardware: false,
            ty: 0,
            state: [0; 16],
            buffer: [0; 128],
            total_bits: [0; 4],
        }
    }
}

/// `SHA_TYPE::SHA2_256`.
pub const SHA2_256: u32 = 2;
/// `ETS_OK`.
pub const ETS_OK: u32 = 0;
/// `secure_boot_status_t::SB_SUCCESS` (a full word, against fault injection).
pub const SB_SUCCESS: u32 = 0x3A5A_5AA5;

/// The ROM's `ets_secure_boot_key_digests_t` (`rom/secure_boot.h`).
#[repr(C)]
pub struct KeyDigests {
    pub key_digests: [*const u8; 3],
    pub allow_key_revoke: bool,
}

extern "C" {
    // rom/spi_flash.h -- SPI1, straight to the chip
    pub fn esp_rom_spiflash_read(src_addr: u32, dest: *mut u32, len: i32) -> i32;
    pub fn esp_rom_spiflash_write(dest_addr: u32, src: *const u32, len: i32) -> i32;
    pub fn esp_rom_spiflash_erase_sector(sector: u32) -> i32;
    pub fn esp_rom_spiflash_unlock() -> i32;
    pub fn esp_rom_spiflash_config_clk(freqdiv: u8, spi: u8) -> i32;
    pub fn esp_rom_spiflash_read_status(chip: *mut u32, status: *mut u32) -> i32;
    pub fn esp_rom_spiflash_read_statushigh(chip: *mut u32, status: *mut u32) -> i32;
    pub fn esp_rom_spiflash_config_readmode(mode: u32) -> i32;
    pub fn esp_rom_spiflash_select_qio_pins(wp_gpio_num: u8, spiconfig: u32);
    // rom/efuse.h -- the flash pins as the eFuse names them
    pub fn ets_efuse_get_spiconfig() -> u32;
    pub fn ets_efuse_get_wp_pad() -> u32;

    // rom/ets_sys.h, rom/uart.h -- the console the ROM already set up
    pub fn uart_tx_one_char(c: u8) -> i32;
    pub fn uart_tx_wait_idle(uart: u8);
    pub fn ets_delay_us(us: u32);
    pub fn software_reset();
    /// The ROM idea of the CPU clock, in MHz (ticks per microsecond).
    pub fn ets_get_cpu_frequency() -> u32;
    pub fn ets_update_cpu_frequency(mhz: u32);
    /// The console UART divisor as a 16.4 fixed-point latch value.
    pub fn uart_div_modify(uart_no: u8, div_latch: u32);

    // rom/rtc.h (regi2c_ctrl.h) -- the analog blocks over the ROM I2C master
    pub fn rom_i2c_writeReg(block: u8, host_id: u8, reg_add: u8, data: u8);
    pub fn rom_i2c_writeReg_Mask(block: u8, host_id: u8, reg_add: u8, msb: u8, lsb: u8, data: u8);

    // rom/crc.h
    pub fn crc32_le(crc: u32, buf: *const u8, len: u32) -> u32;

    // rom/sha.h -- the SHA unit through the ROM
    pub fn ets_sha_enable();
    pub fn ets_sha_disable();
    pub fn ets_sha_init(ctx: *mut ShaCtx, ty: u32) -> u32;
    pub fn ets_sha_update(ctx: *mut ShaCtx, input: *const u8, len: u32, update_ctx: bool);
    pub fn ets_sha_finish(ctx: *mut ShaCtx, out: *mut u8) -> u32;

    // rom/cache.h
    // IDF's linker script exports this one under the ROM's own name.
    #[link_name = "rom_Cache_Suspend_ICache"]
    pub fn Cache_Suspend_ICache() -> u32;
    pub fn Cache_Resume_ICache(autoload: u32);
    pub fn Cache_Suspend_DCache() -> u32;
    pub fn Cache_Resume_DCache(autoload: u32);
    pub fn Cache_Enable_ICache(autoload: u32);
    pub fn Cache_Enable_DCache(autoload: u32);
    pub fn Cache_Invalidate_ICache_All();
    pub fn Cache_Invalidate_DCache_All();
    pub fn Cache_Start_DCache_Preload(addr: u32, size: u32, order: u32) -> u32;
    pub fn Cache_DCache_Preload_Done() -> u32;
    pub fn Cache_End_DCache_Preload(autoload: u32);
    pub fn Cache_Ibus_MMU_Set(ext_ram: u32, vaddr: u32, paddr: u32, psize: u32, num: u32, fixed: u32) -> i32;
    pub fn Cache_Dbus_MMU_Set(ext_ram: u32, vaddr: u32, paddr: u32, psize: u32, num: u32, fixed: u32) -> i32;

    // rom/secure_boot.h -- the verifier the ROM runs on the bootloader itself
    pub fn ets_secure_boot_verify_signature(
        sig: *const u32,
        image_digest: *const u8,
        trusted_keys: *const KeyDigests,
        verified_digest: *mut u8,
    ) -> u32;
    pub fn ets_secure_boot_read_key_digests(trusted_keys: *mut KeyDigests) -> u32;
}
