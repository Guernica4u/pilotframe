//! The pilot cockpit's sway, ported from `client.dll` (docs/hud.md, "Cockpit
//! sway"). The helmet HUD hangs off the cockpit model, so it drifts a little
//! as the pilot moves and turns.
//!
//! The game drives the cockpit with its weapon-sway code ([`crate::sway`]):
//! the cockpit's seven settings fill a weapon-sway block, some with the sign
//! flipped ([`Settings::sway`]).
//!
//! Axes and units are the game's: offsets in inches along (forward, left, up),
//! angles in degrees (pitch down, yaw left, roll right).

use std::path::Path;

use pf_fmt_ui::kv;

use crate::sway::{self, Dir};

/// The pilot class the cockpit's settings come from.
const PILOT_CLASS: &str = "scripts/players/mp/pilot_male_br.set";

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

impl Settings {
    /// The weapon-sway settings the game builds for the cockpit
    /// (`0x1803f8be0`): a fresh block with the cockpit's limits, `gain` as both
    /// rates, and each direction's factors, some with the sign flipped.
    pub fn sway(&self) -> sway::Settings {
        let mut b = sway::Block::blank();
        for i in 0..3 {
            b.min[i] = self.min_origin[i];
            b.max[i] = self.max_origin[i];
            b.min[i + 3] = self.min_angle[i];
            b.max[i + 3] = self.max_angle[i];
        }
        b.translate_gain = self.gain;
        b.rotate_gain = self.gain;
        let (mo, ma, mr) = (self.move_origin, self.move_angle, self.move_angle_roll);
        let (to, ta, tr) = (self.turn_origin, self.turn_angle, self.turn_angle_roll);
        // (translate x, y, z, rotate pitch, yaw, roll)
        *b.dir(Dir::MoveForward) = [mo, 0.0, 0.0, 0.0, 0.0, 0.0];
        *b.dir(Dir::MoveBack) = [-mo, 0.0, 0.0, 0.0, 0.0, 0.0];
        *b.dir(Dir::MoveLeft) = [0.0, mo, 0.0, 0.0, ma, -mr];
        *b.dir(Dir::MoveRight) = [0.0, -mo, 0.0, 0.0, -ma, mr];
        *b.dir(Dir::MoveUp) = [0.0, 0.0, mo, -ma, 0.0, 0.0];
        *b.dir(Dir::MoveDown) = [0.0, 0.0, -mo, ma, 0.0, 0.0];
        *b.dir(Dir::TurnLeft) = [0.0, to, 0.0, 0.0, ta, -tr];
        *b.dir(Dir::TurnRight) = [0.0, -to, 0.0, 0.0, -ta, tr];
        *b.dir(Dir::TurnUp) = [0.0, 0.0, to, ta, 0.0, 0.0];
        *b.dir(Dir::TurnDown) = [0.0, 0.0, -to, -ta, 0.0, 0.0];
        sway::Settings {
            hip: b,
            zoomed: sway::Block::blank(),
            zoomed_enabled: true,
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

    fn frame(velocity: [f32; 3], yaw: f32) -> sway::Input {
        sway::Input {
            dt: 1.0 / 60.0,
            velocity,
            view: [0.0, yaw, 0.0],
            zoomed: false,
            zoom_fraction: 0.0,
            on_ground: true,
        }
    }

    #[test]
    fn running_forward_pulls_back_and_turning_left_lags_right() {
        let s = pilot().sway();
        let mut sway = sway::Sway::default();
        for _ in 0..120 {
            sway.update(&s, &frame([300.0, 0.0, 0.0], 0.0));
        }
        // 300 in/s at 60 fps is 5 inches a frame; × -0.15 = -0.75 inch.
        assert!((sway.origin[0] + 0.75).abs() < 0.01, "{:?}", sway.origin);
        let mut sway = sway::Sway::default();
        let mut yaw = 0.0;
        for _ in 0..120 {
            yaw += 3.0; // 180°/s to the left
            sway.update(&s, &frame([0.0; 3], yaw));
        }
        // 3° a frame × -0.6 = -1.8°, clamped to the -0.5° limit; the offset
        // goes right (negative left) and roll is -3 × 0.2 = -0.6, clamped.
        assert!((sway.angles[1] + 0.5).abs() < 0.01, "{:?}", sway.angles);
        assert!(sway.origin[1] < 0.0 && (sway.angles[2] + 0.5).abs() < 0.01);
    }
}
