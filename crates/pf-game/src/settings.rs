//! Player settings (mouse sensitivity, field of view), saved to `settings.ron`
//! next to the game's exe. A missing or broken file just means defaults.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Settings::load());
    }
}

#[derive(Resource, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Multiplier on the base mouse turn rate (1.0 = default).
    pub mouse_sensitivity: f32,
    /// Field of view in degrees, the way Titanfall counts it: horizontal, on a
    /// 4:3 screen (its PC slider runs 70 to 90; the pilot default is 70).
    pub fov: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mouse_sensitivity: 1.0,
            fov: 70.0,
        }
    }
}

pub const SENSITIVITY_RANGE: (f32, f32, f32) = (0.25, 4.0, 0.25); // min, max, step
pub const FOV_RANGE: (f32, f32, f32) = (70.0, 90.0, 5.0);

impl Settings {
    fn path() -> Option<PathBuf> {
        Some(std::env::current_exe().ok()?.parent()?.join("settings.ron"))
    }

    fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| ron::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        let text =
            ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).unwrap_or_default();
        if let Err(e) = std::fs::write(&path, text) {
            warn!("couldn't save {}: {e}", path.display());
        }
    }
}

/// Step `value` by `dir` steps within `(min, max, step)`, wrapping at the ends.
pub fn step_setting(value: f32, dir: i32, (min, max, step): (f32, f32, f32)) -> f32 {
    let next = value + step * dir as f32;
    if next > max + 1e-3 {
        min
    } else if next < min - 1e-3 {
        max
    } else {
        (next / step).round() * step
    }
}
