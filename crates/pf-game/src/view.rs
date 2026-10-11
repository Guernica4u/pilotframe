//! First-person view effects: view punches (the kick on jumping, landing, starting
//! a wall-run or hang) and the sprint turn tilt.
//!
//! Values are the engine's own defaults read from the owner's `client.dll`, listed
//! in docs/port/movement/r1-engine-values.md ("View"), and the way a punch moves is the
//! game's (read from its code, and checked against a recording of the real game):
//! a punch adds 20 × its angles to an angular speed, and a damped spring pulls
//! the view back, solved exactly over each frame.
//!
//! There are two springs, as in the game: the view punch, and the weapon kick
//! (recoil, [`crate::recoil`]; movement punches add 0.2 × to it too). The kick
//! spring uses the weapon's own constants. The camera shows the view punch plus
//! the share of the kick that isn't the gun's; the gun shows its share.

use bevy::prelude::*;

use crate::recoil::SpringConstants;

/// A punch of 1° adds this much angular speed, degrees/s.
const PUNCH_TO_SPEED: f32 = 20.0;
/// Below this (squared angle and squared speed) the spring stops.
const SPRING_REST: f32 = 0.001;
/// The gun's share of each punch (`viewpunch_jump_scaleWeapon`).
const WEAPON_SCALE: f32 = 0.2;

/// Sprint tilt (`sprinttilt_*`): turning this many degrees per second or more
/// gives the full tilt; the tilt fraction changes at most `MAX_VEL` per second,
/// with `ACCEL` per second².
const SPRINT_TILT_TURN_RANGE: f32 = 120.0;
const SPRINT_TILT_ACCEL: f32 = 35.0;
const SPRINT_TILT_MAX_VEL: f32 = 2.0;

/// A punch: pitch, yaw, roll in degrees, each `base + random × (-0.5..0.5)`. Source
/// convention: positive pitch kicks the view down, positive roll tilts right.
pub struct Punch {
    pub base: Vec3,
    pub random: Vec3,
}

/// `viewpunch_jump_*`.
pub const JUMP: Punch = Punch {
    base: Vec3::new(2.0, 0.0, 0.9),
    random: Vec3::new(1.0, 1.0, 0.5),
};
/// `viewpunch_doublejump_*`.
pub const DOUBLE_JUMP: Punch = Punch {
    base: Vec3::new(4.5, 0.0, 0.0),
    random: Vec3::new(1.0, 1.0, 2.0),
};
/// `viewpunch_wallrun_*` (roll towards the wall side is applied by the caller).
pub const WALLRUN: Punch = Punch {
    base: Vec3::new(-1.5, 0.0, 5.0),
    random: Vec3::new(0.5, 0.5, 0.5),
};
/// `viewpunch_wallhangStart_*`.
pub const WALLHANG: Punch = Punch {
    base: Vec3::new(4.0, 2.0, 4.0),
    random: Vec3::new(1.0, 1.0, 0.5),
};
/// `viewpunch_land_*`, scaled by how hard the landing was.
pub const LAND: Punch = Punch {
    base: Vec3::new(1.5, 0.0, 0.0),
    random: Vec3::new(0.5, 0.0, 1.0),
};

/// A punch angle and its angular speed: pitch, yaw, roll in degrees (Source
/// convention).
#[derive(Default)]
struct Spring {
    angle: Vec3,
    speed: Vec3,
}

impl Spring {
    /// Advance by `dt` seconds (`0x18015ea60`): once angle and speed are both
    /// tiny the spring stops; otherwise each axis is solved exactly
    /// ([`solve`]) with its own constants.
    fn advance(&mut self, c: &SpringConstants, dt: f32) {
        if self.angle.length_squared() <= SPRING_REST && self.speed.length_squared() <= SPRING_REST
        {
            *self = Self::default();
            return;
        }
        for i in 0..3 {
            (self.angle[i], self.speed[i]) = solve(
                self.angle[i],
                self.speed[i],
                c.stiffness[i],
                c.damping[i],
                dt,
            );
        }
    }

    /// In radians, converted from Source's convention to ours: (pitch up, yaw left,
    /// roll left), plus `extra_roll` degrees.
    fn offsets(&self, extra_roll: f32) -> Vec3 {
        Vec3::new(-self.angle.x, self.angle.y, -(self.angle.z + extra_roll)) * std::f32::consts::PI
            / 180.0
    }
}

/// A damped spring x'' + damping·x' + stiffness·x = 0 moved on by `t` seconds
/// exactly, from angle `x` and speed `v` (`0x1804fe210`): critically damped
/// when damping² − 4·stiffness is within ±1e-6 of 0, else over- or underdamped.
/// Exact, so it stays stable however stiff the spring (the R-201's roll spring
/// is 12,500–20,000).
fn solve(x: f32, v: f32, stiffness: f32, damping: f32, t: f32) -> (f32, f32) {
    let disc = damping * damping - 4.0 * stiffness;
    let a = damping * 0.5;
    if disc > 1e-6 {
        // Two real rates.
        let root = disc.sqrt() * 0.5;
        let (r1, r2) = (-a + root, -a - root);
        let c1 = (v - r2 * x) / (r1 - r2);
        let c2 = x - c1;
        let (e1, e2) = ((r1 * t).exp(), (r2 * t).exp());
        (c1 * e1 + c2 * e2, r1 * c1 * e1 + r2 * c2 * e2)
    } else if disc < -1e-6 {
        // Oscillating, decaying at damping / 2.
        let w = (-disc).sqrt() * 0.5;
        let decay = (-a * t).exp();
        let (sin, cos) = (w * t).sin_cos();
        let b = (v + a * x) / w;
        (
            decay * (x * cos + b * sin),
            decay * (v * cos - (a * v + stiffness * x) / w * sin),
        )
    } else {
        let b = v + a * x;
        let decay = (-a * t).exp();
        let line = x + b * t;
        (line * decay, (b - line * a) * decay)
    }
}

/// The view punch and weapon kick springs, plus the sprint tilt.
pub struct ViewEffects {
    view: Spring,
    weapon: Spring,
    /// The kick spring's constants, and how the kick splits between the view
    /// and the gun (the gun's fraction, and how much of it the gun shows):
    /// set from the held weapon each frame ([`Self::set_weapon`]).
    kick_spring: SpringConstants,
    weapon_fraction: f32,
    weapon_vm_scale: f32,
    sprint_tilt: f32,
    sprint_tilt_speed: f32,
    rng: u32,
}

impl Default for ViewEffects {
    fn default() -> Self {
        Self {
            view: Spring::default(),
            weapon: Spring::default(),
            kick_spring: SpringConstants::BASE,
            weapon_fraction: 0.0,
            weapon_vm_scale: 0.0,
            sprint_tilt: 0.0,
            sprint_tilt_speed: 0.0,
            rng: 0,
        }
    }
}

impl ViewEffects {
    /// The held weapon's kick spring and view/gun split for this frame.
    pub fn set_weapon(&mut self, spring: SpringConstants, fraction: f32, vm_scale: f32) {
        self.kick_spring = spring;
        self.weapon_fraction = fraction;
        self.weapon_vm_scale = vm_scale;
    }

    /// A weapon kick (`0x18003c840`): `soft` is added to the kick spring's
    /// speed (× 20), `hard` to its angle. Degrees, Source convention.
    pub fn kick(&mut self, soft: Vec3, hard: Vec3) {
        self.weapon.speed += soft * PUNCH_TO_SPEED;
        self.weapon.angle += hard;
    }

    /// Kick the view and the gun. `side` scales the base yaw and roll (e.g. how
    /// much the wall is to one side, -1..1); `scale` scales the whole punch.
    /// `weapon_roll` false leaves the gun's roll alone (the wall-run punch does).
    pub fn punch(&mut self, punch: &Punch, side: f32, scale: f32, weapon_roll: bool) {
        let r = Vec3::new(self.random(), self.random(), self.random());
        let base = punch.base * Vec3::new(1.0, side, side);
        let kick = (base + punch.random * r) * scale;
        self.view.speed += kick * PUNCH_TO_SPEED;
        let mut gun = kick * WEAPON_SCALE;
        if !weapon_roll {
            gun.z = 0.0;
        }
        self.weapon.speed += gun * PUNCH_TO_SPEED;
    }

    /// Advance the springs by `dt` seconds. `turn_rate` is degrees/s of yaw (left
    /// positive); `sprinting` enables the turn tilt.
    pub fn update(&mut self, dt: f32, turn_rate: f32, sprinting: bool) {
        self.view.advance(&SpringConstants::BASE, dt);
        self.weapon.advance(&self.kick_spring, dt);

        let target = if sprinting {
            (-turn_rate / SPRINT_TILT_TURN_RANGE).clamp(-1.0, 1.0)
        } else {
            0.0
        };
        // Head for the target at up to MAX_VEL, changing speed at up to ACCEL.
        let want =
            ((target - self.sprint_tilt) * 10.0).clamp(-SPRINT_TILT_MAX_VEL, SPRINT_TILT_MAX_VEL);
        let change =
            (want - self.sprint_tilt_speed).clamp(-SPRINT_TILT_ACCEL * dt, SPRINT_TILT_ACCEL * dt);
        self.sprint_tilt_speed += change;
        self.sprint_tilt = (self.sprint_tilt + self.sprint_tilt_speed * dt).clamp(-1.0, 1.0);
    }

    /// Offsets to add to the camera, in radians: (pitch up, yaw left, roll left).
    /// `sprint_max_roll` is the pilot's `sprinttiltMaxRoll` in degrees. The view
    /// punch, plus the kick's pitch and yaw that aren't the gun's and all of its
    /// roll (`0x18003ba40`).
    pub fn offsets(&self, sprint_max_roll: f32) -> Vec3 {
        let view_share = 1.0 - self.weapon_fraction;
        let kick = self.weapon.angle * Vec3::new(view_share, view_share, 1.0);
        Spring {
            angle: self.view.angle + kick,
            speed: Vec3::ZERO,
        }
        .offsets(self.sprint_tilt * sprint_max_roll)
    }

    /// How far the gun is kicked relative to the view, in radians, as `offsets`:
    /// its share of the kick's pitch and yaw, and the kick's roll undone
    /// (`0x1800378d0`).
    pub fn weapon_offsets(&self) -> Vec3 {
        let share = self.weapon_fraction * self.weapon_vm_scale;
        let a = self.weapon.angle;
        Spring {
            angle: Vec3::new(a.x * share, a.y * share, -a.z),
            speed: Vec3::ZERO,
        }
        .offsets(0.0)
    }

    /// -0.5..0.5, from a small fixed-seed generator (the effect doesn't need to be
    /// unpredictable, just varied).
    fn random(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.rng >> 8) as f32 / (1u32 << 24) as f32 - 0.5
    }
}

/// Source's field of view is horizontal, measured on a 4:3 screen; Bevy wants the
/// vertical angle. Wider screens then see more at the sides, as in the original.
pub fn vertical_fov(source_fov_degrees: f32) -> f32 {
    2.0 * ((source_fov_degrees.to_radians() * 0.5).tan() * 0.75).atan()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_fov_70_is_about_55_vertical() {
        assert!((vertical_fov(70.0).to_degrees() - 55.4).abs() < 0.2);
    }

    #[test]
    fn the_exact_spring_matches_small_steps_and_stays_stable_when_stiff() {
        // Against many tiny Euler steps of the same equation.
        let (k, c) = (65.0, 9.0);
        let (mut x, mut v) = (1.0f32, 5.0f32);
        for _ in 0..10_000 {
            let acc = -k * x - c * v;
            x += v * 1e-5;
            v += acc * 1e-5;
        }
        let (ex, ev) = solve(1.0, 5.0, k, c, 0.1);
        assert!(
            (ex - x).abs() < 1e-3 && (ev - v).abs() < 1e-2,
            "{ex} {ev} vs {x} {v}"
        );
        // Overdamped and critical too.
        let (ox, _) = solve(1.0, 0.0, 10.0, 20.0, 0.5);
        assert!(ox > 0.0 && ox < 1.0);
        let (cx, _) = solve(1.0, 0.0, 25.0, 10.0, 0.5);
        assert!(cx > 0.0 && cx < 1.0);
        // The R-201's aimed roll spring at a 60 Hz frame: decays, no blow-up.
        let (mut x, mut v) = (0.5f32, 0.0f32);
        for _ in 0..120 {
            (x, v) = solve(x, v, 20000.0, 20.0, 1.0 / 60.0);
        }
        assert!(x.abs() < 0.01 && v.abs() < 1.0, "{x} {v}");
    }

    #[test]
    fn a_jump_punch_matches_the_real_game() {
        // Recorded in the real game: a jump kicks the view ~2.5-5° down, peaking
        // ~0.13 s in, back near zero by ~0.4 s.
        let mut v = ViewEffects::default();
        v.punch(&JUMP, 1.0, 1.0, true);
        let (mut peak, mut peak_t) = (0.0f32, 0.0);
        for i in 0..240 {
            v.update(1.0 / 120.0, 0.0, false);
            if v.view.angle.x > peak {
                (peak, peak_t) = (v.view.angle.x, i as f32 / 120.0);
            }
        }
        assert!(peak > 2.0 && peak < 5.5, "peak {peak}");
        assert!((0.08..0.2).contains(&peak_t), "peak at {peak_t}");
        assert!(
            v.view.angle.length() < 0.05,
            "should settle: {:?}",
            v.view.angle
        );
        assert!(v.weapon.angle.length() < 0.05);
    }
}
