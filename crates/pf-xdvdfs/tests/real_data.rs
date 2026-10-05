//! Runs against the developer's own disc dump when PF_GAME_DUMP is set; skipped otherwise (CI).
//! Asserts only facts about the image, never game bytes.

use pf_vfs::{FileSource, Vfs};
use pf_xdvdfs::XdvdfsImage;

#[test]
fn opens_local_dump() {
    let Some(path) = pf_testdata::dump() else {
        return;
    };
    let source = FileSource::open(&path).unwrap();
    let image_len = pf_vfs::ReadAt::len(&source);
    let img = XdvdfsImage::open(Box::new(source)).unwrap();

    assert!(
        img.entries().iter().any(|e| !e.is_dir),
        "image has no files"
    );
    for e in img.entries().iter().filter(|e| !e.is_dir) {
        let end = img.data_offset(&e.path).unwrap() + e.size;
        assert!(
            end <= image_len,
            "{} extends past the end of the image",
            e.path
        );
    }
}
