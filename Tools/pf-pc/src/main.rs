//! List and extract files from the PC retail DVDs (3 ISO images).
//!
//! Usage:
//!   pf-pc <disc1.iso> list [filter]
//!   pf-pc <disc1.iso> extract <filter> <out_dir>
//!   pf-pc vpk <file_dir.vpk>           (list the named files inside a VPK)
//!   pf-pc vpk-extract <file_dir.vpk> <filter> <out_dir>
//!
//! Discs 2 and 3 are found next to disc 1 by replacing "Disc 1" in the name.
//! `filter` is a case-insensitive substring of the archive path.
//! Executables, DLLs and scripts are never extracted.

use pf_iso9660::IsoImage;
use pf_vfs::{FileSource, ReadAt, Vfs, VfsFile};
use pf_zip::SplitZip;
use std::path::{Path, PathBuf};
use std::{fs, io};

/// The split ZIP's parts in disk order, and which disc each is on.
const PARTS: [(usize, &str); 3] = [
    (0, "Titanfall.z01"),
    (1, "Titanfall.z02"),
    (2, "TITANFAL.ZIP"),
];
const BLOCKED_EXT: [&str; 7] = [".exe", ".dll", ".nut", ".gnut", ".xex", ".lex", ".sys"];

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage =
        || io::Error::other("usage: pf-pc <disc1.iso> list [filter] | extract <filter> <out_dir>");
    if args.first().map(String::as_str) == Some("vpk-extract") {
        let get = |i: usize| args.get(i).ok_or_else(usage);
        return extract_vpk(
            Path::new(get(1)?),
            &get(2)?.to_ascii_lowercase(),
            Path::new(get(3)?),
        );
    }
    if args.first().map(String::as_str) == Some("vpk") {
        return list_vpk(Path::new(args.get(1).ok_or_else(usage)?));
    }
    let disc1 = PathBuf::from(args.first().ok_or_else(usage)?);
    let zip = open_discs(&disc1)?;

    match args.get(1).map(String::as_str) {
        Some("list") => {
            let filter = args
                .get(2)
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            for e in zip
                .entries()
                .iter()
                .filter(|e| e.name.to_ascii_lowercase().contains(&filter))
            {
                println!(
                    "{:>14}  {:>14}  m{}  {}",
                    e.size, e.compressed_size, e.method, e.name
                );
            }
        }
        Some("extract") => {
            let filter = args.get(2).ok_or_else(usage)?.to_ascii_lowercase();
            let out_dir = PathBuf::from(args.get(3).ok_or_else(usage)?);
            for e in zip.entries() {
                let lower = e.name.to_ascii_lowercase();
                if e.is_dir() || !lower.contains(&filter) {
                    continue;
                }
                if BLOCKED_EXT.iter().any(|x| lower.ends_with(x)) {
                    println!("skip (blocked type) {}", e.name);
                    continue;
                }
                let dest = out_dir.join(&e.name);
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut file = io::BufWriter::new(fs::File::create(&dest)?);
                let n = zip.extract(e, &mut file)?;
                println!("ok  {:>12}  {}", n, e.name);
            }
        }
        _ => return Err(usage()),
    }
    Ok(())
}

fn open_discs(disc1: &Path) -> io::Result<SplitZip> {
    let name = disc1
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| io::Error::other("bad disc 1 path"))?;
    if !name.contains("Disc 1") {
        return Err(io::Error::other(
            "expected the Disc 1 image (name containing \"Disc 1\")",
        ));
    }
    let discs: Vec<IsoImage> = (1..=3)
        .map(|n| {
            let path = disc1.with_file_name(name.replace("Disc 1", &format!("Disc {n}")));
            let src = FileSource::open(&path)
                .map_err(|e| io::Error::other(format!("{}: {e}", path.display())))?;
            IsoImage::open(Box::new(src))
        })
        .collect::<io::Result<_>>()?;

    let central_dir = discs[0].read("Titanfall.cd")?;
    // The ISO images stay open for as long as the ZIP is used, so leak them into
    // 'static references. This is a short-lived CLI, so that's fine.
    let discs: &'static [IsoImage] = Box::leak(discs.into_boxed_slice());
    let parts = PARTS
        .iter()
        .map(|&(disc, file)| -> io::Result<Box<dyn ReadAt>> {
            Ok(Box::new(VfsFile::open(&discs[disc], file)?))
        })
        .collect::<io::Result<_>>()?;
    SplitZip::open(&central_dir, parts)
}

/// Print every file in a VPK directory with sizes, then a per-extension summary.
fn list_vpk(path: &Path) -> io::Result<()> {
    let dir = pf_fmt_vpk::VpkDir::parse(&fs::read(path)?)?;
    let mut by_ext = std::collections::BTreeMap::<String, (usize, u64)>::new();
    for f in &dir.files {
        let packed = if f.chunks.iter().any(|c| c.is_compressed()) {
            "lzham"
        } else {
            "raw"
        };
        println!(
            "{:>12}  {:>12}  {:<5}  {}",
            f.size(),
            f.stored_size(),
            packed,
            f.path
        );
        let ext = f
            .path
            .rsplit_once('.')
            .map(|(_, e)| e)
            .unwrap_or("(none)")
            .to_string();
        let slot = by_ext.entry(ext).or_default();
        slot.0 += 1;
        slot.1 += f.size();
    }
    eprintln!("\n{} files", dir.files.len());
    for (ext, (n, bytes)) in by_ext {
        eprintln!("{n:>7}  {:>9.1} MB  .{ext}", bytes as f64 / 1048576.0);
    }
    Ok(())
}

/// Unpack files from a VPK (dir + data archives in the same folder), checking CRC-32.
fn extract_vpk(dir_path: &Path, filter: &str, out_dir: &Path) -> io::Result<()> {
    let dir_data = fs::read(dir_path)?;
    let dir = pf_fmt_vpk::VpkDir::parse(&dir_data)?;
    let dir_name = dir_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    // englishclient_mp_colony.bsp.pak000_dir.vpk -> client_mp_colony.bsp.pak000_NNN.vpk
    let base = dir_name
        .trim_end_matches("_dir.vpk")
        .split_once("client_")
        .map(|(_, rest)| format!("client_{rest}"))
        .ok_or_else(|| io::Error::other("unexpected VPK dir file name"))?;
    let mut archives = std::collections::HashMap::new();

    let (mut count, mut bytes) = (0usize, 0u64);
    for f in dir.files.iter().filter(|f| f.path.contains(filter)) {
        if BLOCKED_EXT.iter().any(|x| f.path.ends_with(x)) {
            continue;
        }
        let mut data = dir_data[f.preload.0..f.preload.0 + f.preload.1].to_vec();
        for c in &f.chunks {
            let archive =
                match archives.entry(c.archive) {
                    std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                    std::collections::hash_map::Entry::Vacant(e) => {
                        let path = dir_path.with_file_name(format!("{base}_{:03}.vpk", c.archive));
                        e.insert(FileSource::open(&path).map_err(|err| {
                            io::Error::other(format!("{}: {err}", path.display()))
                        })?)
                    }
                };
            let mut raw = vec![0u8; c.compressed_size as usize];
            archive.read_exact_at(c.offset, &mut raw)?;
            if c.is_compressed() {
                raw = pf_lzham::decompress(&raw, c.size as usize)
                    .map_err(|e| io::Error::other(format!("{}: {e}", f.path)))?;
            }
            data.extend_from_slice(&raw);
        }
        if crc32(&data) != f.crc32 {
            return Err(io::Error::other(format!("{}: CRC-32 mismatch", f.path)));
        }
        let dest = out_dir.join(&f.path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&dest, &data)?;
        count += 1;
        bytes += data.len() as u64;
    }
    println!(
        "extracted {count} files, {:.1} MB, all CRCs OK",
        bytes as f64 / 1048576.0
    );
    Ok(())
}

fn crc32(data: &[u8]) -> u32 {
    // Small table-free CRC-32 (IEEE); speed doesn't matter for a research tool.
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (crc & 1).wrapping_neg());
        }
    }
    !crc
}
