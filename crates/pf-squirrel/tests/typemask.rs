//! Native functions with Squirrel type masks (`sq_setparamscheck`), the way
//! launcher.dll checks the arguments of every engine function
//! (docs/port/scripts.md, "Argument checks").

use std::cell::RefCell;
use std::rc::Rc;

use pf_squirrel::{Vm, compile_typemask};

fn run(vm: &mut Vm, src: &str) -> String {
    let log = Rc::new(RefCell::new(String::new()));
    let sink = log.clone();
    vm.out = Box::new(move |s| sink.borrow_mut().push_str(s));
    vm.run_script(src, "test").unwrap();
    log.take()
}

#[test]
fn masks_compile_like_squirrel() {
    assert_eq!(compile_typemask(".n"), Some(vec![u32::MAX, 0x6]));
    assert_eq!(
        compile_typemask(".s|o x"),
        Some(vec![u32::MAX, 0x11, 0x8000])
    );
    assert_eq!(compile_typemask(".s|"), None);
    assert_eq!(compile_typemask(".q"), None);
}

#[test]
fn wrong_argument_types_are_refused() {
    let mut vm = Vm::new();
    vm.register_checked("Two", 3, ".ns", |_, a| Ok(a[1].clone()));
    let out = run(
        &mut vm,
        r#"
        print( Two( 1.5, "a" ) + " " )
        try { Two( "x", "a" ) } catch ( e ) { print( e + "\n" ) }
        try { Two( 1, null ) } catch ( e ) { print( e + "\n" ) }
        try { Two( 1 ) } catch ( e ) { print( e + "\n" ) }
        "#,
    );
    assert_eq!(
        out,
        "1.5 parameter 1 has an invalid type 'string' ; expected: 'integer|float'\n\
         parameter 2 has an invalid type 'null' ; expected: 'string'\n\
         wrong number of parameters: found 2, expected 3\n"
    );
}
