//! Movement replay (a dev tool, ours): run a recording of the real game's
//! inputs through our movement code and write what we do, frame for frame,
//! in the same format, so the two can be compared (`movement_compare.py`).
//!
//! `PF_REPLAY=game.csv [PF_MAP=colony] pf-game` runs headless and writes
//! `game.ours.csv` next to the recording, then exits. The recording comes from
//! `telemetry.py record` (docs/tools.md): per game frame, the pilot's position
//! and velocity (Source axes, Z up), flags, buttons and view angles.
//!
//! Button bits, measured in the game with `telemetry.py keys` (2026-10-12):
//! attack 1, jump 2, forward 8, back 16, left 512, right 1024, sprint 1 << 17,
//! aim (zoom) 1 << 19, crouch 1 << 27. Crouch and aim are not Source's usual
//! IN_DUCK 4 and IN_ATTACK2 2048.

use std::path::{Path, PathBuf};

use bevy::math::{Vec2, Vec3};
use pf_movement::{Mode, MoveInput, MovementState, MovementTuning, StaticWorld, step};

const IN_JUMP: i64 = 1 << 1;
const IN_DUCK: i64 = 1 << 27;
const IN_FORWARD: i64 = 1 << 3;
const IN_BACK: i64 = 1 << 4;
const IN_MOVELEFT: i64 = 1 << 9;
const IN_MOVERIGHT: i64 = 1 << 10;
const IN_ZOOM: i64 = 1 << 19;
const IN_SPEED: i64 = 1 << 17;

/// One recorded game frame.
struct Frame {
    t: f64,
    origin: [f32; 3],
    velocity: [f32; 3],
    flags: i64,
    buttons: i64,
    pitch: f32,
    yaw: f32,
}

fn read_recording(path: &Path) -> Result<Vec<Frame>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().ok_or("empty recording")?.split(',').collect();
    let col = |name: &str| {
        header
            .iter()
            .position(|h| h.trim() == name)
            .ok_or(format!("no column {name}"))
    };
    let c = [
        col("t")?,
        col("ox")?,
        col("oy")?,
        col("oz")?,
        col("vx")?,
        col("vy")?,
        col("vz")?,
        col("flags")?,
        col("buttons")?,
        col("pitch")?,
        col("yaw")?,
    ];
    let mut out = Vec::new();
    for line in lines {
        let f: Vec<&str> = line.split(',').collect();
        let n = |i: usize| f.get(c[i]).and_then(|v| v.trim().parse::<f64>().ok());
        let (Some(t), Some(ox), Some(oy), Some(oz)) = (n(0), n(1), n(2), n(3)) else {
            continue;
        };
        out.push(Frame {
            t,
            origin: [ox as f32, oy as f32, oz as f32],
            velocity: [
                n(4).unwrap_or(0.0) as f32,
                n(5).unwrap_or(0.0) as f32,
                n(6).unwrap_or(0.0) as f32,
            ],
            flags: n(7).unwrap_or(0.0) as i64,
            buttons: n(8).unwrap_or(0.0) as i64,
            pitch: n(9).unwrap_or(0.0) as f32,
            yaw: n(10).unwrap_or(0.0) as f32,
        });
    }
    if out.is_empty() {
        return Err("no frames in the recording".into());
    }
    Ok(out)
}

/// Source axes (Z up) to ours (Y up), as the map loader converts them.
fn to_ours(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], p[2], -p[1])
}

fn to_source(v: Vec3) -> [f32; 3] {
    [v.x, -v.z, v.y]
}

/// Source yaw (degrees, 0 = +X, counter-clockwise from above) to ours
/// (radians, 0 = facing -Z, which is Source +Y).
fn yaw_to_ours(yaw: f32) -> f32 {
    (yaw - 90.0).to_radians()
}

fn yaw_to_source(yaw: f32) -> f32 {
    yaw.to_degrees() + 90.0
}

/// Run the replay if `PF_REPLAY` is set; returns true if it ran (the caller
/// then exits instead of starting the game).
pub fn run_if_requested() -> bool {
    let Some(path) = std::env::var_os("PF_REPLAY").map(PathBuf::from) else {
        return false;
    };
    match replay(&path) {
        Ok(out) => eprintln!("replay written to {}", out.display()),
        Err(e) => eprintln!("replay failed: {e}"),
    }
    true
}

fn load_tuning() -> Result<MovementTuning, String> {
    let root = std::env::var("BEVY_ASSET_ROOT").unwrap_or_else(|_| "crates/pf-game".into());
    let path = Path::new(&root).join("assets/movement.ron");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    ron::from_str(&text).map_err(|e| format!("movement.ron: {e}"))
}

fn world() -> Result<StaticWorld, String> {
    match std::env::var("PF_MAP").as_deref() {
        Ok("colony") => {
            let map = pf_world::load("mp_colony").map_err(|e| e.to_string())?;
            Ok(crate::level::map_collision(&map))
        }
        _ => Ok(crate::map::test_map_collision()),
    }
}

fn replay(path: &Path) -> Result<PathBuf, String> {
    let frames = read_recording(path)?;
    let tuning = load_tuning()?;
    let world = world()?;
    let dt = 1.0 / crate::TICK_HZ as f32;
    // The recording doesn't say which gun was held; assume our default
    // loadout's (record with the same weapon, or the speeds won't match).
    let weapon_speed = crate::viewmodel::default_move_speed();

    let first = &frames[0];
    let mut state = MovementState::new(to_ours(first.origin));
    state.velocity = to_ours(first.velocity);
    state.on_ground = first.flags & 1 != 0;

    let out_path = path.with_extension("ours.csv");
    let mut out = String::from("t,ox,oy,oz,vx,vy,vz,flags,buttons,pitch,yaw,mode\n");
    let end = frames.last().map_or(0.0, |f| f.t);
    let (mut i, mut tick) = (0usize, 0u64);
    let mut last_buttons = first.buttons;
    loop {
        let t = first.t + tick as f64 * dt as f64;
        if t > end {
            break;
        }
        // The input held at this tick: the latest frame at or before it
        // (with a little slack for times rounded in the file).
        while i + 1 < frames.len() && frames[i + 1].t <= t + 1e-4 {
            i += 1;
        }
        let f = &frames[i];
        let held = |bit: i64| f.buttons & bit != 0;
        let axis = |pos: i64, neg: i64| f32::from(held(pos)) - f32::from(held(neg));
        let input = MoveInput {
            wish: Vec2::new(axis(IN_MOVERIGHT, IN_MOVELEFT), axis(IN_FORWARD, IN_BACK)),
            yaw: yaw_to_ours(f.yaw),
            pitch: -f.pitch.to_radians(),
            key_pressed: (f.buttons & !last_buttons)
                & (IN_FORWARD | IN_BACK | IN_MOVELEFT | IN_MOVERIGHT)
                != 0,
            jump: f.buttons & IN_JUMP != 0 && last_buttons & IN_JUMP == 0,
            sprint: held(IN_SPEED),
            crouch: held(IN_DUCK),
            zoom: held(IN_ZOOM),
            weapon_speed,
        };
        last_buttons = f.buttons;
        state = step(&state, &input, &tuning, &world, dt);
        let o = to_source(state.position);
        let v = to_source(state.velocity);
        let mode = match state.mode {
            Mode::Free => "free",
            Mode::WallRun { .. } => "wallrun",
            Mode::WallHang { .. } => "wallhang",
            Mode::Mantle { .. } => "mantle",
        };
        out.push_str(&format!(
            "{:.5},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{},{},{:.3},{:.3},{}\n",
            t - first.t,
            o[0],
            o[1],
            o[2],
            v[0],
            v[1],
            v[2],
            i64::from(state.on_ground),
            f.buttons,
            -input.pitch.to_degrees(),
            yaw_to_source(input.yaw),
            mode
        ));
        tick += 1;
    }
    std::fs::write(&out_path, out).map_err(|e| format!("{}: {e}", out_path.display()))?;
    Ok(out_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axes_round_trip() {
        let p = [1.0, 2.0, 3.0];
        assert_eq!(to_source(to_ours(p)), p);
        assert!((yaw_to_source(yaw_to_ours(37.0)) - 37.0).abs() < 1e-4);
        // Source yaw 90 (facing +Y) is our yaw 0 (facing -Z).
        assert!(yaw_to_ours(90.0).abs() < 1e-6);
    }
}
