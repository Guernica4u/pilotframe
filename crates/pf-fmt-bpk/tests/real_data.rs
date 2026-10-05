//! Runs against the developer's own disc dump when PF_GAME_DUMP is set; skipped otherwise (CI).
//! Asserts only facts about the packs, never game bytes.

use pf_fmt_bpk::Bpk;
use pf_vfs::{FileSource, Vfs, VfsFile};
use pf_xdvdfs::XdvdfsImage;

fn image() -> Option<XdvdfsImage> {
    let path = pf_testdata::dump()?;
    Some(XdvdfsImage::open(Box::new(FileSource::open(path).unwrap())).unwrap())
}

#[test]
fn every_pack_parses() {
    let Some(img) = image() else { return };
    let packs: Vec<_> = img
        .entries()
        .iter()
        .filter(|e| e.path.starts_with("r1/bp_packdata/") && e.path.ends_with(".bpk"))
        .collect();
    assert_eq!(packs.len(), 18);
    for pack in packs {
        let file = VfsFile::open(&img, &pack.path).unwrap();
        let bpk = Bpk::parse(&file).unwrap();
        assert_eq!(
            u64::from(bpk.header.data_offset) + u64::from(bpk.header.data_size),
            pack.size,
            "{}: data offset + size should equal the file size",
            pack.path
        );
        assert!(bpk.entries.iter().all(|e| e.id < bpk.header.id_space), "{}", pack.path);
    }
}

#[test]
fn lobby_pack_fully_decodes() {
    let Some(img) = image() else { return };
    let file = VfsFile::open(&img, "r1/bp_packdata/mp_lobby.bpk").unwrap();
    let bpk = Bpk::parse(&file).unwrap();
    let mut bsp = 0;
    for e in &bpk.entries {
        let data = bpk.read(&file, e).unwrap();
        assert_eq!(data.len(), e.size as usize, "entry {}", e.id);
        bsp += usize::from(data.starts_with(b"PSBr"));
    }
    assert_eq!(bsp, 1, "expected exactly one big-endian BSP in the lobby pack");
}
