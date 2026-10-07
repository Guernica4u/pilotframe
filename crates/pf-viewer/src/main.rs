//! Map viewer: loads a Titanfall `.bsp` and lets you fly around it with the
//! original textures and baked lighting (no props or skybox yet).
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
    // Overcast grey close to the fog colour until the real skybox is drawn.
    .insert_resource(ClearColor(Color::srgb_u8(170, 172, 176)))
    .insert_resource(GlobalAmbientLight {
        brightness: 1500.0,
        ..default()
    })
    .insert_resource(MapGeometry(
        geometry,
        min,
        max,
        find_spawn(Path::new(&path)),
    ))
    .insert_resource(MaterialsDirs(materials_dirs))
    .insert_resource(LightingScale::from_env())
    .add_plugins(MaterialPlugin::<WorldMaterial>::default())
    .add_systems(Startup, spawn_map)
    .add_systems(Update, (capture_cursor, fly));
    // Bake the shader into the exe so the viewer needs no assets folder.
    embedded_asset!(app, "world.wgsl");
    app.run();
}

fn fail(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}

/// Geometry, playable box (min, max) and where to start the camera.
#[derive(Resource)]
struct MapGeometry(pf_fmt_bsp::Geometry, [f32; 3], [f32; 3], Option<Spawn>);

/// Roughly a standing pilot's eye height, in units.
const EYE_HEIGHT: f32 = 60.0;

struct Spawn {
    origin: [f32; 3],
    yaw_degrees: f32,
}

/// The first pilot spawn point from `<map>_spawn.ent` (entity data, KeyValues
/// blocks), found in a `maps` folder above the map file.
fn find_spawn(map: &Path) -> Option<Spawn> {
    let stem = map.file_stem()?.to_str()?;
    let file = map
        .ancestors()
        .map(|dir| dir.join("maps").join(format!("{stem}_spawn.ent")))
        .find(|f| f.is_file())?;
    let text = std::fs::read_to_string(file).ok()?;
    for block in text.split('{').skip(1) {
        let block = block.split('}').next().unwrap_or_default();
        let value = |key: &str| {
            block.lines().find_map(|l| {
                let parts: Vec<&str> = l.split('"').collect();
                (parts.get(1) == Some(&key))
                    .then(|| parts.get(3).copied())
                    .flatten()
            })
        };
        if value("classname") != Some("info_spawnpoint_human_start") {
            continue;
        }
        let nums = |s: &str| -> Vec<f32> {
            s.split_whitespace()
                .filter_map(|n| n.parse().ok())
                .collect()
        };
        let origin = nums(value("origin")?);
        let angles = nums(value("angles").unwrap_or("0 0 0"));
        return Some(Spawn {
            origin: [*origin.first()?, *origin.get(1)?, *origin.get(2)?],
            yaw_degrees: angles.get(1).copied().unwrap_or(0.0),
        });
    }
    None
}

/// Folders to look for `.vmt`/`.vtf` files in, in order.
#[derive(Resource)]
struct MaterialsDirs(Vec<PathBuf>);

/// Standard PBR inputs plus the original's lighting data; see `world.wgsl`.
type WorldMaterial = ExtendedMaterial<StandardMaterial, WorldExtension>;

/// The extra data `world.wgsl` reads. Bevy note: `AsBindGroup` turns these
/// fields into GPU bindings; 100+ keeps clear of the standard material's slots.
/// `Option` textures get a blank stand-in when missing.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
struct WorldExtension {
    #[texture(100)]
    #[sampler(101)]
    texture2: Option<Handle<Image>>,
    #[texture(102)]
    #[sampler(103)]
    lightmap_sky: Option<Handle<Image>>,
    #[texture(104)]
    lightmap_realtime: Option<Handle<Image>>,
    /// x: has texture2, y: has lightmap, z: sky scale, w: sun scale.
    #[uniform(105)]
    settings: Vec4,
    #[uniform(106)]
    sun_direction: Vec4,
    #[uniform(107)]
    sun_color: Vec4,
}

impl MaterialExtension for WorldExtension {
    fn fragment_shader() -> ShaderRef {
        "embedded://pf_viewer/world.wgsl".into()
    }
}

/// Brightness of the two lightmap terms, tuned by eye against reference shots. The exact decode isn't known yet, so
/// these are tunable: `PF_SKY_SCALE` and `PF_SUN_SCALE` environment variables.
#[derive(Resource, Clone, Copy)]
struct LightingScale {
    sky: f32,
    sun: f32,
}

impl LightingScale {
    fn from_env() -> Self {
        let get = |key: &str, default: f32| {
            std::env::var(key)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default)
        };
        Self {
            sky: get("PF_SKY_SCALE", 1.5),
            sun: get("PF_SUN_SCALE", 0.3),
        }
    }
}

/// `PF_LM_DEBUG` picks a debug view in world.wgsl (0 = normal).
fn debug_view() -> f32 {
    std::env::var("PF_LM_DEBUG")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0)
}

/// A lightmap layer as a linear RGBA8 texture (no mips, clamped).
fn lightmap_image(width: u32, height: u32, data: Vec<u8>) -> Image {
    let mut image = Image {
        data: Some(data),
        ..default()
    };
    image.texture_descriptor.size = Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    image.texture_descriptor.format = TextureFormat::Rgba8Unorm;
    image.asset_usage = RenderAssetUsages::RENDER_WORLD;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
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

/// A material's textures once they're on the GPU.
#[derive(Clone)]
struct LoadedTextures {
    texture: Option<Handle<Image>>,
    texture2: Option<Handle<Image>>,
    alpha: AlphaMode,
    decal: bool,
}

/// What we need from a `.vmt` to draw it.
struct LoadedMaterial {
    texture: Option<Image>,
    /// `$basetexture2` for blend materials.
    texture2: Option<Image>,
    alpha: AlphaMode,
    /// `$decal`: drawn on top of another surface, so it needs a depth offset.
    decal: bool,
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
        decal: flag("$decal"),
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
    scale: Res<LightingScale>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<WorldMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let g = &geometry.0;

    // Lightmap pages become textures shared by every mesh on that page.
    let pages: Vec<(Handle<Image>, Handle<Image>)> = g
        .lightmaps
        .iter()
        .map(|p| {
            (
                images.add(lightmap_image(p.width, p.height, p.sky.clone())),
                images.add(lightmap_image(p.width, p.height, p.realtime.clone())),
            )
        })
        .collect();
    let (sun_direction, sun_color) = match g.sun {
        Some(sun) => (
            to_bevy(sun.direction).normalize_or_zero().extend(0.0),
            Vec4::new(sun.color[0], sun.color[1], sun.color[2], debug_view()),
        ),
        None => (Vec4::new(0.0, -1.0, 0.0, 0.0), Vec4::ZERO),
    };
    println!("{} lightmap pages, sun: {:?}", pages.len(), g.sun);

    // Group triangles by (material, lightmap page): one Bevy mesh each.
    type Key = (u32, Option<u16>);
    let mut groups: HashMap<Key, Vec<[pf_fmt_bsp::Corner; 3]>> = HashMap::new();
    for m in &g.meshes {
        groups
            .entry((m.material, m.lightmap))
            .or_default()
            .extend(&m.triangles);
    }
    let (mut textured, mut blended, mut flat) = (0, 0, 0);
    let mut loaded_materials: HashMap<u32, Option<LoadedTextures>> = HashMap::new();

    for ((material, lightmap), tris) in groups {
        let name = g
            .materials
            .get(material as usize)
            .map(|n| n.to_ascii_lowercase())
            .unwrap_or_default();
        // Tool materials (skybox, nodraw, triggers, ...) are invisible in game.
        if name.starts_with("tools/") {
            continue;
        }
        // Load each material's textures once, even if it spans several pages.
        let loaded = loaded_materials
            .entry(material)
            .or_insert_with(|| {
                load_material(&materials_dirs.0, &name).map(|m| LoadedTextures {
                    texture: m.texture.map(|i| images.add(i)),
                    texture2: m.texture2.map(|i| images.add(i)),
                    alpha: m.alpha,
                    decal: m.decal,
                })
            })
            .clone();
        let is_blend = loaded.as_ref().is_some_and(|m| m.texture2.is_some());

        let n_verts = tris.len() * 3;
        let mut positions = Vec::with_capacity(n_verts);
        let mut normals = Vec::with_capacity(n_verts);
        let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(n_verts);
        let mut lightmap_uvs: Vec<[f32; 2]> = Vec::with_capacity(n_verts);
        let mut blend: Vec<[f32; 4]> = Vec::with_capacity(n_verts);
        for tri in &tris {
            for c in tri {
                positions.push(to_bevy(g.positions[c.position as usize]));
                normals.push(to_bevy(c.normal).normalize_or_zero());
                uvs.push(c.uv);
                lightmap_uvs.push(c.lightmap_uv);
                // Only the alpha matters: it's the blend amount.
                blend.push([1.0, 1.0, 1.0, c.color[3] as f32 / 255.0]);
            }
        }
        let count = positions.len() as u32;
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, lightmap_uvs)
        .with_inserted_indices(Indices::U32((0..count).collect()));
        if is_blend {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, blend);
        }

        let (texture, texture2, alpha, decal) = match loaded {
            Some(m) => (m.texture, m.texture2, m.alpha, m.decal),
            None => (None, None, AlphaMode::Opaque, false),
        };
        match (&texture, &texture2) {
            (Some(_), Some(_)) => blended += 1,
            (Some(_), None) => textured += 1,
            _ => flat += 1,
        }
        let page = lightmap.and_then(|l| pages.get(l as usize));
        let material = WorldMaterial {
            base: StandardMaterial {
                base_color: if texture.is_some() {
                    Color::WHITE
                } else {
                    color_for(&name)
                },
                base_color_texture: texture,
                alpha_mode: alpha,
                // Posters and grime sit exactly on a wall; nudge them towards the
                // camera so they stop flickering through it (z-fighting).
                depth_bias: if decal { 100.0 } else { 0.0 },
                // Winding isn't confirmed yet, so draw both sides.
                cull_mode: None,
                double_sided: true,
                ..default()
            },
            extension: WorldExtension {
                settings: Vec4::new(
                    if texture2.is_some() { 1.0 } else { 0.0 },
                    if page.is_some() { 1.0 } else { 0.0 },
                    scale.sky,
                    scale.sun,
                ),
                texture2,
                lightmap_sky: page.map(|p| p.0.clone()),
                lightmap_realtime: page.map(|p| p.1.clone()),
                sun_direction,
                sun_color,
            },
        };
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(material)),
        ));
    }

    println!("materials: {textured} textured, {blended} blended, {flat} flat colour");

    // Not used for surface lighting (world.wgsl uses the baked data), but Bevy's
    // fog takes its glow direction from it, so point it along the map's sun.
    commands.spawn((
        DirectionalLight {
            illuminance: 8000.0,
            ..default()
        },
        Transform::default().looking_to(sun_direction.truncate(), Vec3::Y),
    ));

    // Start where a pilot would: the first pilot spawn point, at eye height.
    // Without a spawn file, start above the middle of the map looking down.
    let (translation, yaw, pitch) = match &geometry.3 {
        Some(spawn) => (
            to_bevy(spawn.origin) + Vec3::Y * EYE_HEIGHT,
            // Source yaw 0 faces +X; Bevy yaw 0 faces -Z. See to_bevy.
            (spawn.yaw_degrees - 90.0).to_radians(),
            0.0,
        ),
        None => {
            let (min, max) = (geometry.1, geometry.2);
            let centre = (to_bevy(min) + to_bevy(max)) * 0.5;
            let height = (max[2] - min[2]).max(1000.0) + 1500.0;
            (centre + Vec3::Y * height, 0.0, -0.7)
        }
    };
    commands.spawn((
        Camera3d::default(),
        // Default near/far planes assume metres; our units are inches.
        Projection::Perspective(PerspectiveProjection {
            fov: 70f32.to_radians(),
            near: 4.0,
            far: 100_000.0,
            ..default()
        }),
        Transform::from_translation(translation).with_rotation(Quat::from_euler(
            EulerRot::YXZ,
            yaw,
            pitch,
            0.0,
        )),
        FlyCamera { yaw, pitch },
        colony_fog(),
    ));
}

/// Distance haze approximating the map's `env_fog_controller` (from
/// `mp_colony_env.ent`): colour 187 174 180, max density 0.5, half-distance
/// about 2000-5000 units starting 500 out, and a blue-grey glow (142 169 189,
/// strength 3) looking towards the sun. Bevy's fog is simpler than Respawn's
/// (no height bands), so this is a close match, not exact.
// TODO: read these values from the .ent file instead of hard-coding Colony's.
fn colony_fog() -> DistanceFog {
    let half_distance = 5000.0;
    DistanceFog {
        // Alpha caps how much fog is mixed in, like fogmaxdensity.
        color: Color::srgba_u8(187, 174, 180, 128),
        directional_light_color: Color::srgba_u8(142, 169, 189, 120),
        directional_light_exponent: 8.0,
        falloff: FogFalloff::Exponential {
            density: std::f32::consts::LN_2 / half_distance,
        },
    }
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
