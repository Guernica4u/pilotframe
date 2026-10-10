//! The pilot's loadout and their weapons' settings, from the game's own files.
//!
//! - **Loadout:** the preset loadouts in `scripts/vscripts/_loadouts.nut`. The
//!   game starts a new pilot on `pilot_preset_loadout_1` (R-201 with HCOG,
//!   Archer, Hammond P2011, cloak, frag grenades, and the ordnance-pack and
//!   longer-bubble passives).
//! - **Weapon settings:** `scripts/weapons/<name>.txt`, the `WeaponData` block,
//!   with mods applied over it. A mod is a block under `Mods`; the weapon's
//!   attachment is one, and each passive is one too: the game's passive code
//!   calls `GiveExtraWeaponMod(<passive>)`, which applies it to every weapon that
//!   defines a mod by that name (the frag's `pas_ordnance_pack` adds a grenade).
//!
//! Notes: docs/weapons.md ("Loadout").

use std::collections::HashMap;
use std::path::Path;

use pf_fmt_ui::kv;

/// The preset a new pilot starts with.
pub const DEFAULT_PRESET: &str = "pilot_preset_loadout_1";

/// One pilot loadout (the fields `_loadouts.nut` sets).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Loadout {
    pub primary: String,
    pub primary_attachment: Option<String>,
    pub secondary: String,
    pub sidearm: String,
    pub special: String,
    pub ordnance: String,
    pub passives: Vec<String>,
}

impl Loadout {
    /// The preset `name` from the game's loadout script, if it's there.
    pub fn preset(dir: &Path, name: &str) -> Option<Self> {
        let text = std::fs::read_to_string(dir.join("scripts/vscripts/_loadouts.nut")).ok()?;
        parse_preset(&text, name)
    }
}

/// Read the preset `name` out of `_loadouts.nut`: the script fills a table with
/// lines like `loadout.primary <- "mp_weapon_rspn101"` starting at
/// `loadout.ref <- "<name>"`, until it appends the table.
fn parse_preset(text: &str, name: &str) -> Option<Loadout> {
    let start = text.find(&format!("loadout.ref <- \"{name}\""))?;
    let mut out = Loadout::default();
    for line in text[start..].lines().skip(1) {
        let line = line.trim();
        if line.starts_with("pilotLoadouts.append") || line.starts_with("loadout.ref") {
            break;
        }
        let Some((field, value)) = line
            .strip_prefix("loadout.")
            .and_then(|l| l.split_once("<-"))
        else {
            continue;
        };
        let value = value.trim();
        let value = value.strip_prefix('"').and_then(|v| v.split('"').next());
        let set = |slot: &mut String| *slot = value.unwrap_or_default().to_string();
        match field.trim() {
            "primary" => set(&mut out.primary),
            "primaryAttachment" => out.primary_attachment = value.map(str::to_string),
            "secondary" => set(&mut out.secondary),
            "sidearm" => set(&mut out.sidearm),
            "special" => set(&mut out.special),
            "ordnance" => set(&mut out.ordnance),
            f if f.starts_with("passive") => out.passives.extend(value.map(str::to_string)),
            _ => {}
        }
    }
    Some(out)
}

/// A weapon's settings (lowercase keys) with mods applied.
#[derive(Clone, Debug, Default)]
pub struct WeaponScript {
    pub values: HashMap<String, String>,
}

impl WeaponScript {
    /// `scripts/weapons/<name>.txt` with each of `mods` that it defines applied,
    /// in order.
    pub fn load(dir: &Path, name: &str, mods: &[&str]) -> Option<Self> {
        let text = std::fs::read_to_string(dir.join(format!("scripts/weapons/{name}.txt"))).ok()?;
        Some(Self::parse(&text, mods))
    }

    fn parse(text: &str, mods: &[&str]) -> Self {
        let Some(root) = kv::parse(text).nodes.into_iter().next() else {
            return Self::default();
        };
        let mut values: HashMap<String, String> = texts(&root).collect();
        let defined = root.get("Mods");
        for m in mods {
            let Some(block) = defined.and_then(|d| d.get(m)) else {
                continue;
            };
            for (key, value) in texts(block) {
                let new = apply_mod(values.get(&key).map(String::as_str), &value);
                values.insert(key, new);
            }
        }
        Self { values }
    }

    pub fn text(&self, key: &str) -> Option<&str> {
        self.values
            .get(&key.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn num(&self, key: &str) -> Option<f32> {
        self.text(key)?.trim().parse().ok()
    }
}

/// A block's plain `"key" "value"` lines, keys lowercased.
fn texts(node: &kv::Node) -> impl Iterator<Item = (String, String)> + '_ {
    node.children().iter().filter_map(|n| match &n.value {
        kv::Value::Text(t) => Some((n.key.to_ascii_lowercase(), t.clone())),
        kv::Value::Block(_) => None,
    })
}

/// A mod's value over the base one. Mods write relative changes as `*n`
/// (times), `/n`, `n++` (plus) and `n--` (minus); anything else replaces.
fn apply_mod(base: Option<&str>, value: &str) -> String {
    let v = value.trim();
    let number = |s: &str| s.trim().parse::<f32>().ok();
    let base_num = base.and_then(number).unwrap_or(0.0);
    let result = if let Some(n) = v.strip_prefix('*').and_then(number) {
        base_num * n
    } else if let Some(n) = v.strip_prefix('/').and_then(number) {
        base_num / n
    } else if let Some(n) = v.strip_suffix("++").and_then(number) {
        base_num + n
    } else if let Some(n) = v.strip_suffix("--").and_then(number) {
        base_num - n
    } else {
        return value.to_string();
    };
    result.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_preset_until_it_is_appended() {
        let text = r#"
	loadout.ref <- "first"
	loadout.primary <- "gun_a"
	loadout.primaryAttachment <- "scope"
	loadout.primaryMod <- null
	loadout.secondary <- "launcher"
	loadout.sidearm <- "pistol"
	loadout.special <- "cloak"
	loadout.ordnance <- "frag"
	loadout.passive1 <- "pack"
	loadout.passive2 <- "bubble"
	pilotLoadouts.append( clone loadout )
	loadout.ref <- "second"
	loadout.primary <- "gun_b"
"#;
        let l = parse_preset(text, "first").unwrap();
        assert_eq!(l.primary, "gun_a");
        assert_eq!(l.primary_attachment.as_deref(), Some("scope"));
        assert_eq!(
            (l.secondary.as_str(), l.sidearm.as_str()),
            ("launcher", "pistol")
        );
        assert_eq!((l.special.as_str(), l.ordnance.as_str()), ("cloak", "frag"));
        assert_eq!(l.passives, ["pack", "bubble"]);
        assert_eq!(parse_preset(text, "second").unwrap().primary, "gun_b");
        assert!(parse_preset(text, "third").is_none());
    }

    #[test]
    fn mods_replace_or_change_values() {
        let text = r#"WeaponData
{
	"ammo_clip_size"	"2"
	"fire_rate"			"4"
	"hud_icon"			"HUD/a"
	Mods
	{
		pack	{ "ammo_clip_size" "1++" }
		fast	{ "fire_rate" "*1.5" }
		slow	{ "fire_rate" "/2" }
		less	{ "ammo_clip_size" "1--" }
		icon	{ "hud_icon" "HUD/b" }
	}
}"#;
        let w = WeaponScript::parse(text, &["pack", "fast", "icon", "missing"]);
        assert_eq!(w.num("ammo_clip_size"), Some(3.0));
        assert_eq!(w.num("fire_rate"), Some(6.0));
        assert_eq!(w.text("hud_icon"), Some("HUD/b"));
        let w = WeaponScript::parse(text, &["slow", "less"]);
        assert_eq!(w.num("fire_rate"), Some(2.0));
        assert_eq!(w.num("ammo_clip_size"), Some(1.0));
    }
}
