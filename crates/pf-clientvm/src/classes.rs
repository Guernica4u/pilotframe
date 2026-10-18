//! Registering a native class the way launcher.dll does: the class goes in
//! the root table under its script name, then the script hook
//! `VSquirrel_OnRegisteredClass( cppName )` (init.nut) runs, which includes
//! `class/<cppname lower>.nut` if it exists. Those files add fields, methods
//! and a constructor to the class. Subclasses copy their base's members
//! when they're made (Squirrel), so a base class is registered, with its
//! file run, before its subclasses.

use std::rc::Rc;

use pf_squirrel::table::Table;
use pf_squirrel::value::{Class, Instance, Ref, UserData, new_ref};
use pf_squirrel::{Value, Vm};

/// Make a class: a copy of `base`'s members plus `members`, then run the
/// hook. Errors from the class file are printed, as the game does.
pub fn register(
    vm: &mut Vm,
    script_name: &str,
    cpp_name: &str,
    base: Option<&Ref<Class>>,
    members: Table,
) -> Ref<Class> {
    let mut all = base
        .map(|b| b.borrow().members.shallow_clone())
        .unwrap_or_default();
    for (k, v) in members.iter() {
        all.insert(k.clone(), v.clone());
    }
    let class = new_ref(Class {
        base: base.cloned(),
        members: all,
        locked: false,
    });
    vm.root
        .borrow_mut()
        .insert(Value::str(script_name), Value::Class(class.clone()));
    let hook = vm
        .root
        .borrow()
        .get(&Value::str("VSquirrel_OnRegisteredClass"))
        .cloned();
    if let Some(hook) = hook {
        let root = Value::Table(vm.root.clone());
        if let Err(e) = vm.call(&hook, root, &[Value::str(cpp_name)]) {
            vm.report_error(&e);
        }
    }
    class
}

/// An instance of a native class holding `user`, with the class's
/// constructor (from its class file, if any) run on it.
pub fn instantiate(vm: &mut Vm, class: &Ref<Class>, user: UserData) -> Result<Value, Value> {
    class.borrow_mut().locked = true;
    let mut values = Table::new();
    for (k, v) in class.borrow().members.iter() {
        if !matches!(v, Value::Closure(_) | Value::Native(_)) {
            values.insert(k.clone(), v.clone());
        }
    }
    let inst = Value::Instance(new_ref(Instance {
        class: class.clone(),
        values,
        user,
    }));
    let ctor = class
        .borrow()
        .members
        .get(&Value::str("constructor"))
        .cloned();
    if let Some(ctor) = ctor {
        vm.call(&ctor, inst.clone(), &[])?;
    }
    Ok(inst)
}

/// The host value an instance carries.
pub fn user_of<T: 'static + Copy>(v: &Value) -> Option<T> {
    let Value::Instance(i) = v else { return None };
    match &i.borrow().user {
        UserData::Host(rc) => rc.downcast_ref::<T>().copied(),
        _ => None,
    }
}

/// Wrap a host value for an instance.
pub fn user<T: 'static>(v: T) -> UserData {
    UserData::Host(Rc::new(v))
}
