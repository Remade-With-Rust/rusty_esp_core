//! Flash through the ROM and the cache. Small things (the loader's own
//! header, the partition table, `otadata`) are read straight off SPI1 by
//! the ROM. An app image is mapped through the data cache, 64 KB pages at
//! the DROM window, and read as memory, the way the C loader's
//! `bootloader_mmap` reads it: the cache fetches whole lines over SPI0 and
//! the ROM's SHA takes the mapped bytes as they are. Erases and writes run
//! with both caches suspended, as ESP-IDF does: a cache fetch while the
//! chip is busy reads garbage.

use crate::rom;

/// A ROM flash routine's non-zero return (`i32::MIN`: a map too large for
/// the window).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fault(pub i32);

const PAGE: u32 = 0x1_0000;
const WINDOW: u32 = 0x3C00_0000;
const WINDOW_LEN: u32 = 0x200_0000;

/// Reads `dst.len() * 4` bytes at `addr` (a multiple of 4).
pub fn read(addr: u32, dst: &mut [u32]) -> Result<(), Fault> {
    if dst.is_empty() {
        return Ok(());
    }
    // SAFETY: `dst` is word-aligned and the length is its own.
    let r = unsafe { rom::esp_rom_spiflash_read(addr, dst.as_mut_ptr(), (dst.len() * 4) as i32) };
    if r == 0 { Ok(()) } else { Err(Fault(r)) }
}

/// Maps `len` bytes of flash at `addr` through the data cache and returns
/// them as memory, valid until the next `map` or the jump.
pub fn map(addr: u32, len: u32) -> Result<&'static [u8], Fault> {
    let first = addr & !(PAGE - 1);
    let span = (addr - first) as u64 + len as u64;
    let pages = span.div_ceil(PAGE as u64);
    if pages * PAGE as u64 > WINDOW_LEN as u64 {
        return Err(Fault(i32::MIN));
    }
    // SAFETY: ROM routines; the window is this loader's to map, nothing of
    // the app runs yet, and the slice is read-only memory behind the cache.
    unsafe {
        let autoload = rom::Cache_Suspend_DCache();
        rom::Cache_Invalidate_DCache_All();
        let rc = rom::Cache_Dbus_MMU_Set(0, WINDOW, first, 64, pages as u32, 0);
        rom::Cache_Resume_DCache(autoload);
        if rc != 0 {
            return Err(Fault(rc));
        }
        Ok(core::slice::from_raw_parts((WINDOW + (addr - first)) as *const u8, len as usize))
    }
}

/// Erases the 4 KB sector holding `addr`.
pub fn erase_sector(addr: u32) -> Result<(), Fault> {
    // SAFETY: a ROM routine; the sector number is in range for any chip
    // the partition table fits.
    with_caches_suspended(|| unsafe { rom::esp_rom_spiflash_erase_sector(addr / 4096) })
}

/// Writes whole words at `addr` (a multiple of 4) into erased flash.
pub fn write(addr: u32, src: &[u32]) -> Result<(), Fault> {
    // SAFETY: `src` is word-aligned and the length is its own.
    with_caches_suspended(|| unsafe { rom::esp_rom_spiflash_write(addr, src.as_ptr(), (src.len() * 4) as i32) })
}

/// Clears the chip's write-protection bits, once before the first write.
pub fn unlock() -> Result<(), Fault> {
    // SAFETY: a ROM routine with no preconditions.
    with_caches_suspended(|| unsafe { rom::esp_rom_spiflash_unlock() })
}

fn with_caches_suspended(f: impl FnOnce() -> i32) -> Result<(), Fault> {
    // SAFETY: ROM routines; the loader runs from internal SRAM, so
    // suspending the caches stops nothing it needs.
    let (i, d) = unsafe { (rom::Cache_Suspend_ICache(), rom::Cache_Suspend_DCache()) };
    let r = f();
    unsafe {
        rom::Cache_Resume_DCache(d);
        rom::Cache_Resume_ICache(i);
    }
    if r == 0 { Ok(()) } else { Err(Fault(r)) }
}

/// A word buffer seen as bytes.
pub fn bytes(words: &[u32]) -> &[u8] {
    // SAFETY: every `u32` is four initialised bytes with no padding, and the
    // slice outlives the view.
    unsafe { core::slice::from_raw_parts(words.as_ptr() as *const u8, words.len() * 4) }
}

/// The little-endian word at `b[..4]`.
pub fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
