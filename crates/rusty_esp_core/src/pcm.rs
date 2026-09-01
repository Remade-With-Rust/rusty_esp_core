//! Borrowed PCM audio blocks.
//!
//! Audio moves through Janus as fixed-size **interleaved** blocks over
//! caller-owned memory, mirroring what an I2S/PDM DMA ring delivers. A
//! [`PcmBlock`] is validated on construction so that its length is a whole
//! number of sample frames.

use crate::error::{Error, Result};
use crate::time::Micros;

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
