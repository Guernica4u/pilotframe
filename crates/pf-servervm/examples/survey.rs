//! Survey tool: start the server VM from the player's install with the
//! server functions ported so far, and list every engine function and
//! entity method the scripts use that we don't have yet, marking the ones
//! client.dll has a twin of (from the local research inventory
//! `Resources/re/natives.json`, when it's there).
//!
//! A missing name gets a stand-in that returns null, so one run finds them
//! all; results after the first stand-in are only a guide. Usage:
//! `cargo run -p pf-servervm --example survey [map] [frames]`.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::rc::Rc;

use pf_clientvm::Host;
use pf_servervm::ServerVm;
use pf_squirrel::{Value, native};

struct SurveyHost {
    level: String,
    time: Cell<f32>,
    pdef: Rc<pf_clientvm::persistence::PersistenceDef>,
}

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
    fn persistence_def(&self) -> Option<Rc<pf_clientvm::persistence::PersistenceDef>> {
        Some(self.pdef.clone())
    }
    fn convar(&self, name: &str) -> String {
        match name {
            "mp_gamemode" => "tdm".into(),
            _ => String::new(),
        }
    }
}

/// Names client.dll registers, from the local inventory (empty if absent).
fn client_names(root: &Path) -> HashSet<String> {
    let Ok(text) = std::fs::read_to_string(root.join("re/natives.json")) else {
        return HashSet::new();
    };
    // Just the "name" fields of records from client.dll.
    let mut out = HashSet::new();
    for rec in text.split("\n {").skip(1) {
        let field = |k: &str| {
            rec.split(&format!("\"{k}\": \""))
                .nth(1)
                .and_then(|r| r.split('"').next())
                .map(str::to_string)
        };
        if field("dll").as_deref() == Some("client")
            && let Some(n) = field("name")
        {
            out.insert(n);
        }
    }
    out
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
    let twins = client_names(&root);

    let host = Rc::new(SurveyHost {
        level: format!("maps/{map}.bsp"),
        time: Default::default(),
        pdef: Rc::new(pf_clientvm::pdef::build(&pf_gamefs::read_text).0),
    });
    let mut svm = ServerVm::new(host.clone());
    let log = Rc::new(RefCell::new(String::new()));
    let sink = log.clone();
    svm.inner.vm.out = Box::new(move |s| sink.borrow_mut().push_str(s));

    let missing: Rc<RefCell<BTreeMap<String, (usize, usize)>>> = Default::default();
    let m = missing.clone();
    let mut order = 0usize;
    svm.inner.vm.missing_hook = Some(Box::new(move |obj, key| {
        let Value::Str(k) = key else { return None };
        let k = k.to_string_lossy();
        let name = match obj {
            Value::Instance(_) => format!("instance.{k}"),
            Value::Table(_) => k.clone(),
            _ => return None,
        };
        m.borrow_mut().entry(name).or_insert((order, 0)).1 += 1;
        order += 1;
        Some(native(&k, 0, |_, _| Ok(Value::Null)))
    }));

    if let Err(e) = svm.start() {
        println!("ERROR during startup: {e}");
    }
    for i in 0..frames {
        host.time.set(i as f32 / 60.0);
        svm.inner.vm.run_frame(i as f32 / 60.0);
    }

    let mut list: Vec<(String, usize, usize)> = missing
        .borrow()
        .iter()
        .map(|(k, &(first, n))| (k.clone(), first, n))
        .collect();
    list.sort_by_key(|e| e.1);
    let with_twin = list.iter().filter(|e| twins.contains(&e.0)).count();
    println!(
        "== {} missing names (first use order; {with_twin} have a client.dll twin, marked *) ==",
        list.len()
    );
    for (name, _, n) in &list {
        let mark = if twins.contains(name) { "*" } else { " " };
        println!("{mark} {name}\t{n}");
    }
    println!("== script output ==");
    print!("{}", log.borrow());
}
