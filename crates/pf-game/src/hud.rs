//! The pilot HUD, built from the game's own files: the layout of the pilot's
//! helmet screen (`scripts/screens/vgui_fullscreen_pilot.res` and its base), the
//! HUD fonts (`.vfont`) and font sizes (`resource/hudscriptedscheme.res`), the
//! textures under `materials/vgui/HUD`, text from the English localisation, key
//! names from the default key binds, and the weapon's crosshair settings.
//! Which elements show and what goes in them follows the HUD script
//! (`cl_main_hud.nut`) for the pilot's loadout ([`crate::loadout`]).
//!
//! The helmet screen is drawn flat into images and shown curved, the way the
//! game warps it (`hudwarp`); textures the game marks `$additive` add light.
//! Not yet like the game (docs/hud.md): the font glow, the boxes drawn around
//! key names, and most of the screen's elements. Without the game files there
//! is simply no HUD.

use std::collections::HashMap;
use std::path::Path;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use pf_fmt_ui::{kv, layout, vfont};

use crate::Screen;
use crate::hudwarp::{self, PIXELS_PER_UNIT, SCREEN};
use crate::level::LevelEntity;
use crate::loadout::{DEFAULT_PRESET, Loadout, WeaponScript};
use crate::viewmodel::Viewmodel;
/// Screen HUD elements (the crosshair) are sized for a screen this tall.
const CROSSHAIR_SPACE_TALL: f32 = 480.0;
/// The R-201's crosshair (`CrosshairData` in its script): 44 × 44, turned 90°,
/// drawn from hip only, faded while sprinting/reloading. It's a "spread" element:
/// the texture is one arm, drawn once per step of `spread_delta_angle` around the
/// centre (the game's default is 90°, so 4 arms) and pushed out by the spread.
const CROSSHAIR_SIZE: f32 = 44.0;
const CROSSHAIR_ROTATION_DEG: f32 = 90.0;
const CROSSHAIR_ARMS: usize = 4;
const CROSSHAIR_ARM_STEP_DEG: f32 = 90.0;
const CROSSHAIR_FADE_INTO_ZOOM_END: f32 = 0.5;
/// How fast the crosshair fades in and out, per second. Our guess.
const CROSSHAIR_FADE_RATE: f32 = 8.0;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::InGame), spawn).add_systems(
            Update,
            (update, place).chain().run_if(in_state(Screen::InGame)),
        );
    }
}

/// An element's rectangle on the helmet screen.
#[derive(Component)]
struct HudBox(layout::Rect);

/// What an element shows that changes while playing.
#[derive(Component, Clone, Copy, PartialEq)]
enum Live {
    AmmoCount,
    MagCount,
    WeaponName,
    /// The bar's filled part; its full width is the box's.
    AmmoBarFill,
}

/// A label's text format from the localisation (`%s1` is the value), as the
/// game's HUD "auto text" uses.
#[derive(Component)]
struct AutoText(String);

/// What the HUD script puts in an element.
enum Fill {
    /// As the layout file has it.
    Layout,
    /// This image (`SetImage`).
    Image(String),
    /// This text.
    Text(String),
    /// A changing value, with its auto-text format token if it has one.
    Live(Live, Option<&'static str>),
}

/// The elements the HUD script shows for this loadout, and what goes in them.
/// - The weapon panel (`MainHud_InitWeapons`).
/// - The two offhand slots (`InitOffhandWeaponHud`): slot 0 is the ordnance
///   (`OFFHAND_RIGHT`, key `+offhand1`), slot 1 the special ability
///   (`OFFHAND_LEFT`, `+offhand2`). Each shows its weapon's `hud_icon` and key,
///   and either a count or a ready bar (see [`shows_bar`]).
/// - The anti-Titan weapon in the d-pad's left slot (`MainHud_UpdateWeapons`):
///   its `hud_icon` and key. Its title and description are drawn at alpha 0.
fn elements(dir: &Path, loadout: &Loadout) -> Vec<(String, Fill)> {
    let mut out: Vec<(String, Fill)> = [
        ("dpadIconBG", Fill::Layout),
        ("dpadIcon", Fill::Layout),
        ("dpadSideBG", Fill::Layout),
        ("weaponAmmoBar", Fill::Live(Live::AmmoBarFill, None)),
        ("weaponLabel", Fill::Live(Live::WeaponName, None)),
        (
            "weaponAmmoCount",
            Fill::Live(Live::AmmoCount, Some("#HudAutoText_ActiveWeaponClipAmmo")),
        ),
        (
            "weaponMagLabel",
            Fill::Live(
                Live::MagCount,
                Some("#HudAutoText_ActiveWeaponMagazineCount"),
            ),
        ),
        ("weaponAmmoMag", Fill::Layout),
    ]
    .into_iter()
    .map(|(n, f)| (n.to_string(), f))
    .collect();
    let passives: Vec<&str> = loadout.passives.iter().map(String::as_str).collect();
    for (slot, name) in [(0, &loadout.ordnance), (1, &loadout.special)] {
        let Some(weapon) = WeaponScript::load(dir, name, &passives) else {
            continue;
        };
        let element = |part: &str| format!("Offhand{slot}{part}");
        out.push((element("BG"), Fill::Layout));
        if let Some(icon) = weapon.text("hud_icon") {
            out.push((element("Icon"), Fill::Image(icon.to_string())));
        }
        out.push((element("Hint"), Fill::Layout));
        if shows_bar(&weapon) {
            // Full while ready; abilities can't be used yet, so always ready.
            out.push((element("Bar"), Fill::Layout));
        } else {
            let format = localise(dir, &format!("#HudAutoText_PilotOffhandWeaponAmmo{slot}"));
            let count = weapon.num("ammo_clip_size").unwrap_or(0.0) as u32;
            let text = format.replace("%s1", &count.to_string());
            out.push((element("Count"), Fill::Text(text)));
        }
    }
    if let Some(icon) = WeaponScript::load(dir, &loadout.secondary, &passives)
        .and_then(|w| w.text("hud_icon").map(str::to_string))
    {
        out.push(("dpadLeftIcon".into(), Fill::Image(icon)));
        out.push(("dpadLeftIconHint".into(), Fill::Layout));
    }
    out
}

/// Whether an offhand slot shows a progress bar instead of a count
/// (`GetOffhandProgressSource`): `offhand` and `offhand_hybrid` weapons only
/// when they have a `charge_time`; every other fire mode always.
fn shows_bar(weapon: &WeaponScript) -> bool {
    match weapon.text("fire_mode").unwrap_or_default() {
        "offhand" | "offhand_hybrid" => weapon.num("charge_time").is_some_and(|t| t != 0.0),
        _ => true,
    }
}

/// The key bound to each command in the game's default PC key binds
/// (`cfg/config_default_pc.cfg`), e.g. `+offhand1` -> `F`. The first key wins
/// when a command has more than one.
fn default_binds(dir: &Path) -> HashMap<String, String> {
    let text = std::fs::read_to_string(dir.join("cfg/config_default_pc.cfg")).unwrap_or_default();
    parse_binds(&text)
}

fn parse_binds(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let line = line.split("//").next().unwrap_or_default();
        let quoted: Vec<&str> = line.split('"').skip(1).step_by(2).collect();
        if line.trim_start().starts_with("bind")
            && let [key, command] = quoted[..]
        {
            out.entry(command.to_ascii_lowercase())
                .or_insert_with(|| key.to_ascii_uppercase());
        }
    }
    out
}

/// A label's `%command%` key-binding tokens replaced by their keys (`+name`
/// first, then `name`), the way labels with key-binding icons show them.
fn bind_text(text: &str, binds: &HashMap<String, String>) -> String {
    let mut parts = text.split('%');
    let mut out = parts.next().unwrap_or_default().to_string();
    let rest: Vec<&str> = parts.collect();
    for pair in rest.chunks(2) {
        let name = pair[0].to_ascii_lowercase();
        match binds.get(&format!("+{name}")).or_else(|| binds.get(&name)) {
            Some(key) if pair.len() == 2 => out.push_str(key),
            _ => {
                out.push('%');
                out.push_str(pair[0]);
                if pair.len() == 2 {
                    out.push('%');
                }
            }
        }
        if let Some(after) = pair.get(1) {
            out.push_str(after);
        }
    }
    out
}

/// Where a VGUI label puts its text in its box (`textAlignment`; labels
/// default to `west`): (across, down).
fn text_alignment(name: Option<&str>) -> (JustifyContent, AlignItems) {
    use {AlignItems as V, JustifyContent as H};
    match name.unwrap_or("west").to_ascii_lowercase().as_str() {
        "north-west" | "northwest" => (H::FlexStart, V::FlexStart),
        "north" => (H::Center, V::FlexStart),
        "north-east" | "northeast" => (H::FlexEnd, V::FlexStart),
        "center" => (H::Center, V::Center),
        "east" => (H::FlexEnd, V::Center),
        "south-west" | "southwest" => (H::FlexStart, V::FlexEnd),
        "south" => (H::Center, V::FlexEnd),
        "south-east" | "southeast" => (H::FlexEnd, V::FlexEnd),
        _ => (H::FlexStart, V::Center),
    }
}

#[derive(Component)]
struct Crosshair {
    alpha: f32,
    /// Which arm: its push direction is `arm × CROSSHAIR_ARM_STEP_DEG`.
    arm: usize,
}

/// Read a `.res` file and lay its `#base` files underneath it.
fn load_res(path: &Path) -> Option<kv::Node> {
    let text = std::fs::read_to_string(path).ok()?;
    let file = kv::parse(&text);
    let mut root = file.nodes.into_iter().next()?;
    for base in file.bases.iter().rev() {
        if let Some(mut merged) = load_res(&path.with_file_name(base)) {
            merged.merge(&root);
            root = merged;
        }
    }
    Some(root)
}

/// The English text for a `#TOKEN`, from `resource/r1_english.txt` (UTF-16).
fn localise(dir: &Path, token: &str) -> String {
    let key = token.trim_start_matches('#');
    let bytes = std::fs::read(dir.join("resource/r1_english.txt")).unwrap_or_default();
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&c| u16::from_le_bytes(c))
        .collect();
    let text = String::from_utf16_lossy(&units);
    kv::parse(&text)
        .nodes
        .first()
        .and_then(|lang| lang.get("Tokens"))
        .and_then(|t| t.text(key))
        .map(str::to_string)
        .unwrap_or_else(|| key.to_string())
}

/// A scheme font: its file and its size (`tall`) in helmet-screen units.
struct SchemeFont {
    handle: Handle<Font>,
    tall: f32,
}

/// The HUD fonts named in `hudscriptedscheme.res`, loaded from their `.vfont`s.
fn load_fonts(dir: &Path, fonts: &mut Assets<Font>) -> HashMap<String, SchemeFont> {
    let mut by_file: HashMap<String, Handle<Font>> = HashMap::new();
    let mut out = HashMap::new();
    let Some(scheme) = std::fs::read_to_string(dir.join("resource/hudscriptedscheme.res"))
        .ok()
        .and_then(|t| kv::parse(&t).nodes.into_iter().next())
    else {
        return out;
    };
    let Some(defs) = scheme.get("Fonts") else {
        return out;
    };
    for def in defs.children() {
        let Some(first) = def.children().first() else {
            continue;
        };
        let (Some(name), Some(tall)) = (first.text("name"), first.text("tall")) else {
            continue;
        };
        let file = name.to_ascii_lowercase();
        let handle = match by_file.get(&file) {
            Some(h) => h.clone(),
            None => {
                let Some(font) = std::fs::read(dir.join(format!("resource/{file}.vfont")))
                    .ok()
                    .and_then(|b| vfont::decode(&b))
                    .map(Font::from_bytes)
                else {
                    continue;
                };
                let h = fonts.add(font);
                by_file.insert(file, h.clone());
                h
            }
        };
        out.insert(
            def.key.to_ascii_lowercase(),
            SchemeFont {
                handle,
                tall: tall.trim().parse().unwrap_or(12.0),
            },
        );
    }
    out
}

/// `"r g b a"` (0-255) to a colour.
fn color(v: Option<&str>) -> Color {
    let c: Vec<f32> = v
        .unwrap_or("255 255 255 255")
        .split_whitespace()
        .filter_map(|n| n.parse().ok())
        .collect();
    let at = |i: usize| c.get(i).copied().unwrap_or(255.0) / 255.0;
    Color::srgba(at(0), at(1), at(2), at(3))
}

/// A HUD texture, and whether its material is `$additive`.
fn hud_image(dir: &Path, images: &mut Assets<Image>, name: &str) -> Option<(Handle<Image>, bool)> {
    let material = format!("vgui/{}", name.replace('\\', "/").to_ascii_lowercase());
    let loaded = pf_world::load_material(&[dir.join("materials")], &material)?;
    let additive = loaded.additive;
    loaded.texture.map(|i| (images.add(i), additive))
}

/// A node at `rect` (helmet-screen units) in the flat HUD image.
fn hud_node(rect: &layout::Rect) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(rect.x * PIXELS_PER_UNIT),
        top: Val::Px(rect.y * PIXELS_PER_UNIT),
        width: Val::Px(rect.w * PIXELS_PER_UNIT),
        height: Val::Px(rect.h * PIXELS_PER_UNIT),
        ..default()
    }
}

fn spawn(
    mut commands: Commands,
    mut fonts: ResMut<Assets<Font>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    weapon: Option<Single<&Viewmodel>>,
) {
    let dir = crate::data::common_dir();
    let Some(root) = load_res(&dir.join("scripts/screens/vgui_fullscreen_pilot.res")) else {
        info!("no HUD: the pilot HUD layout isn't under {}", dir.display());
        return;
    };
    let fonts = load_fonts(&dir, &mut fonts);
    let binds = default_binds(&dir);
    let loadout = Loadout::preset(&dir, DEFAULT_PRESET).unwrap_or_default();
    let weapon_name = weapon
        .as_ref()
        .map(|w| localise(&dir, w.short_name()))
        .unwrap_or_default();
    // Text boxes that size to their text get a rough width from the font size;
    // only the ammo counts do, and they're right-aligned against their pin.
    let rects = layout::layout(&root, SCREEN, |node| {
        let tall = node
            .text("font")
            .and_then(|f| fonts.get(&f.to_ascii_lowercase()))
            .map_or(12.0, |f| f.tall);
        Some(tall * 1.2)
    });

    let layers = hudwarp::spawn(
        &mut commands,
        &mut images,
        &mut meshes,
        &mut materials,
        &hudwarp::PILOT,
    );
    // The window's own HUD (not on the helmet screen): the crosshair.
    let hud = commands
        .spawn((
            LevelEntity,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .id();
    let layer = |additive: bool| {
        if additive {
            layers.additive
        } else {
            layers.normal
        }
    };

    for (name, fill) in elements(&dir, &loadout) {
        let (Some(node), Some(rect)) = (root.get(&name), rects.get(&name.to_ascii_lowercase()))
        else {
            continue;
        };
        let z = node
            .text("zpos")
            .and_then(|z| z.trim().parse::<i32>().ok())
            .unwrap_or(0);
        let base = (HudBox(*rect), hud_node(rect), ZIndex(z));
        match node.text("ControlName").unwrap_or_default() {
            "ImagePanel" => {
                let image = match &fill {
                    Fill::Image(i) => Some(i.as_str()),
                    _ => node.text("image"),
                };
                if let Some((image, additive)) = image.and_then(|i| hud_image(&dir, &mut images, i))
                {
                    commands.spawn((
                        base,
                        ImageNode::new(image).with_color(color(node.text("drawColor"))),
                        ChildOf(layer(additive)),
                    ));
                }
            }
            "Label" => {
                let font = node
                    .text("font")
                    .and_then(|f| fonts.get(&f.to_ascii_lowercase()));
                let (across, down) = text_alignment(node.text("textAlignment"));
                let tall = font.map_or(12.0, |f| f.tall);
                // The box, with the text placed in it by its alignment.
                let (base_box, base_node, base_z) = base;
                let holder = commands
                    .spawn((
                        base_box,
                        Node {
                            display: Display::Flex,
                            justify_content: across,
                            align_items: down,
                            overflow: Overflow::visible(),
                            ..base_node
                        },
                        base_z,
                        ChildOf(layers.normal),
                    ))
                    .id();
                let start = match &fill {
                    Fill::Live(Live::WeaponName, _) => weapon_name.clone(),
                    Fill::Live(..) => String::new(),
                    Fill::Text(t) => t.clone(),
                    _ => bind_text(node.text("labelText").unwrap_or_default(), &binds),
                };
                let right = across == JustifyContent::FlexEnd;
                let mut text = commands.spawn((
                    Text::new(start),
                    TextColor(color(node.text("fgcolor_override"))),
                    TextLayout::justify(if right { Justify::Right } else { Justify::Left })
                        .with_no_wrap(),
                    ChildOf(holder),
                ));
                if let Fill::Live(live, format) = fill {
                    text.insert(live);
                    if let Some(token) = format {
                        text.insert(AutoText(localise(&dir, token)));
                    }
                }
                if let Some(f) = font {
                    text.insert(TextFont {
                        font: FontSource::Handle(f.handle.clone()),
                        font_size: FontSize::Px(tall * PIXELS_PER_UNIT),
                        ..default()
                    });
                }
            }
            "CHudProgressBar" => {
                let bg = node
                    .text("bg_image")
                    .and_then(|i| hud_image(&dir, &mut images, i));
                let fg = node
                    .text("fg_image")
                    .and_then(|i| hud_image(&dir, &mut images, i));
                // Background and fill may blend differently, so each goes on
                // its own layer; the fill's width follows the clip.
                if let Some((bg, additive)) = bg {
                    commands.spawn((
                        base,
                        ImageNode::new(bg).with_color(color(node.text("bgcolor_override"))),
                        ChildOf(layer(additive)),
                    ));
                }
                if let Some((fg, additive)) = fg {
                    let mut fill_part = commands.spawn((
                        HudBox(*rect),
                        hud_node(rect),
                        ZIndex(z + 1),
                        ImageNode::new(fg).with_color(color(node.text("fgcolor_override"))),
                        ChildOf(layer(additive)),
                    ));
                    if let Fill::Live(live, _) = fill {
                        fill_part.insert(live);
                    }
                }
            }
            _ => {}
        }
    }

    // The crosshair: shadow arms under the line arms.
    for (layer, name) in ["standard_plus_shadow", "standard_plus"].iter().enumerate() {
        let Some((image, _)) = hud_image(&dir, &mut images, &format!("hud/crosshairs/{name}"))
        else {
            continue;
        };
        for arm in 0..CROSSHAIR_ARMS {
            let angle = CROSSHAIR_ROTATION_DEG + arm as f32 * CROSSHAIR_ARM_STEP_DEG;
            commands.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                ImageNode::new(image.clone()),
                // The game's UI angles turn the other way round from Bevy's.
                UiTransform::from_rotation(Rot2::degrees(-angle)),
                ZIndex(300 + layer as i32),
                Crosshair { alpha: 1.0, arm },
                ChildOf(hud),
            ));
        }
    }
}

/// Fill in what changes: ammo, magazines, the ammo bar, the crosshair's fade.
#[allow(clippy::type_complexity)]
fn update(
    time: Res<Time>,
    weapon: Option<Single<&Viewmodel>>,
    mut texts: Query<(&Live, &mut Text, Option<&AutoText>)>,
    mut bars: Query<(&Live, &HudBox, &mut Node), Without<Text>>,
    mut crosshairs: Query<(&mut Crosshair, &mut ImageNode)>,
) {
    let Some(w) = weapon else {
        return;
    };
    let auto = |format: Option<&AutoText>, value: u32| {
        format.map_or(value.to_string(), |f| {
            f.0.replace("%s1", &value.to_string())
        })
    };
    for (live, mut text, format) in &mut texts {
        match live {
            Live::AmmoCount => text.0 = auto(format, w.ammo()),
            // Spare magazines, counting a part-full one.
            Live::MagCount => text.0 = auto(format, w.reserve().div_ceil(w.clip_size().max(1))),
            // The gun may arrive after the HUD; look its name up once.
            Live::WeaponName if text.0.is_empty() => {
                text.0 = localise(&crate::data::common_dir(), w.short_name())
            }
            _ => {}
        }
    }
    for (live, HudBox(r), mut node) in &mut bars {
        if *live == Live::AmmoBarFill {
            let fraction = w.ammo() as f32 / w.clip_size().max(1) as f32;
            node.width = Val::Px(r.w * fraction * PIXELS_PER_UNIT);
        }
    }
    // Gone while sprinting or reloading; fades out as aiming gets halfway in.
    let ads_fade = (w.ads() / CROSSHAIR_FADE_INTO_ZOOM_END).clamp(0.0, 1.0);
    let target = if w.reloading() || w.sprint() > 0.5 {
        0.0
    } else {
        1.0 - ads_fade
    };
    let step = CROSSHAIR_FADE_RATE * time.delta_secs();
    for (mut c, mut image) in &mut crosshairs {
        c.alpha += (target - c.alpha).clamp(-step, step);
        image.color = Color::srgba(1.0, 1.0, 1.0, c.alpha);
    }
}

/// Place the crosshair's arms around the middle of the window, each pushed out
/// by the weapon's spread as the game projects it: tan(spread / 2) /
/// tan(fov / 2) × half the screen height, with the fov in the game's (4:3
/// horizontal) terms.
fn place(
    window: Single<&Window, With<PrimaryWindow>>,
    weapon: Option<Single<&Viewmodel>>,
    camera: Option<Single<&Projection, With<crate::player::Player>>>,
    mut crosshairs: Query<(&Crosshair, &mut Node)>,
) {
    let (ww, wh) = (window.width(), window.height());
    let size = CROSSHAIR_SIZE * wh / CROSSHAIR_SPACE_TALL;
    let spread = weapon.map_or(0.0, |w| w.spread());
    // Our camera's fov is vertical; the game's is horizontal on a 4:3 screen.
    let tan_half_fov = match camera.as_deref() {
        Some(Projection::Perspective(p)) => (p.fov * 0.5).tan() / 0.75,
        _ => 1.0,
    };
    let push = (spread.to_radians() * 0.5).tan() / tan_half_fov * wh * 0.5;
    for (c, mut node) in &mut crosshairs {
        let angle = (c.arm as f32 * CROSSHAIR_ARM_STEP_DEG).to_radians();
        let (x, y) = (angle.cos() * push, -angle.sin() * push);
        node.left = Val::Px((ww - size) / 2.0 + x);
        node.top = Val::Px((wh - size) / 2.0 + y);
        node.width = Val::Px(size);
        node.height = Val::Px(size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_binds_fill_label_tokens() {
        let binds = parse_binds(
            "bind \"f\"\t\t\"+offhand1\"\nbind \"q\" \"+offhand2\"\n\
             bind \"MOUSE3\" \"+offhand2\" // also\nbind \"3\" \"weaponSelectOrdnance\"\n",
        );
        assert_eq!(bind_text("%offhand1%", &binds), "F");
        assert_eq!(bind_text("%offhand2%", &binds), "Q");
        assert_eq!(bind_text("[%weaponSelectOrdnance%]", &binds), "[3]");
        assert_eq!(bind_text("%unbound% x", &binds), "%unbound% x");
        assert_eq!(bind_text("100%", &binds), "100%");
    }

    #[test]
    fn offhands_show_a_bar_unless_charge_free_offhand() {
        let weapon = |pairs: &[(&str, &str)]| WeaponScript {
            values: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        };
        assert!(!shows_bar(&weapon(&[("fire_mode", "offhand")])));
        assert!(shows_bar(&weapon(&[
            ("fire_mode", "offhand"),
            ("charge_time", "2")
        ])));
        assert!(shows_bar(&weapon(&[("fire_mode", "offhand_instant")])));
        assert!(shows_bar(&weapon(&[])));
    }
}
