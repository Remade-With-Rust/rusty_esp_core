//! An app image as ESP-IDF's loader reads it (`esp_image_format.c`): the
//! 24-byte header, the segments (RAM ones copied in, flash ones remembered
//! for the MMU), the XOR checksum in the 16-byte tail, the appended SHA-256.
//! The image comes in as memory (the partition mapped through the cache);
//! every byte is hashed as it is walked, so the one pass that loads the
//! image also verifies it, and nothing of it runs before the pass ends.

use crate::{flash::le32, sha::Sha};

pub const MAGIC: u8 = 0xE9;
pub const CHIP_ESP32S3: u16 = 9;
pub const MAX_SEGMENTS: u8 = 16;
pub const CHECKSUM_INIT: u8 = 0xEF;
/// The hash and the checksum walk a segment in pieces this big, so the
/// second read of a piece is the data cache's, not the flash's.
const PIECE: usize = 4096;

// The S3's address map (ESP-IDF `soc.h`).
const DRAM: (u32, u32) = (0x3FC8_8000, 0x3FD0_0000);
const IRAM: (u32, u32) = (0x4037_0000, 0x403E_0000);
const DROM: (u32, u32) = (0x3C00_0000, 0x3E00_0000);
const IROM: (u32, u32) = (0x4200_0000, 0x4400_0000);
const RTC_FAST: (u32, u32) = (0x600F_E000, 0x6010_0000);
const RTC_SLOW: (u32, u32) = (0x5000_0000, 0x5000_2000);
/// This loader's own memory (`memory.x`, both bus aliases), which no
/// segment may touch: the C loader refuses the same overlap.
const LOADER_IRAM: (u32, u32) = (0x403C_B700, 0x403D_9700);
const LOADER_DRAM: (u32, u32) = (0x3FCD_B700, 0x3FCE_9700);

/// A flash segment to map: where it is in flash, where the app sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Map {
    pub flash: u32,
    pub vaddr: u32,
    pub len: u32,
}

pub struct Loaded {
    pub entry: u32,
    pub drom: Option<Map>,
    pub irom: Option<Map>,
    /// Through the appended hash: what a signature sector follows.
    pub len: u32,
    #[cfg_attr(not(feature = "verbose"), allow(dead_code))]
    pub segments: u8,
    #[cfg_attr(not(feature = "verbose"), allow(dead_code))]
    pub ram_bytes: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Magic(u8),
    Chip(u16),
    Segments(u8),
    Entry(u32),
    SegmentAddress { index: u8, addr: u32, len: u32 },
    SegmentOverlapsLoader { index: u8, addr: u32 },
    SegmentUnaligned { index: u8 },
    MapMisaligned { index: u8, vaddr: u32, flash: u32 },
    SecondMap { index: u8 },
    PastPartition,
    Checksum { stored: u8, computed: u8 },
    Sha256,
}

enum Kind {
    Ram,
    Drom,
    Irom,
    /// Below 0x1000_0000: a block the image carries but nothing loads --
    /// 0x0 is the image tool's padding (inserted so a flash-mapped segment
    /// lands on its 64 KB alignment), 0x4 is reserved for an MD5 block.
    /// ESP-IDF's `should_load` skips them and still checksums and hashes
    /// their bytes; so does this. Refusing them refused every image that
    /// needed padding (X10 found it with the DSP probe; C14 never needed any).
    Skipped,
}

fn within(range: (u32, u32), addr: u32, len: u32) -> bool {
    addr >= range.0 && (addr as u64 + len as u64) <= range.1 as u64
}

fn overlaps(range: (u32, u32), addr: u32, len: u32) -> bool {
    (addr as u64) < range.1 as u64 && (range.0 as u64) < addr as u64 + len as u64
}

fn classify(addr: u32, len: u32) -> Option<Kind> {
    if addr < 0x1000_0000 {
        Some(Kind::Skipped)
    } else if within(DRAM, addr, len) || within(IRAM, addr, len) || within(RTC_FAST, addr, len) || within(RTC_SLOW, addr, len) {
        Some(Kind::Ram)
    } else if within(DROM, addr, len) {
        Some(Kind::Drom)
    } else if within(IROM, addr, len) {
        Some(Kind::Irom)
    } else {
        None
    }
}

/// Hashes `data` piece by piece and folds every word into `xor`; with a
/// destination, copies each word there too (whole words: IRAM takes
/// nothing narrower).
fn walk(data: &[u8], sha: &mut Sha, xor: &mut u32, dst: Option<*mut u32>) {
    // the checksum in a register, and one loop per destination so the
    // flash-only walk (most of an image: the mapped segments) has no
    // per-word branch and plain loads the compiler can unroll (W16 of the
    // optimization campaign)
    let mut acc = *xor;
    let mut done = 0usize;
    // the next piece preloaded into the data cache while this one is hashed
    // (round 3): the first piece's preload started here, each later one as
    // its predecessor's work begins, one preload in flight at a time. SPI0
    // fills the cache while the SHA and the checksum run, instead of after:
    // a 951 KB image's load 101 -> 77 ms.
    // SAFETY: ROM cache routines on the mapped window `data` lies in.
    let autoload = unsafe { crate::rom::Cache_Start_DCache_Preload(data.as_ptr() as u32, data.len().min(PIECE) as u32, 0) };
    for piece in data.chunks(PIECE) {
        // SAFETY: as above.
        unsafe {
            while crate::rom::Cache_DCache_Preload_Done() == 0 {}
            let next = done + piece.len();
            if next < data.len() {
                crate::rom::Cache_Start_DCache_Preload(
                    data.as_ptr().add(next) as u32,
                    (data.len() - next).min(PIECE) as u32,
                    0,
                );
            }
        }
        sha.update(piece);
        let words = piece.len() / 4;
        let src = piece.as_ptr() as *const u32;
        match dst {
            Some(d) => {
                for i in 0..words {
                    // SAFETY: the source is the mapped image, word-aligned
                    // (every segment offset is); the destination is inside
                    // the chip's RAM and outside this loader. Volatile so
                    // every word reaches the RAM the app will run from.
                    unsafe {
                        let w = src.add(i).read();
                        acc ^= w;
                        d.add(done / 4 + i).write_volatile(w);
                    }
                }
            }
            None => {
                for i in 0..words {
                    // SAFETY: as above; nothing is written
                    acc ^= unsafe { src.add(i).read() };
                }
            }
        }
        done += piece.len();
    }
    // SAFETY: as above; the last preload finished before its piece was read.
    unsafe {
        while crate::rom::Cache_DCache_Preload_Done() == 0 {}
        crate::rom::Cache_End_DCache_Preload(autoload);
    }
    *xor = acc;
}

/// Verifies the image in `view` (the partition at flash address `base`,
/// mapped, word-aligned) and copies its RAM segments in.
pub fn load(view: &[u8], base: u32, sha: &mut Sha) -> Result<Loaded, Refusal> {
    let h = view.get(..24).ok_or(Refusal::PastPartition)?;
    if h[0] != MAGIC {
        return Err(Refusal::Magic(h[0]));
    }
    let segments = h[1];
    if segments == 0 || segments > MAX_SEGMENTS {
        return Err(Refusal::Segments(segments));
    }
    let entry = le32(&h[4..8]);
    let chip = u16::from_le_bytes([h[12], h[13]]);
    if chip != CHIP_ESP32S3 {
        return Err(Refusal::Chip(chip));
    }
    let hash_appended = h[23] == 1;
    if !(within(IRAM, entry, 4) || within(IROM, entry, 4)) {
        return Err(Refusal::Entry(entry));
    }

    sha.start();
    sha.update(h);
    let mut xor: u32 = 0;
    let mut off: usize = 24;
    let (mut drom, mut irom) = (None, None);
    let mut ram_bytes = 0u32;

    for index in 0..segments {
        let sh = view.get(off..off + 8).ok_or(Refusal::PastPartition)?;
        sha.update(sh);
        off += 8;
        let (addr, len) = (le32(&sh[0..4]), le32(&sh[4..8]));
        if len == 0 {
            continue;
        }
        if addr % 4 != 0 || len % 4 != 0 {
            return Err(Refusal::SegmentUnaligned { index });
        }
        let data = view.get(off..off + len as usize).ok_or(Refusal::PastPartition)?;
        let kind = classify(addr, len).ok_or(Refusal::SegmentAddress { index, addr, len })?;
        match kind {
            Kind::Ram => {
                if overlaps(LOADER_IRAM, addr, len) || overlaps(LOADER_DRAM, addr, len) {
                    return Err(Refusal::SegmentOverlapsLoader { index, addr });
                }
                walk(data, sha, &mut xor, Some(addr as *mut u32));
                ram_bytes += len;
            }
            Kind::Drom | Kind::Irom => {
                let flash_addr = base + off as u32;
                if (flash_addr & 0xFFFF) != (addr & 0xFFFF) {
                    return Err(Refusal::MapMisaligned { index, vaddr: addr, flash: flash_addr });
                }
                let slot = if matches!(kind, Kind::Drom) { &mut drom } else { &mut irom };
                if slot.is_some() {
                    return Err(Refusal::SecondMap { index });
                }
                *slot = Some(Map { flash: flash_addr, vaddr: addr, len });
                walk(data, sha, &mut xor, None);
            }
            Kind::Skipped => walk(data, sha, &mut xor, None),
        }
        off += len as usize;
    }

    // The checksum sits in the last byte of the 16-byte block the segments
    // end in (one byte at least belongs to it).
    let tail_end = (off + 1 + 15) & !15;
    let tail = view.get(off..tail_end).ok_or(Refusal::PastPartition)?;
    sha.update(tail);
    let stored = tail[tail.len() - 1];
    let computed = (xor ^ (xor >> 8) ^ (xor >> 16) ^ (xor >> 24)) as u8 ^ CHECKSUM_INIT;
    if stored != computed {
        return Err(Refusal::Checksum { stored, computed });
    }
    let digest = sha.finish();
    let mut len = tail_end;
    if hash_appended {
        let stored = view.get(tail_end..tail_end + 32).ok_or(Refusal::PastPartition)?;
        if stored != digest {
            return Err(Refusal::Sha256);
        }
        len += 32;
    }
    Ok(Loaded { entry, drom, irom, len: len as u32, segments, ram_bytes })
}
