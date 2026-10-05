//! Format research helper. Prints a hex dump of part of a file inside a disc image.
//! Output is for local study only: don't paste more than struct headers into issues or docs.

use pf_vfs::{FileSource, Vfs};
use pf_xdvdfs::XdvdfsImage;
use std::process::ExitCode;

const USAGE: &str = "usage: pf-probe <image.iso> <path/in/image> [offset] [length]   (numbers accept 0x prefix)\n       pf-probe <image.iso> <path/in/image> --save <out-file>   (keep out-file inside Resources/)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(image), Some(path)) = (args.first(), args.get(1)) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    if path.to_ascii_lowercase().ends_with(".xex") || path.to_ascii_lowercase().ends_with(".lex") {
        eprintln!("refusing to read executables (project rule)");
        return ExitCode::FAILURE;
    }
    if args.get(2).map(String::as_str) == Some("--save") {
        let Some(out) = args.get(3) else {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        };
        let result = XdvdfsImage::open(match FileSource::open(image) {
            Ok(s) => Box::new(s),
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        })
        .and_then(|img| img.read(path))
        .and_then(|bytes| std::fs::write(out, bytes));
        return match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        };
    }

    let offset = args.get(2).map_or(Some(0), |s| parse_num(s));
    let length = args.get(3).map_or(Some(256), |s| parse_num(s));
    let (Some(offset), Some(length)) = (offset, length) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };

    let result = (|| -> std::io::Result<()> {
        let img = XdvdfsImage::open(Box::new(FileSource::open(image)?))?;
        let size = img.find(path).ok_or_else(|| pf_vfs::not_found(path))?.size;
        let length = length.min(size.saturating_sub(offset));
        let mut buf = vec![0u8; length as usize];
        img.read_range(path, offset, &mut buf)?;
        println!("{path}  size {size} ({size:#x})");
        for (i, row) in buf.chunks(16).enumerate() {
            let hex: Vec<_> = row.iter().map(|b| format!("{b:02x}")).collect();
            let ascii: String = row
                .iter()
                .map(|&b| {
                    if (0x20..0x7f).contains(&b) {
                        b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            println!(
                "{:08x}  {:<47}  {ascii}",
                offset + (i * 16) as u64,
                hex.join(" ")
            );
        }
        Ok(())
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn parse_num(s: &str) -> Option<u64> {
    match s.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => s.parse().ok(),
    }
}
