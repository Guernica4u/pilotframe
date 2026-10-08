//! First-person view effects: view punches (the kick on jumping, landing, starting
//! a wall-run or hang) and the sprint turn tilt.
//!
//! Values are the engine's own defaults read from the owner's `client.dll`, listed
//! in docs/movement/r1-engine-values.md ("View"), and the way a punch moves is the
//! game's (read from its code, and checked against a recording of the real game):
//! a punch adds 20 × its angles to an angular speed, and a spring stepped at a
//! fixed 60 Hz pulls the view back. The gun gets the same punch at 0.2 × on a
//! spring of its own.

use bevy::prelude::*;

/// Spring that pulls the view back after a punch (`viewpunch_base_springConstant`
/// and `viewpunch_base_springDamping`, the same for every axis).
const SPRING_CONSTANT: f32 = 65.0;
const SPRING_DAMPING: f32 = 9.0;
/// A punch of 1° adds this much angular speed, degrees/s.
const PUNCH_TO_SPEED: f32 = 20.0;
/// The spring's fixed time step, seconds.
const SPRING_STEP: f32 = 0.0166;
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
    /// One fixed step, as the game does it: move, clamp, damp, pull back.
    fn step(&mut self) {
        if self.angle.length_squared() <= SPRING_REST && self.speed.length_squared() <= SPRING_REST
        {
            *self = Self::default();
            return;
        }
        self.angle += self.speed * SPRING_STEP;
        self.angle = self.angle.clamp(
            Vec3::new(-89.0, -179.0, -89.0),
            Vec3::new(89.0, 179.0, 89.0),
        );
        self.speed *= (-SPRING_DAMPING * SPRING_STEP).exp();
        self.speed -= self.angle * SPRING_CONSTANT * SPRING_STEP;
    }

    /// In radians, converted from Source's convention to ours: (pitch up, yaw left,
    /// roll left), plus `extra_roll` degrees.
    fn offsets(&self, extra_roll: f32) -> Vec3 {
        Vec3::new(-self.angle.x, self.angle.y, -(self.angle.z + extra_roll)) * std::f32::consts::PI
            / 180.0
    }
}

/// The view and gun punch springs, plus the sprint tilt.
#[derive(Default)]
pub struct ViewEffects {
    view: Spring,
    weapon: Spring,
    /// Time not yet stepped by the springs, seconds.
    pending: f32,
    sprint_tilt: f32,
    sprint_tilt_speed: f32,
    rng: u32,
}

impl ViewEffects {
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
        // The game steps its springs at a fixed rate; so do we, whatever the
        // frame rate.
        self.pending = (self.pending + dt).min(0.25);
        while self.pending >= SPRING_STEP {
            self.pending -= SPRING_STEP;
            self.view.step();
            self.weapon.step();
        }

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
    /// `sprint_max_roll` is the pilot's `sprinttiltMaxRoll` in degrees.
    pub fn offsets(&self, sprint_max_roll: f32) -> Vec3 {
        self.view.offsets(self.sprint_tilt * sprint_max_roll)
    }

    /// How far the gun is kicked relative to the view, in radians, as `offsets`.
    pub fn weapon_offsets(&self) -> Vec3 {
        self.weapon.offsets(0.0)
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
