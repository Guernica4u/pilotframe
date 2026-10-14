//! Survey tool: run the client VM's startup from the player's install the
//! way client.dll does, and list every engine function the scripts call
//! that we don't have yet.
//!
//! The order (client.dll's client VM setup): the VM runs `init.nut`,
//! `_consts.nut` and `_threads.nut`; the engine registers its functions;
//! `vscript_client.nut` runs; then `_cl_mapspawn` is included.
//!
//! A missing name gets a stand-in that returns null, so the run goes on and
//! one pass finds them all (the results after the first stand-in are only a
//! guide). Usage: `cargo run -p pf-squirrel --example client_vm [frames]`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;

use pf_squirrel::{Value, Vm, native};

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Resources");
    let Some(install) = pf_gamefs::find_install(&root) else {
        eprintln!("no install found under {}", root.display());
        return;
    };
    pf_gamefs::mount_install(&install, None).expect("mount install");

    let mut vm = Vm::new();
    let log = Rc::new(RefCell::new(String::new()));
    let sink = log.clone();
    vm.out = Box::new(move |s| sink.borrow_mut().push_str(s));

    // DoIncludeScript( name, scope ): compile scripts/vscripts/<name>.nut
    // and run it with `scope` as `this`.
    vm.register("DoIncludeScript", 3, |vm, a| {
        let Value::Str(name) = &a[1] else {
            return Err(pf_squirrel::throw("DoIncludeScript: expected a name"));
        };
        let path = format!("scripts/vscripts/{}.nut", name.to_string_lossy());
        let Some(text) = pf_gamefs::read_text(&path) else {
            return Err(pf_squirrel::throw(format!("script not found: {path}")));
        };
        let f = vm.compile(&text, &path).map_err(pf_squirrel::throw)?;
        vm.call(&f, a[2].clone(), &[])?;
        Ok(Value::Bool(true))
    });

    // Every name looked up in the root table and not found: recorded, and
    // answered with a stand-in function that returns null.
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
    let mut delegate = pf_squirrel::table::Table::new();
    delegate.insert(Value::str("_get"), fallback);
    vm.root.borrow_mut().delegate = Some(pf_squirrel::value::new_ref(delegate));

    let run = |vm: &mut Vm, what: &str, src: &str| {
        let r = vm.run_script(src, what);
        if let Err(e) = r {
            println!("ERROR in {what}: {}", vm.to_display(&e));
        }
    };
    for f in ["init", "_consts", "_threads", "vscript_client"] {
        let path = format!("scripts/vscripts/{f}.nut");
        let text = pf_gamefs::read_text(&path).expect("startup script");
        run(&mut vm, f, &text);
    }
    run(&mut vm, "include", "IncludeFile( \"_cl_mapspawn\" )");

    let frames: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    for i in 0..frames {
        vm.run_frame(i as f32 / 60.0);
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
