//! One small, `Copy`, allocation-free error for the whole family.
//!
//! Function packages add detail by wrapping this in their own error enums;
//! the boundary between packages stays `Error` so a video pipeline can carry
//! an image-capture fault to a mesh sink without a conversion chain.

/// The Janus family error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Error {
    /// The format, rate, geometry or feature is not supported here.
    Unsupported,
    /// A caller-supplied buffer is too small; `needed` is the size that
    /// would have succeeded.
    BufferTooSmall {
        /// Bytes required.
        needed: usize,
    },
    /// Width, height, stride, plane length or sample alignment do not agree.
    InvalidGeometry,
    /// A value violates the contract of the canonical encoding (bad tag,
    /// wrong version, overlong field).
    InvalidFormat,
    /// The peripheral, transport or storage reported a fault.
    Hardware,
    /// The operation did not complete in time.
    Timeout,
    /// The resource is held by someone else right now.
    Busy,
    /// A signature, key or proof failed to verify.
    Crypto,
    /// The caller holds no capability that permits this.
    Denied,
    /// Stored data failed integrity checks.
    Corrupt,
}

impl Error {
    /// A short stable identifier, suitable for logs and wire error codes.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Error::Unsupported => "unsupported",
            Error::BufferTooSmall { .. } => "buffer_too_small",
            Error::InvalidGeometry => "invalid_geometry",
            Error::InvalidFormat => "invalid_format",
            Error::Hardware => "hardware",
            Error::Timeout => "timeout",
            Error::Busy => "busy",
            Error::Crypto => "crypto",
            Error::Denied => "denied",
            Error::Corrupt => "corrupt",
        }
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::BufferTooSmall { needed } => write!(f, "buffer too small: need {needed} bytes"),
            other => f.write_str(other.code()),
        }
    }
}

impl core::error::Error for Error {}

/// Shorthand used throughout the family.
pub type Result<T, E = Error> = core::result::Result<T, E>;
