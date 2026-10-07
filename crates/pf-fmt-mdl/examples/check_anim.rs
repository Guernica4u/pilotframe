//! Pose a first-person model with one of its animations and print where some
//! bones and the skinned mesh end up, relative to the camera bone. A sanity check
//! for the animation reader on real files.
//!
//! `cargo run -p pf-fmt-mdl --example check_anim -- <model.mdl> [animation] [seconds]`

use glam::Vec3;
use pf_fmt_mdl::anim;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = std::path::PathBuf::from(args.next().expect("model.mdl"));
    let wanted = args.next().unwrap_or_else(|| "@draw_seq".into());
    let time: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(10.0);

    let read = |ext: &str| std::fs::read(path.with_extension(ext)).expect(ext);
    let mdl = read("mdl");
    let bones = anim::skeleton(&mdl).unwrap();
    let anims = anim::animations(&mdl).unwrap();
    let a = anims
        .iter()
        .find(|a| a.name == wanted)
        .expect("no such animation");
    println!(
        "{}: {} frames at {} fps, flags {:#x}",
        a.name, a.frames, a.fps, a.flags
    );
    let pose = anim::sample(&mdl, &bones, a, time).unwrap();
    let world = anim::world_transforms(&bones, &pose);

    let cam = bones
        .iter()
        .position(|b| b.name == "jx_c_camera")
        .expect("camera bone");
    let to_cam = world[cam].inverse();
    for name in [
        "jx_c_camera",
        "ja_c_propGun",
        "def_l_wrist",
        "def_r_wrist",
        "def_c_hip",
    ] {
        if let Some(i) = bones.iter().position(|b| b.name == name) {
            let p = to_cam.transform_point3(world[i].translation.into());
            println!("  {name:14} in camera space: {p:7.2}");
        }
    }

    let model = pf_fmt_mdl::load(&mdl, &read("vvd"), &read("dx11.vtx")).unwrap();
    let skin = anim::skinning(&bones, &world);
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for m in &model.meshes {
        for ((p, b), w) in m.positions.iter().zip(&m.bones).zip(&m.weights) {
            let mut out = Vec3::ZERO;
            for k in 0..3 {
                if w[k] > 0.0 {
                    out += skin[b[k] as usize].transform_point3(Vec3::from(*p)) * w[k];
                }
            }
            let c = to_cam.transform_point3(out);
            (lo, hi) = (lo.min(c), hi.max(c));
        }
    }
    println!("  skinned mesh bounds in camera space: {lo:7.2} .. {hi:7.2}");
}
