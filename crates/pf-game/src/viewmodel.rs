//! The first-person weapon ("viewmodel"), drawn by its own camera on top of the
//! world, as Source does: it never clips into walls and uses the pilot's narrower
//! `viewmodelfov` (54). Only the static reference pose for now; animation is next.
//!
//! Models and textures come from the player's own files (`pf-pc
//! common-extract` / `model-materials` into `Resources/pc_extracted/common`).
//! Without them there's simply no weapon on screen.

use std::path::PathBuf;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::view;

/// The viewmodel lives on its own render layer, seen only by its own camera.
const LAYER: usize = 1;
/// Pilot `viewmodelfov` (Source convention, like `fov`).
const VIEWMODEL_FOV: f32 = 54.0;
const WEAPON: &str = "models/weapons/rspn101/ptpov_rspn101.mdl";

/// Marks the viewmodel camera, which follows the player camera as its child.
#[derive(Component)]
pub struct ViewmodelCamera;

fn common_dir() -> PathBuf {
    PathBuf::from("Resources/pc_extracted/common")
}

/// The reference pose has the gun pointing along +y with z up; until animations
/// pose it, turn that to point down the camera's -z.
fn to_view(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], p[2], -p[1])
}

/// Hand-placed "hip" position until the idle animation drives it, units in
/// camera space (right, up, forward is -z).
const HOLD: Vec3 = Vec3::new(5.0, -30.0, -4.0);

/// Spawn the viewmodel camera, its light and the weapon under `camera`.
pub fn spawn(
    commands: &mut Commands,
    camera: Entity,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) {
    let dir = common_dir();
    let read = |ext: &str| std::fs::read(dir.join(WEAPON).with_extension(ext)).ok();
    let model = match (read("mdl"), read("vvd"), read("dx11.vtx")) {
        (Some(mdl), Some(vvd), Some(vtx)) => pf_fmt_mdl::load(&mdl, &vvd, &vtx).ok(),
        _ => None,
    };
    let Some(model) = model else {
        info!("no viewmodel: {WEAPON} not found under {}", dir.display());
        return;
    };
    let material_dirs = vec![dir.join("materials")];

    let view_camera = commands
        .spawn((
            ViewmodelCamera,
            Camera3d::default(),
            Camera {
                // Drawn after the world camera, over its image.
                order: 1,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            Projection::Perspective(PerspectiveProjection {
                fov: view::vertical_fov(VIEWMODEL_FOV),
                near: 0.5,
                far: 500.0,
                ..default()
            }),
            RenderLayers::layer(LAYER),
            Transform::IDENTITY,
            ChildOf(camera),
        ))
        .id();
    commands.spawn((
        DirectionalLight {
            illuminance: 4000.0,
            ..default()
        },
        RenderLayers::layer(LAYER),
        Transform::from_xyz(1.0, 2.0, 1.0).looking_at(Vec3::ZERO, Vec3::Y),
        ChildOf(view_camera),
    ));

    for m in &model.meshes {
        let mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            m.positions.iter().map(|&p| to_view(p)).collect::<Vec<_>>(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_NORMAL,
            m.normals.iter().map(|&n| to_view(n)).collect::<Vec<_>>(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, m.uvs.clone())
        .with_inserted_indices(Indices::U32(m.indices.clone()));
        let loaded = m
            .materials
            .iter()
            .find_map(|name| pf_world::load_material(&material_dirs, name));
        let texture = loaded.and_then(|l| l.texture).map(|i| images.add(i));
        let material = materials.add(StandardMaterial {
            base_color: if texture.is_some() {
                Color::WHITE
            } else {
                Color::srgb(0.4, 0.4, 0.42)
            },
            base_color_texture: texture,
            perceptual_roughness: 0.6,
            cull_mode: None,
            ..default()
        });
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material),
            RenderLayers::layer(LAYER),
            Transform::from_translation(HOLD),
            ChildOf(view_camera),
        ));
    }
    let (lo, hi) = model.meshes.iter().flat_map(|m| m.positions.iter()).fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(lo, hi), &p| (lo.min(Vec3::from(p)), hi.max(Vec3::from(p))),
    );
    info!(
        "viewmodel: {} meshes from {WEAPON}, bounds {lo} .. {hi}",
        model.meshes.len()
    );
}
