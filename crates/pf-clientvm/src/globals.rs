//! client.dll's global script functions (and the few launcher.dll ones
//! that client.dll's VM setup registers), each ported from the function
//! noted beside it.

use std::cell::Cell;
use std::rc::Rc;

use pf_squirrel::{Value, throw};

use crate::ClientVm;

pub fn install(cvm: &mut ClientVm) {
    let vm = &mut cvm.vm;

    // DoIncludeScript( name, scope ) (0x1802cb3a0): includes nest at most 17
    // deep.
    let host = cvm.host.clone();
    let depth = Rc::new(Cell::new(0u32));
    vm.register("DoIncludeScript", 3, move |vm, a| {
        let Value::Str(name) = &a[1] else {
            return Err(throw("Failed to include script \"unknown\""));
        };
        let name = name.to_string_lossy();
        if depth.get() >= 17 {
            return Err(throw(format!("Failed to include script \"{name}\"")));
        }
        depth.set(depth.get() + 1);
        let r = crate::include(vm, &*host, &name, a[2].clone());
        depth.set(depth.get() - 1);
        r?;
        Ok(Value::Bool(true))
    });

    // ScriptExists( name ): the same path rules as DoIncludeScript.
    let host = cvm.host.clone();
    vm.register("ScriptExists", 2, move |_, a| {
        let Value::Str(name) = &a[1] else {
            return Ok(Value::Bool(false));
        };
        Ok(Value::Bool(
            crate::script_path(&name.to_string_lossy()).is_some_and(|p| host.script_exists(&p)),
        ))
    });

    // Which VM this is (0x1802c1f90, 0x1802c1fc0, 0x1802c1f80).
    vm.register("IsClient", 1, |_, _| Ok(Value::Bool(true)));
    vm.register("IsUI", 1, |_, _| Ok(Value::Bool(false)));
    vm.register("IsServer", 1, |_, _| Ok(Value::Bool(false)));
    // The multiplayer game mode's version returns true (0x1804492a0); the
    // single-player one returns false.
    vm.register("IsMultiplayer", 1, |_, _| Ok(Value::Bool(true)));

    // GetMapName (0x1802c2030): the level path without folders or
    // extension, lower-cased.
    let host = cvm.host.clone();
    vm.register("GetMapName", 1, move |_, _| {
        Ok(Value::str(&map_name(&host.level_name())))
    });

    // GetDeveloperLevel (0x1802c21a0): the developer console variable.
    let host = cvm.host.clone();
    vm.register("GetDeveloperLevel", 1, move |_, _| {
        Ok(Value::Int(host.developer()))
    });

    // PrecacheHUDMaterial( name ) (0x1802c6090): `vgui/` is added unless the
    // name contains ".pic"; lower-cased.
    let host = cvm.host.clone();
    vm.register("PrecacheHUDMaterial", 2, move |_, a| {
        let name = match &a[1] {
            Value::Str(s) => s.to_string_lossy(),
            _ => String::new(),
        };
        if name.is_empty() {
            return Err(throw("Empty image name."));
        }
        let path = if name.to_ascii_lowercase().contains(".pic") {
            name.clone()
        } else {
            format!("vgui/{name}")
        };
        if !host.precache_hud_material(&path.to_ascii_lowercase()) {
            return Err(throw(format!("Couldn't precache '{name}'.")));
        }
        Ok(Value::Null)
    });

    // Time and FrameTime (0x1802c2170, 0x1802c2180).
    let host = cvm.host.clone();
    vm.register("Time", 1, move |_, _| Ok(Value::Float(host.time())));
    let host = cvm.host.clone();
    vm.register("FrameTime", 1, move |_, _| {
        Ok(Value::Float(host.frame_time()))
    });

    // RandomInt( min, max ) and RandomFloat( min, max ), inclusive
    // (launcher.dll, from the engine's random stream). The numbers are ours:
    // the game's stream is seeded differently every run, so no sequence can
    // match it anyway.
    let rng = Rc::new(Cell::new(0x2545_f491_4f6c_dd1du64));
    let r = rng.clone();
    vm.register("RandomInt", 3, move |_, a| {
        let (lo, hi) = (int(&a[1]), int(&a[2]));
        if hi <= lo {
            return Ok(Value::Int(lo));
        }
        let span = (hi as i64 - lo as i64 + 1) as u64;
        Ok(Value::Int((lo as i64 + (next(&r) % span) as i64) as i32))
    });
    vm.register("RandomFloat", 3, move |_, a| {
        let (lo, hi) = (num(&a[1]), num(&a[2]));
        let t = (next(&rng) >> 40) as f32 / (1u64 << 24) as f32;
        Ok(Value::Float(lo + (hi - lo) * t))
    });
}

fn next(state: &Cell<u64>) -> u64 {
    // xorshift64*
    let mut x = state.get();
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    state.set(x);
    x.wrapping_mul(0x2545_f491_4f6c_dd1d)
}

fn int(v: &Value) -> i32 {
    match v {
        Value::Int(i) => *i,
        Value::Float(x) => *x as i32,
        _ => 0,
    }
}

fn num(v: &Value) -> f32 {
    match v {
        Value::Int(i) => *i as f32,
        Value::Float(x) => *x,
        _ => 0.0,
    }
}

/// `maps/MP_Colony.bsp` → `mp_colony`.
pub fn map_name(level: &str) -> String {
    let start = level.rfind(['/', '\\']).map_or(0, |i| i + 1);
    let rest = &level[start..];
    let end = rest.rfind('.').unwrap_or(rest.len());
    rest[..end].to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_names() {
        assert_eq!(map_name("maps/MP_Colony.bsp"), "mp_colony");
        assert_eq!(map_name("maps\\mp_lobby.bsp"), "mp_lobby");
        assert_eq!(map_name("mp_angel_city"), "mp_angel_city");
    }
}
