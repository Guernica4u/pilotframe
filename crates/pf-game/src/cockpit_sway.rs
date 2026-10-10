//! The pilot cockpit's sway, ported from `client.dll` (docs/hud.md, "Cockpit
//! sway"). The helmet HUD hangs off the cockpit model, so it drifts a little
//! as the pilot moves and turns.
//!
//! The game drives the cockpit with its weapon-sway code: the cockpit's seven
//! settings fill a weapon-sway block (some with the sign flipped), and the
//! shared update eases an offset and three angles towards targets made from
//! this frame's movement and the recent turn rate. With the cockpit's values
//! every term is linear and symmetric, so it reduces to the sums in
//! [`Sway::update`].
//!
//! Axes and units are the game's: offsets in inches along (forward, left, up),
//! angles in degrees (pitch down, yaw left, roll right).

use std::path::Path;

use pf_fmt_ui::kv;

/// The pilot class the cockpit's settings come from.
const PILOT_CLASS: &str = "scripts/players/mp/pilot_male_br.set";
/// Turn rates are averaged over this many frames (the game keeps a ring of 5
/// and weights each by 0.2).
const TURN_SAMPLES: usize = 5;

/// The player-class settings the cockpit sway uses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// `cockpitSwayGain`: how fast the offset and angles follow their targets
    /// (the game uses it as both exponential rates).
    pub gain: f32,
    /// `cockpitSwayMin/MaxOrigin{X,Y,Z}`.
    pub min_origin: [f32; 3],
    pub max_origin: [f32; 3],
    /// `cockpitSwayMin/MaxAngle{Pitch,Yaw,Roll}`.
    pub min_angle: [f32; 3],
    pub max_angle: [f32; 3],
    pub turn_angle: f32,
    pub turn_angle_roll: f32,
    pub turn_origin: f32,
    pub move_angle: f32,
    pub move_angle_roll: f32,
    pub move_origin: f32,
}

impl Default for Settings {
    /// The defaults in the game's player-settings table (`client.dll`).
    fn default() -> Self {
        Self {
            gain: 3.0,
            min_origin: [-1.0; 3],
            max_origin: [1.0; 3],
            min_angle: [-0.5; 3],
            max_angle: [0.5; 3],
            turn_angle: -4.0,
            turn_angle_roll: 0.0,
            turn_origin: -1.0,
            move_angle: -0.3,
            move_angle_roll: -0.2,
            move_origin: -0.15,
        }
    }
}

impl Settings {
    /// The pilot class's values over the table defaults.
    pub fn load(dir: &Path) -> Self {
        let mut s = Self::default();
        let Some(global) = std::fs::read_to_string(dir.join(PILOT_CLASS))
            .ok()
            .and_then(|t| kv::parse(&t).nodes.into_iter().next())
            .and_then(|root| root.get("global").cloned())
        else {
            return s;
        };
        let num = |key: &str| global.text(key).and_then(|v| v.trim().parse::<f32>().ok());
        let set = |slot: &mut f32, key: &str| {
            if let Some(v) = num(key) {
                *slot = v;
            }
        };
        set(&mut s.gain, "cockpitSwayGain");
        for (i, axis) in ["X", "Y", "Z"].iter().enumerate() {
            set(&mut s.min_origin[i], &format!("cockpitSwayMinOrigin{axis}"));
            set(&mut s.max_origin[i], &format!("cockpitSwayMaxOrigin{axis}"));
        }
        for (i, axis) in ["Pitch", "Yaw", "Roll"].iter().enumerate() {
            set(&mut s.min_angle[i], &format!("cockpitSwayMinAngle{axis}"));
            set(&mut s.max_angle[i], &format!("cockpitSwayMaxAngle{axis}"));
        }
        set(&mut s.turn_angle, "cockpitSwayTurnAngleFactor");
        set(&mut s.turn_angle_roll, "cockpitSwayTurnAngleRollFactor");
        set(&mut s.turn_origin, "cockpitSwayTurnOriginFactor");
        set(&mut s.move_angle, "cockpitSwayMoveAngleFactor");
        set(&mut s.move_angle_roll, "cockpitSwayMoveAngleRollFactor");
        set(&mut s.move_origin, "cockpitSwayMoveOriginFactor");
        s
    }
}

/// The cockpit's current sway.
#[derive(Clone, Debug, Default)]
pub struct Sway {
    /// Offset (forward, left, up), inches.
    pub origin: [f32; 3],
    /// Angles (pitch, yaw, roll), degrees.
    pub angles: [f32; 3],
    /// The last few frames' turn rates (pitch, yaw), degrees per second.
    rates: [[f32; 2]; TURN_SAMPLES],
    next: usize,
    /// Last frame's view (pitch, yaw), degrees.
    last_view: Option<[f32; 2]>,
}

/// `a - b` as an angle in -180..180 degrees.
fn angle_diff(a: f32, b: f32) -> f32 {
    (a - b + 180.0).rem_euclid(360.0) - 180.0
}

impl Sway {
    /// One frame of `dt` seconds. `velocity` is the player's, in the view's
    /// (forward, left, up); `view` is (pitch down, yaw left) in degrees.
    ///
    /// Targets, each clamped to its limits (the game's per-direction factors
    /// with the cockpit's values filled in), with v the velocity × `dt` and
    /// r the averaged turn rate × `dt`:
    /// - offset: forward v_f·move, left v_l·move + r_yaw·turn, up v_u·move − r_pitch·turn
    /// - pitch −v_u·move_angle − r_pitch·turn_angle; yaw v_l·move_angle + r_yaw·turn_angle;
    ///   roll −v_l·move_roll − r_yaw·turn_roll
    ///
    /// Each value then eases towards its target: `x = lerp(target, x, e^(−gain·dt))`.
    pub fn update(&mut self, s: &Settings, dt: f32, velocity: [f32; 3], view: [f32; 2]) {
        if dt <= 0.0 {
            return;
        }
        // Turn rate this frame, then the average of the last few.
        let last = self.last_view.unwrap_or(view);
        self.next = (self.next + 1) % TURN_SAMPLES;
        self.rates[self.next] = [
            angle_diff(view[0], last[0]) / dt,
            angle_diff(view[1], last[1]) / dt,
        ];
        self.last_view = Some(view);
        let average = |i: usize| self.rates.iter().map(|r| r[i]).sum::<f32>() / TURN_SAMPLES as f32;
        // The game scales the turn rate by a frame time from its globals; we
        // take it to be this frame's.
        let (pitch_rate, yaw_rate) = (average(0) * dt, average(1) * dt);
        let [forward, left, up] = velocity.map(|v| v * dt);

        let origin = [
            forward * s.move_origin,
            left * s.move_origin + yaw_rate * s.turn_origin,
            up * s.move_origin - pitch_rate * s.turn_origin,
        ];
        let angles = [
            -up * s.move_angle - pitch_rate * s.turn_angle,
            left * s.move_angle + yaw_rate * s.turn_angle,
            -left * s.move_angle_roll - yaw_rate * s.turn_angle_roll,
        ];
        let keep = (-s.gain * dt).exp();
        for i in 0..3 {
            let target = origin[i].clamp(s.min_origin[i], s.max_origin[i]);
            self.origin[i] = (1.0 - keep) * target + keep * self.origin[i];
            let target = angles[i].clamp(s.min_angle[i], s.max_angle[i]);
            self.angles[i] += (1.0 - keep) * angle_diff(target, self.angles[i]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pilot() -> Settings {
        Settings {
            gain: 5.0,
            turn_angle: -0.6,
            turn_angle_roll: 0.2,
            turn_origin: -0.15,
            move_angle: -0.4,
            move_angle_roll: -0.2,
            move_origin: -0.15,
            ..Settings::default()
        }
    }

    #[test]
    fn still_stays_centred() {
        let mut sway = Sway::default();
        for _ in 0..60 {
            sway.update(&pilot(), 1.0 / 60.0, [0.0; 3], [0.0, 90.0]);
        }
        assert_eq!(sway.origin, [0.0; 3]);
        assert_eq!(sway.angles, [0.0; 3]);
    }

    #[test]
    fn running_forward_pulls_back_and_turning_left_lags_right() {
        let s = pilot();
        let mut sway = Sway::default();
        for _ in 0..120 {
            sway.update(&s, 1.0 / 60.0, [300.0, 0.0, 0.0], [0.0, 0.0]);
        }
        // 300 in/s at 60 fps is 5 inches a frame; × -0.15 = -0.75 inch.
        assert!((sway.origin[0] + 0.75).abs() < 0.01, "{:?}", sway.origin);
        let mut sway = Sway::default();
        let mut yaw = 0.0;
        for _ in 0..120 {
            yaw += 3.0; // 180°/s to the left
            sway.update(&s, 1.0 / 60.0, [0.0; 3], [0.0, yaw]);
        }
        // 3° a frame × -0.6 = -1.8°, clamped to the -0.5° limit; the offset
        // goes right (negative left) and roll is -3 × 0.2 = -0.6, clamped.
        assert!((sway.angles[1] + 0.5).abs() < 0.01, "{:?}", sway.angles);
        assert!(sway.origin[1] < 0.0 && (sway.angles[2] + 0.5).abs() < 0.01);
    }
}
