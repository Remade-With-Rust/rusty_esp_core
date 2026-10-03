//! The partition table at `0x8000` and `otadata`, read as ESP-IDF's loader
//! reads them (`bootloader_utility_get_selected_boot_partition`), with its
//! selection and rollback rule:
//!
//! - an entry left `PendingVerify` (an image that booted once and never
//!   marked itself valid) becomes `Aborted` before anything else;
//! - with both entries unusable, the factory app, else `ota_0`; and when
//!   both are still blank (a freshly flashed board) the slot that boots
//!   gets `otadata[0]` written as `Valid` for it, so the app finds itself
//!   there (`set_actual_ota_seq`);
//! - else the usable entry with the higher sequence picks
//!   `ota_((seq - 1) % count)`, and a `New` image is marked `PendingVerify`
//!   for its one try.

use core::fmt;

use crate::{flash, rom};

pub const TABLE_OFFSET: u32 = 0x8000;
pub const TABLE_WORDS: usize = 0xC00 / 4;
const ENTRY_MAGIC: u16 = 0x50AA;
const MD5_MAGIC: u16 = 0xEBEB;
const TYPE_APP: u8 = 0;
const TYPE_DATA: u8 = 1;
const SUBTYPE_FACTORY: u8 = 0;
const SUBTYPE_OTA_FLAG: u8 = 0x10;
const SUBTYPE_DATA_OTA: u8 = 0;
pub const MAX_OTA: usize = 16;

pub const STATE_NEW: u32 = 0;
pub const STATE_PENDING_VERIFY: u32 = 1;
pub const STATE_VALID: u32 = 2;
pub const STATE_INVALID: u32 = 3;
pub const STATE_ABORTED: u32 = 4;
pub const STATE_UNDEFINED: u32 = 0xFFFF_FFFF;

pub fn state_name(state: u32) -> &'static str {
    match state {
        STATE_NEW => "New",
        STATE_PENDING_VERIFY => "PendingVerify",
        STATE_VALID => "Valid",
        STATE_INVALID => "Invalid",
        STATE_ABORTED => "Aborted",
        STATE_UNDEFINED => "Undefined",
        _ => "?",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Part {
    pub offset: u32,
    pub len: u32,
}

pub struct Table {
    pub otadata: Option<Part>,
    pub factory: Option<Part>,
    pub ota: [Option<Part>; MAX_OTA],
    pub ota_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Flash(flash::Fault),
    /// An entry without the magic before the table's end.
    Corrupt { index: usize, magic: u16 },
    Empty,
    OtadataTooSmall(u32),
}

impl From<flash::Fault> for Error {
    fn from(f: flash::Fault) -> Self {
        Error::Flash(f)
    }
}

pub fn read(buf: &mut [u32; TABLE_WORDS]) -> Result<Table, Error> {
    flash::read(TABLE_OFFSET, buf)?;
    let mut t = Table { otadata: None, factory: None, ota: [None; MAX_OTA], ota_count: 0 };
    let mut n = 0;
    for (index, e) in flash::bytes(buf).chunks_exact(32).enumerate() {
        let magic = u16::from_le_bytes([e[0], e[1]]);
        if magic == MD5_MAGIC || magic == 0xFFFF {
            break;
        }
        if magic != ENTRY_MAGIC {
            return Err(Error::Corrupt { index, magic });
        }
        let p = Part { offset: flash::le32(&e[4..8]), len: flash::le32(&e[8..12]) };
        match (e[2], e[3]) {
            (TYPE_APP, SUBTYPE_FACTORY) => t.factory = Some(p),
            (TYPE_APP, s) if (SUBTYPE_OTA_FLAG..SUBTYPE_OTA_FLAG + MAX_OTA as u8).contains(&s) => {
                t.ota[(s - SUBTYPE_OTA_FLAG) as usize] = Some(p);
                t.ota_count += 1;
            }
            (TYPE_DATA, SUBTYPE_DATA_OTA) => t.otadata = Some(p),
            _ => {}
        }
        n += 1;
    }
    if n == 0 {
        return Err(Error::Empty);
    }
    Ok(t)
}

/// One `esp_ota_select_entry_t`: sequence, 20-byte label, state, CRC of the
/// sequence word.
#[derive(Clone, Copy)]
struct Entry {
    raw: [u32; 8],
}

impl Entry {
    fn seq(&self) -> u32 {
        self.raw[0]
    }
    fn state(&self) -> u32 {
        self.raw[6]
    }
    fn crc_of(seq: u32) -> u32 {
        // SAFETY: a ROM routine over the four bytes of the word.
        unsafe { rom::crc32_le(0xFFFF_FFFF, (&seq as *const u32) as *const u8, 4) }
    }
    fn crc_ok(&self) -> bool {
        self.raw[7] == Self::crc_of(self.seq())
    }
    /// `bootloader_common_ota_select_invalid`.
    fn invalid(&self) -> bool {
        self.seq() == 0xFFFF_FFFF || self.state() == STATE_INVALID || self.state() == STATE_ABORTED
    }
    /// `bootloader_common_ota_select_valid`.
    fn valid(&self) -> bool {
        !self.invalid() && self.crc_ok()
    }
    /// Blank or unreadable: what a freshly flashed board has.
    fn initial(&self) -> bool {
        self.seq() == 0xFFFF_FFFF || !self.crc_ok()
    }
    /// A fresh entry for OTA slot `index`, `Valid`.
    fn fresh(index: usize) -> Self {
        let seq = index as u32 + 1;
        let mut raw = [0xFFFF_FFFFu32; 8];
        raw[0] = seq;
        raw[6] = STATE_VALID;
        raw[7] = Self::crc_of(seq);
        Entry { raw }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Factory,
    Ota(usize),
}

impl fmt::Display for Choice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Choice::Factory => f.write_str("factory"),
            Choice::Ota(i) => write!(f, "ota_{i}"),
        }
    }
}

pub struct Selection {
    pub choice: Choice,
    /// The chosen entry's state after the rule ran, `STATE_UNDEFINED` when
    /// no entry chose.
    pub state: u32,
    pub seq: u32,
    /// Both entries blank or unreadable: the slot that boots gets written
    /// (`ota_has_initial_contents`).
    pub initial: bool,
}

pub fn select(t: &Table) -> Result<Selection, Error> {
    let none = |choice, initial| Selection { choice, state: STATE_UNDEFINED, seq: 0, initial };
    let Some(od) = t.otadata else {
        return Ok(none(Choice::Factory, false));
    };
    if od.len < 2 * 4096 {
        return Err(Error::OtadataTooSmall(od.len));
    }
    let mut e = [Entry { raw: [0; 8] }, Entry { raw: [0; 8] }];
    for (i, entry) in e.iter_mut().enumerate() {
        flash::read(od.offset + 4096 * i as u32, &mut entry.raw)?;
    }

    // The rollback rule's second half: an image that got its try and never
    // confirmed itself is out.
    for (i, entry) in e.iter_mut().enumerate() {
        if entry.state() == STATE_PENDING_VERIFY {
            entry.raw[6] = STATE_ABORTED;
            say!("otadata[{i}] seq {} was PendingVerify: Aborted, rolling back", entry.seq());
            if let Err(err) = write_entry(od.offset + 4096 * i as u32, entry) {
                say!("otadata[{i}] write failed: {:?}", err);
            }
        }
    }

    if (e[0].invalid() && e[1].invalid()) || t.ota_count == 0 {
        return Ok(if t.factory.is_some() {
            none(Choice::Factory, false)
        } else {
            none(Choice::Ota(0), e[0].initial() && e[1].initial())
        });
    }
    let active = match (e[0].valid(), e[1].valid()) {
        (true, true) => usize::from(e[1].seq() > e[0].seq()),
        (true, false) => 0,
        (false, true) => 1,
        // "ota data partition invalid": the factory app, then every slot
        (false, false) => return Ok(none(Choice::Factory, false)),
    };
    let seq = e[active].seq();
    let index = (seq.wrapping_sub(1) % t.ota_count as u32) as usize;
    let mut state = e[active].state();
    // The rule's first half: a new image gets exactly one try.
    if state == STATE_NEW {
        e[active].raw[6] = STATE_PENDING_VERIFY;
        if let Err(err) = write_entry(od.offset + 4096 * active as u32, &e[active]) {
            say!("otadata[{active}] write failed: {:?}", err);
        } else {
            state = STATE_PENDING_VERIFY;
        }
    }
    Ok(Selection { choice: Choice::Ota(index), state, seq, initial: false })
}

/// `set_actual_ota_seq`: on a board whose `otadata` is blank, the OTA slot
/// about to boot is written into `otadata[0]` as `Valid`, so the app's
/// view of the slots matches the loader's.
pub fn write_initial(t: &Table, index: usize) {
    let Some(od) = t.otadata else { return };
    let entry = Entry::fresh(index);
    match write_entry(od.offset, &entry) {
        Ok(()) => say!("otadata[0] was blank: ota_{index} written as seq {} Valid", entry.seq()),
        Err(err) => say!("otadata[0] write failed: {:?}", err),
    }
}

fn write_entry(addr: u32, e: &Entry) -> Result<(), flash::Fault> {
    flash::unlock()?;
    flash::erase_sector(addr)?;
    flash::write(addr, &e.raw)
}
