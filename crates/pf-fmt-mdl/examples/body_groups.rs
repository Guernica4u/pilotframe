//! List a model's body groups and how many meshes each sub-model adds, with every
//! other group at its default.
//!
//! `cargo run -p pf-fmt-mdl --example body_groups -- <model.mdl>`

fn main() {
    let path = std::path::PathBuf::from(std::env::args().nth(1).expect("model.mdl"));
    let read = |ext: &str| std::fs::read(path.with_extension(ext)).expect(ext);
    let (mdl, vvd, vtx) = (read("mdl"), read("vvd"), read("dx11.vtx"));
    let count = |group: &str, sub: usize| {
        pf_fmt_mdl::load_with_body(&mdl, &vvd, &vtx, |n| if n == group { sub } else { 0 })
            .expect("model")
            .meshes
            .len()
    };
    for (name, subs) in pf_fmt_mdl::body_groups(&mdl).expect("body groups") {
        let meshes: Vec<usize> = (0..subs).map(|sub| count(&name, sub)).collect();
        println!("{name:28} sub-models {subs}  meshes per pick {meshes:?}");
    }
}
