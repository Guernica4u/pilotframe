//! The first-person weapon and arms ("viewmodel"), animated with the weapon's own
//! animations and drawn by a second camera on top of the world, as Source does: it
//! never clips into walls and uses the pilot's narrower `viewmodelfov` (54).
//!
//! How the animations are layered (hip/aim base poses, idle and fire as deltas on
//! top, sprint and reload replacing the base) follows the sequences in the model:
//! the aim-in/out animations are full poses, idle and attack are flagged as
//! deltas with a hip and an aim version each. Timings come from the weapon's
//! settings file (`scripts/weapons/mp_weapon_rspn101.txt`).
//!
//! Models and textures come from the player's own files ([`crate::data`]).
//! Without them
//! there's simply no weapon on screen.

use std::path::{Path, PathBuf};

use crate::loadout::{DEFAULT_PRESET, Loadout, WeaponScript};

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::math::Affine3A;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use leafwing_input_manager::prelude::*;
use pf_fmt_mdl::anim::{self, Animation, Bone, Pose};

use crate::Screen;
use crate::input::Action;
use crate::matproxy::{self, ProxyMaterial};
use crate::player::Player;
use crate::recoil::{Recoil, RecoilData};
use crate::self_illum::{SelfIllum, SelfIllumMaterial};
use crate::settings::Settings;
use crate::spread::{Spread, SpreadData, SpreadInput};
use crate::sway::{self, Sway};
use crate::tuning::{Tuning, TuningAsset};
use crate::unlit_two::UnlitTwoTexture;
use crate::view;

/// The viewmodel lives on its own render layer, seen only by its own camera.
const LAYER: usize = 1;
/// Pilot `viewmodelfov` (Source convention, like `fov`).
const VIEWMODEL_FOV: f32 = 54.0;
/// Militia pilot arms (pilot `armsmodel_militia`).
const ARMS: &str = "models/weapons/arms/pov_mcor_pilot_male_br.mdl";
/// Blend time in and out of reload and draw animations, seconds.
const ACTION_FADE: f32 = 0.1;
/// Faster than this on the ground counts as sprinting for the gun (walk is 180.6).
const SPRINT_SPEED: f32 = 190.0;
/// The player tick the game runs weapon spread on, seconds.
const SPREAD_TICK: f32 = 1.0 / 60.0;

pub struct ViewmodelPlugin;

impl Plugin for ViewmodelPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<UnlitTwoTexture>::default())
            .add_plugins(MaterialPlugin::<SelfIllumMaterial>::default())
            .add_systems(
                Update,
                (animate, drive_proxies)
                    .chain()
                    .after(crate::player::place_camera)
                    .run_if(in_state(Screen::InGame)),
            );
    }
}

/// The weapon's numbers from its settings file.
struct WeaponData {
    viewmodel: String,
    /// Localisation token for the HUD name, e.g. `#WPN_RSPN101_SHORT`.
    short_name: String,
    fire_rate: f32,
    clip: u32,
    /// All the rounds you spawn with, the loaded magazine included
    /// (`ammo_default_total`).
    ammo_total: u32,
    /// `ammo_stockpile_max`.
    max_carry: u32,
    spread: SpreadData,
    /// Body group name → sub-model (`bodygroupN_name` / `bodygroupN_set`).
    bodygroups: Vec<(String, usize)>,
    /// Which alternate idle/attack/aim animations to use (`anim_alt_idleAttack`;
    /// 0 = the plain ones).
    anim_alt: u32,
    reload_time: f32,
    reload_empty_time: f32,
    deploy_time: f32,
    zoom_time_in: f32,
    zoom_time_out: f32,
    zoom_fov: f32,
    raise_time: f32,
    lower_time: f32,
    /// Sway and bob (`sway_*`, `bob_*`).
    sway: sway::Settings,
    /// Recoil (`viewkick_*`).
    recoil: RecoilData,
}

impl WeaponData {
    /// The loadout's primary weapon's settings, with its attachment and the
    /// passives applied as mods ([`crate::loadout`]).
    fn load(dir: &Path, loadout: &Loadout) -> Option<Self> {
        let mut mods: Vec<&str> = loadout
            .primary_attachment
            .iter()
            .map(String::as_str)
            .collect();
        mods.extend(loadout.passives.iter().map(String::as_str));
        let script = WeaponScript::load(dir, &loadout.primary, &mods)?;
        let sway = sway::Settings::from_script(&script);
        let recoil = RecoilData::from_script(dir, &script);
        let kv = script.values;
        let num = |k: &str, default: f32| {
            kv.get(k)
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(default)
        };
        let bodygroups = (1..32)
            .filter_map(|i| {
                let name = kv.get(&format!("bodygroup{i}_name"))?;
                Some((
                    name.clone(),
                    num(&format!("bodygroup{i}_set"), 0.0) as usize,
                ))
            })
            .collect();
        Some(Self {
            viewmodel: kv.get("viewmodel")?.replace('\\', "/").to_ascii_lowercase(),
            short_name: kv.get("shortprintname").cloned().unwrap_or_default(),
            fire_rate: num("fire_rate", 10.0),
            clip: num("ammo_clip_size", 30.0) as u32,
            ammo_total: num("ammo_default_total", 0.0) as u32,
            max_carry: num("ammo_stockpile_max", 0.0) as u32,
            spread: SpreadData::from_script(|k| kv.get(k).and_then(|v| v.trim().parse().ok())),
            bodygroups,
            anim_alt: num("anim_alt_idleAttack", 0.0) as u32,
            reload_time: num("reload_time", 2.0),
            reload_empty_time: num("reloadempty_time", 2.5),
            deploy_time: num("deploy_time", 1.0),
            zoom_time_in: num("zoom_time_in", 0.3),
            zoom_time_out: num("zoom_time_out", 0.3),
            zoom_fov: num("zoom_fov", 50.0),
            raise_time: num("raise_time", 0.3),
            lower_time: num("lower_time", 0.25),
            sway,
            recoil,
        })
    }
}

/// A model drawn with CPU skinning: its skeleton and its meshes' bind data.
struct Rig {
    bones: Vec<Bone>,
    meshes: Vec<SkinnedMesh>,
}

struct SkinnedMesh {
    handle: Handle<Mesh>,
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    bones: Vec<[u8; 3]>,
    weights: Vec<[f32; 3]>,
}

/// Something that replaces the gun's normal pose for a while (reload, draw).
struct GunAction {
    anim: usize,
    /// Seconds into the action, and how fast the animation plays.
    time: f32,
    rate: f32,
    /// Refill the magazine when this finishes.
    reload: bool,
}

/// Indexes of the animations we use, by name.
struct AnimIds {
    idle: Option<usize>,
    idle_ads: Option<usize>,
    attack: Option<usize>,
    attack_ads: Option<usize>,
    sprint: Option<usize>,
    reload: Option<usize>,
    reload_empty: Option<usize>,
    draw: Option<usize>,
}

#[derive(Component)]
pub struct Viewmodel {
    mdl: Vec<u8>,
    weapon: Rig,
    /// The arms, and for each arm bone the weapon bone with the same name.
    arms: Option<(Rig, Vec<Option<usize>>)>,
    anims: Vec<Animation>,
    data: WeaponData,
    camera_bone: usize,
    hip: Pose,
    aim: Pose,
    ids: AnimIds,
    clock: f32,
    ads: f32,
    sprint: f32,
    since_shot: f32,
    cooldown: f32,
    ammo: u32,
    /// Spare rounds, which reloads draw from.
    reserve: u32,
    spread: Spread,
    /// Time not yet run through the spread's fixed ticks, seconds.
    spread_pending: f32,
    action: Option<GunAction>,
    sway: Sway,
    recoil: Recoil,
    /// The sway's pivot attachments (hip, aimed) and how far the pivot has
    /// moved from the hip one to the aimed one, 0..1.
    pivots: [Option<pf_fmt_mdl::Attachment>; 2],
    pivot_blend: f32,
    /// The holo-sight value the reticles' `HoloSight` proxy shows, 0..1.
    holo: f32,
}

/// What the HUD shows about the gun.
impl Viewmodel {
    /// Rounds left in the magazine.
    pub fn ammo(&self) -> u32 {
        self.ammo
    }

    /// The magazine's size.
    pub fn clip_size(&self) -> u32 {
        self.data.clip
    }

    /// Spare rounds.
    pub fn reserve(&self) -> u32 {
        self.reserve
    }

    /// The weapon's short name, as a localisation token (`#WPN_..._SHORT`).
    pub fn short_name(&self) -> &str {
        &self.data.short_name
    }

    /// How far into aiming down sights, 0..1.
    pub fn ads(&self) -> f32 {
        self.ads
    }

    /// How far lowered for sprinting, 0..1.
    pub fn sprint(&self) -> f32 {
        self.sprint
    }

    /// The weapon's spread right now, degrees (what the crosshair shows).
    pub fn spread(&self) -> f32 {
        self.spread.degrees(self.ads)
    }

    /// Reloading right now.
    pub fn reloading(&self) -> bool {
        self.action.as_ref().is_some_and(|a| a.reload)
    }
}

/// Camera-bone space (x left, y up, z forward) to Bevy camera space (x right,
/// y up, looking down -z).
fn to_view(p: Vec3) -> Vec3 {
    Vec3::new(-p.x, p.y, -p.z)
}

fn read_model(
    dir: &Path,
    rel: &str,
    bodygroups: &[(String, usize)],
) -> Option<(Vec<u8>, pf_fmt_mdl::Model)> {
    let path = dir.join(rel);
    let read = |ext: &str| std::fs::read(path.with_extension(ext)).ok();
    let mdl = read("mdl")?;
    let body = |name: &str| {
        bodygroups
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map_or(0, |&(_, set)| set)
    };
    let model = pf_fmt_mdl::load_with_body(&mdl, &read("vvd")?, &read("dx11.vtx")?, body).ok()?;
    Some((mdl, model))
}

/// Build a rig's meshes (positions get overwritten every frame by skinning).
#[allow(clippy::too_many_arguments)]
fn build_rig(
    commands: &mut Commands,
    parent: Entity,
    mdl: &[u8],
    model: &pf_fmt_mdl::Model,
    material_dirs: &[PathBuf],
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    reticles: &mut Assets<UnlitTwoTexture>,
    glows: &mut Assets<SelfIllumMaterial>,
    images: &mut Assets<Image>,
) -> Option<Rig> {
    let bones = anim::skeleton(mdl).ok()?;
    let mut out = Vec::new();
    for m in &model.meshes {
        let positions: Vec<Vec3> = m.positions.iter().map(|&p| Vec3::from(p)).collect();
        let normals: Vec<Vec3> = m.normals.iter().map(|&n| Vec3::from(n)).collect();
        let mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, m.uvs.clone())
        .with_inserted_indices(Indices::U32(m.indices.clone()));
        let name = m
            .materials
            .iter()
            .find(|name| pf_world::read_first(material_dirs, &format!("{name}.vmt")).is_some());
        let vmt = name
            .and_then(|n| pf_world::read_first(material_dirs, &format!("{n}.vmt")))
            .map(|t| String::from_utf8_lossy(&t).into_owned())
            .unwrap_or_default();
        let shader = vmt
            .split(|c: char| c.is_whitespace() || c == '{')
            .find(|w| !w.is_empty() && !w.starts_with("//"))
            .unwrap_or_default()
            .trim_matches('"')
            .to_ascii_lowercase();
        let proxies = ProxyMaterial::parse(&vmt);
        let loaded = name.and_then(|n| pf_world::load_material(material_dirs, n));
        // Attachments like the HCOG's reticle are unlit, see-through or glowing,
        // and tinted by the material's `$color2`.
        let (additive, unlit, tint, blend) = loaded
            .as_ref()
            .map_or((false, false, None, AlphaMode::Opaque), |l| {
                (l.additive, l.unlit, l.tint, l.alpha)
            });
        let decal = loaded.as_ref().is_some_and(|l| l.decal);
        // Respawn's `Basic` shader (the ammo counter) draws unlit.
        let unlit = unlit || shader == "basic";
        let texture = loaded.and_then(|l| l.texture).map(|i| images.add(i));
        let handle = meshes.add(mesh);
        if shader == "unlittwotexture" {
            // The reticles: base × scrolling noise.
            // A missing texture would keep the material from ever drawing, so
            // fall back to plain white.
            let second = texture2(material_dirs, &vmt).map(|i| images.add(i));
            let texture = texture.or_else(|| Some(images.add(Image::default())));
            let second = second.or_else(|| Some(images.add(Image::default())));
            let material = reticles.add(crate::unlit_two::material(
                texture.unwrap_or_default(),
                second.unwrap_or_default(),
                additive,
            ));
            let mut entity = commands.spawn((
                Mesh3d(handle.clone()),
                MeshMaterial3d(material),
                RenderLayers::layer(LAYER),
                NoFrustumCulling,
                Transform::IDENTITY,
                ChildOf(parent),
            ));
            entity.insert(DrivenMaterial {
                proxies: proxies.unwrap_or_default(),
                tint: Vec3::from(tint.unwrap_or([1.0; 3])),
            });
            out.push(SkinnedMesh {
                handle,
                positions,
                normals,
                bones: m.bones.clone(),
                weights: m.weights.clone(),
            });
            continue;
        }
        let standard = StandardMaterial {
            base_color: match (tint, texture.is_some()) {
                (Some([r, g, b]), _) => Color::linear_rgb(r, g, b),
                (None, true) => Color::WHITE,
                (None, false) => Color::srgb(0.4, 0.4, 0.42),
            },
            base_color_texture: texture,
            perceptual_roughness: 0.6,
            cull_mode: None,
            unlit,
            alpha_mode: if additive { AlphaMode::Add } else { blend },
            // `$decal` meshes sit on the gun's surface (the ammo counter).
            depth_bias: if decal { DECAL_DEPTH_BIAS } else { 0.0 },
            ..default()
        };
        // `$selfillum` materials (the sight's glass) glow through their mask.
        let glow = crate::self_illum::settings(&vmt).and_then(|(mask, tint)| {
            let image = pf_world::vtf_to_image(&pf_world::read_first(
                material_dirs,
                &format!("{mask}.vtf"),
            )?)?;
            Some((images.add(image), tint))
        });
        let mut entity = commands.spawn((
            Mesh3d(handle.clone()),
            RenderLayers::layer(LAYER),
            // The mesh moves every frame; its spawn-time bounds would be wrong.
            NoFrustumCulling,
            Transform::IDENTITY,
            ChildOf(parent),
        ));
        match glow {
            Some((mask, [r, g, b])) => {
                entity.insert(MeshMaterial3d(glows.add(SelfIllumMaterial {
                    base: standard,
                    extension: SelfIllum {
                        mask,
                        tint: Vec4::new(r, g, b, 1.0),
                    },
                })));
            }
            None => {
                entity.insert(MeshMaterial3d(materials.add(standard)));
            }
        }
        if let Some(proxies) = proxies {
            entity.insert(DrivenMaterial {
                proxies,
                tint: Vec3::from(tint.unwrap_or([1.0; 3])),
            });
        }
        out.push(SkinnedMesh {
            handle,
            positions,
            normals,
            bones: m.bones.clone(),
            weights: m.weights.clone(),
        });
    }
    Some(Rig { bones, meshes: out })
}

/// How far `$decal` meshes are pulled towards the camera so they draw over
/// the surface they sit on. Ours.
const DECAL_DEPTH_BIAS: f32 = 1.0;

/// A mesh whose material its `.vmt` proxies drive every frame.
#[derive(Component)]
struct DrivenMaterial {
    proxies: ProxyMaterial,
    /// `$color2`.
    tint: Vec3,
}

/// The `$texture2` of an `UnlitTwoTexture` material. Read raw: material paths
/// use backslashes, which our KeyValues parser takes as escapes.
fn texture2(material_dirs: &[PathBuf], vmt: &str) -> Option<Image> {
    let line = vmt.lines().find(|l| {
        l.trim_start()
            .to_ascii_lowercase()
            .starts_with("\"$texture2\"")
    })?;
    let path = line
        .split('"')
        .nth(3)?
        .replace('\\', "/")
        .to_ascii_lowercase();
    let path = path.trim_end_matches(".vtf");
    pf_world::vtf_to_image(&pf_world::read_first(
        material_dirs,
        &format!("{path}.vtf"),
    )?)
}

/// A driven mesh and whichever material it has.
type DrivenQuery<'a> = (
    &'a mut DrivenMaterial,
    Option<&'a MeshMaterial3d<StandardMaterial>>,
    Option<&'a MeshMaterial3d<UnlitTwoTexture>>,
);

/// Run each driven material's proxies against the gun, and apply the results:
/// texture transforms, `$color` (× `$color2`) and `$alpha`.
fn drive_proxies(
    viewmodel: Option<Single<&Viewmodel>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut unlit_two: ResMut<Assets<UnlitTwoTexture>>,
    mut driven: Query<DrivenQuery>,
) {
    let Some(vm) = viewmodel else {
        return;
    };
    let cx = matproxy::Context {
        clip: vm.ammo,
        clip_size: vm.data.clip,
        reserve: vm.reserve,
        max_carry: vm.data.max_carry,
        holo: vm.holo,
    };
    for (mut d, std_handle, two_handle) in &mut driven {
        d.proxies.update(&cx);
        let ([r, g, b], alpha) = d.proxies.color_alpha();
        let color = Vec3::new(r, g, b) * d.tint;
        let base = d.proxies.transform("basetexturetransform");
        if let Some(h) = std_handle
            && let Some(mut m) = standard.get_mut(&h.0)
        {
            m.uv_transform = base;
            m.base_color = Color::linear_rgba(color.x, color.y, color.z, alpha);
        }
        if let Some(h) = two_handle
            && let Some(mut m) = unlit_two.get_mut(&h.0)
        {
            m.extension
                .set(color, alpha, base, d.proxies.transform("texture2transform"));
        }
    }
}

/// Spawn the viewmodel camera, its light, and the weapon and arms under `camera`.
pub fn spawn(
    commands: &mut Commands,
    camera: Entity,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    reticles: &mut Assets<UnlitTwoTexture>,
    glows: &mut Assets<SelfIllumMaterial>,
    images: &mut Assets<Image>,
) {
    let dir = crate::data::common_dir();
    let loadout = Loadout::preset(&dir, DEFAULT_PRESET).unwrap_or_default();
    let Some(data) = WeaponData::load(&dir, &loadout) else {
        info!(
            "no viewmodel: the loadout's primary weapon script isn't under {}",
            dir.display()
        );
        return;
    };
    let Some((mdl, model)) = read_model(&dir, &data.viewmodel, &data.bodygroups) else {
        info!("no viewmodel: {} not found", data.viewmodel);
        return;
    };
    let material_dirs = vec![dir.join("materials")];

    let view_camera = commands
        .spawn((
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

    let Some(weapon) = build_rig(
        commands,
        view_camera,
        &mdl,
        &model,
        &material_dirs,
        meshes,
        materials,
        reticles,
        glows,
        images,
    ) else {
        return;
    };
    let arms = read_model(&dir, ARMS, &[]).and_then(|(arms_mdl, arms_model)| {
        let rig = build_rig(
            commands,
            view_camera,
            &arms_mdl,
            &arms_model,
            &material_dirs,
            meshes,
            materials,
            reticles,
            glows,
            images,
        )?;
        let map = rig
            .bones
            .iter()
            .map(|b| weapon.bones.iter().position(|w| w.name == b.name))
            .collect();
        Some((rig, map))
    });

    let anims = anim::animations(&mdl).unwrap_or_default();
    let id = |name: &str| anims.iter().position(|a| a.name == name);
    // The attachment's alternate set (`..._alt1` etc.), where the model has it.
    let alt = |name: &str| {
        let alt = format!("{name}_alt{}", data.anim_alt);
        (data.anim_alt > 0)
            .then(|| id(&alt))
            .flatten()
            .or_else(|| id(name))
    };
    let ids = AnimIds {
        idle: alt("idle_anim"),
        idle_ads: alt("idle_ads_anim"),
        attack: alt("attack_anim"),
        attack_ads: alt("attack_ads_anim"),
        sprint: id("@sprint_seq"),
        reload: id("@reload_seq"),
        reload_empty: id("@reload_empty_seq"),
        draw: id("@draw_seq"),
    };
    // The aim-out animation ends at the hip pose, aim-in at the aim pose.
    let end_pose = |name: &str| {
        alt(name)
            .and_then(|i| anim::sample(&mdl, &weapon.bones, &anims[i], 1e6).ok())
            .unwrap_or_else(|| anim::rest_pose(&weapon.bones))
    };
    let hip = end_pose("@ads_out_seq");
    let aim = end_pose("@ads_in_seq");
    let camera_bone = weapon
        .bones
        .iter()
        .position(|b| b.name == "jx_c_camera")
        .unwrap_or(0);
    let action = ids.draw.map(|anim| GunAction {
        anim,
        time: 0.0,
        rate: anims[anim].duration() / data.deploy_time.max(0.05),
        reload: false,
    });
    info!(
        "viewmodel: {} ({} bones, {} animations){}",
        data.viewmodel,
        weapon.bones.len(),
        anims.len(),
        if arms.is_some() { " + arms" } else { "" }
    );
    let attachments = pf_fmt_mdl::attachments(&mdl).unwrap_or_default();
    let pivot = |name: &str| {
        attachments
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(name))
            .cloned()
    };
    let pivots = [
        pivot(&data.sway.hip.rotate_attach),
        pivot(&data.sway.zoomed.rotate_attach),
    ];
    let ammo = data.clip;
    // `ammo_default_total` counts the loaded magazine too (an R1Delta screenshot:
    // 24 in the clip with 9 spare magazines of the R-201's 300).
    let reserve = data.ammo_total.saturating_sub(data.clip);
    commands.entity(view_camera).insert(Viewmodel {
        mdl,
        weapon,
        arms,
        anims,
        data,
        camera_bone,
        hip,
        aim,
        ids,
        clock: 0.0,
        ads: 0.0,
        sprint: 0.0,
        since_shot: 10.0,
        cooldown: 0.0,
        ammo,
        reserve,
        spread: Spread::default(),
        spread_pending: 0.0,
        action,
        sway: Sway::default(),
        recoil: Recoil::default(),
        pivots,
        pivot_blend: 0.0,
        holo: 0.0,
    });
}

/// Move `value` towards `target`, taking `seconds` for a full 0→1 change.
fn approach(value: f32, target: f32, seconds: f32, dt: f32) -> f32 {
    let step = dt / seconds.max(1e-3);
    if value < target {
        (value + step).min(target)
    } else {
        (value - step).max(target)
    }
}

fn blend(a: &Pose, b: &Pose, t: f32) -> Pose {
    a.iter()
        .zip(b)
        .map(|(&(pa, qa), &(pb, qb))| (pa.lerp(pb, t), qa.slerp(qb, t)))
        .collect()
}

pub fn animate(
    time: Res<Time>,
    settings: Res<Settings>,
    tuning: Res<Tuning>,
    tunings: Res<Assets<TuningAsset>>,
    mut meshes: ResMut<Assets<Mesh>>,
    viewmodel: Single<(&mut Viewmodel, &mut Transform)>,
    player: Single<(&mut Player, &ActionState<Action>, &mut Projection), Without<Viewmodel>>,
) {
    let dt = time.delta_secs();
    let (mut vm, mut transform) = viewmodel.into_inner();
    let vm = vm.as_mut();
    let (mut player, actions, mut projection) = player.into_inner();
    // The kick spring and the view/gun split follow the zoom (recoil).
    let (fraction, vm_scale) = vm.data.recoil.split_at(vm.ads);
    let spring = vm.data.recoil.spring_at(vm.ads);
    player.view_effects().set_weapon(spring, fraction, vm_scale);
    // The gun's own punch: turning its camera the other way moves the gun on
    // screen by the punch.
    let kick = player.weapon_kick();
    transform.rotation = Quat::from_euler(EulerRot::YXZ, kick.y, kick.x, kick.z).inverse();
    let state = *player.movement();
    vm.clock += dt;
    vm.since_shot += dt;
    vm.cooldown -= dt;

    // A reload or draw in progress: finish it when its animation ends.
    if let Some(a) = vm.action.as_mut() {
        a.time += dt;
        let length = vm.anims[a.anim].duration() / a.rate.max(1e-3);
        if a.time >= length {
            if a.reload {
                let take = (vm.data.clip - vm.ammo).min(vm.reserve);
                vm.ammo += take;
                vm.reserve -= take;
            }
            vm.action = None;
        }
    }
    let speed = Vec2::new(state.velocity.x, state.velocity.z).length();
    let sprinting = state.on_ground && speed > SPRINT_SPEED;
    let wants_reload = actions.just_pressed(&Action::Reload) && vm.ammo < vm.data.clip;
    if vm.action.is_none() && vm.reserve > 0 && (wants_reload || vm.ammo == 0) {
        let (anim, length) = if vm.ammo == 0 {
            (vm.ids.reload_empty, vm.data.reload_empty_time)
        } else {
            (vm.ids.reload, vm.data.reload_time)
        };
        if let Some(anim) = anim {
            vm.action = Some(GunAction {
                anim,
                time: 0.0,
                rate: vm.anims[anim].duration() / length.max(0.05),
                reload: true,
            });
        }
    }
    let busy = vm.action.is_some();
    // What the weapon's spread hears about the player each tick, as in the game.
    let wall = matches!(
        state.mode,
        pf_movement::Mode::WallRun { .. } | pf_movement::Mode::WallHang { .. }
    );
    let spread_input = SpreadInput {
        speed,
        run_speed: tunings.get(&tuning.0).map_or(180.6, |t| t.0.walk_speed),
        sprinting,
        crouch: if state.crouched { 1.0 } else { 0.0 },
        ads: vm.ads,
        grounded: state.on_ground || wall,
        on_ground: state.on_ground,
    };
    // Full auto: a shot every 1 / fire_rate seconds while the trigger is held.
    // Dev: `PF_DEV_FIRE=1` holds the trigger (to see recoil in screenshots).
    let trigger = actions.pressed(&Action::Fire) || std::env::var_os("PF_DEV_FIRE").is_some();
    if trigger && !busy && vm.sprint < 0.5 && vm.cooldown <= 0.0 && vm.ammo > 0 {
        vm.ammo -= 1;
        vm.cooldown = 1.0 / vm.data.fire_rate.max(0.1);
        vm.since_shot = 0.0;
        vm.spread.fire(&vm.data.spread, &spread_input, vm.clock);
        let duck = if state.crouched { 1.0 } else { 0.0 };
        let kick = vm.recoil.fire(&vm.data.recoil, vm.clock, vm.ads, duck);
        player.view_effects().kick(kick.soft, kick.hard);
        player.add_aim(kick.perm.x, kick.perm.y);
    }
    if !trigger {
        vm.recoil.release();
    }
    vm.spread_pending = (vm.spread_pending + dt).min(0.25);
    while vm.spread_pending >= SPREAD_TICK {
        vm.spread_pending -= SPREAD_TICK;
        vm.spread.tick(&vm.data.spread, &spread_input);
        vm.spread.decay(&vm.data.spread, vm.clock, SPREAD_TICK);
    }

    // Aim and sprint weights.
    // Dev: `PF_DEV_ADS=1` holds aim (to see aimed effects in screenshots).
    let aim_held = actions.pressed(&Action::Zoom) || std::env::var_os("PF_DEV_ADS").is_some();
    let aiming = aim_held && !busy && !sprinting;
    // The holo-sight value (`0x18009e320`): up while zoomed, down otherwise,
    // over the zoom times. (The game uses the aim-in/out animations' rates
    // when the weapon has them.)
    vm.holo = if aiming {
        vm.holo + dt / vm.data.zoom_time_in.max(1e-3)
    } else {
        vm.holo - dt / vm.data.zoom_time_out.max(1e-3)
    }
    .clamp(0.0, 1.0);
    vm.ads = if aiming {
        approach(vm.ads, 1.0, vm.data.zoom_time_in, dt)
    } else {
        approach(vm.ads, 0.0, vm.data.zoom_time_out, dt)
    };
    let shooting = vm.since_shot < 0.2;
    vm.sprint = if sprinting && !shooting && !busy {
        approach(vm.sprint, 1.0, vm.data.lower_time, dt)
    } else {
        approach(vm.sprint, 0.0, vm.data.raise_time, dt)
    };
    // Smoothstep so the motion eases in and out.
    let ease = |t: f32| t * t * (3.0 - 2.0 * t);
    let (ads, sprint) = (ease(vm.ads), ease(vm.sprint));

    // Zoom the world camera with the aim.
    if let Projection::Perspective(p) = projection.as_mut() {
        let fov = settings.fov + (vm.data.zoom_fov - settings.fov) * ads;
        p.fov = view::vertical_fov(fov);
    }

    // Build the pose: base (hip/aim), sprint, then the idle and fire deltas, then
    // any reload/draw on top.
    let (mdl, bones, anims) = (&vm.mdl, &vm.weapon.bones, &vm.anims);
    let sample =
        |i: Option<usize>, t: f32| i.and_then(|i| anim::sample(mdl, bones, &anims[i], t).ok());
    let mut pose = blend(&vm.hip, &vm.aim, ads);
    if sprint > 0.0
        && let Some(s) = sample(vm.ids.sprint, vm.clock)
    {
        pose = blend(&pose, &s, sprint);
    }
    if let Some(d) = sample(vm.ids.idle, vm.clock) {
        anim::add_delta(&mut pose, &d, 1.0 - ads);
    }
    if let Some(d) = sample(vm.ids.idle_ads, vm.clock) {
        anim::add_delta(&mut pose, &d, ads);
    }
    if shooting {
        if let Some(d) = sample(vm.ids.attack, vm.since_shot) {
            anim::add_delta(&mut pose, &d, 1.0 - ads);
        }
        if let Some(d) = sample(vm.ids.attack_ads, vm.since_shot) {
            anim::add_delta(&mut pose, &d, ads);
        }
    }
    if let Some(a) = &vm.action
        && let Some(p) = sample(Some(a.anim), a.time * a.rate)
    {
        let length = anims[a.anim].duration() / a.rate.max(1e-3);
        let weight = (a.time / ACTION_FADE)
            .min((length - a.time) / ACTION_FADE)
            .clamp(0.0, 1.0);
        pose = blend(&pose, &p, weight);
    }

    let world = anim::world_transforms(bones, &pose);

    // Sway and bob, as the game runs them for the viewmodel (`0x18009f740`):
    // the update, then the pivot moving between the hip and aimed attachments
    // over their blend times, then the sway applied about it.
    let (yaw, pitch) = player.view_angles();
    let (velocity, view) = sway::view_input(state.velocity, yaw, pitch);
    let zoomed = aiming;
    vm.sway.update(
        &vm.data.sway,
        &sway::Input {
            dt,
            velocity,
            view,
            zoomed,
            zoom_fraction: vm.ads,
            on_ground: state.on_ground,
        },
    );
    let target = if zoomed && vm.data.sway.zoomed_enabled {
        1.0
    } else {
        0.0
    };
    let blend_time = if target > 0.0 {
        vm.data.sway.zoomed.rotate_attach_blend_time
    } else {
        vm.data.sway.hip.rotate_attach_blend_time
    };
    vm.pivot_blend = if blend_time <= 0.0 {
        target
    } else {
        approach(vm.pivot_blend, target, blend_time, dt)
    };
    let attachment = |a: &Option<pf_fmt_mdl::Attachment>| {
        a.as_ref().map_or(Affine3A::IDENTITY, |a| {
            let [r0, r1, r2] = a.matrix;
            let local = Affine3A::from_cols_array(&[
                r0[0], r1[0], r2[0], r0[1], r1[1], r2[1], r0[2], r1[2], r2[2], r0[3], r1[3], r2[3],
            ]);
            world.get(a.bone).copied().unwrap_or(Affine3A::IDENTITY) * local
        })
    };
    let (hip_pivot, aim_pivot) = (attachment(&vm.pivots[0]), attachment(&vm.pivots[1]));
    let pivot = {
        let (_, hip_rot, hip_pos) = hip_pivot.to_scale_rotation_translation();
        let (_, aim_rot, aim_pos) = aim_pivot.to_scale_rotation_translation();
        let t = vm.pivot_blend;
        Affine3A::from_rotation_translation(hip_rot.slerp(aim_rot, t), hip_pos.lerp(aim_pos, t))
    };
    let swayed = sway::transform(&vm.sway, pivot);
    let to_camera = world[vm.camera_bone].inverse() * swayed;
    skin(
        &mut meshes,
        &vm.weapon,
        &anim::skinning(bones, &world),
        to_camera,
    );
    if let Some((arms, map)) = &vm.arms {
        // Mapped bones take the weapon's transform; the rest hang off their parent.
        let mut arms_world: Vec<Affine3A> = Vec::with_capacity(arms.bones.len());
        for (i, b) in arms.bones.iter().enumerate() {
            let w = match map[i] {
                Some(j) => world[j],
                None => {
                    let local = Affine3A::from_rotation_translation(b.quat, b.pos);
                    match b.parent {
                        Some(p) if p < i => arms_world[p] * local,
                        _ => local,
                    }
                }
            };
            arms_world.push(w);
        }
        skin(
            &mut meshes,
            arms,
            &anim::skinning(&arms.bones, &arms_world),
            to_camera,
        );
    }
}

/// Write skinned positions and normals into each mesh, in viewmodel camera space.
fn skin(meshes: &mut Assets<Mesh>, rig: &Rig, matrices: &[Affine3A], to_camera: Affine3A) {
    for m in &rig.meshes {
        let Some(mut mesh) = meshes.get_mut(&m.handle) else {
            continue;
        };
        let mut positions = Vec::with_capacity(m.positions.len());
        let mut normals = Vec::with_capacity(m.positions.len());
        for (i, (&p, &n)) in m.positions.iter().zip(&m.normals).enumerate() {
            let (mut pos, mut nor) = (Vec3::ZERO, Vec3::ZERO);
            for k in 0..3 {
                let w = m.weights[i][k];
                if w > 0.0
                    && let Some(mat) = matrices.get(m.bones[i][k] as usize)
                {
                    pos += mat.transform_point3(p) * w;
                    nor += mat.transform_vector3(n) * w;
                }
            }
            positions.push(to_view(to_camera.transform_point3(pos)).to_array());
            normals.push(
                to_view(to_camera.transform_vector3(nor))
                    .normalize_or_zero()
                    .to_array(),
            );
        }
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            VertexAttributeValues::Float32x3(positions),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_NORMAL,
            VertexAttributeValues::Float32x3(normals),
        );
    }
}
