//! The three seams every Janus backend fills.
//!
//! A function package's `-esp` crate implements these over `esp-hal` (Track
//! B) or ESP-IDF (Track A); the host implements them over `std` for tests.
//! Nothing else in the family talks to a clock, an entropy source or
//! persistent storage except through these traits — that is what lets the
//! core crates run unchanged on a laptop, a Pi and a chip.

use crate::error::Result;
use crate::time::Micros;

/// A monotonic clock.
pub trait Clock {
    /// The current instant on the device's monotonic clock.
    fn now(&self) -> Micros;
}

/// A cryptographically secure entropy source — a hardware TRNG on the chip.
///
/// Implementations must never fall back to a deterministic generator without
/// returning an error: a predictable device key is a compromised device.
pub trait Rng {
    /// Fill `buf` with random bytes.
    fn fill(&mut self, buf: &mut [u8]) -> Result<()>;
}

/// Longest key accepted by [`Kv`], matching ESP-IDF NVS.
pub const MAX_KEY_LEN: usize = 15;

/// Small persistent key/value storage — NVS on the chip, a file on the host.
///
/// Keys are ASCII, at most [`MAX_KEY_LEN`] bytes. Values are opaque bytes.
/// Secrets (the device key, adoption grants) go here only when the backend
/// documents that the partition is encrypted; the seam does not encrypt.
pub trait Kv {
    /// Copy the value for `key` into `out`. Returns the value length, or
    /// `Ok(None)` when the key is absent. A too-small `out` returns
    /// [`crate::Error::BufferTooSmall`] with the length needed.
    fn get(&self, key: &str, out: &mut [u8]) -> Result<Option<usize>>;

    /// Store `value` under `key`, replacing any prior value.
    fn put(&mut self, key: &str, value: &[u8]) -> Result<()>;

    /// Remove `key`; returns whether it existed.
    fn remove(&mut self, key: &str) -> Result<bool>;
}

/// Reject a key the seam contract does not allow.
pub fn check_key(key: &str) -> Result<()> {
    let ok = !key.is_empty()
        && key.len() <= MAX_KEY_LEN
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(crate::Error::InvalidFormat)
    }
}

/// Host implementations for tests and tooling.
#[cfg(feature = "std")]
pub mod host {
    use super::{Clock, Kv, Rng, check_key};
    use crate::error::{Error, Result};
    use crate::time::Micros;
    use std::collections::BTreeMap;
    use std::time::Instant;

    /// A [`Clock`] over `std::time::Instant`, epoch at construction.
    #[derive(Debug, Clone)]
    pub struct SystemClock {
        epoch: Instant,
    }

    impl SystemClock {
        /// Start the device epoch now.
        #[must_use]
        pub fn new() -> Self {
            SystemClock {
                epoch: Instant::now(),
            }
        }
    }

    impl Default for SystemClock {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Clock for SystemClock {
        fn now(&self) -> Micros {
            let d = self.epoch.elapsed();
            Micros(u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
        }
    }

    /// An in-memory [`Kv`]; nothing persists past the process.
    #[derive(Debug, Default, Clone)]
    pub struct MemoryKv {
        map: BTreeMap<String, Vec<u8>>,
    }

    impl MemoryKv {
        /// Empty store.
        #[must_use]
        pub fn new() -> Self {
            Self::default()
        }

        /// Number of keys held.
        #[must_use]
        pub fn len(&self) -> usize {
            self.map.len()
        }

        /// True when no keys are held.
        #[must_use]
        pub fn is_empty(&self) -> bool {
            self.map.is_empty()
        }
    }

    impl Kv for MemoryKv {
        fn get(&self, key: &str, out: &mut [u8]) -> Result<Option<usize>> {
            check_key(key)?;
            let Some(value) = self.map.get(key) else {
                return Ok(None);
            };
            let Some(slot) = out.get_mut(..value.len()) else {
                return Err(Error::BufferTooSmall {
                    needed: value.len(),
                });
            };
            slot.copy_from_slice(value);
            Ok(Some(value.len()))
        }

        fn put(&mut self, key: &str, value: &[u8]) -> Result<()> {
            check_key(key)?;
            self.map.insert(key.to_owned(), value.to_vec());
            Ok(())
        }

        fn remove(&mut self, key: &str) -> Result<bool> {
            check_key(key)?;
            Ok(self.map.remove(key).is_some())
        }
    }

    /// A deterministic [`Rng`] for **tests only**. It is not secure and it
    /// says so in its name; the chip backends provide the real TRNG.
    #[derive(Debug, Clone)]
    pub struct InsecureTestRng(u64);

    impl InsecureTestRng {
        /// Seeded generator (xorshift64*).
        #[must_use]
        pub fn seeded(seed: u64) -> Self {
            InsecureTestRng(seed | 1)
        }
    }

    impl Rng for InsecureTestRng {
        fn fill(&mut self, buf: &mut [u8]) -> Result<()> {
            for b in buf {
                let mut x = self.0;
                x ^= x >> 12;
                x ^= x << 25;
                x ^= x >> 27;
                self.0 = x;
                *b = (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8;
            }
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn memory_kv_round_trip() {
            let mut kv = MemoryKv::new();
            assert_eq!(kv.get("k", &mut [0; 4]).unwrap(), None);
            kv.put("k", b"abc").unwrap();
            let mut out = [0u8; 2];
            assert_eq!(
                kv.get("k", &mut out),
                Err(Error::BufferTooSmall { needed: 3 })
            );
            let mut out = [0u8; 8];
            assert_eq!(kv.get("k", &mut out).unwrap(), Some(3));
            assert_eq!(&out[..3], b"abc");
            assert!(kv.remove("k").unwrap());
            assert!(!kv.remove("k").unwrap());
            assert_eq!(
                kv.put("this_key_is_far_too_long", b""),
                Err(Error::InvalidFormat)
            );
        }

        #[test]
        fn clock_is_monotonic() {
            let c = SystemClock::new();
            let a = c.now();
            let b = c.now();
            assert!(b >= a);
        }

        #[test]
        fn test_rng_is_deterministic_and_nonconstant() {
            let mut a = InsecureTestRng::seeded(7);
            let mut b = InsecureTestRng::seeded(7);
            let (mut x, mut y) = ([0u8; 16], [0u8; 16]);
            a.fill(&mut x).unwrap();
            b.fill(&mut y).unwrap();
            assert_eq!(x, y);
            assert!(x.iter().any(|&v| v != x[0]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::check_key;

    #[test]
    fn key_rules() {
        assert!(check_key("device.key").is_ok());
        assert!(check_key("").is_err());
        assert!(check_key("has space").is_err());
        assert!(check_key("0123456789abcde").is_ok());
        assert!(check_key("0123456789abcdef").is_err());
    }
}
