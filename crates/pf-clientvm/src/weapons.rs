//! Weapon info from the weapon files (`scripts/weapons/<name>.txt`):
//! `GetWeaponInfoFileKeyField_Global( weapon, key )` (client.dll
//! 0x1802c6710) and `PrecacheWeapon( weapon )` (0x1802c5ef0).
//!
//! The engine keeps each weapon file as KeyValues, whose reader types every
//! value when loading (0x18065e280), quoted or not:
//! - empty: a string;
//! - 18 characters starting `0x`: a 64-bit integer;
//! - read as a float, it reaches the end and further than read as an
//!   integer: a float (`"13.5"`, `"1e3"`);
//! - read as an integer, it reaches the end without overflowing: an integer;
//! - otherwise a string.
//!
//! The field comes back as that type (a 64-bit integer, or a block, as
//! null).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use pf_fmt_ui::kv;
use pf_squirrel::{Value, throw};

use crate::{ClientVm, Host};

/// A KeyValues value as the engine's reader types it.
#[derive(Clone, Debug, PartialEq)]
pub enum KvValue {
    Str(String),
    Int(i32),
    Float(f32),
    Uint64(u64),
}

pub fn kv_value(s: &str) -> KvValue {
    if s.is_empty() {
        return KvValue::Str(String::new());
    }
    if s.len() == 18 && s.starts_with("0x") {
        let mut v: u64 = 0;
        for c in s.bytes().skip(2) {
            let d = match c {
                b'a'..=b'z' => c - 0x27,
                b'A'..=b'Z' => c - 7,
                _ => c,
            };
            v = v.wrapping_mul(16).wrapping_add((d as i64 - 0x30) as u64);
        }
        return KvValue::Uint64(v);
    }
    let int_end = strtol_end(s);
    let float_end = strtod_end(s);
    if int_end < float_end && float_end == s.len() {
        return KvValue::Float(s.trim_start().parse::<f64>().unwrap_or(0.0) as f32);
    }
    // An integer that overflows 32 bits stays a string (strtol reports the
    // overflow).
    if int_end == s.len()
        && let Ok(v) = s.trim_start().parse::<i64>()
        && let Ok(v) = i32::try_from(v)
    {
        return KvValue::Int(v);
    }
    KvValue::Str(s.to_string())
}

/// Where C's `strtol( s, &end, 10 )` stops (0 if nothing was read).
fn strtol_end(s: &str) -> usize {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && (b[i] as char).is_ascii_whitespace() {
        i += 1;
    }
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == start { 0 } else { i }
}

/// Where C's `strtod` stops (0 if nothing was read).
fn strtod_end(s: &str) -> usize {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && (b[i] as char).is_ascii_whitespace() {
        i += 1;
    }
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let mut digits = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return 0;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let ds = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > ds {
            i = j;
        }
    }
    i
}

/// Weapon files, read once.
#[derive(Default)]
pub struct WeaponFiles {
    files: HashMap<String, Option<kv::Node>>,
}

impl WeaponFiles {
    /// The weapon's `WeaponData` block, or None if there's no such weapon.
    pub fn data(&mut self, host: &dyn Host, name: &str) -> Option<&kv::Node> {
        let key = name.to_ascii_lowercase();
        self.files
            .entry(key.clone())
            .or_insert_with(|| {
                let text = host.read_script(&format!("scripts/weapons/{key}.txt"))?;
                kv::parse(&text).nodes.into_iter().next()
            })
            .as_ref()
    }
}

pub fn install(cvm: &mut ClientVm) {
    let files = Rc::new(RefCell::new(WeaponFiles::default()));

    let host = cvm.host.clone();
    let f = files.clone();
    cvm.vm
        .register("GetWeaponInfoFileKeyField_Global", 3, move |_, a| {
            if !host.is_connected() {
                return Err(throw("Not connected to a server"));
            }
            let name = match &a[1] {
                Value::Str(s) => s.to_string_lossy(),
                _ => String::new(),
            };
            let key = match &a[2] {
                Value::Str(s) => s.to_string_lossy(),
                _ => String::new(),
            };
            let mut f = f.borrow_mut();
            let Some(data) = f.data(&*host, &name) else {
                let shown = if name.is_empty() { "<<null>>" } else { &name };
                return Err(throw(format!(
                    "GetWeaponInfoFileKeyField_Global() - Could not find weapon with name '{shown}' (key was '{key}')"
                )));
            };
            Ok(match data.text(&key).map(kv_value) {
                Some(KvValue::Str(s)) => Value::str(&s),
                Some(KvValue::Int(i)) => Value::Int(i),
                Some(KvValue::Float(x)) => Value::Float(x),
                _ => Value::Null,
            })
        });

    let host = cvm.host.clone();
    cvm.vm.register("PrecacheWeapon", 2, move |_, a| {
        let name = match &a[1] {
            Value::Str(s) => s.to_string_lossy(),
            _ => String::new(),
        };
        if name.is_empty() {
            return Err(throw("Null weapon name"));
        }
        if files.borrow_mut().data(&*host, &name).is_none() || !host.precache_weapon(&name) {
            return Err(throw(format!("Couldn't precache '{name}'.")));
        }
        Ok(Value::Null)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_types_follow_the_engine() {
        assert_eq!(kv_value("13.5"), KvValue::Float(13.5));
        assert_eq!(kv_value("1"), KvValue::Int(1));
        assert_eq!(kv_value("-20"), KvValue::Int(-20));
        assert_eq!(kv_value("1e3"), KvValue::Float(1000.0));
        assert_eq!(kv_value(""), KvValue::Str(String::new()));
        assert_eq!(kv_value("5 "), KvValue::Str("5 ".into()));
        assert_eq!(kv_value("#WPN_R101"), KvValue::Str("#WPN_R101".into()));
        assert_eq!(kv_value("0x0000000000000010"), KvValue::Uint64(16));
    }
}
