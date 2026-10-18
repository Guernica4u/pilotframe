//! Building the persistent data definition from the install's scripts
//! (ours: the game's server loaded it from a file the PC install doesn't
//! have; owner's decision 2026-10-17). Each enum has one rule naming the
//! script list it comes from; the rules, and how each was checked, are in
//! docs/port/scripts.md ("Persistence"). Nothing read here is stored in
//! the repository.

use pf_squirrel::ast::{Expr, visit_exprs};

use crate::persistence::PersistenceDef;

/// The definition, and the problems met (a missing script, an enum the
/// engine would refuse).
pub fn build(read: &dyn Fn(&str) -> Option<String>) -> (PersistenceDef, Vec<String>) {
    let mut def = PersistenceDef::default();
    let mut problems = Vec::new();
    let mut add =
        |name: &str, items: Option<Vec<String>>, source: &str, placeholder: bool| match items {
            Some(items) => {
                if let Err(e) = def.add_enum(name, &items, source, placeholder) {
                    problems.push(e);
                }
            }
            None => problems.push(format!("{name}: couldn't read {source}")),
        };

    // titanDecals: `NULL`, then each `CreateDecalData( type, ref, ... )` in
    // _items.nut in file order. Checked against the Titan models' skin
    // families, which hold the decals in enum order (examples/pdef_check).
    let decals = string_args(read, "scripts/vscripts/_items.nut", "CreateDecalData", 1)
        .map(|refs| with_null(&refs));
    add(
        "titanDecals",
        decals,
        "_items.nut CreateDecalData refs, after NULL",
        false,
    );

    // challenge and dailychallenge: each `AddChallenge( ref, ... )` in
    // _challenges_content.nut in file order, lower-cased (AddChallenge
    // does the same: "Persistent data is stored as lower case"), split by
    // the category last set: DAILY ones form dailychallenge. `challenge`
    // starts with NULL (scripts treat index 0 as "none"); for
    // dailychallenge that's assumed. Challenges the script has commented
    // out may still have been in the original enums: unconfirmed.
    let challenges = challenge_refs(read);
    add(
        "challenge",
        challenges.as_ref().map(|(c, _)| with_null(c)),
        "_challenges_content.nut AddChallenge refs (not DAILY), after NULL",
        true,
    );
    add(
        "dailychallenge",
        challenges.as_ref().map(|(_, d)| with_null(d)),
        "_challenges_content.nut AddChallenge refs (DAILY), after NULL",
        true,
    );

    (def, problems)
}

/// Challenge refs: (regular, daily).
fn challenge_refs(read: &dyn Fn(&str) -> Option<String>) -> Option<(Vec<String>, Vec<String>)> {
    let text = read("scripts/vscripts/_challenges_content.nut")?;
    let stmts = pf_squirrel::parse(&text).ok()?;
    let (mut regular, mut daily, mut in_daily) = (Vec::new(), Vec::new(), false);
    visit_exprs(&stmts, &mut |e| {
        let Expr::Call(callee, args) = e else { return };
        let Expr::Ident(func) = &**callee else { return };
        match &**func {
            "SetChallengeCategory" => {
                in_daily = matches!(args.first(), Some(Expr::Field(_, c)) if &**c == "DAILY");
            }
            "AddChallenge" => {
                if let Some(Expr::Str(r)) = args.first() {
                    let r = r.to_lowercase();
                    if in_daily {
                        daily.push(r)
                    } else {
                        regular.push(r)
                    }
                }
            }
            _ => {}
        }
    });
    Some((regular, daily))
}

fn with_null(items: &[String]) -> Vec<String> {
    std::iter::once("NULL".to_string())
        .chain(items.iter().cloned())
        .collect()
}

/// The string literal at `arg` of every call to the plain function `func`
/// in a script, in source order (calls whose argument isn't a string
/// literal are skipped). None if the script is missing or doesn't parse.
pub fn string_args(
    read: &dyn Fn(&str) -> Option<String>,
    path: &str,
    func: &str,
    arg: usize,
) -> Option<Vec<String>> {
    let text = read(path)?;
    let stmts = pf_squirrel::parse(&text).ok()?;
    let mut out = Vec::new();
    visit_exprs(&stmts, &mut |e| {
        if let Expr::Call(callee, args) = e
            && matches!(&**callee, Expr::Ident(n) if &**n == func)
            && let Some(Expr::Str(s)) = args.get(arg)
        {
            out.push(s.to_string());
        }
    });
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_call_arguments_in_order() {
        let src = r#"
            function Init()
            {
                Make( 1, "a" )
                // Make( 1, "commented" )
                if ( x ) Make( 2, "b" )
                Other( 1, "c" )
                Make( 3, someVar )
            }
        "#;
        let read = |_: &str| Some(src.to_string());
        assert_eq!(
            string_args(&read, "x.nut", "Make", 1),
            Some(vec!["a".to_string(), "b".to_string()])
        );
    }
}
