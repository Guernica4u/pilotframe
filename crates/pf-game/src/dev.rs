//! Development helpers.
//!
//! `PF_SANDBOX_SCREENSHOT=path.png cargo run -p pf-sandbox` saves a screenshot
//! after two seconds and quits: a quick check that the app starts and renders.
//! `PF_AUTOSTART=test` or `=colony` skips the menus and loads that map.
//! `PF_WINDOW=1920x1080` sets the window's size (default 1280 × 720).

use std::path::PathBuf;

use bevy::prelude::*;

use crate::Screen;
use crate::level::ChosenMap;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

pub struct DevPlugin;

impl Plugin for DevPlugin {
    fn build(&self, app: &mut App) {
        let autostart = match std::env::var("PF_AUTOSTART").as_deref() {
            Ok("test") => Some(ChosenMap::TestMap),
            Ok("colony") => Some(ChosenMap::Colony),
            _ => None,
        };
        if let Some(map) = autostart {
            app.insert_resource(map)
                .add_systems(Startup, |mut next: ResMut<NextState<Screen>>| {
                    next.set(Screen::Loading)
                });
        }
        if let Some(path) = std::env::var_os("PF_SANDBOX_SCREENSHOT") {
            app.insert_resource(ScreenshotTo(path.into()))
                .add_systems(Update, screenshot_then_exit);
        }
    }
}

#[derive(Resource)]
struct ScreenshotTo(PathBuf);

// Bevy note: `Local` is per-system state that survives between runs, like a
// `static` that belongs to this one function.
fn screenshot_then_exit(
    mut commands: Commands,
    time: Res<Time>,
    to: Res<ScreenshotTo>,
    mut taken: Local<bool>,
    mut exit: MessageWriter<AppExit>,
) {
    let t = time.elapsed_secs();
    if t > delay() && !*taken {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(to.0.clone()));
        *taken = true;
    }
    if t > delay() + 1.0 {
        exit.write(AppExit::Success);
    }
}

/// Seconds before the screenshot (`PF_SCREENSHOT_DELAY`, default 2): Colony needs longer.
fn delay() -> f32 {
    std::env::var("PF_SCREENSHOT_DELAY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2.0)
}

/// The window's size: `PF_WINDOW` ("1920x1080"), else Bevy's default 1280 × 720.
pub fn window_size() -> (u32, u32) {
    std::env::var("PF_WINDOW")
        .ok()
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
        })
        .unwrap_or((1280, 720))
}
