//! Script threads: `thread`, `wait`, `waitthread` and `delaythread`, driven
//! by game time.

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
    // Threads run at once until their first wait.
    assert_eq!(*log.borrow(), "xa wa main ");
    vm.update(0.6);
    assert_eq!(*log.borrow(), "xa wa main xb ");
    vm.update(1.0);
    // The worker finishes; its waiter carries on next update.
    assert_eq!(*log.borrow(), "xa wa main xb wb ");
    vm.update(1.1);
    assert_eq!(*log.borrow(), "xa wa main xb wb boss ");
    vm.update(2.0);
    assert_eq!(*log.borrow(), "xa wa main xb wb boss da ");
    vm.update(2.1);
    assert_eq!(*log.borrow(), "xa wa main xb wb boss da db ");
}

#[test]
fn waiting_outside_a_thread_is_an_error() {
    let (mut vm, _) = vm_with_log();
    let err = vm.run_script("wait 1", "test").unwrap_err();
    assert!(matches!(err, pf_squirrel::Value::Str(_)));
}
