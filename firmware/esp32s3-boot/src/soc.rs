//! Every SoC register and ROM record the loader touches, so this file is
//! the audit: the super watchdog's auto-feed, the flash-boot watchdog
//! protection the ROM arms for the second stage, the analog resets the C
//! loader configures, the clocks of the SHA and RSA units, the caches and
//! their buses, the PLL and the 80 MHz CPU clock the C loader hands over,
//! the flash clock the header names, the ROM's flash-chip record (the size
//! the app's flash driver trusts), the systimer for the timing figures,
//! and the cache MMU for the jump.

use esp32s3::{EXTMEM, I2C_ANA_MST, RTC_CNTL, SYSTEM, SYSTIMER, TIMG0, UART0};

use crate::{flash, image::Map, rom};

const SWD_WKEY: u32 = 0x8F1D_312A;
const WDT_WKEY: u32 = 0x50D8_3AA1;
const FIB_GLITCH_RST: u8 = 1 << 0;
const FIB_SUPER_WDT_RST: u8 = 1 << 2;

const MMU_TABLE: *mut u32 = 0x600C_5000 as *mut u32;
const MMU_ENTRIES: usize = 512;
const MMU_INVALID: u32 = 1 << 14;
const MMU_PAGE: u32 = 0x1_0000;
/// The last DROM entry, which the C loader points at the app's first
/// rodata page "for the app to find the boot partition".
const DROM_END_ENTRY_VADDR: u32 = 0x3DFF_0000;

/// The analog I2C block and host of the PLL (`regi2c_bbpll.h`).
const I2C_BBPLL: u8 = 0x66;
const I2C_BBPLL_HOSTID: u8 = 1;
/// All six LDO slaves on; the C loader opens `7 >> (cpu MHz / 80)` of them.
const DEFAULT_LDO_SLAVE: u8 = 0x7;

/// What `bootloader_init` does before anything reads flash, minus the
/// clock change (`clocks_up` is this loader's) and the console (the ROM's
/// serves).
pub fn early_init() {
    // SAFETY: register access from the one core that runs, nothing else
    // alive to race it.
    let rtc = unsafe { &*RTC_CNTL::ptr() };
    // The super watchdog feeds itself and resets through the digital path
    // (`bootloader_super_wdt_auto_feed`, `bootloader_ana_super_wdt_reset_config(true)`).
    rtc.swd_wprotect().write(|w| unsafe { w.swd_wkey().bits(SWD_WKEY) });
    rtc.swd_conf().modify(|_, w| w.swd_auto_feed_en().set_bit().swd_bypass_rst().clear_bit());
    rtc.swd_wprotect().write(|w| unsafe { w.swd_wkey().bits(0) });
    rtc.fib_sel().modify(|r, w| unsafe { w.fib_sel().bits(r.fib_sel().bits() & !(FIB_SUPER_WDT_RST | FIB_GLITCH_RST)) });
    // The clock-glitch reset off (`bootloader_ana_clock_glitch_reset_config(false)`).
    rtc.ana_conf().modify(|_, w| w.glitch_rst_en().clear_bit());
    // The flash-boot protection the ROM armed on the RTC watchdog and on
    // MWDT0, off, in that order (`bootloader_config_wdt`).
    rtc.wdtwprotect().write(|w| unsafe { w.wdt_wkey().bits(WDT_WKEY) });
    rtc.wdtconfig0().modify(|_, w| w.wdt_flashboot_mod_en().clear_bit());
    rtc.wdtwprotect().write(|w| unsafe { w.wdt_wkey().bits(0) });
    let timg0 = unsafe { &*TIMG0::ptr() };
    timg0.wdtwprotect().write(|w| unsafe { w.wdt_wkey().bits(WDT_WKEY) });
    timg0.wdtconfig0().modify(|_, w| w.wdt_flashboot_mod_en().clear_bit());
    timg0.wdtwprotect().write(|w| unsafe { w.wdt_wkey().bits(0) });
    // The SHA and RSA units clocked, out of reset, their memory powered.
    let sys = unsafe { &*SYSTEM::ptr() };
    sys.perip_clk_en1().modify(|_, w| w.crypto_sha_clk_en().set_bit().crypto_rsa_clk_en().set_bit());
    sys.perip_rst_en1().modify(|_, w| w.crypto_sha_rst().clear_bit().crypto_rsa_rst().clear_bit());
    sys.rsa_pd_ctrl().modify(|_, w| w.rsa_mem_pd().clear_bit());
    // Both caches enabled and their buses open for both cores, as
    // `cache_hal_init` leaves them; nothing is mapped yet.
    unsafe {
        rom::Cache_Enable_ICache(0);
        rom::Cache_Enable_DCache(0);
        let ext = &*EXTMEM::ptr();
        ext.dcache_ctrl1().modify(|_, w| w.dcache_shut_core0_bus().clear_bit().dcache_shut_core1_bus().clear_bit());
        ext.icache_ctrl1().modify(|_, w| w.icache_shut_core0_bus().clear_bit().icache_shut_core1_bus().clear_bit());
    }
}

/// What the clocks were and are: the CPU in MHz before and after, whether
/// the PLL took, and the UART's clock source (1 the bus, 3 the crystal).
#[derive(Clone, Copy)]
#[cfg_attr(not(feature = "verbose"), allow(dead_code))]
pub struct Clocks {
    pub cpu_before: u32,
    pub cpu_after: u32,
    pub pll: bool,
    pub uart_source: u8,
}

/// `bootloader_clock_configure`: the ROM leaves the CPU on the crystal
/// through a divider (20 MHz) and the flash controllers at half the bus
/// clock (10 MHz). The C loader brings the PLL up at 480 MHz and runs the
/// CPU and the bus at 80 MHz from it; so does this, by the same register
/// writes (`rtc_clk_bbpll_enable`, `rtc_clk_bbpll_configure`,
/// `rtc_clk_cpu_freq_to_pll_mhz(80)`), and the app's own pre-`main` then
/// runs at the clock it would have had under the C loader. Should the
/// PLL's calibration not finish, the crystal stays, undivided (40 MHz).
/// Either way the flash controllers get the divider the header names,
/// `freq_div`, against the bus clock. A UART clocked from the bus keeps
/// its baud rate by scaling its divisor.
pub fn clocks_up(freq_div: u8) -> Clocks {
    // SAFETY: as in `early_init`; ROM routines with no preconditions.
    unsafe {
        let sys = &*SYSTEM::ptr();
        let uart = &*UART0::ptr();
        let cpu_before = rom::ets_get_cpu_frequency();
        let uart_source = uart.clk_conf().read().sclk_sel().bits();
        let conf = sys.sysclk_conf().read();
        let mut cpu_after = cpu_before;
        let mut pll = false;
        if conf.soc_clk_sel().bits() == 0 {
            rom::uart_tx_wait_idle(0);
            if pll_80() {
                pll = true;
                cpu_after = 80;
            } else if conf.pre_div_cnt().bits() != 0 {
                let divider = conf.pre_div_cnt().bits() as u32 + 1;
                sys.sysclk_conf().modify(|_, w| w.pre_div_cnt().bits(0));
                cpu_after = cpu_before * divider;
                rom::ets_update_cpu_frequency(cpu_after);
            }
            if uart_source == 1 && cpu_after != cpu_before && cpu_before != 0 {
                let reg = uart.clkdiv().read().bits();
                let latch = ((reg & 0xFFF) << 4) | ((reg >> 20) & 0xF);
                rom::uart_div_modify(0, latch * cpu_after / cpu_before);
            }
        }
        // The bus is 80 MHz on the PLL; the header's divider against it.
        let div = if pll { freq_div } else { 1 };
        rom::esp_rom_spiflash_config_clk(div, 0);
        rom::esp_rom_spiflash_config_clk(div, 1);
        Clocks { cpu_before, cpu_after, pll, uart_source }
    }
}

unsafe fn bbpll_write(reg: u8, data: u8) {
    rom::rom_i2c_writeReg(I2C_BBPLL, I2C_BBPLL_HOSTID, reg, data);
}

unsafe fn bbpll_write_field(reg: u8, msb: u8, lsb: u8, data: u8) {
    rom::rom_i2c_writeReg_Mask(I2C_BBPLL, I2C_BBPLL_HOSTID, reg, msb, lsb, data);
}

/// The PLL at 480 MHz from the 40 MHz crystal and the CPU at 80 MHz from
/// it: ESP-IDF's `clk_ll_bbpll_enable`, `clk_ll_bbpll_set_config(480, 40)`
/// with its calibration, and `rtc_clk_cpu_freq_to_pll_mhz(80)` (the LDO
/// slaves for 80 MHz; the voltage change is 240 MHz's alone). `false`, with
/// nothing switched, when the calibration does not finish in 50 ms.
unsafe fn pll_80() -> bool {
    let rtc = &*RTC_CNTL::ptr();
    let sys = &*SYSTEM::ptr();
    let ana = &*I2C_ANA_MST::ptr();
    // power: the PLL and its I2C path
    ana.ana_config().modify(|_, w| w.bbpll_pd().clear_bit());
    rtc.options0().modify(|_, w| {
        w.bb_i2c_force_pd().clear_bit();
        w.bbpll_force_pd().clear_bit();
        w.bbpll_i2c_force_pd().clear_bit()
    });
    // digital: 480 MHz
    sys.cpu_per_conf().modify(|_, w| w.pll_freq_sel().set_bit());
    // analog: calibration start, the 480 MHz / 40 MHz constants, wait, stop
    ana.ana_conf0().modify(|_, w| w.bbpll_stop_force_high().clear_bit().bbpll_stop_force_low().set_bit());
    bbpll_write(4, 0x6B); // MODE_HF set (REG4)
    bbpll_write(2, 5 << 4); // OC_REF: dchgp 5, div_ref 0
    bbpll_write(3, 8); // OC_DIV_7_0: 8 (x12 with the 4 the hardware adds)
    bbpll_write_field(5, 2, 0, 0); // OC_DR1
    bbpll_write_field(5, 6, 4, 0); // OC_DR3
    bbpll_write(6, (1 << 6) | (3 << 4) | 3); // DLREF_SEL 1, DHREF_SEL 3, OC_DCUR 3
    bbpll_write_field(9, 1, 0, 3); // OC_VCO_DBIAS
    let deadline = now_us() + 50_000;
    while ana.ana_conf0().read().bbpll_cal_done().bit_is_clear() {
        if now_us() > deadline {
            ana.ana_conf0().modify(|_, w| w.bbpll_stop_force_low().clear_bit().bbpll_stop_force_high().set_bit());
            return false;
        }
    }
    rom::ets_delay_us(10);
    ana.ana_conf0().modify(|_, w| w.bbpll_stop_force_low().clear_bit().bbpll_stop_force_high().set_bit());
    // the switch: 80 MHz from the PLL, undivided, four LDO slaves
    rtc.date().modify(|_, w| w.ldo_slave().bits(DEFAULT_LDO_SLAVE >> 1));
    sys.cpu_per_conf().modify(|_, w| w.cpuperiod_sel().bits(0));
    sys.sysclk_conf().modify(|_, w| w.pre_div_cnt().bits(0).soc_clk_sel().bits(1));
    rom::ets_update_cpu_frequency(80);
    true
}

/// The load at 160 MHz (round 2, R10): the CPU walks the image and feeds
/// the SHA unit, and at 80 MHz that, not the flash, set the pace once the
/// flash ran at 80 MHz. ESP-IDF's `rtc_clk_cpu_freq_to_pll_mhz(160)` from 80:
/// one more LDO slave on BEFORE the clock rises (five of six), the divider,
/// no voltage change (that is 240 MHz's alone); APB stays 80 MHz, so the
/// console and the flash clock are untouched. Only after `pll_80`.
///
/// # Safety
/// The CPU is on the PLL at 80 MHz.
pub unsafe fn cpu_160() {
    let rtc = &*RTC_CNTL::ptr();
    let sys = &*SYSTEM::ptr();
    rtc.date().modify(|_, w| w.ldo_slave().bits(DEFAULT_LDO_SLAVE >> 2));
    sys.cpu_per_conf().modify(|_, w| w.cpuperiod_sel().bits(1));
    rom::ets_update_cpu_frequency(160);
}

/// Back to the 80 MHz the C loader hands over: the clock first, then the
/// LDO slave off, ESP-IDF's order for a falling clock.
///
/// # Safety
/// The CPU is on the PLL (at 160 or 80 MHz).
pub unsafe fn cpu_80() {
    let rtc = &*RTC_CNTL::ptr();
    let sys = &*SYSTEM::ptr();
    sys.cpu_per_conf().modify(|_, w| w.cpuperiod_sel().bits(0));
    rom::ets_update_cpu_frequency(80);
    rtc.date().modify(|_, w| w.ldo_slave().bits(DEFAULT_LDO_SLAVE >> 1));
}

/// The CPU clock as the ROM counts it and the two clock-select registers,
/// raw: what the loader is about to hand over.
#[cfg(feature = "verbose")]
pub fn cpu_regs() -> (u32, u32, u32) {
    // SAFETY: register reads; a ROM routine with no preconditions.
    unsafe {
        let sys = &*SYSTEM::ptr();
        (rom::ets_get_cpu_frequency(), sys.sysclk_conf().read().bits(), sys.cpu_per_conf().read().bits())
    }
}

/// The two flash controllers' clock registers and SPI0's control register,
/// raw: what the loader reads flash with.
#[cfg(feature = "verbose")]
pub fn flash_regs() -> (u32, u32, u32) {
    // SAFETY: register reads.
    unsafe {
        let spi0 = &*esp32s3::SPI0::ptr();
        let spi1 = &*esp32s3::SPI1::ptr();
        (spi0.clock().read().bits(), spi0.ctrl().read().bits(), spi1.clock().read().bits())
    }
}

/// The loader's own image header at flash `0x0`: the flash mode, speed and
/// size the ROM configured from and the app's driver will trust.
#[derive(Clone, Copy)]
pub struct OwnHeader {
    pub mode: u8,
    pub speed: u8,
    pub size_code: u8,
}

impl OwnHeader {
    pub fn mode_name(&self) -> &'static str {
        match self.mode {
            0 => "qio",
            1 => "qout",
            2 => "dio",
            3 => "dout",
            4 => "fast-read",
            5 => "slow-read",
            _ => "?",
        }
    }
    pub fn speed_mhz(&self) -> u32 {
        match self.speed {
            0 => 40,
            1 => 26,
            2 => 20,
            0xF => 80,
            _ => 0,
        }
    }
    /// The SPI clock divider against the 80 MHz bus
    /// (`bootloader_flash_clock_config`).
    pub fn freq_div(&self) -> u8 {
        match self.speed {
            0 => 2,
            1 => 3,
            2 => 4,
            0xF => 1,
            _ => 2,
        }
    }
    pub fn size_bytes(&self) -> u32 {
        (1 << 20) << self.size_code.min(7)
    }
}

pub fn own_header() -> Result<OwnHeader, flash::Fault> {
    let mut hdr = [0u32; 6];
    flash::read(0, &mut hdr)?;
    let h = flash::bytes(&hdr);
    Ok(OwnHeader { mode: h[2], speed: h[3] & 0xF, size_code: h[3] >> 4 })
}

/// The ROM's flash-chip record, through the pointer the ROM keeps to it
/// (`rom_spiflash_legacy_data->chip`): what `esp-storage` and ESP-IDF's
/// driver read the flash size from.
#[repr(C)]
struct RomChip {
    device_id: u32,
    chip_size: u32,
    block_size: u32,
    sector_size: u32,
    page_size: u32,
    status_mask: u32,
}

#[repr(C)]
struct LegacyData {
    chip: RomChip,
    dummy_len_plus: [u8; 3],
    sig_matrix: u8,
}

extern "C" {
    static rom_spiflash_legacy_data: *mut LegacyData;
}

/// Diagnostics only (feature `flash-diag`): the chip id and the two status
/// words as the ROM's own routines return them.
#[cfg(feature = "flash-diag")]
pub fn flash_status() -> (u32, u32, u32, i32, i32) {
    // SAFETY: the ROM's own record, through its own pointer; read-only
    // status commands.
    unsafe {
        let chip = core::ptr::addr_of_mut!((*rom_spiflash_legacy_data).chip) as *mut u32;
        let (mut lo, mut hi) = (0u32, 0u32);
        let a = rom::esp_rom_spiflash_read_status(chip, &mut lo);
        let b = rom::esp_rom_spiflash_read_statushigh(chip, &mut hi);
        ((*rom_spiflash_legacy_data).chip.device_id, lo, hi, a, b)
    }
}

/// Quad I/O (round 3, feature `qio`), ESP-IDF's `bootloader_enable_qio_mode`
/// without its write: when the chip's Quad Enable bit is already set (the
/// bit ESP-IDF's table names for the manufacturer: status bit 6 for MXIC
/// and ISSI, bit 9 -- status register 2's bit 1 -- for the rest), the ROM's
/// read mode goes to QIO for both flash controllers, the WP and HD pins to
/// the controller, and the dummy phase driven. When it is not set the
/// loader stays in the header's mode and says so. Returns whether QIO is on.
#[cfg(feature = "qio")]
pub fn qio_if_enabled() -> bool {
    // SAFETY: the ROM's own record and routines; status reads only; the
    // loader runs from RAM, so changing the read mode under the cache is
    // what ESP-IDF's loader does at the same point.
    unsafe {
        let chip = core::ptr::addr_of_mut!((*rom_spiflash_legacy_data).chip) as *mut u32;
        let mfg = ((*rom_spiflash_legacy_data).chip.device_id >> 16) & 0xFF;
        let (mut lo, mut hi) = (0u32, 0u32);
        if rom::esp_rom_spiflash_read_status(chip, &mut lo) != 0
            || rom::esp_rom_spiflash_read_statushigh(chip, &mut hi) != 0
        {
            return false;
        }
        // `read_statushigh` is RDSR2 (0x35) shifted up a byte
        let status = (lo & 0xFF) | (hi & 0xFF00);
        let qe = match mfg {
            0xC2 | 0x9D => status & (1 << 6) != 0,
            _ => status & (1 << 9) != 0,
        };
        if !qe {
            return false;
        }
        if rom::esp_rom_spiflash_config_readmode(0) != 0 {
            return false;
        }
        rom::esp_rom_spiflash_select_qio_pins(rom::ets_efuse_get_wp_pad() as u8, rom::ets_efuse_get_spiconfig());
        // `bootloader_flash_set_dummy_out`
        (*esp32s3::SPI0::ptr()).ctrl().modify(|_, w| w.fdummy_out().set_bit().d_pol().set_bit().q_pol().set_bit());
        (*esp32s3::SPI1::ptr()).ctrl().modify(|_, w| w.fdummy_out().set_bit().d_pol().set_bit().q_pol().set_bit());
        true
    }
}

/// `bootloader_flash_update_size`: the size from the header into the
/// ROM's record, so the app's flash driver knows the whole chip.
pub fn set_flash_size(bytes: u32) {
    // SAFETY: the ROM's own record, in its RAM, through its own pointer.
    unsafe { (*rom_spiflash_legacy_data).chip.chip_size = bytes }
}

/// Microseconds since reset, from the systimer (16 MHz from the crystal,
/// counting since the chip came up, untouched by the CPU clock).
pub fn now_us() -> u64 {
    // SAFETY: as in `early_init`.
    let st = unsafe { &*SYSTIMER::ptr() };
    if st.conf().read().timer_unit0_work_en().bit_is_clear() {
        st.conf().modify(|_, w| w.timer_unit0_work_en().set_bit());
    }
    st.unit0_op().write(|w| w.update().set_bit());
    while st.unit0_op().read().value_valid().bit_is_clear() {}
    let lo = st.unit0_value().lo().read().bits() as u64;
    let hi = st.unit0_value().hi().read().bits() as u64;
    ((hi << 32) | lo) / 16
}

/// Diagnostics only (feature `jump-stamp`): the systimer before and after
/// `clocks_up` (round 3).
#[cfg(feature = "jump-stamp")]
pub static mut STAMP_US: [u64; 2] = [0; 2];

/// `set_cache_and_start_app`: the header's flash clock divider on both
/// SPI controllers once more, both caches suspended and invalidated, the
/// MMU cleared, the app's rodata and text mapped (64 KB pages, flash), the
/// caches resumed, the jump.
///
/// # Safety
/// The image's RAM segments are in place and every check has passed.
pub unsafe fn start(entry: u32, drom: Option<Map>, irom: Option<Map>, freq_div: u8) -> ! {
    // No wait for the console to drain (round 3): the last line is still in
    // UART0's FIFO, about 9.4 ms of it at 115200, and the FIFO empties on
    // its own after the jump -- the app keeps the 80 MHz bus the divider was
    // set for. An app that changes UART0 or that clock waits for idle first,
    // as ESP-IDF's startup does. Reset to `main` 44.1 -> 34.8 ms.
    rom::esp_rom_spiflash_config_clk(freq_div, 0);
    rom::esp_rom_spiflash_config_clk(freq_div, 1);
    let ia = rom::Cache_Suspend_ICache();
    let da = rom::Cache_Suspend_DCache();
    rom::Cache_Invalidate_ICache_All();
    rom::Cache_Invalidate_DCache_All();
    for i in 0..MMU_ENTRIES {
        MMU_TABLE.add(i).write_volatile(MMU_INVALID);
    }
    if let Some(m) = drom {
        map(rom::Cache_Dbus_MMU_Set, m);
        rom::Cache_Dbus_MMU_Set(0, DROM_END_ENTRY_VADDR, m.flash & !(MMU_PAGE - 1), 64, 1, 0);
    }
    if let Some(m) = irom {
        map(rom::Cache_Ibus_MMU_Set, m);
    }
    rom::Cache_Resume_DCache(da);
    rom::Cache_Resume_ICache(ia);
    // diagnostics only (feature `jump-stamp`): the systimer just before the
    // jump, in RTC_CNTL STORE0 (unused by ESP-IDF and esp-hal), so an app
    // can split the time before its `main` (round 3)
    // (packed in 64 us units: before `clocks_up`, after it, the jump)
    #[cfg(feature = "jump-stamp")]
    (*RTC_CNTL::ptr()).store0().write(|w| {
        let u = |t: u64| ((t / 64) as u32) & 0x3FF;
        w.bits(u(STAMP_US[0]) | (u(STAMP_US[1]) << 10) | (u(now_us()) << 20))
    });
    let app: extern "C" fn() -> ! = core::mem::transmute(entry);
    app()
}

unsafe fn map(set: unsafe extern "C" fn(u32, u32, u32, u32, u32, u32) -> i32, m: Map) {
    let vaddr = m.vaddr & !(MMU_PAGE - 1);
    let paddr = m.flash & !(MMU_PAGE - 1);
    let span = (m.vaddr - vaddr) + m.len;
    let pages = span.div_ceil(MMU_PAGE);
    let rc = set(0, vaddr, paddr, 64, pages, 0);
    if rc != 0 {
        crate::fatal!("cache MMU set {:#x} <- {:#x} x{} failed: {}", vaddr, paddr, pages, rc);
    }
}
