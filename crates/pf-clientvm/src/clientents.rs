//! Entities client script makes for itself (client-only entities): the
//! state script can change on them, and the functions that make them.
//! Ported from client.dll; each function's address is beside it.

use std::rc::Rc;

use pf_squirrel::table::Table;
use pf_squirrel::{Value, throw};

use crate::entities::{Entities, Shared, entity_id, this};
use crate::{ClientVm, EntityInfo};

/// What client script can change on a client-only entity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientOnlyState {
    /// Local players (a bit per player) it's explicitly shown to / hidden
    /// from (`SetVisibleForLocalPlayer`, `SetInvisibleForLocalPlayer`).
    pub visible_for: u32,
    pub invisible_for: u32,
    pub point_camera: Option<PointCamera>,
}

/// A point camera (C_PointCamera): renders the world from its place, for
/// a screen in the world.
#[derive(Clone, Debug, PartialEq)]
pub struct PointCamera {
    pub fov: f32,
    pub active: bool,
    pub fog_enable: bool,
}

fn num(a: &[Value], i: usize) -> f32 {
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

pub fn install(cvm: &mut ClientVm, ents: &Shared) {
    // CreateClientSidePointCamera( origin, angles, fov ) (0x1802b5390):
    // a new point camera there, not active yet.
    let (host, e2) = (cvm.host.clone(), ents.clone());
    cvm.vm
        .register_checked("CreateClientSidePointCamera", 4, ".xxn", move |vm, a| {
            let origin = pf_squirrel::vector::get(&a[1]).unwrap_or_default();
            let angles = pf_squirrel::vector::get(&a[2]).unwrap_or_default();
            let id = Entities::create_client_only(
                &e2,
                EntityInfo {
                    script_class: "C_PointCamera".into(),
                    classname: "point_camera".into(),
                    signifier: "point_camera".into(),
                    origin,
                    local_origin: origin,
                    angles,
                    ..Default::default()
                },
            );
            if let Some(s) = e2.borrow_mut().client_state.get_mut(&id) {
                s.point_camera = Some(PointCamera {
                    fov: num(a, 3),
                    active: false,
                    fog_enable: false,
                });
            }
            Ok(Entities::instance(&e2, vm, &*host, id))
        });
}

/// C_BaseEntity methods that change client-only state.
pub fn base_methods(cvm: &ClientVm, ents: &Shared, members: &mut Table) {
    // SetVisibleForLocalPlayer / SetInvisibleForLocalPlayer( player )
    // (0x1802f50c0, 0x1802f5100): set the player's bit in one mask and
    // clear it in the other (the shift uses the low 5 bits).
    for (name, visible) in [
        ("SetVisibleForLocalPlayer", true),
        ("SetInvisibleForLocalPlayer", false),
    ] {
        let (host, sh) = (cvm.host.clone(), ents.clone());
        members.insert(
            Value::str(name),
            pf_squirrel::native_checked(name, 2, ".n", move |_, a| {
                let e = this(&sh, &*host, a, name)?;
                if !e.client_only {
                    return Err(throw(format!(
                        "You can only call {name}() on client-side entities"
                    )));
                }
                let bit = 1u32 << (int(a, 1) as u32 & 31);
                let id = entity_id(&a[0]).unwrap_or_default();
                if let Some(s) = sh.borrow_mut().client_state.get_mut(&id) {
                    if visible {
                        s.visible_for |= bit;
                        s.invisible_for &= !bit;
                    } else {
                        s.invisible_for |= bit;
                        s.visible_for &= !bit;
                    }
                }
                Ok(Value::Null)
            }),
        );
    }
}

/// C_PointCamera's methods (0x1800d0150 on): client-only cameras only.
pub fn point_camera_methods(cvm: &ClientVm, ents: &Shared) -> Table {
    let mut t = Table::new();
    type Set = fn(&mut PointCamera, &[Value]);
    let methods: [(&str, &str, Set); 3] = [
        ("SetActive", ".b", |c, a| {
            c.active = matches!(a[1], Value::Bool(true))
        }),
        ("SetFOV", ".n", |c, a| c.fov = num(a, 1)),
        ("SetFogEnable", ".b", |c, a| {
            c.fog_enable = matches!(a[1], Value::Bool(true))
        }),
    ];
    for (name, mask, set) in methods {
        let (host, sh) = (cvm.host.clone(), Rc::clone(ents));
        t.insert(
            Value::str(name),
            pf_squirrel::native_checked(name, 2, mask, move |_, a| {
                let e = this(&sh, &*host, a, name)?;
                if !e.client_only {
                    return Err(throw(format!(
                        "You cannot call {name}() on a server entity"
                    )));
                }
                let id = entity_id(&a[0]).unwrap_or_default();
                if let Some(c) = sh
                    .borrow_mut()
                    .client_state
                    .get_mut(&id)
                    .and_then(|s| s.point_camera.as_mut())
                {
                    set(c, a);
                }
                Ok(Value::Null)
            }),
        );
    }
    t
}
