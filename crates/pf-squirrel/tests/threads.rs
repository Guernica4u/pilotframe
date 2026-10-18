//! Script threads and signals, run the way launcher.dll runs them
//! (docs/port/scripts.md, "Threads and signals").

use std::cell::RefCell;
use std::rc::Rc;

fn vm_with_log() -> (pf_squirrel::Vm, Rc<RefCell<String>>) {
    let log = Rc::new(RefCell::new(String::new()));
    let mut vm = pf_squirrel::Vm::new();
    let sink = log.clone();
    vm.out = Box::new(move |s| sink.borrow_mut().push_str(s));
    (vm, log)
}

#[test]
fn threads_wait_on_game_time() {
    let (mut vm, log) = vm_with_log();
    vm.run_script(
        r#"
        function Worker( name, t )
        {
            print( name + "a " )
            wait t
            print( name + "b " )
        }
        function Boss()
        {
            waitthread Worker( "w", 1.0 )
            print( "boss " )
        }
        thread Worker( "x", 0.5 )
        thread Boss()
        delaythread( 2.0 ) Worker( "d", 0 )
        print( "main " )
        "#,
        "test",
    )
    .unwrap();
    // A new thread runs at once until it first waits.
    assert_eq!(*log.borrow(), "xa wa main ");
    vm.run_frame(0.6);
    assert_eq!(*log.borrow(), "xa wa main xb ");
    // The worker finishes; its waiter carries on in the next round of the
    // same frame.
    vm.run_frame(1.0);
    assert_eq!(*log.borrow(), "xa wa main xb wb boss ");
    vm.run_frame(2.0);
    assert_eq!(*log.borrow(), "xa wa main xb wb boss da ");
    // `wait 0` waits until the next frame.
    vm.run_frame(2.1);
    assert_eq!(*log.borrow(), "xa wa main xb wb boss da db ");
}

#[test]
fn signals_wake_and_end_threads() {
    let (mut vm, log) = vm_with_log();
    vm.run_script(
        r#"
        RegisterSignal( "Go" )
        RegisterSignal( "Stop" )
        ::owner <- {}
        function Waiter()
        {
            local r = WaitSignal( owner, "Go" )
            print( "got " + r.signal + " " + r.x + " " )
        }
        function Ender()
        {
            EndSignal( owner, "Stop" )
            OnThreadEnd( function() { print( "ended " ) } )
            wait 100
            print( "never " )
        }
        thread Waiter()
        thread Ender()
        Signal( owner, "Go", { x = 5 } )
        print( "sent " )
        "#,
        "test",
    )
    .unwrap();
    // Signalled outside a frame: the waiter carries on at the next frame.
    assert_eq!(*log.borrow(), "sent ");
    vm.run_frame(0.1);
    assert_eq!(*log.borrow(), "sent got Go 5 ");
    // An end signal ends the thread at once, running its OnThreadEnd.
    vm.run_script("Signal( owner, \"Stop\" )", "test").unwrap();
    assert_eq!(*log.borrow(), "sent got Go 5 ended ");
    vm.run_frame(200.0);
    assert_eq!(*log.borrow(), "sent got Go 5 ended ");
}

#[test]
fn waiting_outside_a_thread_is_an_error() {
    let (mut vm, _) = vm_with_log();
    assert!(vm.run_script("wait 1", "test").is_err());
    assert!(vm.run_script("Signal( {}, \"Nope\" )", "test").is_err());
}
