//! Map viewer: fly around a Titanfall map (drawn by `pf-world`) with no
//! collision. Handy for checking how a map looks.
//!
//! Usage: `cargo run -p pf-viewer --release -- <map>` (e.g. `mp_colony`).
//!
//! Reads the map from the EA install, found under `Resources/` (or set
//! `PF_GAME_DIR` to the folder holding `vpk/`).
//!
//! Controls: click to capture the mouse (Esc releases), WASD to fly, Space/Ctrl
//! up/down, Shift for speed.

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use pf_world::{EYE_HEIGHT, MapGeometry, WorldPlugin};
use std::path::Path;

/// Flying speed in game units (~inches) per second.
const SPEED: f32 = 800.0;
const FAST: f32 = 4.0;
const MOUSE_SENSITIVITY: f32 = 0.002;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: pf-viewer <map, e.g. mp_colony>");
        std::process::exit(2);
    });
    let install = pf_world::gamefs::find_install(Path::new("Resources")).unwrap_or_else(|| {
        eprintln!("error: no EA install found under Resources/ (set PF_GAME_DIR)");
        std::process::exit(1);
    });
    if let Err(e) = pf_world::gamefs::mount_install(&install, None) {
        eprintln!("error: {}: {e}", install.display());
        std::process::exit(1);
    }
    let map = pf_world::load(&path).unwrap_or_else(|e| {
        eprintln!("error: {path}: {e}");
        std::process::exit(1);
    });
    let g = &map.geometry.0;
    println!(
        "{path}: {} meshes, {} triangles, {} materials, {} props",
        g.meshes.len(),
        g.triangle_count(),
        g.materials.len(),
        map.props.props.len(),
    );

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title:
                    "Pilotframe map viewer (unofficial, not affiliated with EA or Respawn)".into(),
                ..default()
            }),
            ..default()
        }))
        // Overcast grey close to the fog colour until the real skybox is drawn.
        .insert_resource(ClearColor(Color::srgb_u8(170, 172, 176)))
        .add_plugins(WorldPlugin)
        .insert_resource(map.geometry)
        .insert_resource(map.props)
        .add_systems(
            Startup,
            (
                pf_world::spawn_map,
                pf_world::props::spawn_props,
                spawn_camera,
            ),
        )
        .add_systems(Update, (capture_cursor, fly))
        .run();
}

#[derive(Component, Default)]
struct FlyCamera {
    yaw: f32,
    pitch: f32,
}

/// Start where a pilot would: the map's first pilot spawn, at eye height.
fn spawn_camera(mut commands: Commands, map: Res<MapGeometry>) {
    let (feet, yaw) = pf_world::spawn_point(&map);
    commands.spawn((
        Camera3d::default(),
        // Default near/far planes assume metres; our units are inches.
        Projection::Perspective(PerspectiveProjection {
            fov: 70f32.to_radians(),
            near: 4.0,
            far: 100_000.0,
            ..default()
        }),
        Transform::from_translation(feet + Vec3::Y * EYE_HEIGHT)
            .with_rotation(Quat::from_rotation_y(yaw)),
        FlyCamera { yaw, pitch: 0.0 },
        pf_world::colony_fog(),
    ));
}

fn capture_cursor(
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    if mouse.just_pressed(MouseButton::Left) {
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
    if keys.just_pressed(KeyCode::Escape) {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
}

fn fly(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    camera: Single<(&mut Transform, &mut FlyCamera)>,
) {
    let (mut transform, mut fly) = camera.into_inner();
    if cursor.grab_mode != CursorGrabMode::None {
        fly.yaw -= mouse_motion.delta.x * MOUSE_SENSITIVITY;
        fly.pitch = (fly.pitch - mouse_motion.delta.y * MOUSE_SENSITIVITY).clamp(-1.54, 1.54);
        transform.rotation = Quat::from_euler(EulerRot::YXZ, fly.yaw, fly.pitch, 0.0);
    }

    let mut dir = Vec3::ZERO;
    let (forward, right) = (*transform.forward(), *transform.right());
    for (key, v) in [
        (KeyCode::KeyW, forward),
        (KeyCode::KeyS, -forward),
        (KeyCode::KeyD, right),
        (KeyCode::KeyA, -right),
        (KeyCode::Space, Vec3::Y),
        (KeyCode::ControlLeft, -Vec3::Y),
    ] {
        if keys.pressed(key) {
            dir += v;
        }
    }
    let speed = if keys.pressed(KeyCode::ShiftLeft) {
        SPEED * FAST
    } else {
        SPEED
    };
    transform.translation += dir.normalize_or_zero() * speed * time.delta_secs();
}
