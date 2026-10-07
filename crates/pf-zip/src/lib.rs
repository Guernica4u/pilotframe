//! Read-only split (multi-disc) ZIP, as used by the PC retail DVDs.
//!
//! The PC installer data is one ZIP64 archive split into parts:
//! `Titanfall.z01` (disc 1), `Titanfall.z02` (disc 2), `TITANFAL.ZIP` (disc 3),
//! plus `Titanfall.cd` on disc 1, a separate copy of the central directory.
//!
//! Public format facts used here (PKWARE APPNOTE):
//! - Central directory records start with `PK\1\2`. Each one gives the method,
//!   CRC-32, sizes, the disk (part) number of the local header and its offset
//!   within that part. Values of 0xFFFFFFFF / 0xFFFF are in the ZIP64 extra
//!   field (id 0x0001), in the order: size, compressed size, offset, disk.
//! - Local headers (`PK\3\4`) are 30 bytes + name + extra; data follows.
//! - A member's data may run past the end of one part into the next, so parts
//!   are treated as one concatenated stream.
//!
//! Only "stored" (0) and "deflate" (8) are supported. Deflate data can't be
//! read at random offsets, so callers stream it out once with [`SplitZip::extract`].

use pf_vfs::{ReadAt, out_of_bounds};
use std::io::{self, Read, Write};

pub const METHOD_STORED: u16 = 0;
pub const METHOD_DEFLATE: u16 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipEntry {
    /// `/`-separated path as stored in the archive.
    pub name: String,
    pub method: u16,
    pub flags: u16,
    pub crc32: u32,
    pub compressed_size: u64,
    pub size: u64,
    /// Part index (0 = first part) holding the local header.
    pub disk: u32,
    /// Offset of the local header within that part.
    pub local_offset: u64,
}

impl ZipEntry {
    pub fn is_dir(&self) -> bool {
        self.name.ends_with('/')
    }

    pub fn is_encrypted(&self) -> bool {
        self.flags & 1 != 0
    }
}

pub struct SplitZip {
    parts: Concat,
    entries: Vec<ZipEntry>,
}

impl SplitZip {
    /// `central_dir`: the raw central directory bytes (e.g. `Titanfall.cd`).
    /// `parts`: every archive part in disk order.
    pub fn open(central_dir: &[u8], parts: Vec<Box<dyn ReadAt>>) -> io::Result<Self> {
        let entries = parse_central_dir(central_dir)?;
        let parts = Concat::new(parts);
        for e in &entries {
            if e.disk as usize >= parts.starts.len() {
                return Err(invalid(format!(
                    "{} is on part {}, which is missing",
                    e.name, e.disk
                )));
            }
        }
        Ok(Self { parts, entries })
    }

    pub fn entries(&self) -> &[ZipEntry] {
        &self.entries
    }

    pub fn find(&self, name: &str) -> Option<&ZipEntry> {
        self.entries
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(name))
    }

    /// Absolute offset (in the concatenated parts) of the member's data.
    pub fn data_offset(&self, entry: &ZipEntry) -> io::Result<u64> {
        let header = self.parts.starts[entry.disk as usize] + entry.local_offset;
        let mut lh = [0u8; 30];
        self.parts.read_exact_at(header, &mut lh)?;
        if &lh[0..4] != b"PK\x03\x04" {
            return Err(invalid(format!(
                "{}: no local header at its offset",
                entry.name
            )));
        }
        let name_len = u16_le(&lh, 26) as u64;
        let extra_len = u16_le(&lh, 28) as u64;
        Ok(header + 30 + name_len + extra_len)
    }

    /// Stream one member's uncompressed bytes into `out`, checking size and CRC-32.
    /// Returns the number of bytes written.
    pub fn extract(&self, entry: &ZipEntry, out: &mut dyn Write) -> io::Result<u64> {
        if entry.is_encrypted() {
            return Err(invalid(format!("{} is encrypted", entry.name)));
        }
        let raw = RangeReader {
            src: &self.parts,
            pos: self.data_offset(entry)?,
            left: entry.compressed_size,
        };
        let mut reader: Box<dyn Read + '_> = match entry.method {
            METHOD_STORED => Box::new(raw),
            METHOD_DEFLATE => Box::new(flate2::read::DeflateDecoder::new(
                io::BufReader::with_capacity(1 << 20, raw),
            )),
            m => return Err(invalid(format!("{}: unsupported method {m}", entry.name))),
        };

        let mut crc = crc32fast::Hasher::new();
        let mut buf = vec![0u8; 1 << 20];
        let mut written = 0u64;
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            crc.update(&buf[..n]);
            out.write_all(&buf[..n])?;
            written += n as u64;
        }
        if written != entry.size {
            return Err(invalid(format!(
                "{}: got {written} bytes, expected {}",
                entry.name, entry.size
            )));
        }
        if crc.finalize() != entry.crc32 {
            return Err(invalid(format!("{}: CRC-32 mismatch", entry.name)));
        }
        Ok(written)
    }
}

/// A `Write` sink that keeps only some byte ranges of what passes through it.
/// Use with [`SplitZip::extract`] to pull a few pieces out of a huge Deflate
/// member in one pass without saving the whole thing.
pub struct RangeCapture {
    pos: u64,
    /// (start, length), with the captured bytes in `data` at the same index.
    ranges: Vec<(u64, u64)>,
    pub data: Vec<Vec<u8>>,
}

impl RangeCapture {
    pub fn new(ranges: Vec<(u64, u64)>) -> Self {
        let data = ranges.iter().map(|_| Vec::new()).collect();
        Self {
            pos: 0,
            ranges,
            data,
        }
    }
}

impl Write for RangeCapture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let (start, end) = (self.pos, self.pos + buf.len() as u64);
        for (i, &(r_start, r_len)) in self.ranges.iter().enumerate() {
            let (lo, hi) = (r_start.max(start), (r_start + r_len).min(end));
            if lo < hi {
                self.data[i].extend_from_slice(&buf[(lo - start) as usize..(hi - start) as usize]);
            }
        }
        self.pos = end;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub fn parse_central_dir(cd: &[u8]) -> io::Result<Vec<ZipEntry>> {
    let mut entries = Vec::new();
    let mut at = 0usize;
    while cd.get(at..at + 4) == Some(b"PK\x01\x02") {
        let rec = cd
            .get(at..at + 46)
            .ok_or_else(|| invalid("central directory record is cut off".into()))?;
        let (name_len, extra_len, comment_len) = (
            u16_le(rec, 28) as usize,
            u16_le(rec, 30) as usize,
            u16_le(rec, 32) as usize,
        );
        let name = cd
            .get(at + 46..at + 46 + name_len)
            .ok_or_else(|| invalid("central directory name is cut off".into()))?;
        let extra = cd
            .get(at + 46 + name_len..at + 46 + name_len + extra_len)
            .ok_or_else(|| invalid("central directory extra field is cut off".into()))?;

        let mut e = ZipEntry {
            name: String::from_utf8_lossy(name).into_owned(),
            flags: u16_le(rec, 8),
            method: u16_le(rec, 10),
            crc32: u32_le(rec, 16),
            compressed_size: u32_le(rec, 20) as u64,
            size: u32_le(rec, 24) as u64,
            disk: u16_le(rec, 34) as u32,
            local_offset: u32_le(rec, 42) as u64,
        };
        apply_zip64(&mut e, extra)?;
        entries.push(e);
        at += 46 + name_len + extra_len + comment_len;
    }
    if entries.is_empty() {
        return Err(invalid("no central directory records found".into()));
    }
    Ok(entries)
}

/// Fill in fields that were too big for the classic record from the ZIP64 extra.
fn apply_zip64(e: &mut ZipEntry, mut extra: &[u8]) -> io::Result<()> {
    while extra.len() >= 4 {
        let (id, len) = (u16_le(extra, 0), u16_le(extra, 2) as usize);
        let body = extra
            .get(4..4 + len)
            .ok_or_else(|| invalid(format!("{}: extra field is cut off", e.name)))?;
        if id == 0x0001 {
            let mut fields = body.as_chunks::<8>().0.iter();
            let mut next = |name: &str| {
                fields
                    .next()
                    .map(|b| u64::from_le_bytes(*b))
                    .ok_or_else(|| invalid(format!("{}: ZIP64 field {name} missing", e.name)))
            };
            if e.size == 0xFFFF_FFFF {
                e.size = next("size")?;
            }
            if e.compressed_size == 0xFFFF_FFFF {
                e.compressed_size = next("compressed size")?;
            }
            if e.local_offset == 0xFFFF_FFFF {
                e.local_offset = next("offset")?;
            }
            // A 0xFFFF disk number would be a u32 after the offsets; not used by these discs.
        }
        extra = &extra[4 + len..];
    }
    Ok(())
}

/// Several byte sources read as one.
struct Concat {
    parts: Vec<Box<dyn ReadAt>>,
    /// Absolute start of each part.
    starts: Vec<u64>,
    len: u64,
}

impl Concat {
    fn new(parts: Vec<Box<dyn ReadAt>>) -> Self {
        let mut starts = Vec::with_capacity(parts.len());
        let mut len = 0;
        for p in &parts {
            starts.push(len);
            len += p.len();
        }
        Self { parts, starts, len }
    }
}

impl ReadAt for Concat {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        if offset
            .checked_add(buf.len() as u64)
            .is_none_or(|end| end > self.len)
        {
            return Err(out_of_bounds(offset, buf.len(), self.len));
        }
        let mut done = 0;
        while done < buf.len() {
            let pos = offset + done as u64;
            // Last part whose start is <= pos.
            let i = self.starts.partition_point(|&s| s <= pos) - 1;
            let in_part = pos - self.starts[i];
            let n = ((self.parts[i].len() - in_part) as usize).min(buf.len() - done);
            self.parts[i].read_exact_at(in_part, &mut buf[done..done + n])?;
            done += n;
        }
        Ok(())
    }
}

/// `Read` over a byte range of a [`ReadAt`].
struct RangeReader<'a> {
    src: &'a dyn ReadAt,
    pos: u64,
    left: u64,
}

impl Read for RangeReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = (buf.len() as u64).min(self.left) as usize;
        if n == 0 {
            return Ok(0);
        }
        self.src.read_exact_at(self.pos, &mut buf[..n])?;
        self.pos += n as u64;
        self.left -= n as u64;
        Ok(n)
    }
}

fn u16_le(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(b[at..at + 2].try_into().unwrap())
}

fn u32_le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn invalid(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}
