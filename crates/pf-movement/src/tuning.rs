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

    /// Top ground speed without sprint, units/s.
    pub walk_speed: f32,
    /// Top ground speed while sprinting forwards, units/s.
    pub sprint_speed: f32,
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

    /// Steepest slope that counts as ground, degrees from flat.
    pub max_slope_deg: f32,
    /// Tallest ledge walked up without jumping, units.
    pub step_height: f32,
}

impl MovementTuning {
    pub fn capsule(&self) -> Capsule {
        Capsule {
            radius: self.capsule_radius,
            height: self.capsule_height.max(2.0 * self.capsule_radius),
        }
    }

    /// Initial upward speed that reaches `jump_height` under `gravity` (v² = 2gh).
    pub fn jump_speed(&self) -> f32 {
        (2.0 * self.gravity * self.jump_height).max(0.0).sqrt()
    }

    /// A surface is walkable if its normal's up component is at least this.
    pub fn min_ground_normal_y(&self) -> f32 {
        self.max_slope_deg.to_radians().cos()
    }
}
