//! A tiny ISO 9660 image: one normal file and one file split into two extents.

use pf_iso9660::{IsoImage, SECTOR};
use pf_vfs::Vfs;

fn record(name: &[u8], sector: u32, len: u32, flags: u8) -> Vec<u8> {
    let mut r = vec![0u8; 33];
    r[2..6].copy_from_slice(&sector.to_le_bytes());
    r[10..14].copy_from_slice(&len.to_le_bytes());
    r[25] = flags;
    r[32] = name.len() as u8;
    r.extend_from_slice(name);
    if r.len() % 2 == 1 {
        r.push(0);
    }
    r[0] = r.len() as u8;
    r
}

fn build() -> Vec<u8> {
    let s = SECTOR as usize;
    let mut img = vec![0u8; s * 24];
    let mut root = Vec::new();
    root.extend(record(&[0], 18, s as u32, 2));
    root.extend(record(&[1], 18, s as u32, 2));
    root.extend(record(b"README.TXT;1", 19, 5, 0));
    root.extend(record(b"BIG.BIN;1", 20, 3, 0x80));
    root.extend(record(b"BIG.BIN;1", 22, 4, 0));
    img[18 * s..18 * s + root.len()].copy_from_slice(&root);
    img[19 * s..19 * s + 5].copy_from_slice(b"hello");
    img[20 * s..20 * s + 3].copy_from_slice(b"abc");
    img[22 * s..22 * s + 4].copy_from_slice(b"defg");

    let pvd = &mut img[16 * s..17 * s];
    pvd[0] = 1;
    pvd[1..6].copy_from_slice(b"CD001");
    pvd[156..156 + 34].copy_from_slice(&record(&[0], 18, s as u32, 2)[..34]);
    img
}

#[test]
fn reads_files_and_joins_extents() {
    let iso = IsoImage::open(Box::new(build())).unwrap();
    assert_eq!(iso.read("readme.txt").unwrap(), b"hello");
    assert_eq!(iso.read("BIG.BIN").unwrap(), b"abcdefg");
    let mut mid = [0u8; 3];
    iso.read_range("big.bin", 2, &mut mid).unwrap();
    assert_eq!(&mid, b"cde");
    assert!(iso.read_range("big.bin", 6, &mut mid).is_err());
}

#[test]
fn rejects_garbage() {
    assert!(IsoImage::open(Box::new(vec![0u8; 40000])).is_err());
    assert!(IsoImage::open(Box::new(Vec::new())).is_err());
}
