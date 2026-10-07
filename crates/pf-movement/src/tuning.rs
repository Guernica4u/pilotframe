//! Every movement number lives here and is loaded from a `.ron` file, so tuning
//! never needs a code change. There's deliberately no `Default`: values come from
//! data, each with its source logged in `docs/movement/values.md`.

use serde::Deserialize;

use crate::collision::Capsule;

/// Units: game units (~1 inch), seconds, degrees.
#[derive(Debug, Clone, PartialEq, Deserialize)]
// Rust note: `deny_unknown_fields` turns a typo in the .ron file into a load error
// instead of a silently ignored value.
#[serde(deny_unknown_fields)]
pub struct MovementTuning {
    pub capsule_radius: f32,
    /// Feet to top of head.
    pub capsule_height: f32,
    /// Feet to camera.
    pub eye_height: f32,
    /// Feet to top of head while crouched.
    pub crouch_height: f32,
    /// Feet to camera while crouched.
    pub crouch_eye_height: f32,

    /// Top ground speed without sprint, units/s.
    pub walk_speed: f32,
    /// Top ground speed while sprinting forwards, units/s.
    pub sprint_speed: f32,
    /// Top ground speed while crouched, units/s.
    pub crouch_speed: f32,
    /// How fast ground speed approaches the wished-for speed, units/s².
    pub ground_accel: f32,
    /// How fast ground speed bleeds off with no input, units/s².
    pub ground_friction: f32,
    /// Steering acceleration in the air, units/s². Never raises speed above the
    /// faster of your current speed and the wished-for speed.
    pub air_accel: f32,

    /// Downward acceleration, units/s².
    pub gravity: f32,
    /// Apex height of a standing jump, units. Launch speed is derived from it.
    pub jump_height: f32,
    /// Height gained by the mid-air jump (one per time in the air), units.
    pub double_jump_height: f32,

    /// Speed a wall-run builds up to along the wall, units/s.
    pub wallrun_speed: f32,
    /// How fast wall-run speed approaches `wallrun_speed`, units/s².
    pub wallrun_accel: f32,
    /// Slower than this along the wall and you can't start, or you slip off, units/s.
    pub wallrun_min_speed: f32,
    /// Downward acceleration while wall-running, units/s². Much less than `gravity`.
    pub wallrun_gravity: f32,
    /// Most upward speed kept when a wall-run starts, units/s.
    pub wallrun_max_lift: f32,
    /// Falling faster than this, a wall won't catch you, units/s.
    pub wallrun_max_fall_speed: f32,
    /// Longest a single wall-run lasts, seconds.
    pub wallrun_time_limit: f32,
    /// Height of a jump off a wall, units.
    pub wall_jump_height: f32,
    /// Speed away from the wall a wall jump adds, units/s.
    pub wall_jump_push: f32,
    /// Longest you can hang on a wall, seconds.
    pub wall_hang_time_limit: f32,

    /// Highest ledge, above the feet, that a mantle can pull you onto, units.
    pub mantle_reach: f32,
    /// How long climbing over a ledge takes, seconds.
    pub mantle_time: f32,

    /// Steepest slope that counts as ground, degrees from flat.
    pub max_slope_deg: f32,
    /// Tallest ledge walked up without jumping, units.
    pub step_height: f32,
}

impl MovementTuning {
    /// The collision shape, standing or crouched.
    pub fn capsule(&self, crouched: bool) -> Capsule {
        let height = if crouched {
            self.crouch_height
        } else {
            self.capsule_height
        };
        Capsule {
            radius: self.capsule_radius,
            height: height.max(2.0 * self.capsule_radius),
        }
    }

    /// Camera height above the feet.
    pub fn eye(&self, crouched: bool) -> f32 {
        if crouched {
            self.crouch_eye_height
        } else {
            self.eye_height
        }
    }

    /// Initial upward speed that reaches `jump_height` under `gravity` (v² = 2gh).
    pub fn jump_speed(&self) -> f32 {
        self.launch_speed(self.jump_height)
    }

    /// Upward speed that rises `height` units under `gravity`.
    pub fn launch_speed(&self, height: f32) -> f32 {
        (2.0 * self.gravity * height).max(0.0).sqrt()
    }

    /// A surface is walkable if its normal's up component is at least this.
    pub fn min_ground_normal_y(&self) -> f32 {
        self.max_slope_deg.to_radians().cos()
    }
}
