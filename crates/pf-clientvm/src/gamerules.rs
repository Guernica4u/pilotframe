//! `GameRules`: the client's instance of client.dll's `CGameRules` class
//! (registered at 0x18044fca0). Team indices must be within the team list
//! ("Invalid team index: %d").

use std::rc::Rc;

use pf_squirrel::table::Table;
use pf_squirrel::value::UserData;
use pf_squirrel::{NativeError, Value, native, throw};

use crate::{ClientVm, Host, TeamInfo};

fn team(host: &dyn Host, a: &[Value]) -> Result<TeamInfo, NativeError> {
    let i = match a.get(1) {
        Some(Value::Int(i)) => *i,
        Some(Value::Float(x)) => *x as i32,
        _ => -1,
    };
    if i < 0 || i >= host.team_count() {
        return Err(throw(format!("Invalid team index: {i}")));
    }
    Ok(host.team(i))
}

pub fn install(cvm: &mut ClientVm) {
    let mut members = Table::new();
    // GetGameMode (0x1804492b0): the `mp_gamemode` console variable.
    let host = cvm.host.clone();
    members.insert(
        Value::str("GetGameMode"),
        native("GetGameMode", 1, move |_, _| {
            Ok(Value::str(&host.convar("mp_gamemode")))
        }),
    );
    type Field = fn(&TeamInfo) -> Value;
    let fields: [(&str, Field); 6] = [
        ("GetTeamName", |t| Value::str(&t.name)),
        ("GetTeamScore", |t| Value::Int(t.score)),
        ("GetTeamScore2", |t| Value::Int(t.score2)),
        ("GetTeamKills", |t| Value::Int(t.kills)),
        ("GetTeamDeaths", |t| Value::Int(t.deaths)),
        ("GetTeamHashtag", |t| Value::str(&t.hashtag)),
    ];
    for (name, field) in fields {
        let host = cvm.host.clone();
        members.insert(
            Value::str(name),
            native(name, 2, move |_, a| Ok(field(&team(&*host, a)?))),
        );
    }
    let class =
        crate::classes::register(&mut cvm.vm, "CGameRules", "CScriptGameRules", None, members);
    let instance = crate::classes::instantiate(&mut cvm.vm, &class, UserData::Host(Rc::new(())))
        .unwrap_or_default();
    cvm.vm
        .root
        .borrow_mut()
        .insert(Value::str("GameRules"), instance);
}
