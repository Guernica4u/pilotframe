//! Survey tool: start the client VM from the player's install, with the
//! client.dll functions ported so far, and list every engine function the
//! scripts call that we don't have yet.
//!
//! A missing name gets a stand-in that returns null, so one run finds them
//! all (results after the first stand-in are only a guide). Usage:
//! `cargo run -p pf-clientvm --example survey [map] [frames]`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;

use pf_clientvm::{ClientVm, Host};
use pf_squirrel::table::Table;
use pf_squirrel::value::new_ref;
use pf_squirrel::{Value, native};

struct FileHost {
    level: String,
    time: std::cell::Cell<f32>,
}

impl Host for FileHost {
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

    let host = Rc::new(FileHost {
        level: format!("maps/{map}.bsp"),
        time: Default::default(),
    });
    let mut cvm = ClientVm::new(host.clone());
    let log = Rc::new(RefCell::new(String::new()));
    let sink = log.clone();
    cvm.vm.out = Box::new(move |s| sink.borrow_mut().push_str(s));

    // Root-table lookups that fail: recorded, answered with a stand-in.
    let missing: Rc<RefCell<BTreeMap<String, (usize, usize)>>> = Default::default();
    let order = Rc::new(RefCell::new(0usize));
    let m = missing.clone();
    let fallback = native("_get", 2, move |_, a| {
        let key = match &a[1] {
            Value::Str(s) => s.to_string_lossy(),
            other => format!("{other:?}"),
        };
        let mut o = order.borrow_mut();
        m.borrow_mut().entry(key.clone()).or_insert((*o, 0)).1 += 1;
        *o += 1;
        Ok(native(&key, 0, |_, _| Ok(Value::Null)))
    });
    let mut delegate = Table::new();
    delegate.insert(Value::str("_get"), fallback);
    cvm.vm.root.borrow_mut().delegate = Some(new_ref(delegate));

    if let Err(e) = cvm.start() {
        println!("ERROR during startup: {e}");
    }
    for i in 0..frames {
        host.time.set(i as f32 / 60.0);
        cvm.vm.run_frame(i as f32 / 60.0);
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
