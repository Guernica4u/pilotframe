//! CI-safe tests: a tiny XDVDFS image built from scratch. No game data involved.

use pf_vfs::Vfs;
use pf_xdvdfs::{Layout, SECTOR, XdvdfsImage};

const S: usize = SECTOR as usize;

fn entry(left: u16, right: u16, sector: u32, size: u32, attributes: u8, name: &str) -> Vec<u8> {
    let mut e = Vec::new();
    e.extend(left.to_le_bytes());
    e.extend(right.to_le_bytes());
    e.extend(sector.to_le_bytes());
    e.extend(size.to_le_bytes());
    e.push(attributes);
    e.push(name.len() as u8);
    e.extend(name.as_bytes());
    while e.len() % 4 != 0 {
        e.push(0xFF);
    }
    e
}

fn put_dir(image: &mut [u8], sector: usize, entries: &[Vec<u8>]) {
    let table = &mut image[sector * S..(sector + 1) * S];
    table.fill(0xFF);
    let mut at = 0;
    for e in entries {
        table[at..at + e.len()].copy_from_slice(e);
        at += e.len();
    }
}

/// readme.txt ("hello") and maps/mp_test.bsp ("rBSP"), with `base` bytes of padding in front.
fn build(base: usize) -> Vec<u8> {
    let mut img = vec![0u8; base + 37 * S];
    let part = &mut img[base..];

    let vd = &mut part[32 * S..33 * S];
    vd[..20].copy_from_slice(b"MICROSOFT*XBOX*MEDIA");
    vd[20..24].copy_from_slice(&33u32.to_le_bytes());
    vd[24..28].copy_from_slice(&(S as u32).to_le_bytes());
    vd[2028..2048].copy_from_slice(b"MICROSOFT*XBOX*MEDIA");

    let readme = entry(0, 0, 35, 5, 0x80, "readme.txt");
    let right = (readme.len() / 4) as u16;
    let readme = entry(0, right, 35, 5, 0x80, "readme.txt");
    let maps = entry(0, 0, 34, S as u32, 0x10, "maps");
    put_dir(part, 33, &[readme, maps]);
    put_dir(part, 34, &[entry(0, 0, 36, 4, 0x80, "mp_test.bsp")]);

    part[35 * S..35 * S + 5].copy_from_slice(b"hello");
    part[36 * S..36 * S + 4].copy_from_slice(b"rBSP");
    img
}

#[test]
fn lists_and_reads_files() {
    let img = XdvdfsImage::open(Box::new(build(0))).unwrap();
    assert_eq!(img.layout(), Layout::GamePartitionOnly);

    let paths: Vec<_> = img.entries().iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, ["maps", "maps/mp_test.bsp", "readme.txt"]);
    assert_eq!(img.read("readme.txt").unwrap(), b"hello");
    assert_eq!(img.read("MAPS/MP_TEST.BSP").unwrap(), b"rBSP");
    assert!(img.read("maps").is_err());
    assert!(img.read("missing.txt").is_err());
}

#[test]
fn finds_xgd3_game_partition() {
    let img = XdvdfsImage::open(Box::new(build(Layout::Xgd3.partition_offset() as usize))).unwrap();
    assert_eq!(img.layout(), Layout::Xgd3);
    assert_eq!(img.read("maps/mp_test.bsp").unwrap(), b"rBSP");
}

#[test]
fn garbage_is_an_error_not_a_panic() {
    assert!(XdvdfsImage::open(Box::new(vec![0u8; 40 * S])).is_err());

    // Valid descriptor, but the root directory points far past the end of the image.
    let mut img = build(0);
    img[32 * S + 20..32 * S + 24].copy_from_slice(&0x00FF_FFFFu32.to_le_bytes());
    assert!(XdvdfsImage::open(Box::new(img)).is_err());
}
