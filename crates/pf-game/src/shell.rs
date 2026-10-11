//! The dev shell (ours, not the game's): boots straight into a map, captures
//! the mouse, and Esc pauses (freeing the mouse) or, held for a second, quits.
//! It stands in for the menus until they're rebuilt from the game's own UI
//! files (roadmap R9).
//!
//! `PF_MAP=colony` or `=test` picks the map (default Colony, or the greybox
//! test map if Colony's files aren't found).

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::Screen;
use crate::level::ChosenMap;

pub struct ShellPlugin;

impl Plugin for ShellPlugin {
    fn build(&self, app: &mut App) {
        let map = match std::env::var("PF_MAP").as_deref() {
            Ok("test") => ChosenMap::TestMap,
            Ok("colony") => ChosenMap::Colony,
            _ if crate::level::colony_path().is_some() => ChosenMap::Colony,
            _ => ChosenMap::TestMap,
        };
        app.insert_resource(map)
            .init_resource::<Paused>()
            .add_systems(
                OnEnter(Screen::InGame),
                |cursor: Single<&mut CursorOptions, With<PrimaryWindow>>| {
                    // Screenshot runs leave the mouse free, so moving it can't
                    // turn the view and the shots stay repeatable.
                    let screenshot = std::env::var_os("PF_SANDBOX_SCREENSHOT").is_some();
                    set_captured(&mut cursor.into_inner(), !screenshot)
                },
            )
            .add_systems(
                Update,
                (toggle_pause, hold_to_quit).run_if(in_state(Screen::InGame)),
            );
    }
}

/// Whether the game is paused (Esc).
#[derive(Resource, Default)]
pub struct Paused(pub bool);

/// Run condition: in game and not paused.
pub fn playing(screen: Res<State<Screen>>, paused: Res<Paused>) -> bool {
    *screen.get() == Screen::InGame && !paused.0
}

fn set_captured(cursor: &mut CursorOptions, captured: bool) {
    cursor.visible = !captured;
    cursor.grab_mode = if captured {
        CursorGrabMode::Locked
    } else {
        CursorGrabMode::None
    };
}

/// Esc (or the pad's Start) pauses and frees the mouse, or resumes.
fn toggle_pause(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut paused: ResMut<Paused>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let pressed = keys.just_pressed(KeyCode::Escape)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::Start));
    if pressed {
        paused.0 = !paused.0;
        set_captured(&mut cursor, !paused.0);
    }
}

/// Holding Esc for a second quits.
fn hold_to_quit(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut held: Local<f32>,
    mut exit: MessageWriter<AppExit>,
) {
    *held = if keys.pressed(KeyCode::Escape) {
        *held + time.delta_secs()
    } else {
        0.0
    };
    if *held > 1.0 {
        exit.write(AppExit::Success);
    }
}
