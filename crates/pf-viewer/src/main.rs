//! Map viewer: loads a Titanfall `.bsp` and lets you fly around it with the
//! original textures (no lightmaps or props yet).
//!
//! Usage: `cargo run -p pf-viewer --release -- <path/to/mp_colony.bsp> [materials_dir]`
//!
//! `materials_dir` defaults to the nearest `materials` folder above the map
//! (the layout `pf-pc vpk-extract` produces). Materials that can't be found
//! fall back to a flat colour.
//!
//! Controls: click to capture the mouse (Esc releases), WASD to fly, Space/Ctrl
//! up/down, Shift for speed.
//!
//! Bevy note: a Bevy app is a list of plugins and *systems* (plain functions).
//! Systems ask for the data they need in their parameters and Bevy passes it in.

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use pf_fmt_vtf::{Format, Vtf, vmt::Vmt};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Flying speed in game units (~inches) per second.
const SPEED: f32 = 800.0;
const FAST: f32 = 4.0;
const MOUSE_SENSITIVITY: f32 = 0.002;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: pf-viewer <map.bsp> [materials_dir]");
        std::process::exit(2);
    });
    let mut materials_dirs: Vec<PathBuf> = std::env::args().skip(2).map(PathBuf::from).collect();
    if materials_dirs.is_empty() {
        materials_dirs = find_materials_dirs(Path::new(&path));
    }
    println!("materials from: {materials_dirs:?}");
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

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Pilotframe map viewer (unofficial, not affiliated with EA or Respawn)".into(),
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
    .insert_resource(MaterialsDirs(materials_dirs))
    .add_plugins(MaterialPlugin::<BlendMaterial>::default())
    .add_systems(Startup, spawn_map)
    .add_systems(Update, (capture_cursor, fly));
    // Bake the blend shader into the exe so the viewer needs no assets folder.
    embedded_asset!(app, "blend.wgsl");
    app.run();
}

fn fail(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}

#[derive(Resource)]
struct MapGeometry(pf_fmt_bsp::Geometry, [f32; 3], [f32; 3]);

/// Folders to look for `.vmt`/`.vtf` files in, in order.
#[derive(Resource)]
struct MaterialsDirs(Vec<PathBuf>);

/// Standard PBR material plus a second texture, for blend materials.
type BlendMaterial = ExtendedMaterial<StandardMaterial, BlendExtension>;

/// The extra data `blend.wgsl` reads. Bevy note: `AsBindGroup` turns these
/// fields into GPU bindings; 100+ keeps clear of the standard material's slots.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
struct BlendExtension {
    #[texture(100)]
    #[sampler(101)]
    texture2: Handle<Image>,
}

impl MaterialExtension for BlendExtension {
    fn fragment_shader() -> ShaderRef {
        "embedded://pf_viewer/blend.wgsl".into()
    }
}

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

/// Walk up from the map file looking for `materials` folders: the map's own
/// (nearest), then a shared `common/materials` (what `pf-pc materials-for` writes).
fn find_materials_dirs(map: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for dir in map.ancestors() {
        for candidate in [dir.join("materials"), dir.join("common").join("materials")] {
            if candidate.is_dir() && !found.contains(&candidate) {
                found.push(candidate);
            }
        }
    }
    found
}

/// Read the first copy of `relative` found in `dirs`.
fn read_first(dirs: &[PathBuf], relative: &str) -> Option<Vec<u8>> {
    dirs.iter()
        .find_map(|d| std::fs::read(d.join(relative)).ok())
}

/// What we need from a `.vmt` to draw it.
struct LoadedMaterial {
    texture: Option<Image>,
    /// `$basetexture2` for blend materials.
    texture2: Option<Image>,
    alpha: AlphaMode,
}

/// Read `<name>.vmt` and its textures. Any failure just means "no texture";
/// the caller falls back to a flat colour.
fn load_material(dirs: &[PathBuf], name: &str) -> Option<LoadedMaterial> {
    let text = read_first(dirs, &format!("{name}.vmt"))?;
    let vmt = Vmt::parse(&String::from_utf8_lossy(&text));
    let flag = |k: &str| vmt.params.get(k).is_some_and(|v| v.trim() == "1");
    let alpha = if flag("$translucent") {
        AlphaMode::Blend
    } else if flag("$alphatest") {
        AlphaMode::Mask(0.5)
    } else {
        AlphaMode::Opaque
    };
    let load = |key: &str| {
        let raw = vmt.params.get(key)?;
        let path = raw.replace('\\', "/").to_ascii_lowercase();
        let path = path.trim_end_matches(".vtf");
        vtf_to_image(&read_first(dirs, &format!("{path}.vtf"))?)
    };
    Some(LoadedMaterial {
        texture: load("$basetexture"),
        texture2: load("$basetexture2"),
        alpha,
    })
}

/// Turn a VTF into a GPU texture. DXT data is uploaded as-is (BC1/2/3) and the
/// GPU decodes it, the same way the original game does.
fn vtf_to_image(bytes: &[u8]) -> Option<Image> {
    let vtf = Vtf::parse(bytes).ok()?;
    if vtf.is_cubemap() || vtf.depth > 1 {
        return None;
    }
    let mut data = vtf.mip_chain().ok()??;
    let format = match vtf.format {
        Format::Dxt1 => TextureFormat::Bc1RgbaUnormSrgb,
        Format::Dxt3 => TextureFormat::Bc2RgbaUnormSrgb,
        Format::Dxt5 => TextureFormat::Bc3RgbaUnormSrgb,
        Format::Bgra8888 => TextureFormat::Bgra8UnormSrgb,
        Format::Rgba8888 => TextureFormat::Rgba8UnormSrgb,
        Format::Bgr888 => {
            data = data
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect();
            TextureFormat::Bgra8UnormSrgb
        }
        Format::Other(_) => return None,
    };
    // Block-compressed textures must be a multiple of 4 pixels across.
    let compressed = matches!(vtf.format, Format::Dxt1 | Format::Dxt3 | Format::Dxt5);
    if compressed && (!vtf.width.is_multiple_of(4) || !vtf.height.is_multiple_of(4)) {
        return None;
    }

    let mut image = Image {
        data: Some(data),
        ..default()
    };
    image.texture_descriptor.size = Extent3d {
        width: vtf.width,
        height: vtf.height,
        depth_or_array_layers: 1,
    };
    image.texture_descriptor.dimension = TextureDimension::D2;
    image.texture_descriptor.format = format;
    image.texture_descriptor.mip_level_count = vtf.mip_count;
    image.asset_usage = RenderAssetUsages::RENDER_WORLD;
    // World textures tile, so wrap UVs; trilinear + anisotropic filtering.
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    Some(image)
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
    materials_dirs: Res<MaterialsDirs>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut blend_materials: ResMut<Assets<BlendMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let g = &geometry.0;

    // Group triangles (with UVs and colours) by material: one Bevy mesh each.
    #[derive(Default)]
    struct Group {
        tris: Vec<[u32; 3]>,
        uvs: Vec<[[f32; 2]; 3]>,
        colors: Vec<[[u8; 4]; 3]>,
    }
    let mut groups: HashMap<u32, Group> = HashMap::new();
    for m in &g.meshes {
        let group = groups.entry(m.material).or_default();
        group.tris.extend(&m.triangles);
        group.uvs.extend(&m.uvs);
        group.colors.extend(&m.colors);
    }
    let (mut textured, mut blended, mut flat) = (0, 0, 0);

    for (material, group) in groups {
        let name = g
            .materials
            .get(material as usize)
            .map(|n| n.to_ascii_lowercase())
            .unwrap_or_default();
        // Tool materials (skybox, nodraw, triggers, ...) are invisible in game.
        if name.starts_with("tools/") {
            continue;
        }
        let loaded = load_material(&materials_dirs.0, &name);
        let is_blend = loaded.as_ref().is_some_and(|m| m.texture2.is_some());

        // Unshared vertices with one normal per triangle (flat shading) until we
        // read the map's own normals and lightmaps.
        let n_verts = group.tris.len() * 3;
        let mut positions = Vec::with_capacity(n_verts);
        let mut normals = Vec::with_capacity(n_verts);
        let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(n_verts);
        let mut blend: Vec<[f32; 4]> = Vec::with_capacity(n_verts);
        for ((t, uv), col) in group.tris.iter().zip(&group.uvs).zip(&group.colors) {
            let [a, b, c] = t.map(|i| to_bevy(g.positions[i as usize]));
            let n = (b - a).cross(c - a).normalize_or_zero();
            positions.extend([a, b, c]);
            normals.extend([n, n, n]);
            uvs.extend(uv);
            // Only the alpha matters: it's the blend amount for blend.wgsl.
            blend.extend(col.map(|c| [1.0, 1.0, 1.0, c[3] as f32 / 255.0]));
        }
        let count = positions.len() as u32;
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32((0..count).collect()));
        if is_blend {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, blend);
        }

        let (texture, texture2, alpha) = match loaded {
            Some(m) => (
                m.texture.map(|i| images.add(i)),
                m.texture2.map(|i| images.add(i)),
                m.alpha,
            ),
            None => (None, None, AlphaMode::Opaque),
        };
        match (&texture, &texture2) {
            (Some(_), Some(_)) => blended += 1,
            (Some(_), None) => textured += 1,
            _ => flat += 1,
        }
        let base = StandardMaterial {
            base_color: if texture.is_some() {
                Color::WHITE
            } else {
                color_for(&name)
            },
            base_color_texture: texture,
            alpha_mode: alpha,
            perceptual_roughness: 0.9,
            // Winding isn't confirmed yet, so draw both sides.
            cull_mode: None,
            double_sided: true,
            ..default()
        };

        let mesh = Mesh3d(meshes.add(mesh));
        match texture2 {
            Some(texture2) => commands.spawn((
                mesh,
                MeshMaterial3d(blend_materials.add(BlendMaterial {
                    base,
                    extension: BlendExtension { texture2 },
                })),
            )),
            None => commands.spawn((mesh, MeshMaterial3d(materials.add(base)))),
        };
    }
    println!("materials: {textured} textured, {blended} blended, {flat} flat colour");

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
