//! A hand-built VPK directory with two files, one split into two chunks.

use pf_fmt_vpk::VpkDir;

fn chunk(out: &mut Vec<u8>, offset: u64, packed: u64, size: u64) {
    out.extend_from_slice(&0u16.to_le_bytes()); // archive
    out.extend_from_slice(&0x0101u16.to_le_bytes()); // load flags
    out.extend_from_slice(&0u32.to_le_bytes()); // texture flags
    for v in [offset, packed, size] {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

fn build() -> Vec<u8> {
    let mut tree = Vec::new();
    tree.extend_from_slice(b"vmt\0materials/test\0wall\0");
    tree.extend_from_slice(&0xDEADBEEFu32.to_le_bytes());
    tree.extend_from_slice(&0u16.to_le_bytes());
    chunk(&mut tree, 0, 10, 10);
    tree.extend_from_slice(&0xFFFFu16.to_le_bytes());
    tree.extend_from_slice(b"\0\0"); // end of names, end of dirs
    tree.extend_from_slice(b"txt\0 \0readme\0");
    tree.extend_from_slice(&1u32.to_le_bytes());
    tree.extend_from_slice(&0u16.to_le_bytes());
    chunk(&mut tree, 10, 5, 8);
    chunk(&mut tree, 15, 4, 4);
    tree.extend_from_slice(&0xFFFFu16.to_le_bytes());
    tree.extend_from_slice(b"\0\0\0"); // names, dirs, extensions

    let mut out = Vec::new();
    out.extend_from_slice(&0x55AA1234u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&3u16.to_le_bytes());
    out.extend_from_slice(&(tree.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&tree);
    out
}

#[test]
fn parses_names_and_chunks() {
    let dir = VpkDir::parse(&build()).unwrap();
    let paths: Vec<_> = dir.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["materials/test/wall.vmt", "readme.txt"]);
    let readme = &dir.files[1];
    assert_eq!(readme.chunks.len(), 2);
    assert_eq!(readme.size(), 12);
    assert_eq!(readme.stored_size(), 9);
    assert!(readme.chunks[0].is_compressed());
    assert!(!dir.files[0].chunks[0].is_compressed());
}

#[test]
fn rejects_garbage() {
    assert!(VpkDir::parse(b"").is_err());
    assert!(VpkDir::parse(&[0u8; 64]).is_err());
    let mut cut = build();
    cut.truncate(40);
    assert!(VpkDir::parse(&cut).is_err());
}
