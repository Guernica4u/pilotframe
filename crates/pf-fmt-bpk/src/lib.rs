//! Reader for `XBAR` packs (`r1/bp_packdata/*.bpk` on the Xbox 360 disc).
//!
//! Layout as worked out in `docs/formats/bpk.md` (all integers big-endian):
//!
//! ```text
//! 0x00  magic "XBAR"
//! 0x04  u32 version (6)
//! 0x08  u32 entry count
//! 0x0C  u32 unknown
//! 0x10  u32 LZX window size (0x20000)
//! 0x14  u32 id space (every entry id is below this)
//! 0x18  u32 data offset (header area length; data_offset + data_size = file size)
//! 0x1C  u32 data size
//! 0x20  u32 unknown, u32 unknown, u32 unknown (hash?), u32 unknown
//! 0x30  entry table: count x 28 bytes, sorted by hash
//!       [id] [unpacked size] [start] [end] [hash hi] [hash lo] [flags]
//! ```
//!
//! An entry whose stored length (`end - start`) equals its unpacked size is raw.
//! Otherwise it's an XMemCompress-style LZX stream: chunks prefixed by
//! `0xFF, u16 frame size, u16 chunk size`, or just `u16 chunk size` for a full frame.
//! Entries bigger than the LZX window are a series of independent streams, one per
//! window of output, separated by five zero bytes. If the last segment didn't
//! compress, it follows the final separator raw.

use lzxd::{Lzxd, WindowSize};
use pf_vfs::ReadAt;
use std::io;

pub const MAGIC: &[u8; 4] = b"XBAR";
const HEADER_LEN: usize = 0x30;
const ENTRY_LEN: usize = 28;
/// LZX always decodes in 32 KB frames.
const FRAME_LEN: usize = 0x8000;
const STREAM_SEPARATOR: [u8; 5] = [0; 5];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub version: u32,
    pub entry_count: u32,
    pub unknown_0c: u32,
    pub window_size: u32,
    pub id_space: u32,
    pub data_offset: u32,
    pub data_size: u32,
    pub unknown_20: [u32; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// Rises with `start`, but sparse: not a dense index.
    pub id: u32,
    pub size: u32,
    pub start: u32,
    pub end: u32,
    /// Two words that look like a 64-bit name hash. The table is sorted by it.
    pub hash: u64,
    pub flags: u32,
}

impl Entry {
    pub fn stored_len(&self) -> u32 {
        self.end - self.start
    }

    pub fn is_compressed(&self) -> bool {
        self.stored_len() != self.size
    }
}

pub struct Bpk {
    pub header: Header,
    pub entries: Vec<Entry>,
}

impl Bpk {
    pub fn parse(source: &dyn ReadAt) -> io::Result<Self> {
        let mut head = [0u8; HEADER_LEN];
        source.read_exact_at(0, &mut head)?;
        if &head[..4] != MAGIC {
            return Err(invalid("not an XBAR pack (bad magic)"));
        }
        let w = |i: usize| be32(&head, i * 4);
        let header = Header {
            version: w(1),
            entry_count: w(2),
            unknown_0c: w(3),
            window_size: w(4),
            id_space: w(5),
            data_offset: w(6),
            data_size: w(7),
            unknown_20: [w(8), w(9), w(10), w(11)],
        };
        if header.version != 6 {
            return Err(invalid(&format!("unsupported XBAR version {}", header.version)));
        }
        window(header.window_size)?;

        let table_len = (header.entry_count as usize)
            .checked_mul(ENTRY_LEN)
            .filter(|&n| (HEADER_LEN + n) as u64 <= source.len())
            .ok_or_else(|| invalid("entry table runs past the end of the file"))?;
        let mut table = vec![0u8; table_len];
        source.read_exact_at(HEADER_LEN as u64, &mut table)?;

        let entries = table
            .as_chunks::<ENTRY_LEN>()
            .0
            .iter()
            .map(|r| {
                let f = |i: usize| be32(r, i * 4);
                Entry {
                    id: f(0),
                    size: f(1),
                    start: f(2),
                    end: f(3),
                    hash: (u64::from(f(4)) << 32) | u64::from(f(5)),
                    flags: f(6),
                }
            })
            .collect::<Vec<_>>();
        for e in &entries {
            if e.end < e.start || u64::from(e.end) > source.len() {
                return Err(invalid(&format!("entry {} has a bad data range", e.id)));
            }
        }
        Ok(Self { header, entries })
    }

    /// Read and, if needed, decompress one entry.
    pub fn read(&self, source: &dyn ReadAt, entry: &Entry) -> io::Result<Vec<u8>> {
        let mut stored = vec![0u8; entry.stored_len() as usize];
        source.read_exact_at(u64::from(entry.start), &mut stored)?;
        if !entry.is_compressed() {
            return Ok(stored);
        }
        decompress(&stored, entry.size as usize, self.header.window_size)
    }
}

/// Decode an XMemCompress-framed LZX stream.
pub fn decompress(stored: &[u8], size: usize, window_size: u32) -> io::Result<Vec<u8>> {
    let window_len = window_size as usize;
    let mut lzx = Lzxd::new(window(window_size)?);
    let mut out = Vec::with_capacity(size);
    let mut at = 0;
    while out.len() < size {
        if !out.is_empty() && out.len() % window_len == 0 && stored.get(at..at + 5) == Some(&STREAM_SEPARATOR) {
            lzx.reset();
            at += STREAM_SEPARATOR.len();
            // A final segment that wouldn't compress is stored raw.
            if stored.len() - at == size - out.len() {
                out.extend_from_slice(&stored[at..]);
                break;
            }
        }
        let (frame, chunk_len, header_len) = match stored.get(at) {
            Some(0xFF) => {
                let b = stored.get(at + 1..at + 5).ok_or_else(|| invalid("truncated LZX chunk header"))?;
                (usize::from(be16(b, 0)), usize::from(be16(b, 2)), 5)
            }
            Some(_) => {
                let b = stored.get(at..at + 2).ok_or_else(|| invalid("truncated LZX chunk header"))?;
                (FRAME_LEN, usize::from(be16(b, 0)), 2)
            }
            None => return Err(invalid("LZX stream ended before the expected size")),
        };
        let chunk = stored
            .get(at + header_len..at + header_len + chunk_len)
            .ok_or_else(|| invalid("LZX chunk runs past the stored data"))?;
        let frame = frame.min(size - out.len());
        let decoded = lzx.decompress_next(chunk, frame).map_err(|e| {
            invalid(&format!(
                "LZX decode failed at stored offset {at:#x} (chunk {chunk_len:#x} bytes, frame {frame:#x}, {} of {size:#x} done): {e}",
                out.len()
            ))
        })?;
        out.extend_from_slice(decoded);
        at += header_len + chunk_len;
    }
    Ok(out)
}

fn window(size: u32) -> io::Result<WindowSize> {
    Ok(match size {
        0x0000_8000 => WindowSize::KB32,
        0x0001_0000 => WindowSize::KB64,
        0x0002_0000 => WindowSize::KB128,
        0x0004_0000 => WindowSize::KB256,
        0x0008_0000 => WindowSize::KB512,
        0x0010_0000 => WindowSize::MB1,
        0x0020_0000 => WindowSize::MB2,
        _ => return Err(invalid(&format!("unsupported LZX window size {size:#x}"))),
    })
}

fn be16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_owned())
}
