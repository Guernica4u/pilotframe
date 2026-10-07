//! Compare each model's collision bounds with its render mesh bounds, to check
//! the `.phy` reader and its axis conversion on real files.
//!
//! `cargo run -p pf-fmt-mdl --example check_phy -- <models dir>`

use std::path::Path;

fn bounds(points: impl Iterator<Item = [f32; 3]>) -> ([f32; 3], [f32; 3]) {
    points.fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), p| {
        (
            [lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])],
            [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])],
        )
    })
}

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x == "phy") {
            out.push(p);
        }
    }
}

fn main() {
    let dir = std::env::args().nth(1).expect("models dir");
    let mut files = Vec::new();
    walk(Path::new(&dir), &mut files);
    let (mut ok, mut bad, mut far) = (0, 0, 0);
    for phy in &files {
        let read = |ext: &str| std::fs::read(phy.with_extension(ext)).ok();
        let Ok(c) = pf_fmt_mdl::load_phy(&read("phy").unwrap()) else {
            bad += 1;
            println!("FAIL {}", phy.display());
            continue;
        };
        ok += 1;
        let (Some(mdl), Some(vvd), Some(vtx)) = (read("mdl"), read("vvd"), read("dx11.vtx")) else {
            continue;
        };
        let Ok(model) = pf_fmt_mdl::load(&mdl, &vvd, &vtx) else {
            continue;
        };
        let (plo, phi) = bounds(c.points.iter().copied());
        let (mlo, mhi) = bounds(
            model
                .meshes
                .iter()
                .flat_map(|m| m.positions.iter().copied()),
        );
        let err = (0..3)
            .map(|i| (plo[i] - mlo[i]).abs().max((phi[i] - mhi[i]).abs()))
            .fold(0.0f32, f32::max);
        let size = (0..3).map(|i| mhi[i] - mlo[i]).fold(0.0f32, f32::max);
        if err > 0.25 * size.max(8.0) {
            far += 1;
            if far <= 10 {
                println!(
                    "bounds differ by {err:.0} (size {size:.0}): {}",
                    phy.display()
                );
            }
        }
    }
    println!("{ok} read, {bad} failed, {far} with bounds far from the render mesh");
}
