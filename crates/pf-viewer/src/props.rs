//! Static props: the models placed around the map (trees, crates, fences...).
//! Each model is loaded once and shared by every prop that uses it, so Bevy
//! can batch the draws.

use crate::{
    LightingScale, MapGeometry, MaterialsDirs, WorldExtension, WorldMaterial, color_for,
    load_material, to_bevy,
};
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Resource)]
pub struct StaticProps {
    pub names: Vec<String>,
    pub props: Vec<pf_fmt_bsp::StaticProp>,
}

/// One loaded model: its meshes and their materials, ready to spawn.
type Parts = Vec<(Handle<Mesh>, Handle<WorldMaterial>)>;

/// `models` folders next to each `materials` folder.
fn models_dirs(materials: &[PathBuf]) -> Vec<PathBuf> {
    materials
        .iter()
        .filter_map(|m| m.parent().map(|p| p.join("models")))
        .filter(|m| m.is_dir())
        .collect()
}

/// Read `<name>` (a path under `models/`) and its `.vvd`/`.dx11.vtx` from the
/// first folder that has all three.
fn read_model(dirs: &[PathBuf], name: &str) -> Option<pf_fmt_mdl::Model> {
    let rel = name.trim_start_matches("models/");
    dirs.iter().find_map(|dir| {
        let mdl = dir.join(rel);
        let read = |ext: &str| std::fs::read(mdl.with_extension(ext)).ok();
        pf_fmt_mdl::load(&read("mdl")?, &read("vvd")?, &read("dx11.vtx")?).ok()
    })
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_props(
    mut commands: Commands,
    props: Res<StaticProps>,
    geometry: Res<MapGeometry>,
    materials_dirs: Res<MaterialsDirs>,
    scale: Res<LightingScale>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<WorldMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let dirs = models_dirs(&materials_dirs.0);
    let (sun_direction, sun_color) = crate::sun_uniforms(&geometry.0);
    let mut loaded: HashMap<u16, Option<Parts>> = HashMap::new();
    let mut material_cache: HashMap<String, Handle<WorldMaterial>> = HashMap::new();
    let (mut placed, mut missing) = (0, 0);

    for prop in &props.props {
        let parts = loaded.entry(prop.model).or_insert_with(|| {
            let name = props.names.get(prop.model as usize)?.to_ascii_lowercase();
            let model = read_model(&dirs, &name)?;
            let parts = model
                .meshes
                .iter()
                .map(|m| {
                    let mesh = Mesh::new(
                        PrimitiveTopology::TriangleList,
                        RenderAssetUsages::default(),
                    )
                    .with_inserted_attribute(
                        Mesh::ATTRIBUTE_POSITION,
                        m.positions.iter().map(|&p| to_bevy(p)).collect::<Vec<_>>(),
                    )
                    .with_inserted_attribute(
                        Mesh::ATTRIBUTE_NORMAL,
                        m.normals.iter().map(|&n| to_bevy(n)).collect::<Vec<_>>(),
                    )
                    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, m.uvs.clone())
                    .with_inserted_indices(Indices::U32(m.indices.clone()));
                    let key = m.materials.first().cloned().unwrap_or_default();
                    let material = material_cache
                        .entry(key)
                        .or_insert_with(|| {
                            // The first $cdmaterials folder that has the .vmt wins.
                            let loaded = m
                                .materials
                                .iter()
                                .find_map(|name| load_material(&materials_dirs.0, name));
                            let (texture, alpha) = match loaded {
                                Some(l) => (l.texture.map(|i| images.add(i)), l.alpha),
                                None => (None, AlphaMode::Opaque),
                            };
                            materials.add(WorldMaterial {
                                base: StandardMaterial {
                                    base_color: if texture.is_some() {
                                        Color::WHITE
                                    } else {
                                        color_for(m.materials.first().map_or("", |s| s))
                                    },
                                    base_color_texture: texture,
                                    alpha_mode: alpha,
                                    double_sided: true,
                                    cull_mode: None,
                                    ..default()
                                },
                                extension: WorldExtension {
                                    // y = 2: no lightmap; ambient (z) + sun x N.L (w).
                                    settings: Vec4::new(0.0, 2.0, scale.prop_ambient, scale.sun),
                                    texture2: None,
                                    lightmap_sky: None,
                                    lightmap_realtime: None,
                                    sun_direction,
                                    sun_color,
                                },
                            })
                        })
                        .clone();
                    (meshes.add(mesh), material)
                })
                .collect();
            Some(parts)
        });
        let Some(parts) = parts else {
            missing += 1;
            continue;
        };

        let transform = Transform {
            translation: to_bevy(prop.origin),
            rotation: source_rotation(prop.angles),
            scale: Vec3::splat(if prop.scale > 0.0 { prop.scale } else { 1.0 }),
        };
        commands
            .spawn((transform, Visibility::default()))
            .with_children(|parent| {
                for (mesh, material) in parts.iter() {
                    parent.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone())));
                }
            });
        placed += 1;
    }
    let models = loaded.values().filter(|p| p.is_some()).count();
    println!("props: {placed} placed from {models} models, {missing} skipped (model not found)");
}

/// Source angles (pitch, yaw, roll in degrees) as a Bevy rotation.
/// Source builds its rotation as yaw about Z, then pitch about Y, then roll
/// about X; we express that in Bevy's axes with the same change of basis as
/// `to_bevy` (x, y, z) -> (x, z, -y).
fn source_rotation([pitch, yaw, roll]: [f32; 3]) -> Quat {
    let r = Mat3::from_rotation_z(yaw.to_radians())
        * Mat3::from_rotation_y(pitch.to_radians())
        * Mat3::from_rotation_x(roll.to_radians());
    let c = Mat3::from_cols(Vec3::X, Vec3::NEG_Z, Vec3::Y);
    Quat::from_mat3(&(c * r * c.transpose()))
}
