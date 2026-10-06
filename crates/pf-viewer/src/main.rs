//! M2 map viewer: loads a Titanfall `.bsp` and lets you fly around its raw
//! geometry. Each material gets its own flat colour; no textures yet.
//!
//! Usage: `cargo run -p pf-viewer --release -- <path/to/mp_colony.bsp>`
//!
//! Controls: click to capture the mouse (Esc releases), WASD to fly, Space/Ctrl
//! up/down, Shift for speed.
//!
//! Bevy note: a Bevy app is a list of plugins and *systems* (plain functions).
//! Systems ask for the data they need in their parameters and Bevy passes it in.

use bevy::asset::RenderAssetUsages;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use std::collections::HashMap;

/// Flying speed in game units (~inches) per second.
const SPEED: f32 = 800.0;
const FAST: f32 = 4.0;
const MOUSE_SENSITIVITY: f32 = 0.002;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: pf-viewer <map.bsp>");
        std::process::exit(2);
    });
    let data = std::fs::read(&path).unwrap_or_else(|e| fail(&format!("{path}: {e}")));
    let bsp = pf_fmt_bsp::Bsp::parse(&data).unwrap_or_else(|e| fail(&e.to_string()));
    let geometry = bsp.geometry().unwrap_or_else(|e| fail(&e.to_string()));
    // The map's playable box from the worldspawn entity; the vertex bounds also
    // include far-away sky geometry.
    let (min, max) = world_box(&bsp).unwrap_or_else(|| geometry.bounds());
    println!(
        "{path}: {} meshes, {} triangles, {} materials, size {:.0} x {:.0} x {:.0} units",
        geometry.meshes.len(),
        geometry.triangle_count(),
        geometry.materials.len(),
        max[0] - min[0],
        max[1] - min[1],
        max[2] - min[2],
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
        .insert_resource(ClearColor(Color::srgb(0.55, 0.65, 0.78)))
        .insert_resource(GlobalAmbientLight {
            brightness: 1500.0,
            ..default()
        })
        .insert_resource(MapGeometry(geometry, min, max))
        .add_systems(Startup, spawn_map)
        .add_systems(Update, (capture_cursor, fly))
        .run();
}

fn fail(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}

#[derive(Resource)]
struct MapGeometry(pf_fmt_bsp::Geometry, [f32; 3], [f32; 3]);

/// `world_mins` / `world_maxs` from the worldspawn entity.
fn world_box(bsp: &pf_fmt_bsp::Bsp) -> Option<([f32; 3], [f32; 3])> {
    let ents = bsp.entities().ok()?;
    let read = |key: &str| -> Option<[f32; 3]> {
        let line = ents
            .lines()
            .find(|l| l.starts_with(&format!("\"{key}\"")))?;
        let value = line.split('"').nth(3)?;
        let v: Vec<f32> = value
            .split_whitespace()
            .filter_map(|n| n.parse().ok())
            .collect();
        Some([*v.first()?, *v.get(1)?, *v.get(2)?])
    };
    Some((read("world_mins")?, read("world_maxs")?))
}

#[derive(Component, Default)]
struct FlyCamera {
    yaw: f32,
    pitch: f32,
}

/// Source maps are Z-up with X forward and Y left; Bevy is Y-up with -Z forward.
/// This rotation keeps handedness, so triangles keep their winding.
fn to_bevy(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], p[2], -p[1])
}

fn spawn_map(
    mut commands: Commands,
    geometry: Res<MapGeometry>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let g = &geometry.0;

    // Group triangles by material so each material is one Bevy mesh.
    let mut groups: HashMap<u32, Vec<[u32; 3]>> = HashMap::new();
    for m in &g.meshes {
        groups.entry(m.material).or_default().extend(&m.triangles);
    }

    for (material, tris) in groups {
        let name = g
            .materials
            .get(material as usize)
            .map(String::as_str)
            .unwrap_or("");
        // Tool materials (skybox, nodraw, triggers, ...) are invisible in game.
        if name.to_ascii_lowercase().starts_with("tools/") {
            continue;
        }
        // Unshared vertices with one normal per triangle: flat shading makes the
        // shapes easy to read without textures.
        let mut positions = Vec::with_capacity(tris.len() * 3);
        let mut normals = Vec::with_capacity(tris.len() * 3);
        for t in &tris {
            let [a, b, c] = t.map(|i| to_bevy(g.positions[i as usize]));
            let n = (b - a).cross(c - a).normalize_or_zero();
            positions.extend([a, b, c]);
            normals.extend([n, n, n]);
        }
        let count = positions.len() as u32;
        let mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_indices(Indices::U32((0..count).collect()));

        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color_for(name),
                perceptual_roughness: 0.9,
                // Winding isn't confirmed yet, so draw both sides.
                cull_mode: None,
                double_sided: true,
                ..default()
            })),
        ));
    }

    commands.spawn((
        DirectionalLight {
            illuminance: 8000.0,
            ..default()
        },
        Transform::from_xyz(0.3, 1.0, 0.5).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Start above the middle of the map, looking down at it.
    let (min, max) = (geometry.1, geometry.2);
    let centre = (to_bevy(min) + to_bevy(max)) * 0.5;
    let height = (max[2] - min[2]).max(1000.0) + 1500.0;
    let pitch = -0.7;
    commands.spawn((
        Camera3d::default(),
        // Default near/far planes assume metres; our units are inches.
        Projection::Perspective(PerspectiveProjection {
            fov: 70f32.to_radians(),
            near: 4.0,
            far: 100_000.0,
            ..default()
        }),
        Transform::from_translation(centre + Vec3::new(0.0, height, 0.0))
            .with_rotation(Quat::from_euler(EulerRot::YXZ, 0.0, pitch, 0.0)),
        FlyCamera { yaw: 0.0, pitch },
    ));
}

/// A stable, fairly bright colour per material name.
fn color_for(name: &str) -> Color {
    let hash = name.bytes().fold(0x811C_9DC5u32, |h, b| {
        (h ^ b as u32).wrapping_mul(0x0100_0193)
    });
    let hue = (hash % 360) as f32;
    Color::hsl(hue, 0.35, 0.6)
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
