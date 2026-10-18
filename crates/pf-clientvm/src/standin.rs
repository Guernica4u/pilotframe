//! **Ours, a marked stand-in** for the game server (owner's call,
//! 2026-10-17: HUD first, the real server scripts in pf-servervm later).
//!
//! It sends the client the same remote calls the server's scripts would
//! (`ServerCallback_SetServerVar( handle, value )`), so the client scripts
//! take their normal path; only the values are chosen by us. Remove once
//! pf-servervm runs a match.

use pf_squirrel::Value;

use crate::ClientVm;
use crate::remote::{RemoteArg, RemoteCall};

impl ClientVm {
    /// Set a server var the way the server does: by its handle (its place
    /// in the client's `_serverVarHandles`), through the remote function.
    /// False if the var or the function isn't registered.
    pub fn standin_set_server_var(&mut self, name: &str, value: RemoteArg) -> bool {
        let Some(remote) = self.remote.clone() else {
            return false;
        };
        let Some(index) = remote
            .borrow()
            .names
            .iter()
            .position(|n| n == "ServerCallback_SetServerVar")
        else {
            return false;
        };
        let root = Value::Table(self.vm.root.clone());
        let handles = self
            .vm
            .try_get(&root, &Value::str("_serverVarHandles"))
            .ok()
            .flatten();
        let Some(Value::Table(t)) = handles else {
            return false;
        };
        let handle = t.borrow().iter().find_map(|(k, v)| match (k, v) {
            (Value::Int(h), Value::Str(s)) if s.to_string_lossy() == name => Some(*h),
            _ => None,
        });
        let Some(handle) = handle else {
            return false;
        };
        let player = self.host.local_player().unwrap_or_default();
        self.run_remote_calls(&[RemoteCall {
            player,
            index,
            replay: true,
            ui: false,
            args: vec![RemoteArg::Int(handle), value],
        }]);
        true
    }

    /// A compile-time enum value from the scripts (`eGameState.Playing`).
    pub fn script_enum(&self, name: &str, item: &str) -> Option<i32> {
        let consts = self.vm.consts.borrow();
        match consts.get(&Value::str(name))? {
            Value::Table(t) => match t.borrow().get(&Value::str(item))? {
                Value::Int(n) => Some(*n),
                _ => None,
            },
            _ => None,
        }
    }

    /// The match as our stand-in server reports it: Playing, started now.
    pub fn standin_match_playing(&mut self) {
        let now = self.host.time();
        if let Some(playing) = self.script_enum("eGameState", "Playing") {
            self.standin_set_server_var("gameState", RemoteArg::Int(playing));
        }
        self.standin_set_server_var("gameStartTime", RemoteArg::Float(now));
    }
}
