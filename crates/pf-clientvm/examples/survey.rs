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
/// The player's cockpit (in the game the HUD hangs off it).
const COCKPIT: EntityId = 2;

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
        if id == COCKPIT {
            return Some(EntityInfo {
                script_class: "C_Titan_Cockpit".into(),
                classname: "titan_cockpit".into(),
                signifier: "titan_cockpit".into(),
                ent_index: 2,
                team: 2,
                // A pilot class's `cockpitmodel` (scripts/players/mp/*.set).
                model: "models/weapons/arms/human_pov_cockpit.mdl".into(),
                ..Default::default()
            });
        }
        (id == PLAYER).then(|| EntityInfo {
            script_class: "C_Portal_Player".into(),
            classname: "player".into(),
            signifier: "player".into(),
            ent_index: 1,
            team: 2,
            health: 200,
            max_health: 200,
            is_player: true,
            cockpit: Some(COCKPIT),
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
    // Ours: a stand-in server reports the match playing (pf-clientvm
    // standin.rs). The HUD's set-up reads the game state, so it goes
    // first; when the real client gets the server's vars relative to the
    // connect scripts is still to be worked out (docs/port/scripts.md).
    cvm.standin_match_playing();
    // The engine then reports the local player connected.
    cvm.run_client_connect_scripts(PLAYER);
    // Then it runs the creation callbacks of the entities it has.
    cvm.entities_created(&[PLAYER, COCKPIT]);
    for i in 0..frames {
        host.time.set(i as f32 / 60.0);
        cvm.vm.run_frame(i as f32 / 60.0);
        cvm.input_frame((i * 1000 / 60) as u32);
        cvm.hud_frame(i as f32 / 60.0);
        cvm.autotext_frame(i as f32 / 60.0, i as f32 / 60.0);
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

    // PF_DUMP_HUD=1: every panel under the HUD root and the screens, with
    // its rectangle in its root's pixels; "-" marks a hidden one (it or a
    // parent).
    if std::env::var_os("PF_DUMP_HUD").is_some()
        && let Some(hud) = &cvm.hud
    {
        let h = hud.borrow();
        let ui = &h.ui;
        let mut roots: Vec<(String, usize)> = Vec::new();
        if let Some(r) = ui.hud_root {
            roots.push(("hud".into(), r));
        }
        for s in &ui.screens {
            roots.push((format!("screen {}", s.name), s.root));
        }
        for (label, root) in roots {
            println!("== {label} ==");
            dump(ui, root, 0.0, 0.0, true, 0);
        }
    }
}

fn dump(ui: &pf_vgui::Ui, id: usize, ox: f32, oy: f32, shown: bool, depth: usize) {
    let Some(p) = ui.panel(id) else { return };
    let shown = shown && p.visible;
    let (x, y) = (ox + p.x, oy + p.y);
    let mut what = String::new();
    if let Some(i) = &p.image {
        what += &format!(" image={i}");
    }
    if !p.text.is_empty() {
        what += &format!(" text={:?}", p.text);
    }
    if p.control == pf_vgui::Control::ProgressBar {
        what += &format!(" bar={:.2}", p.bar_progress);
    }
    println!(
        "{}{} {} {:?} [{x:.0},{y:.0} {:.0}x{:.0}] fg={:?} a={:.0}{what}",
        "  ".repeat(depth),
        if shown { "+" } else { "-" },
        p.name,
        p.control,
        p.w,
        p.h,
        p.fg_color,
        p.panel_alpha,
    );
    for &c in &p.children {
        dump(ui, c, x, y, shown, depth + 1);
    }
}
