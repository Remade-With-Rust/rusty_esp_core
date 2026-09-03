//! Monotonic device time.
//!
//! A chip has no wall clock until something (NTP, the hub, mID adoption)
//! gives it one. Every Janus timestamp is therefore **monotonic microseconds
//! since an arbitrary device epoch** — usually boot — carried as a plain `u64`
//! value. Mapping to wall time is the host's job, done once per session from
//! a (device, host) pair of readings.
//!
//! `u64` here is a value, not an atomic: 32-bit targets lack 64-bit atomics,
//! and nothing in this crate needs them.

/// Monotonic microseconds since the device epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Micros(pub u64);

impl Micros {
    /// The device epoch.
    pub const ZERO: Micros = Micros(0);

    /// Build from milliseconds.
    #[must_use]
    pub const fn from_millis(ms: u64) -> Self {
        Micros(ms.saturating_mul(1_000))
    }

    /// Build from whole seconds.
    #[must_use]
    pub const fn from_secs(s: u64) -> Self {
        Micros(s.saturating_mul(1_000_000))
    }

    /// Whole milliseconds, truncated.
    #[must_use]
    pub const fn as_millis(self) -> u64 {
        self.0 / 1_000
    }

    /// Microseconds elapsed since `earlier`, saturating at zero if `earlier`
    /// is in the future (a wrapped or reset counter never produces a huge
    /// bogus interval).
    #[must_use]
    pub const fn since(self, earlier: Micros) -> u64 {
        self.0.saturating_sub(earlier.0)
    }

    /// This instant plus `micros`, saturating.
    #[must_use]
    pub const fn add_micros(self, micros: u64) -> Micros {
        Micros(self.0.saturating_add(micros))
    }
}

/// The host's mapping from a device's monotonic clock to wall time, with
/// the error bound that came with it.
///
/// A device has no wall clock; the host measures the offset once per session
/// from a request/response exchange and carries the uncertainty along. The
/// rule the family keeps: a component that cannot answer must say so, so an
/// unmeasured mapping reports an **infinite** error (`None`), never zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WallOffset {
    /// `wall_us - device_us` at the moment of measurement (Unix micros minus
    /// device micros).
    offset_us: i64,
    /// Half the round trip of the measurement, in micros; `None` when never
    /// measured.
    error_us: Option<u64>,
    /// The device instant the measurement refers to.
    measured_at: Micros,
}

impl WallOffset {
    /// Never measured: every mapping is unknown and the error is infinite.
    pub const UNKNOWN: WallOffset = WallOffset {
        offset_us: 0,
        error_us: None,
        measured_at: Micros::ZERO,
    };

    /// One-shot from an exchange: the host sent at `host_send_us`, the
    /// device answered with its clock reading `device_us`, the host received
    /// at `host_recv_us` (both host times Unix micros). The device reading is
    /// placed at the middle of the round trip and the error is half of it.
    /// `InvalidFormat` when the receive time precedes the send time.
    pub fn from_exchange(
        host_send_us: u64,
        device_us: Micros,
        host_recv_us: u64,
    ) -> crate::error::Result<Self> {
        if host_recv_us < host_send_us {
            return Err(crate::error::Error::InvalidFormat);
        }
        let rtt = host_recv_us - host_send_us;
        let mid = host_send_us + rtt / 2;
        // Wide arithmetic: a clock reading near `u64::MAX` on either side is
        // garbage, not a reason to overflow. An offset outside `i64` is the
        // same garbage, reported as such.
        let offset = i128::from(mid) - i128::from(device_us.0);
        let offset_us = i64::try_from(offset).map_err(|_| crate::error::Error::InvalidFormat)?;
        Ok(WallOffset {
            offset_us,
            error_us: Some(rtt.div_ceil(2)),
            measured_at: device_us,
        })
    }

    /// True when a measurement exists.
    #[must_use]
    pub const fn is_known(&self) -> bool {
        self.error_us.is_some()
    }

    /// The error bound in micros; `None` is infinite.
    #[must_use]
    pub const fn error_us(&self) -> Option<u64> {
        self.error_us
    }

    /// The device instant the measurement refers to.
    #[must_use]
    pub const fn measured_at(&self) -> Micros {
        self.measured_at
    }

    /// Wall time (Unix micros) for `device`, or `None` when unmeasured.
    #[must_use]
    pub fn to_wall(&self, device: Micros) -> Option<u64> {
        self.error_us?;
        Some((device.0 as i64).saturating_add(self.offset_us).max(0) as u64)
    }

    /// The error bound at `device` allowing for clock drift of `ppm` parts
    /// per million since the measurement; `None` is infinite.
    #[must_use]
    pub fn error_at(&self, device: Micros, ppm: u32) -> Option<u64> {
        let base = self.error_us?;
        let age = device.since(self.measured_at);
        Some(base.saturating_add(
            age / 1_000_000 * u64::from(ppm) + (age % 1_000_000) * u64::from(ppm) / 1_000_000,
        ))
    }

    /// Whichever of two mappings carries the smaller error; a known one
    /// always beats an unknown one.
    #[must_use]
    pub fn better(self, other: WallOffset) -> WallOffset {
        match (self.error_us, other.error_us) {
            (Some(a), Some(b)) => {
                if b < a {
                    other
                } else {
                    self
                }
            }
            (Some(_), None) => self,
            (None, _) => other,
        }
    }
}

impl core::fmt::Display for Micros {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}us", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{Micros, WallOffset};

    #[test]
    fn an_unmeasured_offset_says_so() {
        let w = WallOffset::UNKNOWN;
        assert!(!w.is_known());
        assert_eq!(w.error_us(), None, "infinite, never zero");
        assert_eq!(w.to_wall(Micros(5)), None);
        assert_eq!(w.error_at(Micros(5), 20), None);
    }

    #[test]
    fn one_shot_exchange_places_the_device_reading_mid_round_trip() {
        // host sends at 1 000 000, device says 500, host receives at 1 000 040
        let w = WallOffset::from_exchange(1_000_000, Micros(500), 1_000_040).unwrap();
        assert_eq!(w.error_us(), Some(20));
        assert_eq!(w.to_wall(Micros(500)), Some(1_000_020));
        assert_eq!(w.to_wall(Micros(1_500)), Some(1_001_020));
        assert_eq!(w.measured_at(), Micros(500));
        // drift: 20 ppm over ten device seconds adds 200 us
        assert_eq!(w.error_at(Micros(10_000_500), 20), Some(220));
        assert!(WallOffset::from_exchange(10, Micros(0), 5).is_err());
        let odd = WallOffset::from_exchange(0, Micros(0), 3).unwrap();
        assert_eq!(odd.error_us(), Some(2), "half a round trip rounds up");
        let tight = WallOffset::from_exchange(0, Micros(0), 2).unwrap();
        assert_eq!(w.better(tight).error_us(), Some(1));
        assert_eq!(WallOffset::UNKNOWN.better(w), w);
        assert_eq!(w.better(WallOffset::UNKNOWN), w);
    }

    #[test]
    fn since_saturates_instead_of_wrapping() {
        let a = Micros::from_millis(5);
        let b = Micros::from_millis(7);
        assert_eq!(b.since(a), 2_000);
        assert_eq!(a.since(b), 0);
    }

    #[test]
    fn conversions_round_trip() {
        assert_eq!(Micros::from_secs(3).as_millis(), 3_000);
        assert_eq!(Micros::from_millis(u64::MAX).0, u64::MAX);
    }
}
