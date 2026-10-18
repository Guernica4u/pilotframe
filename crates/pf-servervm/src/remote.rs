//! The server's `Remote`: the same registration as the client's (the twin
//! of client.dll's `CRemoteFunctions`), plus the calls that send a function
//! call to one player's client: `CallFunction_Replay`,
//! `CallFunction_NonReplay` and `CallFunction_UI` ( player, name, ... ).
//!
//! What a call can carry is ported from the client's reader (client.dll
//! 0x180427010): at most 10 arguments, each null, a bool, a 32-bit integer
//! or a 32-bit float. The server's own checks aren't in the install (no
//! server.dll); the error texts here are ours.

use std::cell::RefCell;
use std::rc::Rc;

use pf_clientvm::ClientVm;
use pf_clientvm::remote::{MAX_REMOTE_ARGS, RemoteArg, RemoteCall};
use pf_squirrel::value::UserData;
use pf_squirrel::{Value, native, throw};

/// Calls waiting to be delivered to the clients.
pub type Outbox = Rc<RefCell<Vec<RemoteCall>>>;

pub fn install(cvm: &mut ClientVm) -> Outbox {
    let (mut members, state) = pf_clientvm::remote::members();
    let outbox: Outbox = Rc::default();
    for (name, replay, ui) in [
        ("CallFunction_Replay", true, false),
        ("CallFunction_NonReplay", false, false),
        ("CallFunction_UI", false, true),
    ] {
        let (st, out) = (state.clone(), outbox.clone());
        members.insert(
            Value::str(name),
            native(name, -3, move |_, a| {
                let player = pf_clientvm::entities::entity_id(&a[1])
                    .ok_or_else(|| throw(format!("{name}: first argument must be a player")))?;
                let Value::Str(f) = &a[2] else {
                    return Err(throw(format!("{name}: second argument must be a function name")));
                };
                let f = f.to_string_lossy();
                let index = st
                    .borrow()
                    .names
                    .iter()
                    .position(|n| *n == f)
                    .ok_or_else(|| throw(format!("{name}: '{f}' is not a registered remote function")))?;
                let rest = &a[3..];
                if rest.len() > MAX_REMOTE_ARGS {
                    return Err(throw(format!(
                        "{name}: at most {MAX_REMOTE_ARGS} arguments can be sent"
                    )));
                }
                let mut args = Vec::with_capacity(rest.len());
                for (i, v) in rest.iter().enumerate() {
                    args.push(match v {
                        Value::Null => RemoteArg::Null,
                        Value::Bool(b) => RemoteArg::Bool(*b),
                        Value::Int(n) => RemoteArg::Int(*n),
                        Value::Float(x) => RemoteArg::Float(*x),
                        other => {
                            return Err(throw(format!(
                                "{name}: argument {} is a {}; only null, bool, integer and float can be sent",
                                i + 1,
                                other.type_name()
                            )));
                        }
                    });
                }
                out.borrow_mut().push(RemoteCall {
                    player,
                    index,
                    replay,
                    ui,
                    args,
                });
                Ok(Value::Null)
            }),
        );
    }
    let class = pf_clientvm::classes::register(
        &mut cvm.vm,
        "CRemoteFunctions",
        "CServerScriptRemoteFunctions",
        None,
        members,
    );
    let instance =
        pf_clientvm::classes::instantiate(&mut cvm.vm, &class, UserData::Host(state.clone()))
            .unwrap_or_default();
    cvm.vm
        .root
        .borrow_mut()
        .insert(Value::str("Remote"), instance);
    cvm.remote = Some(state);
    outbox
}
