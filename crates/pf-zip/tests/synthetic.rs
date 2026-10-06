//! A two-member ZIP (stored + deflate) split across two parts, with the second
//! member's data crossing the part boundary.

use pf_vfs::ReadAt;
use pf_zip::SplitZip;
use std::io::Write;

struct Member {
    name: &'static str,
    method: u16,
    data: Vec<u8>,
}

fn build() -> (Vec<u8>, Vec<u8>, Vec<u8>, Vec<Member>) {
    let members = vec![
        Member {
            name: "a.txt",
            method: 0,
            data: b"hello stored".to_vec(),
        },
        Member {
            name: "vpk/b.bin",
            method: 8,
            data: (0..5000u32).map(|i| (i % 251) as u8).collect(),
        },
    ];
    let mut archive = Vec::new();
    let mut cd = Vec::new();
    for m in &members {
        let packed = if m.method == 8 {
            let mut enc =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            enc.write_all(&m.data).unwrap();
            enc.finish().unwrap()
        } else {
            m.data.clone()
        };
        let offset = archive.len() as u32;
        archive.extend_from_slice(b"PK\x03\x04");
        archive.extend_from_slice(&[0u8; 22]);
        archive.extend_from_slice(&(m.name.len() as u16).to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(m.name.as_bytes());
        archive.extend_from_slice(&packed);

        let mut rec = vec![0u8; 46];
        rec[0..4].copy_from_slice(b"PK\x01\x02");
        rec[10..12].copy_from_slice(&m.method.to_le_bytes());
        rec[16..20].copy_from_slice(&crc32fast::hash(&m.data).to_le_bytes());
        rec[20..24].copy_from_slice(&(packed.len() as u32).to_le_bytes());
        rec[24..28].copy_from_slice(&(m.data.len() as u32).to_le_bytes());
        rec[28..30].copy_from_slice(&(m.name.len() as u16).to_le_bytes());
        rec[42..46].copy_from_slice(&offset.to_le_bytes());
        cd.extend_from_slice(&rec);
        cd.extend_from_slice(m.name.as_bytes());
    }
    // Split in the middle of the second member's data. Both headers stay on part 0.
    let split = archive.len() - 100;
    let part1 = archive.split_off(split);
    (cd, archive, part1, members)
}

#[test]
fn extracts_across_parts() {
    let (cd, p0, p1, members) = build();
    let zip = SplitZip::open(&cd, vec![Box::new(p0) as Box<dyn ReadAt>, Box::new(p1)]).unwrap();
    assert_eq!(zip.entries().len(), 2);
    for m in &members {
        let e = zip.find(m.name).unwrap();
        let mut out = Vec::new();
        zip.extract(e, &mut out).unwrap();
        assert_eq!(out, m.data, "{}", m.name);
    }
}

#[test]
fn detects_corruption_and_garbage() {
    let (cd, p0, mut p1, _) = build();
    let last = p1.len() - 5;
    p1[last] ^= 0xFF;
    let zip = SplitZip::open(&cd, vec![Box::new(p0) as Box<dyn ReadAt>, Box::new(p1)]).unwrap();
    let e = zip.find("vpk/b.bin").unwrap().clone();
    assert!(zip.extract(&e, &mut Vec::new()).is_err());
    assert!(pf_zip::parse_central_dir(b"nope").is_err());
}
