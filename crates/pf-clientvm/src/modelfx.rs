//! Model effects data (`ModelFX_*`): client script describes, per model,
//! groups of particle effects on attachment tags (spawn effects, effects
//! by health, damage and break effects, break gibs). The game keeps these
//! definitions and plays them on entities with that model; here they're
//! stored for the effect code to read. Ported from client.dll
//! (0x18015a4e0 and the functions after it; docs/port/scripts.md,
//! "Model effects").

use std::cell::RefCell;
use std::rc::Rc;

use pf_squirrel::{Value, throw};

use crate::ClientVm;

/// Who sees a group's effects (`visibleTo`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Visibility {
    #[default]
    All,
    Friend,
    Foe,
    NotLocalPlayer,
}

/// A particle effect by name, with the index it had when it was added
/// (0 if it wasn't loaded).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fx {
    pub name: String,
    pub index: i32,
}

/// An effect shown while health is at or below `health_frac`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HealthFx {
    pub health_frac: f32,
    pub fx: Fx,
    pub one_shot_only: bool,
    pub sound: String,
    pub one_shot_sound: String,
}

/// A damage effect by health and state. A looping one has no delays
/// (both -1).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HealthDamageFx {
    pub health_frac: f32,
    pub fx: Fx,
    pub state_index: i32,
    pub min_delay: f32,
    pub max_delay: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DamageFx {
    pub fx: Fx,
    pub state_index: i32,
    pub min_delay: f32,
    pub max_delay: f32,
    pub sound: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BreakFx {
    pub fx: Fx,
    pub state_index: i32,
    pub sound: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BreakGib {
    pub state_index: i32,
    pub model: String,
    pub gib_type: i32,
    pub min_velocity: f32,
    pub max_velocity: f32,
}

/// Everything on one attachment tag.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TagFx {
    pub tag: String,
    pub spawn_fx: Option<Fx>,
    /// Kept in the order added; each must have a lower `health_frac` than
    /// the one before.
    pub health_fx: Vec<HealthFx>,
    pub health_damage_fx: Vec<HealthDamageFx>,
    pub damage_fx: Vec<DamageFx>,
    pub break_fx: Vec<BreakFx>,
    pub break_gibs: Vec<BreakGib>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FxGroup {
    pub name: String,
    pub start_active: bool,
    pub visibility: Visibility,
    /// Set by `ModelFX_AddTagHealthFX*`.
    pub has_health_fx: bool,
    /// Set by `ModelFX_AddTagSpawnFX`.
    pub has_spawn_fx: bool,
    pub tags: Vec<TagFx>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelFx {
    pub model: String,
    /// Set by every `ModelFX_AddTag*` except the spawn effect.
    pub has_state_fx: bool,
    /// Set by a break effect or gib with state index -1.
    pub has_whole_break: bool,
    /// The one group with the state (damage and break) effects, an index
    /// into `groups`.
    pub state_group: Option<usize>,
    /// Kept in the order made. Beginning a group name a second time adds a
    /// second group of that name, as the game does.
    pub groups: Vec<FxGroup>,
}

#[derive(Default)]
pub struct ModelFxData {
    pub models: Vec<ModelFx>,
    /// The definition being written: (model, group).
    current: Option<(usize, usize)>,
}

pub type Shared = Rc<RefCell<ModelFxData>>;

/// Model names, group names and tags are matched ignoring ASCII case.
fn find<T>(list: &[T], name: &str, key: impl Fn(&T) -> &str) -> Option<usize> {
    list.iter().position(|x| key(x).eq_ignore_ascii_case(name))
}

/// The game copies names into fixed buffers: at most `size - 1` bytes.
fn clip(s: &str, size: usize) -> String {
    let mut n = s.len().min(size - 1);
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    s[..n].to_string()
}

impl ModelFxData {
    /// The effects of a model, if any were defined.
    pub fn model(&self, name: &str) -> Option<&ModelFx> {
        find(&self.models, name, |m| &m.model).map(|i| &self.models[i])
    }

    /// Forget everything (the client VM shutting down, 0x18015a400).
    pub fn clear(&mut self) {
        self.models.clear();
        self.current = None;
    }

    /// The current group's tag, made if new (0x18015a6f0).
    fn tag(&mut self, tag: &str) -> Option<(&mut ModelFx, usize, usize)> {
        let (m, g) = self.current?;
        let model = &mut self.models[m];
        let tags = &mut model.groups[g].tags;
        let t = find(tags, tag, |t| &t.tag).unwrap_or_else(|| {
            tags.push(TagFx {
                tag: tag.to_string(),
                ..Default::default()
            });
            tags.len() - 1
        });
        Some((model, g, t))
    }
}

/// The checks the state effects share: only one group of a model may have
/// them.
fn claim_state_group(model: &mut ModelFx, group: usize) -> Result<(), String> {
    let other = model.state_group.is_some_and(|g| g != group);
    model.state_group = Some(group);
    if other {
        return Err("ModelFX_AddTagBreakFX: only one group should have body group fx.".into());
    }
    Ok(())
}

fn s(a: &[Value], i: usize) -> String {
    match a.get(i) {
        Some(Value::Str(s)) => s.to_string_lossy(),
        _ => String::new(),
    }
}

fn f(a: &[Value], i: usize) -> f32 {
    match a.get(i) {
        Some(Value::Int(n)) => *n as f32,
        Some(Value::Float(x)) => *x,
        _ => 0.0,
    }
}

fn int(a: &[Value], i: usize) -> i32 {
    match a.get(i) {
        Some(Value::Int(n)) => *n,
        Some(Value::Float(x)) => *x as i32,
        _ => 0,
    }
}

fn b(a: &[Value], i: usize) -> bool {
    matches!(a.get(i), Some(Value::Bool(true)))
}

/// Without a `ModelFX_BeginData` first the game would crash (it writes
/// through an empty pointer); we raise an error instead (ours).
const NO_BEGIN: &str = "ModelFX: no ModelFX_BeginData before this";

pub fn install(cvm: &mut ClientVm) -> Shared {
    let data: Shared = Rc::default();
    let vm = &mut cvm.vm;

    // The type masks are the ones launcher.dll builds from each function's
    // parameter types: `.` for `this`, `s` string, `n` number, `b` bool.

    // ModelFX_BeginData( groupName, modelName, visibleTo, startActive )
    // (0x18015a4e0).
    let d = data.clone();
    vm.register_checked("ModelFX_BeginData", 5, ".sssb", move |_, a| {
        let mut d = d.borrow_mut();
        if d.current.is_some() {
            return Err(throw(
                "ModelFX_BeginData: previous BeginData not have matching EndData",
            ));
        }
        let model = s(a, 2);
        let m = find(&d.models, &model, |m| &m.model).unwrap_or_else(|| {
            d.models.push(ModelFx {
                model: model.clone(),
                ..Default::default()
            });
            d.models.len() - 1
        });
        let vis = s(a, 3);
        let visibility = match vis.as_str() {
            "all" => Some(Visibility::All),
            "notLocalPlayer" => Some(Visibility::NotLocalPlayer),
            "friend" => Some(Visibility::Friend),
            "foe" => Some(Visibility::Foe),
            _ => None,
        };
        let groups = &mut d.models[m].groups;
        groups.push(FxGroup {
            name: s(a, 1),
            start_active: b(a, 4),
            visibility: visibility.unwrap_or_default(),
            ..Default::default()
        });
        let g = groups.len() - 1;
        d.current = Some((m, g));
        // The group stays (seen by everyone) even when the name is wrong.
        if visibility.is_none() {
            return Err(throw(format!(
                "ModelFX_BeginData: invalid visibility {vis}, should be 'all', 'notLocalPlayer', 'friend', or 'foe'"
            )));
        }
        Ok(Value::Null)
    });

    // ModelFX_EndData (0x18014fe70).
    let d = data.clone();
    vm.register_checked("ModelFX_EndData", 1, ".", move |_, _| {
        d.borrow_mut().current = None;
        Ok(Value::Null)
    });

    // Effect names are copied into 32-byte buffers, the gib model into 64.
    // Adding an effect loads it (when it isn't a dedicated server), then
    // keeps its index.
    let host = cvm.host.clone();
    let fx = move |name: &str| {
        host.precache_particle_system(name);
        Fx {
            name: clip(name, 32),
            index: if name.is_empty() {
                0
            } else {
                host.particle_system_index(name)
            },
        }
    };
    let fx = Rc::new(fx);

    // ModelFX_AddTagSpawnFX( tagName, fxName ) (0x18015a760): replaces the
    // tag's spawn effect.
    let (d, fxf) = (data.clone(), fx.clone());
    vm.register_checked("ModelFX_AddTagSpawnFX", 3, ".ss", move |_, a| {
        let mut d = d.borrow_mut();
        let (model, g, t) = d.tag(&s(a, 1)).ok_or_else(|| throw(NO_BEGIN))?;
        let group = &mut model.groups[g];
        group.tags[t].spawn_fx = Some(fxf(&s(a, 2)));
        group.has_spawn_fx = true;
        Ok(Value::Null)
    });

    // ModelFX_AddTagHealthFX( healthFrac, tagName, fxName, oneShotOnly )
    // (0x18015a800) and ModelFX_AddTagHealthFXAndSound( ..., soundName,
    // oneShotSoundName ) (0x18015a9a0). Each must have a lower healthFrac
    // than the tag's previous one; one that doesn't is still added.
    for (name, n, mask) in [
        ("ModelFX_AddTagHealthFX", 5, ".nssb"),
        ("ModelFX_AddTagHealthFXAndSound", 7, ".nssbss"),
    ] {
        let (d, fxf) = (data.clone(), fx.clone());
        vm.register_checked(name, n, mask, move |_, a| {
            let mut d = d.borrow_mut();
            let (tag, fx_name) = (s(a, 2), s(a, 3));
            let (model, g, t) = d.tag(&tag).ok_or_else(|| throw(NO_BEGIN))?;
            let frac = f(a, 1);
            let list = &mut model.groups[g].tags[t].health_fx;
            let out_of_order = list.last().is_some_and(|l| l.health_frac <= frac);
            list.push(HealthFx {
                health_frac: frac,
                fx: fxf(&fx_name),
                one_shot_only: b(a, 4),
                sound: clip(&s(a, 5), 32),
                one_shot_sound: clip(&s(a, 6), 32),
            });
            model.has_state_fx = true;
            model.groups[g].has_health_fx = true;
            if out_of_order {
                return Err(throw(format!(
                    "{name}: adding an effect '{fx_name}' to tag '{tag}' with healthFrac {frac:.3} <= previous"
                )));
            }
            Ok(Value::Null)
        });
    }

    // ModelFX_AddTagBreakFX( stateIndex, tagName, fxName, soundName )
    // (0x18015ab60).
    let (d, fxf) = (data.clone(), fx.clone());
    vm.register_checked("ModelFX_AddTagBreakFX", 5, ".nsss", move |_, a| {
        let mut d = d.borrow_mut();
        let (model, g, t) = d.tag(&s(a, 2)).ok_or_else(|| throw(NO_BEGIN))?;
        let state = int(a, 1);
        let item = BreakFx {
            fx: fxf(&s(a, 3)),
            state_index: state,
            sound: clip(&s(a, 4), 32),
        };
        if state == -1 {
            model.has_whole_break = true;
        }
        model.has_state_fx = true;
        model.groups[g].tags[t].break_fx.push(item);
        claim_state_group(model, g).map_err(throw)?;
        Ok(Value::Null)
    });

    // ModelFX_AddTagBreakGib( stateIndex, tagName, modelName, gibType,
    // minVelocity, maxVelocity ) (0x18015ad00).
    let d = data.clone();
    vm.register_checked("ModelFX_AddTagBreakGib", 7, ".nssnnn", move |_, a| {
        let mut d = d.borrow_mut();
        let (model, g, t) = d.tag(&s(a, 2)).ok_or_else(|| throw(NO_BEGIN))?;
        let state = int(a, 1);
        let item = BreakGib {
            state_index: state,
            model: clip(&s(a, 3), 64),
            gib_type: int(a, 4),
            min_velocity: f(a, 5),
            max_velocity: f(a, 6),
        };
        if state == -1 {
            model.has_whole_break = true;
        }
        model.has_state_fx = true;
        model.groups[g].tags[t].break_gibs.push(item);
        claim_state_group(model, g).map_err(throw)?;
        Ok(Value::Null)
    });

    // ModelFX_AddTagDamageFX( stateIndex, tagName, fxName, mindelay,
    // maxdelay, soundName ) (0x18015aea0).
    let (d, fxf) = (data.clone(), fx.clone());
    vm.register_checked("ModelFX_AddTagDamageFX", 7, ".nssnns", move |_, a| {
        let mut d = d.borrow_mut();
        let (model, g, t) = d.tag(&s(a, 2)).ok_or_else(|| throw(NO_BEGIN))?;
        let item = DamageFx {
            fx: fxf(&s(a, 3)),
            state_index: int(a, 1),
            min_delay: f(a, 4),
            max_delay: f(a, 5),
            sound: clip(&s(a, 6), 32),
        };
        model.groups[g].tags[t].damage_fx.push(item);
        model.has_state_fx = true;
        claim_state_group(model, g).map_err(throw)?;
        Ok(Value::Null)
    });

    // ModelFX_AddTagHealthDamageFX( stateIndex, healthFrac, tagName, fxName,
    // mindelay, maxdelay ) (0x18015b030) and ModelFX_AddTagHealthDamageFXLoop
    // ( stateIndex, healthFrac, tagName, fxName ) (0x18015b1d0, delays -1).
    // Both report an order error under the first one's name.
    for (name, n, mask) in [
        ("ModelFX_AddTagHealthDamageFX", 7, ".nnssnn"),
        ("ModelFX_AddTagHealthDamageFXLoop", 5, ".nnss"),
    ] {
        let (d, fxf) = (data.clone(), fx.clone());
        let looping = n == 5;
        vm.register_checked(name, n, mask, move |_, a| {
            let mut d = d.borrow_mut();
            let (tag, fx_name) = (s(a, 3), s(a, 4));
            let (model, g, t) = d.tag(&tag).ok_or_else(|| throw(NO_BEGIN))?;
            let frac = f(a, 2);
            let list = &mut model.groups[g].tags[t].health_damage_fx;
            let out_of_order = list.last().is_some_and(|l| l.health_frac <= frac);
            let (min_delay, max_delay) = if looping {
                (-1.0, -1.0)
            } else {
                (f(a, 5), f(a, 6))
            };
            list.push(HealthDamageFx {
                health_frac: frac,
                fx: fxf(&fx_name),
                state_index: int(a, 1),
                min_delay,
                max_delay,
            });
            model.has_state_fx = true;
            // A second error in one call replaces the first.
            claim_state_group(model, g).map_err(throw)?;
            if out_of_order {
                return Err(throw(format!(
                    "ModelFX_AddTagHealthDamageFX: adding an effect '{fx_name}' to tag '{tag}' with healthFrac {frac:.3} <= previous"
                )));
            }
            Ok(Value::Null)
        });
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_clipped_like_the_game() {
        assert_eq!(clip("abc", 32), "abc");
        assert_eq!(clip(&"x".repeat(40), 32).len(), 31);
    }
}
