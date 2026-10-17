//! Entities in client script: each entity has one script instance (so `==`
//! works), of the script class client.dll registers for its C++ class. The
//! classes and their bases come from client.dll's class descriptors
//! (Resources/re/scripts/classnatives.py); methods are ported one by one.
//! A method called on an entity that's gone fails as in the game:
//! "Attempted to call NAME on invalid entity (no context)".

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use pf_squirrel::table::Table;
use pf_squirrel::value::{Class, Ref, UserData, new_ref};
use pf_squirrel::{NativeError, Value, Vm, native, throw};

use crate::{ClientVm, EntityId, EntityInfo, Host};

/// The entity script classes and their bases, base classes first.
pub const CLASSES: &[(&str, Option<&str>)] = &[
    ("C_BaseEntity", None),
    ("C_BaseAnimating", Some("C_BaseEntity")),
    ("C_BaseCombatCharacter", Some("C_BaseAnimating")),
    ("C_BasePlayer", Some("C_BaseCombatCharacter")),
    ("C_Portal_Player", Some("C_BasePlayer")),
    ("C_AI_BaseNPC", Some("C_BaseCombatCharacter")),
    ("C_NPC_Titan", Some("C_AI_BaseNPC")),
    ("C_NPC_CombineDropship", Some("C_AI_BaseNPC")),
    ("C_NPC_SentryTurret", Some("C_AI_BaseNPC")),
    ("C_Titan_Driveable", Some("C_BaseCombatCharacter")),
    ("C_Projectile", Some("C_BaseCombatCharacter")),
    ("C_BaseGrenade", Some("C_Projectile")),
    ("C_Missile", Some("C_Projectile")),
    ("C_BaseCombatWeapon", Some("C_BaseAnimating")),
    ("C_WeaponX", Some("C_BaseCombatWeapon")),
    ("C_Titan_Cockpit", Some("C_BaseAnimating")),
    ("C_FirstPersonProxy", Some("C_BaseAnimating")),
    ("ScriptMover", Some("C_BaseAnimating")),
    ("C_TitanSoul", Some("C_BaseEntity")),
    ("C_DynamicLight", Some("C_BaseEntity")),
    ("C_CascadeLight", Some("C_BaseEntity")),
    ("C_PointCamera", Some("C_BaseEntity")),
    ("C_HardPointEntity", Some("C_BaseEntity")),
    ("C_VGuiScreen", Some("C_BaseEntity")),
    ("C_WindowHint", Some("C_BaseEntity")),
];

/// Life states (the root-table constants `LIFE_*`).
pub const LIFE_ALIVE: i32 = 0;

/// Client-only entities (made by client script, e.g. VGUI screens) get ids
/// from here up, out of the host's way.
pub const CLIENT_ONLY_BASE: EntityId = 0x4000_0000;

/// Script classes and the one instance per entity.
pub struct Entities {
    pub classes: HashMap<&'static str, Ref<Class>>,
    instances: HashMap<EntityId, Value>,
    /// Entities that exist only in client script.
    pub client_only: HashMap<EntityId, EntityInfo>,
    next_client_only: EntityId,
    /// Script scopes made by `ValidateScriptScope`.
    scopes: HashMap<EntityId, Value>,
    /// The unique-key serial number (launcher.dll's, counted per VM).
    key_serial: u64,
}

pub type Shared = Rc<RefCell<Entities>>;

impl Entities {
    /// The entity's script instance (made on first use), or null if the
    /// entity doesn't exist.
    pub fn instance(this: &Shared, vm: &mut Vm, host: &dyn Host, id: EntityId) -> Value {
        if let Some(v) = this.borrow().instances.get(&id) {
            return v.clone();
        }
        let Some(info) = Entities::info(this, host, id) else {
            return Value::Null;
        };
        let class = {
            let e = this.borrow();
            e.classes
                .get(info.script_class.as_str())
                .or_else(|| e.classes.get("C_BaseEntity"))
                .cloned()
        };
        let Some(class) = class else {
            return Value::Null;
        };
        // Stored before the constructor runs, so a constructor that asks
        // for its own entity gets this instance.
        let v = match crate::classes::instantiate(vm, &class, crate::classes::user(id)) {
            Ok(v) => v,
            Err(e) => {
                let msg = vm.to_display(&e);
                (vm.out)(&format!(
                    "Failed to CreateScriptInstance for {}.  Check log for script errors in the class's constructor.\nSCRIPT ERROR: {msg}\n",
                    info.classname
                ));
                return Value::Null;
            }
        };
        this.borrow_mut().instances.insert(id, v.clone());
        v
    }

    /// An entity's state: client-only ones first, then the host's.
    pub fn info(this: &Shared, host: &dyn Host, id: EntityId) -> Option<EntityInfo> {
        if id >= CLIENT_ONLY_BASE {
            return this.borrow().client_only.get(&id).cloned();
        }
        host.entity(id)
    }

    /// Make a client-only entity; its id.
    pub fn create_client_only(this: &Shared, mut info: EntityInfo) -> EntityId {
        let mut e = this.borrow_mut();
        let id = CLIENT_ONLY_BASE + e.next_client_only;
        e.next_client_only += 1;
        info.client_only = true;
        e.client_only.insert(id, info);
        id
    }

    /// Forget an entity's instance (it was destroyed). Its scope stays in
    /// the root table: releasing it is still to be ported.
    pub fn forget(this: &Shared, id: EntityId) {
        let mut e = this.borrow_mut();
        e.instances.remove(&id);
        e.scopes.remove(&id);
    }

    /// The entity's scope (client.dll 0x18002a710): made on first use by
    /// init.nut's `VSquirrel_OnCreateScope( key, root )`, under a new
    /// unique key, with `self` set to the entity. Null if the instance or
    /// the scope can't be made.
    pub fn validate_scope(this: &Shared, vm: &mut Vm, host: &dyn Host, id: EntityId) -> Value {
        if let Some(s) = this.borrow().scopes.get(&id) {
            return s.clone();
        }
        let inst = Entities::instance(this, vm, host, id);
        let Some(info) = Entities::info(this, host, id) else {
            return Value::Null;
        };
        // The key's root: the entity's name, or its class name.
        let root_name = if info.name.is_empty() {
            &info.classname
        } else {
            &info.name
        };
        let key = {
            let mut e = this.borrow_mut();
            e.key_serial += 1;
            unique_key(e.key_serial, host.time(), root_name)
        };
        let root = Value::Table(vm.root.clone());
        let Some(create) = vm
            .try_get(&root, &Value::str("VSquirrel_OnCreateScope"))
            .ok()
            .flatten()
        else {
            return Value::Null;
        };
        let scope = match vm.call(&create, root.clone(), &[Value::str(&key), root]) {
            Ok(s @ Value::Table(_)) => s,
            Ok(_) => return Value::Null,
            Err(e) => {
                vm.report_error(&e);
                return Value::Null;
            }
        };
        if let Value::Table(t) = &scope {
            t.borrow_mut().insert(Value::str("self"), inst);
        }
        this.borrow_mut().scopes.insert(id, scope.clone());
        scope
    }
}

/// launcher.dll's GenerateUniqueKey (0x1800099d0): `%x%x%llx_%s` of a
/// random number below 0x1000, the clock in milliseconds and the serial
/// number. The game's random number and clock make these keys differ
/// every run; ours uses game time and a hash of the serial (ours).
fn unique_key(serial: u64, time: f32, root: &str) -> String {
    let random = (serial.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 52) as u32 & 0xfff;
    let ms = (time * 1000.0) as u32;
    format!("{random:x}{ms:x}{serial:x}_{root}")
}

/// The entity an instance stands for.
pub fn entity_id(v: &Value) -> Option<EntityId> {
    let Value::Instance(i) = v else { return None };
    match &i.borrow().user {
        UserData::Host(rc) => rc.downcast_ref::<EntityId>().copied(),
        _ => None,
    }
}

/// `this` as a live entity, or the game's error.
pub fn this(
    shared: &Shared,
    host: &dyn Host,
    a: &[Value],
    method: &str,
) -> Result<EntityInfo, NativeError> {
    entity_id(&a[0])
        .and_then(|id| Entities::info(shared, host, id))
        .ok_or_else(|| {
            throw(format!(
                "Attempted to call {method} on invalid entity (no context)"
            ))
        })
}

/// An angle into [-180, 180] (client.dll 0x1804fb9f0).
pub fn angle_normalize(a: f32) -> f32 {
    let mut a = a % 360.0;
    if a > 180.0 {
        a -= 360.0;
    }
    if a < -180.0 {
        a += 360.0;
    }
    a
}

type Getter = fn(&EntityInfo) -> MethodValue;

/// What a simple getter returns.
pub enum MethodValue {
    Int(i32),
    Bool(bool),
    Str(String),
    Vector([f32; 3]),
}

pub fn install(cvm: &mut ClientVm) -> Shared {
    let shared: Shared = Rc::new(RefCell::new(Entities {
        classes: HashMap::new(),
        instances: HashMap::new(),
        client_only: HashMap::new(),
        next_client_only: 0,
        scopes: HashMap::new(),
        key_serial: 0,
    }));

    // C_BaseEntity's getters (client.dll 0x180300630 registers them).
    let base: &[(&str, Getter)] = &[
        ("GetOrigin", |e| MethodValue::Vector(e.origin)),
        ("GetLocalOrigin", |e| MethodValue::Vector(e.local_origin)),
        // Each component normalized (0x180027ef0).
        ("GetAngles", |e| {
            MethodValue::Vector(e.angles.map(angle_normalize))
        }),
        ("EyePosition", |e| MethodValue::Vector(e.eye_position)),
        ("EyeAngles", |e| MethodValue::Vector(e.eye_angles)),
        ("GetVelocity", |e| MethodValue::Vector(e.velocity)),
        ("GetForwardVector", |e| {
            MethodValue::Vector(pf_squirrel::vector::angles_forward(e.angles))
        }),
        ("GetRightVector", |e| {
            MethodValue::Vector(pf_squirrel::vector::angles_right_up(e.angles).0)
        }),
        ("GetUpVector", |e| {
            MethodValue::Vector(pf_squirrel::vector::angles_right_up(e.angles).1)
        }),
        ("GetTeam", |e| MethodValue::Int(e.team)),
        ("GetTeamNumber", |e| MethodValue::Int(e.team)),
        ("GetHealth", |e| MethodValue::Int(e.health)),
        ("GetMaxHealth", |e| MethodValue::Int(e.max_health)),
        ("GetLifeState", |e| MethodValue::Int(e.life_state)),
        ("IsEntAlive", |e| {
            MethodValue::Bool(e.life_state == LIFE_ALIVE)
        }),
        ("GetEntIndex", |e| MethodValue::Int(e.ent_index)),
        ("GetClassname", |e| MethodValue::Str(e.classname.clone())),
        ("GetSignifierName", |e| {
            MethodValue::Str(e.signifier.clone())
        }),
        ("GetName", |e| MethodValue::Str(e.name.clone())),
        ("IsPlayer", |e| MethodValue::Bool(e.is_player)),
        ("IsNPC", |e| MethodValue::Bool(e.is_npc)),
        ("IsTitan", |e| MethodValue::Bool(e.is_titan)),
        ("IsClientOnly", |e| MethodValue::Bool(e.client_only)),
    ];
    let vector_class = cvm.vm.root.borrow().get(&Value::str("Vector")).cloned();

    let mut members_of: HashMap<&str, Table> = HashMap::new();
    let mut base_members = Table::new();
    for &(name, getter) in base {
        let host = cvm.host.clone();
        let vector_class = vector_class.clone();
        let sh = shared.clone();
        base_members.insert(
            Value::str(name),
            native(name, 1, move |vm, a| {
                let e = this(&sh, &*host, a, name)?;
                Ok(match getter(&e) {
                    MethodValue::Int(i) => Value::Int(i),
                    MethodValue::Bool(b) => Value::Bool(b),
                    MethodValue::Str(s) => Value::str(&s),
                    MethodValue::Vector(v) => {
                        let class = vector_class.clone().unwrap_or_default();
                        vm.call(
                            &class,
                            Value::Null,
                            &[Value::Float(v[0]), Value::Float(v[1]), Value::Float(v[2])],
                        )?
                    }
                })
            }),
        );
    }

    // ValidateScriptScope (0x18002a710) and GetScriptScope (0x180027ea0):
    // the scope, or null if it hasn't been made.
    let (host, sh) = (cvm.host.clone(), shared.clone());
    base_members.insert(
        Value::str("ValidateScriptScope"),
        native("ValidateScriptScope", 1, move |vm, a| {
            this(&sh, &*host, a, "ValidateScriptScope")?;
            let id = entity_id(&a[0]).unwrap_or_default();
            let scope = Entities::validate_scope(&sh, vm, &*host, id);
            Ok(Value::Bool(!scope.is_null()))
        }),
    );
    let (host, sh) = (cvm.host.clone(), shared.clone());
    base_members.insert(
        Value::str("GetScriptScope"),
        native("GetScriptScope", 1, move |_, a| {
            this(&sh, &*host, a, "GetScriptScope")?;
            let id = entity_id(&a[0]).unwrap_or_default();
            Ok(sh.borrow().scopes.get(&id).cloned().unwrap_or_default())
        }),
    );
    members_of.insert("C_BaseEntity", base_members);

    // C_BasePlayer.GetActiveBurnCardIndex (0x1800368b0): the networked
    // value as it is (the class file subtracts one).
    let (host, sh) = (cvm.host.clone(), shared.clone());
    let mut player_members = Table::new();
    player_members.insert(
        Value::str("GetActiveBurnCardIndex"),
        native("GetActiveBurnCardIndex", 1, move |_, a| {
            let e = this(&sh, &*host, a, "GetActiveBurnCardIndex")?;
            Ok(Value::Int(e.active_burn_card_index))
        }),
    );
    members_of.insert("C_BasePlayer", player_members);

    // Register the classes base first, each with its class file run (see
    // classes.rs).
    for &(name, base_name) in CLASSES {
        let own = members_of.remove(name).unwrap_or_default();
        let base = base_name.and_then(|b| shared.borrow().classes.get(b).cloned());
        let class = crate::classes::register(&mut cvm.vm, name, name, base.as_ref(), own);
        shared.borrow_mut().classes.insert(name, class);
    }

    // Players.
    let host = cvm.host.clone();
    let s = shared.clone();
    cvm.vm.register("GetPlayerArray", 1, move |vm, _| {
        let players: Vec<Value> = host
            .players()
            .into_iter()
            .map(|id| Entities::instance(&s, vm, &*host, id))
            .filter(|v| !v.is_null())
            .collect();
        Ok(Value::Array(new_ref(players)))
    });
    for name in ["GetLocalViewPlayer", "GetLocalClientPlayer"] {
        let host = cvm.host.clone();
        let s = shared.clone();
        cvm.vm.register(name, 1, move |vm, _| {
            Ok(match host.local_player() {
                Some(id) => Entities::instance(&s, vm, &*host, id),
                None => Value::Null,
            })
        });
    }

    // IsValid on an entity instance: is it still there?
    let host = cvm.host.clone();
    let sh = shared.clone();
    cvm.vm.entity_is_valid = Some(Box::new(move |v| {
        entity_id(v).is_some_and(|id| Entities::info(&sh, &*host, id).is_some())
    }));
    shared
}
