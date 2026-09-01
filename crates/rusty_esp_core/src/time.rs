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

impl core::fmt::Display for Micros {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}us", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::Micros;

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
