//! The VGUI animation controller (vgui's `AnimationController`, statically
//! linked in client.dll; docs/port/hud.md "Animation controller"). HUD
//! elements' `...OverTime` methods run animations on it; it moves each
//! animated panel variable from its value when the animation starts to the
//! target, along one of seven curves.
//!
//! Ported from client.dll: starting an animation (0x1806afd50 colours,
//! 0x1806afc80 numbers, 0x1806af9b0), cancelling (0x1806ae2d0), the update
//! (0x1806adf00), the curves (0x1806a9f50) and reading and writing panel
//! variables (0x1806aadc0, 0x1806ab3f0). The game's HUD controller runs on
//! real time (`Plat_FloatTime`), not game time.

use crate::{PanelId, Ui};

/// What an animation changes.
#[derive(Clone, Debug, PartialEq)]
pub enum Var {
    /// x and y together.
    Position,
    /// wide and tall together.
    Size,
    /// The foreground colour (an image panel's draw colour).
    FgColor,
    BgColor,
    XPos,
    YPos,
    Wide,
    Tall,
    /// The panel's own alpha: every panel's "alpha" animation variable
    /// (client.dll 0x18069b040 registers it as the float `m_flAlpha`).
    Alpha,
    /// Any other name: a panel setting by name (not ported yet; such
    /// animations run but change nothing).
    Other(String),
}

impl Var {
    /// The controller's names, matched ignoring case.
    pub fn from_name(name: &str) -> Var {
        match name.to_ascii_lowercase().as_str() {
            "position" => Var::Position,
            "size" => Var::Size,
            "fgcolor" => Var::FgColor,
            "bgcolor" => Var::BgColor,
            "xpos" => Var::XPos,
            "ypos" => Var::YPos,
            "wide" => Var::Wide,
            "tall" => Var::Tall,
            "alpha" => Var::Alpha,
            other => Var::Other(other.to_string()),
        }
    }
}

/// The curves (interpolators), as scripts number them (0 to 6).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    Linear,
    /// t².
    Accel,
    /// √t.
    Deaccel,
    /// (cos(2π·t·freq) + 1) / 2.
    Pulse,
    /// 1 while a random number in 0..1 is below freq, else 0.
    Flicker,
    /// (3 − 2t)·t².
    SimpleSpline,
    /// Falls, bounces up and settles.
    Bounce,
}

impl Curve {
    pub fn from_index(i: i32) -> Option<Curve> {
        Some(match i {
            0 => Curve::Linear,
            1 => Curve::Accel,
            2 => Curve::Deaccel,
            3 => Curve::Pulse,
            4 => Curve::Flicker,
            5 => Curve::SimpleSpline,
            6 => Curve::Bounce,
            _ => return None,
        })
    }
}

/// One running animation.
#[derive(Clone, Debug)]
pub struct Animation {
    pub panel: PanelId,
    pub var: Var,
    pub start_time: f32,
    pub end_time: f32,
    pub curve: Curve,
    /// The curve's parameter (pulse frequency, flicker chance; 1 for the
    /// HUD element commands).
    pub param: f32,
    /// Read from the panel when the start time comes.
    pub start: [f32; 4],
    pub started: bool,
    pub end: [f32; 4],
}

/// The controller: its clock and the running animations, in start order.
pub struct Controller {
    pub time: f32,
    pub active: Vec<Animation>,
    rng: u64,
}

impl Default for Controller {
    fn default() -> Self {
        Controller {
            time: 0.0,
            active: Vec::new(),
            rng: 0x9e37_79b9_7f4a_7c15,
        }
    }
}

impl Controller {
    /// Run an animation of `var` on `panel` to `end`: any running one of
    /// that variable on that panel is cancelled first, then it starts after
    /// `delay` and lasts `duration` seconds.
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &mut self,
        panel: PanelId,
        var: Var,
        end: [f32; 4],
        delay: f32,
        duration: f32,
        curve: Curve,
        param: f32,
    ) {
        self.cancel(panel, &var);
        let start_time = self.time + delay;
        self.active.push(Animation {
            panel,
            var,
            start_time,
            end_time: start_time + duration,
            curve,
            param,
            start: [0.0; 4],
            started: false,
            end,
        });
    }

    /// Cancel a running animation of `var` on `panel`. As in the game, only
    /// the first one found goes. (The game can also spare one sequence's
    /// animations; the HUD's calls never do.)
    pub fn cancel(&mut self, panel: PanelId, var: &Var) {
        if let Some(i) = self
            .active
            .iter()
            .position(|a| a.panel == panel && &a.var == var)
        {
            self.active.remove(i);
        }
    }

    /// Advance to `time` and apply every running animation to `ui`.
    pub fn update(&mut self, ui: &mut Ui, time: f32) {
        self.time = time;
        let mut i = 0;
        while i < self.active.len() {
            let now = self.time;
            if self.active[i].start_time > now {
                i += 1;
                continue;
            }
            if ui.panel(self.active[i].panel).is_none() {
                self.active.remove(i);
                continue;
            }
            if !self.active[i].started {
                let a = &self.active[i];
                let v = get_value(ui, a.panel, &a.var);
                let a = &mut self.active[i];
                a.start = v;
                a.started = true;
            }
            let a = self.active[i].clone();
            let value = if a.end_time <= now {
                a.end
            } else {
                let r = self.random();
                interpolate(
                    a.curve,
                    a.param,
                    now,
                    a.start_time,
                    a.end_time,
                    a.start,
                    a.end,
                    r,
                )
            };
            set_value(ui, a.panel, &a.var, value);
            if now < a.end_time {
                i += 1;
            } else {
                self.active.remove(i);
            }
        }
    }

    /// A number in 0..1 for the flicker curve (ours: the game takes it from
    /// its own random stream).
    fn random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// A value between `start` and `end` at `now` (client.dll 0x1806a9f50).
/// The fraction is a 32-bit float; the trig is done in doubles, as the game
/// does. `random` is only used by the flicker curve.
#[allow(clippy::too_many_arguments)]
pub fn interpolate(
    curve: Curve,
    param: f32,
    now: f32,
    start_time: f32,
    end_time: f32,
    start: [f32; 4],
    end: [f32; 4],
    random: f32,
) -> [f32; 4] {
    let pi = std::f64::consts::PI;
    let mut t = (now - start_time) / (end_time - start_time);
    t = match curve {
        Curve::Linear => t,
        Curve::Accel => t * t,
        Curve::Deaccel => t.sqrt(),
        Curve::Pulse => ((((t * 2.0) as f64 * pi * param as f64).cos() + 1.0) * 0.5) as f32,
        Curve::Flicker => {
            if param <= random {
                0.0
            } else {
                1.0
            }
        }
        Curve::SimpleSpline => (3.0 - t * 2.0) * t * t,
        Curve::Bounce => {
            if t < 0.33 {
                (1.0 - (t as f64 * pi * 3.030_302_909_889_611_5).sin()) as f32
            } else if t < 0.67 {
                let s = ((t - 0.33) as f64 * pi * 2.941_176_439_651_569).sin();
                (((1.0 - s) + 1.0) * 0.5) as f32
            } else {
                let s = ((t - 0.67) as f64 * pi * 3.030_303_183_556_486_4).sin();
                ((1.0 - s) * 0.200_000_002_980_232_24 + 0.800_000_011_920_929) as f32
            }
        }
    };
    let mut out = [0.0; 4];
    for k in 0..4 {
        out[k] = (end[k] - start[k]) * t + start[k];
    }
    out
}

/// A colour component as the game writes it: truncated, low 8 bits.
fn byte(v: f32) -> u8 {
    v as i64 as u8
}

/// A panel variable's current value (0x1806aadc0).
pub fn get_value(ui: &Ui, panel: PanelId, var: &Var) -> [f32; 4] {
    let Some(p) = ui.panel(panel) else {
        return [0.0; 4];
    };
    let rgba = |c: crate::Rgba| [c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32];
    match var {
        Var::Position => [p.x.trunc(), p.y.trunc(), 0.0, 0.0],
        Var::Size => [p.w.trunc(), p.h.trunc(), 0.0, 0.0],
        Var::FgColor => rgba(p.fg_color),
        Var::BgColor => rgba(p.bg_color),
        Var::XPos => [p.x.trunc(), 0.0, 0.0, 0.0],
        Var::YPos => [p.y.trunc(), 0.0, 0.0, 0.0],
        Var::Wide => [p.w.trunc(), 0.0, 0.0, 0.0],
        Var::Tall => [p.h.trunc(), 0.0, 0.0, 0.0],
        Var::Alpha => [p.panel_alpha, 0.0, 0.0, 0.0],
        Var::Other(_) => [0.0; 4],
    }
}

/// Write a panel variable (0x1806ab3f0): positions and sizes as whole
/// pixels (truncated), colours as truncated bytes.
pub fn set_value(ui: &mut Ui, panel: PanelId, var: &Var, v: [f32; 4]) {
    let Some(p) = ui.panel_mut(panel) else {
        return;
    };
    let whole = |f: f32| f as i32 as f32;
    match var {
        Var::Position => {
            p.x = whole(v[0]);
            p.y = whole(v[1]);
        }
        Var::Size => {
            p.w = whole(v[0]);
            p.h = whole(v[1]);
        }
        Var::FgColor => p.fg_color = [byte(v[0]), byte(v[1]), byte(v[2]), byte(v[3])],
        Var::BgColor => p.bg_color = [byte(v[0]), byte(v[1]), byte(v[2]), byte(v[3])],
        Var::XPos => p.x = whole(v[0]),
        Var::YPos => p.y = whole(v[0]),
        Var::Wide => p.w = whole(v[0]),
        Var::Tall => p.h = whole(v[0]),
        Var::Alpha => p.panel_alpha = v[0],
        Var::Other(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_hit_their_ends() {
        let (s, e) = ([0.0; 4], [100.0, 0.0, 0.0, 0.0]);
        let at = |c, now| interpolate(c, 1.0, now, 0.0, 1.0, s, e, 0.5)[0];
        assert_eq!(at(Curve::Linear, 0.25), 25.0);
        assert_eq!(at(Curve::Accel, 0.5), 25.0);
        assert_eq!(at(Curve::Deaccel, 0.25), 50.0);
        assert_eq!(at(Curve::SimpleSpline, 0.5), 50.0);
        assert!((at(Curve::Pulse, 0.0) - 100.0).abs() < 1e-4);
        assert!((at(Curve::Pulse, 0.5) - 0.0).abs() < 1e-3);
        assert!((at(Curve::Bounce, 0.0) - 100.0).abs() < 1e-4);
    }

    #[test]
    fn an_animation_runs_and_ends_on_its_target() {
        let mut ui = Ui::default();
        let id = ui.add_test_panel();
        let mut c = Controller::default();
        c.update(&mut ui, 10.0);
        c.run(
            id,
            Var::FgColor,
            [255.0, 255.0, 255.0, 0.0],
            0.0,
            1.0,
            Curve::Linear,
            1.0,
        );
        c.update(&mut ui, 10.5);
        assert_eq!(ui.panel(id).unwrap().fg_color, [255, 255, 255, 127]);
        c.update(&mut ui, 11.0);
        assert_eq!(ui.panel(id).unwrap().fg_color, [255, 255, 255, 0]);
        assert!(c.active.is_empty());
    }
}
