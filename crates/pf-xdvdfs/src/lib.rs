//! Read-only XDVDFS, the filesystem on Xbox / Xbox 360 game discs.
//!
//! Public format facts used here:
//! - 2048-byte sectors.
//! - The volume descriptor sits at sector 32 of the game partition and starts
//!   and ends with the magic `MICROSOFT*XBOX*MEDIA`.
//! - Directories are binary trees of entries. Child links are counted in
//!   4-byte units from the start of the directory table.
//! - All fields are little-endian, even on the (big-endian) 360.
//!
//! Full disc images contain a video partition first, so the game partition
//! starts at a fixed offset that depends on the disc generation (XGD1/2/3).
//! "Game partition only" images start at 0.

use pf_vfs::{ReadAt, Vfs, VfsEntry, not_found, out_of_bounds};
use std::collections::{HashMap, HashSet};
use std::io;

pub const SECTOR: u64 = 2048;
const MAGIC: &[u8; 20] = b"MICROSOFT*XBOX*MEDIA";
const VOLUME_DESCRIPTOR_SECTOR: u64 = 32;
const ATTR_DIRECTORY: u8 = 0x10;
const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    GamePartitionOnly,
    Xgd1,
    Xgd2,
    Xgd3,
}

impl Layout {
    pub fn partition_offset(self) -> u64 {
        match self {
            Layout::GamePartitionOnly => 0,
            Layout::Xgd1 => 0x1830_0000,
            Layout::Xgd2 => 0x0FD9_0000,
            Layout::Xgd3 => 0x0208_0000,
        }
    }
}

pub struct XdvdfsImage {
    source: Box<dyn ReadAt>,
    layout: Layout,
    entries: Vec<VfsEntry>,
    /// Absolute byte offset of each entry's data, parallel to `entries`.
    locations: Vec<u64>,
    /// Lower-cased path -> index into `entries`.
    index: HashMap<String, usize>,
}

impl XdvdfsImage {
    pub fn open(source: Box<dyn ReadAt>) -> io::Result<Self> {
        let layout = detect_layout(&*source)?;
        let base = layout.partition_offset();

        let mut vd = [0u8; SECTOR as usize];
        source.read_exact_at(base + VOLUME_DESCRIPTOR_SECTOR * SECTOR, &mut vd)?;
        if &vd[2028..2048] != MAGIC {
            return Err(invalid("volume descriptor is missing its trailing magic"));
        }
        let root_sector = u32_le(&vd, 20);
        let root_size = u32_le(&vd, 24);

        let mut found = Vec::new();
        read_dir(&*source, base, root_sector, root_size, "", 0, &mut found)?;
        found.sort_by(|a, b| a.0.path.cmp(&b.0.path));

        let (entries, locations): (Vec<_>, Vec<_>) = found.into_iter().unzip();
        let index = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.path.to_ascii_lowercase(), i))
            .collect();
        Ok(Self {
            source,
            layout,
            entries,
            locations,
            index,
        })
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// Absolute byte offset of a file's data inside the image.
    pub fn data_offset(&self, path: &str) -> Option<u64> {
        self.index
            .get(&path.to_ascii_lowercase())
            .map(|&i| self.locations[i])
    }
}

impl Vfs for XdvdfsImage {
    fn entries(&self) -> &[VfsEntry] {
        &self.entries
    }

    fn find(&self, path: &str) -> Option<&VfsEntry> {
        self.index
            .get(&path.to_ascii_lowercase())
            .map(|&i| &self.entries[i])
    }

    fn read_range(&self, path: &str, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let &i = self
            .index
            .get(&path.to_ascii_lowercase())
            .ok_or_else(|| not_found(path))?;
        let entry = &self.entries[i];
        if entry.is_dir {
            return Err(invalid(&format!("{path} is a directory")));
        }
        if offset
            .checked_add(buf.len() as u64)
            .is_none_or(|end| end > entry.size)
        {
            return Err(out_of_bounds(offset, buf.len(), entry.size));
        }
        self.source.read_exact_at(self.locations[i] + offset, buf)
    }
}

fn detect_layout(source: &dyn ReadAt) -> io::Result<Layout> {
    for layout in [
        Layout::GamePartitionOnly,
        Layout::Xgd3,
        Layout::Xgd2,
        Layout::Xgd1,
    ] {
        let at = layout.partition_offset() + VOLUME_DESCRIPTOR_SECTOR * SECTOR;
        let mut magic = [0u8; 20];
        if source.read_exact_at(at, &mut magic).is_ok() && &magic == MAGIC {
            return Ok(layout);
        }
    }
    Err(invalid(
        "no XDVDFS volume descriptor found (not an Xbox disc image?)",
    ))
}

fn read_dir(
    source: &dyn ReadAt,
    base: u64,
    sector: u32,
    size: u32,
    prefix: &str,
    depth: usize,
    out: &mut Vec<(VfsEntry, u64)>,
) -> io::Result<()> {
    if depth > MAX_DEPTH {
        return Err(invalid("directory tree is too deep (corrupt image?)"));
    }
    if size == 0 {
        return Ok(());
    }
    let mut table = vec![0u8; size as usize];
    source.read_exact_at(base + u64::from(sector) * SECTOR, &mut table)?;

    let mut pending = vec![0usize];
    let mut visited = HashSet::new();
    while let Some(at) = pending.pop() {
        if !visited.insert(at) {
            continue;
        }
        let header = table
            .get(at..at + 14)
            .ok_or_else(|| invalid("directory entry runs past its table"))?;
        let left = u16_le(header, 0);
        // 0xFFFF marks padding, which is also how an empty directory looks.
        if left == 0xFFFF {
            continue;
        }
        let right = u16_le(header, 2);
        let start = u32_le(header, 4);
        let len = u32_le(header, 8);
        let attributes = header[12];
        let name_len = usize::from(header[13]);
        let name = table
            .get(at + 14..at + 14 + name_len)
            .ok_or_else(|| invalid("directory entry name runs past its table"))?;
        let name = String::from_utf8_lossy(name);
        let path = if prefix.is_empty() {
            name.into_owned()
        } else {
            format!("{prefix}/{name}")
        };

        for link in [left, right] {
            if link != 0 {
                pending.push(usize::from(link) * 4);
            }
        }

        let is_dir = attributes & ATTR_DIRECTORY != 0;
        let location = base + u64::from(start) * SECTOR;
        out.push((
            VfsEntry {
                path: path.clone(),
                size: if is_dir { 0 } else { u64::from(len) },
                is_dir,
            },
            location,
        ));
        if is_dir {
            read_dir(source, base, start, len, &path, depth + 1, out)?;
        }
    }
    Ok(())
}

fn u16_le(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_owned())
}
