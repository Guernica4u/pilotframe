//! List and extract files from the game's VPK archives (the EA install's
//! `vpk/` folder).
//!
//! Usage:
//!   pf-vpk list <file_dir.vpk>                       (the named files inside)
//!   pf-vpk extract <file_dir.vpk> <filter> <out_dir>
//!
//! `filter` is a case-insensitive substring of the file's path. Executables and
//! DLLs are never extracted; game scripts only with `PF_READ_SCRIPTS=1`, for
//! reading locally (never committed or shipped).

use pf_vfs::{FileSource, ReadAt};
use std::path::Path;
use std::{fs, io};

const BLOCKED_EXT: [&str; 7] = [".exe", ".dll", ".nut", ".gnut", ".xex", ".lex", ".sys"];

/// Whether a file must not be extracted. Game scripts may be read locally as
/// reference (never committed or shipped), so `PF_READ_SCRIPTS=1` lets `.nut`
/// and `.gnut` through; executables never.
fn blocked(path: &str) -> bool {
    let script = [".nut", ".gnut"].iter().any(|x| path.ends_with(x));
    BLOCKED_EXT.iter().any(|x| path.ends_with(x))
        && !(script && std::env::var_os("PF_READ_SCRIPTS").is_some())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = || {
        io::Error::other(
            "usage: pf-vpk list <file_dir.vpk> | extract <file_dir.vpk> <filter> <out_dir>",
        )
    };
    let get = |i: usize| args.get(i).ok_or_else(usage);
    match args.first().map(String::as_str) {
        Some("list") => list_vpk(Path::new(get(1)?)),
        Some("extract") => extract_vpk(
            Path::new(get(1)?),
            &get(2)?.to_ascii_lowercase(),
            Path::new(get(3)?),
        ),
        _ => Err(usage()),
    }
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

    let (mut count, mut bytes, mut bad) = (0usize, 0u64, 0usize);
    for f in dir.files.iter().filter(|f| f.path.contains(filter)) {
        if blocked(&f.path) {
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
        // Report a bad file and carry on (EA installs rebuild their sounds
        // after install, so some pack entries don't match their checksums).
        if crc32(&data) != f.crc32 {
            eprintln!("skip (CRC-32 mismatch) {}", f.path);
            bad += 1;
            continue;
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
        "extracted {count} files, {:.1} MB; {bad} skipped for bad CRCs",
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
