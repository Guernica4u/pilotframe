//! The game's VGUI panels as client script drives them (docs/port/hud.md).
//!
//! A *screen* (`CreateClientsideVGuiScreen( "vgui_fullscreen_pilot", ... )`)
//! is a panel tree built from the game's files:
//! - `scripts/vgui_screens.txt` names each screen's `.res` file and its size
//!   in pixels;
//! - the `.res` file (with its `#base` files merged under it) lists the
//!   panels: their control (`ImagePanel`, `Label`, `CHudProgressBar`…),
//!   place, size, colour, image, text.
//!
//! Scripts then change panels through HUD elements (pf-clientvm); the game
//! draws them (pf-game). This crate only holds the state.
//!
//! Layer 0: no Bevy, no file access (files come through a closure).

use std::collections::HashMap;

pub mod anim;

use pf_fmt_ui::kv::{self, Node};
use pf_fmt_ui::layout;

pub type PanelId = usize;

/// The VGUI control class a panel was made as (`ControlName`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Control {
    /// A plain container (a screen's root, or no `ControlName`).
    Panel,
    ImagePanel,
    Label,
    ProgressBar,
    MapOverview,
    NestedPanel,
    DamageIndicator,
    Other(String),
}

impl Control {
    fn from_name(name: Option<&str>) -> Control {
        match name.map(str::to_ascii_lowercase).as_deref() {
            None | Some("panel") | Some("editablepanel") => Control::Panel,
            Some("imagepanel") => Control::ImagePanel,
            Some("label") => Control::Label,
            Some("chudprogressbar") => Control::ProgressBar,
            Some("cmapoverview") => Control::MapOverview,
            Some("cnestedpanel") => Control::NestedPanel,
            Some("chuddamageindicator") => Control::DamageIndicator,
            Some(other) => Control::Other(other.to_string()),
        }
    }
}

/// RGBA, 0–255.
pub type Rgba = [u8; 4];

#[derive(Clone, Debug)]
pub struct Panel {
    pub name: String,
    pub control: Control,
    pub parent: Option<PanelId>,
    pub children: Vec<PanelId>,
    /// Its block from the `.res` file.
    pub settings: Node,
    /// Position and size in the parent's units (a HUD screen is 852 × 480).
    /// The position is the panel's own (`GetPos`/`SetPos`, vgui2.dll VPanel
    /// +0x4c): for a panel pinned to a sibling it's the offset from the
    /// pin; where it ends up is [`Ui::solve`]'s answer.
    pub x: f32,
    pub y: f32,
    /// `pin_to_sibling`: the sibling's name, our corner, the sibling's
    /// corner (VPanel +0x80, +0x88, +0x89).
    pub pin: Option<Pin>,
    pub w: f32,
    pub h: f32,
    pub zpos: i32,
    pub visible: bool,
    pub enabled: bool,
    /// Foreground colour: a label's text, an image panel's draw colour.
    pub fg_color: Rgba,
    pub bg_color: Rgba,
    /// The panel's own alpha (`SetPanelAlpha`), 0–255.
    pub panel_alpha: f32,
    pub text: String,
    /// Arguments for a localized text's `%s1`..`%s5`.
    pub text_args: Vec<String>,
    pub image: Option<String>,
    pub font: Option<String>,
    pub scale: [f32; 2],
    pub rotation: f32,
    /// Progress bars: the value set by script, and how it moves on its own
    /// (client.dll 0x18041b8c0, 0x18041b930): `rate` per second from
    /// `start` until `end`, on game time ([`Panel::bar_value`]).
    pub bar_progress: f32,
    pub bar_rate: f32,
    pub bar_start: f32,
    pub bar_end: f32,
    /// Which way the bar fills (`SetBarProgressDirection`): 0 left to
    /// right, 1 right to left, 2 bottom to top, 3 top to bottom.
    pub bar_direction: i32,
    /// `SetBarSegmentInfo( gap, size )`: a segmented bar's gap and segment
    /// size (stored; segmented painting isn't ported yet).
    pub bar_segments: Option<(i32, i32)>,
    /// Spherical warp on (`WarpEnable`, client.dll +0x104 bit 0x10).
    pub warp: bool,
    /// Progress bars: where the value comes from (`SetBarProgressSource*`,
    /// `SetBarProgressRemap`).
    pub bar: BarSource,
    /// The HUD element wrapping it, if a script made one.
    pub element: Option<usize>,
}

impl Panel {
    /// A progress bar's value at game time `now` (client.dll 0x18041cc80):
    /// the script's value moved by its rate between start and end, then
    /// remapped, clamped to 0..1 on both sides. Other sources read the
    /// value the client last read from the game (`BarSource::value`).
    pub fn bar_value(&self, now: f32) -> f32 {
        self.bar_value_from(now, self.bar.value)
    }

    /// [`Panel::bar_value`] with the source's value given.
    pub fn bar_value_from(&self, now: f32, source_value: f32) -> f32 {
        let raw = if self.bar.source == 0 {
            let t = now.max(self.bar_start).min(self.bar_end);
            (t - self.bar_start) * self.bar_rate + self.bar_progress
        } else {
            source_value
        };
        // The remap as client.dll stores it (0x18041bbd0): in-low, 1 / in
        // range, out-low, out range; an empty in range gives out-low.
        let (in_lo, in_scale, out_lo, out_range) = match self.bar.remap {
            Some([a, b, c, _]) if a == b => (0.0, 1.0, c, 0.0),
            Some([a, b, c, d]) => (a, 1.0 / (b - a), c, d - c),
            None => (0.0, 1.0, 0.0, 1.0),
        };
        let v = ((raw - in_lo) * in_scale).clamp(0.0, 1.0);
        (v * out_range + out_lo).clamp(0.0, 1.0)
    }
}

/// A panel pinned to a sibling (`pin_to_sibling`, `pin_corner_to_sibling`,
/// `pin_to_sibling_corner`). Corners: 0 top-left, 1 top-right,
/// 2 bottom-left, 3 bottom-right, 4 top centre, 5 right centre, 6 bottom
/// centre, 7 left centre, 8 centre.
#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    pub sibling: String,
    pub corner: u8,
    pub sibling_corner: u8,
}

/// A corner's place across and down a rectangle (vgui2.dll's table at
/// 0x1800c1ee0); out of range reads as the top-left.
fn corner_fraction(corner: u8) -> (f32, f32) {
    match corner {
        1 => (1.0, 0.0),
        2 => (0.0, 1.0),
        3 => (1.0, 1.0),
        4 => (0.5, 0.0),
        5 => (1.0, 0.5),
        6 => (0.5, 1.0),
        7 => (0.0, 0.5),
        8 => (0.5, 0.5),
        _ => (0.0, 0.0),
    }
}

/// A progress bar's source: PROGRESS_SOURCE_* (0 = set by script), the
/// entity it reads, and the remap (inLo, inHi, outLo, outHi).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BarSource {
    pub source: i32,
    pub source_entity: Option<u32>,
    pub remap: Option<[f32; 4]>,
    /// The source's value (PROGRESS_SOURCE_* other than 0), read from the
    /// game by the client each frame (pf-clientvm `bar_source_value`).
    pub value: f32,
}

/// A screen made from `vgui_screens.txt`.
#[derive(Clone, Debug)]
pub struct Screen {
    pub name: String,
    pub root: PanelId,
    pub pixels_wide: f32,
    pub pixels_high: f32,
}

/// Every panel the client has.
#[derive(Default)]
pub struct Ui {
    pub panels: Vec<Option<Panel>>,
    pub screens: Vec<Screen>,
    /// The HUD's own root panel (client.dll's `CHudScriptElements`), in
    /// screen pixels: the parent of the elements `Hud.HudElement` finds.
    pub hud_root: Option<PanelId>,
    /// Scheme colours by name (`resource/hudscriptedscheme.res`).
    pub scheme_colors: HashMap<String, Rgba>,
    /// The scheme's `InheritableProperties`: templates panels name with
    /// `InheritProperties`.
    templates: Option<Node>,
    defs: Option<Node>,
}

/// A file from the install as text.
pub type ReadFile<'a> = &'a dyn Fn(&str) -> Option<String>;

impl Ui {
    /// An empty UI with the screen definitions and scheme colours loaded.
    pub fn new(read: ReadFile) -> Ui {
        let defs =
            read("scripts/vgui_screens.txt").and_then(|t| kv::parse(&t).nodes.into_iter().next());
        let mut scheme_colors = HashMap::new();
        let scheme = read("resource/hudscriptedscheme.res")
            .and_then(|t| kv::parse(&t).nodes.into_iter().next());
        let templates = scheme
            .as_ref()
            .and_then(|s| s.get("InheritableProperties"))
            .cloned();
        if let Some(scheme) = scheme
            && let Some(colors) = scheme.get("Colors")
        {
            for c in colors.children() {
                if let kv::Value::Text(t) = &c.value
                    && let Some(rgba) = parse_rgba(t)
                {
                    scheme_colors.insert(c.key.to_ascii_lowercase(), rgba);
                }
            }
        }
        Ui {
            panels: Vec::new(),
            screens: Vec::new(),
            hud_root: None,
            scheme_colors,
            templates,
            defs,
        }
    }

    pub fn panel(&self, id: PanelId) -> Option<&Panel> {
        self.panels.get(id).and_then(Option::as_ref)
    }

    pub fn panel_mut(&mut self, id: PanelId) -> Option<&mut Panel> {
        self.panels.get_mut(id).and_then(Option::as_mut)
    }

    /// A colour value: `"r g b a"` or a scheme colour name.
    pub fn color(&self, v: Option<&str>, default: Rgba) -> Rgba {
        let Some(v) = v else { return default };
        parse_rgba(v)
            .or_else(|| {
                self.scheme_colors
                    .get(&v.trim().to_ascii_lowercase())
                    .copied()
            })
            .unwrap_or(default)
    }

    /// Build a screen by its `vgui_screens.txt` name. None if there's no such
    /// screen or its `.res` file is missing.
    pub fn create_screen(&mut self, name: &str, read: ReadFile) -> Option<usize> {
        let def = self.defs.as_ref()?.get(name)?.clone();
        let res = def.text("resfile")?.to_string();
        let pw: f32 = def
            .text("pixelswide")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0.0);
        let ph: f32 = def
            .text("pixelshigh")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0.0);
        let layout_root = load_res(&res, read)?;
        let root = self.add(Panel::new(name, Control::Panel, None, Node::block(name)));
        if let Some(p) = self.panel_mut(root) {
            (p.w, p.h) = (pw, ph);
        }
        self.add_children(root, &layout_root, (pw, ph), 1.0, read);
        self.screens.push(Screen {
            name: name.to_string(),
            root,
            pixels_wide: pw,
            pixels_high: ph,
        });
        Some(self.screens.len() - 1)
    }

    /// Build the HUD's root panel from its layout file (client.dll
    /// 0x18041eb90 loads `resource/UI/HudScripted_mp.res`, `_sp` or, in the
    /// optimized lobby, `_lobby`), sized to the screen in pixels. The
    /// panel is proportional: the file's numbers are in units of a
    /// 480-tall screen. None if the file is missing.
    pub fn create_hud(&mut self, res: &str, screen: (f32, f32), read: ReadFile) -> Option<PanelId> {
        let layout_root = load_res(res, read)?;
        let root = self.add(Panel::new(
            "CHudScriptElements",
            Control::Panel,
            None,
            Node::block("CHudScriptElements"),
        ));
        if let Some(p) = self.panel_mut(root) {
            (p.w, p.h) = screen;
        }
        self.add_children(root, &layout_root, screen, screen.1 / 480.0, read);
        self.hud_root = Some(root);
        Some(root)
    }

    /// A plain panel for tests.
    #[cfg(test)]
    pub(crate) fn add_test_panel(&mut self) -> PanelId {
        self.add(Panel::new(
            "test",
            Control::Panel,
            None,
            Node::block("test"),
        ))
    }

    fn add(&mut self, p: Panel) -> PanelId {
        self.panels.push(Some(p));
        self.panels.len() - 1
    }

    /// The panels of a layout block become children of `parent`, placed by
    /// the game's layout rules on a parent of `size`.
    ///
    /// A nested panel (`CNestedPanel`) gets its own children from its
    /// `controlSettingsFile`, laid out on its own size.
    fn add_children(
        &mut self,
        parent: PanelId,
        block: &Node,
        size: (f32, f32),
        scale: f32,
        read: ReadFile,
    ) {
        let block = &self.inherit(block);
        let rects = layout::layout_proportional(block, size, scale, |_| None);
        for n in block.children().iter().filter(|n| !n.children().is_empty()) {
            let control = Control::from_name(n.text("ControlName"));
            let mut p = Panel::new(&n.key, control, Some(parent), n.clone());
            if let Some(r) = rects.get(&n.key.to_ascii_lowercase()) {
                (p.x, p.y, p.w, p.h) = (r.x, r.y, r.w, r.h);
            }
            // A pinned panel keeps its offset from the pin; `solve` places
            // it (the layout file's numbers, scaled like any other).
            if let Some(sibling) = n.text("pin_to_sibling") {
                let num = |k: &str| {
                    n.text(k)
                        .and_then(|v| v.trim().parse::<f32>().ok())
                        .unwrap_or(0.0)
                };
                let corner = |k: &str| {
                    n.text(k)
                        .and_then(|v| v.trim().parse::<i32>().ok())
                        .unwrap_or(0) as u8
                };
                p.x = num("xpos") * scale;
                p.y = num("ypos") * scale;
                p.pin = Some(Pin {
                    sibling: sibling.to_string(),
                    corner: corner("pin_corner_to_sibling"),
                    sibling_corner: corner("pin_to_sibling_corner"),
                });
            }
            self.apply_settings(&mut p);
            let nested = (p.control == Control::NestedPanel)
                .then(|| {
                    n.text("controlSettingsFile")
                        .and_then(|f| load_res(f, read))
                })
                .flatten();
            let own_size = (p.w, p.h);
            let id = self.add(p);
            if let Some(par) = self.panel_mut(parent) {
                par.children.push(id);
            }
            if let Some(file) = nested {
                self.add_children(id, &file, own_size, scale, read);
            }
        }
    }

    /// The block with each panel's `InheritProperties` template applied
    /// (client.dll 0x1806894b0): every key of the template the panel
    /// doesn't have is copied in; the panel's own keys win whole.
    fn inherit(&self, block: &Node) -> Node {
        let mut out = block.clone();
        let (Some(templates), kv::Value::Block(items)) = (&self.templates, &mut out.value) else {
            return out;
        };
        for n in items.iter_mut() {
            let Some(t) = n
                .text("InheritProperties")
                .and_then(|name| templates.get(name))
            else {
                continue;
            };
            if let kv::Value::Block(keys) = &mut n.value {
                for k in t.children() {
                    if !keys.iter().any(|m| m.key.eq_ignore_ascii_case(&k.key)) {
                        keys.push(k.clone());
                    }
                }
            }
        }
        out
    }

    /// The `.res` keys every panel kind reads.
    fn apply_settings(&self, p: &mut Panel) {
        let s = &p.settings;
        let flag = |k: &str, d: bool| s.text(k).map_or(d, |v| v.trim() != "0");
        p.visible = flag("visible", true);
        p.enabled = flag("enabled", true);
        p.zpos = s
            .text("zpos")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0);
        let white = [255, 255, 255, 255];
        p.fg_color = match p.control {
            Control::ImagePanel => self.color(s.text("drawColor"), white),
            _ => self.color(s.text("fgcolor_override").or(s.text("FgColor")), white),
        };
        p.bg_color = self.color(
            s.text("bgcolor_override").or(s.text("BgColor")),
            [0, 0, 0, 0],
        );
        p.text = s.text("labelText").unwrap_or("").to_string();
        p.image = s.text("image").map(str::to_string);
        p.font = s.text("font").map(str::to_string);
    }

    /// A child of `parent` by name (case-insensitive), searching only its
    /// direct children (as `HudElement` does).
    /// Where every panel is, relative to its parent, as vgui2.dll's
    /// `VPanel::Solve` (0x1800358e0) places it each frame: a panel's own
    /// position, or for a pinned one, the sibling's corner less our corner
    /// (each side truncated to whole pixels) plus the offset, which points
    /// left from a left corner (0, 2, 7) and up from a top one (0, 1, 4).
    /// Indexed by `PanelId`.
    ///
    /// The game solves panels in tree order, so a panel pinned to a later
    /// sibling uses that sibling's place from the frame before; this solves
    /// the sibling first, which gives the same places once nothing moves.
    pub fn solve(&self) -> Vec<(f32, f32)> {
        let mut out: Vec<Option<(f32, f32)>> = vec![None; self.panels.len()];
        let mut busy = vec![false; self.panels.len()];
        for id in 0..self.panels.len() {
            self.solve_one(id, &mut out, &mut busy);
        }
        out.into_iter().map(Option::unwrap_or_default).collect()
    }

    fn solve_one(
        &self,
        id: PanelId,
        out: &mut [Option<(f32, f32)>],
        busy: &mut [bool],
    ) -> (f32, f32) {
        if let Some(v) = out[id] {
            return v;
        }
        let Some(p) = self.panel(id) else {
            return (0.0, 0.0);
        };
        let own = (p.x.trunc(), p.y.trunc());
        let sibling = p.pin.as_ref().and_then(|pin| {
            let parent = p.parent?;
            let s = self.find_child(parent, &pin.sibling)?;
            (s != id).then_some((pin, s))
        });
        let place = match sibling {
            // A pin cycle: the cycle's first panel keeps its own position.
            Some(_) if busy[id] => own,
            Some((pin, s)) => {
                busy[id] = true;
                let (sx, sy) = self.solve_one(s, out, busy);
                busy[id] = false;
                let sib = self
                    .panel(s)
                    .map_or((0.0, 0.0), |q| (q.w.trunc(), q.h.trunc()));
                let (fx, fy) = corner_fraction(pin.sibling_corner);
                let (mx, my) = corner_fraction(pin.corner);
                let out_x = if matches!(pin.sibling_corner, 0 | 2 | 7) {
                    -1.0
                } else {
                    1.0
                };
                let out_y = if matches!(pin.sibling_corner, 0 | 1 | 4) {
                    -1.0
                } else {
                    1.0
                };
                let x = ((sib.0 * fx + sx) as i32 as f32 - p.w.trunc() * mx) as i32 as f32;
                let y = ((sib.1 * fy + sy) as i32 as f32 - p.h.trunc() * my) as i32 as f32;
                (own.0 * out_x + x, own.1 * out_y + y)
            }
            None => own,
        };
        out[id] = Some(place);
        place
    }

    pub fn find_child(&self, parent: PanelId, name: &str) -> Option<PanelId> {
        self.panel(parent)?.children.iter().copied().find(|&c| {
            self.panel(c)
                .is_some_and(|p| p.name.eq_ignore_ascii_case(name))
        })
    }
}

impl Panel {
    fn new(name: &str, control: Control, parent: Option<PanelId>, settings: Node) -> Panel {
        Panel {
            name: name.to_string(),
            control,
            parent,
            children: Vec::new(),
            settings,
            x: 0.0,
            y: 0.0,
            pin: None,
            w: 0.0,
            h: 0.0,
            zpos: 0,
            visible: true,
            enabled: true,
            fg_color: [255, 255, 255, 255],
            bg_color: [0, 0, 0, 0],
            panel_alpha: 255.0,
            text: String::new(),
            text_args: Vec::new(),
            image: None,
            font: None,
            scale: [1.0, 1.0],
            rotation: 0.0,
            bar_progress: 0.0,
            bar_rate: 0.0,
            bar_start: 0.0,
            bar_end: 0.0,
            bar_direction: 0,
            bar_segments: None,
            warp: false,
            bar: BarSource::default(),
            element: None,
        }
    }
}

trait BlockExt {
    fn block(name: &str) -> Node;
}

impl BlockExt for Node {
    fn block(name: &str) -> Node {
        Node {
            key: name.to_string(),
            value: kv::Value::Block(Vec::new()),
        }
    }
}

/// `"r g b a"` → RGBA (alpha 255 if left out).
pub fn parse_rgba(v: &str) -> Option<Rgba> {
    let n: Vec<u8> = v
        .split_whitespace()
        .map(|p| p.parse::<f32>().ok().map(|f| f.clamp(0.0, 255.0) as u8))
        .collect::<Option<Vec<_>>>()?;
    match n.as_slice() {
        [r, g, b] => Some([*r, *g, *b, 255]),
        [r, g, b, a] => Some([*r, *g, *b, *a]),
        _ => None,
    }
}

/// A `.res` file's root block with its `#base` files merged under it (bases
/// are relative to the file's folder).
pub fn load_res(path: &str, read: ReadFile) -> Option<Node> {
    let file = kv::parse(&read(path)?);
    let mut root = file.nodes.into_iter().next()?;
    let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
    for base in file.bases.iter().rev() {
        let base_path = if dir.is_empty() {
            base.clone()
        } else {
            format!("{dir}/{base}")
        };
        if let Some(mut b) = load_res(&base_path, read) {
            b.merge(&root);
            b.key = root.key.clone();
            root = b;
        }
    }
    Some(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(path: &str) -> Option<String> {
        Some(
            match path {
                "scripts/vgui_screens.txt" => {
                    r#""VGUI_Screens" { "hud" { "type" "vgui_screen_panel" "pixelswide" 852 "pixelshigh" 480 "resfile" "scripts/screens/hud.res" } }"#
                }
                "scripts/screens/base.res" => {
                    "b { Screen { ControlName ImagePanel xpos 0 ypos 0 wide 852 tall 480 drawColor \"0 0 0 0\" } }"
                }
                "scripts/screens/hud.res" => {
                    "#base \"base.res\"\nh { Health { ControlName Label xpos 10 ypos r30 wide 100 tall 20 labelText \"#HP\" fgcolor_override Friendly visible 0 } }"
                }
                "resource/hudscriptedscheme.res" => {
                    r#"Scheme { Colors { Friendly "70 150 230 255" } }"#
                }
                _ => return None,
            }
            .to_string(),
        )
    }

    #[test]
    fn builds_a_screen_from_its_res_and_base() {
        let mut ui = Ui::new(&files);
        let s = ui.create_screen("hud", &files).unwrap();
        let root = ui.screens[s].root;
        assert_eq!(ui.panel(root).unwrap().children.len(), 2);
        let hp = ui.find_child(root, "health").unwrap();
        let p = ui.panel(hp).unwrap();
        assert_eq!(p.control, Control::Label);
        assert_eq!((p.x, p.y, p.w), (10.0, 450.0, 100.0));
        assert_eq!(p.fg_color, [70, 150, 230, 255]);
        assert!(!p.visible);
        assert_eq!(p.text, "#HP");
        let screen = ui.find_child(root, "Screen").unwrap();
        assert_eq!(ui.panel(screen).unwrap().fg_color, [0, 0, 0, 0]);
    }

    #[test]
    fn pinned_panels_follow_their_sibling_each_solve() {
        let pin = |sibling: &str, corner, sibling_corner| {
            Some(Pin {
                sibling: sibling.into(),
                corner,
                sibling_corner,
            })
        };
        let mut ui = Ui::default();
        let root = ui.add_test_panel();
        let mut child = |name: &str, x, y, w, h, pin: Option<Pin>| {
            let mut p = Panel::new(name, Control::Panel, Some(root), Node::block(name));
            (p.x, p.y, p.w, p.h, p.pin) = (x, y, w, h, pin);
            let id = ui.add(p);
            ui.panels[root].as_mut().unwrap().children.push(id);
            id
        };
        let safe = child("Safe", 61.0, 40.0, 730.0, 400.0, None);
        // Bottom-right to bottom-right, 20 in and 8 down from it.
        let right = child("Right", 20.0, -8.0, 72.0, 66.0, pin("Safe", 3, 3));
        // Bottom-left to bottom-left: the offset points left and down.
        let left = child("Left", 20.0, -8.0, 72.0, 66.0, pin("Safe", 2, 2));
        // Pinned to a pinned panel: its top-right on the other's top-left.
        let side = child("Side", 0.0, 0.0, 108.0, 66.0, pin("Right", 1, 0));
        let at = ui.solve();
        assert_eq!(
            at[right],
            (61.0 + 730.0 - 72.0 + 20.0, 40.0 + 400.0 - 66.0 - 8.0)
        );
        assert_eq!(at[left], (61.0 - 20.0, 40.0 + 400.0 - 66.0 - 8.0));
        assert_eq!(at[side], (at[right].0 - 108.0, at[right].1));
        // The script grows the safe area: everything pinned moves with it.
        ui.panels[safe].as_mut().unwrap().w = 766.0;
        let at = ui.solve();
        assert_eq!(at[right].0, 61.0 + 766.0 - 72.0 + 20.0);
        assert_eq!(at[side].0, at[right].0 - 108.0);
    }
}
