//! Respawn VPK v2.3 (Titanfall PC): the directory file (`*_dir.vpk`).
//!
//! A map ships as `englishclient_<map>.bsp.pak000_dir.vpk` (the index, here)
//! plus `client_<map>.bsp.pak000_000.vpk` (the data).
//!
//! Layout as observed in the owner's files (matches public community notes):
//! - Header, 16 bytes LE: magic 0x55AA1234, major u16 = 2, minor u16 = 3,
//!   tree size u32, signature size u32 (0).
//! - Tree, like Valve VPK: for each extension (NUL-terminated), each directory
//!   (NUL-terminated), each file name (NUL-terminated), then one entry. An
//!   empty string ends each level. A lone space means "no directory/extension".
//! - Entry: CRC-32 u32, preload length u16, then chunks. Each chunk is
//!   archive index u16, load flags u16, texture flags u32, offset u64,
//!   compressed size u64, uncompressed size u64. An archive index of 0xFFFF
//!   ends the chunk list. Preload bytes (if any) follow.
//! - A chunk whose compressed size differs from its size is LZHAM-compressed.

use std::io;

const MAGIC: u32 = 0x55AA_1234;
const HEADER_LEN: usize = 16;
const END_OF_CHUNKS: u16 = 0xFFFF;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// Which `pak000_NNN.vpk` holds it (0 = `_000`).
    pub archive: u16,
    pub load_flags: u16,
    pub texture_flags: u32,
    pub offset: u64,
    pub compressed_size: u64,
    pub size: u64,
}

impl Chunk {
    pub fn is_compressed(&self) -> bool {
        self.compressed_size != self.size
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VpkFile {
    /// `dir/name.ext`, as stored (lower case in practice).
    pub path: String,
    pub crc32: u32,
    /// Offset and length of preload bytes inside the dir file.
    pub preload: (usize, usize),
    pub chunks: Vec<Chunk>,
}

impl VpkFile {
    pub fn size(&self) -> u64 {
        self.preload.1 as u64 + self.chunks.iter().map(|c| c.size).sum::<u64>()
    }

    pub fn stored_size(&self) -> u64 {
        self.chunks.iter().map(|c| c.compressed_size).sum()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub major: u16,
    pub minor: u16,
    pub tree_size: u32,
    pub signature_size: u32,
}

pub struct VpkDir {
    pub header: Header,
    pub files: Vec<VpkFile>,
}

impl VpkDir {
    pub fn parse(data: &[u8]) -> io::Result<Self> {
        let mut r = Cursor { data, pos: 0 };
        if r.u32()? != MAGIC {
            return Err(invalid("not a VPK directory file (bad magic)"));
        }
        let header = Header {
            major: r.u16()?,
            minor: r.u16()?,
            tree_size: r.u32()?,
            signature_size: r.u32()?,
        };
        if (header.major, header.minor) != (2, 3) {
            return Err(invalid(&format!(
                "unsupported VPK version {}.{}",
                header.major, header.minor
            )));
        }
        let tree_end = HEADER_LEN + header.tree_size as usize;
        if tree_end > data.len() {
            return Err(invalid("VPK tree runs past the end of the file"));
        }

        let mut files = Vec::new();
        loop {
            let ext = r.cstr()?;
            if ext.is_empty() {
                break;
            }
            loop {
                let dir = r.cstr()?;
                if dir.is_empty() {
                    break;
                }
                loop {
                    let name = r.cstr()?;
                    if name.is_empty() {
                        break;
                    }
                    files.push(read_entry(&mut r, join(&dir, &name, &ext))?);
                }
            }
        }
        if r.pos > tree_end {
            return Err(invalid("VPK tree is longer than its header says"));
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Self { header, files })
    }
}

fn read_entry(r: &mut Cursor, path: String) -> io::Result<VpkFile> {
    let crc32 = r.u32()?;
    let preload_len = r.u16()? as usize;
    let mut chunks = Vec::new();
    loop {
        let archive = r.u16()?;
        if archive == END_OF_CHUNKS {
            break;
        }
        chunks.push(Chunk {
            archive,
            load_flags: r.u16()?,
            texture_flags: r.u32()?,
            offset: r.u64()?,
            compressed_size: r.u64()?,
            size: r.u64()?,
        });
    }
    let preload = (r.pos, preload_len);
    r.skip(preload_len)?;
    Ok(VpkFile {
        path,
        crc32,
        preload,
        chunks,
    })
}

fn join(dir: &str, name: &str, ext: &str) -> String {
    let mut s = String::new();
    if dir != " " {
        s.push_str(dir);
        s.push('/');
    }
    s.push_str(name);
    if ext != " " {
        s.push('.');
        s.push_str(ext);
    }
    s
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn take(&mut self, n: usize) -> io::Result<&[u8]> {
        let b = self
            .data
            .get(self.pos..self.pos + n)
            .ok_or_else(|| invalid("VPK directory is cut off"))?;
        self.pos += n;
        Ok(b)
    }
    fn skip(&mut self, n: usize) -> io::Result<()> {
        self.take(n).map(|_| ())
    }
    fn u16(&mut self) -> io::Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn cstr(&mut self) -> io::Result<String> {
        let rest = &self.data[self.pos..];
        let len = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| invalid("unterminated string in VPK tree"))?;
        let s = String::from_utf8_lossy(&rest[..len]).into_owned();
        self.pos += len + 1;
        Ok(s)
    }
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}
