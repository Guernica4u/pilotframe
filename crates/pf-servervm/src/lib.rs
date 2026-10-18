//! The game's server script VM: our Squirrel interpreter running the game's
//! own server scripts (owner's decision 2026-10-17: our game plays the
//! server's part with its real scripts).
//!
//! The PC game ships no server.dll, so the server's engine functions can't
//! be read from code. Each one is ported from client.dll where client.dll
//! has the same function (a "twin"), or worked out from how the scripts use
//! it; which is which is noted beside each (docs/port/server.md).
//!
//! Startup (ours, mirroring client.dll's order, docs/port/scripts.md):
//! 1. the VM runs `init.nut`, `_consts.nut` and `_threads.nut`;
//! 2. the server functions are registered;
//! 3. `vscript_server.nut` runs;
//! 4. `_mapspawn` is included into the root table.

use std::rc::Rc;

use pf_clientvm::{ClientVm, Host};

pub mod remote;
use pf_squirrel::Value;

/// The server VM. It reuses the client VM's frame (the VM and the host
/// interface); only the functions registered differ.
pub struct ServerVm {
    pub inner: ClientVm,
    /// Calls for the clients (`Remote.CallFunction_*`), set up by `start`.
    pub outbox: Option<remote::Outbox>,
}

impl ServerVm {
    pub fn new(host: Rc<dyn Host>) -> ServerVm {
        ServerVm {
            inner: ClientVm::new(host),
            outbox: None,
        }
    }

    /// Run the startup. Errors are the first script error, as text.
    pub fn start(&mut self) -> Result<(), String> {
        for f in ["init", "_consts", "_threads"] {
            self.run_file(f)?;
        }
        install(&mut self.inner);
        self.outbox = Some(remote::install(&mut self.inner));
        self.run_file("vscript_server")?;
        let root = Value::Table(self.inner.vm.root.clone());
        let vm = &mut self.inner.vm;
        pf_clientvm::include(vm, &*self.inner.host, "_mapspawn", root)
            .map_err(|e| vm.to_display(&e))?;
        Ok(())
    }

    fn run_file(&mut self, name: &str) -> Result<(), String> {
        let path =
            pf_clientvm::script_path(name).ok_or_else(|| format!("bad script name {name}"))?;
        let text = self
            .inner
            .host
            .read_script(&path)
            .ok_or_else(|| format!("script not found: {path}"))?;
        let vm = &mut self.inner.vm;
        vm.run_script(&text, &path)
            .map(|_| ())
            .map_err(|e| vm.to_display(&e))
    }
}

/// The server's functions.
fn install(cvm: &mut ClientVm) {
    // Twins: the functions client.dll registers that the server has under
    // the same name and meaning (including, from, time, random numbers…).
    pf_clientvm::globals::install(cvm);
    pf_clientvm::gamerules::install(cvm);
    pf_clientvm::engine::install(cvm);
    pf_clientvm::progression::install(cvm);
    pf_clientvm::weapons::install(cvm);
    pf_clientvm::misc::install(cvm);
    pf_clientvm::persistence::install(cvm);
    // Which VM this is: the server (the twins of client.dll 0x1802c1f90,
    // 0x1802c1fc0, 0x1802c1f80 with the opposite answers).
    let vm = &mut cvm.vm;
    vm.register("IsServer", 1, |_, _| Ok(Value::Bool(true)));
    vm.register("IsClient", 1, |_, _| Ok(Value::Bool(false)));
    vm.register("IsUI", 1, |_, _| Ok(Value::Bool(false)));
}
