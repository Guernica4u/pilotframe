//! Weapon sway and bob, ported from `client.dll` (docs/port/weapons.md, "Sway and
//! bob"). The game uses the same code for the gun (from the weapon script's
//! `sway_*` and `bob_*` settings) and for the pilot's cockpit, which carries
//! the helmet HUD ([`crate::cockpit_sway`]).
//!
//! Each frame the update eases an offset and three angles towards targets made
//! from the player's movement and turning, and runs a walking bob. The result
//! is applied as a rotation about a pivot (an attachment on the gun model)
//! plus the offset.
//!
//! Axes and units are the game's: offsets in inches along (forward, left, up),
//! angles in degrees (pitch down, yaw left, roll right).

use bevy::math::{Affine3A, Quat, Vec3};

use crate::loadout::WeaponScript;

/// Turn rates are averaged over this many frames (a ring of 5, each × 0.2).
const TURN_SAMPLES: usize = 5;
/// The aimed-in angle limits are fully in by this fraction of the way into
/// aiming (the game scales the aim fraction by 20).
const ZOOM_LIMIT_SCALE: f32 = 20.0;

/// The directions a block has factors for, in the game's order. Each has six
/// factors: translate x, y, z, rotate pitch, yaw, roll.
#[derive(Clone, Copy, Debug)]
pub enum Dir {
    MoveForward,
    MoveBack,
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    TurnLeft,
    TurnRight,
    TurnUp,
    TurnDown,
}

const DIR_NAMES: [&str; 10] = [
    "move_forward",
    "move_back",
    "move_left",
    "move_right",
    "move_up",
    "move_down",
    "turn_left",
    "turn_right",
    "turn_up",
    "turn_down",
];
const PART_NAMES: [&str; 6] = [
    "translate_x",
    "translate_y",
    "translate_z",
    "rotate_pitch",
    "rotate_yaw",
    "rotate_roll",
];

/// One set of sway and bob settings (the game keeps one for hip fire and one
/// for aiming).
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    /// The attachment the sway rotates about, and how long the pivot takes to
    /// move to this block's when switching.
    pub rotate_attach: String,
    pub rotate_attach_blend_time: f32,
    /// Limits for (x, y, z, pitch, yaw, roll).
    pub min: [f32; 6],
    pub max: [f32; 6],
    pub translate_gain: f32,
    pub rotate_gain: f32,
    pub bob_cycle_time: f32,
    pub bob_min_speed: f32,
    pub bob_max_speed: f32,
    pub bob_vert_dist: f32,
    pub bob_horz_dist: f32,
    pub bob_pitch: f32,
    pub bob_yaw: f32,
    pub bob_roll: f32,
    pub bob_gain: f32,
    pub dirs: [[f32; 6]; 10],
}

impl Block {
    /// A fresh block as the game initialises one (`0x18047f030`): no limits,
    /// both gains 12, no bob, no factors.
    pub fn blank() -> Self {
        Self {
            rotate_attach: String::new(),
            rotate_attach_blend_time: 0.0,
            min: [f32::MIN; 6],
            max: [f32::MAX; 6],
            translate_gain: 12.0,
            rotate_gain: 12.0,
            bob_cycle_time: 0.0,
            bob_min_speed: 0.1,
            bob_max_speed: 0.2,
            bob_vert_dist: 0.0,
            bob_horz_dist: 0.0,
            bob_pitch: 0.0,
            bob_yaw: 0.0,
            bob_roll: 0.0,
            bob_gain: 12.0,
            dirs: [[0.0; 6]; 10],
        }
    }

    pub fn dir(&mut self, d: Dir) -> &mut [f32; 6] {
        &mut self.dirs[d as usize]
    }

    /// A block from a weapon script, as the weapon loader reads it
    /// (`0x18047ddd0`): keys with `suffix` (`""` or `"_zoomed"`), missing ones
    /// defaulting to 0, except the gains (`gain` for translate/rotate, 12 for
    /// bob) and the bob speeds (0.1, 0.2).
    fn from_script(w: &WeaponScript, suffix: &str, gain: f32) -> Self {
        let num = |key: &str, default: f32| w.num(&format!("{key}{suffix}")).unwrap_or(default);
        let mut b = Self::blank();
        b.rotate_attach = w
            .text(&format!("sway_rotate_attach{suffix}"))
            .unwrap_or_default()
            .to_string();
        b.rotate_attach_blend_time = num("sway_rotate_attach_blend_time", 0.0);
        for (i, axis) in ["x", "y", "z", "pitch", "yaw", "roll"].iter().enumerate() {
            b.min[i] = num(&format!("sway_min_{axis}"), 0.0);
            b.max[i] = num(&format!("sway_max_{axis}"), 0.0);
        }
        b.translate_gain = num("sway_translate_gain", gain);
        b.rotate_gain = num("sway_rotate_gain", gain);
        b.bob_cycle_time = num("bob_cycle_time", 0.0);
        b.bob_min_speed = num("bob_min_speed", 0.1);
        b.bob_max_speed = num("bob_max_speed", 0.2);
        b.bob_vert_dist = num("bob_vert_dist", 0.0);
        b.bob_horz_dist = num("bob_horz_dist", 0.0);
        b.bob_pitch = num("bob_pitch", 0.0);
        b.bob_yaw = num("bob_yaw", 0.0);
        b.bob_roll = num("bob_roll", 0.0);
        b.bob_gain = num("bob_gain", 12.0);
        for (d, dir) in DIR_NAMES.iter().enumerate() {
            for (p, part) in PART_NAMES.iter().enumerate() {
                b.dirs[d][p] = num(&format!("sway_{dir}_{part}"), 0.0);
            }
        }
        b
    }
}

/// A weapon's (or the cockpit's) sway settings: hip and aimed blocks.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub hip: Block,
    pub zoomed: Block,
    /// `sway_enable_zoomed`: use the aimed block while aiming (default on).
    pub zoomed_enabled: bool,
}

impl Settings {
    /// From the weapon script. The aimed block's gains default to 24.
    pub fn from_script(w: &WeaponScript) -> Self {
        Self {
            hip: Block::from_script(w, "", 12.0),
            zoomed: Block::from_script(w, "_zoomed", 24.0),
            zoomed_enabled: w.num("sway_enable_zoomed").is_none_or(|v| v != 0.0),
        }
    }
}

/// What the update needs to know about the player this frame.
#[derive(Clone, Copy, Debug)]
pub struct Input {
    pub dt: f32,
    /// Velocity in the view's (forward, left, up), inches per second.
    pub velocity: [f32; 3],
    /// View angles (pitch down, yaw left, roll), degrees.
    pub view: [f32; 3],
    /// Zoomed in (the player's zoom state, on while aiming).
    pub zoomed: bool,
    /// How far into aiming, 0..1.
    pub zoom_fraction: f32,
    /// On the ground (the bob only runs there).
    pub on_ground: bool,
}

/// The sway's state.
#[derive(Clone, Debug, Default)]
pub struct Sway {
    /// Sway offset (forward, left, up) and angles (pitch, yaw, roll).
    pub origin: [f32; 3],
    pub angles: [f32; 3],
    /// Bob offset and angles, added on top.
    pub bob_origin: [f32; 3],
    pub bob_angles: [f32; 3],
    last_view: Option<[f32; 3]>,
    rates: [[f32; 3]; TURN_SAMPLES],
    next: usize,
    bob_phase: f32,
    /// Eased bob waves: |sin| at twice the step rate, sin at the step rate,
    /// vertical and horizontal distance.
    bob: [f32; 4],
}

/// `a - b` as an angle in -180..180 degrees.
pub fn angle_diff(a: f32, b: f32) -> f32 {
    (a - b + 180.0).rem_euclid(360.0) - 180.0
}

fn normalize(a: f32) -> f32 {
    angle_diff(a, 0.0)
}

/// `x` from `lo..hi` to 0..1, clamped.
fn remap_clamped(x: f32, lo: f32, hi: f32) -> f32 {
    if lo == hi {
        return if x >= hi { 1.0 } else { 0.0 };
    }
    ((x - lo) / (hi - lo)).clamp(0.0, 1.0)
}

impl Sway {
    /// One frame (`0x18047f1e0`).
    pub fn update(&mut self, s: &Settings, input: &Input) {
        let dt = input.dt;
        if dt <= 0.0 {
            return;
        }
        let b = if input.zoomed && s.zoomed_enabled {
            &s.zoomed
        } else {
            &s.hip
        };
        let [forward, left, up] = input.velocity;
        let ease = |rate: f32| (-rate * dt).exp();

        // The walking bob: a phase that runs with speed while on the ground,
        // and settles on a whole step when not.
        let speed = (forward * forward + left * left).sqrt();
        let step = remap_clamped(speed, b.bob_min_speed, b.bob_max_speed);
        let cycle = b.bob_cycle_time;
        let waves = if !input.on_ground || cycle <= 0.0 || step <= 0.0 {
            let target = if self.bob_phase > cycle * 1.5 {
                cycle * 2.0
            } else if self.bob_phase < cycle * 0.5 {
                0.0
            } else {
                cycle
            };
            let e = ease(b.bob_gain);
            self.bob_phase = (1.0 - e) * target + e * self.bob_phase;
            [0.0; 4]
        } else {
            self.bob_phase = (step * dt + self.bob_phase).rem_euclid(cycle * 2.0);
            let tau = std::f32::consts::TAU;
            let short = (self.bob_phase.rem_euclid(cycle) * tau / cycle).sin();
            let long = (self.bob_phase * tau / (cycle * 2.0)).sin();
            [short, long, short * b.bob_vert_dist, long * b.bob_horz_dist]
        };
        let e = ease(b.bob_gain);
        for (v, target) in self.bob.iter_mut().zip(waves) {
            *v = (1.0 - e) * target + e * *v;
        }
        self.bob_origin = [0.0, self.bob[3], self.bob[2]];
        self.bob_angles = [
            self.bob[0] * b.bob_pitch,
            self.bob[1] * b.bob_yaw,
            self.bob[1] * b.bob_roll,
        ];

        // Turn rates: this frame's angle change over the frame time, averaged
        // over the last few frames.
        let last = self.last_view.unwrap_or(input.view);
        self.next = (self.next + 1) % TURN_SAMPLES;
        self.rates[self.next] = [0, 1, 2].map(|i| angle_diff(input.view[i], last[i]) / dt);
        self.last_view = Some(input.view);
        let average = |i: usize| self.rates.iter().map(|r| r[i]).sum::<f32>() / TURN_SAMPLES as f32;
        // The game scales the turn rate by a frame time from its globals; we
        // take it to be this frame's.
        let (pitch_rate, yaw_rate) = (average(0) * dt, average(1) * dt);

        // Targets: each input adds |input| × the factors for its direction.
        let mut target = [0.0f32; 6];
        let mut add = |amount: f32, positive: Dir, negative: Dir| {
            let dir = if amount < 0.0 { negative } else { positive };
            for (t, f) in target.iter_mut().zip(b.dirs[dir as usize]) {
                *t += amount.abs() * f;
            }
        };
        add(forward * dt, Dir::MoveForward, Dir::MoveBack);
        add(left * dt, Dir::MoveLeft, Dir::MoveRight);
        add(up * dt, Dir::MoveUp, Dir::MoveDown);
        add(yaw_rate, Dir::TurnLeft, Dir::TurnRight);
        // Pitch is positive looking down.
        add(-pitch_rate, Dir::TurnUp, Dir::TurnDown);

        let (keep_t, keep_r) = (ease(b.translate_gain), ease(b.rotate_gain));
        for i in 0..3 {
            let t = target[i].max(b.min[i]).min(b.max[i]);
            self.origin[i] = (1.0 - keep_t) * t + keep_t * self.origin[i];
            let t = normalize(target[i + 3]).max(b.min[i + 3]).min(b.max[i + 3]);
            self.angles[i] += (1.0 - keep_r) * angle_diff(t, self.angles[i]);
        }

        // Aiming narrows the angle limits from the hip ones to the aimed ones
        // over the first part of the zoom.
        if s.zoomed_enabled {
            let k = (input.zoom_fraction * ZOOM_LIMIT_SCALE).clamp(0.0, 1.0);
            for i in 0..3 {
                let lo = s.hip.min[i + 3] + (s.zoomed.min[i + 3] - s.hip.min[i + 3]) * k;
                let hi = s.hip.max[i + 3] + (s.zoomed.max[i + 3] - s.hip.max[i + 3]) * k;
                self.angles[i] = self.angles[i].max(lo).min(hi);
            }
        }
    }

    /// The total offset and angles to apply (`0x18047fed0`): sway plus bob.
    pub fn total(&self) -> ([f32; 3], [f32; 3]) {
        (
            [0, 1, 2].map(|i| self.origin[i] + self.bob_origin[i]),
            [0, 1, 2].map(|i| self.angles[i] + self.bob_angles[i]),
        )
    }
}

/// The player's velocity and view in the game's terms, from Bevy's: velocity
/// (Y up) and view yaw/pitch in radians (yaw 0 faces -Z and turns left, pitch
/// looks up). Returns velocity in the view's (forward, left, up) and the view
/// angles (pitch down, yaw left, roll) in degrees.
pub fn view_input(velocity: Vec3, yaw: f32, pitch: f32) -> ([f32; 3], [f32; 3]) {
    let view = Quat::from_euler(bevy::math::EulerRot::YXZ, yaw, pitch, 0.0);
    // Bevy's view space is x right, y up, -z forward.
    let local = view.inverse() * velocity;
    (
        [-local.z, -local.x, local.y],
        [-pitch.to_degrees(), yaw.to_degrees(), 0.0],
    )
}

/// The game's rotation for (pitch down, yaw left, roll right) in degrees, in
/// its model axes (x forward, y left, z up).
pub fn rotation(angles: [f32; 3]) -> Quat {
    let [pitch, yaw, roll] = angles.map(f32::to_radians);
    Quat::from_rotation_z(yaw) * Quat::from_rotation_y(pitch) * Quat::from_rotation_x(roll)
}

/// The sway as a transform in the gun model's space (`0x18047fed0`): rotate
/// about `pivot` by the angles, then move by the offset.
pub fn transform(sway: &Sway, pivot: Affine3A) -> Affine3A {
    let (origin, angles) = sway.total();
    Affine3A::from_translation(Vec3::from(origin))
        * pivot
        * Affine3A::from_quat(rotation(angles))
        * pivot.inverse()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(velocity: [f32; 3], view: [f32; 3]) -> Input {
        Input {
            dt: 1.0 / 60.0,
            velocity,
            view,
            zoomed: false,
            zoom_fraction: 0.0,
            on_ground: true,
        }
    }

    fn r201_like() -> Settings {
        let mut hip = Block::blank();
        hip.min = [-0.3, -0.5, -0.5, -3.0, -3.5, -2.0];
        hip.max = [0.3, 0.5, 0.1, 3.0, 3.5, 3.0];
        hip.translate_gain = 10.0;
        hip.dir(Dir::MoveBack)[0] = -2.0;
        hip.dir(Dir::TurnLeft)[4] = -1.0;
        hip.dir(Dir::TurnRight)[4] = 1.0;
        hip.bob_cycle_time = 0.45;
        hip.bob_max_speed = 150.0;
        hip.bob_vert_dist = 0.075;
        let mut zoomed = Block::blank();
        zoomed.min = [0.0, 0.0, 0.0, -0.01, -0.03, 0.0];
        zoomed.max = [0.0, 0.0, 0.0, 0.01, 0.03, 0.0];
        Settings {
            hip,
            zoomed,
            zoomed_enabled: true,
        }
    }

    #[test]
    fn standing_still_stays_put() {
        let s = r201_like();
        let mut sway = Sway::default();
        for _ in 0..60 {
            sway.update(&s, &input([0.0; 3], [0.0, 90.0, 0.0]));
        }
        assert_eq!(sway.total(), ([0.0; 3], [0.0; 3]));
    }

    #[test]
    fn backing_up_moves_the_gun_back_within_its_limit() {
        let s = r201_like();
        let mut sway = Sway::default();
        for _ in 0..120 {
            sway.update(&s, &input([-300.0, 0.0, 0.0], [0.0; 3]));
        }
        // 5 inches a frame × the back factor -2 = -10, clamped to -0.3.
        assert!((sway.origin[0] + 0.3).abs() < 1e-3, "{:?}", sway.origin);
    }

    #[test]
    fn turning_left_swings_the_gun_right_and_aiming_narrows_it() {
        let s = r201_like();
        let mut sway = Sway::default();
        let mut yaw = 0.0;
        for _ in 0..120 {
            yaw += 1.0;
            sway.update(&s, &input([0.0; 3], [0.0, yaw, 0.0]));
        }
        assert!((sway.angles[1] + 1.0).abs() < 0.01, "{:?}", sway.angles);
        let mut aimed = input([0.0; 3], [0.0, yaw + 1.0, 0.0]);
        aimed.zoomed = true;
        aimed.zoom_fraction = 1.0;
        sway.update(&s, &aimed);
        assert!(sway.angles[1].abs() <= 0.03 + 1e-6, "{:?}", sway.angles);
    }

    #[test]
    fn running_bobs_and_settles_when_stopping() {
        let s = r201_like();
        let mut sway = Sway::default();
        let mut highest = 0.0f32;
        for _ in 0..120 {
            sway.update(&s, &input([250.0, 0.0, 0.0], [0.0; 3]));
            highest = highest.max(sway.bob_origin[2].abs());
        }
        assert!(highest > 0.03 && highest <= 0.075, "{highest}");
        for _ in 0..240 {
            sway.update(&s, &input([0.0; 3], [0.0; 3]));
        }
        assert!(sway.bob_origin[2].abs() < 1e-3);
    }
}
