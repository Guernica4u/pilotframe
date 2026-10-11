//! Material proxies: the small programs in a `.vmt`'s `Proxies` block that
//! change its variables every frame. The ones the gun's sight and ammo counter
//! use are ported from `client.dll` (docs/weapons.md, "The HCOG reticle" and
//! "On-gun ammo counter"); `Divide`, `UniformNoise` and `TextureTransform` are
//! Source's standard ones.
//!
//! A material keeps its variables (`$name`, up to four numbers) and its 2D
//! texture transforms; each frame the proxies run in order against what the
//! gun and player are doing ([`Context`]), and the caller reads the results
//! (`$basetexturetransform`, `$texture2transform`, `$color`, `$alpha`).

use std::collections::HashMap;

use bevy::math::{Affine2, Vec2};
use pf_fmt_ui::kv;

/// What the proxies can read about the gun.
#[derive(Clone, Copy, Debug, Default)]
pub struct Context<'a> {
    pub clip: u32,
    pub clip_size: u32,
    pub reserve: u32,
    /// `ammo_stockpile_max`.
    pub max_carry: u32,
    /// The viewmodel's holo-sight value, 0..1 (`HoloSight`).
    pub holo: f32,
    /// Each attachment's offset from the eye along its own right and up axes,
    /// by lowercase name (`HoloSightOffset`).
    pub sights: Option<&'a HashMap<String, Vec2>>,
}

/// A reference to a variable (or one of its numbers), or a literal number.
#[derive(Clone, Debug, PartialEq)]
enum Value {
    Var(String, Option<usize>),
    Literal(f32),
}

impl Value {
    fn parse(text: &str) -> Self {
        let t = text.trim();
        if let Some(name) = t.strip_prefix('$') {
            if let Some((n, rest)) = name.split_once('[') {
                let index = rest.trim_end_matches(']').trim().parse().ok();
                return Value::Var(n.to_ascii_lowercase(), index);
            }
            return Value::Var(name.to_ascii_lowercase(), None);
        }
        Value::Literal(t.parse().unwrap_or(0.0))
    }
}

#[derive(Clone, Debug)]
enum Proxy {
    ClipAmmo(Value),
    MaxClipAmmo(Value),
    RemainingAmmo(Value),
    RemainingClipCount(Value),
    MaxCarryAmmo(Value),
    ClipAmmoColor(Value),
    RemainingAmmoColor(Value),
    BackgroundAmmoColor(Value),
    HoloSight(Value),
    HoloSightOffset {
        attachment: String,
        horizontal: f32,
        vertical: f32,
        result: Value,
    },
    Divide {
        a: Value,
        b: Value,
        result: Value,
    },
    UniformNoise {
        min: f32,
        max: f32,
        result: Value,
    },
    NumericCounterAtlasFrame {
        value: Value,
        digit: Value,
        result: Value,
    },
    TickCounterAtlasFrames {
        ticks: Value,
        row_length: Value,
        row_index: Value,
        size: Value,
        start: Value,
        count: Value,
    },
    TextureAtlas {
        size: Value,
        start: Value,
        count: Option<Value>,
        result: String,
    },
    ProgressBar {
        value: Value,
        max: Value,
        result: String,
    },
    TextureTransform {
        translate: Option<Value>,
        scale: Option<Value>,
        rotate: Option<Value>,
        center: Option<Value>,
        result: String,
    },
}

/// Red, orange and blue for the ammo counter (`ClipAmmoColor` and friends).
const RED: [f32; 3] = [1.0, 0.29, 0.26];
const ORANGE: [f32; 3] = [1.0, 0.5, 0.25];
const BLUE: [f32; 3] = [0.52, 0.905, 1.0];
const BACKGROUND_WARNING: [f32; 3] = [1.0, 0.69, 0.65];

/// A material's variables and proxies.
#[derive(Clone, Debug, Default)]
pub struct ProxyMaterial {
    /// Each variable's numbers, and how many it was given (a single number
    /// stands for all of a vector, as Source reads it).
    vars: HashMap<String, [f32; 4]>,
    widths: HashMap<String, usize>,
    transforms: HashMap<String, Affine2>,
    proxies: Vec<Proxy>,
    rng: u32,
}

/// Up to four numbers from a VMT value: `"1"`, `"[1 0.5 0]"`, `"{255 0 0}"`.
/// A proxy's number setting, or `default` if it's missing.
fn number(text: Option<&str>, default: f32) -> f32 {
    text.and_then(|t| t.trim().parse().ok()).unwrap_or(default)
}

fn numbers(text: &str) -> [f32; 4] {
    numbers_and_width(text).0
}

fn numbers_and_width(text: &str) -> ([f32; 4], usize) {
    let mut out = [0.0; 4];
    let cleaned = text.trim_matches(|c| matches!(c, '[' | ']' | '{' | '}' | '"' | ' '));
    let mut width = 0;
    for (o, n) in out.iter_mut().zip(cleaned.split_whitespace()) {
        *o = n.parse().unwrap_or(0.0);
        width += 1;
    }
    (out, width)
}

impl ProxyMaterial {
    /// From the `.vmt` text: its `$variables` and the proxies the sight and
    /// counter use (others are skipped). `None` if it has no proxies we know.
    pub fn parse(vmt: &str) -> Option<Self> {
        let root = kv::parse(vmt).nodes.into_iter().next()?;
        let mut m = Self::default();
        for n in root.children() {
            if let kv::Value::Text(t) = &n.value
                && let Some(name) = n.key.strip_prefix('$')
            {
                let (n, width) = numbers_and_width(t);
                m.vars.insert(name.to_ascii_lowercase(), n);
                m.widths.insert(name.to_ascii_lowercase(), width);
            }
        }
        // Source's defaults for the colour and alpha.
        m.vars.entry("color".into()).or_insert([1.0, 1.0, 1.0, 0.0]);
        m.vars.entry("alpha".into()).or_insert([1.0, 0.0, 0.0, 0.0]);
        for p in root.get("Proxies")?.children() {
            let v = |key: &str| p.text(key).map(Value::parse);
            let r = || v("resultVar").unwrap_or(Value::Literal(0.0));
            let name = |key: &str| {
                p.text(key)
                    .map(|t| t.trim().trim_start_matches('$').to_ascii_lowercase())
                    .unwrap_or_default()
            };
            let proxy = match p.key.to_ascii_lowercase().as_str() {
                "clipammo" => Proxy::ClipAmmo(r()),
                "maxclipammo" => Proxy::MaxClipAmmo(r()),
                "remainingammo" => Proxy::RemainingAmmo(r()),
                "remainingclipcount" => Proxy::RemainingClipCount(r()),
                "maxcarryammo" => Proxy::MaxCarryAmmo(r()),
                "clipammocolor" => Proxy::ClipAmmoColor(r()),
                "remainingammocolor" => Proxy::RemainingAmmoColor(r()),
                "backgroundammocolor" => Proxy::BackgroundAmmoColor(r()),
                "holosight" => Proxy::HoloSight(r()),
                // Defaults from client.dll.
                "holosightoffset" => Proxy::HoloSightOffset {
                    attachment: name("attachmentName"),
                    horizontal: number(p.text("horizontalScalar"), 0.5),
                    vertical: number(p.text("verticalScalar"), -0.5),
                    result: r(),
                },
                "divide" => Proxy::Divide {
                    a: v("srcVar1")?,
                    b: v("srcVar2")?,
                    result: r(),
                },
                "uniformnoise" => Proxy::UniformNoise {
                    min: p.text("minVal").map_or(0.0, |t| numbers(t)[0]),
                    max: p.text("maxVal").map_or(1.0, |t| numbers(t)[0]),
                    result: r(),
                },
                "numericcounteratlasframe" => Proxy::NumericCounterAtlasFrame {
                    value: v("value")?,
                    digit: v("digitIndex")?,
                    result: v("resultAtlasFrameIndexVar")?,
                },
                "tickcounteratlasframes" => Proxy::TickCounterAtlasFrames {
                    ticks: v("tickCount")?,
                    row_length: v("rowLength")?,
                    row_index: v("rowIndex")?,
                    size: v("atlasFrameSize")?,
                    start: v("resultAtlasFrameStartVar")?,
                    count: v("resultAtlasFrameCountVar")?,
                },
                "textureatlas" => Proxy::TextureAtlas {
                    size: v("frameSize")?,
                    start: v("frameStart")?,
                    count: v("frameCount"),
                    result: name("resultVar"),
                },
                "progressbar" => Proxy::ProgressBar {
                    value: v("value")?,
                    max: v("max")?,
                    result: name("resultVar"),
                },
                "texturetransform" => Proxy::TextureTransform {
                    translate: v("translateVar"),
                    scale: v("scaleVar"),
                    rotate: v("rotateVar"),
                    center: v("centerVar"),
                    result: name("resultVar"),
                },
                _ => continue,
            };
            m.proxies.push(proxy);
        }
        (!m.proxies.is_empty()).then_some(m)
    }

    fn get(&self, v: &Value) -> f32 {
        match v {
            Value::Literal(x) => *x,
            Value::Var(name, i) => self.vars.get(name).map_or(0.0, |n| n[i.unwrap_or(0)]),
        }
    }

    fn get2(&self, v: &Value) -> Vec2 {
        match v {
            Value::Literal(x) => Vec2::splat(*x),
            Value::Var(name, Some(i)) => Vec2::splat(self.vars.get(name).map_or(0.0, |n| n[*i])),
            Value::Var(name, None) => self.vars.get(name).map_or(Vec2::ZERO, |n| {
                if self.widths.get(name).copied().unwrap_or(2) == 1 {
                    Vec2::splat(n[0])
                } else {
                    Vec2::new(n[0], n[1])
                }
            }),
        }
    }

    fn set(&mut self, v: &Value, x: f32) {
        if let Value::Var(name, i) = v {
            let slot = self.vars.entry(name.clone()).or_default();
            match i {
                Some(i) => slot[*i] = x,
                // A whole variable set from one number takes it everywhere,
                // as Source's result variables do.
                None => *slot = [x; 4],
            }
        }
    }

    fn set_vector(&mut self, v: &Value, x: [f32; 4]) {
        if let Value::Var(name, _) = v {
            self.vars.insert(name.clone(), x);
        }
    }

    fn set_color(&mut self, v: &Value, c: [f32; 3]) {
        if let Value::Var(name, _) = v {
            self.vars.insert(name.clone(), [c[0], c[1], c[2], 0.0]);
        }
    }

    /// Run the proxies for this frame.
    pub fn update(&mut self, cx: &Context<'_>) {
        let proxies = std::mem::take(&mut self.proxies);
        let clip_size = cx.clip_size.max(1);
        for p in &proxies {
            match p {
                Proxy::ClipAmmo(r) => self.set(r, cx.clip as f32),
                Proxy::MaxClipAmmo(r) => self.set(r, clip_size as f32),
                Proxy::RemainingAmmo(r) => self.set(r, cx.reserve as f32),
                Proxy::RemainingClipCount(r) => self.set(r, cx.reserve.div_ceil(clip_size) as f32),
                Proxy::MaxCarryAmmo(r) => self.set(r, cx.max_carry as f32),
                Proxy::ClipAmmoColor(r) => {
                    let c = if cx.clip * 10 <= cx.clip_size {
                        RED
                    } else if cx.clip as f32 >= cx.clip_size as f32 * 0.4 {
                        BLUE
                    } else {
                        ORANGE
                    };
                    self.set_color(r, c);
                }
                Proxy::RemainingAmmoColor(r) => {
                    let c = if cx.reserve <= cx.clip_size {
                        RED
                    } else if cx.reserve <= cx.clip_size * 3 {
                        ORANGE
                    } else {
                        BLUE
                    };
                    self.set_color(r, c);
                }
                Proxy::BackgroundAmmoColor(r) => {
                    let warn = cx.clip * 10 <= cx.clip_size;
                    self.set_color(r, if warn { BACKGROUND_WARNING } else { [1.0; 3] });
                }
                Proxy::HoloSight(r) => self.set(r, cx.holo),
                Proxy::HoloSightOffset {
                    attachment,
                    horizontal,
                    vertical,
                    result,
                } => {
                    if let Some(at) = cx.sights.and_then(|s| s.get(attachment)) {
                        self.set_vector(result, [at.x * horizontal, at.y * vertical, 0.0, 0.0]);
                    }
                }
                Proxy::Divide { a, b, result } => {
                    let (a, b) = (self.get(a), self.get(b));
                    self.set(result, if b == 0.0 { 0.0 } else { a / b });
                }
                Proxy::UniformNoise { min, max, result } => {
                    let x = min + (max - min) * self.random();
                    self.set(result, x);
                }
                Proxy::NumericCounterAtlasFrame {
                    value,
                    digit,
                    result,
                } => {
                    let (value, digit) = (self.get(value) as i32, self.get(digit) as i32);
                    let place = 10i32.saturating_pow(digit.max(0) as u32);
                    let frame = if value < place && digit > 0 {
                        0
                    } else {
                        (value / place).rem_euclid(10) + 1
                    };
                    self.set(result, frame as f32);
                }
                Proxy::TickCounterAtlasFrames {
                    ticks,
                    row_length,
                    row_index,
                    size,
                    start,
                    count,
                } => {
                    let ticks = self.get(ticks) as i32;
                    let size = self.get(size) as i32;
                    let row = self.get(row_index) as i32;
                    let mut length = self.get(row_length) as i32;
                    if length < 1 {
                        length = size;
                    }
                    let shown = if (row + 1) * length <= ticks {
                        length
                    } else if row * length >= ticks {
                        0
                    } else {
                        ticks % length.max(1)
                    };
                    self.set(start, (size - shown) as f32);
                    self.set(count, length as f32);
                }
                Proxy::TextureAtlas {
                    size,
                    start,
                    count,
                    result,
                } => {
                    let size = self.get2(size).max(Vec2::splat(1e-6));
                    let start = self.get2(start);
                    let count = count.as_ref().map_or(Vec2::ONE, |c| self.get2(c));
                    let t = Affine2::from_scale_angle_translation(count / size, 0.0, start / size);
                    self.transforms.insert(result.clone(), t);
                }
                Proxy::ProgressBar { value, max, result } => {
                    let (value, max) = (self.get(value), self.get(max));
                    let fraction = if max == 0.0 { 0.0 } else { value / max };
                    let t = Affine2::from_translation(Vec2::new(1.0 - fraction, 0.0));
                    self.transforms.insert(result.clone(), t);
                }
                Proxy::TextureTransform {
                    translate,
                    scale,
                    rotate,
                    center,
                    result,
                } => {
                    let center = center.as_ref().map_or(Vec2::splat(0.5), |c| self.get2(c));
                    let translate = translate.as_ref().map_or(Vec2::ZERO, |t| self.get2(t));
                    let scale = scale.as_ref().map_or(Vec2::ONE, |s| self.get2(s));
                    let angle = rotate.as_ref().map_or(0.0, |r| self.get(r)).to_radians();
                    let t = Affine2::from_translation(center + translate)
                        * Affine2::from_angle(angle)
                        * Affine2::from_scale(scale)
                        * Affine2::from_translation(-center);
                    self.transforms.insert(result.clone(), t);
                }
            }
        }
        self.proxies = proxies;
    }

    /// A texture transform result (identity if none).
    pub fn transform(&self, name: &str) -> Affine2 {
        self.transforms
            .get(name)
            .copied()
            .unwrap_or(Affine2::IDENTITY)
    }

    /// `$color` as RGB and `$alpha`.
    pub fn color_alpha(&self) -> ([f32; 3], f32) {
        let c = self.vars.get("color").copied().unwrap_or([1.0; 4]);
        let a = self.vars.get("alpha").map_or(1.0, |a| a[0]);
        ([c[0], c[1], c[2]], a)
    }

    fn random(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.rng >> 8) as f32 / (1u32 << 24) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGIT: &str = r#""Basic"
{
	"$ammo" "0"
	"$atlasFrames" "[16 1]"
	"$atlasFrameStart" "[0 0]"
	"Proxies"
	{
		"ClipAmmo" { "resultVar" "$ammo" }
		"NumericCounterAtlasFrame"
		{
			"value" "$ammo"
			"digitIndex" "1"
			"resultAtlasFrameIndexVar" "$atlasFrameStart[0]"
		}
		"TextureAtlas"
		{
			"frameSize" "$atlasFrames"
			"frameStart" "$atlasFrameStart"
			"resultVar" "$basetexturetransform"
		}
		"ClipAmmoColor" { "resultVar" "$color" }
	}
}"#;

    fn cx(clip: u32, reserve: u32) -> Context<'static> {
        Context {
            clip,
            clip_size: 24,
            reserve,
            max_carry: 240,
            holo: 0.0,
            sights: None,
        }
    }

    #[test]
    fn tens_digit_shows_and_blanks() {
        let mut m = ProxyMaterial::parse(DIGIT).unwrap();
        m.update(&cx(24, 216));
        // Tens digit of 24 is 2 → frame 3 of 16.
        let t = m.transform("basetexturetransform");
        assert!((t.translation.x - 3.0 / 16.0).abs() < 1e-6, "{t:?}");
        assert!((t.matrix2.x_axis.x - 1.0 / 16.0).abs() < 1e-6);
        assert_eq!(m.color_alpha().0, BLUE);
        // 7 rounds: no tens digit (blank frame 0), and orange below 40%.
        m.update(&cx(7, 216));
        assert_eq!(m.transform("basetexturetransform").translation.x, 0.0);
        assert_eq!(m.color_alpha().0, ORANGE);
        m.update(&cx(2, 216));
        assert_eq!(m.color_alpha().0, RED);
    }

    #[test]
    fn magazine_ticks_slide_the_strip() {
        let vmt = r#""Basic"
{
	"$remainingClipCount" "0"
	"$rowLength" "0"
	"$atlasFrames" "[10 1]"
	"$atlasFrameStart" "[0 0]"
	"$atlasFrameCount" "[1 1]"
	"Proxies"
	{
		"RemainingClipCount" { "resultVar" "$remainingClipCount" }
		"MaxCarryAmmo" { "resultVar" "$maxCarryAmmo" }
		"MaxClipAmmo" { "resultVar" "$maxClipAmmo" }
		"Divide" { "srcVar1" "$maxCarryAmmo" "srcVar2" "$maxClipAmmo" "resultVar" "$rowLength" }
		"TickCounterAtlasFrames"
		{
			"tickCount" "$remainingClipCount"
			"rowLength" "$rowLength"
			"rowIndex" "0"
			"atlasFrameSize" "$atlasFrames[0]"
			"resultAtlasFrameStartVar" "$atlasFrameStart[0]"
			"resultAtlasFrameCountVar" "$atlasFrameCount[0]"
		}
		"TextureAtlas"
		{
			"frameSize" "$atlasFrames"
			"frameStart" "$atlasFrameStart"
			"frameCount" "$atlasFrameCount"
			"resultVar" "$basetexturetransform"
		}
	}
}"#;
        let mut m = ProxyMaterial::parse(vmt).unwrap();
        // 206 spare → 9 magazines (rounded up): start 10 - 9 = 1.
        m.update(&cx(14, 206));
        let t = m.transform("basetexturetransform");
        assert!((t.translation.x - 0.1).abs() < 1e-6, "{t:?}");
        assert!((t.matrix2.x_axis.x - 1.0).abs() < 1e-6);
    }
}
