//! `Remote`: the client's instance of client.dll's `CRemoteFunctions`
//! class (registered at 0x1804338d0). Scripts list, in order, the client
//! functions the server may call; the order is each function's number in
//! remote calls. Up to 256 names in 0x2000 bytes.

use std::cell::RefCell;
use std::rc::Rc;

use pf_squirrel::table::Table;
use pf_squirrel::value::UserData;
use pf_squirrel::{Value, native, throw};

use crate::ClientVm;

#[derive(Default)]
pub struct RemoteFunctions {
    pub began: bool,
    pub ended: bool,
    /// In registration order: a function's index is its number.
    pub names: Vec<String>,
}

const MAX_ENTRIES: usize = 0x100;
const MAX_BYTES: usize = 0x2000;

pub fn install(cvm: &mut ClientVm) {
    let state = Rc::new(RefCell::new(RemoteFunctions::default()));
    let mut members = Table::new();

    let s = state.clone();
    members.insert(
        Value::str("BeginRegisteringFunctions"),
        native("BeginRegisteringFunctions", 1, move |_, _| {
            let mut s = s.borrow_mut();
            s.began = true;
            s.ended = false;
            s.names.clear();
            Ok(Value::Null)
        }),
    );
    // 0x180422d40
    let s = state.clone();
    members.insert(
        Value::str("RegisterFunction"),
        native("RegisterFunction", 2, move |_, a| {
            let name = match &a[1] {
                Value::Str(n) => n.to_string_lossy(),
                _ => return Err(throw("RegisterFunction: expected a name")),
            };
            let mut s = s.borrow_mut();
            if !s.began {
                return Err(throw(format!(
                    "Can't register '{name}' - Haven't called BeginRegisteringFunctions()."
                )));
            }
            if s.ended {
                return Err(throw(format!(
                    "Can't register '{name}' - Already called EndRegisteringFunctions()."
                )));
            }
            let used: usize = s.names.iter().map(|n| n.len() + 1).sum();
            if s.names.len() >= MAX_ENTRIES || used + name.len() + 1 > MAX_BYTES {
                return Err(throw(format!(
                    "Out of room to register client function '{name}' - Usage: {}/{MAX_ENTRIES} entries, {used}/{MAX_BYTES} buffer.",
                    s.names.len()
                )));
            }
            s.names.push(name);
            Ok(Value::Null)
        }),
    );
    // 0x180422ce0
    let s = state.clone();
    members.insert(
        Value::str("EndRegisteringFunctions"),
        native("EndRegisteringFunctions", 1, move |_, _| {
            let mut s = s.borrow_mut();
            if !s.began {
                return Err(throw(
                    "Can't end - Never called BeginRegisteringFunctions().",
                ));
            }
            if s.ended {
                return Err(throw("Already called EndRegisteringFunctions()."));
            }
            s.ended = true;
            Ok(Value::Null)
        }),
    );

    let class = crate::classes::register(
        &mut cvm.vm,
        "CRemoteFunctions",
        "CClientScriptRemoteFunctions",
        None,
        members,
    );
    let instance =
        crate::classes::instantiate(&mut cvm.vm, &class, UserData::Host(state)).unwrap_or_default();
    cvm.vm
        .root
        .borrow_mut()
        .insert(Value::str("Remote"), instance);
}
