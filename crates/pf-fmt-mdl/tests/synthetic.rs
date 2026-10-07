//! Bad input must give an error, never a panic. (A full synthetic model is a
//! lot of bytes; the real-data check is `examples/check_models.rs`.)

#[test]
fn rejects_garbage() {
    assert!(pf_fmt_mdl::load(b"", b"", b"").is_err());
    assert!(pf_fmt_mdl::load(&[0u8; 400], &[0u8; 64], &[0u8; 36]).is_err());

    // A valid-looking header with absurd counts and offsets.
    let mut mdl = vec![0u8; 400];
    mdl[0..4].copy_from_slice(b"IDST");
    mdl[4..8].copy_from_slice(&52i32.to_le_bytes());
    for at in [204usize, 212, 220, 232] {
        mdl[at..at + 4].copy_from_slice(&i32::MAX.to_le_bytes());
    }
    let mut vvd = vec![0u8; 64];
    vvd[0..4].copy_from_slice(b"IDSV");
    let mut vtx = vec![0u8; 36];
    vtx[0..4].copy_from_slice(&7i32.to_le_bytes());
    assert!(pf_fmt_mdl::load(&mdl, &vvd, &vtx).is_err());
}
