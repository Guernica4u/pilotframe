//! `Remote`: the client's instance of client.dll's `CRemoteFunctions`
//! class (registered at 0x1804338d0). Scripts list, in order, the client
//! functions the server may call; the order is each function's number in
//! remote calls. Up to 256 names in 0x2000 bytes.
//!
//! The server's `Remote` (pf-servervm) registers the same list and adds
//! `CallFunction_Replay/NonReplay/UI`, which queue `RemoteCall`s; the client
//! runs them each frame (`ClientVm::run_remote_calls`, client.dll
//! 0x180427850).

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

/// One argument of a remote call: what the message can carry (client.dll
/// 0x180427010 reads a 2-bit type: null, bool, 32-bit int, 32-bit float).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RemoteArg {
    Null,
    Bool(bool),
    Int(i32),
    Float(f32),
}

impl RemoteArg {
    pub fn to_value(self) -> Value {
        match self {
            RemoteArg::Null => Value::Null,
            RemoteArg::Bool(b) => Value::Bool(b),
            RemoteArg::Int(i) => Value::Int(i),
            RemoteArg::Float(f) => Value::Float(f),
        }
    }
}

/// At most 10 arguments (the count is 4 bits; more than 10 is refused).
pub const MAX_REMOTE_ARGS: usize = 10;
/// Queued calls a client keeps (client.dll: "Too many remote function calls
/// for player - no free slots out of 600 total.").
pub const MAX_QUEUED_CALLS: usize = 600;

/// A call the server sent to one player's client.
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteCall {
    pub player: crate::EntityId,
    /// The function's number: its place in the registered list.
    pub index: usize,
    /// `CallFunction_Replay` (also seen in kill replays).
    pub replay: bool,
    /// `CallFunction_UI`: runs in the UI VM.
    pub ui: bool,
    pub args: Vec<RemoteArg>,
}

pub type Shared = Rc<RefCell<RemoteFunctions>>;

pub fn install(cvm: &mut ClientVm) {
    let (class_members, state) = members();
    let class = crate::classes::register(
        &mut cvm.vm,
        "CRemoteFunctions",
        "CClientScriptRemoteFunctions",
        None,
        class_members,
    );
    let instance = crate::classes::instantiate(&mut cvm.vm, &class, UserData::Host(state.clone()))
        .unwrap_or_default();
    cvm.vm
        .root
        .borrow_mut()
        .insert(Value::str("Remote"), instance);
    cvm.remote = Some(state);
}

/// The registration methods both VMs share, and their state.
pub fn members() -> (Table, Shared) {
    let state: Shared = Rc::new(RefCell::new(RemoteFunctions::default()));
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

    (members, state)
}

impl ClientVm {
    /// Run calls the server sent (client.dll 0x180427850): each by its
    /// number in our own list; a client call is looked up through the
    /// player's scope and run with that scope as `this`. UI calls need the
    /// UI VM, which we don't run yet: they're skipped (placeholder).
    pub fn run_remote_calls(&mut self, calls: &[RemoteCall]) {
        let (Some(remote), Some(ents)) = (self.remote.clone(), self.entities.clone()) else {
            return;
        };
        for c in calls.iter().take(MAX_QUEUED_CALLS) {
            let Some(name) = remote.borrow().names.get(c.index).cloned() else {
                continue;
            };
            if c.ui {
                continue;
            }
            let id = self.host.local_player().unwrap_or(c.player);
            let scope =
                crate::entities::Entities::validate_scope(&ents, &mut self.vm, &*self.host, id);
            if scope.is_null() {
                continue;
            }
            let Ok(Some(f)) = self.vm.try_get(&scope, &Value::str(&name)) else {
                continue;
            };
            let args: Vec<Value> = c.args.iter().map(|a| a.to_value()).collect();
            if let Err(e) = self.vm.call(&f, scope, &args) {
                self.vm.report_error(&e);
            }
        }
    }
}
