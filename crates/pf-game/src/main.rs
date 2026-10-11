//! Pilotframe: the game app. Main menu → map select → walk the map with pilot
//! movement (`pf-movement`). Maps: a built-in greybox test map (needs no game
//! files) and Colony (from files unpacked from the player's own discs).
//!
//! Run with `cargo run -p pf-game --release`.
//!
//! Bevy note: a Bevy app is a list of *plugins*, each adding *systems* (plain
//! functions) that run on *schedules* like `Startup`, `Update` and `FixedUpdate`.
//! Systems declare what data they need in their parameters, and Bevy hands it over.
//! *States* (here [`Screen`]) switch whole groups of systems on and off.

mod cockpit_sway;
mod data;
mod dev;
mod hud;
mod hudbar;
mod hudwarp;
mod input;
mod level;
mod loadout;
mod map;
mod matproxy;
mod menu;
mod player;
mod recoil;
mod self_illum;
mod settings;
mod spread;
mod sway;
mod tuning;
mod unlit_two;
mod view;
mod viewmodel;

use bevy::prelude::*;

/// Simulation ticks per second. Placeholder: PLAN §2 says to check what tick rate
/// the original used.
const TICK_HZ: f64 = 60.0;

/// Which screen the game is on.
#[derive(States, Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Screen {
    #[default]
    MainMenu,
    MapSelect,
    Options,
    /// One frame: reads the chosen map and builds its collision.
    Loading,
    InGame,
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Pilotframe (unofficial, not affiliated with EA or Respawn)".into(),
                resolution: dev::window_size().into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(Time::<Fixed>::from_hz(TICK_HZ))
        .init_state::<Screen>()
        .add_plugins((
            settings::SettingsPlugin,
            tuning::TuningPlugin,
            input::InputPlugin,
            pf_world::WorldPlugin,
            level::LevelPlugin,
            player::PlayerPlugin,
            viewmodel::ViewmodelPlugin,
            hud::HudPlugin,
            menu::MenuPlugin,
            dev::DevPlugin,
        ))
        .run();
}
