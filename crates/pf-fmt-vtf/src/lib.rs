//! Valve Texture Format (`.vtf`) v7.x as used by Titanfall PC (all v7.5), plus
//! a small reader for `.vmt` material files.
//!
//! Public format facts (Valve Developer Community wiki, "VTF (Valve Texture Format)"):
//! - Header, little-endian: `VTF\0`, version u32 x2, header size u32 (+12),
//!   width u16 (+16), height u16 (+18), flags u32 (+20), frames u16 (+24),
//!   first frame u16 (+26), reflectivity (+32), bump scale (+48),
//!   high-res format u32 (+52), mip count u8 (+56), low-res format u32 (+57),
//!   low-res width u8 (+61), height u8 (+62), depth u16 (+63, v7.2+),
//!   resource count u32 (+68, v7.3+), resources from +80: tag [u8; 3], flags u8,
//!   data u32. Tag `0x30 0 0` holds the offset of the high-res image data.
//! - High-res data stores mips from smallest to largest; each mip holds every
//!   frame, then every face, then every depth slice.

use std::io;

pub mod vmt;

/// Image formats we can hand to the GPU or convert. Numbers are Valve's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Rgba8888,
    Bgr888,
    Bgra8888,
    Dxt1,
    Dxt3,
    Dxt5,
    /// Anything else, with Valve's number. Seen in Titanfall: 34 (rare, unknown).
    Other(u32),
}

impl Format {
    pub fn from_id(id: u32) -> Self {
        match id {
            0 => Self::Rgba8888,
            3 => Self::Bgr888,
            12 => Self::Bgra8888,
            13 => Self::Dxt1,
            14 => Self::Dxt3,
            15 => Self::Dxt5,
            n => Self::Other(n),
        }
    }

    /// Bytes for one `w x h` image, or `None` for unknown formats.
    pub fn image_size(self, w: u32, h: u32) -> Option<usize> {
        let blocks = || (w.div_ceil(4).max(1) * h.div_ceil(4).max(1)) as usize;
        Some(match self {
            Self::Rgba8888 | Self::Bgra8888 => (w * h * 4) as usize,
            Self::Bgr888 => (w * h * 3) as usize,
            Self::Dxt1 => blocks() * 8,
            Self::Dxt3 | Self::Dxt5 => blocks() * 16,
            Self::Other(_) => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Vtf<'a> {
    pub version: (u32, u32),
    pub width: u32,
    pub height: u32,
    pub flags: u32,
    pub frames: u32,
    pub depth: u32,
    pub format: Format,
    pub mip_count: u32,
    data: &'a [u8],
    /// Offset of the high-res image data.
    image_offset: usize,
}

/// Cube map flag (`TEXTUREFLAGS_ENVMAP`); such files hold 6 (or 7) faces.
const FLAG_ENVMAP: u32 = 0x4000;

impl<'a> Vtf<'a> {
    pub fn parse(data: &'a [u8]) -> io::Result<Self> {
        if data.len() < 64 || &data[0..4] != b"VTF\0" {
            return Err(invalid("not a VTF file"));
        }
        let version = (u32_at(data, 4), u32_at(data, 8));
        if version.0 != 7 || version.1 > 5 {
            return Err(invalid(&format!(
                "unsupported VTF version {}.{}",
                version.0, version.1
            )));
        }
        let header_size = u32_at(data, 12) as usize;
        let width = u16_at(data, 16) as u32;
        let height = u16_at(data, 18) as u32;
        let flags = u32_at(data, 20);
        let frames = (u16_at(data, 24) as u32).max(1);
        let format = Format::from_id(u32_at(data, 52));
        let mip_count = (data[56] as u32).max(1);
        let low_format = Format::from_id(u32_at(data, 57));
        let (low_w, low_h) = (data[61] as u32, data[62] as u32);
        let depth = if version.1 >= 2 {
            (u16_at(data, 63) as u32).max(1)
        } else {
            1
        };

        // Prefer the resource directory (v7.3+); older files put the low-res
        // thumbnail right after the header, then the high-res data.
        let mut image_offset = None;
        if version.1 >= 3 {
            let count = u32_at(data, 68) as usize;
            for i in 0..count.min(32) {
                let at = 80 + i * 8;
                let entry = data
                    .get(at..at + 8)
                    .ok_or_else(|| invalid("VTF resource list is cut off"))?;
                if entry[0..3] == [0x30, 0, 0] {
                    image_offset = Some(u32_at(entry, 4) as usize);
                }
            }
        }
        let image_offset = match image_offset {
            Some(o) => o,
            None => {
                let low = if low_w == 0 || low_h == 0 {
                    0
                } else {
                    low_format.image_size(low_w, low_h).unwrap_or(0)
                };
                header_size + low
            }
        };
        if width == 0 || height == 0 {
            return Err(invalid("VTF has zero size"));
        }

        Ok(Self {
            version,
            width,
            height,
            flags,
            frames,
            depth,
            format,
            mip_count,
            data,
            image_offset,
        })
    }

    fn faces(&self) -> u32 {
        if self.flags & FLAG_ENVMAP != 0 { 6 } else { 1 }
    }

    pub fn is_cubemap(&self) -> bool {
        self.flags & FLAG_ENVMAP != 0
    }

    /// Size of mip `level` (0 = full size).
    pub fn mip_size(&self, level: u32) -> (u32, u32) {
        ((self.width >> level).max(1), (self.height >> level).max(1))
    }

    /// Frame 0, face 0, slice 0 of each mip, largest first, as one buffer
    /// (the layout GPUs expect). Returns `None` for formats we don't know.
    pub fn mip_chain(&self) -> io::Result<Option<Vec<u8>>> {
        let per_image = |level: u32| {
            let (w, h) = self.mip_size(level);
            self.format.image_size(w, h)
        };
        if per_image(0).is_none() {
            return Ok(None);
        }
        let copies = (self.frames * self.faces() * self.depth) as usize;

        // Offsets of each mip, walking from the smallest (stored first).
        let mut offsets = vec![0usize; self.mip_count as usize];
        let mut at = self.image_offset;
        for level in (0..self.mip_count).rev() {
            offsets[level as usize] = at;
            at += per_image(level).unwrap() * copies;
        }
        if at > self.data.len() {
            return Err(invalid("VTF image data runs past the end of the file"));
        }

        let mut out = Vec::new();
        for level in 0..self.mip_count {
            let start = offsets[level as usize];
            out.extend_from_slice(&self.data[start..start + per_image(level).unwrap()]);
        }
        Ok(Some(out))
    }
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}
