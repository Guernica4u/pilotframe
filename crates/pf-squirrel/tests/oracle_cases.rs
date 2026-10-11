//! Every oracle case (oracle/cases.txt, run in the real game for comparison)
//! must parse: one compile error would stop the whole file in the game.

#[test]
fn oracle_cases_parse() {
    let text = include_str!("../oracle/cases.txt");
    let mut bad = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let (id, code) = line.split_once(' ').unwrap_or((line, ""));
        let wrapped = format!("try {{ (function() {{ {code} }})() }} catch ( e ) {{ print(e) }}");
        if let Err(e) = pf_squirrel::parse(&wrapped) {
            bad.push(format!("{id}: {e}"));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}
