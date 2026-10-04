//! Espressif's NVS partition format, read and written in pure Rust: the
//! [`Kv`] seam's home on Track B, where there is no ESP-IDF to ask.
//!
//! **The format** (version 2, what every ESP-IDF since 4.x reads and what
//! `espino-nvs` writes byte for byte with `nvs_partition_gen.py`): a
//! partition is pages of 4,096 bytes. A page starts with a 32-byte header
//! (state, sequence number, version, CRC), then a 32-byte entry-state
//! bitmap (two bits per entry: `11` empty, `10` written, `00` erased),
//! then up to 126 entries of 32 bytes. An entry is a namespace index, a
//! type, a span (entries it occupies), a chunk index, a CRC over its own
//! header, a 16-byte key and 8 bytes of data. A namespace is an entry of
//! type `U8` in namespace 0 whose key is the name and whose value is the
//! index. A primitive is one entry; a string is a header (length and the
//! data's CRC) followed by the bytes in the next entries; a blob is
//! `BLOB_DATA` chunks — each a header plus data entries, never split across
//! a page — and one `BLOB_IDX` entry naming the total size, the chunk count
//! and the first chunk index. Every bit in flash goes only from 1 to 0
//! until a page is erased, which is what lets an entry be added, or marked
//! erased, in place.
//!
//! **What this reads:** everything — primitives, strings, blobs, old
//! single-page blobs — from every page, with the header and data CRCs
//! checked. **What this writes:** blobs, and namespaces as it needs them,
//! and the erasure of any entry; that is what a device key needs
//! (`rusty_esp_mid` stores it as a 32-byte blob under one key, once per
//! lifetime), and it is exactly what ESP-IDF's `nvs_set_blob` leaves in
//! flash, so a partition written here reads under Track A and the other
//! way round. **Reclaiming space:** one page is kept free, as NVS
//! requires, and when every other page is full a `put` compacts: the full
//! page with the most erased entries has its live entries copied, header and
//! span together, into the reserve page, which becomes the active page with
//! the next sequence number, and the old page is erased. Until that erase
//! every value is in flash twice, identical, so a power loss in between
//! loses nothing. Before this (2026-10-04) `put` failed with
//! `BufferTooSmall` once the pages were full: four adoptions and a few
//! setup sessions filled the bench XIAO's 3-page identity partition (E3's
//! C16, run 3: the adoption's record refused by the store).
//!
//! Replacing a value marks the old entries erased and then writes the new
//! ones. A power loss between the two loses the key; ESP-IDF orders it the
//! other way round at the cost of a duplicate its loader must resolve. For
//! a key written once this window never opens, and this module says so
//! rather than carrying the resolver.
//!
//! The flash behind it is the [`Flash`] trait: reads and writes of
//! four-byte-aligned words at partition-relative offsets, and page erase —
//! `esp-storage` on the chip, a `Vec` in the tests. [`NvsKv`] is one
//! namespace of one partition as a [`Kv`].

use core::cell::RefCell;

use crate::error::{Error, Result};
use crate::hal::{Kv, check_key};

/// Bytes per page.
pub const PAGE_SIZE: u32 = 4096;
/// Bytes per entry.
pub const ENTRY_SIZE: u32 = 32;
/// Entries a page holds after its header and bitmap.
pub const ENTRIES_PER_PAGE: u32 = 126;
/// Longest key, as NVS defines it.
pub const MAX_KEY: usize = 15;
/// Longest namespace name.
pub const MAX_NAMESPACE: usize = 15;
/// Pages this reader will index: 256 KB of partition.
pub const MAX_PAGES: usize = 64;

const BITMAP_OFFSET: u32 = 32;
const ENTRIES_OFFSET: u32 = 64;
/// Pages whose verified entry headers the reader remembers (W17).
const VERIFIED_PAGES: usize = 8;
/// Chunk numbers a blob `get` notes on its first scan (W14).
const NOTED: usize = 4;
/// The noted chunks: number, place, header.
type Noted = [Option<(u8, At, Entry)>; NOTED];

const STATE_UNINITIALIZED: u32 = 0xFFFF_FFFF;
const STATE_ACTIVE: u32 = 0xFFFF_FFFE;
const STATE_FULL: u32 = 0xFFFF_FFFC;
const VERSION2: u8 = 0xFE;
const CHUNK_ANY: u8 = 0xFF;

const ENTRY_EMPTY: u8 = 0b11;
const ENTRY_WRITTEN: u8 = 0b10;
const ENTRY_ERASED: u8 = 0b00;

const TYPE_U8: u8 = 0x01;
const TYPE_SZ: u8 = 0x21;
const TYPE_BLOB: u8 = 0x41;
const TYPE_BLOB_DATA: u8 = 0x42;
const TYPE_BLOB_IDX: u8 = 0x48;

/// CRC-32 as NVS computes it: the reflected polynomial, seed `0xFFFFFFFF`,
/// the final inversion — `zlib.crc32(data, 0xFFFFFFFF)`.
#[must_use]
pub fn crc32(data: &[u8]) -> u32 {
    !crc32_update(!0xFFFF_FFFFu32, data)
}

/// The reflected polynomial's remainders, slicing-by-4: `CRC_TABLES[0]` is
/// the one-byte table, and `CRC_TABLES[k][b]` is byte `b` followed by `k`
/// zero bytes, so four bytes fold in one step of four lookups where the
/// bitwise form took thirty-two shift-and-branch steps (W1 for the table,
/// W18 for the slicing; the bitwise form is the tests' oracle).
const CRC_TABLES: [[u32; 256]; 4] = {
    let mut t = [[0u32; 256]; 4];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
            k += 1;
        }
        t[0][i] = c;
        i += 1;
    }
    let mut i = 0;
    while i < 256 {
        let mut k = 1;
        while k < 4 {
            let prev = t[k - 1][i];
            t[k][i] = (prev >> 8) ^ t[0][(prev & 0xFF) as usize];
            k += 1;
        }
        i += 1;
    }
    t
};

/// Fold `data` into a running (uninverted) CRC.
fn crc32_update(mut crc: u32, data: &[u8]) -> u32 {
    let mut words = data.chunks_exact(4);
    for w in &mut words {
        let x = crc ^ u32::from_le_bytes([w[0], w[1], w[2], w[3]]);
        crc = CRC_TABLES[3][(x & 0xFF) as usize]
            ^ CRC_TABLES[2][((x >> 8) & 0xFF) as usize]
            ^ CRC_TABLES[1][((x >> 16) & 0xFF) as usize]
            ^ CRC_TABLES[0][(x >> 24) as usize];
    }
    for &b in words.remainder() {
        crc = CRC_TABLES[0][((crc ^ u32::from(b)) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc
}

/// A flash region holding one NVS partition.
///
/// Offsets are relative to the partition. Reads and writes are of whole
/// four-byte words at four-byte offsets (what `esp-storage` accepts); a
/// write only clears bits; [`Flash::erase_page`] sets a whole page to
/// `0xFF`.
pub trait Flash {
    /// Bytes in the partition: a multiple of [`PAGE_SIZE`].
    fn len(&self) -> u32;
    /// Copy `buf.len()` bytes from `offset`.
    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<()>;
    /// Program `data` at `offset`: every `0` bit in `data` clears the bit
    /// in flash, every `1` leaves it.
    fn write(&mut self, offset: u32, data: &[u8]) -> Result<()>;
    /// Set the page at `offset` (a multiple of [`PAGE_SIZE`]) to `0xFF`.
    fn erase_page(&mut self, offset: u32) -> Result<()>;
    /// True when the partition holds no pages.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// One 32-byte entry.
#[derive(Clone, Copy)]
struct Entry([u8; 32]);

impl Entry {
    fn ns(&self) -> u8 {
        self.0[0]
    }
    fn ty(&self) -> u8 {
        self.0[1]
    }
    fn span(&self) -> u32 {
        u32::from(self.0[2]).max(1)
    }
    fn chunk(&self) -> u8 {
        self.0[3]
    }
    fn key(&self) -> &[u8] {
        let field = &self.0[8..24];
        let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
        &field[..end]
    }
    fn data(&self) -> &[u8] {
        &self.0[24..32]
    }
    fn u16_at(&self, at: usize) -> u16 {
        u16::from_le_bytes([self.0[at], self.0[at + 1]])
    }
    fn u32_at(&self, at: usize) -> u32 {
        u32::from_le_bytes([self.0[at], self.0[at + 1], self.0[at + 2], self.0[at + 3]])
    }
    /// The header CRC covers bytes 0..4 and 8..32.
    fn crc_ok(&self) -> bool {
        let mut covered = [0u8; 28];
        covered[..4].copy_from_slice(&self.0[..4]);
        covered[4..].copy_from_slice(&self.0[8..]);
        crc32(&covered) == self.u32_at(4)
    }
    fn seal(&mut self) {
        let mut covered = [0u8; 28];
        covered[..4].copy_from_slice(&self.0[..4]);
        covered[4..].copy_from_slice(&self.0[8..]);
        self.0[4..8].copy_from_slice(&crc32(&covered).to_le_bytes());
    }
    fn new(ns: u8, ty: u8, span: u8, chunk: u8, key: &[u8]) -> Self {
        let mut e = [0xFFu8; 32];
        e[0] = ns;
        e[1] = ty;
        e[2] = span;
        e[3] = chunk;
        e[8..24].fill(0);
        e[8..8 + key.len()].copy_from_slice(key);
        Entry(e)
    }
}

/// Where an entry sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct At {
    page: u32,
    index: u32,
}

/// A page's header, decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PageHeader {
    state: u32,
    seq: u32,
}

/// An NVS partition over a [`Flash`].
pub struct Nvs<F: Flash> {
    flash: F,
    pages: u32,
    /// Data pages in sequence order (oldest first), then their count.
    order: [u32; MAX_PAGES],
    ordered: usize,
    /// The last namespace resolved: its name, the name's length, its index.
    /// What a scan would answer while the partition is unchanged; every
    /// method that writes clears it (W2 of the optimization campaign).
    ns_cache: Option<([u8; 16], u8, u8)>,
    /// Entry headers whose CRC has been checked since the last write, one
    /// bit each, for the first [`VERIFIED_PAGES`] pages (W17); cleared by
    /// every method that writes.
    verified: [[u64; 2]; VERIFIED_PAGES],
}

impl<F: Flash> core::fmt::Debug for Nvs<F> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Nvs")
            .field("pages", &self.pages)
            .field("data_pages", &self.ordered)
            .finish_non_exhaustive()
    }
}

impl<F: Flash> Nvs<F> {
    /// Open the partition: index its pages. `InvalidGeometry` when the
    /// flash is not whole pages, fewer than two of them, or more than
    /// [`MAX_PAGES`]; `Corrupt` when a page header fails its CRC.
    pub fn open(flash: F) -> Result<Self> {
        let len = flash.len();
        if len == 0 || len % PAGE_SIZE != 0 {
            return Err(Error::InvalidGeometry);
        }
        let pages = len / PAGE_SIZE;
        if pages < 2 || pages as usize > MAX_PAGES {
            return Err(Error::InvalidGeometry);
        }
        let mut nvs = Nvs {
            flash,
            pages,
            order: [0; MAX_PAGES],
            ordered: 0,
            ns_cache: None,
            verified: [[0; 2]; VERIFIED_PAGES],
        };
        nvs.index_pages()?;
        Ok(nvs)
    }

    /// The flash back, for a caller that wants to close the partition.
    pub fn into_flash(self) -> F {
        self.flash
    }

    // ---- pages -----------------------------------------------------------

    fn header(&mut self, page: u32) -> Result<PageHeader> {
        let mut h = [0u8; 32];
        self.flash.read(page * PAGE_SIZE, &mut h)?;
        let state = u32::from_le_bytes([h[0], h[1], h[2], h[3]]);
        let seq = u32::from_le_bytes([h[4], h[5], h[6], h[7]]);
        if state == STATE_ACTIVE || state == STATE_FULL {
            if h[8] != VERSION2 {
                return Err(Error::Unsupported);
            }
            let crc = u32::from_le_bytes([h[28], h[29], h[30], h[31]]);
            if crc32(&h[4..28]) != crc {
                return Err(Error::Corrupt);
            }
        }
        Ok(PageHeader { state, seq })
    }

    /// Sort the data pages by sequence number, oldest first.
    fn index_pages(&mut self) -> Result<()> {
        let mut seqs = [0u32; MAX_PAGES];
        let mut n = 0usize;
        for page in 0..self.pages {
            let h = self.header(page)?;
            if h.state == STATE_ACTIVE || h.state == STATE_FULL {
                // insertion sort: MAX_PAGES is small and this runs once
                let mut i = n;
                while i > 0 && seqs[i - 1] > h.seq {
                    seqs[i] = seqs[i - 1];
                    self.order[i] = self.order[i - 1];
                    i -= 1;
                }
                seqs[i] = h.seq;
                self.order[i] = page;
                n += 1;
            }
        }
        self.ordered = n;
        Ok(())
    }

    fn bitmap(&mut self, page: u32) -> Result<[u8; 32]> {
        let mut b = [0u8; 32];
        self.flash.read(page * PAGE_SIZE + BITMAP_OFFSET, &mut b)?;
        Ok(b)
    }

    fn entry_state(bitmap: &[u8; 32], index: u32) -> u8 {
        let bit = (index * 2) as usize;
        (bitmap[bit / 8] >> (bit & 7)) & 0b11
    }

    fn entry(&mut self, at: At) -> Result<Entry> {
        let mut e = [0u8; 32];
        self.flash.read(
            at.page * PAGE_SIZE + ENTRIES_OFFSET + at.index * ENTRY_SIZE,
            &mut e,
        )?;
        Ok(Entry(e))
    }

    /// Visit every written entry header, oldest page first, in order;
    /// data entries are stepped over by their header's span. The visitor
    /// returns `true` to stop.
    fn scan(&mut self, mut visit: impl FnMut(At, &Entry) -> bool) -> Result<()> {
        for i in 0..self.ordered {
            let page = self.order[i];
            let bitmap = self.bitmap(page)?;
            let mut index = 0u32;
            while index < ENTRIES_PER_PAGE {
                match Self::entry_state(&bitmap, index) {
                    ENTRY_WRITTEN => {
                        let at = At { page, index };
                        let e = self.entry(at)?;
                        // a header checked since the last write is not
                        // checked again (W17); the flash cannot have
                        // changed under it
                        let (p, w, bit) =
                            (page as usize, (index / 64) as usize, 1u64 << (index % 64));
                        let known = p < VERIFIED_PAGES && self.verified[p][w] & bit != 0;
                        if !known {
                            if !e.crc_ok() {
                                return Err(Error::Corrupt);
                            }
                            if p < VERIFIED_PAGES {
                                self.verified[p][w] |= bit;
                            }
                        }
                        if visit(at, &e) {
                            return Ok(());
                        }
                        index += e.span();
                    }
                    ENTRY_ERASED => index += 1,
                    // an empty slot ends the page: entries are appended in order
                    _ => break,
                }
            }
        }
        Ok(())
    }

    // ---- namespaces ------------------------------------------------------

    fn namespace_index(&mut self, name: &[u8]) -> Result<Option<u8>> {
        if let Some((cached, len, index)) = &self.ns_cache {
            if &cached[..usize::from(*len)] == name {
                return Ok(Some(*index));
            }
        }
        let found = self.namespace_index_scan(name)?;
        if let (Some(index), Ok(len)) = (found, u8::try_from(name.len())) {
            if name.len() <= 16 {
                let mut cached = [0u8; 16];
                cached[..name.len()].copy_from_slice(name);
                self.ns_cache = Some((cached, len, index));
            }
        }
        Ok(found)
    }

    fn namespace_index_scan(&mut self, name: &[u8]) -> Result<Option<u8>> {
        let mut found = None;
        self.scan(|_, e| {
            if e.ns() == 0 && e.ty() == TYPE_U8 && e.key() == name {
                found = Some(e.data()[0]);
                true
            } else {
                false
            }
        })?;
        Ok(found)
    }

    /// The smallest namespace index not in use, 1..=254.
    fn free_namespace_index(&mut self) -> Result<u8> {
        let mut used = [false; 256];
        self.scan(|_, e| {
            if e.ns() == 0 && e.ty() == TYPE_U8 {
                used[usize::from(e.data()[0])] = true;
            }
            false
        })?;
        (1..=254u8)
            .find(|&i| !used[usize::from(i)])
            .ok_or(Error::BufferTooSmall { needed: 0 })
    }

    // ---- lookup ----------------------------------------------------------

    /// The `BLOB_DATA` header for chunk `chunk` of `key` in `ns`.
    fn find_chunk(&mut self, ns: u8, key: &[u8], chunk: u8) -> Result<Option<(At, Entry)>> {
        let mut found = None;
        self.scan(|at, e| {
            if e.ns() == ns && e.ty() == TYPE_BLOB_DATA && e.chunk() == chunk && e.key() == key {
                found = Some((at, *e));
                true
            } else {
                false
            }
        })?;
        Ok(found)
    }

    /// Copy `len` data bytes that follow the header at `at` into `out`,
    /// checking them against `crc`.
    fn read_data(&mut self, at: At, len: usize, crc: u32, out: &mut [u8]) -> Result<()> {
        let mut running = !0xFFFF_FFFFu32;
        let mut done = 0usize;
        let mut index = at.index + 1;
        while done < len {
            if index >= ENTRIES_PER_PAGE {
                return Err(Error::Corrupt);
            }
            let e = self.entry(At {
                page: at.page,
                index,
            })?;
            let take = (len - done).min(ENTRY_SIZE as usize);
            out[done..done + take].copy_from_slice(&e.0[..take]);
            // the CRC over the bytes as they stream past
            running = crc32_update(running, &e.0[..take]);
            done += take;
            index += 1;
        }
        if !running != crc {
            return Err(Error::Corrupt);
        }
        Ok(())
    }

    /// The length of the value under `key` in `ns`, with its bytes in
    /// `out` when it fits: strings without their NUL, primitives as
    /// little-endian bytes, blobs assembled from their chunks. `Ok(None)`
    /// when the namespace or the key is absent; `BufferTooSmall` names the
    /// length when `out` is short.
    pub fn get(&mut self, ns: &str, key: &str, out: &mut [u8]) -> Result<Option<usize>> {
        check_namespace(ns)?;
        check_key(key)?;
        let Some(ns) = self.namespace_index(ns.as_bytes())? else {
            return Ok(None);
        };
        // a blob's chunks are found by key too; the index is what names the
        // value. On the way to it, the first header of each chunk number of
        // this key is noted (W14), so the chunks need no second scan.
        let mut noted: Noted = [None; NOTED];
        let Some((at, e)) = self.find_not_chunk_noting(ns, key.as_bytes(), &mut noted)? else {
            return Ok(None);
        };
        match e.ty() {
            TYPE_BLOB_IDX => {
                let total = e.u32_at(24) as usize;
                let count = e.0[28];
                let start = e.0[29];
                if out.len() < total {
                    return Err(Error::BufferTooSmall { needed: total });
                }
                let mut done = 0usize;
                for c in 0..count {
                    let chunk = start.wrapping_add(c);
                    let found = match noted.iter().flatten().find(|(n, _, _)| *n == chunk) {
                        Some(&(_, cat, ce)) => Some((cat, ce)),
                        None => self.find_chunk(ns, key.as_bytes(), chunk)?,
                    };
                    let Some((cat, ce)) = found else {
                        return Err(Error::Corrupt);
                    };
                    let size = usize::from(ce.u16_at(24));
                    if done + size > total {
                        return Err(Error::Corrupt);
                    }
                    self.read_data(cat, size, ce.u32_at(28), &mut out[done..done + size])?;
                    done += size;
                }
                if done != total {
                    return Err(Error::Corrupt);
                }
                Ok(Some(total))
            }
            TYPE_SZ | TYPE_BLOB => {
                let stored = usize::from(e.u16_at(24));
                let crc = e.u32_at(28);
                // a string is stored with its NUL and handed back without
                let want = if e.ty() == TYPE_SZ {
                    stored.saturating_sub(1)
                } else {
                    stored
                };
                if out.len() < want {
                    return Err(Error::BufferTooSmall { needed: want });
                }
                if e.ty() == TYPE_SZ {
                    // the CRC covers the NUL: read through a scratch entry at a time
                    let mut scratch = [0u8; 32];
                    let mut running = !0xFFFF_FFFFu32;
                    let mut done = 0usize;
                    let mut index = at.index + 1;
                    while done < stored {
                        if index >= ENTRIES_PER_PAGE {
                            return Err(Error::Corrupt);
                        }
                        let de = self.entry(At {
                            page: at.page,
                            index,
                        })?;
                        scratch.copy_from_slice(&de.0);
                        let take = (stored - done).min(32);
                        // the string's bytes without the NUL, the CRC over all
                        let keep = take.min(want.saturating_sub(done));
                        out[done..done + keep].copy_from_slice(&scratch[..keep]);
                        running = crc32_update(running, &scratch[..take]);
                        done += take;
                        index += 1;
                    }
                    if !running != crc {
                        return Err(Error::Corrupt);
                    }
                } else {
                    self.read_data(at, stored, crc, &mut out[..stored])?;
                }
                Ok(Some(want))
            }
            ty => {
                let width = match ty & 0x0F {
                    0x01 => 1,
                    0x02 => 2,
                    0x04 => 4,
                    0x08 => 8,
                    _ => return Err(Error::Unsupported),
                };
                if out.len() < width {
                    return Err(Error::BufferTooSmall { needed: width });
                }
                out[..width].copy_from_slice(&e.data()[..width]);
                Ok(Some(width))
            }
        }
    }

    /// `find`, skipping `BLOB_DATA` chunks (they carry the key too).
    /// [`Nvs::find_not_chunk`], noting the first header of each chunk number
    /// of `key` it passes (up to [`NOTED`] numbers; the first occurrence
    /// only, so a noted chunk is the one `find_chunk` would find).
    fn find_not_chunk_noting(
        &mut self,
        ns: u8,
        key: &[u8],
        noted: &mut Noted,
    ) -> Result<Option<(At, Entry)>> {
        let mut found = None;
        self.scan(|at, e| {
            if e.ns() != ns || e.key() != key {
                return false;
            }
            if e.ty() != TYPE_BLOB_DATA {
                found = Some((at, *e));
                return true;
            }
            let chunk = e.chunk();
            // the first occurrence of a number only; with no slot free the
            // number is simply not noted and its lookup scans
            if !noted.iter().flatten().any(|(n, _, _)| *n == chunk) {
                if let Some(free) = noted.iter_mut().find(|slot| slot.is_none()) {
                    *free = Some((chunk, at, *e));
                }
            }
            false
        })?;
        Ok(found)
    }

    fn find_not_chunk(&mut self, ns: u8, key: &[u8]) -> Result<Option<(At, Entry)>> {
        let mut found = None;
        self.scan(|at, e| {
            if e.ns() == ns && e.ty() != TYPE_BLOB_DATA && e.key() == key {
                found = Some((at, *e));
                true
            } else {
                false
            }
        })?;
        Ok(found)
    }

    // ---- writing ---------------------------------------------------------

    /// Clear bits in the entry-state bitmap: the two bits of `index` to
    /// `state`, through the aligned word that holds them.
    fn mark(&mut self, page: u32, index: u32, state: u8) -> Result<()> {
        let bit = index * 2;
        let byte = BITMAP_OFFSET + bit / 8;
        let word = byte & !3;
        let mut w = [0u8; 4];
        self.flash.read(page * PAGE_SIZE + word, &mut w)?;
        let i = (byte - word) as usize;
        w[i] &= !(0b11 << (bit & 7)) | (state << (bit & 7));
        self.flash.write(page * PAGE_SIZE + word, &w)
    }

    /// The first empty slot of `page`, or `ENTRIES_PER_PAGE` when full.
    fn next_free(&mut self, page: u32) -> Result<u32> {
        let bitmap = self.bitmap(page)?;
        Ok((0..ENTRIES_PER_PAGE)
            .find(|&i| Self::entry_state(&bitmap, i) == ENTRY_EMPTY)
            .unwrap_or(ENTRIES_PER_PAGE))
    }

    /// The active page, if there is one.
    fn active_page(&mut self) -> Result<Option<u32>> {
        for i in 0..self.ordered {
            let page = self.order[i];
            if self.header(page)?.state == STATE_ACTIVE {
                return Ok(Some(page));
            }
        }
        Ok(None)
    }

    /// Bring a free page into use as the active one: erase it if it is not
    /// blank, write its header with the next sequence number. The last free
    /// page is the reserve NVS keeps: rather than take it, a full page with
    /// erased entries is compacted into it ([`Self::compact`]), and the
    /// compacted page comes back as the active one.
    fn activate_page(&mut self) -> Result<u32> {
        let mut free = 0u32;
        let mut first = None;
        let mut max_seq = None;
        for page in 0..self.pages {
            let h = self.header(page)?;
            if h.state == STATE_ACTIVE || h.state == STATE_FULL {
                max_seq = Some(max_seq.map_or(h.seq, |m: u32| m.max(h.seq)));
            } else {
                free += 1;
                if first.is_none() {
                    first = Some((page, h.state));
                }
            }
        }
        let Some((page, state)) = first else {
            return Err(Error::BufferTooSmall { needed: 0 });
        };
        if free < 2 {
            return self.compact(page, state, max_seq);
        }
        self.start_page(page, state, max_seq)?;
        Ok(page)
    }

    /// `page`, free, becomes the active page with the next sequence number
    /// (erased first unless blank).
    fn start_page(&mut self, page: u32, state: u32, max_seq: Option<u32>) -> Result<()> {
        if state != STATE_UNINITIALIZED {
            self.flash.erase_page(page * PAGE_SIZE)?;
        }
        let seq = max_seq.map_or(0, |m| m.wrapping_add(1));
        let mut h = [0xFFu8; 32];
        h[0..4].copy_from_slice(&STATE_ACTIVE.to_le_bytes());
        h[4..8].copy_from_slice(&seq.to_le_bytes());
        h[8] = VERSION2;
        let crc = crc32(&h[4..28]);
        h[28..32].copy_from_slice(&crc.to_le_bytes());
        self.flash.write(page * PAGE_SIZE, &h)?;
        self.index_pages()
    }

    /// Reclaim the erased entries of one full page, into the reserve page
    /// `target`: the full page with the most erased entries is the victim;
    /// its live entries are copied, each header with the data entries of its
    /// span, into `target`, which becomes the active page; then the victim
    /// is erased and is the new reserve. Until that erase every value is in
    /// flash twice, identical. `Err(BufferTooSmall)` when no full page has
    /// anything to reclaim: the partition is truly full.
    fn compact(&mut self, target: u32, target_state: u32, max_seq: Option<u32>) -> Result<u32> {
        let mut victim: Option<(u32, u32)> = None;
        for page in 0..self.pages {
            if self.header(page)?.state != STATE_FULL {
                continue;
            }
            let bitmap = self.bitmap(page)?;
            let erased = (0..ENTRIES_PER_PAGE)
                .filter(|&i| Self::entry_state(&bitmap, i) == ENTRY_ERASED)
                .count() as u32;
            if erased > 0 && victim.is_none_or(|(_, most)| erased > most) {
                victim = Some((page, erased));
            }
        }
        let Some((victim, _)) = victim else {
            return Err(Error::BufferTooSmall { needed: 0 });
        };
        self.start_page(target, target_state, max_seq)?;
        let bitmap = self.bitmap(victim)?;
        let mut i = 0u32;
        let mut free = 0u32;
        while i < ENTRIES_PER_PAGE {
            if Self::entry_state(&bitmap, i) != ENTRY_WRITTEN {
                i += 1;
                continue;
            }
            let e = self.entry(At { page: victim, index: i })?;
            let span = e.span().clamp(1, ENTRIES_PER_PAGE - i);
            for k in 0..span {
                let mut raw = [0u8; ENTRY_SIZE as usize];
                self.flash.read(
                    victim * PAGE_SIZE + ENTRIES_OFFSET + (i + k) * ENTRY_SIZE,
                    &mut raw,
                )?;
                self.flash.write(
                    target * PAGE_SIZE + ENTRIES_OFFSET + (free + k) * ENTRY_SIZE,
                    &raw,
                )?;
                self.mark(target, free + k, ENTRY_WRITTEN)?;
            }
            free += span;
            i += span;
        }
        self.flash.erase_page(victim * PAGE_SIZE)?;
        self.ns_cache = None;
        self.verified = [[0; 2]; VERIFIED_PAGES];
        self.index_pages()?;
        Ok(target)
    }

    fn mark_full(&mut self, page: u32) -> Result<()> {
        self.flash
            .write(page * PAGE_SIZE, &STATE_FULL.to_le_bytes())?;
        self.index_pages()
    }

    /// The active page with at least `entries` free slots, moving to a
    /// fresh page when the current one is short. A fresh page may be a
    /// compacted one with entries already on it; one that is still short is
    /// marked full and the next taken, once.
    fn page_with_room(&mut self, entries: u32) -> Result<(u32, u32)> {
        if let Some(page) = self.active_page()? {
            let free = self.next_free(page)?;
            if ENTRIES_PER_PAGE - free >= entries {
                return Ok((page, free));
            }
            self.mark_full(page)?;
        }
        for _ in 0..2 {
            let page = self.activate_page()?;
            let free = self.next_free(page)?;
            if ENTRIES_PER_PAGE - free >= entries {
                return Ok((page, free));
            }
            self.mark_full(page)?;
        }
        Err(Error::BufferTooSmall { needed: entries as usize })
    }

    /// Write one entry at a slot and mark it written.
    fn write_entry(&mut self, at: At, e: &Entry) -> Result<()> {
        self.flash.write(
            at.page * PAGE_SIZE + ENTRIES_OFFSET + at.index * ENTRY_SIZE,
            &e.0,
        )?;
        self.mark(at.page, at.index, ENTRY_WRITTEN)
    }

    /// Write `data` into the entries after `at` (already counted in the
    /// caller's room), padded with `0xFF`, and mark them written.
    fn write_data(&mut self, at: At, data: &[u8]) -> Result<()> {
        for (i, chunk) in data.chunks(ENTRY_SIZE as usize).enumerate() {
            let index = at.index + 1 + i as u32;
            let mut e = [0xFFu8; 32];
            e[..chunk.len()].copy_from_slice(chunk);
            self.flash.write(
                at.page * PAGE_SIZE + ENTRIES_OFFSET + index * ENTRY_SIZE,
                &e,
            )?;
            self.mark(at.page, index, ENTRY_WRITTEN)?;
        }
        Ok(())
    }

    /// Mark the entries of the value at `at` erased: its header and, for a
    /// string or a chunk, the data entries its span covers.
    fn erase_span(&mut self, at: At, e: &Entry) -> Result<()> {
        for i in 0..e.span() {
            self.mark(at.page, at.index + i, ENTRY_ERASED)?;
        }
        Ok(())
    }

    /// The namespace's index, creating the namespace when it is new.
    fn namespace_index_or_create(&mut self, name: &[u8]) -> Result<u8> {
        if let Some(i) = self.namespace_index(name)? {
            return Ok(i);
        }
        let i = self.free_namespace_index()?;
        let (page, index) = self.page_with_room(1)?;
        let mut e = Entry::new(0, TYPE_U8, 1, CHUNK_ANY, name);
        e.0[24] = i;
        e.seal();
        self.write_entry(At { page, index }, &e)?;
        Ok(i)
    }

    /// Remove `key` from `ns`, whatever its type; `Ok(false)` when it was
    /// not there.
    pub fn remove(&mut self, ns: &str, key: &str) -> Result<bool> {
        self.ns_cache = None;
        self.verified = [[0; 2]; VERIFIED_PAGES];
        check_namespace(ns)?;
        check_key(key)?;
        let Some(ns) = self.namespace_index(ns.as_bytes())? else {
            return Ok(false);
        };
        self.remove_in(ns, key.as_bytes())
    }

    fn remove_in(&mut self, ns: u8, key: &[u8]) -> Result<bool> {
        let Some((at, e)) = self.find_not_chunk(ns, key)? else {
            return Ok(false);
        };
        if e.ty() == TYPE_BLOB_IDX {
            let count = e.0[28];
            let start = e.0[29];
            // the index first, so a reader never sees an index without its chunks
            self.mark(at.page, at.index, ENTRY_ERASED)?;
            for c in 0..count {
                if let Some((cat, ce)) = self.find_chunk(ns, key, start.wrapping_add(c))? {
                    self.erase_span(cat, &ce)?;
                }
            }
        } else {
            self.erase_span(at, &e)?;
        }
        Ok(true)
    }

    /// Store `value` as a blob under `key` in `ns`, replacing what was
    /// there. The new value is written whole before the old one is erased,
    /// under the other chunk version (chunk numbers from 0 or from 128, as
    /// ESP-IDF alternates them), so a reader sees the old value or the new
    /// one and never a mix; a write that fails erases what it wrote and
    /// leaves the old value as it was. `BufferTooSmall { needed: 0 }` when
    /// the partition's pages are used up (one is always kept free);
    /// `InvalidFormat` for an empty value, a value of more than 127 chunks,
    /// or a name outside the rules.
    pub fn put_blob(&mut self, ns: &str, key: &str, value: &[u8]) -> Result<()> {
        self.ns_cache = None;
        self.verified = [[0; 2]; VERIFIED_PAGES];
        check_namespace(ns)?;
        check_key(key)?;
        if value.is_empty() || value.len() > 0xFFFF * 64 {
            return Err(Error::InvalidFormat);
        }
        let ns = self.namespace_index_or_create(ns.as_bytes())?;
        let key = key.as_bytes();
        // the value there now decides the new version's chunk numbers
        let old = self.find_not_chunk(ns, key)?;
        let start = match old {
            Some((_, e)) if e.ty() == TYPE_BLOB_IDX => e.0[29] ^ 0x80,
            _ => 0,
        };
        let mut written: u8 = 0;
        match self.write_blob(ns, key, value, start, &mut written) {
            Ok(()) => {}
            Err(e) => {
                // the chunks this write put down go; the old value stays
                for c in 0..written {
                    if let Some((cat, ce)) = self.find_chunk(ns, key, start.wrapping_add(c))? {
                        self.erase_span(cat, &ce)?;
                    }
                }
                return Err(e);
            }
        }
        // the new index is down: the old value goes, index first
        if let Some((at, e)) = old {
            if e.ty() == TYPE_BLOB_IDX {
                let count = e.0[28];
                let old_start = e.0[29];
                self.mark(at.page, at.index, ENTRY_ERASED)?;
                for c in 0..count {
                    if let Some((cat, ce)) = self.find_chunk(ns, key, old_start.wrapping_add(c))? {
                        self.erase_span(cat, &ce)?;
                    }
                }
            } else {
                self.erase_span(at, &e)?;
            }
        }
        Ok(())
    }

    /// The chunks of `value` numbered from `start`, then its index;
    /// `written` counts the chunks put down, for the caller to undo.
    fn write_blob(
        &mut self,
        ns: u8,
        key: &[u8],
        value: &[u8],
        start: u8,
        written: &mut u8,
    ) -> Result<()> {
        // chunks as large as the page's tail allows, as the reference writes
        // them; the index after the last, on a fresh page if the tail is short
        let mut offset = 0usize;
        let mut remaining = value.len();
        loop {
            if *written >= 127 {
                return Err(Error::InvalidFormat);
            }
            // room for a header and at least one data entry
            let (page, free) = self.page_with_room(2)?;
            let tailroom = ((ENTRIES_PER_PAGE - free - 1) * ENTRY_SIZE) as usize;
            let chunk_size = remaining.min(tailroom);
            let chunk = &value[offset..offset + chunk_size];
            let data_entries = chunk_size.div_ceil(ENTRY_SIZE as usize);
            let mut e = Entry::new(
                ns,
                TYPE_BLOB_DATA,
                u8::try_from(data_entries + 1).map_err(|_| Error::InvalidFormat)?,
                start.wrapping_add(*written),
                key,
            );
            e.0[24..26].copy_from_slice(&(chunk_size as u16).to_le_bytes());
            e.0[28..32].copy_from_slice(&crc32(chunk).to_le_bytes());
            e.seal();
            let at = At { page, index: free };
            // data first, header last: an interrupted write leaves no header
            self.write_data(at, chunk)?;
            self.write_entry(at, &e)?;
            *written += 1;
            offset += chunk_size;
            remaining -= chunk_size;
            if remaining > 0 || tailroom - chunk_size < ENTRY_SIZE as usize {
                self.mark_full(page)?;
            }
            if remaining == 0 {
                let (page, free) = self.page_with_room(1)?;
                let mut idx = Entry::new(ns, TYPE_BLOB_IDX, 1, CHUNK_ANY, key);
                idx.0[24..28].copy_from_slice(&(value.len() as u32).to_le_bytes());
                idx.0[28] = *written;
                idx.0[29] = start;
                idx.seal();
                self.write_entry(At { page, index: free }, &idx)?;
                return Ok(());
            }
        }
    }
}

fn check_namespace(ns: &str) -> Result<()> {
    if ns.is_empty() || ns.len() > MAX_NAMESPACE || ns.bytes().any(|b| b == 0 || b >= 0x80) {
        return Err(Error::InvalidFormat);
    }
    Ok(())
}

/// One namespace of one NVS partition as a [`Kv`]: `get` reads a value of
/// any type, `put` writes a blob (what ESP-IDF's `set_blob` writes, so the
/// two tracks read each other's), `remove` erases.
pub struct NvsKv<F: Flash> {
    nvs: RefCell<Nvs<F>>,
    namespace: [u8; MAX_NAMESPACE],
    namespace_len: usize,
}

impl<F: Flash> core::fmt::Debug for NvsKv<F> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NvsKv")
            .field("namespace", &self.namespace())
            .finish_non_exhaustive()
    }
}

impl<F: Flash> NvsKv<F> {
    /// `namespace` on the partition `flash` holds.
    pub fn open(flash: F, namespace: &str) -> Result<Self> {
        check_namespace(namespace)?;
        let mut name = [0u8; MAX_NAMESPACE];
        name[..namespace.len()].copy_from_slice(namespace.as_bytes());
        Ok(NvsKv {
            nvs: RefCell::new(Nvs::open(flash)?),
            namespace: name,
            namespace_len: namespace.len(),
        })
    }

    /// The namespace this reads and writes.
    #[must_use]
    pub fn namespace(&self) -> &str {
        core::str::from_utf8(&self.namespace[..self.namespace_len]).unwrap_or("")
    }

    /// The partition, for anything beyond this namespace.
    pub fn partition(&self) -> core::cell::RefMut<'_, Nvs<F>> {
        self.nvs.borrow_mut()
    }
}

impl<F: Flash> Kv for NvsKv<F> {
    fn get(&self, key: &str, out: &mut [u8]) -> Result<Option<usize>> {
        let ns = self.namespace();
        self.nvs.borrow_mut().get(ns, key, out)
    }

    fn put(&mut self, key: &str, value: &[u8]) -> Result<()> {
        let ns = self.namespace();
        self.nvs.borrow_mut().put_blob(ns, key, value)
    }

    fn remove(&mut self, key: &str) -> Result<bool> {
        let ns = self.namespace();
        self.nvs.borrow_mut().remove(ns, key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bitwise CRC the reader used before the table (W1): the oracle.
    fn crc32_bitwise(data: &[u8]) -> u32 {
        let mut crc = !0xFFFF_FFFFu32;
        for &b in data {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    #[test]
    fn the_table_crc_is_the_bitwise_crc_for_every_length_and_byte() {
        let mut x = 0x2545_F491u32;
        let data: Vec<u8> = (0..4096)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect();
        for len in 0..300 {
            assert_eq!(
                crc32(&data[..len]),
                crc32_bitwise(&data[..len]),
                "len {len}"
            );
        }
        assert_eq!(crc32(&data), crc32_bitwise(&data));
        for b in 0..=255u8 {
            assert_eq!(crc32(&[b]), crc32_bitwise(&[b]), "byte {b}");
        }
        // zlib's check value
        assert_eq!(
            crc32(b"123456789") ^ 0xFFFF_FFFF ^ 0xFFFF_FFFF,
            crc32_bitwise(b"123456789")
        );
    }
    use std::vec;
    use std::vec::Vec;

    /// NOR flash in memory: a write clears bits, an erase sets a page, and
    /// every access is word-aligned or the test fails.
    struct RamFlash(Vec<u8>);

    impl RamFlash {
        fn blank(pages: usize) -> Self {
            RamFlash(vec![0xFF; pages * PAGE_SIZE as usize])
        }
        fn from_image(image: &[u8]) -> Self {
            RamFlash(image.to_vec())
        }
    }

    impl Flash for RamFlash {
        fn len(&self) -> u32 {
            self.0.len() as u32
        }
        fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<()> {
            assert_eq!(offset % 4, 0, "read offset alignment");
            assert_eq!(buf.len() % 4, 0, "read length alignment");
            let o = offset as usize;
            buf.copy_from_slice(&self.0[o..o + buf.len()]);
            Ok(())
        }
        fn write(&mut self, offset: u32, data: &[u8]) -> Result<()> {
            assert_eq!(offset % 4, 0, "write offset alignment");
            assert_eq!(data.len() % 4, 0, "write length alignment");
            let o = offset as usize;
            for (cell, &b) in self.0[o..o + data.len()].iter_mut().zip(data) {
                *cell &= b;
            }
            Ok(())
        }
        fn erase_page(&mut self, offset: u32) -> Result<()> {
            assert_eq!(offset % PAGE_SIZE, 0);
            let o = offset as usize;
            self.0[o..o + PAGE_SIZE as usize].fill(0xFF);
            Ok(())
        }
    }

    // Written by `espino-nvs` (byte-identical to Espressif's
    // nvs_partition_gen.py); see tests/fixtures/nvs/README.md.
    const SETTINGS: &[u8] = include_bytes!("../tests/fixtures/nvs/janus-settings.bin");
    const IDENTITY: &[u8] = include_bytes!("../tests/fixtures/nvs/identity-blob.bin");
    const MIXED: &[u8] = include_bytes!("../tests/fixtures/nvs/mixed.bin");

    fn get(nvs: &mut Nvs<RamFlash>, ns: &str, key: &str) -> Option<Vec<u8>> {
        let mut out = vec![0u8; 8192];
        let n = nvs.get(ns, key, &mut out).unwrap()?;
        out.truncate(n);
        Some(out)
    }

    /// W14: blob reads that take their chunks from the first scan return
    /// what was written, across rewrites, removals, blobs of one chunk and
    /// of more chunks than the reader notes (which falls back to a scan).
    /// Full pages are compacted, not fatal (E3's C16, 2026-10-04): a
    /// three-page partition takes a key written once (the device key's
    /// shape) and a value rewritten far past what two pages hold without
    /// reclaiming; every live value reads back after each write, and the
    /// pages never exceed what NVS allows (one in reserve).
    #[test]
    fn a_full_partition_compacts_and_keeps_every_live_value() {
        let mut nvs = Nvs::open(RamFlash::blank(3)).unwrap();
        let key_once = [0x5au8; 32];
        nvs.put_blob("identity", "device_key", &key_once).unwrap();
        // 322 bytes, as an adoption record: 11 entries a write; two pages
        // hold ~22 writes, so 60 rewrites compact several times over
        let mut record = [0u8; 322];
        let mut out = [0u8; 400];
        for round in 0..60u32 {
            record[..4].copy_from_slice(&round.to_le_bytes());
            record[4..].fill(round as u8);
            nvs.put_blob("identity", "adoption", &record)
                .unwrap_or_else(|e| panic!("round {round}: {e:?}"));
            nvs.put_blob("identity", "owner_pin", &[round as u8; 48]).unwrap();
            let n = nvs.get("identity", "adoption", &mut out).unwrap().unwrap();
            assert_eq!(&out[..n], &record[..], "round {round}: the adoption");
            let n = nvs.get("identity", "owner_pin", &mut out).unwrap().unwrap();
            assert_eq!(&out[..n], &[round as u8; 48], "round {round}: the pin");
            let n = nvs.get("identity", "device_key", &mut out).unwrap().unwrap();
            assert_eq!(&out[..n], &key_once[..], "round {round}: the key written once");
            // one page free, as NVS requires
            let free = (0..3)
                .filter(|&p| {
                    let st = nvs.header(p).unwrap().state;
                    st != STATE_ACTIVE && st != STATE_FULL
                })
                .count();
            assert!(free >= 1, "round {round}: no reserve page");
        }
        // what came back is readable by a fresh open too
        let image = nvs.into_flash().0;
        let mut again = Nvs::open(RamFlash::from_image(&image)).unwrap();
        let n = again.get("identity", "device_key", &mut out).unwrap().unwrap();
        assert_eq!(&out[..n], &key_once[..]);
        let n = again.get("identity", "adoption", &mut out).unwrap().unwrap();
        assert_eq!(&out[..4], &59u32.to_le_bytes());
        assert_eq!(n, 322);
    }

    /// A partition whose live entries fill both usable pages is truly full:
    /// the put fails and what was there stays readable.
    #[test]
    fn a_truly_full_partition_still_refuses_and_keeps_its_values() {
        let mut nvs = Nvs::open(RamFlash::blank(3)).unwrap();
        let mut n = 0;
        loop {
            let key = format!("k{n}");
            match nvs.put_blob("ns", &key, &[n as u8; 200]) {
                Ok(()) => n += 1,
                Err(Error::BufferTooSmall { .. }) => break,
                Err(e) => panic!("{e:?}"),
            }
            assert!(n < 200, "never fills");
        }
        assert!(n >= 20, "two pages hold more than {n} values of 200 bytes");
        let mut out = [0u8; 256];
        for k in 0..n {
            let got = nvs.get("ns", &format!("k{k}"), &mut out).unwrap().unwrap();
            assert_eq!(&out[..got], &[k as u8; 200], "k{k} after the refusal");
        }
    }

    #[test]
    fn noted_chunks_read_back_exactly_what_was_written() {
        let mut nvs = Nvs::open(RamFlash::blank(8)).unwrap();
        let mut truth: std::collections::BTreeMap<std::string::String, Vec<u8>> =
            std::collections::BTreeMap::new();
        let mut x = 0x2545_F491u32;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x
        };
        let sizes = [1usize, 16, 31, 33, 322, 900, 4000, 20_000];
        for round in 0..60 {
            let key = std::format!("k{}", next() % 5);
            if next() % 4 == 0 {
                let removed = nvs.remove("janus", &key).unwrap();
                assert_eq!(removed, truth.remove(&key).is_some(), "round {round}");
            } else {
                let len = sizes[(next() as usize) % sizes.len()];
                let value: Vec<u8> = (0..len).map(|i| (i as u32 ^ next()) as u8).collect();
                match nvs.put_blob("janus", &key, &value) {
                    Ok(()) => {
                        truth.insert(key.clone(), value);
                    }
                    // a full partition refuses; nothing changed for that key
                    Err(Error::BufferTooSmall { .. }) => {}
                    Err(e) => panic!("round {round}: {e:?}"),
                }
            }
            for (k, v) in &truth {
                let mut out = vec![0u8; 32 * 1024];
                let n = nvs.get("janus", k, &mut out).unwrap();
                assert_eq!(n.map(|n| &out[..n]), Some(&v[..]), "round {round} key {k}");
            }
        }
    }

    /// W2: a reader that has cached a namespace answers every get exactly
    /// as a reader opened fresh on the same flash, across writes, removals
    /// and namespaces created after the cache was filled.
    #[test]
    fn the_namespace_cache_answers_as_a_fresh_reader_would() {
        let mut nvs = Nvs::open(RamFlash::blank(3)).unwrap();
        nvs.put_blob("janus", "a", b"one").unwrap();
        let keys = [
            ("janus", "a"),
            ("janus", "b"),
            ("other", "a"),
            ("third", "z"),
        ];
        let check = |nvs: &mut Nvs<RamFlash>| {
            let mut fresh = Nvs::open(RamFlash(nvs.flash.0.clone())).unwrap();
            for (ns, key) in keys {
                assert_eq!(get(nvs, ns, key), get(&mut fresh, ns, key), "{ns}/{key}");
            }
        };
        check(&mut nvs);
        nvs.put_blob("other", "a", b"two").unwrap();
        check(&mut nvs);
        nvs.put_blob("janus", "b", &[7; 300]).unwrap();
        check(&mut nvs);
        assert!(nvs.remove("janus", "a").unwrap());
        check(&mut nvs);
        nvs.put_blob("third", "z", b"three").unwrap();
        check(&mut nvs);
        // the same namespace asked again and again, with no write between
        for _ in 0..3 {
            assert_eq!(get(&mut nvs, "other", "a").unwrap(), b"two");
        }
    }

    #[test]
    fn reads_a_provisioning_image_key_for_key() {
        let mut nvs = Nvs::open(RamFlash::from_image(SETTINGS)).unwrap();
        assert_eq!(get(&mut nvs, "janus", "name").unwrap(), b"porch-cam");
        assert_eq!(get(&mut nvs, "janus", "wifi.ssid").unwrap(), b"census");
        assert_eq!(get(&mut nvs, "janus", "wifi.psk").unwrap(), b"census-pass");
        assert_eq!(get(&mut nvs, "janus", "maker").unwrap(), b"did:mata:test");
        assert_eq!(
            get(&mut nvs, "janus", "blink_ms").unwrap(),
            250u32.to_le_bytes()
        );
        assert_eq!(get(&mut nvs, "janus", "fps").unwrap(), [10u8]);
        assert_eq!(get(&mut nvs, "janus", "absent"), None);
        assert_eq!(get(&mut nvs, "other", "name"), None);
    }

    #[test]
    fn reads_the_identity_blob_and_refuses_a_short_buffer() {
        let mut nvs = Nvs::open(RamFlash::from_image(IDENTITY)).unwrap();
        let want: Vec<u8> = (0u8..32).collect();
        assert_eq!(get(&mut nvs, "janus", "mid.devkey").unwrap(), want);
        let mut short = [0u8; 16];
        assert_eq!(
            nvs.get("janus", "mid.devkey", &mut short),
            Err(Error::BufferTooSmall { needed: 32 })
        );
    }

    #[test]
    fn reads_a_blob_that_spans_pages_and_two_namespaces() {
        let mut nvs = Nvs::open(RamFlash::from_image(MIXED)).unwrap();
        assert_eq!(get(&mut nvs, "ns1", "small").unwrap(), [7u8]);
        let big: Vec<u8> = (0..5000u32).map(|i| (i * 7 % 251) as u8).collect();
        assert_eq!(get(&mut nvs, "ns1", "big").unwrap(), big);
        assert_eq!(get(&mut nvs, "ns2", "str").unwrap(), b"hello");
        assert_eq!(
            get(&mut nvs, "ns1", "after").unwrap(),
            (-5i32).to_le_bytes()
        );
    }

    #[test]
    fn writes_the_identity_blob_byte_for_byte_as_the_reference_does() {
        let mut nvs = Nvs::open(RamFlash::blank(3)).unwrap();
        let secret: Vec<u8> = (0u8..32).collect();
        nvs.put_blob("janus", "mid.devkey", &secret).unwrap();
        assert_eq!(nvs.into_flash().0, IDENTITY);
    }

    #[test]
    fn put_get_replace_remove() {
        let mut nvs = Nvs::open(RamFlash::blank(3)).unwrap();
        assert_eq!(get(&mut nvs, "janus", "mid.devkey"), None);
        nvs.put_blob("janus", "mid.devkey", &[1u8; 32]).unwrap();
        assert_eq!(get(&mut nvs, "janus", "mid.devkey").unwrap(), [1u8; 32]);
        nvs.put_blob("janus", "mid.devkey", &[2u8; 100]).unwrap();
        assert_eq!(get(&mut nvs, "janus", "mid.devkey").unwrap(), [2u8; 100]);
        assert!(nvs.remove("janus", "mid.devkey").unwrap());
        assert!(!nvs.remove("janus", "mid.devkey").unwrap());
        assert_eq!(get(&mut nvs, "janus", "mid.devkey"), None);
        nvs.put_blob("janus", "mid.devkey", &[3u8; 32]).unwrap();
        assert_eq!(get(&mut nvs, "janus", "mid.devkey").unwrap(), [3u8; 32]);
        // reopening reads the same
        let mut again = Nvs::open(nvs.into_flash()).unwrap();
        assert_eq!(get(&mut again, "janus", "mid.devkey").unwrap(), [3u8; 32]);
    }

    #[test]
    fn a_blob_larger_than_a_page_and_the_reserved_page() {
        let mut nvs = Nvs::open(RamFlash::blank(4)).unwrap();
        let big: Vec<u8> = (0..5000u32).map(|i| (i % 253) as u8).collect();
        nvs.put_blob("ns", "big", &big).unwrap();
        assert_eq!(get(&mut nvs, "ns", "big").unwrap(), big);
        // the blob fills page 0 and part of page 1; `more` fills page 1 and
        // starts page 2; `over` fits in page 2; `over2` would need page 3,
        // the one kept free, and is refused
        let more: Vec<u8> = vec![9u8; 3000];
        nvs.put_blob("ns", "more", &more).unwrap();
        assert_eq!(get(&mut nvs, "ns", "more").unwrap(), more);
        nvs.put_blob("ns", "over", &[1u8; 3000]).unwrap();
        assert_eq!(
            nvs.put_blob("ns", "over2", &[1u8; 3000]),
            Err(Error::BufferTooSmall { needed: 0 })
        );
        // what was written is still readable after the refusal
        assert_eq!(get(&mut nvs, "ns", "big").unwrap(), big);
        assert_eq!(get(&mut nvs, "ns", "more").unwrap(), more);
        assert_eq!(get(&mut nvs, "ns", "over").unwrap(), [1u8; 3000]);
        assert_eq!(get(&mut nvs, "ns", "over2"), None);
    }

    #[test]
    fn a_damaged_entry_is_corrupt_not_wrong() {
        let mut flash = RamFlash::from_image(IDENTITY);
        // page 0: the namespace entry, the chunk header, then the data entry
        let data = 64 + 2 * 32;
        flash.0[data + 3] ^= 0x01;
        let mut nvs = Nvs::open(flash).unwrap();
        let mut out = [0u8; 32];
        assert_eq!(
            nvs.get("janus", "mid.devkey", &mut out),
            Err(Error::Corrupt)
        );
    }

    /// W17: a damaged header is `Corrupt` on every get, before and after
    /// other gets have verified the headers in front of it.
    #[test]
    fn a_damaged_header_stays_corrupt_across_cached_gets() {
        let mut nvs = Nvs::open(RamFlash::blank(3)).unwrap();
        nvs.put_blob("janus", "a", b"first").unwrap();
        nvs.put_blob("janus", "b", b"second").unwrap();
        let mut flash = nvs.into_flash();
        // the last header written is `b`'s index: damage its size field
        let last = (0..ENTRIES_PER_PAGE as usize)
            .rev()
            .map(|i| 64 + i * 32)
            .find(|&o| flash.0[o..o + 32].iter().any(|&x| x != 0xFF))
            .unwrap();
        flash.0[last + 24] ^= 0x01;
        let mut nvs = Nvs::open(flash).unwrap();
        let mut out = [0u8; 32];
        for _ in 0..3 {
            // `a` is found before the damaged header and reads fine
            assert_eq!(nvs.get("janus", "a", &mut out), Ok(Some(5)));
            assert_eq!(&out[..5], b"first");
            // `b`'s lookup reaches the damaged header every time
            assert_eq!(nvs.get("janus", "b", &mut out), Err(Error::Corrupt));
        }
    }

    #[test]
    fn the_kv_seam_over_a_namespace() {
        let mut kv = NvsKv::open(RamFlash::blank(3), "janus").unwrap();
        let mut out = [0u8; 32];
        assert_eq!(kv.get("mid.devkey", &mut out).unwrap(), None);
        kv.put("mid.devkey", &[5u8; 32]).unwrap();
        assert_eq!(kv.get("mid.devkey", &mut out).unwrap(), Some(32));
        assert_eq!(out, [5u8; 32]);
        assert!(kv.remove("mid.devkey").unwrap());
        assert_eq!(kv.get("bad key!", &mut out), Err(Error::InvalidFormat));
    }

    #[test]
    fn geometry_is_checked() {
        assert!(matches!(
            Nvs::open(RamFlash(vec![0xFF; 100])),
            Err(Error::InvalidGeometry)
        ));
        assert!(matches!(
            Nvs::open(RamFlash::blank(1)),
            Err(Error::InvalidGeometry)
        ));
    }
}
