//! The oracle cases (oracle/cases.txt) run in our interpreter, against what
//! the real game printed (oracle/game_results_run*.txt). Every line must
//! match exactly. Runs the cases the way the oracle file does in the game:
//! `T` and `P` defined in the root table, each case compiled on its own with
//! `compilestring` and called, errors caught and printed.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// The game's result for each case; later runs override earlier ones.
fn game_results() -> HashMap<String, String> {
    let mut out = HashMap::new();
    for text in [
        include_str!("../oracle/game_results_run1.txt"),
        include_str!("../oracle/game_results_run2.txt"),
    ] {
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            if let Some((id, rest)) = line.split_once('|') {
                out.insert(id.to_string(), rest.to_string());
            }
        }
    }
    out
}

/// The oracle file's header: the same `T` and `P` the game ran.
const HEADER: &str = r#"
::T <- function( id, v )
{
	local s
	try { s = "" + v } catch ( e ) { s = "<" + typeof v + ">" }
	print( id + "|" + typeof v + "|" + s + "\n" )
}
::P <- function( id, s ) { print( id + "|" + s + "\n" ) }
"#;

/// Squirrel string literal escaping, as oracle.py does it.
fn escape(code: &str) -> String {
    code.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace("}; ", "}\\n")
}

#[test]
fn interpreter_matches_the_game() {
    let printed = Rc::new(RefCell::new(String::new()));
    let mut vm = pf_squirrel::Vm::new();
    let sink = printed.clone();
    vm.out = Box::new(move |s| sink.borrow_mut().push_str(s));
    vm.run_script(HEADER, "header").expect("header runs");

    let game = game_results();
    let mut bad = Vec::new();
    let mut checked = 0;
    let mut time = 0.0f32;
    for line in include_str!("../oracle/cases.txt").lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let (id, code) = line.split_once(' ').unwrap_or((line, ""));
        // Cases the game hasn't run yet are run too (their output shows with
        // PF_ORACLE_SHOW=1) but not checked.
        let expected = game.get(id);
        printed.borrow_mut().clear();
        let src = format!(
            "try {{ compilestring( \"{}\" )() }} catch ( e ) {{ print( \"{id}|ERR|\" + e + \"\\n\" ) }}",
            escape(code)
        );
        if let Err(e) = vm.run_script(&src, id) {
            bad.push(format!("{id}: harness failed: {e:?}"));
            continue;
        }
        // Thread cases print a few frames later: run a second of frames.
        for _ in 0..60 {
            time += 1.0 / 60.0;
            vm.run_frame(time);
        }
        let out = printed.borrow().clone();
        // The game's console log drops unprintable characters (tabs stay,
        // oracle S015; character 1 vanished, M004), so ours are dropped too.
        let ours: String = out
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{id}|")))
            .unwrap_or("<nothing printed>")
            .chars()
            .filter(|c| !c.is_control() || *c == '\t')
            .collect();
        let Some(expected) = expected else {
            if std::env::var_os("PF_ORACLE_SHOW").is_some() {
                eprintln!("{id}|{ours}");
            }
            continue;
        };
        checked += 1;
        if &ours != expected {
            bad.push(format!("{id}: game {expected:?}, ours {ours:?}"));
        }
    }
    assert!(checked > 140, "only {checked} cases checked");
    assert!(
        bad.is_empty(),
        "{} of {checked} differ:\n{}",
        bad.len(),
        bad.join("\n")
    );
}
