//! Milestone 0 inventory: lists every file in an Xbox 360 disc image and
//! summarises extensions, folders, sizes and magic numbers.
//!
//! The full JSON report is written next to the image (inside the gitignored
//! resources folder) and must never be committed. `.xex`/`.lex` executables are
//! listed but never read.

use pf_vfs::{FileSource, Vfs};
use pf_xdvdfs::XdvdfsImage;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: pf-inventory <image.iso> [--out report.json] [--hash]";

#[derive(Serialize)]
struct FileRecord {
    path: String,
    size: u64,
    magic_hex: String,
    magic_ascii: String,
    sha256: Option<String>,
}

#[derive(Serialize)]
struct Report {
    image: String,
    layout: String,
    file_count: usize,
    dir_count: usize,
    total_bytes: u64,
    files: Vec<FileRecord>,
}

#[derive(Default)]
struct Tally {
    count: usize,
    bytes: u64,
}

fn main() -> ExitCode {
    let mut image = None;
    let mut out = None;
    let mut hash = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--hash" => hash = true,
            "--out" => out = args.next().map(PathBuf::from),
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            _ if image.is_none() => image = Some(PathBuf::from(arg)),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::FAILURE;
            }
        }
    }
    let Some(image) = image else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let out = out.unwrap_or_else(|| image.with_file_name("inventory.json"));

    match run(&image, &out, hash) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(image_path: &PathBuf, out: &PathBuf, hash: bool) -> io::Result<()> {
    let image = XdvdfsImage::open(Box::new(FileSource::open(image_path)?))?;

    let mut files = Vec::new();
    let mut by_ext: BTreeMap<String, Tally> = BTreeMap::new();
    let mut by_folder: BTreeMap<String, Tally> = BTreeMap::new();
    let mut magic_by_ext: BTreeMap<(String, String), usize> = BTreeMap::new();

    for entry in image.entries().iter().filter(|e| !e.is_dir) {
        let ext = extension(&entry.path);
        // .lex files are XEX2 modules too.
        let is_xex = ext == "xex" || ext == "lex";

        let mut magic = vec![0u8; entry.size.min(16) as usize];
        if !is_xex {
            image.read_range(&entry.path, 0, &mut magic)?;
        }
        let (magic_hex, magic_ascii) = if is_xex {
            ("(not read)".to_owned(), "(not read)".to_owned())
        } else {
            (hex(&magic), ascii(&magic))
        };
        let sha256 = if hash && !is_xex {
            Some(sha256_of(&image, &entry.path, entry.size)?)
        } else {
            None
        };

        let folder = entry
            .path
            .split_once('/')
            .map_or("(root)", |(top, _)| top)
            .to_owned();
        add(by_ext.entry(ext.clone()).or_default(), entry.size);
        add(by_folder.entry(folder).or_default(), entry.size);
        let magic4 = format!(
            "{} {}",
            hex(&magic[..magic.len().min(4)]),
            ascii(&magic[..magic.len().min(4)])
        );
        if !is_xex {
            *magic_by_ext.entry((ext, magic4)).or_default() += 1;
        }

        files.push(FileRecord {
            path: entry.path.clone(),
            size: entry.size,
            magic_hex,
            magic_ascii,
            sha256,
        });
    }

    let total_bytes: u64 = files.iter().map(|f| f.size).sum();
    let dir_count = image.entries().iter().filter(|e| e.is_dir).count();

    println!("image:   {}", image_path.display());
    println!("layout:  {:?}", image.layout());
    println!(
        "files:   {}   dirs: {}   total: {}",
        files.len(),
        dir_count,
        mb(total_bytes)
    );

    println!("\n## Extensions (by total size)");
    print_tallies(&by_ext);

    println!("\n## Top-level folders");
    print_tallies(&by_folder);

    println!("\n## Most common magic (first 4 bytes) per extension");
    let mut grouped: BTreeMap<&str, Vec<(&str, usize)>> = BTreeMap::new();
    for ((ext, magic), count) in &magic_by_ext {
        grouped.entry(ext).or_default().push((magic, *count));
    }
    for (ext, mut magics) in grouped {
        magics.sort_by_key(|&(_, count)| std::cmp::Reverse(count));
        let shown: Vec<_> = magics
            .iter()
            .take(3)
            .map(|(m, c)| format!("[{m}] x{c}"))
            .collect();
        println!("  {ext:<10} {}", shown.join("  "));
    }

    println!("\n## 40 largest files");
    let mut largest: Vec<_> = files.iter().collect();
    largest.sort_by_key(|f| std::cmp::Reverse(f.size));
    for f in largest.iter().take(40) {
        println!("  {:>10}  {}", mb(f.size), f.path);
    }

    let report = Report {
        image: image_path.display().to_string(),
        layout: format!("{:?}", image.layout()),
        file_count: files.len(),
        dir_count,
        total_bytes,
        files,
    };
    std::fs::write(
        out,
        serde_json::to_vec_pretty(&report).map_err(io::Error::other)?,
    )?;
    println!("\nfull report: {}", out.display());
    Ok(())
}

fn add(tally: &mut Tally, size: u64) {
    tally.count += 1;
    tally.bytes += size;
}

fn print_tallies(map: &BTreeMap<String, Tally>) {
    let mut rows: Vec<_> = map.iter().collect();
    rows.sort_by_key(|(_, t)| std::cmp::Reverse(t.bytes));
    for (name, t) in rows {
        println!("  {name:<24} {:>7} files  {:>10}", t.count, mb(t.bytes));
    }
}

fn extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext.to_ascii_lowercase(),
        _ => "(none)".to_owned(),
    }
}

fn sha256_of(image: &XdvdfsImage, path: &str, size: u64) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0;
    while done < size {
        let n = (size - done).min(buf.len() as u64) as usize;
        image.read_range(path, done, &mut buf[..n])?;
        hasher.update(&buf[..n]);
        done += n as u64;
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| {
            if (0x20..0x7f).contains(&b) {
                b as char
            } else {
                '.'
            }
        })
        .collect()
}

fn mb(bytes: u64) -> String {
    format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
}
