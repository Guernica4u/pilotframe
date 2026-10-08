//! The pilot HUD, built from the game's own files: the layout of the pilot's
//! helmet screen (`scripts/screens/vgui_fullscreen_pilot.res` and its base), the
//! HUD fonts (`.vfont`) and font sizes (`resource/hudscriptedscheme.res`), the
//! textures under `materials/vgui/HUD`, the weapon's name from the English
//! localisation, and the weapon's crosshair settings.
//!
//! Not yet like the game (docs/hud.md): the helmet screen's curve ("hudwarp"),
//! additive blending of some textures, the font glow, and the crosshair opening
//! up with spread. Without the game files there is simply no HUD.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use pf_fmt_ui::{kv, layout, vfont};

use crate::Screen;
use crate::level::LevelEntity;
use crate::viewmodel::Viewmodel;

/// The helmet screen's size in its own units (`vgui_screens.txt`).
const SCREEN: (f32, f32) = (852.0, 480.0);
/// Screen HUD elements (the crosshair) are sized for a screen this tall.
const CROSSHAIR_SPACE_TALL: f32 = 480.0;
/// The parts of the helmet screen we draw so far: the weapon panel.
const ELEMENTS: [&str; 8] = [
    "dpadIconBG",
    "dpadIcon",
    "dpadSideBG",
    "weaponAmmoBar",
    "weaponLabel",
    "weaponAmmoCount",
    "weaponMagLabel",
    "weaponAmmoMag",
];
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

#[derive(Component)]
struct Crosshair {
    alpha: f32,
}

/// The font size an element's text was laid out at, in helmet-screen units.
#[derive(Component)]
struct FontUnits(f32);

fn common_dir() -> PathBuf {
    PathBuf::from("Resources/pc_extracted/common")
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

fn hud_image(dir: &Path, images: &mut Assets<Image>, name: &str) -> Option<Handle<Image>> {
    let material = format!("vgui/{}", name.replace('\\', "/").to_ascii_lowercase());
    pf_world::load_material(&[dir.join("materials")], &material)?
        .texture
        .map(|i| images.add(i))
}

fn spawn(
    mut commands: Commands,
    mut fonts: ResMut<Assets<Font>>,
    mut images: ResMut<Assets<Image>>,
    weapon: Option<Single<&Viewmodel>>,
) {
    let dir = common_dir();
    let Some(root) = load_res(&dir.join("scripts/screens/vgui_fullscreen_pilot.res")) else {
        info!("no HUD: the pilot HUD layout isn't under {}", dir.display());
        return;
    };
    let fonts = load_fonts(&dir, &mut fonts);
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

    for name in ELEMENTS {
        let (Some(node), Some(rect)) = (root.get(name), rects.get(&name.to_ascii_lowercase()))
        else {
            continue;
        };
        let z = node
            .text("zpos")
            .and_then(|z| z.trim().parse::<i32>().ok())
            .unwrap_or(0);
        let base = (
            HudBox(*rect),
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            ZIndex(z),
            ChildOf(hud),
        );
        match node.text("ControlName").unwrap_or_default() {
            "ImagePanel" => {
                if let Some(image) = node
                    .text("image")
                    .and_then(|i| hud_image(&dir, &mut images, i))
                {
                    commands.spawn((
                        base,
                        ImageNode::new(image).with_color(color(node.text("drawColor"))),
                    ));
                }
            }
            "Label" => {
                let font = node
                    .text("font")
                    .and_then(|f| fonts.get(&f.to_ascii_lowercase()));
                let live = match name {
                    "weaponAmmoCount" => Live::AmmoCount,
                    "weaponMagLabel" => Live::MagCount,
                    _ => Live::WeaponName,
                };
                let east = node
                    .text("textAlignment")
                    .is_some_and(|a| a.eq_ignore_ascii_case("east"));
                let tall = font.map_or(12.0, |f| f.tall);
                let mut text = commands.spawn((
                    base,
                    Text::new(if live == Live::WeaponName {
                        weapon_name.clone()
                    } else {
                        String::new()
                    }),
                    TextColor(color(node.text("fgcolor_override"))),
                    TextLayout::justify(if east { Justify::Right } else { Justify::Left }),
                    FontUnits(tall),
                    live,
                ));
                if let Some(f) = font {
                    text.insert(TextFont {
                        font: FontSource::Handle(f.handle.clone()),
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
                let bar = commands.spawn(base).id();
                if let Some(bg) = bg {
                    commands.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        ImageNode::new(bg).with_color(color(node.text("bgcolor_override"))),
                        ChildOf(bar),
                    ));
                }
                if let Some(fg) = fg {
                    commands.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        ImageNode::new(fg).with_color(color(node.text("fgcolor_override"))),
                        Live::AmmoBarFill,
                        ChildOf(bar),
                    ));
                }
            }
            _ => {}
        }
    }

    // The crosshair: shadow arms under the line arms.
    for (layer, name) in ["standard_plus_shadow", "standard_plus"].iter().enumerate() {
        let Some(image) = hud_image(&dir, &mut images, &format!("hud/crosshairs/{name}")) else {
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
                UiTransform::from_rotation(Rot2::degrees(angle)),
                ZIndex(300 + layer as i32),
                Crosshair { alpha: 1.0 },
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
    mut texts: Query<(&Live, &mut Text)>,
    mut bars: Query<(&Live, &mut Node), Without<Text>>,
    mut crosshairs: Query<(&mut Crosshair, &mut ImageNode)>,
) {
    let Some(w) = weapon else {
        return;
    };
    for (live, mut text) in &mut texts {
        match live {
            Live::AmmoCount => text.0 = w.ammo().to_string(),
            // Spare magazines, counting a part-full one.
            Live::MagCount => text.0 = w.reserve().div_ceil(w.clip_size().max(1)).to_string(),
            // The gun may arrive after the HUD; look its name up once.
            Live::WeaponName if text.0.is_empty() => {
                text.0 = localise(&common_dir(), w.short_name())
            }
            _ => {}
        }
    }
    for (live, mut node) in &mut bars {
        if *live == Live::AmmoBarFill {
            let fraction = w.ammo() as f32 / w.clip_size().max(1) as f32;
            node.width = Val::Percent(fraction * 100.0);
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

/// Scale the helmet screen to the window (centred, keeping its shape) and place
/// every element; the crosshair sits in the middle, sized by window height.
#[allow(clippy::type_complexity)]
fn place(
    window: Single<&Window, With<PrimaryWindow>>,
    mut boxes: Query<(
        &HudBox,
        &mut Node,
        Option<&FontUnits>,
        Option<&mut TextFont>,
    )>,
    mut crosshairs: Query<&mut Node, (With<Crosshair>, Without<HudBox>)>,
) {
    let (ww, wh) = (window.width(), window.height());
    let scale = (ww / SCREEN.0).min(wh / SCREEN.1);
    let (ox, oy) = ((ww - SCREEN.0 * scale) / 2.0, (wh - SCREEN.1 * scale) / 2.0);
    for (HudBox(r), mut node, units, font) in &mut boxes {
        node.left = Val::Px(ox + r.x * scale);
        node.top = Val::Px(oy + r.y * scale);
        node.width = Val::Px(r.w * scale);
        node.height = Val::Px(r.h * scale);
        if let (Some(FontUnits(tall)), Some(mut font)) = (units, font) {
            font.font_size = FontSize::Px(tall * scale);
        }
    }
    let size = CROSSHAIR_SIZE * wh / CROSSHAIR_SPACE_TALL;
    for mut node in &mut crosshairs {
        node.left = Val::Px((ww - size) / 2.0);
        node.top = Val::Px((wh - size) / 2.0);
        node.width = Val::Px(size);
        node.height = Val::Px(size);
    }
}
