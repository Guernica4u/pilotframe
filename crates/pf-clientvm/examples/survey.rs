//! Survey tool: start the client VM from the player's install, with the
//! client.dll functions ported so far, and list every engine function and
//! entity method the scripts use that we don't have yet.
//!
//! A missing name gets a stand-in that returns null (through the VM's
//! debugging hook), so one run finds them all; results after the first
//! stand-in are only a guide. Usage:
//! `cargo run -p pf-clientvm --example survey [map] [frames]`.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;

use pf_clientvm::persistence::PersistenceDef;
use pf_clientvm::{ClientVm, EntityId, EntityInfo, Host};
use pf_squirrel::{Value, native};

/// A stand-in game: Colony, Attrition, one local pilot on IMC.
struct SurveyHost {
    level: String,
    time: Cell<f32>,
    pdef: Rc<PersistenceDef>,
}

const PLAYER: EntityId = 1;

impl Host for SurveyHost {
    fn read_script(&self, path: &str) -> Option<String> {
        pf_gamefs::read_text(path)
    }
    fn level_name(&self) -> String {
        self.level.clone()
    }
    fn time(&self) -> f32 {
        self.time.get()
    }
    fn frame_time(&self) -> f32 {
        1.0 / 60.0
    }
    fn convar(&self, name: &str) -> String {
        match name {
            // Attrition.
            "mp_gamemode" => "tdm".into(),
            _ => String::new(),
        }
    }
    fn entity(&self, id: EntityId) -> Option<EntityInfo> {
        (id == PLAYER).then(|| EntityInfo {
            script_class: "C_Portal_Player".into(),
            classname: "player".into(),
            signifier: "player".into(),
            ent_index: 1,
            team: 2,
            health: 200,
            max_health: 200,
            is_player: true,
            ..Default::default()
        })
    }
    fn players(&self) -> Vec<EntityId> {
        vec![PLAYER]
    }
    fn local_player(&self) -> Option<EntityId> {
        Some(PLAYER)
    }
    /// Survey stand-in: every persistent path reads as 0 until the server
    /// work brings the real definitions.
    fn persistent_var(&self, _: &str) -> Option<pf_clientvm::PersistentValue> {
        Some(pf_clientvm::PersistentValue::Int(0))
    }
    fn persistence_def(&self) -> Option<Rc<PersistenceDef>> {
        Some(self.pdef.clone())
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Resources");
    let Some(install) = pf_gamefs::find_install(&root) else {
        eprintln!("no install found under {}", root.display());
        return;
    };
    pf_gamefs::mount_install(&install, None).expect("mount install");
    let map = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "mp_colony".into());
    let frames: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    // The persistence definition, built from the install's scripts.
    let (pdef, problems) = pf_clientvm::pdef::build(&pf_gamefs::read_text);
    for p in &problems {
        println!("persistence: {p}");
    }
    let host = Rc::new(SurveyHost {
        level: format!("maps/{map}.bsp"),
        time: Default::default(),
        pdef: Rc::new(pdef),
    });
    let mut cvm = ClientVm::new(host.clone());
    let log = Rc::new(RefCell::new(String::new()));
    let sink = log.clone();
    cvm.vm.out = Box::new(move |s| sink.borrow_mut().push_str(s));

    // Failed lookups: recorded (as `Class.method` on entities), answered
    // with a stand-in function.
    let missing: Rc<RefCell<BTreeMap<String, (usize, usize)>>> = Default::default();
    let m = missing.clone();
    let h = host.clone();
    let mut order = 0usize;
    cvm.vm.missing_hook = Some(Box::new(move |obj, key| {
        let Value::Str(k) = key else { return None };
        let k = k.to_string_lossy();
        let name = match obj {
            Value::Instance(_) => match pf_clientvm::entities::entity_id(obj) {
                Some(id) => format!(
                    "{}.{k}",
                    h.entity(id).map(|e| e.script_class).unwrap_or_default()
                ),
                None => format!("instance.{k}"),
            },
            Value::Table(_) => k.clone(),
            // A method on a non-object (null, a number…): not a missing
            // engine function; let it fail with the script's trace.
            _ => return None,
        };
        m.borrow_mut().entry(name).or_insert((order, 0)).1 += 1;
        order += 1;
        Some(native(&k, 0, |_, _| Ok(Value::Null)))
    }));

    if let Err(e) = cvm.start() {
        println!("ERROR during startup: {e}");
    }
    // The engine then reports the local player connected.
    cvm.run_client_connect_scripts(PLAYER);
    // Ours: a stand-in server reports the match playing (pf-clientvm
    // standin.rs), once the connect scripts have set up the HUD.
    cvm.standin_match_playing();
    for i in 0..frames {
        host.time.set(i as f32 / 60.0);
        cvm.vm.run_frame(i as f32 / 60.0);
        cvm.input_frame((i * 1000 / 60) as u32);
        cvm.hud_frame(i as f32 / 60.0);
    }

    let mut list: Vec<(String, usize, usize)> = missing
        .borrow()
        .iter()
        .map(|(k, &(first, n))| (k.clone(), first, n))
        .collect();
    list.sort_by_key(|e| e.1);
    println!("== {} missing names (first use order) ==", list.len());
    for (name, _, n) in &list {
        println!("{name}\t{n}");
    }
    println!("== script output ==");
    print!("{}", log.borrow());
}
