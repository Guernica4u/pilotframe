//! Controller rumble definitions (`Rumble_CreateGraph`,
//! `Rumble_CreatePlayParams`): named graphs of motor power over time, and
//! named play settings that point at a graph. Stored here for the rumble
//! player. Ported from client.dll (0x180442300, 0x180440c20).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use pf_squirrel::{Value, Vm, throw};

use crate::ClientVm;

/// Four graphs of (time, power) points, one per motor.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RumbleGraph {
    pub heavy_motor: Vec<[f32; 2]>,
    pub light_motor: Vec<[f32; 2]>,
    pub left_trigger: Vec<[f32; 2]>,
    pub right_trigger: Vec<[f32; 2]>,
}

/// How to play a graph. Missing fields keep these defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct RumblePlayParams {
    /// The graph's name.
    pub name: String,
    /// Where it plays from; all `f32::MAX` (the engine's "no position")
    /// when not set.
    pub position: [f32; 3],
    pub scale: f32,
    pub scale_min: f32,
    pub scale_max: f32,
    pub distance_min: f32,
    pub distance_max: f32,
    pub looping: bool,
}

impl Default for RumblePlayParams {
    fn default() -> Self {
        RumblePlayParams {
            name: String::new(),
            position: [f32::MAX; 3],
            scale: 1.0,
            scale_min: 1.0,
            scale_max: 1.0,
            distance_min: 1.0,
            distance_max: 1.0,
            looping: false,
        }
    }
}

/// Graphs and play params by name (case-sensitive; a second definition
/// replaces the first). Cleared when the client VM shuts down.
#[derive(Default)]
pub struct Rumbles {
    pub graphs: HashMap<String, RumbleGraph>,
    pub play_params: HashMap<String, RumblePlayParams>,
}

pub type Shared = Rc<RefCell<Rumbles>>;

/// The engine's error text drops one trailing line break (client.dll
/// 0x1802c4fe0); these messages all end in one.
fn fail(msg: impl AsRef<str>) -> pf_squirrel::NativeError {
    throw(msg.as_ref().trim_end_matches('\n'))
}

/// An array of Vectors as (x, y) points; the bad index if one isn't a
/// Vector.
fn points(v: &Value) -> Result<Vec<[f32; 2]>, usize> {
    let Value::Array(a) = v else {
        return Ok(Vec::new());
    };
    a.borrow()
        .iter()
        .enumerate()
        .map(|(i, p)| pf_squirrel::vector::get(p).map(|v| [v[0], v[1]]).ok_or(i))
        .collect()
}

/// A field of the params table: None if it's missing.
fn field(vm: &mut Vm, t: &Value, key: &str) -> Option<Value> {
    // The engine's table read: a missing key is skipped, not an error.
    vm.try_get(t, &Value::str(key)).ok().flatten()
}

pub fn install(cvm: &mut ClientVm) -> Shared {
    let data: Shared = Rc::default();
    let vm = &mut cvm.vm;

    // Rumble_CreateGraph( name, heavyMotor, lightMotor, leftTrigger,
    // rightTrigger ): registered with the mask ".saaaa", then each check
    // made again in the function.
    let d = data.clone();
    vm.register_checked("Rumble_CreateGraph", 6, ".saaaa", move |_, a| {
        let name = match &a[1] {
            Value::Str(s) => s.to_string_lossy(),
            _ => {
                return Err(fail(
                    "Rumble_CreateGraph: First parameter must be a string (name of the graph)\n",
                ));
            }
        };
        let ordinals = ["Second", "Third", "Fourth", "Fifth"];
        let motors = ["leftMotor", "rightMotor", "leftTrigger", "rightTrigger"];
        let mut graphs: Vec<Vec<[f32; 2]>> = Vec::new();
        for i in 0..4 {
            if !matches!(a[2 + i], Value::Array(_)) {
                return Err(fail(format!(
                    "Rumble_CreateGraph: {} parameter must be an array (of 2d vectors)\n",
                    ordinals[i]
                )));
            }
            match points(&a[2 + i]) {
                Ok(p) => graphs.push(p),
                Err(bad) => {
                    return Err(fail(format!(
                        "Rumble_CreateGraph: {}[{bad}] is not a 2d vector\n",
                        motors[i]
                    )));
                }
            }
        }
        let mut g = graphs.into_iter();
        let graph = RumbleGraph {
            heavy_motor: g.next().unwrap_or_default(),
            light_motor: g.next().unwrap_or_default(),
            left_trigger: g.next().unwrap_or_default(),
            right_trigger: g.next().unwrap_or_default(),
        };
        d.borrow_mut().graphs.insert(name, graph);
        Ok(Value::Null)
    });

    // Rumble_CreatePlayParams( name, params ): mask ".st".
    let d = data.clone();
    vm.register_checked("Rumble_CreatePlayParams", 3, ".st", move |vm, a| {
        let Value::Str(name) = &a[1] else {
            return Err(fail(
                "Rumble_CreatePlayParams: First parameter must be a string (name of the play params to create)\n",
            ));
        };
        let name = name.to_string_lossy();
        let t = &a[2];
        if !matches!(t, Value::Table(_)) {
            return Err(fail(
                "Rumble_CreatePlayParams: Second parameter must be a table (of param values)\n",
            ));
        }
        let mut p = RumblePlayParams::default();
        if let Some(v) = field(vm, t, "name") {
            let Value::Str(s) = v else {
                return Err(fail(
                    "Rumble_CreatePlayParams: Failed to get name string from the \"name\" field in the passed in params\n",
                ));
            };
            p.name = s.to_string_lossy();
        }
        if let Some(v) = field(vm, t, "position") {
            p.position = pf_squirrel::vector::get(&v).ok_or_else(|| {
                fail("Rumble_CreatePlayParams: Failed to get position vector from the \"position\" field in the passed in params\n")
            })?;
        }
        for (key, slot) in [
            ("scale", &mut p.scale),
            ("scaleMin", &mut p.scale_min),
            ("scaleMax", &mut p.scale_max),
            ("distanceMin", &mut p.distance_min),
            ("distanceMax", &mut p.distance_max),
        ] {
            if let Some(v) = field(vm, t, key) {
                *slot = match v {
                    Value::Int(n) => n as f32,
                    Value::Float(x) => x,
                    _ => {
                        return Err(fail(format!(
                            "Rumble_CreatePlayParams: Failed to get float from the \"{key}\" field in the passed in params\n"
                        )));
                    }
                };
            }
        }
        if let Some(v) = field(vm, t, "loop") {
            let Value::Bool(b) = v else {
                return Err(fail(
                    "Rumble_CreatePlayParams: Failed to get boolean from the \"loop\" field in the passed in params\n",
                ));
            };
            p.looping = b;
        }
        d.borrow_mut().play_params.insert(name, p);
        Ok(Value::Null)
    });
    data
}
