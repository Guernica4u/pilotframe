//! A hand-built VTF 7.5: 8x8 DXT1, 4 mips, no thumbnail, resource directory
//! pointing at the image data. Each mip is filled with its own byte so we can
//! check the chain comes out largest first.

use pf_fmt_vtf::{Format, Vtf};

fn build() -> Vec<u8> {
    let header_size = 80 + 8; // one resource entry
    let mut h = vec![0u8; header_size];
    h[0..4].copy_from_slice(b"VTF\0");
    h[4..8].copy_from_slice(&7u32.to_le_bytes());
    h[8..12].copy_from_slice(&5u32.to_le_bytes());
    h[12..16].copy_from_slice(&(header_size as u32).to_le_bytes());
    h[16..18].copy_from_slice(&8u16.to_le_bytes());
    h[18..20].copy_from_slice(&8u16.to_le_bytes());
    h[24..26].copy_from_slice(&1u16.to_le_bytes()); // frames
    h[52..56].copy_from_slice(&13u32.to_le_bytes()); // DXT1
    h[56] = 4; // mips: 8, 4, 2, 1
    h[57..61].copy_from_slice(&u32::MAX.to_le_bytes()); // no thumbnail
    h[63..65].copy_from_slice(&1u16.to_le_bytes()); // depth
    h[68..72].copy_from_slice(&1u32.to_le_bytes()); // one resource
    h[80..83].copy_from_slice(&[0x30, 0, 0]);
    h[84..88].copy_from_slice(&(header_size as u32).to_le_bytes());
    // Smallest first: 1x1, 2x2, 4x4 are one 8-byte block each; 8x8 is 4 blocks.
    for (fill, len) in [(1u8, 8), (2, 8), (3, 8), (4, 32)] {
        h.extend(std::iter::repeat_n(fill, len));
    }
    h
}

#[test]
fn reads_header_and_mip_chain() {
    let data = build();
    let vtf = Vtf::parse(&data).unwrap();
    assert_eq!((vtf.width, vtf.height, vtf.mip_count), (8, 8, 4));
    assert_eq!(vtf.format, Format::Dxt1);
    let chain = vtf.mip_chain().unwrap().unwrap();
    assert_eq!(chain.len(), 32 + 8 + 8 + 8);
    assert!(chain[..32].iter().all(|&b| b == 4), "largest mip first");
    assert_eq!(chain[32], 3);
    assert_eq!(chain[48], 1);
}

#[test]
fn rejects_garbage() {
    assert!(Vtf::parse(b"VTF\0").is_err());
    assert!(Vtf::parse(&[0u8; 100]).is_err());
    let mut cut = build();
    cut.truncate(100);
    assert!(Vtf::parse(&cut).unwrap().mip_chain().is_err());
}
