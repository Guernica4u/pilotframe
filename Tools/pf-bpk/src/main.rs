//! Format research tool for XBAR packs.
//!
//!   pf-bpk <file.bpk> info              header + entry stats
//!   pf-bpk <file.bpk> list              one line per entry: id, hash, size, flags, magic
//!   pf-bpk <file.bpk> magic             decompress everything, histogram of leading bytes
//!   pf-bpk <file.bpk> extract <dir>     write every entry to <dir> (keep it inside Resources/)

use pf_fmt_bpk::Bpk;
use pf_vfs::FileSource;
use std::collections::BTreeMap;
use std::io;
use std::process::ExitCode;

const USAGE: &str = "usage: pf-bpk <file.bpk> info | list | magic | extract <dir>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        [file, "info"] => info(file),
        [file, "list"] => list(file),
        [file, "magic"] => magic(file),
        [file, "extract", dir] => extract(file, dir),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn open(file: &str) -> io::Result<(FileSource, Bpk)> {
    let source = FileSource::open(file)?;
    let bpk = Bpk::parse(&source)?;
    Ok((source, bpk))
}

fn info(file: &str) -> io::Result<()> {
    let (_, bpk) = open(file)?;
    println!("{:#x?}", bpk.header);
    let compressed = bpk.entries.iter().filter(|e| e.is_compressed()).count();
    let unpacked: u64 = bpk.entries.iter().map(|e| u64::from(e.size)).sum();
    println!(
        "entries: {}  compressed: {}  raw: {}",
        bpk.entries.len(),
        compressed,
        bpk.entries.len() - compressed
    );
    println!("unpacked total: {:.2} MB", unpacked as f64 / 1048576.0);
    let mut flags: BTreeMap<u32, usize> = BTreeMap::new();
    for e in &bpk.entries {
        *flags.entry(e.flags).or_default() += 1;
    }
    println!("flags: {flags:x?}");
    Ok(())
}

fn list(file: &str) -> io::Result<()> {
    let (source, bpk) = open(file)?;
    println!(
        "{:>5}  {:<16}  {:>9}  {:>5}  {:<4}  magic",
        "id", "hash", "size", "flags", "lzx"
    );
    for e in &bpk.entries {
        let data = bpk.read(&source, e)?;
        let lzx = if e.is_compressed() { "yes" } else { "" };
        println!(
            "{:>5}  {:016x}  {:>9}  {:>5}  {:<4}  {}",
            e.id,
            e.hash,
            e.size,
            e.flags,
            lzx,
            label(&data)
        );
    }
    Ok(())
}

fn magic(file: &str) -> io::Result<()> {
    let (source, bpk) = open(file)?;
    let mut by_magic: BTreeMap<String, (usize, u64)> = BTreeMap::new();
    let mut failed = 0;
    for e in &bpk.entries {
        match bpk.read(&source, e) {
            Ok(data) => {
                let t = by_magic.entry(label(&data)).or_default();
                t.0 += 1;
                t.1 += data.len() as u64;
            }
            Err(err) => {
                failed += 1;
                if failed <= 5 {
                    eprintln!("entry {} failed: {err}", e.id);
                }
            }
        }
    }
    let mut rows: Vec<_> = by_magic.into_iter().collect();
    rows.sort_by_key(|(_, (_, bytes))| std::cmp::Reverse(*bytes));
    for (magic, (count, bytes)) in rows {
        println!(
            "  {magic:<28} {count:>5}  {:>10.2} MB",
            bytes as f64 / 1048576.0
        );
    }
    println!("decode failures: {failed}");
    Ok(())
}

fn extract(file: &str, dir: &str) -> io::Result<()> {
    let (source, bpk) = open(file)?;
    std::fs::create_dir_all(dir)?;
    for e in &bpk.entries {
        let data = bpk.read(&source, e)?;
        let name = format!("{:04}_{:016x}.{}", e.id, e.hash, guess_ext(&data));
        std::fs::write(std::path::Path::new(dir).join(name), data)?;
    }
    println!("wrote {} entries to {dir}", bpk.entries.len());
    Ok(())
}

/// First 4 bytes as hex + printable ASCII.
fn label(data: &[u8]) -> String {
    let head = &data[..data.len().min(4)];
    let hex: String = head.iter().map(|b| format!("{b:02x}")).collect();
    let ascii: String = head
        .iter()
        .map(|&b| {
            if (0x20..0x7f).contains(&b) {
                b as char
            } else {
                '.'
            }
        })
        .collect();
    format!("{hex} {ascii}")
}

fn guess_ext(data: &[u8]) -> &'static str {
    match data.get(..4) {
        Some(b"rBSP") | Some(b"PSBr") => "bsp",
        Some(b"IDST") | Some(b"TSDI") => "mdl",
        Some(b"VTF\0") | Some(b"VTFX") => "vtf",
        _ if data
            .iter()
            .take(256)
            .all(|&b| b == b'\t' || b == b'\r' || b == b'\n' || (0x20..0x7f).contains(&b)) =>
        {
            "txt"
        }
        _ => "bin",
    }
}
