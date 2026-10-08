//! Walk the movement code across a real map's collision and print where it gets
//! stuck. A debugging aid for map collision (stairs, lips, seams).
//!
//! `cargo run -p pf-movement --release --example walk_map -- <map.bsp> <x> <y> <z> <yaw°> [seconds]`
//! Coordinates are the map's own (Source, Z up); yaw 0 walks along +X.

use glam::{Vec2, Vec3};
use pf_movement::{MoveInput, MovementState, MovementTuning, StaticWorld, step};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let num = |i: usize| args[i].parse::<f32>().unwrap();
    let data = std::fs::read(&args[0]).unwrap();
    let bsp = pf_fmt_bsp::Bsp::parse(&data).unwrap();
    let g = bsp.geometry().unwrap();
    // Source (x, y, z) to Y up: (x, z, -y).
    let to = |p: [f32; 3]| Vec3::new(p[0], p[2], -p[1]);
    let points: Vec<Vec3> = g.positions.iter().map(|&p| to(p)).collect();
    let tris: Vec<[u32; 3]> = g
        .meshes
        .iter()
        .filter(|m| {
            !g.materials[m.material as usize]
                .to_ascii_lowercase()
                .starts_with("tools")
        })
        .flat_map(|m| m.triangles.iter().map(|t| t.map(|c| c.position)))
        .collect();
    let mut world = StaticWorld::new();
    world.add_trimesh(&points, &tris);
    let tuning: MovementTuning =
        ron::from_str(&std::fs::read_to_string("crates/pf-game/assets/movement.ron").unwrap())
            .unwrap();

    let start = to([num(1), num(2), num(3)]);
    // Source yaw 0 = +X; ours: yaw 0 faces -Z, +90° faces -X, so turn by -90°.
    let yaw = (num(4) - 90.0).to_radians();
    let seconds = args.get(5).map_or(3.0, |s| s.parse().unwrap());
    let input = MoveInput {
        wish: Vec2::Y,
        yaw,
        ..Default::default()
    };
    let mut s = MovementState::new(start + Vec3::Y * 2.0);
    let dt = 1.0 / 60.0;
    let quiet = std::env::var("QUIET").is_ok();
    // Once walking at full speed: count airborne ticks and sudden slowdowns.
    let (mut landed, mut air, mut stops, mut full) = (false, 0, 0, false);
    for tick in 0..(seconds / dt) as usize {
        let before = s.position;
        let prev_speed = Vec2::new(s.velocity.x, s.velocity.z).length();
        s = step(&s, &input, &tuning, &world, dt);
        let speed = Vec2::new(s.velocity.x, s.velocity.z).length();
        landed |= s.on_ground;
        full |= speed > tuning.walk_speed - 1.0;
        if landed && full && !s.on_ground && s.mode == pf_movement::Mode::Free {
            air += 1;
            if quiet && std::env::var("AIR").is_ok() {
                println!(
                    "  air at ({:.0}, {:.0}, {:.1}) vy {:.1}",
                    s.position.x, -s.position.z, s.position.y, s.velocity.y
                );
            }
        }
        if full && prev_speed - speed > 40.0 {
            stops += 1;
            if quiet {
                println!(
                    "  slowdown {prev_speed:.0} -> {speed:.0} at ({:.0}, {:.0}, {:.0}) {:?}",
                    s.position.x, -s.position.z, s.position.y, s.mode
                );
            }
        }
        if !quiet && tick % 6 == 0 {
            let speed = Vec2::new(s.velocity.x, s.velocity.z).length();
            println!(
                "t {:4.2} source pos ({:7.1}, {:7.1}, {:6.1}) speed {speed:5.0} vy {:6.1} ground {} moved {:5.2} {:?}",
                tick as f32 * dt,
                s.position.x,
                -s.position.z,
                s.position.y,
                s.velocity.y,
                s.on_ground,
                (s.position - before).length(),
                s.mode
            );
        }
    }
    println!(
        "end ({:.0}, {:.0}, {:.0}): {air} airborne ticks while walking, {stops} sudden slowdowns",
        s.position.x, -s.position.z, s.position.y
    );
}
