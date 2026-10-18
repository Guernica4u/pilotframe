//! The game's client script VM: our Squirrel interpreter (`pf-squirrel`)
//! with client.dll's script functions, started the way client.dll starts it
//! (docs/port/scripts.md, "Startup and the compiler's extras"):
//! 1. the VM runs `init.nut`, `_consts.nut` and `_threads.nut` (launcher.dll);
//! 2. client.dll registers its functions and constants;
//! 3. `vscript_client.nut` runs;
//! 4. `_cl_mapspawn` is included into the root table.
//!
//! Every function here is ported from client.dll (its address is noted);
//! the game's state comes from the `Host`.
//!
//! Layer 0: no Bevy, no file access (scripts come through the host).

use std::rc::Rc;

use pf_squirrel::{Value, Vm};

pub mod autotext;
pub mod classes;
pub mod clientents;
pub mod consts;
pub mod engine;
pub mod entities;
pub mod gamerules;
pub mod globals;
pub mod hud;
pub mod input;
pub mod misc;
pub mod modelfx;
pub mod pdef;
pub mod persistence;
pub mod progression;
pub mod remote;
pub mod rumble;
pub mod standin;
pub mod weapons;

/// What the client VM needs from the game.
pub trait Host {
    /// A file from the install (`scripts/vscripts/...`), or None.
    fn read_script(&self, path: &str) -> Option<String>;
    /// Whether a script file exists (`ScriptExists`).
    fn script_exists(&self, path: &str) -> bool {
        self.read_script(path).is_some()
    }
    /// The loaded level, as the engine names it (`maps/mp_colony.bsp`).
    fn level_name(&self) -> String;
    /// The `developer` console variable (0 in the shipped game).
    fn developer(&self) -> i32 {
        0
    }
    /// Game time in seconds (`Time()`).
    fn time(&self) -> f32;
    /// The last frame's length in seconds (`FrameTime()`).
    fn frame_time(&self) -> f32;
    /// A console variable's value as text ("" if there's no such variable).
    fn convar(&self, name: &str) -> String {
        let _ = name;
        String::new()
    }
    /// Load a material for the HUD (`vgui/...`, lower case). False if it
    /// couldn't be loaded.
    fn precache_hud_material(&self, path: &str) -> bool {
        let _ = path;
        true
    }
    /// How many teams there are (unassigned, spectator, IMC, Militia).
    fn team_count(&self) -> i32 {
        4
    }
    /// A team (index below `team_count`).
    fn team(&self, index: i32) -> TeamInfo {
        let _ = index;
        TeamInfo::default()
    }
    /// Connected to a server (in our game: a match is running).
    fn is_connected(&self) -> bool {
        true
    }
    /// A variable of the current playlist (from the install's playlist
    /// file), or None.
    fn playlist_var(&self, name: &str) -> Option<String> {
        let _ = name;
        None
    }
    /// Load a material (any path). False if it couldn't be loaded.
    fn precache_material(&self, name: &str) -> bool {
        let _ = name;
        true
    }
    /// The optimized lobby is on (`optimized_lobby_enabled` in the lobby):
    /// precaching is skipped and the HUD uses its lobby layout.
    fn optimized_lobby(&self) -> bool {
        false
    }
    /// Load a model. False if it couldn't be loaded. (The game skips
    /// loading in the optimized lobby and reports success.)
    fn precache_model(&self, name: &str) -> bool {
        let _ = name;
        true
    }
    /// Load a particle system; its index.
    fn precache_particle_system(&self, name: &str) -> i32 {
        let _ = name;
        0
    }
    /// Load an impact effect table; its index.
    fn precache_impact_table(&self, name: &str) -> i32 {
        let _ = name;
        0
    }
    /// Create a colour-correction layer; its index.
    fn color_correction_register(&self, name: &str) -> i32 {
        let _ = name;
        0
    }
    fn color_correction_set_exclusive(&self, index: i32, exclusive: bool) {
        let _ = (index, exclusive);
    }
    /// An entity's current state, or None if it doesn't exist (any more).
    fn entity(&self, id: EntityId) -> Option<EntityInfo> {
        let _ = id;
        None
    }
    /// Every player, in entity index order.
    fn players(&self) -> Vec<EntityId> {
        Vec::new()
    }
    /// The local player (the one this client controls or watches).
    fn local_player(&self) -> Option<EntityId> {
        None
    }
    /// Load a weapon (its models, sounds, effects). False if it can't be.
    fn precache_weapon(&self, name: &str) -> bool {
        let _ = name;
        true
    }
    /// A private match (not matchmade).
    fn is_private_match(&self) -> bool {
        false
    }
    /// A loaded particle system's index, 0 if it isn't loaded.
    fn particle_system_index(&self, name: &str) -> i32 {
        let _ = name;
        0
    }
    /// Whether the engine has a console command with this name.
    fn concommand_exists(&self, name: &str) -> bool {
        let _ = name;
        true
    }
    /// The persistent data definition, or None if it isn't loaded (in the
    /// game: not connected to a server). See `pdef::build`.
    fn persistence_def(&self) -> Option<Rc<persistence::PersistenceDef>> {
        None
    }
    /// Whether keyboard, mouse and controller go to the game (not, e.g.,
    /// to the console). Nothing reads as pressed otherwise.
    fn input_to_game(&self) -> bool {
        true
    }
    /// Whether the game's own menus (the UI VM) have the input: the client
    /// VM's button callbacks don't run then.
    fn game_ui_active(&self) -> bool {
        false
    }
    /// A controller is in use: controller buttons count.
    fn controller_mode(&self) -> bool {
        false
    }
    /// Whether a button is held, by the engine's button code
    /// (`input::BUTTONS`).
    fn button_down(&self, code: i32) -> bool {
        let _ = code;
        false
    }
    /// A controller axis, -32767..32767, by the engine's axis number.
    fn axis_raw(&self, axis: i32) -> i32 {
        let _ = axis;
        0
    }
    /// The console command bound to a button ("" if none).
    fn key_binding(&self, code: i32) -> String {
        let _ = code;
        String::new()
    }
    /// The other team (the game rules' answer: IMC 2 and Militia 3 face
    /// each other).
    fn other_team(&self, team: i32) -> i32 {
        match team {
            2 => 3,
            3 => 2,
            t => t,
        }
    }
    /// The local player's active weapon, for HUD auto text.
    fn active_weapon(&self) -> Option<autotext::WeaponHud> {
        None
    }
    /// The local player's offhand weapon in slot 0 or 1.
    fn offhand_weapon(&self, slot: usize) -> Option<autotext::WeaponHud> {
        let _ = slot;
        None
    }
    /// The local player is spectating with the spectator controls on.
    fn spectator_controls_active(&self) -> bool {
        false
    }
    /// A networked field of an entity, by the script method that reads it
    /// (`entities::FIELDS`: "GetSuitPower", "IsWallHanging"…). None reads as
    /// the field's zero, as a fresh entity's would.
    fn entity_field(&self, id: EntityId, method: &str) -> Option<FieldValue> {
        let _ = (id, method);
        None
    }
    /// Every entity the client has, in its entity list's order.
    fn entities(&self) -> Vec<EntityId> {
        self.players()
    }
    /// Stop a sound playing on an entity for the local player. False if
    /// the sound name isn't known (the game then does nothing).
    fn stop_sound_on_entity(&self, id: EntityId, sound: &str) -> bool {
        let _ = (id, sound);
        true
    }
    /// The local client is watching a kill replay (engine.dll).
    fn watching_kill_replay(&self) -> bool {
        false
    }
    /// The local player's persistent value at a path (`"xp"`,
    /// `"pilotLoadouts[0].name"`); None if the path isn't a variable. The
    /// real definitions come with the server work (docs/port/server.md).
    fn persistent_var(&self, path: &str) -> Option<PersistentValue> {
        let _ = path;
        None
    }
    /// The game window's size in pixels (`Hud.GetScreenSize()`).
    fn screen_size(&self) -> (i32, i32) {
        (1920, 1080)
    }
}

/// A networked entity field read by name (`Host::entity_field`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FieldValue {
    Int(i32),
    Float(f32),
    Bool(bool),
}

/// A persistent variable's value as `GetPersistentVar` returns it
/// (client.dll 0x1802c88a0): an enum gives its item's name, and the `NULL`
/// item gives null.
#[derive(Clone, Debug, PartialEq)]
pub enum PersistentValue {
    Bool(bool),
    Int(i32),
    Float(f32),
    String(String),
    Enum(String),
    /// A struct: reading one is an error.
    Struct,
}

/// An entity, as the host names it.
pub type EntityId = u32;

/// What client script can read of an entity. The host fills it from the
/// game; the script functions apply the game's own rules to it (angle
/// normalizing and so on).
#[derive(Clone, Debug, Default)]
pub struct EntityInfo {
    /// The script class of its C++ class (`C_Portal_Player`, `C_WeaponX`…).
    pub script_class: String,
    pub classname: String,
    /// Its script-facing type name (`player`, `npc_titan`…).
    pub signifier: String,
    pub name: String,
    pub ent_index: i32,
    pub origin: [f32; 3],
    pub local_origin: [f32; 3],
    pub angles: [f32; 3],
    pub velocity: [f32; 3],
    pub eye_position: [f32; 3],
    pub eye_angles: [f32; 3],
    pub team: i32,
    pub health: i32,
    pub max_health: i32,
    pub life_state: i32,
    pub is_player: bool,
    pub is_npc: bool,
    pub is_titan: bool,
    pub client_only: bool,
    /// A player's current camera (`CameraPosition`/`CameraAngles`): the
    /// view the game renders from (eye, third-person or a script camera).
    pub camera_origin: [f32; 3],
    pub camera_angles: [f32; 3],
    /// Player fields as the server networks them: cinematic event flags
    /// (client.dll +0x2a54), total XP (+0x3610), a bad reputation
    /// (+0x2bd8), when a Titan is next available (+0x3638) and the player
    /// settings (class) name.
    pub cinematic_event_flags: i32,
    pub xp: i32,
    pub bad_reputation: bool,
    pub next_titan_respawn_available: f32,
    pub player_settings: String,
    /// A player's cockpit entity (client.dll +0x3220), if it has one.
    pub cockpit: Option<EntityId>,
    /// The player's pet Titan (+0x35e8), its mode (+0x364c), when its hot
    /// drop lands (+0x2a40), and the player settings' class
    /// (`GetPlayerClass`).
    pub pet_titan: Option<EntityId>,
    pub pet_titan_mode: i32,
    pub hot_drop_impact_time: f32,
    pub player_class: String,
    /// The model's path (`GetModelName`).
    pub model: String,
    /// The player's objective (client.dll +0x2a48) and when it ends
    /// (+0x2a50).
    pub objective_index: i32,
    pub objective_end_time: f32,
    /// A player's burn card slot as the game networks it (0: none; script
    /// sees one less).
    pub active_burn_card_index: i32,
}

/// What the game rules report about a team.
#[derive(Clone, Debug, Default)]
pub struct TeamInfo {
    pub name: String,
    pub score: i32,
    pub score2: i32,
    pub kills: i32,
    pub deaths: i32,
    pub hashtag: String,
}

pub struct ClientVm {
    pub vm: Vm,
    pub host: Rc<dyn Host>,
    /// The entity script classes and instances (set up by `start`).
    pub entities: Option<entities::Shared>,
    /// HUD elements and VGUI panels (set up by `start`).
    pub hud: Option<hud::SharedHud>,
    /// Button, stick and console-command callbacks (set up by `start`).
    pub input: Option<input::Shared>,
    /// Model effect definitions (set up by `start`).
    pub model_fx: Option<modelfx::Shared>,
    /// Rumble graphs and play params (set up by `start`).
    pub rumble: Option<rumble::Shared>,
    /// The functions the server may call (set up by `start`).
    pub remote: Option<remote::Shared>,
    /// The XP table scripts fill (set up by `start`).
    pub xp_table: Option<Rc<std::cell::RefCell<progression::XpTable>>>,
}

impl ClientVm {
    /// A client VM with client.dll's functions, before any script runs.
    pub fn new(host: Rc<dyn Host>) -> ClientVm {
        let mut vm = Vm::new();
        vm.developer = host.developer();
        ClientVm {
            vm,
            host,
            entities: None,
            hud: None,
            input: None,
            model_fx: None,
            rumble: None,
            remote: None,
            xp_table: None,
        }
    }

    /// Run the startup, in client.dll's order. Errors are the first
    /// script error, as text.
    pub fn start(&mut self) -> Result<(), String> {
        for f in ["init", "_consts", "_threads"] {
            self.run_file(f)?;
        }
        for &(name, value) in consts::ROOT {
            self.vm
                .root
                .borrow_mut()
                .insert(Value::str(name), Value::Int(value));
        }
        for &(name, value) in consts::CONST {
            self.vm
                .consts
                .borrow_mut()
                .insert(Value::str(name), Value::Int(value));
        }
        globals::install(self);
        remote::install(self);
        gamerules::install(self);
        engine::install(self);
        progression::install(self);
        weapons::install(self);
        misc::install(self);
        self.input = Some(input::install(self));
        self.model_fx = Some(modelfx::install(self));
        self.rumble = Some(rumble::install(self));
        persistence::install(self);
        let ents = entities::install(self);
        clientents::install(self, &ents);
        let hud = hud::install(self);
        hud::install_crosshair(self, &hud);
        hud::install_screens(self, &hud, &ents);
        self.entities = Some(ents);
        self.hud = Some(hud);
        self.run_file("vscript_client")?;
        // client.dll includes `_cl_mapspawn` with no scope: the root table.
        let root = Value::Table(self.vm.root.clone());
        include(&mut self.vm, &*self.host, "_cl_mapspawn", root)
            .map_err(|e| self.vm.to_display(&e))?;
        Ok(())
    }

    fn run_file(&mut self, name: &str) -> Result<(), String> {
        let path = script_path(name).ok_or_else(|| format!("bad script name {name}"))?;
        let text = self
            .host
            .read_script(&path)
            .ok_or_else(|| format!("script not found: {path}"))?;
        self.vm
            .run_script(&text, &path)
            .map(|_| ())
            .map_err(|e| self.vm.to_display(&e))
    }
}

/// `scripts/vscripts/<name>`: `.nut` added if there's no extension; any
/// other extension is refused (client.dll 0x1802cb080).
pub fn script_path(name: &str) -> Option<String> {
    let file = match name.rfind('.') {
        None => format!("{name}.nut"),
        Some(i) if name[i..].eq_ignore_ascii_case(".nut") => name.to_string(),
        Some(_) => return None,
    };
    Some(format!("scripts/vscripts/{file}"))
}

/// Compile a script file and run it with `scope` as `this`.
pub fn include(vm: &mut Vm, host: &dyn Host, name: &str, scope: Value) -> Result<(), Value> {
    let fail = || Value::str(&format!("Failed to include script \"{name}\""));
    let path = script_path(name).ok_or_else(fail)?;
    let text = host.read_script(&path).ok_or_else(fail)?;
    let f = vm.compile(&text, &path).map_err(|m| Value::str(&m))?;
    // No scope means the root table.
    let scope = if scope.is_null() {
        Value::Table(vm.root.clone())
    } else {
        scope
    };
    vm.call(&f, scope, &[])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_paths() {
        assert_eq!(
            script_path("client/cl_main_hud").as_deref(),
            Some("scripts/vscripts/client/cl_main_hud.nut")
        );
        assert_eq!(
            script_path("_flag.nut").as_deref(),
            Some("scripts/vscripts/_flag.nut")
        );
        assert_eq!(script_path("data.txt"), None);
    }
}
