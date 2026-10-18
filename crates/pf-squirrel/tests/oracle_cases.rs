//! The oracle cases (oracle/cases.txt) against the real game's answers
//! (oracle/game_results_run*.txt): our parser must accept every case the game
//! compiled, and reject every case the game refused to compile, with the same
//! message.

use std::collections::HashMap;

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

/// Compile errors the game reports (runtime errors are for the interpreter).
const COMPILE_ERRORS: &[&str] = &[
    "invalid numeric format",
    "expression expected",
    "string expected",
    "end of statement expected (; or lf)",
    "function with default parameters cannot have variable number of parameters",
];

#[test]
fn parser_agrees_with_the_game() {
    let game = game_results();
    let mut bad = Vec::new();
    for line in include_str!("../oracle/cases.txt").lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let (id, code) = line.split_once(' ').unwrap_or((line, ""));
        // Compiled as the oracle file does: `}; ` becomes a line break.
        let code = code.replace("}; ", "}\n");
        let wrapped = format!("try {{ (function() {{ {code} }})() }} catch ( e ) {{ print(e) }}");
        let ours = pf_squirrel::parse(&wrapped);
        let expected_error = game.get(id).and_then(|r| {
            r.strip_prefix("ERR|")
                .filter(|m| COMPILE_ERRORS.contains(m))
                .map(str::to_string)
        });
        match (ours, expected_error) {
            (Ok(_), None) => {}
            (Err(e), Some(msg)) if e.message.starts_with(&msg) => {}
            (Ok(_), Some(msg)) => bad.push(format!("{id}: game refused ({msg}), we accept")),
            (Err(e), None) => bad.push(format!("{id}: game compiled it, we refuse: {e}")),
            (Err(e), Some(msg)) => bad.push(format!("{id}: game says {msg:?}, we say {e}")),
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}
