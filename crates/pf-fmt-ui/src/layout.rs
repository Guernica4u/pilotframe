//! Where VGUI panels go: each element of a `.res` layout gets a rectangle in the
//! layout's own units (a HUD screen is 852 × 480).
//!
//! The rules, as the files use them:
//! - `xpos` / `ypos`: a number, `c±n` (from the centre), `r±n` (from the right or
//!   bottom edge), or `%n` (percent of the screen).
//! - `wide` / `tall`: a number, `%n`, or `f n` (the full screen minus n).
//! - `pin_to_sibling` places one of our corners (`pin_corner_to_sibling`) on one
//!   of the sibling's (`pin_to_sibling_corner`), then moves by `xpos`/`ypos`
//!   *outward from that corner*: to the left of a left corner and above a top
//!   one, to the right/below otherwise. (The game's left and right panels use
//!   the same numbers on mirrored corners, e.g. `GameInfoBG` and `dpadIconBG`
//!   both `xpos 20`; checked against a screenshot of the real HUD.)
//!   Corners: 0 top-left, 1 top-right, 2 bottom-left, 3 bottom-right, 4 top
//!   centre, 5 right centre, 6 bottom centre, 7 left centre, 8 centre.
//! - `auto_wide_tocontents`: as wide as its text, which the caller measures.
//! - A *proportional* panel (the HUD's own root) has its numbers in units of
//!   a 480-tall screen: every plain number (not a percentage) is scaled by
//!   screen height / 480 (`layout_proportional`).

use crate::kv::Node;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    /// A corner (0..=8) of this rectangle.
    pub fn corner(&self, corner: u32) -> (f32, f32) {
        let (fx, fy) = corner_fraction(corner);
        (self.x + self.w * fx, self.y + self.h * fy)
    }
}

fn corner_fraction(corner: u32) -> (f32, f32) {
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

/// Lay out every element (child block) of `root` on a `screen`-sized panel.
/// `content_wide` gives an element's text width for `auto_wide_tocontents`.
pub fn layout(
    root: &Node,
    screen: (f32, f32),
    content_wide: impl Fn(&Node) -> Option<f32>,
) -> HashMap<String, Rect> {
    layout_proportional(root, screen, 1.0, content_wide)
}

/// `layout` with every plain number scaled by `scale` (a proportional
/// panel: screen height / 480).
pub fn layout_proportional(
    root: &Node,
    screen: (f32, f32),
    scale: f32,
    content_wide: impl Fn(&Node) -> Option<f32>,
) -> HashMap<String, Rect> {
    let elements: HashMap<String, &Node> = root
        .children()
        .iter()
        .filter(|n| !n.children().is_empty())
        .map(|n| (n.key.to_ascii_lowercase(), n))
        .collect();
    let mut solver = Solver {
        elements,
        screen,
        scale,
        content_wide,
        done: HashMap::new(),
        busy: Vec::new(),
    };
    let names: Vec<String> = solver.elements.keys().cloned().collect();
    for name in names {
        solver.rect(&name);
    }
    solver.done
}

struct Solver<'a, F> {
    elements: HashMap<String, &'a Node>,
    screen: (f32, f32),
    /// Plain numbers are multiplied by this.
    scale: f32,
    content_wide: F,
    done: HashMap<String, Rect>,
    /// Elements being solved right now, to stop pin cycles.
    busy: Vec<String>,
}

impl<F: Fn(&Node) -> Option<f32>> Solver<'_, F> {
    fn rect(&mut self, name: &str) -> Option<Rect> {
        let key = name.to_ascii_lowercase();
        if let Some(r) = self.done.get(&key) {
            return Some(*r);
        }
        let node = *self.elements.get(&key)?;
        if self.busy.contains(&key) {
            return None;
        }
        self.busy.push(key.clone());
        let (sw, sh) = self.screen;
        let k = self.scale;
        let mut w = size(node.text("wide"), sw, k);
        let h = size(node.text("tall"), sh, k);
        if node.text("auto_wide_tocontents") == Some("1")
            && let Some(cw) = (self.content_wide)(node)
        {
            w = cw;
        }
        let num = |k: &str| node.text(k).and_then(|v| v.trim().parse::<u32>().ok());
        let sibling = node.text("pin_to_sibling").map(str::to_string);
        let rect = match sibling.and_then(|s| self.rect(&s)) {
            Some(sib) => {
                let sib_corner = num("pin_to_sibling_corner").unwrap_or(0);
                let (px, py) = sib.corner(sib_corner);
                let (sx, sy) = corner_fraction(sib_corner);
                let out = |f: f32| if f == 0.0 { -1.0 } else { 1.0 };
                let (fx, fy) = corner_fraction(num("pin_corner_to_sibling").unwrap_or(0));
                let dx = plain(node.text("xpos")) * k;
                let dy = plain(node.text("ypos")) * k;
                Rect {
                    x: px - w * fx + dx * out(sx),
                    y: py - h * fy + dy * out(sy),
                    w,
                    h,
                }
            }
            None => Rect {
                x: position(node.text("xpos"), sw, k),
                y: position(node.text("ypos"), sh, k),
                w,
                h,
            },
        };
        self.busy.pop();
        self.done.insert(key, rect);
        Some(rect)
    }
}

/// A plain number (pin offsets), 0 if missing.
fn plain(v: Option<&str>) -> f32 {
    v.and_then(|s| s.trim().parse().ok()).unwrap_or(0.0)
}

/// A number from a layout value, 0 if it doesn't parse.
fn number(s: &str) -> f32 {
    s.trim().parse().unwrap_or(0.0)
}

fn position(v: Option<&str>, extent: f32, scale: f32) -> f32 {
    let Some(s) = v.map(str::trim) else {
        return 0.0;
    };
    if let Some(rest) = s.strip_prefix(['c', 'C']) {
        extent / 2.0 + number(rest) * scale
    } else if let Some(rest) = s.strip_prefix(['r', 'R']) {
        extent - number(rest) * scale
    } else if let Some(rest) = s.strip_prefix('%') {
        extent * number(rest) / 100.0
    } else {
        number(s) * scale
    }
}

fn size(v: Option<&str>, extent: f32, scale: f32) -> f32 {
    let Some(s) = v.map(str::trim) else {
        return 0.0;
    };
    if let Some(rest) = s.strip_prefix('%') {
        extent * number(rest) / 100.0
    } else if let Some(rest) = s.strip_prefix(['f', 'F']) {
        extent - number(rest) * scale
    } else {
        number(s) * scale
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv::parse;

    #[test]
    fn centres_pins_and_offsets() {
        let file = parse(
            r#"screen {
                Screen   { xpos 0 ypos 0 wide 852 tall 480 }
                SafeArea { wide 730 tall 400 pin_to_sibling Screen
                           pin_corner_to_sibling 8 pin_to_sibling_corner 8 }
                Icon     { xpos 20 ypos -8 wide 72 tall 66 pin_to_sibling SafeArea
                           pin_corner_to_sibling 3 pin_to_sibling_corner 3 }
                Side     { wide 108 tall 66 pin_to_sibling Icon
                           pin_corner_to_sibling 1 pin_to_sibling_corner 0 }
                Text     { wide 80 tall 14 auto_wide_tocontents 1 pin_to_sibling Side
                           pin_corner_to_sibling 3 pin_to_sibling_corner 5 }
                Centre   { xpos c-64 ypos r30 wide 128 tall 30 }
            }"#,
        );
        let r = layout(&file.nodes[0], (852.0, 480.0), |_| Some(25.0));
        let safe = r["safearea"];
        assert_eq!((safe.x, safe.y), (61.0, 40.0));
        let icon = r["icon"];
        assert_eq!((icon.x, icon.y), (791.0 - 72.0 + 20.0, 440.0 - 66.0 - 8.0));
        let side = r["side"];
        assert_eq!((side.x + side.w, side.y), (icon.x, icon.y));
        let text = r["text"];
        assert_eq!(
            (text.w, text.x + text.w, text.y + text.h),
            (25.0, icon.x, side.y + 33.0)
        );
        let c = r["centre"];
        assert_eq!((c.x, c.y), (362.0, 450.0));
    }

    #[test]
    fn offsets_point_outward_from_the_pinned_corner() {
        // The game's Offhand0Icon / Offhand1Icon: same numbers, mirrored corners.
        let file = parse(
            r#"s {
                Safe  { xpos 0 ypos 0 wide 100 tall 100 }
                Right { xpos -20 ypos -10 wide 10 tall 10 pin_to_sibling Safe
                        pin_corner_to_sibling 3 pin_to_sibling_corner 3 }
                Left  { xpos -20 ypos -10 wide 10 tall 10 pin_to_sibling Safe
                        pin_corner_to_sibling 2 pin_to_sibling_corner 2 }
            }"#,
        );
        let r = layout(&file.nodes[0], (100.0, 100.0), |_| None);
        assert_eq!((r["right"].x, r["right"].y), (70.0, 80.0));
        assert_eq!((r["left"].x, r["left"].y), (20.0, 80.0));
    }

    #[test]
    fn pin_cycles_do_not_hang() {
        let file = parse("s { A { wide 1 pin_to_sibling B } B { wide 1 pin_to_sibling A } }");
        let r = layout(&file.nodes[0], (10.0, 10.0), |_| None);
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn proportional_scales_numbers_not_percentages() {
        let file = parse(r#"r { A { xpos r10 ypos 20 wide %50 tall 40 } }"#);
        let r = layout_proportional(&file.nodes[0], (1920.0, 1080.0), 1080.0 / 480.0, |_| None);
        let a = r["a"];
        assert_eq!((a.x, a.y, a.w, a.h), (1920.0 - 22.5, 45.0, 960.0, 90.0));
    }
}
