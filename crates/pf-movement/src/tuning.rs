//! Every movement number lives here and is loaded from a `.ron` file, so tuning
//! never needs a code change. There's deliberately no `Default`: values come from
//! data, each with its source logged in `docs/movement/values.md` (and the engine
//! defaults in `docs/movement/r1-engine-values.md`).

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
    /// How fast ground speed builds towards the wished-for speed, units/s².
    pub acceleration: f32,
    /// Ground friction: each second you lose `friction` × max(speed, stop_speed).
    pub friction: f32,
    /// Below this speed friction stops you at a constant rate instead of a fading one.
    pub stop_speed: f32,

    /// Air control only steers up to this speed in any one direction, units/s.
    /// Above it you keep your momentum but can't add to it (Source-style).
    pub air_speed: f32,
    /// Steering acceleration in the air, units/s².
    pub air_acceleration: f32,

    /// Downward acceleration, units/s².
    pub gravity: f32,
    /// Apex height of a standing jump, units. Launch speed is derived from it.
    pub jump_height: f32,
    /// Height gained by the mid-air jump (one per time in the air), units.
    pub double_jump_height: f32,
    /// After walking off a ledge you can still jump for this long, seconds.
    pub jump_grace_time: f32,

    /// Longest a wall-run lasts before you start slipping, seconds.
    pub wallrun_time_limit: f32,
    /// How long the slip lasts before you fall off, seconds.
    pub wallrun_slip_time: f32,
    /// Downward kick when slipping starts, units/s.
    pub wallrun_slip_speed: f32,
    /// Fraction of speed lost when slipping starts.
    pub wallrun_slip_slowdown: f32,
    /// Upward speed given when a wall-run starts, units/s.
    pub wallrun_up_boost: f32,
    /// Gravity starts at zero on a wall and ramps to full over this, seconds.
    pub wallrun_gravity_ramp_time: f32,
    /// Top speed along the wall, units/s.
    pub wallrun_max_speed: f32,
    /// Acceleration along the wall (Source-style factor: units/s² per unit/s).
    pub wallrun_accelerate: f32,
    /// Friction along the wall (same meaning as `friction`).
    pub wallrun_friction: f32,
    /// Fastest you move up or down the wall, units/s.
    pub wallrun_max_speed_vertical: f32,
    /// Upward acceleration from pushing forward along the wall, before
    /// `wallrun_upward_push` scales it, units/s².
    pub wallrun_accelerate_vertical: f32,
    /// How much of `wallrun_accelerate_vertical` holding forward gives, 0..1.
    pub wallrun_upward_push: f32,
    /// Slower than this along the wall and you can't start a wall-run, units/s.
    pub wallrun_min_speed: f32,
    /// Speed away from the wall when you let go or fall off, units/s.
    pub wallrun_fall_away_speed: f32,
    /// Wall-runs (without landing) before the upward boost starts fading, and the
    /// count at which it's gone.
    pub wallrun_strength_loss_start: f32,
    pub wallrun_strength_loss_end: f32,
    /// Jumping off a wall: speed up, away from the wall, and towards your input.
    pub wall_jump_up_speed: f32,
    pub wall_jump_outward_speed: f32,
    pub wall_jump_input_speed: f32,
    /// Longest you can hang on a wall before slipping, seconds.
    pub wall_hang_time_limit: f32,
    /// Time to come to a stop when a wall-hang starts, seconds.
    pub wall_hang_stop_time: f32,
    /// How long the slip at the end of a hang lasts, seconds.
    pub wall_hang_slip_time: f32,

    /// Highest ledge above the feet that a mantle can reach, units.
    pub mantle_height: f32,
    /// Most the view may point away from the wall to start a mantle, degrees
    /// (`_wallrun` while wall-running).
    pub mantle_max_view_angle: f32,
    pub mantle_max_view_angle_wallrun: f32,
    /// Eye height relative to the ledge that picks the mantle speed: below
    /// `mantle_eye_below` takes `mantle_time_below`, below `mantle_eye_level` takes
    /// `mantle_time_level`, below `mantle_eye_above` takes `mantle_time_above`,
    /// otherwise `mantle_time_high`. Units and seconds.
    pub mantle_eye_below: f32,
    pub mantle_eye_level: f32,
    pub mantle_eye_above: f32,
    pub mantle_time_below: f32,
    pub mantle_time_level: f32,
    pub mantle_time_above: f32,
    pub mantle_time_high: f32,

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

    /// How long a mantle takes, from where the eyes are relative to the ledge top.
    pub fn mantle_time(&self, eye_above_ledge: f32) -> f32 {
        if eye_above_ledge < self.mantle_eye_below {
            self.mantle_time_below
        } else if eye_above_ledge < self.mantle_eye_level {
            self.mantle_time_level
        } else if eye_above_ledge < self.mantle_eye_above {
            self.mantle_time_above
        } else {
            self.mantle_time_high
        }
    }
}
