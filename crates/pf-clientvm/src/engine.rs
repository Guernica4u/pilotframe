//! client.dll script functions that hand work to the engine: precaching,
//! playlists, colour correction, visibility groups. Each one's own rules
//! (errors, limits) are ported here; the effect goes to the host.

use std::cell::Cell;
use std::rc::Rc;

use pf_squirrel::{NativeError, Value, throw};

use crate::ClientVm;

fn name_arg(a: &[Value], i: usize) -> String {
    match a.get(i) {
        Some(Value::Str(s)) => s.to_string_lossy(),
        _ => String::new(),
    }
}

fn int_arg(a: &[Value], i: usize) -> i32 {
    match a.get(i) {
        Some(Value::Int(n)) => *n,
        Some(Value::Float(x)) => *x as i32,
        Some(Value::Bool(b)) => *b as i32,
        _ => 0,
    }
}

pub fn install(cvm: &mut ClientVm) {
    let vm = &mut cvm.vm;

    // IsConnected (0x1802b5950).
    let host = cvm.host.clone();
    vm.register("IsConnected", 1, move |_, _| {
        Ok(Value::Bool(host.is_connected()))
    });

    // GetCurrentPlaylistVar( name ) (0x18044a730) and
    // GetCurrentPlaylistVarOrUseValue( name, default ) (0x180449490).
    let host = cvm.host.clone();
    vm.register("GetCurrentPlaylistVar", 2, move |_, a| {
        if !host.is_connected() {
            return Err(throw("GetCurrentPlaylistVar: Not connected to a server"));
        }
        let name = name_arg(a, 1);
        Ok(Value::str(&host.playlist_var(&name).unwrap_or_default()))
    });
    let host = cvm.host.clone();
    vm.register("GetCurrentPlaylistVarOrUseValue", 3, move |_, a| {
        if !host.is_connected() {
            return Err(throw(
                "GetCurrentPlaylistVarOrUseValue: Not connected to a server",
            ));
        }
        let name = name_arg(a, 1);
        Ok(match host.playlist_var(&name) {
            Some(v) => Value::str(&v),
            None => a[2].clone(),
        })
    });

    // PrecacheMaterial( name ) (0x1802c5f90).
    let host = cvm.host.clone();
    vm.register("PrecacheMaterial", 2, move |_, a| {
        let name = name_arg(a, 1);
        if name.is_empty() {
            return Err(throw("Empty image name."));
        }
        if !host.precache_material(&name) {
            return Err(throw(format!("Couldn't precache '{name}'.")));
        }
        Ok(Value::Null)
    });
    // PrecacheModel( name ) (0x1802c5f40). The host skips loading in the
    // optimized lobby, as the game does.
    let host = cvm.host.clone();
    vm.register_checked("PrecacheModel", 2, ".s", move |_, a| {
        let name = name_arg(a, 1);
        if name.is_empty() {
            return Err(throw("Empty model name."));
        }
        if !host.precache_model(&name) {
            return Err(throw(format!("Couldn't precache '{name}'.")));
        }
        Ok(Value::Null)
    });
    // PrecacheParticleSystem( name ) (0x18013d580): its particle system index.
    let host = cvm.host.clone();
    vm.register("PrecacheParticleSystem", 2, move |_, a| {
        Ok(Value::Int(host.precache_particle_system(&name_arg(a, 1))))
    });
    // PrecacheImpactEffectTable( name ) (0x1802c5fe0).
    let host = cvm.host.clone();
    vm.register("PrecacheImpactEffectTable", 2, move |_, a| {
        let name = name_arg(a, 1);
        if name.is_empty() {
            return Err(throw("Empty table name."));
        }
        Ok(Value::Int(host.precache_impact_table(&name)))
    });

    // ColorCorrection_Register( name ) (0x18012d990) and
    // ColorCorrection_SetExclusive( index, exclusive ) (0x18012c520).
    let host = cvm.host.clone();
    vm.register("ColorCorrection_Register", 2, move |_, a| {
        Ok(Value::Int(host.color_correction_register(&name_arg(a, 1))))
    });
    let host = cvm.host.clone();
    vm.register("ColorCorrection_SetExclusive", 3, move |_, a| {
        host.color_correction_set_exclusive(int_arg(a, 1), a[2].truthy());
        Ok(Value::Null)
    });

    // CreateVisGroup (0x18041efa0): handles 0..63; the 65th call and later
    // fail (the counter still moves on).
    let next = Rc::new(Cell::new(0i32));
    vm.register(
        "CreateVisGroup",
        1,
        move |_, _| -> Result<Value, NativeError> {
            let id = next.get();
            next.set(id + 1);
            if next.get() > 0x40 {
                return Err(throw("No free visGroups."));
            }
            Ok(Value::Int(id))
        },
    );
}
