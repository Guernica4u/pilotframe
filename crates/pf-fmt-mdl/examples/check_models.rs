//! Load every `.mdl` under a folder and report how many parse.
//! `cargo run -p pf-fmt-mdl --example check_models --release -- <models_dir>`

use std::path::Path;

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x == "mdl") {
            out.push(p);
        }
    }
}

fn main() {
    let dir = std::env::args()
        .nth(1)
        .expect("usage: check_models <models_dir>");
    let mut files = Vec::new();
    walk(Path::new(&dir), &mut files);
    let (mut ok, mut tris, mut failed) = (0, 0usize, Vec::new());
    for mdl in &files {
        let read = |ext: &str| std::fs::read(mdl.with_extension(ext)).unwrap_or_default();
        match pf_fmt_mdl::load(&read("mdl"), &read("vvd"), &read("dx11.vtx")) {
            Ok(m) => {
                ok += 1;
                tris += m.meshes.iter().map(|x| x.indices.len() / 3).sum::<usize>();
            }
            Err(e) => failed.push(format!("{}: {e}", mdl.display())),
        }
    }
    println!("{ok}/{} models OK, {tris} triangles", files.len());
    for f in failed.iter().take(15) {
        println!("  {f}");
    }
}
