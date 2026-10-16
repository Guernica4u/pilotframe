//! Runs compiled scripts. The behaviour (how values index, compare, add up
//! and fail, and the error messages) follows launcher.dll's interpreter as
//! written up in docs/port/scripts.md and checked by the oracle cases.
//!
//! Script threads (`threads.rs`) each have their own stack, so one can stop
//! in the middle of a function and carry on later. A call made from Rust (an
//! engine callback, a native function calling back into script) gets a
//! fresh stack and must finish without waiting.

use std::rc::Rc;

use crate::ast::BinOp;
use crate::compiler::{self, Op};
use crate::fmt::float_to_string;
use crate::table::Table;
use crate::value::*;

/// What a native function returns.
pub type NativeResult = Result<Value, NativeError>;

pub enum NativeError {
    /// A script error (usually a message string).
    Throw(Value),
    /// Stop the calling script thread for now (the native has queued it to
    /// carry on later). The call then returns the thread's resume value.
    Suspend,
}

impl From<Value> for NativeError {
    fn from(v: Value) -> NativeError {
        NativeError::Throw(v)
    }
}

/// A script error with this message.
pub fn throw(msg: impl AsRef<str>) -> NativeError {
    NativeError::Throw(Value::str(msg.as_ref()))
}

/// How a run of a stack stopped.
pub(crate) enum Outcome {
    Returned(Value),
    /// The thread stopped for now.
    Suspended,
}

/// How a call started.
pub(crate) enum Start {
    /// Finished at once (a native function, a class without constructor).
    Done(Value),
    /// A script frame was pushed.
    Frame,
    /// A native function stopped the thread.
    Suspend,
}

struct Trap {
    pc: usize,
    stack_len: usize,
}

struct Frame {
    closure: Rc<Closure>,
    pc: usize,
    /// Stack index of `this`; parameters and locals follow.
    base: usize,
    vargv: Vec<Value>,
    traps: Vec<Trap>,
    /// A constructor call returns this instance instead.
    ctor: Option<Value>,
}

pub(crate) struct Exec {
    pub(crate) stack: Vec<Value>,
    frames: Vec<Frame>,
    /// A script thread (may wait), not a call from Rust.
    is_thread: bool,
}

impl Exec {
    pub(crate) fn new(is_thread: bool) -> Exec {
        Exec {
            stack: Vec::new(),
            frames: Vec::new(),
            is_thread,
        }
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().unwrap_or_default()
    }

    fn top(&self) -> &Value {
        self.stack.last().expect("empty script stack")
    }
}

/// Names of metamethods and the per-type method tables.
pub(crate) struct Builtins {
    pub table: Table,
    pub array: Table,
    pub string: Table,
    pub number: Table,
    pub function: Table,
    pub class: Table,
    pub instance: Table,
    pub weakref: Table,
}

/// The host's entity check (see `Vm::entity_is_valid`).
pub type EntityCheck = Box<dyn Fn(&Value) -> bool>;

/// See `Vm::missing_hook`.
pub type MissingHook = Box<dyn FnMut(&Value, &Value) -> Option<Value>>;

/// One script VM (the game has a client VM and a UI VM).
pub struct Vm {
    pub root: Ref<Table>,
    pub consts: Ref<Table>,
    /// The last error raised. The game never clears it, and its arithmetic
    /// error message includes it (oracle E002/E003).
    pub last_error: Value,
    /// Where `print` goes.
    pub out: Box<dyn FnMut(&str)>,
    pub(crate) builtins: Builtins,
    pub(crate) threads: crate::threads::Threads,
    /// The `developer` console variable, as `developer()` reports it.
    pub developer: i32,
    /// The host's answer to "is this instance a live engine entity?"
    /// (`IsValid`). None: no instance is.
    pub entity_is_valid: Option<EntityCheck>,
    /// A debugging aid (not in the game): asked for a value whenever a
    /// lookup would fail with "does not exist" (object, key). The survey
    /// tool uses it to list missing engine functions in one run.
    pub missing_hook: Option<MissingHook>,
    /// Where the last error happened: `source:line` of each frame it passed
    /// through, innermost first (for error reports; not in the game).
    pub error_trace: Vec<String>,
    tracing: bool,
    /// Nested runs (calls from Rust into script), to stop runaway recursion.
    depth: usize,
}

const MAX_DEPTH: usize = 200;
const MAX_FRAMES: usize = 10_000;

impl Default for Vm {
    fn default() -> Vm {
        Vm::new()
    }
}

impl Vm {
    /// A VM with the standard library (base functions, math, string and the
    /// per-type methods) installed.
    pub fn new() -> Vm {
        let mut vm = Vm {
            root: new_ref(Table::new()),
            consts: new_ref(Table::new()),
            last_error: Value::Null,
            out: Box::new(|s| print!("{s}")),
            builtins: Builtins {
                table: Table::new(),
                array: Table::new(),
                string: Table::new(),
                number: Table::new(),
                function: Table::new(),
                class: Table::new(),
                instance: Table::new(),
                weakref: Table::new(),
            },
            threads: Default::default(),
            developer: 0,
            entity_is_valid: None,
            missing_hook: None,
            error_trace: Vec::new(),
            tracing: false,
            depth: 0,
        };
        crate::stdlib::install(&mut vm);
        crate::threads::install(&mut vm);
        crate::vector::install(&mut vm);
        vm
    }

    /// Add a native function to the root table.
    pub fn register(
        &mut self,
        name: &str,
        nparams: i32,
        f: impl Fn(&mut Vm, &[Value]) -> NativeResult + 'static,
    ) {
        let v = native(name, nparams, f);
        self.root.borrow_mut().insert(Value::str(name), v);
    }

    /// Compile a script into a function (its `main`). Errors are the
    /// compiler's message.
    pub fn compile(&mut self, src: &str, source_name: &str) -> Result<Value, String> {
        let stmts = crate::parser::parse(src).map_err(|e| e.message)?;
        let proto = compiler::compile(&stmts, source_name, &mut self.consts.borrow_mut())
            .map_err(|e| e.message)?;
        Ok(Value::Closure(Rc::new(Closure {
            proto,
            free: Vec::new(),
            defaults: Vec::new(),
            env: None,
        })))
    }

    /// Compile and run a script with the root table as `this`.
    pub fn run_script(&mut self, src: &str, source_name: &str) -> Result<Value, Value> {
        let f = self.compile(src, source_name).map_err(|m| Value::str(&m))?;
        let root = Value::Table(self.root.clone());
        self.call(&f, root, &[])
    }

    /// Call a function from Rust. It must finish without waiting.
    pub fn call(&mut self, f: &Value, this: Value, args: &[Value]) -> Result<Value, Value> {
        if self.depth >= MAX_DEPTH {
            return Err(self.raise_str("Native stack overflow"));
        }
        let mut ex = Exec::new(false);
        ex.stack.push(f.clone());
        ex.stack.push(this);
        ex.stack.extend_from_slice(args);
        self.depth += 1;
        let r = self.call_in(&mut ex, 0, args.len());
        self.depth -= 1;
        if self.depth == 0 {
            self.tracing = false;
        }
        r
    }

    fn call_in(&mut self, ex: &mut Exec, at: usize, argc: usize) -> Result<Value, Value> {
        match self.begin_call(ex, at, argc) {
            Ok(Start::Done(v)) => Ok(v),
            Ok(Start::Suspend) => {
                Err(self.raise_str("cannot suspend through native calls/metamethods"))
            }
            Ok(Start::Frame) => {
                let stop = ex.frames.len() - 1;
                match self.run(ex, stop)? {
                    Outcome::Returned(v) => {
                        ex.stack.truncate(at);
                        Ok(v)
                    }
                    Outcome::Suspended => {
                        Err(self.raise_str("cannot suspend through native calls/metamethods"))
                    }
                }
            }
            Err(e) => {
                self.last_error = e.clone();
                Err(e)
            }
        }
    }

    // ---- Running frames ----

    /// Set up a call to the value at `stack[at]` (`this` and `argc`
    /// arguments follow it).
    pub(crate) fn begin_call(
        &mut self,
        ex: &mut Exec,
        at: usize,
        argc: usize,
    ) -> Result<Start, Value> {
        let callee = ex.stack[at].clone();
        match callee {
            Value::Closure(c) => {
                self.push_frame(ex, c, at, argc, None)?;
                Ok(Start::Frame)
            }
            Value::Native(n) => {
                let found = argc as i32 + 1;
                if (n.nparams > 0 && found != n.nparams) || (n.nparams < 0 && found < -n.nparams) {
                    return Err(self.raise_str(&format!(
                        "wrong number of parameters: found {found}, expected {}",
                        n.nparams.abs()
                    )));
                }
                let mut args: Vec<Value> = ex.stack.drain(at + 1..).collect();
                ex.stack.truncate(at);
                if let Some(env) = &n.env {
                    args[0] = env.clone();
                }
                match (n.f)(self, &args) {
                    Ok(v) => Ok(Start::Done(v)),
                    Err(NativeError::Throw(e)) => Err(e),
                    Err(NativeError::Suspend) => Ok(Start::Suspend),
                }
            }
            Value::Class(cls) => {
                let inst = self.instantiate(&cls);
                let ctor = cls
                    .borrow()
                    .members
                    .get(&Value::str("constructor"))
                    .cloned();
                match ctor {
                    Some(Value::Closure(c)) => {
                        ex.stack[at + 1] = inst.clone();
                        self.push_frame(ex, c, at, argc, Some(inst))?;
                        Ok(Start::Frame)
                    }
                    Some(Value::Native(n)) => {
                        let mut args: Vec<Value> = ex.stack.drain(at + 1..).collect();
                        ex.stack.truncate(at);
                        args[0] = inst.clone();
                        match (n.f)(self, &args) {
                            Ok(_) => Ok(Start::Done(inst)),
                            Err(NativeError::Throw(e)) => Err(e),
                            Err(NativeError::Suspend) => {
                                Err(self
                                    .raise_str("cannot suspend through native calls/metamethods"))
                            }
                        }
                    }
                    _ => {
                        ex.stack.truncate(at);
                        Ok(Start::Done(inst))
                    }
                }
            }
            Value::Instance(ref i) => {
                // `_call` metamethod: called with the instance as `this`.
                let mm = i
                    .borrow()
                    .class
                    .borrow()
                    .members
                    .get(&Value::str("_call"))
                    .cloned();
                match mm {
                    Some(f) => {
                        ex.stack[at] = f;
                        ex.stack[at + 1] = callee.clone();
                        self.begin_call(ex, at, argc)
                    }
                    None => {
                        Err(self.raise_str(&format!("attempt to call '{}'", callee.type_name())))
                    }
                }
            }
            other => Err(self.raise_str(&format!("attempt to call '{}'", other.type_name()))),
        }
    }

    /// Check the argument count and push a script frame. The rules
    /// (launcher.dll): missing arguments may come from default values;
    /// extra ones go to `vargv` if the function takes `...`; otherwise
    /// "wrong number of parameters". Counts include `this`.
    fn push_frame(
        &mut self,
        ex: &mut Exec,
        c: Rc<Closure>,
        at: usize,
        argc: usize,
        ctor: Option<Value>,
    ) -> Result<(), Value> {
        if ex.frames.len() >= MAX_FRAMES {
            return Err(self.raise_str("stack overflow"));
        }
        let p = &c.proto;
        let found = argc + 1;
        let mut vargv = Vec::new();
        if found != p.nparams {
            if p.ndefaults > 0 && found < p.nparams && p.nparams - found <= p.ndefaults {
                let first_default = p.nparams - p.ndefaults;
                for i in found..p.nparams {
                    ex.stack.push(c.defaults[i - first_default].clone());
                }
            } else if p.varargs && found >= p.nparams {
                vargv = ex.stack.drain(at + 1 + p.nparams..).collect();
            } else {
                let name = p.name.as_deref().unwrap_or("(unnamed_function)");
                return Err(self.raise_str(&format!(
                    "wrong number of parameters (calling {name}): found {found}, expected {}",
                    p.nparams
                )));
            }
        }
        if let Some(env) = &c.env {
            ex.stack[at + 1] = env.clone();
        }
        ex.frames.push(Frame {
            closure: c,
            pc: 0,
            base: at + 1,
            vargv,
            traps: Vec::new(),
            ctor,
        });
        Ok(())
    }

    /// Run until the frame count drops to `stop` (that frame returned) or
    /// the thread waits. Errors unwind to the nearest `try`, or out.
    pub(crate) fn run(&mut self, ex: &mut Exec, stop: usize) -> Result<Outcome, Value> {
        loop {
            match self.exec(ex, stop) {
                Ok(o) => return Ok(o),
                Err(e) => {
                    self.last_error = e.clone();
                    if !self.tracing {
                        self.error_trace.clear();
                        self.tracing = true;
                    }
                    for f in ex.frames[stop..].iter().rev() {
                        let p = &f.closure.proto;
                        let line = p.lines.get(f.pc.saturating_sub(1)).copied().unwrap_or(0);
                        let name = p.name.as_deref().unwrap_or("(unnamed_function)");
                        self.error_trace.push(format!("{}:{line} {name}", p.source));
                    }
                    // Find a `try` in the frames this run owns.
                    let mut caught = false;
                    while ex.frames.len() > stop {
                        let f = ex.frames.last_mut().expect("frame");
                        if let Some(trap) = f.traps.pop() {
                            f.pc = trap.pc;
                            ex.stack.truncate(trap.stack_len);
                            ex.stack.push(e.clone());
                            caught = true;
                            self.tracing = false;
                            break;
                        }
                        let f = ex.frames.pop().expect("frame");
                        ex.stack.truncate(f.base - 1);
                    }
                    if !caught {
                        return Err(e);
                    }
                }
            }
        }
    }

    fn exec(&mut self, ex: &mut Exec, stop: usize) -> Result<Outcome, Value> {
        loop {
            let fi = ex.frames.len() - 1;
            let (op, base) = {
                let f = &mut ex.frames[fi];
                let op = f.closure.proto.code[f.pc];
                f.pc += 1;
                (op, f.base)
            };
            match op {
                Op::Null => ex.stack.push(Value::Null),
                Op::True => ex.stack.push(Value::Bool(true)),
                Op::False => ex.stack.push(Value::Bool(false)),
                Op::Int(i) => ex.stack.push(Value::Int(i)),
                Op::Float(x) => ex.stack.push(Value::Float(x)),
                Op::Const(k) => {
                    let v = ex.frames[fi].closure.proto.consts[k as usize].clone();
                    ex.stack.push(v);
                }
                Op::Local(i) => {
                    let v = ex.stack[base + i as usize].clone();
                    ex.stack.push(v);
                }
                Op::SetLocal(i) => {
                    let v = ex.top().clone();
                    ex.stack[base + i as usize] = v;
                }
                Op::Free(i) => {
                    let v = ex.frames[fi].closure.free[i as usize].clone();
                    ex.stack.push(v);
                }
                Op::SetFree(_) => return Err(self.raise_str("cannot assign to a free variable")),
                Op::Root => ex.stack.push(Value::Table(self.root.clone())),
                Op::Vargc => {
                    let n = ex.frames[fi].vargv.len();
                    ex.stack.push(Value::Int(n as i32));
                }
                Op::Vargv => {
                    let i = ex.pop();
                    let v = match i {
                        Value::Int(i) => ex.frames[fi].vargv.get(i as usize).cloned(),
                        Value::Float(x) => ex.frames[fi].vargv.get(x as i32 as usize).cloned(),
                        other => {
                            return Err(self.raise_str(&format!(
                                "indexing 'vargv' with {}",
                                other.type_name()
                            )));
                        }
                    };
                    match v {
                        Some(v) => ex.stack.push(v),
                        None => return Err(self.raise_str("vargv index out of range")),
                    }
                }
                Op::GetEnv(k) => {
                    let key = ex.frames[fi].closure.proto.consts[k as usize].clone();
                    let this = ex.stack[base].clone();
                    let v = self.get_env(&this, &key)?;
                    ex.stack.push(v);
                }
                Op::SetEnv(k) => {
                    let key = ex.frames[fi].closure.proto.consts[k as usize].clone();
                    let this = ex.stack[base].clone();
                    let v = ex.top().clone();
                    self.set_env(&this, &key, v)?;
                }
                Op::Get => {
                    let key = ex.pop();
                    let obj = ex.pop();
                    let v = self.get(&obj, &key)?;
                    ex.stack.push(v);
                }
                Op::GetK(k) => {
                    let key = ex.frames[fi].closure.proto.consts[k as usize].clone();
                    let obj = ex.pop();
                    let v = self.get(&obj, &key)?;
                    ex.stack.push(v);
                }
                Op::Set => {
                    let v = ex.pop();
                    let key = ex.pop();
                    let obj = ex.pop();
                    self.set(&obj, &key, v.clone())?;
                    ex.stack.push(v);
                }
                Op::NewSlot => {
                    let v = ex.pop();
                    let key = ex.pop();
                    let obj = ex.pop();
                    self.new_slot(&obj, key, v.clone())?;
                    ex.stack.push(v);
                }
                Op::Delete => {
                    let key = ex.pop();
                    let obj = ex.pop();
                    let v = self.delete(&obj, &key)?;
                    ex.stack.push(v);
                }
                Op::Dup => {
                    let v = ex.top().clone();
                    ex.stack.push(v);
                }
                Op::Dup2 => {
                    let n = ex.stack.len();
                    let (a, b) = (ex.stack[n - 2].clone(), ex.stack[n - 1].clone());
                    ex.stack.push(a);
                    ex.stack.push(b);
                }
                Op::Pop => {
                    ex.stack.pop();
                }
                Op::Swap => {
                    let n = ex.stack.len();
                    ex.stack.swap(n - 1, n - 2);
                }
                Op::Bin(op) => {
                    let b = ex.pop();
                    let a = ex.pop();
                    let v = self.binary(op, &a, &b)?;
                    ex.stack.push(v);
                }
                Op::Neg => {
                    let a = ex.pop();
                    let v = self.negate(&a)?;
                    ex.stack.push(v);
                }
                Op::Not => {
                    let a = ex.pop();
                    ex.stack.push(Value::Bool(!a.truthy()));
                }
                Op::BitNot => match ex.pop() {
                    Value::Int(i) => ex.stack.push(Value::Int(!i)),
                    other => {
                        return Err(self.raise_str(&format!(
                            "attempt to perform a bitwise op on a {}",
                            other.type_name()
                        )));
                    }
                },
                Op::Typeof => {
                    let a = ex.pop();
                    let v = self.type_of(&a)?;
                    ex.stack.push(v);
                }
                Op::Clone => {
                    let a = ex.pop();
                    let v = self.clone_value(&a)?;
                    ex.stack.push(v);
                }
                Op::Resume => {
                    let a = ex.pop();
                    return Err(self.raise_str(&format!(
                        "trying to resume a '{}',only genenerator can be resumed",
                        a.type_name()
                    )));
                }
                Op::Jump(to) => ex.frames[fi].pc = to as usize,
                Op::JumpFalse(to) => {
                    if !ex.pop().truthy() {
                        ex.frames[fi].pc = to as usize;
                    }
                }
                Op::JumpTrue(to) => {
                    if ex.pop().truthy() {
                        ex.frames[fi].pc = to as usize;
                    }
                }
                Op::AndJump(to) => {
                    if ex.top().truthy() {
                        ex.stack.pop();
                    } else {
                        ex.frames[fi].pc = to as usize;
                    }
                }
                Op::OrJump(to) => {
                    if ex.top().truthy() {
                        ex.frames[fi].pc = to as usize;
                    } else {
                        ex.stack.pop();
                    }
                }
                Op::Call(n) => {
                    let at = ex.stack.len() - n as usize - 2;
                    match self.begin_call(ex, at, n as usize)? {
                        Start::Done(v) => ex.stack.push(v),
                        Start::Frame => {}
                        Start::Suspend => {
                            if !ex.is_thread {
                                return Err(self.raise_str(
                                    "Cannot suspend thread from within code function calls",
                                ));
                            }
                            return Ok(Outcome::Suspended);
                        }
                    }
                }
                Op::NewTable(n) => {
                    ex.stack
                        .push(Value::Table(new_ref(Table::with_capacity(n as usize))));
                }
                Op::NewArray(n) => {
                    ex.stack
                        .push(Value::Array(new_ref(Vec::with_capacity(n as usize))));
                }
                Op::ArrayPush => {
                    let v = ex.pop();
                    if let Value::Array(a) = ex.top() {
                        a.borrow_mut().push(v);
                    }
                }
                Op::TableSlot => {
                    let v = ex.pop();
                    let k = ex.pop();
                    if k.is_null() {
                        return Err(self.raise_str("null cannot be used as index"));
                    }
                    if let Value::Table(t) = ex.top() {
                        t.borrow_mut().insert(k, v);
                    }
                }
                Op::Closure(i) => {
                    let proto = ex.frames[fi].closure.proto.protos[i as usize].clone();
                    let free = ex.stack.split_off(ex.stack.len() - proto.nfree);
                    let defaults = ex.stack.split_off(ex.stack.len() - proto.ndefaults);
                    ex.stack.push(Value::Closure(Rc::new(Closure {
                        proto,
                        free,
                        defaults,
                        env: None,
                    })));
                }
                Op::NewClass(has_base) => {
                    let base_class = if has_base {
                        match ex.pop() {
                            Value::Class(c) => Some(c),
                            other => {
                                return Err(self.raise_str(&format!(
                                    "trying to inherit from a {}",
                                    other.type_name()
                                )));
                            }
                        }
                    } else {
                        None
                    };
                    let members = base_class
                        .as_ref()
                        .map(|b| b.borrow().members.shallow_clone())
                        .unwrap_or_default();
                    ex.stack.push(Value::Class(new_ref(Class {
                        base: base_class,
                        members,
                        locked: false,
                    })));
                }
                Op::ClassMember => {
                    let v = ex.pop();
                    let k = ex.pop();
                    let cls = ex.top().clone();
                    self.new_slot(&cls, k, v)?;
                }
                Op::Delegate => {
                    let t = ex.pop();
                    let parent = ex.pop();
                    let Value::Table(table) = &t else {
                        return Err(self.raise_str(&format!("delegating a '{}'", t.type_name())));
                    };
                    match parent {
                        Value::Table(p) => table.borrow_mut().delegate = Some(p),
                        Value::Null => table.borrow_mut().delegate = None,
                        other => {
                            return Err(self
                                .raise_str(&format!("using '{}' as delegate", other.type_name())));
                        }
                    }
                    ex.stack.push(t);
                }
                Op::IncLocal(i, d, post) => {
                    let old = ex.stack[base + i as usize].clone();
                    let new = self.binary(BinOp::Add, &old, &Value::Int(d as i32))?;
                    ex.stack[base + i as usize] = new.clone();
                    ex.stack.push(if post { old } else { new });
                }
                Op::IncFree(..) => return Err(self.raise_str("cannot assign to a free variable")),
                Op::IncEnv(k, d, post) => {
                    let key = ex.frames[fi].closure.proto.consts[k as usize].clone();
                    let this = ex.stack[base].clone();
                    let old = self.get_env(&this, &key)?;
                    let new = self.binary(BinOp::Add, &old, &Value::Int(d as i32))?;
                    self.set_env(&this, &key, new.clone())?;
                    ex.stack.push(if post { old } else { new });
                }
                Op::IncGet(d, post) => {
                    let key = ex.pop();
                    let obj = ex.pop();
                    let old = self.get(&obj, &key)?;
                    let new = self.binary(BinOp::Add, &old, &Value::Int(d as i32))?;
                    self.set(&obj, &key, new.clone())?;
                    ex.stack.push(if post { old } else { new });
                }
                Op::ForeachNext(slot, end) => {
                    let s = base + slot as usize;
                    let container = ex.stack[s].clone();
                    let pos = match ex.stack[s + 1] {
                        Value::Int(i) => i as usize,
                        _ => 0,
                    };
                    let prev_key = ex.stack[s + 2].clone();
                    match self.foreach_next(&container, pos, prev_key)? {
                        Some((next, k, v)) => {
                            ex.stack[s + 1] = Value::Int(next as i32);
                            ex.stack[s + 2] = k;
                            ex.stack[s + 3] = v;
                        }
                        None => ex.frames[fi].pc = end as usize,
                    }
                }
                Op::PushTrap(to) => {
                    let stack_len = ex.stack.len();
                    ex.frames[fi].traps.push(Trap {
                        pc: to as usize,
                        stack_len,
                    });
                }
                Op::PopTrap => {
                    ex.frames[fi].traps.pop();
                }
                Op::Throw => {
                    let e = ex.pop();
                    return Err(e);
                }
                Op::Return | Op::ReturnNull => {
                    let v = if op == Op::Return {
                        ex.pop()
                    } else {
                        Value::Null
                    };
                    let f = ex.frames.pop().expect("frame");
                    let v = f.ctor.unwrap_or(v);
                    ex.stack.truncate(f.base - 1);
                    if ex.frames.len() == stop {
                        return Ok(Outcome::Returned(v));
                    }
                    ex.stack.push(v);
                }
                Op::SetTop(n) => ex.stack.truncate(base + n as usize),
            }
        }
    }

    // ---- Errors ----

    /// An error raised by the VM: a message string.
    pub fn raise_str(&mut self, msg: &str) -> Value {
        Value::str(msg)
    }

    /// A key as error messages show it, cut to 50 characters (`%.50s`).
    fn key_text(&self, key: &Value) -> String {
        let s = self.plain_string(key);
        s.chars().take(50).collect()
    }

    fn index_error(&mut self, obj: &Value, key: &Value) -> Value {
        let k = self.key_text(key);
        match obj {
            Value::Array(_) | Value::Class(_) | Value::Table(_) | Value::Instance(_) => {
                self.raise_str(&format!("the index '{k}' does not exist"))
            }
            other => {
                let msg = format!("cannot index {} with '{k}'", other.type_name());
                self.raise_str(&msg)
            }
        }
    }

    // ---- Getting and setting ----

    /// A name through `this`, then the root table.
    fn get_env(&mut self, this: &Value, key: &Value) -> Result<Value, Value> {
        if let Some(v) = self.try_get(this, key)? {
            return Ok(v);
        }
        if let Some(v) = self.root.borrow().get(key) {
            return Ok(v.clone());
        }
        if let Some(v) = self.missing_hook.as_mut().and_then(|h| h(this, key)) {
            return Ok(v);
        }
        let k = self.key_text(key);
        Err(self.raise_str(&format!("the index '{k}' does not exist")))
    }

    fn set_env(&mut self, this: &Value, key: &Value, v: Value) -> Result<(), Value> {
        if self.try_set(this, key, v.clone())? {
            return Ok(());
        }
        if self.root.borrow_mut().set(key, v) {
            return Ok(());
        }
        let k = self.key_text(key);
        Err(self.raise_str(&format!("the index '{k}' does not exist")))
    }

    /// `obj[key]`, with delegates, metamethods and the per-type methods.
    pub fn get(&mut self, obj: &Value, key: &Value) -> Result<Value, Value> {
        match self.try_get(obj, key)? {
            Some(v) => Ok(v),
            None => {
                if let Some(v) = self.missing_hook.as_mut().and_then(|h| h(obj, key)) {
                    return Ok(v);
                }
                Err(self.index_error(obj, key))
            }
        }
    }

    fn try_get(&mut self, obj: &Value, key: &Value) -> Result<Option<Value>, Value> {
        Ok(match obj {
            Value::Table(t) => return self.table_get(t, obj, key),
            Value::Array(a) => match key {
                Value::Int(_) | Value::Float(_) => {
                    let i = match key {
                        Value::Int(i) => *i,
                        Value::Float(x) => *x as i32,
                        _ => unreachable!(),
                    };
                    if i < 0 {
                        None
                    } else {
                        a.borrow().get(i as usize).cloned()
                    }
                }
                _ => self.builtins.array.get(key).cloned(),
            },
            Value::Str(s) => match key {
                Value::Int(i) => s.bytes().get(*i as usize).map(|&b| Value::Int(b as i32)),
                _ => self.builtins.string.get(key).cloned(),
            },
            Value::Int(_) | Value::Float(_) | Value::Bool(_) => {
                self.builtins.number.get(key).cloned()
            }
            Value::Closure(_) | Value::Native(_) => self.builtins.function.get(key).cloned(),
            Value::Instance(i) => {
                // The instance's fields, then the class (methods), then
                // `_get`, then the instance methods.
                if let Some(v) = i.borrow().values.get(key) {
                    return Ok(Some(v.clone()));
                }
                let class = i.borrow().class.clone();
                if let Some(v) = class.borrow().members.get(key) {
                    return Ok(Some(v.clone()));
                }
                let mm = class.borrow().members.get(&Value::str("_get")).cloned();
                if let Some(mm) = mm {
                    return self
                        .call(&mm, obj.clone(), std::slice::from_ref(key))
                        .map(Some);
                }
                self.builtins.instance.get(key).cloned()
            }
            Value::Class(c) => {
                if let Some(v) = c.borrow().members.get(key) {
                    return Ok(Some(v.clone()));
                }
                self.builtins.class.get(key).cloned()
            }
            Value::WeakRef(_) => self.builtins.weakref.get(key).cloned(),
            Value::Null => None,
        })
    }

    /// A table's own slot; else its delegate (searched the same way, so the
    /// delegate's own delegate or methods count), then the delegate's `_get`.
    /// A table without a delegate falls back to the table methods.
    fn table_get(
        &mut self,
        t: &Ref<Table>,
        this: &Value,
        key: &Value,
    ) -> Result<Option<Value>, Value> {
        if let Some(v) = t.borrow().get(key) {
            return Ok(Some(v.clone()));
        }
        let delegate = t.borrow().delegate.clone();
        match delegate {
            Some(d) => {
                let dv = Value::Table(d.clone());
                if let Some(v) = self.table_get(&d, &dv, key)? {
                    return Ok(Some(v));
                }
                let mm = d.borrow().get(&Value::str("_get")).cloned();
                match mm {
                    Some(mm) => self
                        .call(&mm, this.clone(), std::slice::from_ref(key))
                        .map(Some),
                    None => Ok(None),
                }
            }
            None => Ok(self.builtins.table.get(key).cloned()),
        }
    }

    /// `obj[key] = v` (the slot must exist).
    pub fn set(&mut self, obj: &Value, key: &Value, v: Value) -> Result<(), Value> {
        if let Value::Array(_) = obj
            && !matches!(key, Value::Int(_) | Value::Float(_))
        {
            let msg = format!("indexing {} with {}", obj.type_name(), key.type_name());
            return Err(self.raise_str(&msg));
        }
        if self.try_set(obj, key, v)? {
            Ok(())
        } else {
            let k = self.key_text(key);
            Err(self.raise_str(&format!("the index '{k}' does not exist")))
        }
    }

    fn try_set(&mut self, obj: &Value, key: &Value, v: Value) -> Result<bool, Value> {
        Ok(match obj {
            Value::Table(t) => {
                if t.borrow_mut().set(key, v.clone()) {
                    return Ok(true);
                }
                let delegate = t.borrow().delegate.clone();
                if let Some(d) = delegate {
                    if self.try_set(&Value::Table(d.clone()), key, v.clone())? {
                        return Ok(true);
                    }
                    let mm = d.borrow().get(&Value::str("_set")).cloned();
                    if let Some(mm) = mm {
                        self.call(&mm, obj.clone(), &[key.clone(), v])?;
                        return Ok(true);
                    }
                }
                false
            }
            Value::Array(a) => {
                let i = match key {
                    Value::Int(i) => *i,
                    Value::Float(x) => *x as i32,
                    _ => return Ok(false),
                };
                let mut a = a.borrow_mut();
                match a.get_mut(i.max(0) as usize) {
                    Some(slot) if i >= 0 => {
                        *slot = v;
                        true
                    }
                    _ => false,
                }
            }
            Value::Instance(i) => {
                if i.borrow_mut().values.set(key, v.clone()) {
                    return Ok(true);
                }
                let class = i.borrow().class.clone();
                let mm = class.borrow().members.get(&Value::str("_set")).cloned();
                if let Some(mm) = mm {
                    self.call(&mm, obj.clone(), &[key.clone(), v])?;
                    return Ok(true);
                }
                false
            }
            Value::Class(c) => c.borrow_mut().members.set(key, v),
            _ => false,
        })
    }

    /// `obj[key] <- v`.
    pub fn new_slot(&mut self, obj: &Value, key: Value, v: Value) -> Result<(), Value> {
        if key.is_null() {
            return Err(self.raise_str("null cannot be used as index"));
        }
        match obj {
            Value::Table(t) => {
                // A delegate's `_newslot` handles keys the table doesn't have.
                let delegate = t.borrow().delegate.clone();
                if let Some(d) = delegate
                    && !t.borrow().contains(&key)
                {
                    let mm = d.borrow().get(&Value::str("_newslot")).cloned();
                    if let Some(mm) = mm {
                        self.call(&mm, obj.clone(), &[key, v])?;
                        return Ok(());
                    }
                }
                t.borrow_mut().insert(key, v);
                Ok(())
            }
            Value::Class(c) => {
                if c.borrow().locked {
                    return Err(self
                        .raise_str("trying to modify a class that has already been instantiated"));
                }
                c.borrow_mut().members.insert(key, v);
                Ok(())
            }
            Value::Instance(i) => {
                let class = i.borrow().class.clone();
                let mm = class.borrow().members.get(&Value::str("_newslot")).cloned();
                match mm {
                    Some(mm) => {
                        self.call(&mm, obj.clone(), &[key, v])?;
                        Ok(())
                    }
                    None => {
                        Err(self.raise_str("class instances do not support the new slot operator"))
                    }
                }
            }
            Value::Array(_) => {
                Err(self.raise_str("cannot create new slots in arrays; use = instead of <-"))
            }
            other => {
                let msg = format!("cannot create new slots in type {}", other.type_name());
                Err(self.raise_str(&msg))
            }
        }
    }

    pub fn delete(&mut self, obj: &Value, key: &Value) -> Result<Value, Value> {
        match obj {
            Value::Table(t) => {
                let removed = t.borrow_mut().remove(key);
                match removed {
                    Some(v) => Ok(v),
                    None => {
                        let k = self.key_text(key);
                        Err(self.raise_str(&format!("the index '{k}' does not exist")))
                    }
                }
            }
            other => {
                let msg = format!("attempt to delete a slot from a {}", other.type_name());
                Err(self.raise_str(&msg))
            }
        }
    }

    fn instantiate(&mut self, cls: &Ref<Class>) -> Value {
        cls.borrow_mut().locked = true;
        let mut values = Table::new();
        for (k, v) in cls.borrow().members.iter() {
            if !matches!(v, Value::Closure(_) | Value::Native(_)) {
                values.insert(k.clone(), v.clone());
            }
        }
        Value::Instance(new_ref(Instance {
            class: cls.clone(),
            values,
            user: UserData::None,
        }))
    }

    fn foreach_next(
        &mut self,
        c: &Value,
        pos: usize,
        prev_key: Value,
    ) -> Result<Option<(usize, Value, Value)>, Value> {
        // An instance with `_nexti` gives its keys one by one (Vector: x, y,
        // z); the values are read by key.
        if let Some(mm) = self.metamethod(c, "_nexti") {
            let prev = if pos == 0 { Value::Null } else { prev_key };
            let key = self.call(&mm, c.clone(), &[prev])?;
            if key.is_null() {
                return Ok(None);
            }
            let v = self.get(c, &key)?;
            return Ok(Some((pos + 1, key, v)));
        }
        Ok(match c {
            Value::Table(t) => t.borrow().next(pos).map(|(i, k, v)| (i + 1, k, v)),
            Value::Array(a) => a
                .borrow()
                .get(pos)
                .map(|v| (pos + 1, Value::Int(pos as i32), v.clone())),
            Value::Str(s) => s
                .bytes()
                .get(pos)
                .map(|&b| (pos + 1, Value::Int(pos as i32), Value::Int(b as i32))),
            Value::Class(cls) => cls
                .borrow()
                .members
                .next(pos)
                .map(|(i, k, v)| (i + 1, k, v)),
            Value::Instance(i) => i.borrow().values.next(pos).map(|(i, k, v)| (i + 1, k, v)),
            Value::Null => None,
            other => {
                let msg = format!("cannot iterate {}", other.type_name());
                return Err(self.raise_str(&msg));
            }
        })
    }

    // ---- Operators ----

    pub fn binary(&mut self, op: BinOp, a: &Value, b: &Value) -> Result<Value, Value> {
        use BinOp::*;
        match op {
            Add | Sub | Mul | Div | Mod => self.arith(op, a, b),
            Shl | Shr | UShr | BitAnd | BitOr | BitXor => match (a, b) {
                (Value::Int(x), Value::Int(y)) => Ok(Value::Int(match op {
                    Shl => x.wrapping_shl(*y as u32),
                    Shr => x.wrapping_shr(*y as u32),
                    UShr => (*x as u32).wrapping_shr(*y as u32) as i32,
                    BitAnd => x & y,
                    BitOr => x | y,
                    _ => x ^ y,
                })),
                _ => {
                    let msg = format!(
                        "bitwise op between '{}' and '{}'",
                        a.type_name(),
                        b.type_name()
                    );
                    Err(self.raise_str(&msg))
                }
            },
            Eq => Ok(Value::Bool(self.equal(a, b))),
            Ne => Ok(Value::Bool(!self.equal(a, b))),
            Lt | Le | Gt | Ge => {
                let c = self.compare(a, b)?;
                Ok(Value::Bool(match op {
                    Lt => c < 0,
                    Le => c <= 0,
                    Gt => c > 0,
                    _ => c >= 0,
                }))
            }
            In => Ok(Value::Bool(self.has_key(b, a))),
            InstanceOf => match (a, b) {
                (Value::Instance(i), Value::Class(c)) => {
                    let mut cur = Some(i.borrow().class.clone());
                    while let Some(k) = cur {
                        if Rc::ptr_eq(&k, c) {
                            return Ok(Value::Bool(true));
                        }
                        cur = k.borrow().base.clone();
                    }
                    Ok(Value::Bool(false))
                }
                _ => {
                    let msg = format!(
                        "cannot apply instanceof between a {} and a {}",
                        b.type_name(),
                        a.type_name()
                    );
                    Err(self.raise_str(&msg))
                }
            },
        }
    }

    fn arith(&mut self, op: BinOp, a: &Value, b: &Value) -> Result<Value, Value> {
        use BinOp::*;
        match (a, b) {
            (Value::Int(x), Value::Int(y)) => {
                let (x, y) = (*x, *y);
                return Ok(Value::Int(match op {
                    Add => x.wrapping_add(y),
                    Sub => x.wrapping_sub(y),
                    Mul => x.wrapping_mul(y),
                    Div => {
                        if y == 0 {
                            return Err(self.raise_str("division by zero"));
                        }
                        x.wrapping_div(y)
                    }
                    _ => {
                        if y == 0 {
                            return Err(self.raise_str("modulo by zero"));
                        }
                        x.wrapping_rem(y)
                    }
                }));
            }
            (Value::Int(_) | Value::Float(_), Value::Int(_) | Value::Float(_)) => {
                let x = num(a);
                let y = num(b);
                return Ok(Value::Float(match op {
                    Add => x + y,
                    Sub => x - y,
                    Mul => x * y,
                    Div => x / y,
                    _ => x % y,
                }));
            }
            _ => {}
        }
        // A string on either side of `+` joins the two as text.
        if op == Add && (matches!(a, Value::Str(_)) || matches!(b, Value::Str(_))) {
            let mut s = self.to_bytes(a)?;
            s.extend(self.to_bytes(b)?);
            return Ok(Value::Str(SqStr::new(&s)));
        }
        // Metamethods on tables (through the delegate) and instances.
        let name = match op {
            Add => "_add",
            Sub => "_sub",
            Mul => "_mul",
            Div => "_div",
            _ => "_modulo",
        };
        if let Some(mm) = self.metamethod(a, name) {
            return self.call(&mm, a.clone(), std::slice::from_ref(b));
        }
        let c = match op {
            Add => '+',
            Sub => '-',
            Mul => '*',
            Div => '/',
            _ => '%',
        };
        let msg = if self.last_error.is_null() {
            format!(
                "arith op {c} between '{}' and '{}'",
                a.type_name(),
                b.type_name()
            )
        } else {
            let last = self.plain_string(&self.last_error.clone());
            format!(
                "arith op {c} between '{}' and '{}' ({last})",
                a.type_name(),
                b.type_name()
            )
        };
        Err(self.raise_str(&msg))
    }

    fn negate(&mut self, a: &Value) -> Result<Value, Value> {
        match a {
            Value::Int(i) => Ok(Value::Int(i.wrapping_neg())),
            Value::Float(x) => Ok(Value::Float(-x)),
            _ => {
                if let Some(mm) = self.metamethod(a, "_unm") {
                    return self.call(&mm, a.clone(), &[]);
                }
                let msg = format!("attempt to negate a {}", a.type_name());
                Err(self.raise_str(&msg))
            }
        }
    }

    /// A table's (through its delegate) or instance's (through its class)
    /// metamethod.
    fn metamethod(&self, v: &Value, name: &str) -> Option<Value> {
        let key = Value::str(name);
        match v {
            Value::Table(t) => {
                let d = t.borrow().delegate.clone()?;
                d.borrow().get(&key).cloned()
            }
            Value::Instance(i) => i.borrow().class.borrow().members.get(&key).cloned(),
            _ => None,
        }
    }

    /// `==`: numbers by value (an integer equals the same float); everything
    /// else needs the same type, and objects must be the same object.
    pub fn equal(&self, a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Int(_) | Value::Float(_), Value::Int(_) | Value::Float(_)) => match (a, b) {
                (Value::Int(x), Value::Int(y)) => x == y,
                _ => num(a) == num(b),
            },
            _ => a.raw_eq(b),
        }
    }

    /// Ordering for `<` and friends and for `sort`: numbers by value,
    /// strings byte by byte, `_cmp` on instances and tables; otherwise
    /// objects of one type by address, and different types fail.
    pub fn compare(&mut self, a: &Value, b: &Value) -> Result<i32, Value> {
        match (a, b) {
            (Value::Int(x), Value::Int(y)) => return Ok(sign(x.cmp(y))),
            (Value::Int(_) | Value::Float(_), Value::Int(_) | Value::Float(_)) => {
                let (x, y) = (num(a), num(b));
                return Ok(if x < y {
                    -1
                } else if x > y {
                    1
                } else {
                    0
                });
            }
            (Value::Str(x), Value::Str(y)) => return Ok(sign(x.bytes().cmp(y.bytes()))),
            _ => {}
        }
        if std::mem::discriminant(a) == std::mem::discriminant(b) {
            if let Some(mm) = self.metamethod(a, "_cmp") {
                let r = self.call(&mm, a.clone(), std::slice::from_ref(b))?;
                return Ok(match r {
                    Value::Int(i) => i.signum(),
                    Value::Float(x) => (x as i32).signum(),
                    _ => 0,
                });
            }
            return Ok(match (a, b) {
                (Value::Bool(x), Value::Bool(y)) => sign(x.cmp(y)),
                (Value::Null, Value::Null) => 0,
                _ => sign(a.object_id().cmp(&b.object_id())),
            });
        }
        let msg = format!(
            "comparsion between '{}' and '{}'",
            a.type_name(),
            b.type_name()
        );
        Err(self.raise_str(&msg))
    }

    /// `key in obj`: the object's own slots only.
    fn has_key(&self, obj: &Value, key: &Value) -> bool {
        match obj {
            Value::Table(t) => t.borrow().contains(key),
            Value::Array(a) => match key {
                Value::Int(i) => *i >= 0 && (*i as usize) < a.borrow().len(),
                _ => false,
            },
            Value::Instance(i) => {
                let i = i.borrow();
                i.values.contains(key) || i.class.borrow().members.contains(key)
            }
            Value::Class(c) => c.borrow().members.contains(key),
            Value::Str(s) => match key {
                Value::Int(i) => *i >= 0 && (*i as usize) < s.len(),
                _ => false,
            },
            _ => false,
        }
    }

    fn type_of(&mut self, v: &Value) -> Result<Value, Value> {
        if let Some(mm) = self.metamethod(v, "_typeof") {
            return self.call(&mm, v.clone(), &[]);
        }
        Ok(Value::str(v.type_name()))
    }

    pub fn clone_value(&mut self, v: &Value) -> Result<Value, Value> {
        Ok(match v {
            Value::Table(t) => Value::Table(new_ref(t.borrow().shallow_clone())),
            Value::Array(a) => Value::Array(new_ref(a.borrow().clone())),
            Value::Instance(i) => {
                let copy = {
                    let i = i.borrow();
                    Instance {
                        class: i.class.clone(),
                        values: i.values.shallow_clone(),
                        user: i.user.clone(),
                    }
                };
                let new = Value::Instance(new_ref(copy));
                if let Some(mm) = self.metamethod(&new, "_cloned") {
                    self.call(&mm, new.clone(), std::slice::from_ref(v))?;
                }
                new
            }
            Value::Closure(_) | Value::Native(_) | Value::Class(_) | Value::WeakRef(_) => {
                let msg = format!("cloning a {}", v.type_name());
                return Err(self.raise_str(&msg));
            }
            other => other.clone(),
        })
    }

    pub fn to_number(&mut self, v: &Value) -> Result<f32, Value> {
        match v {
            Value::Int(i) => Ok(*i as f32),
            Value::Float(x) => Ok(*x),
            other => {
                let msg = format!(
                    "wrong argument type, expected 'float' got '{}'",
                    other.type_name()
                );
                Err(self.raise_str(&msg))
            }
        }
    }

    // ---- Text ----

    /// A value as text without metamethods (error messages, keys).
    pub fn plain_string(&self, v: &Value) -> String {
        match v {
            Value::Null => "(null : 0x0000000000000000)".into(),
            Value::Bool(b) => b.to_string(),
            Value::Int(i) => i.to_string(),
            Value::Float(x) => float_to_string(*x),
            Value::Str(s) => s.to_string_lossy(),
            other => format!(
                "({} : 0x{:016X})",
                other.type_name(),
                other.object_id().unwrap_or(0)
            ),
        }
    }

    /// A value as text the way string `+` and `print` make it, calling
    /// `_tostring` where there is one.
    pub fn to_bytes(&mut self, v: &Value) -> Result<Vec<u8>, Value> {
        if let Value::Str(s) = v {
            return Ok(s.bytes().to_vec());
        }
        if let Some(mm) = self.metamethod(v, "_tostring") {
            let r = self.call(&mm, v.clone(), &[])?;
            if let Value::Str(s) = r {
                return Ok(s.bytes().to_vec());
            }
        }
        Ok(self.plain_string(v).into_bytes())
    }

    /// Print a script error the way the game's console shows one, with
    /// where it happened (ours: the frames it passed through).
    pub fn report_error(&mut self, e: &Value) {
        let msg = self.to_display(e);
        let at = self.error_trace.join(" <- ");
        (self.out)(&format!("SCRIPT ERROR: {msg}\n    at {at}\n"));
    }

    /// For showing errors: the text, or the plain form if `_tostring` fails.
    pub fn to_display(&mut self, v: &Value) -> String {
        match self.to_bytes(v) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(_) => self.plain_string(v),
        }
    }
}

fn num(v: &Value) -> f32 {
    match v {
        Value::Int(i) => *i as f32,
        Value::Float(x) => *x,
        _ => 0.0,
    }
}

fn sign(o: std::cmp::Ordering) -> i32 {
    o as i32
}

/// A native function value.
pub fn native(
    name: &str,
    nparams: i32,
    f: impl Fn(&mut Vm, &[Value]) -> NativeResult + 'static,
) -> Value {
    Value::Native(Rc::new(NativeFn {
        name: name.into(),
        nparams,
        f: Rc::new(f),
        env: None,
    }))
}
