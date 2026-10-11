//! Drawing a Titanfall map in Bevy, shared by the map viewer and the game:
//! world geometry with the original textures and baked lighting, static props,
//! fog, and the map's collision triangles. Everything is read from the
//! player's EA install through [`gamefs`]; nothing is bundled.
//!
//! Use: mount the install ([`gamefs::mount_install`]), add [`WorldPlugin`],
//! call [`load`] and insert the result with
//! [`LoadedMap::insert`], then run [`spawn_map`] and [`props::spawn_props`].
//! Everything they spawn carries [`MapEntity`] so a game can clear the map.

/// The game's files (re-exported so callers can write `pf_world::gamefs`).
pub use pf_gamefs as gamefs;
pub mod props;

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;
use pf_fmt_vtf::{Format, Vtf, vmt::Vmt};
use std::collections::HashMap;
use std::io;

/// Marks everything a map spawns, so it can be despawned in one go.
#[derive(Component)]
pub struct MapEntity;

/// The world material, its shader and the lighting settings.
pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(LightingScale::from_env())
            .add_plugins(MaterialPlugin::<WorldMaterial>::default());
        // Bake the shader into the exe so no assets folder is needed.
        embedded_asset!(app, "world.wgsl");
    }
}

/// A parsed map, ready to insert as resources.
pub struct LoadedMap {
    pub geometry: MapGeometry,
    pub props: props::StaticProps,
}

impl LoadedMap {
    pub fn insert(self, commands: &mut Commands) {
        commands.insert_resource(self.geometry);
        commands.insert_resource(self.props);
    }
}

/// Mount a map's pack and read its `.bsp` and spawn points (`name` like
/// `mp_colony`).
pub fn load(name: &str) -> io::Result<LoadedMap> {
    gamefs::mount_map(name)?;
    let data = gamefs::read(&format!("maps/{name}.bsp"))
        .ok_or_else(|| io::Error::other(format!("maps/{name}.bsp isn't in the install")))?;
    let bsp = pf_fmt_bsp::Bsp::parse(&data)?;
    let geometry = bsp.geometry()?;
    let (names, props) = bsp.static_props().unwrap_or_default();
    // The map's playable box from the worldspawn entity; the vertex bounds also
    // include far-away sky geometry.
    let (min, max) = world_box(&bsp).unwrap_or_else(|| geometry.bounds());
    Ok(LoadedMap {
        geometry: MapGeometry(geometry, min, max, find_spawn(name)),
        props: props::StaticProps { names, props },
    })
}

/// Where a player starts: feet position (Bevy space) and yaw (radians, Bevy
/// convention: 0 faces -Z). Falls back to the middle of the map, up high.
pub fn spawn_point(map: &MapGeometry) -> (Vec3, f32) {
    match &map.3 {
        // Source yaw 0 faces +X; Bevy yaw 0 faces -Z. See to_bevy.
        Some(s) => (to_bevy(s.origin), (s.yaw_degrees - 90.0).to_radians()),
        None => {
            let centre = (to_bevy(map.1) + to_bevy(map.2)) * 0.5;
            (centre + Vec3::Y * 1000.0, 0.0)
        }
    }
}

/// The world's collision triangles in Bevy space: every world mesh except
/// materials the player can pass through (sky, triggers, ...). Invisible walls
/// (`tools/toolsclip*`, `toolsnodraw`) are kept: they block players in game.
/// Props aren't included yet.
pub fn collision_triangles(map: &MapGeometry) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let g = &map.0;
    let positions: Vec<Vec3> = g.positions.iter().map(|&p| to_bevy(p)).collect();
    let passable = |material: u32| {
        let name = g
            .materials
            .get(material as usize)
            .map(|n| n.to_ascii_lowercase())
            .unwrap_or_default();
        [
            "toolsskybox",
            "toolstrigger",
            "toolsinvisible",
            "toolsfog",
            "toolsblock_los",
        ]
        .iter()
        .any(|t| name.ends_with(t))
    };
    let triangles = g
        .meshes
        .iter()
        .filter(|m| !passable(m.material))
        .flat_map(|m| m.triangles.iter().map(|t| t.map(|c| c.position)))
        .collect();
    (positions, triangles)
}

/// Geometry, playable box (min, max) and where to start the camera.
#[derive(Resource)]
pub struct MapGeometry(
    pub pf_fmt_bsp::Geometry,
    pub [f32; 3],
    pub [f32; 3],
    pub Option<Spawn>,
);

/// Roughly a standing pilot's eye height, in units.
pub const EYE_HEIGHT: f32 = 60.0;

pub struct Spawn {
    pub origin: [f32; 3],
    pub yaw_degrees: f32,
}

/// The first pilot spawn point from `maps/<map>_spawn.ent` (entity data,
/// KeyValues blocks).
pub fn find_spawn(name: &str) -> Option<Spawn> {
    let text = gamefs::read_text(&format!("maps/{name}_spawn.ent"))?;
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

/// Standard PBR inputs plus the original's lighting data; see `world.wgsl`.
pub type WorldMaterial = ExtendedMaterial<StandardMaterial, WorldExtension>;

/// The extra data `world.wgsl` reads. Bevy note: `AsBindGroup` turns these
/// fields into GPU bindings; 100+ keeps clear of the standard material's slots.
/// `Option` textures get a blank stand-in when missing.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct WorldExtension {
    #[texture(100)]
    #[sampler(101)]
    pub texture2: Option<Handle<Image>>,
    #[texture(102)]
    #[sampler(103)]
    pub lightmap_sky: Option<Handle<Image>>,
    #[texture(104)]
    pub lightmap_realtime: Option<Handle<Image>>,
    /// x: has texture2, y: has lightmap, z: sky scale, w: sun scale.
    #[uniform(105)]
    pub settings: Vec4,
    #[uniform(106)]
    pub sun_direction: Vec4,
    #[uniform(107)]
    pub sun_color: Vec4,
}

impl MaterialExtension for WorldExtension {
    fn fragment_shader() -> ShaderRef {
        "embedded://pf_world/world.wgsl".into()
    }
}

/// Brightness of the two lightmap terms, tuned by eye against reference shots. The exact decode isn't known yet, so
/// these are tunable: `PF_SKY_SCALE` and `PF_SUN_SCALE` environment variables.
#[derive(Resource, Clone, Copy)]
pub struct LightingScale {
    pub sky: f32,
    pub sun: f32,
    /// Flat ambient for props, which have no lightmap (PF_PROP_AMBIENT).
    pub prop_ambient: f32,
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
            prop_ambient: get("PF_PROP_AMBIENT", 0.3),
        }
    }
}

/// `PF_LM_DEBUG` picks a debug view in world.wgsl (0 = normal).
pub fn debug_view() -> f32 {
    std::env::var("PF_LM_DEBUG")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0)
}

/// A lightmap layer as a linear RGBA8 texture (no mips, clamped).
pub fn lightmap_image(width: u32, height: u32, data: Vec<u8>) -> Image {
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
pub fn world_box(bsp: &pf_fmt_bsp::Bsp) -> Option<([f32; 3], [f32; 3])> {
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

/// A file under `materials/` (e.g. `dev/noise_post.vtf`).
pub fn read_material_file(relative: &str) -> Option<Vec<u8>> {
    gamefs::read(&format!("materials/{relative}"))
}

/// A material's textures once they're on the GPU.
#[derive(Clone)]
pub struct LoadedTextures {
    texture: Option<Handle<Image>>,
    pub texture2: Option<Handle<Image>>,
    pub alpha: AlphaMode,
    pub decal: bool,
}

/// What we need from a `.vmt` to draw it.
pub struct LoadedMaterial {
    pub texture: Option<Image>,
    /// `$basetexture2` for blend materials.
    pub texture2: Option<Image>,
    pub alpha: AlphaMode,
    /// `$decal`: drawn on top of another surface, so it needs a depth offset.
    pub decal: bool,
    /// `$additive`: adds light instead of covering what's behind.
    pub additive: bool,
    /// An `Unlit*` shader: not affected by lighting.
    pub unlit: bool,
    /// `$color2` (the tint the game multiplies the texture by), if given.
    pub tint: Option<[f32; 3]>,
}

/// Read `<name>.vmt` and its textures. Any failure just means "no texture";
/// the caller falls back to a flat colour.
pub fn load_material(name: &str) -> Option<LoadedMaterial> {
    let text = read_material_file(&format!("{name}.vmt"))?;
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
        vtf_to_image(&read_material_file(&format!("{path}.vtf"))?)
    };
    Some(LoadedMaterial {
        texture: load("$basetexture"),
        texture2: load("$basetexture2"),
        alpha,
        decal: flag("$decal"),
        additive: flag("$additive"),
        unlit: vmt.shader.to_ascii_lowercase().starts_with("unlit"),
        tint: vmt.params.get("$color2").and_then(|v| {
            let n: Vec<f32> = v
                .trim_matches(|c| c == '[' || c == ']' || c == '"' || c == ' ')
                .split_whitespace()
                .filter_map(|x| x.parse().ok())
                .collect();
            (n.len() == 3).then(|| [n[0], n[1], n[2]])
        }),
    })
}

/// Turn a VTF into a GPU texture. DXT data is uploaded as-is (BC1/2/3) and the
/// GPU decodes it, the same way the original game does.
pub fn vtf_to_image(bytes: &[u8]) -> Option<Image> {
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

/// Source maps are Z-up with X forward and Y left; Bevy is Y-up with -Z forward.
/// This rotation keeps handedness, so triangles keep their winding.
pub fn to_bevy(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], p[2], -p[1])
}

/// The sun as shader uniforms: (direction it travels, colour + debug view in w).
pub fn sun_uniforms(g: &pf_fmt_bsp::Geometry) -> (Vec4, Vec4) {
    match g.sun {
        Some(sun) => (
            to_bevy(sun.direction).normalize_or_zero().extend(0.0),
            Vec4::new(sun.color[0], sun.color[1], sun.color[2], debug_view()),
        ),
        None => (Vec4::new(0.0, -1.0, 0.0, 0.0), Vec4::ZERO),
    }
}

pub fn spawn_map(
    mut commands: Commands,
    geometry: Res<MapGeometry>,
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
    let (sun_direction, sun_color) = sun_uniforms(g);
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
                load_material(&name).map(|m| LoadedTextures {
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
            MapEntity,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(material)),
        ));
    }

    println!("materials: {textured} textured, {blended} blended, {flat} flat colour");

    // Not used for surface lighting (world.wgsl uses the baked data), but Bevy's
    // fog takes its glow direction from it, so point it along the map's sun.
    commands.spawn((
        MapEntity,
        DirectionalLight {
            illuminance: 8000.0,
            ..default()
        },
        Transform::default().looking_to(sun_direction.truncate(), Vec3::Y),
    ));
}

/// Distance haze approximating the map's `env_fog_controller` (from
/// `mp_colony_env.ent`): colour 187 174 180, max density 0.5, half-distance
/// about 2000-5000 units starting 500 out, and a blue-grey glow (142 169 189,
/// strength 3) looking towards the sun. Bevy's fog is simpler than Respawn's
/// (no height bands), so this is a close match, not exact.
// TODO: read these values from the .ent file instead of hard-coding Colony's.
pub fn colony_fog() -> DistanceFog {
    let half_distance = 7000.0;
    DistanceFog {
        // Alpha caps how much fog is mixed in, like fogmaxdensity.
        color: Color::srgba_u8(187, 174, 180, 100),
        directional_light_color: Color::srgba_u8(142, 169, 189, 120),
        directional_light_exponent: 8.0,
        falloff: FogFalloff::Exponential {
            density: std::f32::consts::LN_2 / half_distance,
        },
    }
}

/// A stable, fairly bright colour per material name.
pub fn color_for(name: &str) -> Color {
    let hash = name.bytes().fold(0x811C_9DC5u32, |h, b| {
        (h ^ b as u32).wrapping_mul(0x0100_0193)
    });
    let hue = (hash % 360) as f32;
    Color::hsl(hue, 0.35, 0.6)
}
