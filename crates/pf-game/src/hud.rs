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
//! Not yet like the game (docs/port/hud.md): the font glow, the boxes drawn around
//! key names, and most of the screen's elements. Without the game files there
//! is simply no HUD.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use pf_fmt_ui::{kv, layout, vfont};

use crate::Screen;
use crate::hudbar::CircularBar;
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
        app.add_plugins(UiMaterialPlugin::<CircularBar>::default())
            .add_systems(OnEnter(Screen::InGame), spawn)
            .add_systems(
                Update,
                (update, titan_meter, crop_bars, cockpit_sway, place)
                    .chain()
                    .run_if(in_state(Screen::InGame)),
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
    /// The hex's dark cover over the part of the Titan still to build.
    TitanCover,
    /// The hex's ring background, shown while building.
    TitanRing,
    /// The Titan icon: building, then ready.
    TitanIcon,
    /// Time left until the Titan is ready.
    TitanTimer,
    /// Time left in the match.
    MatchClock,
}

/// A straight bar's fill showing only this fraction of its image, from the
/// left; set up once the image's size is known.
#[derive(Component)]
struct BarCrop(f32);

/// The image an element switches to when the Titan is ready.
#[derive(Component)]
struct ReadyImage(Handle<Image>);

/// The Titan build timer (`UpdateTitanModeHUD`, `cl_replacement_titan_hud.nut`).
/// The first Titan takes the playlist's `titan_build_time`. Nothing earns time
/// off it yet (kills do in the game), so it simply counts down from the start.
/// The cockpit's sway settings and state ([`crate::cockpit_sway`]).
#[derive(Resource)]
struct CockpitSway {
    settings: crate::sway::Settings,
    sway: crate::sway::Sway,
}

#[derive(Resource)]
struct TitanBuild {
    total: f32,
    start: f32,
    /// The match's time limit, seconds.
    match_length: f32,
}

/// The game mode and team the HUD shows. Pilotframe has no modes or teams
/// yet, so this is Pilot Hunter (the game's `tdm`) on the Militia, as in the
/// R1Delta screenshot.
const GAME_MODE: &str = "tdm";
const GAME_MODE_NAME: &str = "#GAMEMODE_PILOT_HUNTER";
const FRIENDLY_TEAM_NAME: &str = "#TEAM_MCOR";
const ENEMY_TEAM_NAME: &str = "#TEAM_IMC";
/// `TEAM_ICON_MILITIA` (`cl_main_hud.nut`).
const FRIENDLY_TEAM_ICON: &str = "../ui/scoreboard_mcorp_logo";

/// A value from the first playlist's `vars` (`playlists.txt`).
fn playlist_var(key: &str) -> Option<f32> {
    let text = crate::data::read_text("playlists.txt")?;
    let root = kv::parse(&text).nodes.into_iter().next()?;
    let vars = root.get("rotation")?.get("vars")?;
    vars.text(key)?.trim().parse().ok()
}

/// The playlist's `titan_build_time` (`playlists.txt`, the first playlist's
/// `vars`); the script's own default is 340.
fn titan_build_time() -> f32 {
    playlist_var("titan_build_time").unwrap_or(340.0)
}

/// Seconds as the localisation's countdown format (`HudAutoText_CountdownTime`,
/// "%s1:%s2"): minutes, then two-digit seconds, counting up from a part second.
fn countdown(format: &str, seconds: f32) -> String {
    let total = seconds.max(0.0).ceil() as u32;
    format
        .replace("%s1", &(total / 60).to_string())
        .replace("%s2", &format!("{:02}", total % 60))
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
    /// A progress bar with the colours the script sets (`None` keeps the
    /// layout's) and what changes on its fill and background.
    Bar {
        fill: Option<Live>,
        background: Option<Live>,
        fg: Option<Color>,
        bg: Option<Color>,
    },
    /// This image now, then the second one when the Titan is ready.
    TitanImages(&'static str, &'static str),
    /// A straight bar filled to this fraction.
    Fraction(f32),
    /// A key hint with key-binding icons on (`EnableKeyBindingIcons`): the
    /// bound key drawn on a key-cap image.
    KeyHint,
}

/// The hex's Titan icons (`cl_replacement_titan_hud.nut` constants).
const TITAN_BUILDING_ICON: &str = "HUD/titan_build_icon_5";
const TITAN_READY_ICON: &str = "HUD/titan_stance_ready";

/// The elements the HUD script shows for this loadout, and what goes in them.
/// - The weapon panel (`MainHud_InitWeapons`).
/// - The two offhand slots (`InitOffhandWeaponHud`): slot 0 is the ordnance
///   (`OFFHAND_RIGHT`, key `+offhand1`), slot 1 the special ability
///   (`OFFHAND_LEFT`, `+offhand2`). Each shows its weapon's `hud_icon` and key,
///   and either a count or a ready bar (see [`shows_bar`]).
/// - The anti-Titan weapon in the d-pad's left slot (`MainHud_UpdateWeapons`):
///   its `hud_icon` and key. Its title and description are drawn at alpha 0.
fn elements(loadout: &Loadout) -> Vec<(String, Fill)> {
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
        let Some(weapon) = WeaponScript::load(name, &passives) else {
            continue;
        };
        let element = |part: &str| format!("Offhand{slot}{part}");
        out.push((element("BG"), Fill::Layout));
        if let Some(icon) = weapon.text("hud_icon") {
            out.push((element("Icon"), Fill::Image(icon.to_string())));
        }
        out.push((element("Hint"), Fill::KeyHint));
        if shows_bar(&weapon) {
            // Full while ready; abilities can't be used yet, so always ready.
            out.push((element("Bar"), Fill::Layout));
        } else {
            let format = localise(&format!("#HudAutoText_PilotOffhandWeaponAmmo{slot}"));
            let count = weapon.num("ammo_clip_size").unwrap_or(0.0) as u32;
            let text = format.replace("%s1", &count.to_string());
            out.push((element("Count"), Fill::Text(text)));
        }
    }
    if let Some(icon) = WeaponScript::load(&loadout.secondary, &passives)
        .and_then(|w| w.text("hud_icon").map(str::to_string))
    {
        out.push(("dpadLeftIcon".into(), Fill::Image(icon)));
        out.push(("dpadLeftIconHint".into(), Fill::KeyHint));
    }
    // The hex's down slot while the Titan builds (`UpdateTitanModeHUD`): the
    // building icon, the call-in key, a countdown, a dark cover over the part
    // still to build, and the ring background at black 160.
    out.extend(
        [
            (
                "dpadDownIconBG",
                Fill::Bar {
                    fill: Some(Live::TitanCover),
                    background: None,
                    fg: None,
                    bg: None,
                },
            ),
            (
                "dpadDownIconProgressBar",
                Fill::Bar {
                    fill: None,
                    background: Some(Live::TitanRing),
                    fg: Some(Color::NONE),
                    bg: Some(Color::srgba_u8(0, 0, 0, 160)),
                },
            ),
            (
                "dpadDownIcon",
                Fill::TitanImages(TITAN_BUILDING_ICON, TITAN_READY_ICON),
            ),
            ("dpadDownIconHint", Fill::KeyHint),
            (
                "dpadDownDesc",
                Fill::Live(Live::TitanTimer, Some("#HudAutoText_CountdownTime")),
            ),
        ]
        .map(|(n, f)| (n.to_string(), f)),
    );
    out.extend(score_panel());
    out
}

/// The score panel, bottom left (`MainHud_InitScoreBars`):
/// - The hex with the team's logo, the time left above it (`HATT_COUNTDOWN_TIME`
///   to the match's end) and the mode's name beside it.
/// - A strip per team: its score bar (remapped from 0..score limit to
///   0.011..0.96 of the strip), its score and its name.
/// - The 8-player status background; the Titan counts draw nothing with no
///   Titans out.
fn score_panel() -> Vec<(String, Fill)> {
    let limit = playlist_var(&format!("{GAME_MODE}_scorelimit")).unwrap_or(1.0);
    let score = 0.0;
    let fraction = 0.011 + (score / limit.max(1.0)).clamp(0.0, 1.0) * (0.96 - 0.011);
    let team =
        |token: &str| localise("#HudAutoText_FriendlyTeamName").replace("%s1", &localise(token));
    let score_text = localise("#HudAutoText_FriendlyTeamScore").replace("%s1", "0");
    [
        ("GameInfoBG", Fill::Layout),
        ("GameInfoFG", Fill::Layout),
        ("ScoresBG", Fill::Layout),
        ("ScoresFG", Fill::Layout),
        ("Player_Status_BG", Fill::Layout),
        ("GameModeLabel", Fill::Text(localise(GAME_MODE_NAME))),
        ("ScoresFriendly", Fill::Fraction(fraction)),
        ("ScoresLabelFriendly", Fill::Text(score_text.clone())),
        (
            "ScoresTeamLabelFriendly",
            Fill::Text(team(FRIENDLY_TEAM_NAME)),
        ),
        ("ScoresEnemy", Fill::Fraction(fraction)),
        ("ScoresLabelEnemy", Fill::Text(score_text)),
        ("ScoresTeamLabelEnemy", Fill::Text(team(ENEMY_TEAM_NAME))),
        (
            "GameInfoLabel",
            Fill::Live(Live::MatchClock, Some("#HudAutoText_CountdownTime")),
        ),
        ("GameInfoTeamIcon", Fill::Image(FRIENDLY_TEAM_ICON.into())),
    ]
    .map(|(n, f)| (n.to_string(), f))
    .into()
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
fn default_binds() -> HashMap<String, String> {
    let text = crate::data::read_text("cfg/config_default_pc.cfg").unwrap_or_default();
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

/// The key-cap image for a key (`client.dll`, `0x18043a300`): a named sprite
/// from `scripts/hud_textures.txt`. Mouse buttons, the wheel and the arrow keys
/// have their own pictures and no text; other keys get `icon_key_generic`
/// (keys of one or two letters, and `;` `'` `,` `.`) or `icon_key_wide`, with
/// the key's name on top. Returns the sprite's file, its rectangle in pixels,
/// and whether the key's name is drawn on it.
fn key_icon(key: &str) -> Option<(String, Rect, bool)> {
    let upper = key.to_ascii_uppercase();
    let (sprite, text) = match upper.as_str() {
        "MOUSE1" => ("icon_mouseLeft", false),
        "MOUSE2" => ("icon_mouseRight", false),
        "MOUSE3" => ("icon_mouseThree", false),
        "MWHEELUP" => ("icon_mouseWheel_up", false),
        "MWHEELDOWN" => ("icon_mouseWheel_down", false),
        "UPARROW" => ("icon_key_up", false),
        "LEFTARROW" => ("icon_key_left", false),
        "DOWNARROW" => ("icon_key_down", false),
        "RIGHTARROW" => ("icon_key_right", false),
        "SEMICOLON" | "'" | "," | "." => ("icon_key_generic", true),
        k if k.chars().count() >= 3 => ("icon_key_wide", true),
        _ => ("icon_key_generic", true),
    };
    let text_file = crate::data::read_text("scripts/hud_textures.txt")?;
    let file = kv::parse(&text_file);
    let data = file.nodes.first()?.get("TextureData")?;
    let entry = data
        .children()
        .iter()
        .find(|n| n.key.eq_ignore_ascii_case(sprite))?;
    let num = |k: &str| entry.text(k).and_then(|v| v.trim().parse::<f32>().ok());
    let (x, y) = (num("x")?, num("y")?);
    let rect = Rect::new(x, y, x + num("width")?, y + num("height")?);
    // The file is a material path from `materials/`; ours adds the `vgui/`.
    let file = entry.text("file")?.replace('\\', "/");
    let file = file
        .strip_prefix("vgui/")
        .or_else(|| file.strip_prefix("VGUI/"))
        .unwrap_or(&file)
        .to_string();
    Some((file, rect, text))
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
fn load_res(path: &str) -> Option<kv::Node> {
    let text = crate::data::read_text(path)?;
    let file = kv::parse(&text);
    let mut root = file.nodes.into_iter().next()?;
    let folder = path.rsplit_once('/').map_or("", |(d, _)| d);
    for base in file.bases.iter().rev() {
        if let Some(mut merged) = load_res(&format!("{folder}/{base}")) {
            merged.merge(&root);
            root = merged;
        }
    }
    Some(root)
}

/// The English text for a `#TOKEN`, from `resource/r1_english.txt` (UTF-16).
/// The file is read once and kept.
fn localise(token: &str) -> String {
    // Bevy note: not Bevy at all; `OnceLock` is a static filled on first use.
    static TOKENS: std::sync::OnceLock<HashMap<String, String>> = std::sync::OnceLock::new();
    let tokens = TOKENS.get_or_init(|| {
        let text = crate::data::read_text("resource/r1_english.txt").unwrap_or_default();
        kv::parse(&text)
            .nodes
            .first()
            .and_then(|lang| lang.get("Tokens"))
            .map(|t| {
                // Reversed, so the first of any repeated key wins.
                t.children()
                    .iter()
                    .rev()
                    .filter_map(|n| match &n.value {
                        kv::Value::Text(v) => Some((n.key.to_ascii_lowercase(), v.clone())),
                        kv::Value::Block(_) => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    });
    let key = token.trim_start_matches('#');
    tokens
        .get(&key.to_ascii_lowercase())
        .cloned()
        .unwrap_or_else(|| key.to_string())
}

/// A scheme font: its file and its size (`tall`) in helmet-screen units.
struct SchemeFont {
    handle: Handle<Font>,
    tall: f32,
}

/// The HUD fonts named in `hudscriptedscheme.res`, loaded from their `.vfont`s.
fn load_fonts(fonts: &mut Assets<Font>) -> HashMap<String, SchemeFont> {
    let mut by_file: HashMap<String, Handle<Font>> = HashMap::new();
    let mut out = HashMap::new();
    let Some(scheme) = crate::data::read_text("resource/hudscriptedscheme.res")
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
                let Some(font) = crate::data::read(&format!("resource/{file}.vfont"))
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
fn hud_image(images: &mut Assets<Image>, name: &str) -> Option<(Handle<Image>, bool)> {
    let material = format!("vgui/{}", name.replace('\\', "/").to_ascii_lowercase());
    let loaded = pf_world::load_material(&material)?;
    let additive = loaded.additive;
    loaded.texture.map(|i| (images.add(i), additive))
}

/// `MainHud_InitAspectRatio` for the pilot HUD: with the safe-area setting off
/// (`cl_safearea` defaults to 0 on PC), `SafeArea` grows to 90% × 90% of the
/// screen and `SafeAreaCenter` to 90% × 95%. Both are then narrowed by
/// `aspectRatioScale` (the window's aspect ÷ 16:9, at most 1) so the HUD stays
/// on narrower screens. Most HUD elements are pinned to one of the two.
fn resize_safe_area(root: &mut kv::Node, aspect: f32) {
    const NOSAFE_X: f32 = 0.10;
    const NOSAFE_Y: f32 = 0.10;
    let scale_x = (aspect / (16.0 / 9.0)).min(1.0);
    let (w, h) = SCREEN;
    // VGUI sizes are whole pixels.
    let wide = (w - w * NOSAFE_X).trunc() * scale_x;
    let text = format!(
        "x {{ SafeArea {{ wide {wide} tall {} }} SafeAreaCenter {{ wide {wide} tall {} }} }}",
        (h - h * NOSAFE_Y).trunc(),
        (h - h * NOSAFE_Y * 0.5).trunc(),
    );
    if let Some(overlay) = kv::parse(&text).nodes.first() {
        root.merge(overlay);
    }
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

#[allow(clippy::too_many_arguments)]
fn spawn(
    mut commands: Commands,
    time: Res<Time>,
    mut bar_materials: ResMut<Assets<CircularBar>>,
    mut fonts: ResMut<Assets<Font>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    weapon: Option<Single<&Viewmodel>>,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
) {
    let Some(mut root) = load_res("scripts/screens/vgui_fullscreen_pilot.res") else {
        info!("no HUD: the pilot HUD layout isn't in the game files");
        return;
    };
    let aspect = window.map_or(16.0 / 9.0, |w| w.width() / w.height().max(1.0));
    resize_safe_area(&mut root, aspect);
    let fonts = load_fonts(&mut fonts);
    let binds = default_binds();
    let loadout = Loadout::preset(DEFAULT_PRESET).unwrap_or_default();
    commands.insert_resource(CockpitSway {
        settings: crate::cockpit_sway::Settings::load().sway(),
        sway: default(),
    });
    commands.insert_resource(TitanBuild {
        total: titan_build_time().max(1.0),
        start: time.elapsed_secs(),
        match_length: playlist_var(&format!("{GAME_MODE}_timelimit")).unwrap_or(0.0) * 60.0,
    });
    let weapon_name = weapon
        .as_ref()
        .map(|w| localise(w.short_name()))
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

    for (name, fill) in elements(&loadout) {
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
                    Fill::TitanImages(now, _) => Some(*now),
                    _ => node.text("image"),
                };
                if let Some((image, additive)) = image.and_then(|i| hud_image(&mut images, i)) {
                    let mut panel = commands.spawn((
                        base,
                        ImageNode::new(image).with_color(color(node.text("drawColor"))),
                        ChildOf(layer(additive)),
                    ));
                    if let Fill::TitanImages(_, ready) = fill
                        && let Some((ready, _)) = hud_image(&mut images, ready)
                    {
                        panel.insert((Live::TitanIcon, ReadyImage(ready)));
                    }
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
                let all_caps = node.text("allcaps").is_some_and(|v| v.trim() == "1");
                let start = if all_caps {
                    start.to_uppercase()
                } else {
                    start
                };
                let right = across == JustifyContent::FlexEnd;
                // Text insets as Source's `Label` paints them: `textinsety` is
                // added to y whatever the alignment; `textinsetx` moves the
                // text in from the side it's aligned to (none when centred).
                let inset = |key: &str| {
                    node.text(key)
                        .and_then(|v| v.trim().parse::<f32>().ok())
                        .unwrap_or(0.0)
                        * PIXELS_PER_UNIT
                };
                let inset_x = match across {
                    JustifyContent::FlexStart => inset("textinsetx"),
                    JustifyContent::FlexEnd => -inset("textinsetx"),
                    _ => 0.0,
                };
                // Key-binding icons (the text image's paint, `0x180726870`): the
                // key-cap is 1.5 × the font's height, centred on the text line,
                // as wide as its sprite's shape. The key's name is centred on
                // it, shrunk if it's wider than the cap less one character's
                // share of padding.
                if matches!(fill, Fill::KeyHint)
                    && let Some((file, sprite, with_text)) = key_icon(&start)
                    && let Some((cap, _)) = hud_image(&mut images, &file)
                {
                    let height = 1.5 * tall * PIXELS_PER_UNIT;
                    let width = sprite.width() / sprite.height() * height;
                    let icon = commands
                        .spawn((
                            Node {
                                left: Val::Px(inset_x),
                                top: Val::Px(inset("textinsety")),
                                width: Val::Px(width),
                                height: Val::Px(height),
                                flex_shrink: 0.0,
                                display: Display::Flex,
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            ImageNode {
                                rect: Some(sprite),
                                ..ImageNode::new(cap)
                            },
                            ChildOf(holder),
                        ))
                        .id();
                    if with_text {
                        // Glyph widths aren't measured here: about 0.6 of
                        // the font's height each.
                        let chars = start.chars().count().max(1) as f32;
                        let text_width = chars * 0.6 * tall * PIXELS_PER_UNIT;
                        let needed = text_width / chars + text_width;
                        let fit = (width / needed).min(1.0);
                        let mut text = commands.spawn((
                            Text::new(start),
                            TextColor(Color::WHITE),
                            TextLayout::justify(Justify::Center).with_no_wrap(),
                            ChildOf(icon),
                        ));
                        if let Some(f) = font {
                            text.insert(TextFont {
                                font: FontSource::Handle(f.handle.clone()),
                                font_size: FontSize::Px(tall * PIXELS_PER_UNIT * fit),
                                ..default()
                            });
                        }
                    }
                    continue;
                }
                let mut text = commands.spawn((
                    Node {
                        left: Val::Px(inset_x),
                        top: Val::Px(inset("textinsety")),
                        ..default()
                    },
                    Text::new(start),
                    TextColor(color(node.text("fgcolor_override"))),
                    TextLayout::justify(if right { Justify::Right } else { Justify::Left })
                        .with_no_wrap(),
                    ChildOf(holder),
                ));
                if let Fill::Live(live, format) = fill {
                    text.insert(live);
                    if let Some(token) = format {
                        text.insert(AutoText(localise(token)));
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
                let fraction = match fill {
                    Fill::Fraction(f) => f,
                    _ => 1.0,
                };
                let (fill_live, bg_live, fg_color, bg_color) = match fill {
                    Fill::Live(live, _) => (Some(live), None, None, None),
                    Fill::Bar {
                        fill,
                        background,
                        fg,
                        bg,
                    } => (fill, background, fg, bg),
                    _ => (None, None, None, None),
                };
                let fg_color = fg_color.unwrap_or_else(|| color(node.text("fgcolor_override")));
                let bg_color = bg_color.unwrap_or_else(|| color(node.text("bgcolor_override")));
                let bg = node
                    .text("bg_image")
                    .and_then(|i| hud_image(&mut images, i));
                let fg = node
                    .text("fg_image")
                    .and_then(|i| hud_image(&mut images, i));
                let setting = |key: &str| node.text(key).is_some_and(|v| v.trim() == "1");
                // Background and fill may blend differently, so each goes on
                // its own layer.
                if let Some((bg, additive)) = bg {
                    let mut back = commands.spawn((
                        base,
                        ImageNode::new(bg).with_color(bg_color),
                        ChildOf(layer(additive)),
                    ));
                    if let Some(live) = bg_live {
                        back.insert(live);
                    }
                }
                // An invisible fill isn't drawn at all.
                if let Some((fg, additive)) = fg.filter(|_| fg_color.alpha() > 0.0) {
                    let mut fill_rect = *rect;
                    fill_rect.w *= fraction;
                    let mut fill_part = commands.spawn((
                        HudBox(*rect),
                        hud_node(&fill_rect),
                        ZIndex(z + 1),
                        ChildOf(layer(additive)),
                    ));
                    if setting("CircularEnabled") {
                        let clockwise = setting("CircularClockwise");
                        let bar = CircularBar::new(fg, fg_color, clockwise, 1.0);
                        fill_part.insert(MaterialNode(bar_materials.add(bar)));
                    } else {
                        // A straight bar: its width follows the value, showing
                        // that much of its image from the left.
                        fill_part.insert(ImageNode::new(fg).with_color(fg_color));
                        if fraction < 1.0 {
                            fill_part.insert(BarCrop(fraction));
                        }
                    }
                    if let Some(live) = fill_live {
                        fill_part.insert(live);
                    }
                }
            }
            _ => {}
        }
    }

    // The crosshair: shadow arms under the line arms.
    for (layer, name) in ["standard_plus_shadow", "standard_plus"].iter().enumerate() {
        let Some((image, _)) = hud_image(&mut images, &format!("hud/crosshairs/{name}")) else {
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
            // Whole spare magazines: the count drops when a reload takes from
            // a full one (seen in the game; the code computing it isn't read yet).
            Live::MagCount => text.0 = auto(format, w.reserve() / w.clip_size().max(1)),
            // The gun may arrive after the HUD; look its name up once.
            Live::WeaponName if text.0.is_empty() => text.0 = localise(w.short_name()),
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

/// Move the helmet screen with the cockpit's sway. The game sways the cockpit
/// model the screen hangs off: an offset and a rotation about the eye. On our
/// flat screen that's (for the small angles involved, at the screen's 208
/// inches):
/// - left/up offsets and yaw/pitch shift it (yaw by 208·tan(yaw));
/// - roll turns it;
/// - the forward offset scales it by 208 / (208 + forward).
fn cockpit_sway(
    time: Res<Time>,
    state: Option<ResMut<CockpitSway>>,
    player: Option<Single<&crate::player::Player>>,
    mut panels: Query<&mut Transform, With<hudwarp::HudPanel>>,
) {
    let (Some(mut state), Some(player)) = (state, player) else {
        return;
    };
    let (yaw, pitch) = player.view_angles();
    let (velocity, view) = crate::sway::view_input(player.movement().velocity, yaw, pitch);
    let CockpitSway { settings, sway } = &mut *state;
    sway.update(
        settings,
        &crate::sway::Input {
            dt: time.delta_secs(),
            velocity,
            view,
            zoomed: false,
            zoom_fraction: 0.0,
            on_ground: player.movement().on_ground,
        },
    );

    let units = hudwarp::UNITS_PER_WORLD_UNIT;
    let distance = hudwarp::PANEL_DISTANCE;
    let ([forward, left, up], [sway_pitch, sway_yaw, sway_roll]) = sway.total();
    let x = -left * units - distance * sway_yaw.to_radians().tan() * units;
    let y = up * units - distance * sway_pitch.to_radians().tan() * units;
    let scale = distance / (distance + forward);
    for mut t in &mut panels {
        t.translation.x = x;
        t.translation.y = y;
        // The game's roll turns the panel clockwise on screen.
        t.rotation = Quat::from_rotation_z(-sway_roll.to_radians());
        t.scale = Vec3::new(scale, scale, 1.0);
    }
}

/// Cut straight bars' fills to their fraction of the image, once it's loaded.
fn crop_bars(images: Res<Assets<Image>>, mut bars: Query<(&BarCrop, &mut ImageNode)>) {
    for (BarCrop(fraction), mut node) in &mut bars {
        if node.rect.is_none()
            && let Some(image) = images.get(&node.image)
        {
            let size = image.size_f32();
            node.rect = Some(Rect::new(0.0, 0.0, size.x * fraction, size.y));
        }
    }
}

/// The hex's Titan meter while building, then ready
/// (`UpdateTitanModeHUD`):
/// - The cover's bar is set to the fraction still to build and runs down
///   (`SetDpadCooldown` with `SetBarProgressOverTime`).
/// - The text counts down to the Titan being ready, then goes blank.
/// - When ready, the icon switches to the ready one and the ring's background
///   goes clear (`SetDpadProgressColor` 0, 0).
#[allow(clippy::type_complexity)]
fn titan_meter(
    time: Res<Time>,
    build: Option<Res<TitanBuild>>,
    mut bars: ResMut<Assets<CircularBar>>,
    covers: Query<(&Live, &MaterialNode<CircularBar>)>,
    mut texts: Query<(&Live, &mut Text, Option<&AutoText>)>,
    mut images: Query<(&Live, &mut ImageNode, Option<&ReadyImage>)>,
) {
    let Some(build) = build else {
        return;
    };
    let left = (build.total - (time.elapsed_secs() - build.start)).max(0.0);
    let ready = left <= 0.0;
    for (live, MaterialNode(handle)) in &covers {
        if *live == Live::TitanCover
            && let Some(mut bar) = bars.get_mut(handle)
        {
            bar.set_progress(left / build.total);
        }
    }
    let match_left = (build.match_length - (time.elapsed_secs() - build.start)).max(0.0);
    for (live, mut text, format) in &mut texts {
        if *live == Live::MatchClock
            && let Some(f) = format
        {
            let shown = countdown(&f.0, match_left);
            if text.0 != shown {
                text.0 = shown;
            }
        }
        if *live == Live::TitanTimer {
            let shown = match (ready, format) {
                (false, Some(f)) => countdown(&f.0, left),
                _ => String::new(),
            };
            if text.0 != shown {
                text.0 = shown;
            }
        }
    }
    if !ready {
        return;
    }
    for (live, mut image, ready_image) in &mut images {
        match (live, ready_image) {
            (Live::TitanIcon, Some(ReadyImage(r))) if image.image != *r => image.image = r.clone(),
            (Live::TitanRing, _) => image.color = Color::NONE,
            _ => {}
        }
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
    fn countdown_shows_minutes_and_two_digit_seconds() {
        assert_eq!(countdown("%s1:%s2", 224.0), "3:44");
        assert_eq!(countdown("%s1:%s2", 223.2), "3:44");
        assert_eq!(countdown("%s1:%s2", 65.0), "1:05");
        assert_eq!(countdown("%s1:%s2", 0.0), "0:00");
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
