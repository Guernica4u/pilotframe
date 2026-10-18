//! Server → client remote calls, with synthetic scripts.

use std::cell::RefCell;
use std::rc::Rc;

use pf_clientvm::remote::{RemoteArg, RemoteCall};
use pf_clientvm::{ClientVm, EntityId, EntityInfo, Host};

/// One player, entity 1.
struct OnePlayer;

impl Host for OnePlayer {
    fn read_script(&self, _: &str) -> Option<String> {
        None
    }
    fn level_name(&self) -> String {
        String::new()
    }
    fn time(&self) -> f32 {
        0.0
    }
    fn frame_time(&self) -> f32 {
        0.0
    }
    fn entity(&self, id: EntityId) -> Option<EntityInfo> {
        (id == 1).then(|| EntityInfo {
            script_class: "C_BasePlayer".into(),
            classname: "player".into(),
            is_player: true,
            ent_index: 1,
            ..Default::default()
        })
    }
    fn local_player(&self) -> Option<EntityId> {
        Some(1)
    }
}

#[test]
fn server_calls_reach_the_client_by_number() {
    let host = Rc::new(OnePlayer);
    // The server side: register two functions and send calls.
    let mut server = ClientVm::new(host.clone());
    let outbox = pf_servervm::remote::install(&mut server);
    let ents = pf_clientvm::entities::install(&mut server);
    server.entities = Some(ents.clone());
    let player = pf_clientvm::entities::Entities::instance(&ents, &mut server.vm, &*host, 1);
    server
        .vm
        .root
        .borrow_mut()
        .insert(pf_squirrel::Value::str("player"), player);
    server
        .vm
        .run_script(
            r#"
            Remote.BeginRegisteringFunctions()
            Remote.RegisterFunction( "First" )
            Remote.RegisterFunction( "Second" )
            Remote.EndRegisteringFunctions()
            Remote.CallFunction_Replay( player, "Second", 7, 1.5, true, null )
            try { Remote.CallFunction_NonReplay( player, "Second", [] ) } catch ( e ) {}
            "#,
            "server",
        )
        .unwrap();
    let calls: Vec<RemoteCall> = outbox.borrow().clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].index, 1);
    assert_eq!(
        calls[0].args,
        vec![
            RemoteArg::Int(7),
            RemoteArg::Float(1.5),
            RemoteArg::Bool(true),
            RemoteArg::Null
        ]
    );

    // The client side runs it in the player's scope.
    let mut client = ClientVm::new(host.clone());
    let log = Rc::new(RefCell::new(String::new()));
    let sink = log.clone();
    client.vm.out = Box::new(move |s| sink.borrow_mut().push_str(s));
    client
        .vm
        .run_script(include_str!("init_scope.nut"), "init")
        .unwrap();
    pf_clientvm::remote::install(&mut client);
    let ents = pf_clientvm::entities::install(&mut client);
    client.entities = Some(ents);
    client
        .vm
        .run_script(
            r#"
            Remote.BeginRegisteringFunctions()
            Remote.RegisterFunction( "First" )
            Remote.RegisterFunction( "Second" )
            Remote.EndRegisteringFunctions()
            function Second( a, b, c, d ) { print( a + " " + b + " " + c + " " + d + " " + ( "self" in this ) ) }
            "#,
            "client",
        )
        .unwrap();
    client.run_remote_calls(&calls);
    assert_eq!(*log.borrow(), "7 1.5 true (null : 0x0000000000000000) true");
}
