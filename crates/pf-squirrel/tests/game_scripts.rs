//! Every script in the player's install must parse. Skipped (passes) when the
//! install isn't present, so it's safe in CI.

use std::path::Path;

fn mount() -> bool {
    // Tests run from the crate folder; the install lives under the workspace.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Resources");
    match pf_gamefs::find_install(&root) {
        Some(install) => pf_gamefs::mount_install(&install, None).is_ok(),
        None => false,
    }
}

#[test]
fn all_game_scripts_parse() {
    if !mount() {
        eprintln!("no install found; skipping");
        return;
    }
    let files: Vec<String> = pf_gamefs::list("scripts/vscripts/")
        .into_iter()
        .filter(|f| f.ends_with(".nut"))
        .collect();
    assert!(files.len() > 400, "only {} scripts found", files.len());
    let mut failures = Vec::new();
    for f in &files {
        let text = pf_gamefs::read_text(f).unwrap_or_default();
        if let Err(e) = pf_squirrel::parse(&text) {
            failures.push(format!("{f}: {e}"));
        }
    }
    for f in failures.iter().take(40) {
        eprintln!("{f}");
    }
    assert!(
        failures.is_empty(),
        "{} of {} scripts failed to parse",
        failures.len(),
        files.len()
    );
}

#[test]
fn all_game_scripts_compile() {
    if !mount() {
        eprintln!("no install found; skipping");
        return;
    }
    let vm = pf_squirrel::Vm::new();
    let mut failures = Vec::new();
    let files: Vec<String> = pf_gamefs::list("scripts/vscripts/")
        .into_iter()
        .filter(|f| f.ends_with(".nut"))
        .collect();
    for f in &files {
        let text = pf_gamefs::read_text(f).unwrap_or_default();
        let stmts = pf_squirrel::parse(&text).expect("parses");
        let consts = vm.consts.clone();
        if let Err(e) = pf_squirrel::compiler::compile(&stmts, f, &mut consts.borrow_mut()) {
            failures.push(format!("{f}:{}: {}", e.line, e.message));
        }
    }
    for f in failures.iter().take(40) {
        eprintln!("{f}");
    }
    assert!(
        failures.is_empty(),
        "{} scripts failed to compile",
        failures.len()
    );
}
