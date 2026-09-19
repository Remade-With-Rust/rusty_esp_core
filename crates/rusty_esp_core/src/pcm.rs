//! Borrowed PCM audio blocks.
//!
//! Audio moves through Janus as fixed-size **interleaved** blocks over
//! caller-owned memory, mirroring what an I2S/PDM DMA ring delivers. A
//! [`PcmBlock`] is validated on construction so that its length is a whole
//! number of sample frames.

use crate::error::{Error, Result};
use crate::time::Micros;

/// A byte buffer viewed as `&[i16]`, or `None` when it cannot be.
///
/// Interleaved 16-bit PCM arrives as bytes -- that is what a DMA ring hands
/// over and what [`PcmBlock`] carries -- but every kernel that reads it wants
/// samples. Reassembling each one with `i16::from_le_bytes([b[0], b[1]])`
/// costs two byte loads, a shift and an or on a 32-bit core, because a
/// halfword load needs 2-byte alignment the compiler cannot prove a `&[u8]`
/// has. Measured on an ESP32-S3 that is **42-59%** of the audio element
/// kernels; `StereoToMono` spent 56 of its 72 loop instructions on it.
///
/// `None` means "take the byte path", and a caller must always have one:
///
/// - the slice is not 2-byte aligned, or its length is odd;
/// - the target is big-endian, where the in-memory order of an `i16` is not
///   the little-endian order the wire format defines.
///
/// In practice a heap or DMA buffer is aligned and this returns `Some`, but
/// the byte path stays as the oracle and the fallback, exactly as a scalar
/// kernel stays the oracle for its vector twin.
#[allow(unsafe_code)]
#[must_use]
pub fn as_i16(bytes: &[u8]) -> Option<&[i16]> {
    if cfg!(target_endian = "big") {
        return None;
    }
    // SAFETY: `i16` is plain old data -- no padding, no niches, no invalid
    // bit patterns and no `Drop` -- so viewing initialised bytes as `i16` is
    // sound once the alignment is right, which is what `align_to` finds. The
    // view is taken ONLY when both the prefix and the suffix it had to split
    // off are empty, i.e. the buffer was already aligned and its length is
    // even, so the returned slice covers exactly the bytes passed in and
    // nothing is skipped or invented. This is what `bytemuck::try_cast_slice`
    // does; see `lib.rs` for why it is spelled out rather than imported.
    let (prefix, mid, suffix) = unsafe { bytes.align_to::<i16>() };
    if prefix.is_empty() && suffix.is_empty() {
        Some(mid)
    } else {
        None
    }
}

/// [`as_i16`] for a buffer being written.
#[allow(unsafe_code)]
#[must_use]
pub fn as_i16_mut(bytes: &mut [u8]) -> Option<&mut [i16]> {
    if cfg!(target_endian = "big") {
        return None;
    }
    // SAFETY: as in `as_i16`, and the `&mut` is exclusive for the lifetime of
    // the returned slice because it is reborrowed from the caller's.
    let (prefix, mid, suffix) = unsafe { bytes.align_to_mut::<i16>() };
    if prefix.is_empty() && suffix.is_empty() {
        Some(mid)
    } else {
        None
    }
}

/// Round an `f32` to the nearest `i16`, ties away from zero, saturating.
///
/// Every PCM element that works in floats ends on this: `DcBlock`, `Biquad`,
/// `Agc` and the `f32 -> i16` conversion all call it once per sample, so it
/// is the single most-executed numeric primitive in the audio path.
///
/// `libm::roundf(x)` IS `truncf(x + copysignf(0.5 - 0.25*EPSILON, x))`, and a
/// cast to an integer truncates as well -- so the obvious spelling truncates
/// TWICE, once in the float unit and again in the cast. Adding the bias and
/// converting once is the same `i16` for every one of the 2^32 patterns.
///
/// The final conversion is `to_int_unchecked`, and the three tests above it
/// are what make that sound: a plain `as i16` is a SATURATING cast that emits
/// its own range and NaN tests, and a census of the flashed ELF found those
/// still being emitted after the explicit tests had already proved the range
/// -- four float compares per sample where two suffice.
#[allow(unsafe_code)]
#[must_use]
pub fn round_sat_i16(v: f32) -> i16 {
    let t = v + libm_copysignf(0.5 - 0.25 * f32::EPSILON, v);
    if t >= 32767.0 {
        return i16::MAX;
    }
    if t <= -32768.0 {
        return i16::MIN;
    }
    if t.is_nan() {
        // what the saturating cast does, and what the old form did by
        // falling through both comparisons
        return 0;
    }
    // SAFETY: the three tests above leave `t` strictly inside
    // (-32768.0, 32767.0) and not NaN, so its truncation toward zero is an
    // `i16`, which is exactly `to_int_unchecked`'s requirement.
    unsafe { t.to_int_unchecked::<i16>() }
}

/// Round an `f32` to the nearest integer, TIES TO EVEN, and saturate to
/// `i16`. The sibling of [`round_sat_i16`], which breaks ties away from zero.
///
/// Both roundings are needed and they are not interchangeable: sample-format
/// conversion follows `rintf`, i.e. the hardware default of ties-to-even,
/// while the element chain rounds half away from zero. Keeping them as two
/// named functions in one place is what stops the wrong one being reached for.
///
/// The rounding itself is `(v + 1.5*2^23) - 1.5*2^23`: for `|v| < 2^22` that
/// addition lands in `[2^23, 2^24)` where one ulp is exactly 1.0, so it
/// rounds to the nearest integer and the subtraction gives that integer back
/// — two additions where `rintf` is a call.
///
/// As in [`round_sat_i16`], the final conversion is `to_int_unchecked`: the
/// range test above it is what earns that, and a plain `as i16` would emit
/// its own range and NaN tests on top of the ones already done.
#[allow(unsafe_code)]
#[must_use]
pub fn rint_sat_i16(v: f32) -> i16 {
    /// `1.5 * 2^23`.
    const ROUND_F32: f32 = 12_582_912.0;
    // The in-range case first: it is what audio actually hits. NaN fails both
    // comparisons and falls through to the explicit test below.
    if v > -32768.0 && v < 32767.0 {
        let r = (v + ROUND_F32) - ROUND_F32;
        // SAFETY: `v` is strictly inside (-32768, 32767) and not NaN, and the
        // two additions above return the nearest INTEGER to it -- which is
        // therefore in [-32768, 32767] and exactly representable as `i16`.
        return unsafe { r.to_int_unchecked::<i16>() };
    }
    if v.is_nan() {
        // `rintf(NaN) as i16` is 0; say so rather than rely on the cast.
        0
    } else if v >= 32767.0 {
        i16::MAX
    } else {
        i16::MIN
    }
}

/// `copysignf` without a dependency: the sign of `y` on the magnitude of `x`.
#[inline]
fn libm_copysignf(x: f32, y: f32) -> f32 {
    f32::from_bits((x.to_bits() & 0x7fff_ffff) | (y.to_bits() & 0x8000_0000))
}

/// [`as_i16`] for unsigned 16-bit words: packed RGB565 pixels, and anything
/// else that is a `u16` living in a byte buffer.
///
/// Same contract and the same reasons to refuse. A camera hands over RGB565
/// as bytes, so every pixel a converter or a downscaler reads is two byte
/// loads, a shift and an or, and every one it writes is two byte stores.
#[allow(unsafe_code)]
#[must_use]
pub fn as_u16(bytes: &[u8]) -> Option<&[u16]> {
    if cfg!(target_endian = "big") {
        return None;
    }
    // SAFETY: as in `as_i16`; `u16` is plain old data with no invalid bit
    // patterns, and the view is taken only when nothing was split off either
    // end, so it covers exactly the bytes passed in.
    let (prefix, mid, suffix) = unsafe { bytes.align_to::<u16>() };
    if prefix.is_empty() && suffix.is_empty() {
        Some(mid)
    } else {
        None
    }
}

/// [`as_u16`] for a buffer being written.
#[allow(unsafe_code)]
#[must_use]
pub fn as_u16_mut(bytes: &mut [u8]) -> Option<&mut [u16]> {
    if cfg!(target_endian = "big") {
        return None;
    }
    // SAFETY: as in `as_u16`, and the `&mut` is exclusive for the lifetime of
    // the returned slice because it is reborrowed from the caller's.
    let (prefix, mid, suffix) = unsafe { bytes.align_to_mut::<u16>() };
    if prefix.is_empty() && suffix.is_empty() {
        Some(mid)
    } else {
        None
    }
}

/// [`as_i16`] for 32-bit floats: the other half of every PCM conversion.
///
/// Same contract, same reasons to refuse, and the same 4-byte-per-sample
/// marshalling to avoid -- an `f32` reassembled from a `&[u8]` is FOUR byte
/// loads plus three shifts and three ors.
#[allow(unsafe_code)]
#[must_use]
pub fn as_f32(bytes: &[u8]) -> Option<&[f32]> {
    if cfg!(target_endian = "big") {
        return None;
    }
    // SAFETY: as in `as_i16`. `f32` has no invalid bit patterns -- every
    // 32-bit word is some float, signalling NaNs included -- so viewing
    // initialised, correctly-aligned bytes as `f32` is sound. The view is
    // taken only when nothing was split off either end.
    let (prefix, mid, suffix) = unsafe { bytes.align_to::<f32>() };
    if prefix.is_empty() && suffix.is_empty() {
        Some(mid)
    } else {
        None
    }
}

/// [`as_f32`] for a buffer being written.
#[allow(unsafe_code)]
#[must_use]
pub fn as_f32_mut(bytes: &mut [u8]) -> Option<&mut [f32]> {
    if cfg!(target_endian = "big") {
        return None;
    }
    // SAFETY: as in `as_f32`, and the `&mut` is exclusive for the lifetime of
    // the returned slice because it is reborrowed from the caller's.
    let (prefix, mid, suffix) = unsafe { bytes.align_to_mut::<f32>() };
    if prefix.is_empty() && suffix.is_empty() {
        Some(mid)
    } else {
        None
    }
}

/// Sample encoding of one channel value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SampleFormat {
    /// Signed 16-bit little-endian.
    I16,
    /// Signed 24-bit in the top bits of a 32-bit little-endian word (what
    /// most I2S codecs and PDM filters produce).
    I24In32,
    /// Signed 32-bit little-endian.
    I32,
    /// IEEE 754 binary32 little-endian.
    F32,
}

impl SampleFormat {
    /// Bytes per single-channel sample.
    #[must_use]
    pub const fn bytes(self) -> usize {
        match self {
            SampleFormat::I16 => 2,
            SampleFormat::I24In32 | SampleFormat::I32 | SampleFormat::F32 => 4,
        }
    }

    /// Stable wire tag.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            SampleFormat::I16 => "s16le",
            SampleFormat::I24In32 => "s24le32",
            SampleFormat::I32 => "s32le",
            SampleFormat::F32 => "f32le",
        }
    }
}

/// Rate, channel count and sample encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcmFormat {
    /// Frames per second.
    pub sample_rate_hz: u32,
    /// Interleaved channels per frame (1 = mono, 2 = stereo).
    pub channels: u8,
    /// Encoding of each channel value.
    pub sample: SampleFormat,
}

impl PcmFormat {
    /// Build a format, rejecting a zero rate or zero channels.
    pub const fn new(sample_rate_hz: u32, channels: u8, sample: SampleFormat) -> Result<Self> {
        if sample_rate_hz == 0 || channels == 0 {
            return Err(Error::InvalidGeometry);
        }
        Ok(PcmFormat {
            sample_rate_hz,
            channels,
            sample,
        })
    }

    /// 16 kHz mono 16-bit — the voice default.
    pub const PCM16_16K_MONO: PcmFormat = PcmFormat {
        sample_rate_hz: 16_000,
        channels: 1,
        sample: SampleFormat::I16,
    };

    /// 48 kHz stereo 16-bit — the media default.
    pub const PCM16_48K_STEREO: PcmFormat = PcmFormat {
        sample_rate_hz: 48_000,
        channels: 2,
        sample: SampleFormat::I16,
    };

    /// Bytes per sample frame (all channels).
    #[must_use]
    pub const fn frame_bytes(&self) -> usize {
        self.sample.bytes() * self.channels as usize
    }

    /// Bytes needed to hold `micros` of audio, rounded down to a whole frame.
    #[must_use]
    pub const fn bytes_for_micros(&self, micros: u64) -> usize {
        let frames = (self.sample_rate_hz as u64) * micros / 1_000_000;
        (frames as usize) * self.frame_bytes()
    }

    /// Duration in microseconds of `frames` sample frames.
    #[must_use]
    pub const fn micros_for_frames(&self, frames: usize) -> u64 {
        (frames as u64) * 1_000_000 / (self.sample_rate_hz as u64)
    }
}

/// A borrowed block of interleaved PCM.
#[derive(Debug, Clone, Copy)]
pub struct PcmBlock<'a> {
    /// Rate, channels, encoding.
    pub format: PcmFormat,
    /// Capture instant of the first frame on the device's monotonic clock.
    pub timestamp: Micros,
    /// Interleaved sample bytes; a whole number of frames.
    pub data: &'a [u8],
}

impl<'a> PcmBlock<'a> {
    /// A block over `data`, which must be non-empty and frame-aligned.
    pub fn new(format: PcmFormat, timestamp: Micros, data: &'a [u8]) -> Result<Self> {
        let fb = format.frame_bytes();
        if data.is_empty() || data.len() % fb != 0 {
            return Err(Error::InvalidGeometry);
        }
        Ok(PcmBlock {
            format,
            timestamp,
            data,
        })
    }

    /// Sample frames in this block.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.data.len() / self.format.frame_bytes()
    }

    /// Duration of this block in microseconds.
    #[must_use]
    pub fn duration_micros(&self) -> u64 {
        self.format.micros_for_frames(self.frames())
    }

    /// Timestamp of the frame that would follow this block.
    #[must_use]
    pub fn end(&self) -> Micros {
        self.timestamp.add_micros(self.duration_micros())
    }

    /// Iterate the samples of an [`SampleFormat::I16`] block, interleaved.
    /// Returns `None` for any other encoding.
    #[must_use]
    pub fn samples_i16(&self) -> Option<impl Iterator<Item = i16> + 'a> {
        if self.format.sample != SampleFormat::I16 {
            return None;
        }
        Some(
            self.data
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]])),
        )
    }
}

#[cfg(test)]
mod i16_view {
    use super::{as_i16, as_i16_mut};

    /// The view must agree with the byte path it replaces, for every value,
    /// and must REFUSE every case where it would not.
    #[test]
    // `vec!`, not an array, deliberately: the alignment a HEAP buffer has is
    // the alignment real PCM has, and a stack array's is not the same
    // question. clippy cannot see that the allocation is the point.
    #[allow(clippy::useless_vec)]
    fn agrees_with_from_le_bytes_and_refuses_the_rest() {
        // one extra byte at the front so a deliberately misaligned view is
        // available from the same allocation
        let mut raw = vec![0u8; 1 + 64];
        for (i, b) in raw.iter_mut().enumerate() {
            *b = (i.wrapping_mul(97) ^ (i >> 3)) as u8;
        }
        let buf = &raw[1..]; // 64 bytes, alignment unknown but length even

        // a refusal is always allowed here; the caller has a byte path
        if let Some(v) = as_i16(buf) {
            assert_eq!(v.len(), buf.len() / 2);
            for (k, &s) in v.iter().enumerate() {
                assert_eq!(
                    s,
                    i16::from_le_bytes([buf[k * 2], buf[k * 2 + 1]]),
                    "sample {k} disagrees with the byte path"
                );
            }
        }

        // odd length is never viewable
        assert!(as_i16(&raw[1..64]).is_none(), "odd length must refuse");
        assert!(as_i16(&raw[..1]).is_none(), "one byte must refuse");
        assert!(as_i16(&[]).is_some_and(<[i16]>::is_empty) || as_i16(&[]).is_none());

        // a Vec<u16>'s bytes ARE aligned, so that case must succeed and the
        // values must round-trip -- this is the path the elements will take
        let words: Vec<u16> = (0..32).map(|i| (i * 2477) as u16).collect();
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let aligned = as_i16(&bytes).expect("a fresh Vec<u8> is 2-byte aligned here");
        for (k, &s) in aligned.iter().enumerate() {
            assert_eq!(s as u16, words[k], "word {k}");
        }
    }

    /// Writing through the view must land the same bytes the byte path would.
    #[test]
    fn writes_land_where_the_byte_path_would_put_them() {
        let vals: [i16; 8] = [0, -1, 1, i16::MIN, i16::MAX, -12345, 30000, -30000];

        let mut via_view = vec![0u8; 16];
        let mut ok = false;
        if let Some(v) = as_i16_mut(&mut via_view) {
            v.copy_from_slice(&vals);
            ok = true;
        }

        let mut via_bytes = vec![0u8; 16];
        for (k, &x) in vals.iter().enumerate() {
            via_bytes[k * 2..k * 2 + 2].copy_from_slice(&x.to_le_bytes());
        }

        if ok {
            assert_eq!(via_view, via_bytes, "the two write paths disagree");
        }
        assert!(as_i16_mut(&mut via_view[..15]).is_none(), "odd length");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_arithmetic() {
        let f = PcmFormat::PCM16_16K_MONO;
        assert_eq!(f.frame_bytes(), 2);
        assert_eq!(f.bytes_for_micros(20_000), 640);
        assert_eq!(f.micros_for_frames(320), 20_000);
        let s = PcmFormat::PCM16_48K_STEREO;
        assert_eq!(s.frame_bytes(), 4);
    }

    #[test]
    fn block_must_be_frame_aligned() {
        let f = PcmFormat::PCM16_48K_STEREO;
        assert_eq!(
            PcmBlock::new(f, Micros::ZERO, &[0; 6]).err(),
            Some(Error::InvalidGeometry)
        );
        assert_eq!(
            PcmBlock::new(f, Micros::ZERO, &[]).err(),
            Some(Error::InvalidGeometry)
        );
        let b = PcmBlock::new(f, Micros::from_millis(1), &[0; 8]).unwrap();
        assert_eq!(b.frames(), 2);
        assert_eq!(b.end().0, 1_000 + 41);
    }

    #[test]
    fn i16_iteration() {
        let f = PcmFormat::PCM16_16K_MONO;
        let bytes = [0x01, 0x00, 0xFF, 0xFF];
        let b = PcmBlock::new(f, Micros::ZERO, &bytes).unwrap();
        let v: [i16; 2] = {
            let mut it = b.samples_i16().unwrap();
            [it.next().unwrap(), it.next().unwrap()]
        };
        assert_eq!(v, [1, -1]);
        let f32 = PcmFormat::new(16_000, 1, SampleFormat::F32).unwrap();
        assert!(
            PcmBlock::new(f32, Micros::ZERO, &[0; 4])
                .unwrap()
                .samples_i16()
                .is_none()
        );
    }
}
