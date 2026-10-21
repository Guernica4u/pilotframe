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

use crate::{ClientVm, EntityId, EntityInfo, FieldValue, Host};

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
    /// What client script can change on them.
    pub client_state: HashMap<EntityId, crate::clientents::ClientOnlyState>,
    next_client_only: EntityId,
    /// Script scopes made by `ValidateScriptScope`.
    scopes: HashMap<EntityId, Value>,
    /// The unique-key serial number (launcher.dll's, counted per VM).
    key_serial: u64,
    /// Players whose connect scripts have run, by entity index.
    connected: std::collections::HashSet<i32>,
    /// Entities with `EnableHealthChangedCallback` on.
    pub health_callbacks: std::collections::HashSet<EntityId>,
    /// Players whose crosshair names are hidden (`HideCrosshairNames`, bit
    /// 4 of a client-side player flags field, client.dll +0x3bb0).
    pub crosshair_names_hidden: std::collections::HashSet<EntityId>,
    /// Entities that call back to script before being destroyed
    /// (`SetDoDestroyCallback`, client.dll +0x2ed).
    pub destroy_callbacks: std::collections::HashSet<EntityId>,
    /// Cockpits that capture the screen before the view models
    /// (`SetCaptureScreenBeforeViewmodels`, +0x2360).
    pub capture_before_viewmodels: std::collections::HashSet<EntityId>,
    /// Players with the script menu on (bit 1 of the same flags field).
    pub script_menu_on: std::collections::HashSet<EntityId>,
    /// Movement parents client script set (`SetParentRaw`).
    pub parents: HashMap<EntityId, Parent>,
    /// Offsets from the parent's attachment (`SetAttachOffsetOrigin`,
    /// `SetAttachOffsetAngles`; client.dll +0x180 and its angle twin).
    pub attach_origin: HashMap<EntityId, [f32; 3]>,
    pub attach_angles: HashMap<EntityId, [f32; 3]>,
}

/// A movement parent set from client script.
#[derive(Clone, Debug, PartialEq)]
pub struct Parent {
    pub parent: EntityId,
    /// An attachment name ("" for none), or a hitbox when `hitbox` is set.
    pub attachment: String,
    pub hitbox: Option<i32>,
    /// Keep the current offset from the parent (maintainOffset, or a blend
    /// time above 0).
    pub keep_offset: bool,
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
        e.client_state.insert(id, Default::default());
        id
    }

    /// Remove a client-only entity: it's gone for `IsValid` and every
    /// method. (Script destroy callbacks are still to be ported.)
    pub fn destroy_client_only(this: &Shared, id: EntityId) {
        this.borrow_mut().client_only.remove(&id);
        this.borrow_mut().client_state.remove(&id);
        Entities::forget(this, id);
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

impl ClientVm {
    /// The local player has arrived: run `CodeCallback_RunClientConnectScripts
    /// ( player )` once per player (client.dll 0x180087b70): the player's
    /// scope is made, the function is looked up through it and called with
    /// the root table as `this`.
    pub fn run_client_connect_scripts(&mut self, id: EntityId) {
        let Some(ents) = self.entities.clone() else {
            return;
        };
        let Some(info) = Entities::info(&ents, &*self.host, id) else {
            return;
        };
        if ents.borrow().connected.contains(&info.ent_index) {
            return;
        }
        let scope = Entities::validate_scope(&ents, &mut self.vm, &*self.host, id);
        if scope.is_null() {
            return;
        }
        let name = Value::str("CodeCallback_RunClientConnectScripts");
        if let Ok(Some(f)) = self.vm.try_get(&scope, &name) {
            let player = Entities::instance(&ents, &mut self.vm, &*self.host, id);
            let root = Value::Table(self.vm.root.clone());
            if let Err(e) = self.vm.call(&f, root, &[player]) {
                self.vm.report_error(&e);
            }
        }
        ents.borrow_mut().connected.insert(info.ent_index);
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

/// The script methods that only read one networked field (client.dll: each
/// body is a single load from the entity and a return; listed with
/// Resources/re scripts' getter scan): class, method, the field's type.
pub const FIELDS: &[(&str, &str, FieldKind)] = &[
    ("C_BaseEntity", "GetLastFiredTime", FieldKind::Float),
    ("C_BaseEntity", "IsRenderingWithViewModels", FieldKind::Bool),
    ("C_BaseEntity", "GetCloakEndTime", FieldKind::Float),
    ("C_BaseAnimating", "IsSequenceFinished", FieldKind::Bool),
    ("C_BaseAnimating", "Anim_IsActive", FieldKind::Bool),
    (
        "C_BaseCombatCharacter",
        "NameIconAlphaToLocalPlayer",
        FieldKind::Float,
    ),
    ("C_BasePlayer", "CanDoContextAction", FieldKind::Bool),
    ("C_BasePlayer", "IsOffhandDisabled", FieldKind::Bool),
    ("C_BasePlayer", "GetSuitPower", FieldKind::Float),
    ("C_BasePlayer", "GetSuitJumpPower", FieldKind::Float),
    ("C_BasePlayer", "HasMic", FieldKind::Bool),
    ("C_BasePlayer", "IsWallHanging", FieldKind::Bool),
    ("C_BasePlayer", "IsSprinting", FieldKind::Bool),
    ("C_BasePlayer", "IsDodging", FieldKind::Bool),
    (
        "C_BasePlayer",
        "PlayerMelee_IsAttackActive",
        FieldKind::Bool,
    ),
    ("C_BasePlayer", "PlayerMelee_GetState", FieldKind::Int),
    ("C_BasePlayer", "PlayerLunge_IsLunging", FieldKind::Bool),
    ("C_BasePlayer", "GetSplitScreenPlayerSlot", FieldKind::Int),
    (
        "C_BasePlayer",
        "SmartAmmo_GetHighestFraction",
        FieldKind::Float,
    ),
    (
        "C_BasePlayer",
        "SmartAmmo_GetPrevHighestFraction",
        FieldKind::Float,
    ),
    ("C_BasePlayer", "GetOutOfBoundsDeadTime", FieldKind::Float),
    ("C_BasePlayer", "GetVoicePackIndex", FieldKind::Int),
    ("C_BasePlayer", "GetPassives", FieldKind::Int),
    ("C_BasePlayer", "GetForcedDialogueOnly", FieldKind::Bool),
    ("C_BasePlayer", "GetTitanBuildTime", FieldKind::Float),
    ("C_BasePlayer", "GetTitanBubbleShieldTime", FieldKind::Float),
    ("C_BasePlayer", "GetGen", FieldKind::Int),
    ("C_BasePlayer", "GetRank", FieldKind::Int),
    ("C_BasePlayer", "IsPlayingRanked", FieldKind::Bool),
    (
        "C_BasePlayer",
        "Leech_IsContextActionEnabled",
        FieldKind::Bool,
    ),
];

/// The type of a field in `FIELDS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Int,
    Float,
    Bool,
}

/// What a simple getter returns.
pub enum MethodValue {
    Int(i32),
    Bool(bool),
    Float(f32),
    Str(String),
    Vector([f32; 3]),
}

pub fn install(cvm: &mut ClientVm) -> Shared {
    let shared: Shared = Rc::new(RefCell::new(Entities {
        classes: HashMap::new(),
        instances: HashMap::new(),
        client_only: HashMap::new(),
        client_state: HashMap::new(),
        next_client_only: 0,
        scopes: HashMap::new(),
        key_serial: 0,
        connected: Default::default(),
        health_callbacks: Default::default(),
        crosshair_names_hidden: Default::default(),
        script_menu_on: Default::default(),
        attach_origin: Default::default(),
        attach_angles: Default::default(),
        destroy_callbacks: Default::default(),
        capture_before_viewmodels: Default::default(),
        parents: HashMap::new(),
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
                    MethodValue::Float(f) => Value::Float(f),
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
    // Destroy (0x1802f2eb0): only entities made by client script.
    let (host, sh) = (cvm.host.clone(), shared.clone());
    base_members.insert(
        Value::str("Destroy"),
        native("Destroy", 1, move |_, a| {
            let e = this(&sh, &*host, a, "Destroy")?;
            if !e.client_only {
                return Err(throw("You cannot destroy a server entity in client script"));
            }
            if let Some(id) = entity_id(&a[0]) {
                Entities::destroy_client_only(&sh, id);
            }
            Ok(Value::Null)
        }),
    );
    // SetDoDestroyCallback( bool ) (0x180028580).
    let (host, sh) = (cvm.host.clone(), shared.clone());
    base_members.insert(
        Value::str("SetDoDestroyCallback"),
        pf_squirrel::native_checked("SetDoDestroyCallback", 2, ".b", move |_, a| {
            this(&sh, &*host, a, "SetDoDestroyCallback")?;
            let id = entity_id(&a[0]).unwrap_or_default();
            let mut e = sh.borrow_mut();
            if matches!(a[1], Value::Bool(true)) {
                e.destroy_callbacks.insert(id);
            } else {
                e.destroy_callbacks.remove(&id);
            }
            Ok(Value::Null)
        }),
    );
    // GetModelName (0x1800282a0).
    let (host, sh) = (cvm.host.clone(), shared.clone());
    base_members.insert(
        Value::str("GetModelName"),
        native("GetModelName", 1, move |_, a| {
            let e = this(&sh, &*host, a, "GetModelName")?;
            Ok(Value::str(&e.model))
        }),
    );
    // IsValidInternal (launcher.dll 0x18000c920, added to every class):
    // the entity still exists.
    let (host, sh) = (cvm.host.clone(), shared.clone());
    base_members.insert(
        Value::str("IsValidInternal"),
        native("IsValidInternal", 1, move |_, a| {
            Ok(Value::Bool(entity_id(&a[0]).is_some_and(|id| {
                Entities::info(&sh, &*host, id).is_some()
            })))
        }),
    );
    // Enable/DisableHealthChangedCallback (0x1802f2990, 0x1802f29a0): the
    // game then calls ClientCallback_OnHealthChanged when its health
    // changes (that call comes with entity updates; not ported yet).
    for (name, on) in [
        ("EnableHealthChangedCallback", true),
        ("DisableHealthChangedCallback", false),
    ] {
        let (host, sh) = (cvm.host.clone(), shared.clone());
        base_members.insert(
            Value::str(name),
            native(name, 1, move |_, a| {
                this(&sh, &*host, a, name)?;
                let id = entity_id(&a[0]).unwrap_or_default();
                let mut e = sh.borrow_mut();
                if on {
                    e.health_callbacks.insert(id);
                } else {
                    e.health_callbacks.remove(&id);
                }
                Ok(Value::Null)
            }),
        );
    }
    // SetParentRaw( parent, maintainOffset, blendTime, useHitbox,
    // attachment, hitbox ) (0x18002ddf0), the class file's SetParent and
    // SetParentWithHitbox call it. The model checks (BaseAnimating, model,
    // hitbox range) wait for models in entity state.
    let (host, sh) = (cvm.host.clone(), shared.clone());
    base_members.insert(
        Value::str("SetParentRaw"),
        native("SetParentRaw", 7, move |_, a| {
            let e = this(&sh, &*host, a, "SetParentRaw")?;
            let id = entity_id(&a[0]).unwrap_or_default();
            let attachment = match &a[5] {
                Value::Str(s) => s.to_string_lossy(),
                _ => String::new(),
            };
            let hitbox = match a[6] {
                Value::Int(n) => n,
                Value::Float(x) => x as i32,
                _ => 0,
            };
            if hitbox != 0 && !attachment.is_empty() {
                return Err(throw(
                    "SetParent: Cannot attach to hitbox and standard attachment at same time.",
                ));
            }
            let parent = entity_id(&a[1]).filter(|&p| Entities::info(&sh, &*host, p).is_some());
            let Some(parent) = parent else {
                return Err(throw(format!(
                    "Tried to SetParent for entity {} ({}), but the parent entity argument is not valid.",
                    e.classname, e.name
                )));
            };
            if parent == id {
                return Err(throw(format!(
                    "Tried to SetParent of {} ({} {}) to itself.",
                    e.name, e.classname, e.ent_index
                )));
            }
            let blend = match a[3] {
                Value::Int(n) => n as f32,
                Value::Float(x) => x,
                _ => 0.0,
            };
            let use_hitbox = matches!(a[4], Value::Bool(true));
            sh.borrow_mut().parents.insert(
                id,
                Parent {
                    parent,
                    attachment,
                    hitbox: use_hitbox.then_some(hitbox),
                    keep_offset: matches!(a[2], Value::Bool(true)) || blend > 0.0,
                },
            );
            Ok(Value::Null)
        }),
    );
    crate::clientents::base_methods(cvm, &shared, &mut base_members);
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
    // CameraPosition / CameraAngles (0x180035270, 0x180035360).
    for (name, angles) in [("CameraPosition", false), ("CameraAngles", true)] {
        let (host, sh) = (cvm.host.clone(), shared.clone());
        let vector_class = vector_class.clone();
        player_members.insert(
            Value::str(name),
            native(name, 1, move |vm, a| {
                let e = this(&sh, &*host, a, name)?;
                let v = if angles {
                    e.camera_angles
                } else {
                    e.camera_origin
                };
                let class = vector_class.clone().unwrap_or_default();
                Ok(vm.call(
                    &class,
                    Value::Null,
                    &[Value::Float(v[0]), Value::Float(v[1]), Value::Float(v[2])],
                )?)
            }),
        );
    }
    // Networked player fields (client.dll 0x180079ea0, 0x1800368d0,
    // 0x180079ef0, 0x180079e30, 0x18003eac0).
    let fields: [(&str, Getter); 10] = [
        ("GetPetTitanMode", |e| MethodValue::Int(e.pet_titan_mode)),
        ("GetHotDropImpactTime", |e| {
            MethodValue::Float(e.hot_drop_impact_time)
        }),
        ("GetPlayerClass", |e| {
            MethodValue::Str(e.player_class.clone())
        }),
        ("GetObjectiveIndex", |e| MethodValue::Int(e.objective_index)),
        ("GetObjectiveEndTime", |e| {
            MethodValue::Float(e.objective_end_time)
        }),
        ("GetCinematicEventFlags", |e| {
            MethodValue::Int(e.cinematic_event_flags)
        }),
        ("GetXP", |e| MethodValue::Int(e.xp)),
        ("HasBadReputation", |e| MethodValue::Bool(e.bad_reputation)),
        ("GetNextTitanRespawnAvailable", |e| {
            MethodValue::Float(e.next_titan_respawn_available)
        }),
        ("GetPlayerSettings", |e| {
            MethodValue::Str(e.player_settings.clone())
        }),
    ];
    for (name, getter) in fields {
        let (host, sh) = (cvm.host.clone(), shared.clone());
        player_members.insert(
            Value::str(name),
            native(name, 1, move |_, a| {
                let e = this(&sh, &*host, a, name)?;
                Ok(match getter(&e) {
                    MethodValue::Int(i) => Value::Int(i),
                    MethodValue::Bool(b) => Value::Bool(b),
                    MethodValue::Float(f) => Value::Float(f),
                    MethodValue::Str(s) => Value::str(&s),
                    MethodValue::Vector(_) => Value::Null,
                })
            }),
        );
    }
    // GetLevel (0x1800368e0): the level for the player's XP, from the XP
    // table ("XP N too low" below 0).
    let (host, sh) = (cvm.host.clone(), shared.clone());
    let xp_table = cvm.xp_table.clone();
    player_members.insert(
        Value::str("GetLevel"),
        native("GetLevel", 1, move |_, a| {
            let e = this(&sh, &*host, a, "GetLevel")?;
            if e.xp < 0 {
                return Err(throw(format!("XP {} too low", e.xp)));
            }
            let level = xp_table.as_ref().map_or(1, |t| t.borrow().level_for(e.xp));
            Ok(Value::Int(level))
        }),
    );
    // GetPersistentVar( path ) (0x180036810, 0x1802c88a0): the local
    // player's persistent data.
    let (host, sh) = (cvm.host.clone(), shared.clone());
    player_members.insert(
        Value::str("GetPersistentVar"),
        pf_squirrel::native_checked("GetPersistentVar", 2, ".s", move |_, a| {
            let e = this(&sh, &*host, a, "GetPersistentVar")?;
            let local = host.local_player().and_then(|id| host.entity(id));
            if local.is_none_or(|l| l.ent_index != e.ent_index) {
                return Err(throw("Player must be local."));
            }
            let path = match &a[1] {
                Value::Str(s) => s.to_string_lossy(),
                _ => String::new(),
            };
            if path.is_empty() {
                return Err(throw("Blank var name not allowed."));
            }
            use crate::PersistentValue as P;
            match host.persistent_var(&path) {
                None => Err(throw(format!("Invalid var name '{path}'."))),
                Some(P::Bool(b)) => Ok(Value::Bool(b)),
                Some(P::Int(i)) => Ok(Value::Int(i)),
                Some(P::Float(f)) => Ok(Value::Float(f)),
                Some(P::String(s)) => Ok(Value::str(&s)),
                Some(P::Enum(s)) if s.is_empty() => Ok(Value::Null),
                Some(P::Enum(s)) => Ok(Value::str(&s)),
                Some(P::Struct) => Err(throw("Specified var is a struct.")),
            }
        }),
    );
    // Hide/UnhideCrosshairNames (0x18007c710, 0x18007c720).
    for (name, hide) in [
        ("HideCrosshairNames", true),
        ("UnhideCrosshairNames", false),
    ] {
        let (host, sh) = (cvm.host.clone(), shared.clone());
        player_members.insert(
            Value::str(name),
            native(name, 1, move |_, a| {
                this(&sh, &*host, a, name)?;
                let id = entity_id(&a[0]).unwrap_or_default();
                let mut e = sh.borrow_mut();
                if hide {
                    e.crosshair_names_hidden.insert(id);
                } else {
                    e.crosshair_names_hidden.remove(&id);
                }
                Ok(Value::Null)
            }),
        );
    }
    // SetScriptMenuOff (0x18007c6d0): clears bit 1 of the player's
    // client-side flags (the script menu is off).
    let (host, sh) = (cvm.host.clone(), shared.clone());
    player_members.insert(
        Value::str("SetScriptMenuOff"),
        native("SetScriptMenuOff", 1, move |_, a| {
            this(&sh, &*host, a, "SetScriptMenuOff")?;
            let id = entity_id(&a[0]).unwrap_or_default();
            sh.borrow_mut().script_menu_on.remove(&id);
            Ok(Value::Null)
        }),
    );
    // GetPetTitan (0x180037590): the entity, or null.
    let (host, sh) = (cvm.host.clone(), shared.clone());
    player_members.insert(
        Value::str("GetPetTitan"),
        native("GetPetTitan", 1, move |vm, a| {
            let e = this(&sh, &*host, a, "GetPetTitan")?;
            Ok(match e.pet_titan {
                Some(t) => Entities::instance(&sh, vm, &*host, t),
                None => Value::Null,
            })
        }),
    );
    // GetCockpit (0x180087cf0): the cockpit entity, or null.
    let (host, sh) = (cvm.host.clone(), shared.clone());
    player_members.insert(
        Value::str("GetCockpit"),
        native("GetCockpit", 1, move |vm, a| {
            let e = this(&sh, &*host, a, "GetCockpit")?;
            Ok(match e.cockpit {
                Some(c) => Entities::instance(&sh, vm, &*host, c),
                None => Value::Null,
            })
        }),
    );
    // GetOffhandWeapon( slot ) (0x18001bd50): slots 0–3.
    let (host, sh) = (cvm.host.clone(), shared.clone());
    player_members.insert(
        Value::str("GetOffhandWeapon"),
        pf_squirrel::native_checked("GetOffhandWeapon", 2, ".n", move |vm, a| {
            let e = this(&sh, &*host, a, "GetOffhandWeapon")?;
            let slot = match a[1] {
                Value::Int(n) => n,
                Value::Float(x) => x as i32,
                _ => 0,
            };
            if !(0..4).contains(&slot) {
                return Err(throw(format!(
                    "Invalid offhand index [{slot}].  Range is [0, 3]"
                )));
            }
            let id = entity_id(&a[0]).unwrap_or_default();
            let _ = e;
            Ok(match host.offhand_weapon_entity(id, slot) {
                Some(w) => Entities::instance(&sh, vm, &*host, w),
                None => Value::Null,
            })
        }),
    );
    // GetOrdnanceWeapon (0x18001ce00 -> 0x18001c280): the first of the four
    // main weapon slots whose weapon file sets `ordnance 1` (its info +0x461,
    // read by 0x180481f20). (The game also asks the player whether it can
    // use the weapon; ours always can.)
    let (host, sh) = (cvm.host.clone(), shared.clone());
    player_members.insert(
        Value::str("GetOrdnanceWeapon"),
        native("GetOrdnanceWeapon", 1, move |vm, a| {
            this(&sh, &*host, a, "GetOrdnanceWeapon")?;
            let id = entity_id(&a[0]).unwrap_or_default();
            let found = host.main_weapons(id).into_iter().take(4).find(|&w| {
                host.weapon_info_field(w, "ordnance")
                    .and_then(|v| v.trim().parse::<i32>().ok())
                    .is_some_and(|n| n != 0)
            });
            Ok(match found {
                Some(w) => Entities::instance(&sh, vm, &*host, w),
                None => Value::Null,
            })
        }),
    );
    // GetWeaponAmmoLoaded( weapon ) (0x18001af10): the weapon's clip; the
    // local player only.
    let (host, sh) = (cvm.host.clone(), shared.clone());
    player_members.insert(
        Value::str("GetWeaponAmmoLoaded"),
        native("GetWeaponAmmoLoaded", 2, move |_, a| {
            this(&sh, &*host, a, "GetWeaponAmmoLoaded")?;
            let weapon = entity_id(&a[1])
                .filter(|&w| {
                    Entities::info(&sh, &*host, w).is_some_and(|e| e.script_class == "C_WeaponX")
                })
                .ok_or_else(|| throw("Argument is not a valid weapon"))?;
            if entity_id(&a[0]) != host.local_player() {
                return Err(throw("Cannot be called on non-local players"));
            }
            Ok(Value::Int(host.weapon_clip_ammo(weapon)))
        }),
    );
    // GetCrosshairPos (0x18008a980): [x, y], fractions of the screen.
    let (host, sh) = (cvm.host.clone(), shared.clone());
    player_members.insert(
        Value::str("GetCrosshairPos"),
        native("GetCrosshairPos", 1, move |_, a| {
            this(&sh, &*host, a, "GetCrosshairPos")?;
            let id = entity_id(&a[0]).unwrap_or_default();
            let (x, y) = host.crosshair_pos(id);
            Ok(Value::Array(new_ref(vec![
                Value::Float(x),
                Value::Float(y),
            ])))
        }),
    );
    members_of.insert("C_BasePlayer", player_members);
    // Weapons.
    let weapon_members = members_of.entry("C_WeaponX").or_default();
    // GetWeaponInfoFileKeyField( key ) (0x1804824b0 -> 0x1802e86f0): the
    // value in the weapon file's block as its KeyValues type (string, int,
    // float), or null. The weapon files quote every value, so they read as
    // strings.
    let (host, sh) = (cvm.host.clone(), shared.clone());
    weapon_members.insert(
        Value::str("GetWeaponInfoFileKeyField"),
        pf_squirrel::native_checked("GetWeaponInfoFileKeyField", 2, ".s", move |_, a| {
            this(&sh, &*host, a, "GetWeaponInfoFileKeyField")?;
            let id = entity_id(&a[0]).unwrap_or_default();
            let key = match &a[1] {
                Value::Str(s) => s.to_string_lossy(),
                _ => String::new(),
            };
            Ok(host
                .weapon_info_field(id, &key)
                .map_or(Value::Null, |v| Value::str(&v)))
        }),
    );
    // GetWeaponModSetting( key ) (0x180483270 -> 0x1802edae0): a modable
    // setting with the weapon's mods applied. The game checks the key
    // against its table of modable settings and returns it typed by that
    // table (int, float, bool, string); the table's names are filled in at
    // run time and aren't read yet, so here the type comes from the text
    // (a marked placeholder), and an absent setting reads null.
    let (host, sh) = (cvm.host.clone(), shared.clone());
    weapon_members.insert(
        Value::str("GetWeaponModSetting"),
        pf_squirrel::native_checked("GetWeaponModSetting", 2, ".s", move |_, a| {
            this(&sh, &*host, a, "GetWeaponModSetting")?;
            let id = entity_id(&a[0]).unwrap_or_default();
            let key = match &a[1] {
                Value::Str(s) => s.to_string_lossy(),
                _ => String::new(),
            };
            let Some(v) = host.weapon_mod_setting(id, &key) else {
                return Ok(Value::Null);
            };
            let t = v.trim();
            Ok(if let Ok(n) = t.parse::<i32>() {
                Value::Int(n)
            } else if let Ok(x) = t.parse::<f32>() {
                Value::Float(x)
            } else {
                Value::str(&v)
            })
        }),
    );
    // HasModDefined( name ) (0x180483220) and HasMod( name ) (0x18048bae0):
    // an empty name is "Null string for modname" (false); HasMod on a mod
    // the weapon doesn't define is an error naming both.
    for (name, active) in [("HasModDefined", false), ("HasMod", true)] {
        let (host, sh) = (cvm.host.clone(), shared.clone());
        weapon_members.insert(
            Value::str(name),
            pf_squirrel::native_checked(name, 2, ".s", move |_, a| {
                let e = this(&sh, &*host, a, name)?;
                let id = entity_id(&a[0]).unwrap_or_default();
                let m = match &a[1] {
                    Value::Str(s) => s.to_string_lossy(),
                    _ => String::new(),
                };
                if m.is_empty() {
                    return Err(throw("Null string for modname"));
                }
                let defined = host.weapon_mod_defined(id, &m);
                if !active {
                    return Ok(Value::Bool(defined));
                }
                if !defined {
                    return Err(throw(format!(
                        "Undefined mod name '{m}' requested on weapon '{}'.",
                        e.classname
                    )));
                }
                Ok(Value::Bool(host.weapon_mod_active(id, &m)))
            }),
        );
    }
    // GetMods (0x18048bb70): the active mods' names, in the file's order.
    let (host, sh) = (cvm.host.clone(), shared.clone());
    weapon_members.insert(
        Value::str("GetMods"),
        native("GetMods", 1, move |_, a| {
            this(&sh, &*host, a, "GetMods")?;
            let id = entity_id(&a[0]).unwrap_or_default();
            let mods = host.weapon_mods(id).iter().map(|m| Value::str(m)).collect();
            Ok(Value::Array(new_ref(mods)))
        }),
    );
    // IsReadyToFire (0x180487270): the next allowed attack time (+0x230c)
    // has come.
    let (host, sh) = (cvm.host.clone(), shared.clone());
    weapon_members.insert(
        Value::str("IsReadyToFire"),
        native("IsReadyToFire", 1, move |_, a| {
            this(&sh, &*host, a, "IsReadyToFire")?;
            let id = entity_id(&a[0]).unwrap_or_default();
            Ok(Value::Bool(host.weapon_next_attack_time(id) <= host.time()))
        }),
    );
    // GetWeaponPrintName (0x180020c90): the weapon's `printname`; the
    // game's default for a weapon without one is
    // "!!! Missing printname on weapon" (its modable settings table).
    let (host, sh) = (cvm.host.clone(), shared.clone());
    weapon_members.insert(
        Value::str("GetWeaponPrintName"),
        native("GetWeaponPrintName", 1, move |_, a| {
            this(&sh, &*host, a, "GetWeaponPrintName")?;
            let id = entity_id(&a[0]).unwrap_or_default();
            let name = host
                .weapon_mod_setting(id, "printname")
                .unwrap_or_else(|| "!!! Missing printname on weapon".into());
            Ok(Value::str(&name))
        }),
    );
    // LookupAttachment( name ) (0x1802f0970): the model's attachment id.
    let (host, sh) = (cvm.host.clone(), shared.clone());
    members_of.entry("C_BaseAnimating").or_default().insert(
        Value::str("LookupAttachment"),
        pf_squirrel::native_checked("LookupAttachment", 2, ".s", move |_, a| {
            this(&sh, &*host, a, "LookupAttachment")?;
            let id = entity_id(&a[0]).unwrap_or_default();
            let name = match &a[1] {
                Value::Str(s) => s.to_string_lossy(),
                _ => String::new(),
            };
            Ok(Value::Int(host.lookup_attachment(id, &name)))
        }),
    );
    // GetParent (0x18002c5b0): the movement parent, or null.
    let (host, sh) = (cvm.host.clone(), shared.clone());
    members_of.entry("C_BaseEntity").or_default().insert(
        Value::str("GetParent"),
        native("GetParent", 1, move |vm, a| {
            this(&sh, &*host, a, "GetParent")?;
            let id = entity_id(&a[0]).unwrap_or_default();
            let parent = sh.borrow().parents.get(&id).map(|p| p.parent);
            Ok(match parent {
                Some(p) => Entities::instance(&sh, vm, &*host, p),
                None => Value::Null,
            })
        }),
    );
    // SetAttachOffsetOrigin / SetAttachOffsetAngles( vector ) (0x1802f84f0,
    // 0x1802f8580): client-only entities with a parent only.
    for (name, angles) in [
        ("SetAttachOffsetOrigin", false),
        ("SetAttachOffsetAngles", true),
    ] {
        let (host, sh) = (cvm.host.clone(), shared.clone());
        members_of.entry("C_BaseEntity").or_default().insert(
            Value::str(name),
            pf_squirrel::native_checked(name, 2, ".x", move |_, a| {
                let e = this(&sh, &*host, a, name)?;
                if !e.client_only {
                    return Err(throw("Only valid with client-only entities."));
                }
                let id = entity_id(&a[0]).unwrap_or_default();
                if !sh.borrow().parents.contains_key(&id) {
                    return Err(throw("This entity is not parented."));
                }
                let v = pf_squirrel::vector::get(&a[1]).unwrap_or_default();
                let mut s = sh.borrow_mut();
                if angles {
                    s.attach_angles.insert(id, v);
                } else {
                    s.attach_origin.insert(id, v);
                }
                Ok(Value::Null)
            }),
        );
    }
    for &(class, name, kind) in FIELDS {
        let (host, sh) = (cvm.host.clone(), shared.clone());
        members_of.entry(class).or_default().insert(
            Value::str(name),
            native(name, 1, move |_, a| {
                this(&sh, &*host, a, name)?;
                let id = entity_id(&a[0]).unwrap_or_default();
                Ok(match (host.entity_field(id, name), kind) {
                    (Some(FieldValue::Int(v)), _) => Value::Int(v),
                    (Some(FieldValue::Float(v)), _) => Value::Float(v),
                    (Some(FieldValue::Bool(v)), _) => Value::Bool(v),
                    (None, FieldKind::Int) => Value::Int(0),
                    (None, FieldKind::Float) => Value::Float(0.0),
                    (None, FieldKind::Bool) => Value::Bool(false),
                })
            }),
        );
    }
    // C_Titan_Cockpit.SetCaptureScreenBeforeViewmodels( bool ) (0x1803f5b50).
    let (host, sh) = (cvm.host.clone(), shared.clone());
    let mut cockpit_members = Table::new();
    cockpit_members.insert(
        Value::str("SetCaptureScreenBeforeViewmodels"),
        pf_squirrel::native_checked("SetCaptureScreenBeforeViewmodels", 2, ".b", move |_, a| {
            this(&sh, &*host, a, "SetCaptureScreenBeforeViewmodels")?;
            let id = entity_id(&a[0]).unwrap_or_default();
            let mut e = sh.borrow_mut();
            if matches!(a[1], Value::Bool(true)) {
                e.capture_before_viewmodels.insert(id);
            } else {
                e.capture_before_viewmodels.remove(&id);
            }
            Ok(Value::Null)
        }),
    );
    members_of.insert("C_Titan_Cockpit", cockpit_members);
    members_of.insert(
        "C_PointCamera",
        crate::clientents::point_camera_methods(cvm, &shared),
    );

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
    // GetPlayerArrayOfTeam( team ) (0x1802c41d0 -> 0x1802c3fa0): -1 every
    // player, -2 (the default) every player not on team 1, else that team.
    let host = cvm.host.clone();
    let s = shared.clone();
    cvm.vm.register("GetPlayerArrayOfTeam", 0, move |vm, a| {
        let team = match a.get(1) {
            Some(Value::Int(n)) => *n,
            Some(Value::Float(x)) => *x as i32,
            _ => -2,
        };
        let players: Vec<Value> = host
            .players()
            .into_iter()
            .filter(|&id| {
                let t = Entities::info(&s, &*host, id).map_or(0, |e| e.team);
                match team {
                    -1 => true,
                    -2 => t != 1,
                    _ => t == team,
                }
            })
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

    // StopSoundOnEntity( ent, sound ) (0x1804440a0).
    let (host, s) = (cvm.host.clone(), shared.clone());
    cvm.vm
        .register_checked("StopSoundOnEntity", 3, "..s", move |_, a| {
            let id = entity_id(&a[1]).filter(|&id| Entities::info(&s, &*host, id).is_some());
            let Some(id) = id else {
                return Err(throw("StopSoundOnEntity - null entity"));
            };
            let sound = match &a[2] {
                Value::Str(n) => n.to_string_lossy(),
                _ => String::new(),
            };
            host.stop_sound_on_entity(id, &sound);
            Ok(Value::Null)
        });
    // GetClientEntArrayBySignifier( signifier ) (0x1802c6320): every client
    // entity with that signifier, in the client's entity list order
    // (client-only ones after the host's).
    let (host, s) = (cvm.host.clone(), shared.clone());
    cvm.vm
        .register("GetClientEntArrayBySignifier", 0, move |vm, a| {
            if a.len() != 2 {
                return Err(throw("Function takes 1 parameter"));
            }
            let sig = match &a[1] {
                Value::Str(n) => n.to_string_lossy(),
                _ => String::new(),
            };
            let mut ids = host.entities();
            let mut client: Vec<EntityId> = s.borrow().client_only.keys().copied().collect();
            client.sort();
            ids.extend(client);
            let mut out = Vec::new();
            for id in ids {
                let matches = Entities::info(&s, &*host, id).is_some_and(|e| e.signifier == sig);
                if matches {
                    let v = Entities::instance(&s, vm, &*host, id);
                    if !v.is_null() {
                        out.push(v);
                    }
                }
            }
            Ok(Value::Array(new_ref(out)))
        });

    // IsValid on an entity instance: is it still there?
    let host = cvm.host.clone();
    let sh = shared.clone();
    cvm.vm.entity_is_valid = Some(Box::new(move |v| {
        entity_id(v).is_some_and(|id| Entities::info(&sh, &*host, id).is_some())
    }));
    shared
}

impl ClientVm {
    /// Entities that appeared since the last frame: their creation
    /// callbacks run (client.dll 0x1802fc260): the local player's first,
    /// then the others in order, each as
    /// `ClientCodeCallback_OnEntityCreation( entity )` with the root table
    /// as `this`. The engine also runs this right after the connect
    /// callback.
    pub fn entities_created(&mut self, ids: &[EntityId]) {
        let Some(ents) = self.entities.clone() else {
            return;
        };
        let root = Value::Table(self.vm.root.clone());
        let Ok(Some(f)) = self
            .vm
            .try_get(&root, &Value::str("ClientCodeCallback_OnEntityCreation"))
        else {
            return;
        };
        let local = self.host.local_player();
        let order = ids
            .iter()
            .filter(|&&id| Some(id) == local)
            .chain(ids.iter().filter(|&&id| Some(id) != local));
        for &id in order {
            let e = Entities::instance(&ents, &mut self.vm, &*self.host, id);
            if e.is_null() {
                continue;
            }
            if let Err(err) = self.vm.call(&f, root.clone(), &[e]) {
                self.vm.report_error(&err);
            }
        }
    }
}
