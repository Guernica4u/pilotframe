//! Read-only ISO 9660, the filesystem on the PC retail DVDs.
//!
//! Public format facts used here (ECMA-119):
//! - 2048-byte sectors. The primary volume descriptor is at sector 16:
//!   type byte 1, then `CD001`. The root directory record is at byte 156.
//! - A directory record: length (u8), extent sector (u32 LE at +2), data length
//!   (u32 LE at +10), flags (+25: 0x02 = directory, 0x80 = "more extents
//!   follow"), name length (+32), name (+33). Records never cross a sector;
//!   a zero length byte means "skip to the next sector".
//! - Files over 4 GB are split into several records with the same name, all but
//!   the last flagged 0x80 (multi-extent). We join them into one file.
//!
//! Joliet/Rock Ridge long names are ignored. The Titanfall discs don't need them.

use pf_vfs::{ReadAt, Vfs, VfsEntry, not_found, out_of_bounds};
use std::collections::{HashMap, HashSet};
use std::io;

pub const SECTOR: u64 = 2048;
const PVD_SECTOR: u64 = 16;
const FLAG_DIR: u8 = 0x02;
const FLAG_MULTI_EXTENT: u8 = 0x80;
const MAX_DEPTH: usize = 64;

/// One contiguous piece of a file: (absolute byte offset, length).
type Extent = (u64, u64);

pub struct IsoImage {
    source: Box<dyn ReadAt>,
    entries: Vec<VfsEntry>,
    /// Extents of each entry, parallel to `entries` (empty for directories).
    extents: Vec<Vec<Extent>>,
    index: HashMap<String, usize>,
}

impl IsoImage {
    pub fn open(source: Box<dyn ReadAt>) -> io::Result<Self> {
        let mut pvd = [0u8; SECTOR as usize];
        source.read_exact_at(PVD_SECTOR * SECTOR, &mut pvd)?;
        if pvd[0] != 1 || &pvd[1..6] != b"CD001" {
            return Err(invalid(
                "no ISO 9660 primary volume descriptor at sector 16",
            ));
        }
        let root = &pvd[156..156 + 34];
        let (root_sector, root_len) = (u32_le(root, 2), u32_le(root, 10));

        let mut found = Vec::new();
        let mut visited = HashSet::new();
        read_dir(
            &*source,
            root_sector,
            root_len,
            "",
            0,
            &mut visited,
            &mut found,
        )?;
        found.sort_by(|a, b| a.0.path.cmp(&b.0.path));

        let (entries, extents): (Vec<_>, Vec<_>) = found.into_iter().unzip();
        let index = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.path.to_ascii_lowercase(), i))
            .collect();
        Ok(Self {
            source,
            entries,
            extents,
            index,
        })
    }

    fn lookup(&self, path: &str) -> Option<usize> {
        self.index.get(&path.to_ascii_lowercase()).copied()
    }
}

impl Vfs for IsoImage {
    fn entries(&self) -> &[VfsEntry] {
        &self.entries
    }

    fn find(&self, path: &str) -> Option<&VfsEntry> {
        self.lookup(path).map(|i| &self.entries[i])
    }

    fn read_range(&self, path: &str, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let i = self
            .lookup(path)
            .filter(|&i| !self.entries[i].is_dir)
            .ok_or_else(|| not_found(path))?;
        let size = self.entries[i].size;
        if offset
            .checked_add(buf.len() as u64)
            .is_none_or(|end| end > size)
        {
            return Err(out_of_bounds(offset, buf.len(), size));
        }

        // Walk the extents, copying the part of each that overlaps the request.
        let (mut file_pos, mut done) = (0u64, 0usize);
        for &(start, len) in &self.extents[i] {
            if done == buf.len() {
                break;
            }
            let want = offset + done as u64;
            if want < file_pos + len {
                let skip = want - file_pos;
                let n = ((len - skip) as usize).min(buf.len() - done);
                self.source
                    .read_exact_at(start + skip, &mut buf[done..done + n])?;
                done += n;
            }
            file_pos += len;
        }
        Ok(())
    }
}

#[allow(clippy::type_complexity)]
fn read_dir(
    source: &dyn ReadAt,
    sector: u32,
    len: u32,
    prefix: &str,
    depth: usize,
    visited: &mut HashSet<u32>,
    out: &mut Vec<(VfsEntry, Vec<Extent>)>,
) -> io::Result<()> {
    if depth > MAX_DEPTH || !visited.insert(sector) {
        return Err(invalid("directory tree is too deep or loops"));
    }
    let mut table = vec![0u8; len as usize];
    source.read_exact_at(sector as u64 * SECTOR, &mut table)?;

    let mut pos = 0usize;
    // True when the previous record was flagged "more extents follow".
    let mut continuing = false;
    while pos < table.len() {
        let rec_len = table[pos] as usize;
        if rec_len == 0 {
            pos = (pos / SECTOR as usize + 1) * SECTOR as usize;
            continue;
        }
        let rec = table
            .get(pos..pos + rec_len)
            .filter(|r| r.len() >= 33)
            .ok_or_else(|| invalid("directory record runs past its table"))?;
        pos += rec_len;

        let name_len = rec[32] as usize;
        let raw = rec
            .get(33..33 + name_len)
            .ok_or_else(|| invalid("directory record name runs past the record"))?;
        if raw == [0] || raw == [1] {
            continue; // "." and ".."
        }
        let name = String::from_utf8_lossy(raw);
        let name = name
            .split(';')
            .next()
            .unwrap_or_default()
            .trim_end_matches('.');
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        let (extent, size, flags) = (u32_le(rec, 2), u32_le(rec, 10), rec[25]);
        let piece = (extent as u64 * SECTOR, size as u64);

        if flags & FLAG_DIR != 0 {
            out.push((
                VfsEntry {
                    path: path.clone(),
                    size: 0,
                    is_dir: true,
                },
                Vec::new(),
            ));
            read_dir(source, extent, size, &path, depth + 1, visited, out)?;
            continuing = false;
            continue;
        }

        // Continue a multi-extent file if the previous record was its earlier part.
        match out.last_mut() {
            Some((prev, parts)) if continuing && prev.path == path => {
                prev.size += piece.1;
                parts.push(piece);
            }
            _ => out.push((
                VfsEntry {
                    path,
                    size: piece.1,
                    is_dir: false,
                },
                vec![piece],
            )),
        }
        continuing = flags & FLAG_MULTI_EXTENT != 0;
    }
    Ok(())
}

fn u32_le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}
