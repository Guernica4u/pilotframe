//! Byte sources and a read-only virtual filesystem.
//!
//! Parsers take a [`ReadAt`] instead of a path, so the same code works on a disc
//! image, an in-memory buffer, or (later) a browser `File`. Only [`FileSource`]
//! touches `std::fs`, and it is compiled out on wasm32.

use std::io;

/// Random-access, read-only bytes.
pub trait ReadAt: Send + Sync {
    fn len(&self) -> u64;

    /// Fill `buf` from `offset`, or fail. Never returns a short read.
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()>;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub fn out_of_bounds(offset: u64, wanted: usize, len: u64) -> io::Error {
    io::Error::new(
        io::ErrorKind::UnexpectedEof,
        format!("read of {wanted} bytes at {offset:#x} is past the end ({len:#x})"),
    )
}

impl ReadAt for Vec<u8> {
    fn len(&self) -> u64 {
        self.len() as u64
    }

    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let start = usize::try_from(offset).map_err(|_| out_of_bounds(offset, buf.len(), self.len() as u64))?;
        let src = start
            .checked_add(buf.len())
            .and_then(|end| self.get(start..end))
            .ok_or_else(|| out_of_bounds(offset, buf.len(), self.len() as u64))?;
        buf.copy_from_slice(src);
        Ok(())
    }
}

/// A file on the native filesystem (setup tool, inventory, tests).
#[cfg(not(target_arch = "wasm32"))]
pub struct FileSource {
    file: std::sync::Mutex<std::fs::File>,
    len: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl FileSource {
    pub fn open(path: impl AsRef<std::path::Path>) -> io::Result<Self> {
        let file = std::fs::File::open(path)?;
        let len = file.metadata()?.len();
        Ok(Self { file: std::sync::Mutex::new(file), len })
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ReadAt for FileSource {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        use std::io::{Read, Seek, SeekFrom};
        if offset.checked_add(buf.len() as u64).is_none_or(|end| end > self.len) {
            return Err(out_of_bounds(offset, buf.len(), self.len));
        }
        let mut file = self.file.lock().map_err(|_| io::Error::other("file lock poisoned"))?;
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(buf)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VfsEntry {
    /// `/`-separated, relative to the filesystem root, original casing.
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
}

/// A read-only tree of files (a disc image, an extracted folder, a package).
pub trait Vfs: Send + Sync {
    /// All entries, sorted by path.
    fn entries(&self) -> &[VfsEntry];

    /// Read part of a file. Paths are matched case-insensitively.
    fn read_range(&self, path: &str, offset: u64, buf: &mut [u8]) -> io::Result<()>;

    fn find(&self, path: &str) -> Option<&VfsEntry> {
        self.entries().iter().find(|e| e.path.eq_ignore_ascii_case(path))
    }

    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        let entry = self.find(path).ok_or_else(|| not_found(path))?;
        let size = usize::try_from(entry.size).map_err(|_| io::Error::other("file too large for memory"))?;
        let mut buf = vec![0; size];
        self.read_range(path, 0, &mut buf)?;
        Ok(buf)
    }
}

/// One file inside a [`Vfs`], viewed as a [`ReadAt`], so a parser can read a pack
/// straight out of a disc image without extracting it first.
pub struct VfsFile<'a> {
    vfs: &'a dyn Vfs,
    path: String,
    len: u64,
}

impl<'a> VfsFile<'a> {
    pub fn open(vfs: &'a dyn Vfs, path: &str) -> io::Result<Self> {
        let entry = vfs.find(path).filter(|e| !e.is_dir).ok_or_else(|| not_found(path))?;
        Ok(Self { vfs, path: entry.path.clone(), len: entry.size })
    }
}

impl ReadAt for VfsFile<'_> {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        self.vfs.read_range(&self.path, offset, buf)
    }
}

pub fn not_found(path: &str) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, format!("no such file: {path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec_reads_and_bounds() {
        let data = b"abcdef".to_vec();
        let mut buf = [0; 3];
        data.read_exact_at(2, &mut buf).unwrap();
        assert_eq!(&buf, b"cde");
        assert!(data.read_exact_at(4, &mut buf).is_err());
        assert!(data.read_exact_at(u64::MAX, &mut buf).is_err());
    }
}
