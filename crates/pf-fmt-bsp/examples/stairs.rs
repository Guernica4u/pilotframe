//! List the surfaces whose material name contains a word, with their slope and
//! bounds, to see how a map builds (for example) its stairs.
//!
//! `cargo run -p pf-fmt-bsp --example stairs -- <map.bsp> [word]`

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("map.bsp");
    let word = args.next().unwrap_or_else(|| "stair".into());
    let data = std::fs::read(&path).unwrap();
    let bsp = pf_fmt_bsp::Bsp::parse(&data).unwrap();
    let g = bsp.geometry().unwrap();
    for (mi, name) in g.materials.iter().enumerate() {
        if !name.to_ascii_lowercase().contains(&word) {
            continue;
        }
        let mut slopes = [0usize; 10];
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        let mut tris = 0;
        for m in g.meshes.iter().filter(|m| m.material as usize == mi) {
            for t in &m.triangles {
                let p: Vec<[f32; 3]> = t.iter().map(|c| g.positions[c.position as usize]).collect();
                let u = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
                let v = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
                let n = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
                // Source is Z up: angle from flat.
                let deg = (n[2].abs() / len).acos().to_degrees();
                slopes[((deg / 10.0) as usize).min(9)] += 1;
                for q in &p {
                    for k in 0..3 {
                        lo[k] = lo[k].min(q[k]);
                        hi[k] = hi[k].max(q[k]);
                    }
                }
                tris += 1;
            }
        }
        if tris > 0 {
            println!("{name}: {tris} triangles, slope histogram (10° buckets) {slopes:?}");
            println!("   bounds {lo:?} .. {hi:?}");
        }
    }
}
