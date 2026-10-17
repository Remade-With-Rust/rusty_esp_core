//! Secure boot v2's signature sector, checked by the ROM's own verifier,
//! the routine the ROM runs on the bootloader itself once the eFuse is
//! burnt. Whose keys are trusted is the policy:
//!
//! - `SECURE_BOOT_EN` set in the eFuse: the eFuse key digests (the real
//!   thing, the sacrificial board's case; not provable on the bench);
//! - else a digest compiled in through `JANUS_BOOT_KEY_DIGEST` (64 hex
//!   digits, `espsecure.py digest_sbv2_public_key` of the maker's key):
//!   the same check in software, for a board whose eFuse is untouched;
//! - else open: no signature is required and the sector is ignored.
//!
//! The digest the signature covers is SHA-256 of the image padded with
//! `0xFF` to a sector, the sector after it holds up to three 1216-byte
//! blocks; `espino-sign` writes both.

use crate::{rom, sha::Sha};

pub enum Policy {
    Open,
    Compiled(&'static [u8; 32]),
    Efuse,
}

const COMPILED: Option<[u8; 32]> = parse_digest(option_env!("JANUS_BOOT_KEY_DIGEST"));
static COMPILED_DIGEST: Option<[u8; 32]> = COMPILED;

const fn nibble(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => panic!("JANUS_BOOT_KEY_DIGEST: not a hex digit"),
    }
}

const fn parse_digest(s: Option<&str>) -> Option<[u8; 32]> {
    match s {
        None => None,
        Some(s) => {
            let b = s.as_bytes();
            if b.len() != 64 {
                panic!("JANUS_BOOT_KEY_DIGEST must be 64 hex digits");
            }
            let mut out = [0u8; 32];
            let mut i = 0;
            while i < 32 {
                out[i] = (nibble(b[2 * i]) << 4) | nibble(b[2 * i + 1]);
                i += 1;
            }
            Some(out)
        }
    }
}

pub fn policy() -> Policy {
    // SAFETY: a read of a read-only eFuse shadow register.
    let efuse = unsafe { &*esp32s3::EFUSE::ptr() };
    if efuse.rd_repeat_data2().read().secure_boot_en().bit_is_set() {
        return Policy::Efuse;
    }
    match &COMPILED_DIGEST {
        Some(d) => Policy::Compiled(d),
        None => Policy::Open,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// No signature sector after the image (or no room for one).
    NoSignature,
    /// The eFuse holds no usable key digest.
    NoTrustedKeys,
    /// The ROM's verdict, other than `SB_SUCCESS`.
    Rejected(u32),
    /// The ROM said yes but the digest it recovered is not the image's.
    DigestMismatch,
}

/// Hashes the image padded to a sector, takes the sector after it and asks
/// the ROM whether one of its blocks signs that digest with a trusted key.
/// `view` is the mapped partition, `image_len` the image through its
/// appended hash.
pub fn verify(view: &[u8], image_len: u32, policy: &Policy, sha: &mut Sha) -> Result<(), Refusal> {
    let padded = ((image_len as usize) + 4095) & !4095;
    let Some(sig) = view.get(padded..padded + 4096) else {
        return Err(Refusal::NoSignature);
    };
    if sig[0] != 0xE7 {
        return Err(Refusal::NoSignature);
    }
    sha.start();
    sha.update(&view[..padded]);
    let digest = sha.finish();

    let mut trusted = rom::KeyDigests { key_digests: [core::ptr::null(); 3], allow_key_revoke: false };
    match policy {
        Policy::Open => return Ok(()),
        Policy::Compiled(d) => trusted.key_digests[0] = d.as_ptr(),
        // SAFETY: a ROM routine filling the struct it is given.
        Policy::Efuse => {
            if unsafe { rom::ets_secure_boot_read_key_digests(&mut trusted) } != rom::ETS_OK {
                return Err(Refusal::NoTrustedKeys);
            }
        }
    }
    let mut verified = [0u8; 32];
    // SAFETY: the sector (word-aligned: the partition is, and `padded` is a
    // multiple of 4096), the digest, the key list and the output are all
    // valid for the ROM's reads and writes.
    let verdict = unsafe {
        rom::ets_secure_boot_verify_signature(
            sig.as_ptr() as *const u32,
            digest.as_ptr(),
            &trusted,
            verified.as_mut_ptr(),
        )
    };
    if verdict != rom::SB_SUCCESS {
        return Err(Refusal::Rejected(verdict));
    }
    if verified != digest {
        return Err(Refusal::DigestMismatch);
    }
    Ok(())
}
