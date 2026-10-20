//! The pilot HUD run by the game's own HUD scripts (roadmap R11,
//! docs/port/hud.md). The client VM (`pf-clientvm`) runs the client scripts
//! from the install; each frame the panels the scripts set up on the pilot's
//! helmet screen (`vgui_fullscreen_pilot`) are drawn into the helmet
//! screen's layers, which `crate::hudwarp` shows curved.
//!
//! Switched on with `PF_SCRIPT_HUD=1` while it is checked against the game;
//! until then the hand-built helmet screen (`crate::hud`) is the default.
//!
//! Stand-ins (ours, marked): there is no server yet, so the game state comes
//! from `pf-clientvm`'s stand-in server (the match playing, Attrition, the
//! local pilot on IMC), and every persistent variable reads 0. The weapon
//! the HUD shows is the viewmodel's.
//!
//! Bevy note: the VM uses `Rc`, so it can't move between threads; it lives
//! in a *non-send* resource, which Bevy only hands to systems on the main
//! thread.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use pf_clientvm::autotext::WeaponHud;
use pf_clientvm::persistence::PersistenceDef;
use pf_clientvm::{ClientVm, EntityId, EntityInfo, Host, PersistentValue};
use pf_vgui::{Control, PanelId, Ui};

use crate::Screen;
use crate::hud::{self, HelmetLayers, SchemeFont};
use crate::hudbar::CircularBar;
use crate::hudwarp::PIXELS_PER_UNIT;
use crate::viewmodel::Viewmodel;

/// The helmet screen the scripts fill.
const PILOT_SCREEN: &str = "vgui_fullscreen_pilot";

/// The local player and its cockpit, as the stand-in game numbers them.
const PLAYER: EntityId = 1;
const COCKPIT: EntityId = 2;

/// Whether the script-run HUD is on (`PF_SCRIPT_HUD`).
pub fn enabled() -> bool {
    std::env::var_os("PF_SCRIPT_HUD").is_some()
}

pub struct ScriptHudPlugin;

impl Plugin for ScriptHudPlugin {
    fn build(&self, app: &mut App) {
        if !enabled() {
            return;
        }
        app.add_systems(OnEnter(Screen::InGame), start.after(hud::spawn))
            .add_systems(
                Update,
                (feed, run_frame, draw)
                    .chain()
                    .run_if(in_state(Screen::InGame)),
            );
    }
}

/// What the scripts can see of the game.
struct GameHost {
    level: String,
    /// The pilot class's cockpit model: the HUD scripts build the pilot
    /// helmet screen only on a pilot cockpit.
    cockpit_model: String,
    time: Cell<f32>,
    frame_time: Cell<f32>,
    screen: Cell<(i32, i32)>,
    weapon: RefCell<Option<WeaponHud>>,
    pdef: Rc<PersistenceDef>,
}

impl Host for GameHost {
    fn read_script(&self, path: &str) -> Option<String> {
        crate::data::read_text(path)
    }
    fn level_name(&self) -> String {
        self.level.clone()
    }
    fn time(&self) -> f32 {
        self.time.get()
    }
    fn frame_time(&self) -> f32 {
        self.frame_time.get()
    }
    fn convar(&self, name: &str) -> String {
        match name {
            // Stand-in: Attrition, as the stand-in server reports.
            "mp_gamemode" => "tdm".into(),
            _ => String::new(),
        }
    }
    fn entity(&self, id: EntityId) -> Option<EntityInfo> {
        if id == COCKPIT {
            return Some(EntityInfo {
                script_class: "C_Titan_Cockpit".into(),
                classname: "titan_cockpit".into(),
                signifier: "titan_cockpit".into(),
                ent_index: 2,
                team: 2,
                model: self.cockpit_model.clone(),
                ..Default::default()
            });
        }
        (id == PLAYER).then(|| EntityInfo {
            script_class: "C_Portal_Player".into(),
            classname: "player".into(),
            signifier: "player".into(),
            ent_index: 1,
            team: 2,
            health: 200,
            max_health: 200,
            is_player: true,
            cockpit: Some(COCKPIT),
            ..Default::default()
        })
    }
    fn players(&self) -> Vec<EntityId> {
        vec![PLAYER]
    }
    fn local_player(&self) -> Option<EntityId> {
        Some(PLAYER)
    }
    fn active_weapon(&self) -> Option<WeaponHud> {
        self.weapon.borrow().clone()
    }
    /// Stand-in: every persistent path reads 0 until the server work brings
    /// the player's real data.
    fn persistent_var(&self, _: &str) -> Option<PersistentValue> {
        Some(PersistentValue::Int(0))
    }
    fn persistence_def(&self) -> Option<Rc<PersistenceDef>> {
        Some(self.pdef.clone())
    }
    fn screen_size(&self) -> (i32, i32) {
        self.screen.get()
    }
}

/// The running VM and what has been drawn for its panels.
struct ScriptHud {
    cvm: ClientVm,
    host: Rc<GameHost>,
    /// Seconds since the map started (the VM's clock).
    start: f32,
    fonts: HashMap<String, SchemeFont>,
    binds: HashMap<String, String>,
    images: HashMap<String, Option<(Handle<Image>, bool)>>,
    drawn: HashMap<PanelId, Drawn>,
    debugged: bool,
}

/// The UI nodes drawn for one panel.
struct Drawn {
    /// The image or bar background, or a label's box.
    main: Entity,
    /// A label's text, or a bar's fill.
    inner: Option<Entity>,
    /// The image (or bar image) it was made with; a change remakes it.
    made_with: Option<String>,
}

fn start(world: &mut World) {
    let window = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .iter(world)
        .next()
        .map_or((1920, 1080), |w| {
            (
                w.resolution.physical_width() as i32,
                w.resolution.physical_height() as i32,
            )
        });
    let level = match std::env::var("PF_MAP").as_deref() {
        Ok("colony") => "maps/mp_colony.bsp".to_string(),
        // Ours: the greybox test map isn't one of the game's.
        _ => "maps/pf_greybox.bsp".to_string(),
    };
    let (pdef, _) = pf_clientvm::pdef::build(&crate::data::read_text);
    let host = Rc::new(GameHost {
        level,
        cockpit_model: crate::cockpit_sway::cockpit_model().unwrap_or_default(),
        time: Cell::new(0.0),
        frame_time: Cell::new(1.0 / 60.0),
        screen: Cell::new(window),
        weapon: RefCell::new(None),
        pdef: Rc::new(pdef),
    });
    let mut cvm = ClientVm::new(host.clone());
    cvm.vm.out = Box::new(|s| eprint!("{s}"));
    if let Err(e) = cvm.start() {
        error!("client scripts' start-up failed: {e}");
        return;
    }
    // The same order as the survey (docs/port/scripts.md): the stand-in
    // server's game state, the connect step, then the creation callbacks.
    cvm.standin_match_playing();
    cvm.run_client_connect_scripts(PLAYER);
    cvm.entities_created(&[PLAYER, COCKPIT]);

    let fonts = hud::load_fonts(&mut world.resource_mut::<Assets<Font>>());
    let start = world.resource::<Time>().elapsed_secs();
    world.insert_non_send(ScriptHud {
        cvm,
        host,
        start,
        fonts,
        binds: hud::default_binds(),
        images: HashMap::new(),
        drawn: HashMap::new(),
        debugged: false,
    });
}

/// Hand the scripts this frame's state: the clock and the weapon.
fn feed(time: Res<Time>, hud: Option<NonSend<ScriptHud>>, weapon: Option<Single<&Viewmodel>>) {
    let Some(hud) = hud else { return };
    hud.host.time.set(time.elapsed_secs() - hud.start);
    hud.host.frame_time.set(time.delta_secs());
    *hud.host.weapon.borrow_mut() = weapon.map(|w| WeaponHud {
        id: 0,
        print_name: w.short_name().to_string(),
        short_print_name: w.short_name().to_string(),
        clip_ammo: w.ammo() as i32,
        clip_size: w.clip_size() as i32,
        reserve_ammo: w.reserve() as i32,
    });
}

/// Run the scripts' threads, input callbacks, HUD animations and auto text.
fn run_frame(time: Res<Time>, hud: Option<NonSendMut<ScriptHud>>) {
    let Some(mut hud) = hud else { return };
    let game = hud.host.time.get();
    let real = time.elapsed_secs();
    hud.cvm.vm.run_frame(game);
    hud.cvm.input_frame((real * 1000.0) as u32);
    hud.cvm.hud_frame(real);
    hud.cvm.autotext_frame(game, real);
}

/// Where a panel is and how it shows, after its parents.
struct Placed {
    id: PanelId,
    rect: Rect,
    shown: bool,
}

/// Every panel under `root`, parents before children, in screen units.
fn place(ui: &Ui, root: PanelId) -> Vec<Placed> {
    fn walk(ui: &Ui, id: PanelId, origin: Vec2, shown: bool, out: &mut Vec<Placed>) {
        let Some(p) = ui.panel(id) else { return };
        let min = origin + Vec2::new(p.x, p.y);
        let shown = shown && p.visible;
        out.push(Placed {
            id,
            rect: Rect::from_corners(min, min + Vec2::new(p.w, p.h)),
            shown,
        });
        for &c in &p.children {
            walk(ui, c, min, shown, out);
        }
    }
    let mut out = Vec::new();
    walk(ui, root, Vec2::ZERO, true, &mut out);
    out
}

fn rgba(c: [u8; 4], alpha: f32) -> Color {
    Color::srgba_u8(c[0], c[1], c[2], (c[3] as f32 * alpha) as u8)
}

/// A label's text as drawn: a `#token` localised with its arguments, key
/// names for `%command%`, upper case if `allcaps`.
fn label_text(p: &pf_vgui::Panel, binds: &HashMap<String, String>) -> String {
    let mut text = if p.text.starts_with('#') {
        hud::localise(&p.text)
    } else {
        p.text.clone()
    };
    for (i, arg) in p.text_args.iter().enumerate() {
        let arg = if arg.starts_with('#') {
            hud::localise(arg)
        } else {
            arg.clone()
        };
        text = text.replace(&format!("%s{}", i + 1), &arg);
    }
    let text = hud::bind_text(&text, binds);
    if p.settings.text("allcaps").is_some_and(|v| v.trim() == "1") {
        text.to_uppercase()
    } else {
        text
    }
}

fn node_at(rect: Rect) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(rect.min.x * PIXELS_PER_UNIT),
        top: Val::Px(rect.min.y * PIXELS_PER_UNIT),
        width: Val::Px(rect.width() * PIXELS_PER_UNIT),
        height: Val::Px(rect.height() * PIXELS_PER_UNIT),
        ..default()
    }
}

/// Draw the helmet screen's panels: make UI nodes for new ones, then copy
/// each panel's place, visibility, colour, text and bar value onto them.
#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    hud: Option<NonSendMut<ScriptHud>>,
    layers: Option<Res<HelmetLayers>>,
    mut images: ResMut<Assets<Image>>,
    mut bars: ResMut<Assets<CircularBar>>,
    mut nodes: Query<&mut Node>,
    mut image_nodes: Query<&mut ImageNode>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
    bar_materials: Query<&MaterialNode<CircularBar>>,
) {
    let (Some(mut hud), Some(layers)) = (hud, layers) else {
        return;
    };
    let hud = &mut *hud;
    let Some(shared) = hud.cvm.hud.clone() else {
        return;
    };
    let state = shared.borrow();
    let ui = &state.ui;
    let Some(root) = ui
        .screens
        .iter()
        // The scripts' own copy (`Create_PilotHud`) is the newest.
        .rev()
        .find(|s| s.name.eq_ignore_ascii_case(PILOT_SCREEN))
        .map(|s| s.root)
    else {
        return;
    };
    let all = place(ui, root);
    if std::env::var_os("PF_SCRIPT_HUD_DEBUG").is_some() && !hud.drawn.is_empty() && !hud.debugged {
        hud.debugged = true;
        for pl in &all {
            if let Some(p) = ui.panel(pl.id) {
                info!(
                    "panel {} {:?} shown={} rect={:?} drawn={}",
                    p.name,
                    p.control,
                    pl.shown,
                    pl.rect,
                    hud.drawn.contains_key(&pl.id)
                );
            }
        }
    }
    for placed in all {
        let Some(p) = ui.panel(placed.id) else {
            continue;
        };
        let alpha = p.panel_alpha / 255.0;
        // The image this panel draws with: an image panel's, a bar's fill.
        let image_name = match p.control {
            Control::ImagePanel => p.image.clone(),
            Control::ProgressBar => p.settings.text("fg_image").map(str::to_string),
            _ => None,
        };
        // Remake it if its image changed (the layer may differ).
        if let Some(d) = hud.drawn.get(&placed.id)
            && d.made_with != image_name
        {
            commands.entity(d.main).despawn();
            if let Some(i) = d.inner {
                commands.entity(i).despawn();
            }
            hud.drawn.remove(&placed.id);
        }
        if !hud.drawn.contains_key(&placed.id) {
            let Some(d) = make(
                &mut commands,
                hud,
                &layers,
                &mut images,
                &mut bars,
                p,
                image_name.clone(),
            ) else {
                continue;
            };
            hud.drawn.insert(placed.id, d);
            // Filled in from next frame, once the nodes exist.
            continue;
        }
        let d = &hud.drawn[&placed.id];
        let display = if placed.shown {
            Display::Flex
        } else {
            Display::None
        };
        let base = node_at(placed.rect);
        if let Ok(mut node) = nodes.get_mut(d.main) {
            node.left = base.left;
            node.top = base.top;
            node.width = base.width;
            node.height = base.height;
            node.display = display;
        }
        match p.control {
            Control::ImagePanel => {
                if let Ok(mut i) = image_nodes.get_mut(d.main) {
                    i.color = rgba(p.fg_color, alpha);
                }
            }
            Control::Label => {
                if let Some(inner) = d.inner
                    && let Ok((mut text, mut color)) = texts.get_mut(inner)
                {
                    let t = label_text(p, &hud.binds);
                    if text.0 != t {
                        text.0 = t;
                    }
                    color.0 = rgba(p.fg_color, alpha);
                }
            }
            Control::ProgressBar => {
                if let Ok(mut i) = image_nodes.get_mut(d.main) {
                    i.color = rgba(p.bg_color, alpha);
                }
                let Some(fill) = d.inner else { continue };
                let progress = p.bar_value(hud.host.time.get());
                let circular = bar_materials.get(fill).ok().map(|m| m.0.clone());
                if let Ok(mut node) = nodes.get_mut(fill) {
                    node.left = base.left;
                    node.top = base.top;
                    node.height = base.height;
                    node.display = display;
                    // A straight bar's fill is cut to the value; a circular
                    // one cuts itself in its shader.
                    node.width = if circular.is_some() {
                        base.width
                    } else {
                        Val::Px(placed.rect.width() * progress * PIXELS_PER_UNIT)
                    };
                }
                if let Some(m) = circular {
                    if let Some(mut bar) = bars.get_mut(&m) {
                        bar.set_progress(progress);
                        bar.color = rgba(p.fg_color, alpha).into();
                    }
                } else if let Ok(mut i) = image_nodes.get_mut(fill) {
                    // That much of its image, from the left.
                    i.color = rgba(p.fg_color, alpha);
                    if let Some(size) = images.get(&i.image).map(|im| im.size_f32()) {
                        i.rect = Some(Rect::new(0.0, 0.0, size.x * progress, size.y));
                    }
                }
            }
            _ => {}
        }
    }
}

/// A HUD texture by its `vgui/` name, loaded once.
fn image(
    hud: &mut ScriptHud,
    images: &mut Assets<Image>,
    name: &str,
) -> Option<(Handle<Image>, bool)> {
    hud.images
        .entry(name.to_ascii_lowercase())
        .or_insert_with(|| {
            let loaded = hud::hud_image(images, name);
            if loaded.is_none() {
                warn!("script HUD: can't load vgui/{name}");
            }
            loaded
        })
        .clone()
}

/// Make the UI nodes for a panel; None for a panel that draws nothing
/// itself (containers, and kinds not drawn yet: the minimap, damage
/// indicators).
fn make(
    commands: &mut Commands,
    hud: &mut ScriptHud,
    layers: &HelmetLayers,
    images: &mut Assets<Image>,
    bars: &mut Assets<CircularBar>,
    p: &pf_vgui::Panel,
    image_name: Option<String>,
) -> Option<Drawn> {
    let layer = |additive: bool| {
        if additive {
            layers.additive
        } else {
            layers.normal
        }
    };
    let hidden = Node {
        position_type: PositionType::Absolute,
        display: Display::None,
        ..default()
    };
    let z = ZIndex(p.zpos);
    match p.control {
        Control::ImagePanel => {
            let (handle, additive) = image(hud, images, image_name.as_deref()?)?;
            let main = commands
                .spawn((hidden, z, ImageNode::new(handle), ChildOf(layer(additive))))
                .id();
            Some(Drawn {
                main,
                inner: None,
                made_with: image_name,
            })
        }
        Control::Label => {
            let font_name = p
                .font
                .clone()
                .or_else(|| p.settings.text("font").map(str::to_string))
                .map(|f| f.to_ascii_lowercase());
            let font = font_name.as_ref().and_then(|f| hud.fonts.get(f));
            let tall = font.map_or(12.0, |f| f.tall);
            let (across, down) = hud::text_alignment(p.settings.text("textAlignment"));
            let main = commands
                .spawn((
                    Node {
                        justify_content: across,
                        align_items: down,
                        overflow: Overflow::visible(),
                        ..hidden
                    },
                    z,
                    ChildOf(layers.normal),
                ))
                .id();
            let inset = |key: &str| {
                p.settings
                    .text(key)
                    .and_then(|v| v.trim().parse::<f32>().ok())
                    .unwrap_or(0.0)
                    * PIXELS_PER_UNIT
            };
            let inset_x = match across {
                JustifyContent::FlexStart => inset("textinsetx"),
                JustifyContent::FlexEnd => -inset("textinsetx"),
                _ => 0.0,
            };
            let right = across == JustifyContent::FlexEnd;
            let mut text = commands.spawn((
                Node {
                    left: Val::Px(inset_x),
                    top: Val::Px(inset("textinsety")),
                    ..default()
                },
                Text::new(String::new()),
                TextColor(Color::NONE),
                TextLayout::justify(if right { Justify::Right } else { Justify::Left })
                    .with_no_wrap(),
                ChildOf(main),
            ));
            if let Some(f) = font {
                text.insert(TextFont {
                    font: FontSource::Handle(f.handle.clone()),
                    font_size: FontSize::Px(tall * PIXELS_PER_UNIT),
                    ..default()
                });
            }
            let inner = text.id();
            Some(Drawn {
                main,
                inner: Some(inner),
                made_with: image_name,
            })
        }
        Control::ProgressBar => {
            let bg = p
                .settings
                .text("bg_image")
                .map(str::to_string)
                .and_then(|n| image(hud, images, &n));
            let fg = image_name.as_deref().and_then(|n| image(hud, images, n));
            let main = match bg {
                Some((bg, additive)) => commands
                    .spawn((
                        hidden.clone(),
                        z,
                        ImageNode::new(bg),
                        ChildOf(layer(additive)),
                    ))
                    .id(),
                None => commands
                    .spawn((hidden.clone(), z, ChildOf(layers.normal)))
                    .id(),
            };
            let setting = |key: &str| p.settings.text(key).is_some_and(|v| v.trim() == "1");
            let inner = fg.map(|(fg, additive)| {
                let mut fill =
                    commands.spawn((hidden, ZIndex(p.zpos + 1), ChildOf(layer(additive))));
                if setting("CircularEnabled") {
                    let bar = CircularBar::new(fg, Color::WHITE, setting("CircularClockwise"), 0.0);
                    fill.insert(MaterialNode(bars.add(bar)));
                } else {
                    fill.insert(ImageNode::new(fg));
                }
                fill.id()
            });
            Some(Drawn {
                main,
                inner,
                made_with: image_name,
            })
        }
        _ => None,
    }
}
