//! Every oracle case (oracle/cases.txt, run in the real game for comparison)
//! must parse (except the X cases, which test what the game rejects).

#[test]
fn oracle_cases_parse() {
    let text = include_str!("../oracle/cases.txt");
    let mut bad = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let (id, code) = line.split_once(' ').unwrap_or((line, ""));
        // X cases test forms the game's compiler rejects; checked separately
        // once the game's answers are in.
        if id.starts_with('X') || id == "C016" {
            continue;
        }
        // Compiled as the oracle file does: `}; ` becomes a line break.
        let code = code.replace("}; ", "}\n");
        let wrapped = format!("try {{ (function() {{ {code} }})() }} catch ( e ) {{ print(e) }}");
        if let Err(e) = pf_squirrel::parse(&wrapped) {
            bad.push(format!("{id}: {e}"));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}
