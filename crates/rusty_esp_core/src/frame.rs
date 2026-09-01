//! Borrowed image and video frames.
//!
//! A [`Frame`] is a *view*: geometry, timestamp and up to three planes that
//! borrow memory the caller owns. Constructors validate that the memory is
//! large enough for the geometry, so a downstream consumer can index without
//! re-checking and without `unsafe`.

use crate::error::{Error, Result};
use crate::time::Micros;

/// Pixel layout of a frame.
///
/// Compressed formats ([`PixelFormat::Jpeg`]) have no fixed byte length; the
/// frame's packed plane holds the coded bytes. The first uncompressed formats
/// are the ones ESP camera sensors emit and ESP LCDs consume — including
/// `RGB565`, which the FFmpeg-shaped vocabulary lacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PixelFormat {
    /// JPEG/JFIF coded bytes (what OV2640/OV5640 deliver in JPEG mode).
    Jpeg,
    /// 8-bit luma only.
    Gray8,
    /// 16-bit packed RGB, 5-6-5, little-endian.
    Rgb565,
    /// 24-bit packed RGB.
    Rgb888,
    /// 24-bit packed BGR.
    Bgr888,
    /// 32-bit packed RGBA.
    Rgba8888,
    /// Packed 4:2:2, YUYV byte order (Y0 U Y1 V).
    Yuyv422,
    /// Planar 4:2:0 — three planes, chroma at half resolution each way.
    Yuv420p,
    /// Raw sensor data, 8 bits per photosite (Bayer or mono).
    Raw8,
}

impl PixelFormat {
    /// Bits per pixel for packed formats; `None` for compressed or planar.
    #[must_use]
    pub const fn packed_bits_per_pixel(self) -> Option<u32> {
        match self {
            PixelFormat::Gray8 | PixelFormat::Raw8 => Some(8),
            PixelFormat::Rgb565 | PixelFormat::Yuyv422 => Some(16),
            PixelFormat::Rgb888 | PixelFormat::Bgr888 => Some(24),
            PixelFormat::Rgba8888 => Some(32),
            PixelFormat::Jpeg | PixelFormat::Yuv420p => None,
        }
    }

    /// True for coded formats whose byte length is not a function of geometry.
    #[must_use]
    pub const fn is_compressed(self) -> bool {
        matches!(self, PixelFormat::Jpeg)
    }

    /// True for formats carried as separate planes.
    #[must_use]
    pub const fn is_planar(self) -> bool {
        matches!(self, PixelFormat::Yuv420p)
    }

    /// Stable wire tag.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            PixelFormat::Jpeg => "jpeg",
            PixelFormat::Gray8 => "gray8",
            PixelFormat::Rgb565 => "rgb565",
            PixelFormat::Rgb888 => "rgb888",
            PixelFormat::Bgr888 => "bgr888",
            PixelFormat::Rgba8888 => "rgba8888",
            PixelFormat::Yuyv422 => "yuyv422",
            PixelFormat::Yuv420p => "yuv420p",
            PixelFormat::Raw8 => "raw8",
        }
    }
}

/// Width, height and pixel format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Geometry {
    /// Pixels per row.
    pub width: u32,
    /// Rows.
    pub height: u32,
    /// Pixel layout.
    pub format: PixelFormat,
}

impl Geometry {
    /// Build a geometry, rejecting a zero dimension or an odd size where the
    /// format needs even dimensions (4:2:2 needs even width, 4:2:0 even both).
    pub const fn new(width: u32, height: u32, format: PixelFormat) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(Error::InvalidGeometry);
        }
        let even_w = width % 2 == 0;
        let even_h = height % 2 == 0;
        let ok = match format {
            PixelFormat::Yuyv422 => even_w,
            PixelFormat::Yuv420p => even_w && even_h,
            _ => true,
        };
        if ok {
            Ok(Geometry {
                width,
                height,
                format,
            })
        } else {
            Err(Error::InvalidGeometry)
        }
    }

    /// Byte length of a tightly packed frame in this geometry, or `None` for a
    /// compressed format. Overflow yields `None` too.
    #[must_use]
    pub const fn byte_len(&self) -> Option<usize> {
        let pixels = (self.width as u64) * (self.height as u64);
        let bytes = match self.format {
            PixelFormat::Yuv420p => pixels * 3 / 2,
            PixelFormat::Jpeg => return None,
            other => match other.packed_bits_per_pixel() {
                Some(bits) => pixels * (bits as u64) / 8,
                None => return None,
            },
        };
        if bytes > usize::MAX as u64 {
            None
        } else {
            Some(bytes as usize)
        }
    }

    /// Bytes per row of a tightly packed (non-planar) frame.
    #[must_use]
    pub const fn packed_stride(&self) -> Option<usize> {
        match self.format.packed_bits_per_pixel() {
            Some(bits) => Some((self.width as usize) * (bits as usize) / 8),
            None => None,
        }
    }
}

/// One plane of pixels: `data` holds at least `stride * rows` bytes.
#[derive(Debug, Clone, Copy)]
pub struct Plane<'a> {
    /// Pixel bytes, row-major, `stride` bytes per row.
    pub data: &'a [u8],
    /// Bytes from the start of one row to the start of the next.
    pub stride: usize,
}

impl<'a> Plane<'a> {
    /// A plane over `data` with `stride`, validated against `rows` and
    /// `row_bytes` (the used bytes per row, `<= stride`).
    pub fn new(data: &'a [u8], stride: usize, rows: usize, row_bytes: usize) -> Result<Self> {
        if row_bytes > stride || rows == 0 {
            return Err(Error::InvalidGeometry);
        }
        let needed = stride
            .checked_mul(rows - 1)
            .and_then(|n| n.checked_add(row_bytes))
            .ok_or(Error::InvalidGeometry)?;
        if data.len() < needed {
            return Err(Error::BufferTooSmall { needed });
        }
        Ok(Plane { data, stride })
    }

    /// Row `index` as a slice of exactly `stride` bytes, or `None` past the end.
    #[must_use]
    pub fn row(&self, index: usize) -> Option<&'a [u8]> {
        let start = index.checked_mul(self.stride)?;
        let end = start.checked_add(self.stride)?;
        self.data.get(start..end)
    }
}

/// The pixel memory of a frame.
#[derive(Debug, Clone, Copy)]
pub enum Planes<'a> {
    /// One contiguous buffer: packed pixels, or coded bytes for JPEG.
    Packed(&'a [u8]),
    /// Three planes (luma, then the two chroma planes).
    Planar {
        /// Luma.
        y: Plane<'a>,
        /// First chroma plane.
        u: Plane<'a>,
        /// Second chroma plane.
        v: Plane<'a>,
    },
}

/// A borrowed frame: geometry, capture timestamp, sequence and pixel views.
#[derive(Debug, Clone, Copy)]
pub struct Frame<'a> {
    /// Width, height, format.
    pub geometry: Geometry,
    /// Capture instant on the device's monotonic clock.
    pub timestamp: Micros,
    /// Wrapping capture counter from the source; gaps mean drops.
    pub sequence: u32,
    /// The pixels.
    pub planes: Planes<'a>,
}

impl<'a> Frame<'a> {
    /// A packed or coded frame over `data`.
    ///
    /// For uncompressed formats `data` must hold at least
    /// [`Geometry::byte_len`] bytes. For JPEG it must be non-empty and start
    /// with the SOI marker `FF D8`.
    pub fn packed(
        geometry: Geometry,
        timestamp: Micros,
        sequence: u32,
        data: &'a [u8],
    ) -> Result<Self> {
        if geometry.format.is_planar() {
            return Err(Error::InvalidGeometry);
        }
        match geometry.byte_len() {
            Some(needed) if data.len() < needed => return Err(Error::BufferTooSmall { needed }),
            Some(_) => {}
            None => {
                if data.len() < 2 || data[0] != 0xFF || data[1] != 0xD8 {
                    return Err(Error::InvalidFormat);
                }
            }
        }
        Ok(Frame {
            geometry,
            timestamp,
            sequence,
            planes: Planes::Packed(data),
        })
    }

    /// A planar 4:2:0 frame over three caller-owned planes.
    pub fn yuv420p(
        geometry: Geometry,
        timestamp: Micros,
        sequence: u32,
        y: (&'a [u8], usize),
        u: (&'a [u8], usize),
        v: (&'a [u8], usize),
    ) -> Result<Self> {
        if geometry.format != PixelFormat::Yuv420p {
            return Err(Error::InvalidGeometry);
        }
        let (w, h) = (geometry.width as usize, geometry.height as usize);
        let y = Plane::new(y.0, y.1, h, w)?;
        let u = Plane::new(u.0, u.1, h / 2, w / 2)?;
        let v = Plane::new(v.0, v.1, h / 2, w / 2)?;
        Ok(Frame {
            geometry,
            timestamp,
            sequence,
            planes: Planes::Planar { y, u, v },
        })
    }

    /// The coded bytes of a compressed frame, or `None` for raw pixels.
    #[must_use]
    pub fn coded(&self) -> Option<&'a [u8]> {
        match self.planes {
            Planes::Packed(data) if self.geometry.format.is_compressed() => Some(data),
            _ => None,
        }
    }

    /// Total bytes referenced by this frame's views.
    #[must_use]
    pub fn byte_len(&self) -> usize {
        match self.planes {
            Planes::Packed(data) => data.len(),
            Planes::Planar { y, u, v } => y.data.len() + u.data.len() + v.data.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_byte_lengths() {
        let g = Geometry::new(320, 240, PixelFormat::Rgb565).unwrap();
        assert_eq!(g.byte_len(), Some(320 * 240 * 2));
        assert_eq!(g.packed_stride(), Some(640));
        let g = Geometry::new(320, 240, PixelFormat::Yuv420p).unwrap();
        assert_eq!(g.byte_len(), Some(320 * 240 * 3 / 2));
        let g = Geometry::new(320, 240, PixelFormat::Jpeg).unwrap();
        assert_eq!(g.byte_len(), None);
    }

    #[test]
    fn geometry_rejects_bad_dimensions() {
        assert_eq!(
            Geometry::new(0, 10, PixelFormat::Gray8),
            Err(Error::InvalidGeometry)
        );
        assert_eq!(
            Geometry::new(11, 10, PixelFormat::Yuyv422),
            Err(Error::InvalidGeometry)
        );
        assert_eq!(
            Geometry::new(10, 11, PixelFormat::Yuv420p),
            Err(Error::InvalidGeometry)
        );
        assert!(Geometry::new(11, 11, PixelFormat::Gray8).is_ok());
    }

    #[test]
    fn packed_frame_checks_buffer_size() {
        let g = Geometry::new(4, 2, PixelFormat::Gray8).unwrap();
        let buf = [0u8; 7];
        assert_eq!(
            Frame::packed(g, Micros::ZERO, 0, &buf).err(),
            Some(Error::BufferTooSmall { needed: 8 })
        );
        let buf = [0u8; 8];
        let f = Frame::packed(g, Micros::ZERO, 0, &buf).unwrap();
        assert_eq!(f.byte_len(), 8);
        assert!(f.coded().is_none());
    }

    #[test]
    fn jpeg_frame_needs_soi() {
        let g = Geometry::new(4, 2, PixelFormat::Jpeg).unwrap();
        assert_eq!(
            Frame::packed(g, Micros::ZERO, 0, &[0, 0]).err(),
            Some(Error::InvalidFormat)
        );
        let f = Frame::packed(g, Micros::ZERO, 1, &[0xFF, 0xD8, 0xFF, 0xD9]).unwrap();
        assert_eq!(f.coded().map(<[u8]>::len), Some(4));
    }

    #[test]
    fn planar_frame_validates_each_plane() {
        let g = Geometry::new(4, 2, PixelFormat::Yuv420p).unwrap();
        let y = [0u8; 8];
        let u = [0u8; 2];
        let v = [0u8; 1];
        assert_eq!(
            Frame::yuv420p(g, Micros::ZERO, 0, (&y, 4), (&u, 2), (&v, 2)).err(),
            Some(Error::BufferTooSmall { needed: 2 })
        );
        let v = [0u8; 2];
        let f = Frame::yuv420p(g, Micros::ZERO, 0, (&y, 4), (&u, 2), (&v, 2)).unwrap();
        assert_eq!(f.byte_len(), 12);
        if let Planes::Planar { y, .. } = f.planes {
            assert_eq!(y.row(1).map(<[u8]>::len), Some(4));
            assert!(y.row(2).is_none());
        } else {
            panic!("expected planar");
        }
    }
}
