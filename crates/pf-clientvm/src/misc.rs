//! Smaller client.dll script functions: profiling marks, file-system hints,
//! platform checks, the bot-loadout name lists, impact tables, particle
//! lookups and `Graph`.

use std::cell::RefCell;
use std::rc::Rc;

use pf_squirrel::{Value, throw};

use crate::ClientVm;

fn name_arg(a: &[Value], i: usize) -> String {
    match a.get(i) {
        Some(Value::Str(s)) => s.to_string_lossy(),
        _ => String::new(),
    }
}

/// Impact tables precached for pilot landing and Titan footsteps, landing
/// and dodging (-1 for none).
#[derive(Default, Clone, Copy, Debug)]
pub struct ImpactTables {
    pub pilot_land: i32,
    pub titan_footstep: i32,
    pub titan_land: i32,
    pub titan_dodge: i32,
}

pub fn install(cvm: &mut ClientVm) {
    let vm = &mut cvm.vm;

    // PerfStart/PerfEnd( id ) (0x1802c4250/0x1802c4260) only time script
    // code for the profiler: nothing the game shows.
    vm.register("PerfStart", 2, |_, _| Ok(Value::Null));
    vm.register("PerfEnd", 2, |_, _| Ok(Value::Null));
    // PerfInitLabel( id, name ) (0x1802c4240) and PerfClearAll
    // (0x1802c4280) do nothing in the shipped game.
    vm.register_checked("PerfInitLabel", 3, ".ns", |_, _| Ok(Value::Null));
    vm.register_checked("PerfClearAll", 1, ".", |_, _| Ok(Value::Null));

    // AngleNormalize( angle ) (0x1804fb9f0): wrapped to [-180, 180].
    vm.register_checked("AngleNormalize", 2, ".n", |_, a| {
        Ok(Value::Float(crate::entities::angle_normalize(num(a, 1))))
    });

    // SafeareaSettingIsEnabled (0x1802c9220): the cl_safearea console
    // variable (default "0") is not 0.
    let host = cvm.host.clone();
    vm.register_checked("SafeareaSettingIsEnabled", 1, ".", move |_, _| {
        let v: f32 = host.convar("cl_safearea").trim().parse().unwrap_or(0.0);
        Ok(Value::Bool(v as i32 != 0))
    });

    // EverythingUnlockedConVarEnabled (0x1802c9290): the
    // everything_unlocked console variable (default "0") is not 0.
    let host = cvm.host.clone();
    vm.register_checked("EverythingUnlockedConVarEnabled", 1, ".", move |_, _| {
        let v: f32 = host
            .convar("everything_unlocked")
            .trim()
            .parse()
            .unwrap_or(0.0);
        Ok(Value::Bool(v as i32 != 0))
    });

    // AreSpectatorControlsActiveForLocalClientPlayer (0x1802b5ab0): the
    // local player is in observer mode 3 or 7 with the controls flag set.
    let host = cvm.host.clone();
    vm.register_checked(
        "AreSpectatorControlsActiveForLocalClientPlayer",
        1,
        ".",
        move |_, _| Ok(Value::Bool(host.spectator_controls_active())),
    );

    // IsWatchingKillReplay (0x1802b5970).
    let host = cvm.host.clone();
    vm.register_checked("IsWatchingKillReplay", 1, ".", move |_, _| {
        Ok(Value::Bool(host.watching_kill_replay()))
    });

    // GetCPULevel (0x1802b5b40): the particle_cpu_level console variable
    // (0 minimum spec, 1 medium, 2 high).
    let host = cvm.host.clone();
    vm.register_checked("GetCPULevel", 1, ".", move |_, _| {
        let v: f32 = host
            .convar("particle_cpu_level")
            .trim()
            .parse()
            .unwrap_or(0.0);
        Ok(Value::Int(v as i32))
    });
    // VPKNotifyFile( path ) (0x1802c1f70): a hint for building VPKs.
    vm.register("VPKNotifyFile", 2, |_, _| Ok(Value::Null));
    // Durango_IsDurango (0x180449bc0): the Xbox One build only.
    vm.register("Durango_IsDurango", 1, |_, _| Ok(Value::Bool(false)));

    // IsPrivateMatch (0x180449af0); its error names its old name.
    let host = cvm.host.clone();
    vm.register("IsPrivateMatch", 1, move |_, _| {
        if !host.is_connected() {
            return Err(throw("AreWeMatchmaking: Not connected to a server"));
        }
        Ok(Value::Bool(host.is_private_match()))
    });

    // ClearItemTypes / RegisterItemType( type, name ) (0x1802e89b0/c0): the
    // names `bot_loadout` completes, per item type.
    let items: Rc<RefCell<Vec<(i32, String)>>> = Default::default();
    let it = items.clone();
    vm.register("ClearItemTypes", 1, move |_, _| {
        it.borrow_mut().clear();
        Ok(Value::Null)
    });
    vm.register("RegisterItemType", 3, move |_, a| {
        let t = match a[1] {
            Value::Int(n) => n,
            Value::Float(x) => x as i32,
            _ => 0,
        };
        items.borrow_mut().push((t, name_arg(a, 2)));
        Ok(Value::Null)
    });

    // PrecachePilotImpactEffectTables( land ) (0x1802c4f50) and
    // PrecacheTitanImpactEffectTables( footstep, land, dodge ) (0x1802c4ed0):
    // an empty name means none (-1).
    let tables = Rc::new(RefCell::new(ImpactTables {
        pilot_land: -1,
        titan_footstep: -1,
        titan_land: -1,
        titan_dodge: -1,
    }));
    let host = cvm.host.clone();
    let t = tables.clone();
    vm.register("PrecachePilotImpactEffectTables", 2, move |_, a| {
        let n = name_arg(a, 1);
        t.borrow_mut().pilot_land = if n.is_empty() {
            -1
        } else {
            host.precache_impact_table(&n)
        };
        Ok(Value::Null)
    });
    let host = cvm.host.clone();
    vm.register("PrecacheTitanImpactEffectTables", 4, move |_, a| {
        let index = |i: usize| {
            let n = name_arg(a, i);
            if n.is_empty() {
                -1
            } else {
                host.precache_impact_table(&n)
            }
        };
        let mut t = tables.borrow_mut();
        t.titan_footstep = index(1);
        t.titan_land = index(2);
        t.titan_dodge = index(3);
        Ok(Value::Null)
    });

    // Graph( v, a, b, c, d ) (0x1802c4480): v mapped from [a, b] onto
    // [c, d]; GraphCapped (0x1802c44d0) clamps to [c, d]. Five numbers
    // (mask ".nnnnn"), worked in 32-bit floats.
    vm.register_checked("Graph", 6, ".nnnnn", |_, a| {
        let [v, a0, b0, c, d] = [1, 2, 3, 4, 5].map(|i| num(a, i));
        Ok(Value::Float(graph(v, a0, b0, c, d, false)))
    });
    vm.register_checked("GraphCapped", 6, ".nnnnn", |_, a| {
        let [v, a0, b0, c, d] = [1, 2, 3, 4, 5].map(|i| num(a, i));
        Ok(Value::Float(graph(v, a0, b0, c, d, true)))
    });

    // GetParticleSystemIndex( name ) (0x18013d590): 0 if it isn't loaded.
    let host = cvm.host.clone();
    vm.register("GetParticleSystemIndex", 2, move |_, a| {
        Ok(Value::Int(host.particle_system_index(&name_arg(a, 1))))
    });
}

fn num(a: &[Value], i: usize) -> f32 {
    match a.get(i) {
        Some(Value::Int(n)) => *n as f32,
        Some(Value::Float(x)) => *x,
        _ => 0.0,
    }
}

/// `Graph`/`GraphCapped`. With a == b (or either not a number, which the
/// game's compare also treats as equal) the result is d when v >= b, else
/// c. The clamp keeps 0..1 and turns not-a-number into 0, like the SSE
/// max/min the game uses.
pub fn graph(v: f32, a: f32, b: f32, c: f32, d: f32, capped: bool) -> f32 {
    if a == b || a.is_nan() || b.is_nan() {
        return if v - b >= 0.0 { d } else { c };
    }
    let mut t = (v - a) / (b - a);
    if capped {
        t = if t > 0.0 { t } else { 0.0 };
        t = if t < 1.0 { t } else { 1.0 };
    }
    (d - c) * t + c
}

#[cfg(test)]
mod tests {
    use super::graph;

    #[test]
    fn graph_maps_and_caps() {
        assert_eq!(graph(0.5, 0.0, 1.0, 300.0, 500.0, false), 400.0);
        assert_eq!(graph(2.0, 0.0, 1.0, 300.0, 500.0, false), 700.0);
        assert_eq!(graph(2.0, 0.0, 1.0, 300.0, 500.0, true), 500.0);
        assert_eq!(graph(-1.0, 0.0, 1.0, 300.0, 500.0, true), 300.0);
        // a == b: a step at b.
        assert_eq!(graph(1.0, 1.0, 1.0, 3.0, 5.0, false), 5.0);
        assert_eq!(graph(0.9, 1.0, 1.0, 3.0, 5.0, false), 3.0);
    }
}
