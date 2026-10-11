//! Recoil ("view kick"), ported from `client.dll` (docs/port/weapons.md, "Recoil").
//!
//! Each shot rolls a kick from the weapon script's `viewkick_*` settings,
//! scales it by how far into a burst the gun is and by crouching, and splits it
//! into a "soft" kick (added to the kick spring's speed) and a "hard" kick
//! (added to its angle) — see [`crate::view`]. A small permanent kick moves the
//! player's actual aim. The kick spring has the weapon's own constants
//! (`viewkick_spring`, a set in `scripts/weapons/springs.txt`), lerped from hip
//! to aimed by the zoom fraction.
//!
//! Angles are the game's: degrees, pitch down, yaw left, roll right.

use bevy::math::{Vec2, Vec3};
use pf_fmt_ui::kv;

use crate::loadout::WeaponScript;

/// A kick spring's constants per axis (pitch, yaw, roll).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringConstants {
    pub stiffness: Vec3,
    pub damping: Vec3,
}

impl SpringConstants {
    /// The engine's base view-punch spring (`viewpunch_base_spring*`, and the
    /// pilot's `punch_pilot` set): 65 and 9 on every axis.
    pub const BASE: Self = Self {
        stiffness: Vec3::splat(65.0),
        damping: Vec3::splat(9.0),
    };

    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        Self {
            stiffness: a.stiffness.lerp(b.stiffness, t),
            damping: a.damping.lerp(b.damping, t),
        }
    }
}

/// A spring set from `springs.txt`: hip fire and aimed constants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringSet {
    pub hip: SpringConstants,
    pub ads: SpringConstants,
}

impl SpringSet {
    /// The set `name` from `scripts/weapons/springs.txt`.
    pub fn load(name: &str) -> Option<Self> {
        let text = crate::data::read_text("scripts/weapons/springs.txt")?;
        let root = kv::parse(&text).nodes.into_iter().next()?;
        let set = root.get(name)?;
        let num = |key: &str| set.text(key).and_then(|v| v.trim().parse::<f32>().ok());
        let axes = |prefix: &str, what: &str| -> Option<Vec3> {
            Some(Vec3::new(
                num(&format!("{prefix}_pitch_{what}"))?,
                num(&format!("{prefix}_yaw_{what}"))?,
                num(&format!("{prefix}_roll_{what}"))?,
            ))
        };
        Some(Self {
            hip: SpringConstants {
                stiffness: axes("hipfire", "springConstant")?,
                damping: axes("hipfire", "damping")?,
            },
            ads: SpringConstants {
                stiffness: axes("ads", "springConstant")?,
                damping: axes("ads", "damping")?,
            },
        })
    }

    /// The constants at `zoom` (0 hip .. 1 aimed).
    pub fn at(&self, zoom: f32) -> SpringConstants {
        SpringConstants::lerp(&self.hip, &self.ads, zoom)
    }
}

/// One axis of the kick: `base` plus a random amount between half of
/// `exclude` and half of `random`, with a random sign.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KickAxis {
    pub base: f32,
    pub random: f32,
    pub exclude: f32,
}

impl KickAxis {
    fn roll(&self, r: f32, flip: bool) -> f32 {
        let size = self.exclude * 0.5 + (self.random * 0.5 - self.exclude * 0.5) * r;
        self.base + if flip { -size } else { size }
    }
}

/// The weapon's recoil settings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecoilData {
    pub spring: Option<SpringSet>,
    pub pitch: KickAxis,
    pub yaw: KickAxis,
    /// Roll: `base` plus a random amount between `min` and `max`, randomly signed.
    pub roll_base: f32,
    pub roll_min: f32,
    pub roll_max: f32,
    /// Soft and hard scales for (pitch, yaw, roll).
    pub soft: Vec3,
    pub hard: Vec3,
    /// How much of the kick moves the gun instead of the view, and how much of
    /// that the gun shows, hip fire and aimed.
    pub weapon_fraction: Vec2,
    pub weapon_fraction_vm_scale: Vec2,
    /// Burst scale: the first shot's (0 = none), and min/max (hip, aimed).
    pub scale_first_shot: Vec2,
    pub scale_min: Vec2,
    pub scale_max: Vec2,
    pub value_per_shot: f32,
    pub value_lerp_start: f32,
    pub value_lerp_end: f32,
    pub value_decay_delay: f32,
    pub value_decay_rate: f32,
    pub duck_scale: f32,
    /// The permanent kick (moves the aim itself).
    pub perm_pitch: KickAxis,
    pub perm_yaw: KickAxis,
}

impl RecoilData {
    /// From the weapon script; missing values take the settings table's
    /// defaults (scales and `duck_scale` 1, the rest 0).
    pub fn from_script(w: &WeaponScript) -> Self {
        let num = |key: &str, default: f32| w.num(key).unwrap_or(default);
        let axis = |name: &str| KickAxis {
            base: num(&format!("viewkick_{name}_base"), 0.0),
            random: num(&format!("viewkick_{name}_random"), 0.0),
            exclude: num(&format!("viewkick_{name}_random_innerexclude"), 0.0),
        };
        let pair =
            |hip: &str, ads: &str, default: f32| Vec2::new(num(hip, default), num(ads, default));
        Self {
            spring: w
                .text("viewkick_spring")
                .and_then(|name| SpringSet::load(name.trim())),
            pitch: axis("pitch"),
            yaw: axis("yaw"),
            roll_base: num("viewkick_roll_base", 0.0),
            roll_min: num("viewkick_roll_randomMin", 0.0),
            roll_max: num("viewkick_roll_randomMax", 0.0),
            soft: Vec3::new(
                num("viewkick_pitch_softScale", 0.0),
                num("viewkick_yaw_softScale", 0.0),
                num("viewkick_roll_softScale", 0.0),
            ),
            hard: Vec3::new(
                num("viewkick_pitch_hardScale", 0.0),
                num("viewkick_yaw_hardScale", 0.0),
                num("viewkick_roll_hardScale", 0.0),
            ),
            weapon_fraction: pair(
                "viewkick_hipfire_weaponFraction",
                "viewkick_ads_weaponFraction",
                0.0,
            ),
            weapon_fraction_vm_scale: pair(
                "viewkick_hipfire_weaponFraction_vmScale",
                "viewkick_ads_weaponFraction_vmScale",
                0.0,
            ),
            scale_first_shot: pair(
                "viewkick_scale_firstshot_hipfire",
                "viewkick_scale_firstshot_ads",
                0.0,
            ),
            scale_min: pair("viewkick_scale_min_hipfire", "viewkick_scale_min_ads", 1.0),
            scale_max: pair("viewkick_scale_max_hipfire", "viewkick_scale_max_ads", 1.0),
            value_per_shot: num("viewkick_scale_valuePerShot", 0.0),
            value_lerp_start: num("viewkick_scale_valueLerpStart", 0.0),
            value_lerp_end: num("viewkick_scale_valueLerpEnd", 0.0),
            value_decay_delay: num("viewkick_scale_valueDecayDelay", 0.0),
            value_decay_rate: num("viewkick_scale_valueDecayRate", 0.0),
            duck_scale: num("viewkick_duck_scale", 1.0),
            perm_pitch: KickAxis {
                base: num("viewkick_perm_pitch_base", 0.0),
                random: num("viewkick_perm_pitch_random", 0.0),
                exclude: num("viewkick_perm_pitch_random_innerexclude", 0.0),
            },
            perm_yaw: KickAxis {
                base: num("viewkick_perm_yaw_base", 0.0),
                random: num("viewkick_perm_yaw_random", 0.0),
                exclude: num("viewkick_perm_yaw_random_innerexclude", 0.0),
            },
        }
    }

    /// The kick spring's constants at `zoom`, the base ones if the weapon has
    /// no set of its own.
    pub fn spring_at(&self, zoom: f32) -> SpringConstants {
        self.spring.map_or(SpringConstants::BASE, |s| s.at(zoom))
    }

    /// The gun's share of the kick and how much of it the gun shows, at `zoom`.
    pub fn split_at(&self, zoom: f32) -> (f32, f32) {
        let lerp = |v: Vec2| v.x + (v.y - v.x) * zoom;
        (
            lerp(self.weapon_fraction),
            lerp(self.weapon_fraction_vm_scale),
        )
    }
}

/// What one shot does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Kick {
    /// Added to the kick spring's speed (× 20) and angle.
    pub soft: Vec3,
    pub hard: Vec3,
    /// Added to the aim itself: (pitch down, yaw left), degrees.
    pub perm: Vec2,
}

/// The weapon's recoil state: the burst value and its timing.
#[derive(Clone, Debug, Default)]
pub struct Recoil {
    stored: f32,
    last_shot: f32,
    /// Shots since the trigger was pulled.
    shots_in_pull: u32,
    rng: u32,
}

impl Recoil {
    /// The burst value now: it holds for `decay_delay` after the last shot,
    /// then falls at `decay_rate` per second (`0x180488060`).
    fn value(&self, d: &RecoilData, now: f32) -> f32 {
        if self.stored == 0.0 {
            return 0.0;
        }
        let late = now - self.last_shot - d.value_decay_delay;
        if late <= 0.0 {
            return self.stored;
        }
        (self.stored - late * d.value_decay_rate).max(0.0)
    }

    /// The kick scale for a shot now (`0x18048b940`): the first shot's scale
    /// if it has one, else from min to max as the burst value goes from its
    /// lerp start to its end; each lerped hip → aimed by `zoom`.
    fn scale(&self, d: &RecoilData, now: f32, zoom: f32) -> f32 {
        let lerp = |v: Vec2| v.x + (v.y - v.x) * zoom;
        let first = lerp(d.scale_first_shot);
        if self.shots_in_pull == 0 && first != 0.0 {
            return first;
        }
        let range = d.value_lerp_end - d.value_lerp_start;
        let t = if range == 0.0 {
            1.0
        } else {
            ((self.value(d, now) - d.value_lerp_start) / range).clamp(0.0, 1.0)
        };
        lerp(d.scale_min) + (lerp(d.scale_max) - lerp(d.scale_min)) * t
    }

    /// The trigger was let go: the next shot is a first shot again.
    pub fn release(&mut self) {
        self.shots_in_pull = 0;
    }

    /// Fire one shot at time `now` (seconds), `zoom` into aiming (0..1),
    /// `duck` crouched (0..1). The kick uses the burst value from before this
    /// shot, which then grows by `valuePerShot`, up to its lerp end
    /// (`0x180491190`, `0x18049c4c0`).
    pub fn fire(&mut self, d: &RecoilData, now: f32, zoom: f32, duck: f32) -> Kick {
        let scale = self.scale(d, now, zoom) * ((d.duck_scale - 1.0) * duck + 1.0);
        let (r, f) = (self.random(), self.flip());
        let pitch = d.pitch.roll(r, f);
        let (r, f) = (self.random(), self.flip());
        let yaw = d.yaw.roll(r, f);
        let r = self.random();
        let size = d.roll_min + (d.roll_max - d.roll_min) * r;
        let roll = d.roll_base + if self.flip() { -size } else { size };
        let kick = Vec3::new(pitch * scale, yaw * scale, roll);
        let (r, f) = (self.random(), self.flip());
        let perm_pitch = d.perm_pitch.roll(r, f) * scale;
        let (r, f) = (self.random(), self.flip());
        let perm_yaw = d.perm_yaw.roll(r, f) * scale;

        self.stored = (self.value(d, now) + d.value_per_shot).clamp(0.0, d.value_lerp_end.max(0.0));
        self.last_shot = now;
        self.shots_in_pull += 1;
        Kick {
            soft: kick * d.soft,
            hard: kick * d.hard,
            perm: Vec2::new(perm_pitch, perm_yaw),
        }
    }

    /// 0..1 from a small fixed-seed generator (the game uses its shared,
    /// named random streams, "weaponKickX" etc., so client and server agree).
    fn random(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.rng >> 8) as f32 / (1u32 << 24) as f32
    }

    fn flip(&mut self) -> bool {
        self.random() < 0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r201() -> RecoilData {
        RecoilData {
            pitch: KickAxis {
                base: -0.35,
                random: 0.2,
                exclude: 0.0,
            },
            yaw: KickAxis {
                base: 0.0,
                random: 0.3,
                exclude: 0.15,
            },
            roll_min: 0.1,
            roll_max: 0.2,
            soft: Vec3::new(1.0, 1.2, 0.2),
            hard: Vec3::new(1.0, 0.8, 1.85),
            weapon_fraction: Vec2::new(0.5, 0.0),
            weapon_fraction_vm_scale: Vec2::new(0.5, 1.0),
            scale_first_shot: Vec2::new(0.0, 0.25),
            scale_min: Vec2::new(0.3, 0.4),
            scale_max: Vec2::new(1.5, 1.0),
            value_per_shot: 1.0,
            value_lerp_start: 0.0,
            value_lerp_end: 2.0,
            value_decay_delay: 0.3,
            value_decay_rate: 25.0,
            duck_scale: 0.9,
            perm_yaw: KickAxis {
                base: 0.025,
                random: 0.05,
                exclude: 0.0,
            },
            ..RecoilData::default()
        }
    }

    #[test]
    fn kicks_up_and_builds_over_a_burst_then_recovers() {
        let d = r201();
        let mut r = Recoil::default();
        // First hip shot: burst value 0 → min scale 0.3. Pitch -0.35 ± 0.1.
        let k = r.fire(&d, 0.0, 0.0, 0.0);
        assert!(
            k.hard.x < 0.0 && k.hard.x.abs() <= 0.45 * 0.3 + 1e-4,
            "{k:?}"
        );
        // Yaw is at least half the inner exclude either way.
        assert!(k.hard.y.abs() >= 0.075 * 0.3 * 0.8 - 1e-4, "{k:?}");
        // Shots 0.074 s apart (13.5/s): the value reaches 2 → max scale 1.5.
        let mut last = k;
        for i in 1..6 {
            last = r.fire(&d, i as f32 * 0.074, 0.0, 0.0);
        }
        assert!(last.hard.x.abs() >= 0.25 * 1.5 - 1e-4, "{last:?}");
        // After a pause past the delay the value has decayed to 0.
        assert_eq!(r.value(&d, 0.37 + 0.3 + 0.1), 0.0);
    }

    #[test]
    fn first_aimed_shot_uses_its_own_scale() {
        let d = r201();
        let mut r = Recoil::default();
        let k = r.fire(&d, 0.0, 1.0, 0.0);
        // Pitch at most 0.45 × 0.25 aimed on the first shot.
        assert!(k.hard.x.abs() <= 0.45 * 0.25 + 1e-4, "{k:?}");
        r.release();
        assert_eq!(r.shots_in_pull, 0);
    }

    #[test]
    fn split_and_springs_lerp_with_zoom() {
        let mut d = r201();
        assert_eq!(d.split_at(0.0), (0.5, 0.5));
        assert_eq!(d.split_at(1.0), (0.0, 1.0));
        assert_eq!(d.spring_at(0.5), SpringConstants::BASE);
        d.spring = Some(SpringSet {
            hip: SpringConstants {
                stiffness: Vec3::new(110.0, 85.0, 12500.0),
                damping: Vec3::new(10.0, 7.0, 15.0),
            },
            ads: SpringConstants {
                stiffness: Vec3::new(75.0, 95.0, 20000.0),
                damping: Vec3::splat(20.0),
            },
        });
        assert_eq!(d.spring_at(1.0).stiffness.x, 75.0);
    }
}
