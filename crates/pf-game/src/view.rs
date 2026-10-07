//! First-person view effects: view punches (the kick on jumping, landing, starting
//! a wall-run or hang) and the sprint turn tilt.
//!
//! Values are the engine's own defaults read from the owner's `client.dll`, listed
//! in docs/movement/r1-engine-values.md ("View"). How a punch turns into motion
//! (an angular kick on a damped spring) is our reading of the settings' names and
//! descriptions.

use bevy::prelude::*;

/// Spring that pulls the view back after a punch (`viewpunch_base_springConstant`
/// and `viewpunch_base_springDamping`, the same for every axis).
const SPRING_CONSTANT: f32 = 65.0;
const SPRING_DAMPING: f32 = 9.0;

/// Sprint tilt (`sprinttilt_*`): turning this many degrees per second or more
/// gives the full tilt; the tilt fraction changes at most `MAX_VEL` per second,
/// with `ACCEL` per second².
const SPRINT_TILT_TURN_RANGE: f32 = 120.0;
const SPRINT_TILT_ACCEL: f32 = 35.0;
const SPRINT_TILT_MAX_VEL: f32 = 2.0;

/// A punch: pitch, yaw, roll in degrees, each `base + random × (-1..1)`. Source
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

/// The current punch offset and its spring, plus the sprint tilt.
#[derive(Default)]
pub struct ViewEffects {
    /// Pitch, yaw, roll offsets in degrees (Source convention) and their speeds.
    angle: Vec3,
    speed: Vec3,
    sprint_tilt: f32,
    sprint_tilt_speed: f32,
    rng: u32,
}

impl ViewEffects {
    /// Kick the view. `side` flips yaw and roll (e.g. which side the wall is on);
    /// `scale` scales the whole punch.
    pub fn punch(&mut self, punch: &Punch, side: f32, scale: f32) {
        let r = Vec3::new(self.random(), self.random(), self.random());
        let mut kick = (punch.base + punch.random * r) * scale;
        kick.y *= side;
        kick.z *= side;
        // An angular kick: the spring's natural speed turns the punch angle into a
        // starting speed, so the peak is roughly half the punch once damping acts.
        self.speed += kick * SPRING_CONSTANT.sqrt();
    }

    /// Advance the springs by `dt` seconds. `turn_rate` is degrees/s of yaw (left
    /// positive); `sprinting` enables the turn tilt.
    pub fn update(&mut self, dt: f32, turn_rate: f32, sprinting: bool) {
        // Small fixed sub-steps keep the stiff spring stable at low frame rates.
        let steps = (dt / (1.0 / 240.0)).ceil().max(1.0);
        let h = dt / steps;
        for _ in 0..steps as usize {
            let accel = -SPRING_CONSTANT * self.angle - SPRING_DAMPING * self.speed;
            self.speed += accel * h;
            self.angle += self.speed * h;
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

    /// Offsets to add to the camera, in radians: (pitch up, yaw left, roll left),
    /// i.e. converted from Source's convention to ours. `sprint_max_roll` is the
    /// pilot's `sprinttiltMaxRoll` in degrees.
    pub fn offsets(&self, sprint_max_roll: f32) -> Vec3 {
        let roll = self.angle.z + self.sprint_tilt * sprint_max_roll;
        Vec3::new(-self.angle.x, self.angle.y, -roll) * std::f32::consts::PI / 180.0
    }

    /// -1..1, from a small fixed-seed generator (the effect doesn't need to be
    /// unpredictable, just varied).
    fn random(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.rng >> 8) as f32 / (1u32 << 23) as f32 - 1.0
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
    fn a_punch_kicks_then_settles() {
        let mut v = ViewEffects::default();
        v.punch(&LAND, 1.0, 1.0);
        let mut peak = 0.0f32;
        for _ in 0..120 {
            v.update(1.0 / 60.0, 0.0, false);
            peak = peak.max(v.angle.x.abs());
        }
        assert!(peak > 0.3 && peak < 3.0, "peak {peak}");
        assert!(v.angle.length() < 0.05, "should settle: {:?}", v.angle);
    }
}
