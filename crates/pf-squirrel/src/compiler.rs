//! The syntax tree to our own bytecode, run by `vm.rs`.
//!
//! The bytecode is ours (a simple stack machine), not the game's. What has to
//! match the game is how names resolve, which follows Squirrel 2's rules:
//! - a name is a local of this function, else one of its free variables
//!   (`function() : (a)`), else a constant (`const`/`enum`, fixed when the
//!   code is compiled), else a field of `this`, falling back to the root
//!   table;
//! - functions can't see the locals of the function around them (Squirrel 2
//!   has no closures over locals; scripts pass values in as free variables);
//! - a call on a plain name passes the caller's `this` along; `a.f()` passes
//!   `a`.

use std::rc::Rc;

use crate::ast::*;
use crate::table::Table;
use crate::value::Value;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Null,
    True,
    False,
    Int(i32),
    Float(f32),
    /// A value from the function's constant list.
    Const(u32),
    Local(u16),
    /// Store into a local; the value stays on the stack.
    SetLocal(u16),
    Free(u16),
    SetFree(u16),
    Root,
    Vargc,
    /// `vargv[i]`: pops `i`.
    Vargv,
    /// A name through `this`, then the root table.
    GetEnv(u32),
    /// Assign a name through `this`, then the root table; the value stays.
    SetEnv(u32),
    /// `[obj key] -> [value]`.
    Get,
    /// `[obj] -> [obj.k]`.
    GetK(u32),
    /// `[obj key value] -> [value]`.
    Set,
    /// `[obj key value] -> [value]` (`<-`).
    NewSlot,
    /// `[obj key] -> [old value]`.
    Delete,
    Dup,
    /// Duplicate the top two values.
    Dup2,
    Pop,
    Swap,
    Bin(BinOp),
    Neg,
    Not,
    BitNot,
    Typeof,
    Clone,
    Resume,
    Jump(u32),
    /// Pop; jump if false.
    JumpFalse(u32),
    /// Pop; jump if true.
    JumpTrue(u32),
    /// `&&`: if the top is false jump (keeping it), else pop it.
    AndJump(u32),
    /// `||`: if the top is true jump (keeping it), else pop it.
    OrJump(u32),
    /// `[callee this args..] -> [result]`; the count excludes `this`.
    Call(u16),
    /// A table sized for this many entries.
    NewTable(u32),
    NewArray(u32),
    /// `[array value] -> [array]`.
    ArrayPush,
    /// `[table key value] -> [table]` (a table literal's entry).
    TableSlot,
    /// Make a function from `protos[i]`: pops its free variables, and
    /// before them its default parameter values.
    Closure(u32),
    /// A class, with a base class popped first if true.
    NewClass(bool),
    /// `[class key value] -> [class]`.
    ClassMember,
    /// `[parent table] -> [table]`.
    Delegate,
    /// `++`/`--` on a local: (slot, delta, postfix).
    IncLocal(u16, i8, bool),
    IncFree(u16, i8, bool),
    IncEnv(u32, i8, bool),
    /// `++`/`--` on `[obj key]`.
    IncGet(i8, bool),
    /// `foreach` step. The slot holds the container; the next three hold
    /// the position, the key and the value. Jumps when done.
    ForeachNext(u16, u32),
    /// Start a `try`: errors jump to the target.
    PushTrap(u32),
    PopTrap,
    Throw,
    Return,
    ReturnNull,
    /// Drop locals down to this many.
    SetTop(u16),
}

/// A compiled function.
#[derive(Debug, Default)]
pub struct Proto {
    /// None for an unnamed function ("(unnamed_function)" in errors).
    pub name: Option<Rc<str>>,
    pub source: Rc<str>,
    /// Parameters including `this`.
    pub nparams: usize,
    /// The parameter names (without `this`).
    pub param_names: Vec<Rc<str>>,
    pub ndefaults: usize,
    pub varargs: bool,
    pub nfree: usize,
    pub code: Vec<Op>,
    /// The script line of each op.
    pub lines: Vec<u32>,
    pub consts: Vec<Value>,
    pub protos: Vec<Rc<Proto>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompileError {
    pub line: u32,
    pub message: String,
}

type Result<T> = std::result::Result<T, CompileError>;

/// Compile a script's statements into a function taking only `this`.
/// `consts` is the VM's constant table: `const` and `enum` add to it, and
/// names found in it are fixed at compile time (as in the game).
pub fn compile(stmts: &[Stmt], source: &str, consts: &mut Table) -> Result<Rc<Proto>> {
    let mut c = Compiler {
        source: source.into(),
        consts,
    };
    let def = FuncDef {
        name: "main".into(),
        params: Vec::new(),
        varargs: true,
        free_vars: Vec::new(),
        body: stmts.to_vec(),
        line: 1,
    };
    let mut proto = c.function(&def)?;
    // The top level of a script is "main" with variable arguments.
    proto.name = Some("main".into());
    Ok(Rc::new(proto))
}

struct Compiler<'a> {
    source: Rc<str>,
    consts: &'a mut Table,
}

struct Loop {
    breaks: Vec<usize>,
    continues: Vec<usize>,
    /// Locals alive at the loop's start (`break`/`continue` drop the rest).
    depth: usize,
    traps: usize,
    /// A `switch`: a `break` target but not a `continue` one.
    is_switch: bool,
}

struct Func {
    proto: Proto,
    locals: Vec<Rc<str>>,
    free: Vec<Rc<str>>,
    loops: Vec<Loop>,
    traps: usize,
    line: u32,
}

impl Func {
    fn emit(&mut self, op: Op) -> usize {
        self.proto.code.push(op);
        self.proto.lines.push(self.line);
        self.proto.code.len() - 1
    }

    fn here(&self) -> u32 {
        self.proto.code.len() as u32
    }

    /// Point the jump at `at` to the current position.
    fn patch(&mut self, at: usize) {
        let to = self.here();
        self.patch_to(at, to);
    }

    fn patch_to(&mut self, at: usize, to: u32) {
        self.proto.code[at] = match self.proto.code[at] {
            Op::Jump(_) => Op::Jump(to),
            Op::JumpFalse(_) => Op::JumpFalse(to),
            Op::JumpTrue(_) => Op::JumpTrue(to),
            Op::AndJump(_) => Op::AndJump(to),
            Op::OrJump(_) => Op::OrJump(to),
            Op::PushTrap(_) => Op::PushTrap(to),
            Op::ForeachNext(s, _) => Op::ForeachNext(s, to),
            other => other,
        };
    }

    fn constant(&mut self, v: Value) -> u32 {
        if let Some(i) = self.proto.consts.iter().position(|c| c.raw_eq(&v)) {
            return i as u32;
        }
        self.proto.consts.push(v);
        (self.proto.consts.len() - 1) as u32
    }

    fn name(&mut self, s: &str) -> u32 {
        self.constant(Value::str(s))
    }

    fn local(&self, name: &str) -> Option<u16> {
        self.locals
            .iter()
            .rposition(|l| &**l == name)
            .map(|i| i as u16)
    }

    fn free_var(&self, name: &str) -> Option<u16> {
        self.free
            .iter()
            .position(|l| &**l == name)
            .map(|i| i as u16)
    }

    fn add_local(&mut self, name: Rc<str>) -> u16 {
        self.locals.push(name);
        (self.locals.len() - 1) as u16
    }

    /// Drop locals back to `depth` (emitting the stack drop).
    fn end_scope(&mut self, depth: usize) {
        if self.locals.len() > depth {
            self.emit(Op::SetTop(depth as u16));
            self.locals.truncate(depth);
        }
    }
}

/// Where a name resolves.
enum Name {
    Local(u16),
    Free(u16),
    Const(Value),
    Env,
}

impl Compiler<'_> {
    fn err<T>(&self, f: &Func, message: impl Into<String>) -> Result<T> {
        Err(CompileError {
            line: f.line,
            message: message.into(),
        })
    }

    fn function(&mut self, def: &FuncDef) -> Result<Proto> {
        let mut f = Func {
            proto: Proto {
                name: (!def.name.is_empty()).then(|| def.name.clone()),
                source: self.source.clone(),
                nparams: def.params.len() + 1,
                param_names: def.params.iter().map(|p| p.name.clone()).collect(),
                ndefaults: def.params.iter().filter(|p| p.default.is_some()).count(),
                varargs: def.varargs,
                nfree: def.free_vars.len(),
                ..Proto::default()
            },
            locals: vec!["this".into()],
            free: def.free_vars.clone(),
            loops: Vec::new(),
            traps: 0,
            line: def.line,
        };
        for p in &def.params {
            f.add_local(p.name.clone());
        }
        for s in &def.body {
            self.stmt(&mut f, s)?;
        }
        f.emit(Op::ReturnNull);
        Ok(f.proto)
    }

    fn resolve(&self, f: &Func, name: &str) -> Name {
        if let Some(i) = f.local(name) {
            Name::Local(i)
        } else if let Some(i) = f.free_var(name) {
            Name::Free(i)
        } else if let Some(v) = self.consts.get(&Value::str(name)) {
            Name::Const(v.clone())
        } else {
            Name::Env
        }
    }

    // ---- Statements ----

    fn stmt(&mut self, f: &mut Func, s: &Stmt) -> Result<()> {
        f.line = s.line;
        match &s.kind {
            StmtKind::Expr(e) => {
                self.expr(f, e)?;
                f.emit(Op::Pop);
            }
            StmtKind::Local(vars) => {
                for (name, value) in vars {
                    match value {
                        Some(v) => self.expr(f, v)?,
                        None => {
                            f.emit(Op::Null);
                        }
                    }
                    f.add_local(name.clone());
                }
            }
            StmtKind::If(cond, then, other) => {
                self.expr(f, cond)?;
                let to_else = f.emit(Op::JumpFalse(0));
                self.scoped(f, then)?;
                match other {
                    Some(other) => {
                        let to_end = f.emit(Op::Jump(0));
                        f.patch(to_else);
                        self.scoped(f, other)?;
                        f.patch(to_end);
                    }
                    None => f.patch(to_else),
                }
            }
            StmtKind::While(cond, body) => {
                let start = f.here();
                self.expr(f, cond)?;
                let to_end = f.emit(Op::JumpFalse(0));
                self.loop_body(f, body, false)?;
                let lp = f.loops.pop().expect("loop");
                f.emit(Op::Jump(start));
                f.patch(to_end);
                self.close_loop(f, lp, start);
            }
            StmtKind::DoWhile(body, cond) => {
                let start = f.here();
                self.loop_body(f, body, false)?;
                let lp = f.loops.pop().expect("loop");
                let cont = f.here();
                f.line = s.line;
                self.expr(f, cond)?;
                f.emit(Op::JumpTrue(start));
                self.close_loop(f, lp, cont);
            }
            StmtKind::For {
                init,
                cond,
                step,
                body,
            } => {
                let depth = f.locals.len();
                if let Some(init) = init {
                    self.stmt(f, init)?;
                }
                let start = f.here();
                let to_end = match cond {
                    Some(c) => {
                        self.expr(f, c)?;
                        Some(f.emit(Op::JumpFalse(0)))
                    }
                    None => None,
                };
                self.loop_body(f, body, false)?;
                let lp = f.loops.pop().expect("loop");
                let cont = f.here();
                if let Some(step) = step {
                    f.line = s.line;
                    self.expr(f, step)?;
                    f.emit(Op::Pop);
                }
                f.emit(Op::Jump(start));
                if let Some(at) = to_end {
                    f.patch(at);
                }
                self.close_loop(f, lp, cont);
                f.end_scope(depth);
            }
            StmtKind::Foreach {
                key,
                value,
                container,
                body,
            } => {
                let depth = f.locals.len();
                self.expr(f, container)?;
                let slot = f.add_local("@container".into());
                f.emit(Op::Null);
                f.add_local("@position".into());
                f.emit(Op::Null);
                f.add_local(key.clone().unwrap_or_else(|| "@key".into()));
                f.emit(Op::Null);
                f.add_local(value.clone());
                let start = f.here();
                let next = f.emit(Op::ForeachNext(slot, 0));
                self.loop_body(f, body, false)?;
                let lp = f.loops.pop().expect("loop");
                f.emit(Op::Jump(start));
                f.patch(next);
                self.close_loop(f, lp, start);
                f.end_scope(depth);
            }
            StmtKind::Switch(subject, cases) => {
                let depth = f.locals.len();
                self.expr(f, subject)?;
                let slot = f.add_local("@switch".into());
                // The tests first, then the bodies in order (falling through).
                let mut to_body = Vec::new();
                for (label, _) in cases {
                    if let Some(label) = label {
                        f.emit(Op::Local(slot));
                        self.expr(f, label)?;
                        f.emit(Op::Bin(BinOp::Eq));
                        to_body.push(Some(f.emit(Op::JumpTrue(0))));
                    } else {
                        to_body.push(None);
                    }
                }
                let to_default = f.emit(Op::Jump(0));
                f.loops.push(Loop {
                    breaks: Vec::new(),
                    continues: Vec::new(),
                    depth: f.locals.len(),
                    traps: f.traps,
                    is_switch: true,
                });
                let mut default_at = None;
                for ((label, body), jump) in cases.iter().zip(&to_body) {
                    match jump {
                        Some(at) => f.patch(*at),
                        None => default_at = Some(f.here()),
                    }
                    let _ = label;
                    let inner = f.locals.len();
                    for st in body {
                        self.stmt(f, st)?;
                    }
                    f.end_scope(inner);
                }
                let end = f.here();
                f.patch_to(to_default, default_at.unwrap_or(end));
                let lp = f.loops.pop().expect("switch");
                for at in lp.breaks {
                    f.patch(at);
                }
                f.end_scope(depth);
            }
            StmtKind::Break => {
                let Some(lp) = f.loops.last() else {
                    return self.err(f, "'break' has to be in a loop block");
                };
                let (depth, traps) = (lp.depth, lp.traps);
                for _ in traps..f.traps {
                    f.emit(Op::PopTrap);
                }
                if f.locals.len() > depth {
                    f.emit(Op::SetTop(depth as u16));
                }
                let at = f.emit(Op::Jump(0));
                f.loops.last_mut().expect("loop").breaks.push(at);
            }
            StmtKind::Continue => {
                let Some(i) = f.loops.iter().rposition(|l| !l.is_switch) else {
                    return self.err(f, "'continue' has to be in a loop block");
                };
                let (depth, traps) = (f.loops[i].depth, f.loops[i].traps);
                for _ in traps..f.traps {
                    f.emit(Op::PopTrap);
                }
                if f.locals.len() > depth {
                    f.emit(Op::SetTop(depth as u16));
                }
                let at = f.emit(Op::Jump(0));
                f.loops[i].continues.push(at);
            }
            StmtKind::Return(value) => match value {
                Some(v) => {
                    self.expr(f, v)?;
                    f.emit(Op::Return);
                }
                None => {
                    f.emit(Op::ReturnNull);
                }
            },
            StmtKind::Block(stmts) => {
                let depth = f.locals.len();
                for st in stmts {
                    self.stmt(f, st)?;
                }
                f.end_scope(depth);
            }
            StmtKind::Function(path, def) => {
                // `function a::b::c()` is `a.b.c <- function`; a plain name
                // is a new slot in `this`.
                let (last, init) = path.split_last().expect("function name");
                if init.is_empty() {
                    f.emit(Op::Local(0));
                } else {
                    self.name_value(f, &init[0])?;
                    for p in &init[1..] {
                        let k = f.name(p);
                        f.emit(Op::GetK(k));
                    }
                }
                let k = f.name(last);
                f.emit(Op::Const(k));
                self.closure(f, def)?;
                f.emit(Op::NewSlot);
                f.emit(Op::Pop);
            }
            StmtKind::Class(target, def) => {
                self.slot_target(f, target)?;
                self.class(f, def)?;
                f.emit(Op::NewSlot);
                f.emit(Op::Pop);
            }
            StmtKind::Const(name, value) => {
                let v = self.const_value(f, value)?;
                self.consts.insert(Value::str(name), v);
            }
            StmtKind::Enum(name, items) => {
                let mut t = Table::new();
                for (k, v) in items {
                    let v = self.const_value(f, v)?;
                    t.insert(Value::str(k), v);
                }
                self.consts
                    .insert(Value::str(name), Value::Table(crate::value::new_ref(t)));
            }
            StmtKind::Try(body, name, catch) => {
                let trap = f.emit(Op::PushTrap(0));
                f.traps += 1;
                self.scoped(f, body)?;
                f.traps -= 1;
                f.emit(Op::PopTrap);
                let to_end = f.emit(Op::Jump(0));
                f.patch(trap);
                // The VM pushes the error here; it becomes the catch's local.
                let depth = f.locals.len();
                f.add_local(name.clone());
                self.scoped(f, catch)?;
                f.end_scope(depth);
                f.patch(to_end);
            }
            StmtKind::Throw(e) => {
                self.expr(f, e)?;
                f.emit(Op::Throw);
            }
            // `wait t` calls the root table's `Wait( t )`, as the game's
            // compiler does.
            StmtKind::Wait(e) => {
                f.emit(Op::Root);
                let k = f.name("Wait");
                f.emit(Op::GetK(k));
                f.emit(Op::Local(0));
                self.expr(f, e)?;
                f.emit(Op::Call(1));
                f.emit(Op::Pop);
            }
            StmtKind::Empty => {}
        }
        Ok(())
    }

    /// A statement in its own scope (an `if` branch, a loop body).
    fn scoped(&mut self, f: &mut Func, s: &Stmt) -> Result<()> {
        let depth = f.locals.len();
        self.stmt(f, s)?;
        f.end_scope(depth);
        Ok(())
    }

    /// A loop body with a new break/continue context (left on `f.loops` for
    /// the caller to close).
    fn loop_body(&mut self, f: &mut Func, body: &Stmt, is_switch: bool) -> Result<()> {
        f.loops.push(Loop {
            breaks: Vec::new(),
            continues: Vec::new(),
            depth: f.locals.len(),
            traps: f.traps,
            is_switch,
        });
        self.scoped(f, body)
    }

    fn close_loop(&mut self, f: &mut Func, lp: Loop, cont: u32) {
        for at in lp.breaks {
            f.patch(at);
        }
        for at in lp.continues {
            f.patch_to(at, cont);
        }
    }

    /// A `const`/`enum` value: a literal.
    fn const_value(&self, f: &Func, e: &Expr) -> Result<Value> {
        Ok(match e {
            Expr::Null => Value::Null,
            Expr::Bool(b) => Value::Bool(*b),
            Expr::Int(i) => Value::Int(*i as i32),
            Expr::Float(x) => Value::Float(*x as f32),
            Expr::Str(s) => Value::str(s),
            _ => return self.err(f, "scalar expected : integer,float or string"),
        })
    }

    // ---- Expressions (each leaves one value) ----

    fn expr(&mut self, f: &mut Func, e: &Expr) -> Result<()> {
        match e {
            Expr::Null => {
                f.emit(Op::Null);
            }
            Expr::Bool(true) => {
                f.emit(Op::True);
            }
            Expr::Bool(false) => {
                f.emit(Op::False);
            }
            // Integers are 32-bit: a bigger literal wraps (oracle I017).
            Expr::Int(i) => {
                f.emit(Op::Int(*i as i32));
            }
            Expr::Float(x) => {
                f.emit(Op::Float(*x as f32));
            }
            Expr::Str(s) => {
                let k = f.name(s);
                f.emit(Op::Const(k));
            }
            Expr::Ident(name) => self.name_value(f, name)?,
            Expr::Root(name) => {
                f.emit(Op::Root);
                let k = f.name(name);
                f.emit(Op::GetK(k));
            }
            Expr::This => {
                f.emit(Op::Local(0));
            }
            Expr::Vargc => {
                f.emit(Op::Vargc);
            }
            Expr::Vargv(i) => {
                self.expr(f, i)?;
                f.emit(Op::Vargv);
            }
            Expr::Field(obj, name) => {
                self.expr(f, obj)?;
                let k = f.name(name);
                f.emit(Op::GetK(k));
            }
            Expr::Index(obj, key) => {
                self.expr(f, obj)?;
                self.expr(f, key)?;
                f.emit(Op::Get);
            }
            Expr::Call(callee, args) => {
                self.call_setup(f, callee, args)?;
                f.emit(Op::Call(args.len() as u16));
            }
            Expr::Unary(op, e) => {
                self.expr(f, e)?;
                f.emit(match op {
                    UnOp::Neg => Op::Neg,
                    UnOp::Not => Op::Not,
                    UnOp::BitNot => Op::BitNot,
                });
            }
            Expr::IncDec {
                target,
                delta,
                post,
            } => {
                let d = *delta as i8;
                match &**target {
                    Expr::Ident(name) => match self.resolve(f, name) {
                        Name::Local(i) => {
                            f.emit(Op::IncLocal(i, d, *post));
                        }
                        Name::Free(i) => {
                            f.emit(Op::IncFree(i, d, *post));
                        }
                        Name::Const(_) => return self.err(f, "can't assign expression"),
                        Name::Env => {
                            let k = f.name(name);
                            f.emit(Op::IncEnv(k, d, *post));
                        }
                    },
                    other => {
                        self.slot_target(f, other)?;
                        f.emit(Op::IncGet(d, *post));
                    }
                }
            }
            Expr::Binary(op, a, b) => {
                self.expr(f, a)?;
                self.expr(f, b)?;
                f.emit(Op::Bin(*op));
            }
            Expr::And(a, b) => {
                self.expr(f, a)?;
                let at = f.emit(Op::AndJump(0));
                self.expr(f, b)?;
                f.patch(at);
            }
            Expr::Or(a, b) => {
                self.expr(f, a)?;
                let at = f.emit(Op::OrJump(0));
                self.expr(f, b)?;
                f.patch(at);
            }
            Expr::Ternary(c, a, b) => {
                self.expr(f, c)?;
                let to_b = f.emit(Op::JumpFalse(0));
                self.expr(f, a)?;
                let to_end = f.emit(Op::Jump(0));
                f.patch(to_b);
                self.expr(f, b)?;
                f.patch(to_end);
            }
            Expr::Assign { target, op, value } => self.assign(f, target, *op, value)?,
            Expr::NewSlot(target, value) => {
                if let Expr::Ident(name) = &**target
                    && f.local(name).is_some()
                {
                    return self.err(f, "can't 'create' a local slot");
                }
                self.slot_target(f, target)?;
                self.expr(f, value)?;
                f.emit(Op::NewSlot);
            }
            Expr::Table(entries) => {
                f.emit(Op::NewTable(entries.len() as u32));
                for (k, v) in entries {
                    self.expr(f, k)?;
                    self.expr(f, v)?;
                    f.emit(Op::TableSlot);
                }
            }
            Expr::Array(items) => {
                f.emit(Op::NewArray(items.len() as u32));
                for v in items {
                    self.expr(f, v)?;
                    f.emit(Op::ArrayPush);
                }
            }
            Expr::Function(def) => self.closure(f, def)?,
            Expr::Class(def) => self.class(f, def)?,
            Expr::Delete(target) => {
                self.slot_target(f, target)?;
                f.emit(Op::Delete);
            }
            Expr::Clone(e) => {
                self.expr(f, e)?;
                f.emit(Op::Clone);
            }
            Expr::Typeof(e) => {
                self.expr(f, e)?;
                f.emit(Op::Typeof);
            }
            Expr::Resume(e) => {
                self.expr(f, e)?;
                f.emit(Op::Resume);
            }
            Expr::Delegate(parent, table) => {
                self.expr(f, parent)?;
                self.expr(f, table)?;
                f.emit(Op::Delegate);
            }
            Expr::Thread(call) => self.thread_call(f, "SpinOff", None, call)?,
            Expr::WaitThread(call) => self.thread_call(f, "SpinOffAndWaitFor", None, call)?,
            Expr::WaitThreadSolo(call) => {
                self.thread_call(f, "SpinOffAndWaitForSolo", None, call)?
            }
            Expr::DelayThread(delay, call) => {
                self.thread_call(f, "SpinOffDelayed", Some(delay), call)?
            }
            Expr::Comma(items) => {
                for (i, e) in items.iter().enumerate() {
                    self.expr(f, e)?;
                    if i + 1 < items.len() {
                        f.emit(Op::Pop);
                    }
                }
            }
        }
        Ok(())
    }

    fn name_value(&mut self, f: &mut Func, name: &str) -> Result<()> {
        match self.resolve(f, name) {
            Name::Local(i) => {
                f.emit(Op::Local(i));
            }
            Name::Free(i) => {
                f.emit(Op::Free(i));
            }
            Name::Const(v) => {
                let k = f.constant(v);
                f.emit(Op::Const(k));
            }
            Name::Env => {
                let k = f.name(name);
                f.emit(Op::GetEnv(k));
            }
        }
        Ok(())
    }

    /// Push the object and key a slot expression refers to (for `<-`,
    /// `delete`, `++` and compound assignment). A plain name is a field of
    /// `this`.
    fn slot_target(&mut self, f: &mut Func, target: &Expr) -> Result<()> {
        match target {
            Expr::Ident(name) => {
                f.emit(Op::Local(0));
                let k = f.name(name);
                f.emit(Op::Const(k));
            }
            Expr::Root(name) => {
                f.emit(Op::Root);
                let k = f.name(name);
                f.emit(Op::Const(k));
            }
            Expr::Field(obj, name) => {
                self.expr(f, obj)?;
                let k = f.name(name);
                f.emit(Op::Const(k));
            }
            Expr::Index(obj, key) => {
                self.expr(f, obj)?;
                self.expr(f, key)?;
            }
            _ => return self.err(f, "can't assign expression"),
        }
        Ok(())
    }

    fn assign(
        &mut self,
        f: &mut Func,
        target: &Expr,
        op: Option<BinOp>,
        value: &Expr,
    ) -> Result<()> {
        if let Expr::Ident(name) = target {
            match self.resolve(f, name) {
                Name::Local(i) => {
                    if let Some(op) = op {
                        f.emit(Op::Local(i));
                        self.expr(f, value)?;
                        f.emit(Op::Bin(op));
                    } else {
                        self.expr(f, value)?;
                    }
                    f.emit(Op::SetLocal(i));
                    return Ok(());
                }
                Name::Free(i) => {
                    if let Some(op) = op {
                        f.emit(Op::Free(i));
                        self.expr(f, value)?;
                        f.emit(Op::Bin(op));
                    } else {
                        self.expr(f, value)?;
                    }
                    f.emit(Op::SetFree(i));
                    return Ok(());
                }
                Name::Const(_) => return self.err(f, "can't assign expression"),
                Name::Env => {
                    let k = f.name(name);
                    if let Some(op) = op {
                        f.emit(Op::GetEnv(k));
                        self.expr(f, value)?;
                        f.emit(Op::Bin(op));
                    } else {
                        self.expr(f, value)?;
                    }
                    f.emit(Op::SetEnv(k));
                    return Ok(());
                }
            }
        }
        if matches!(target, Expr::Vargv(_)) {
            return self.err(f, "can't assign expression");
        }
        // Squirrel 2's compiler only checks that the last operand of the
        // left side is assignable, so `0 + x -= w` compiles (one game script
        // has it). The assignment then lands in the sum's temporary: the
        // result is `(0 + x) - w` and `x` is unchanged. From the compiler's
        // design; to be confirmed by oracle cases Q001/Q002.
        if let Expr::Binary(_, _, last) = target
            && matches!(&**last, Expr::Ident(n) if f.local(n).is_some())
        {
            self.expr(f, target)?;
            match op {
                Some(op) => {
                    self.expr(f, value)?;
                    f.emit(Op::Bin(op));
                }
                None => {
                    f.emit(Op::Pop);
                    self.expr(f, value)?;
                }
            }
            return Ok(());
        }
        self.slot_target(f, target)?;
        if let Some(op) = op {
            f.emit(Op::Dup2);
            f.emit(Op::Get);
            self.expr(f, value)?;
            f.emit(Op::Bin(op));
        } else {
            self.expr(f, value)?;
        }
        f.emit(Op::Set);
        Ok(())
    }

    /// Push `callee, this, args...` for a call.
    fn call_setup(&mut self, f: &mut Func, callee: &Expr, args: &[Expr]) -> Result<()> {
        match callee {
            Expr::Field(obj, name) => {
                self.expr(f, obj)?;
                f.emit(Op::Dup);
                let k = f.name(name);
                f.emit(Op::GetK(k));
                f.emit(Op::Swap);
            }
            Expr::Index(obj, key) => {
                self.expr(f, obj)?;
                f.emit(Op::Dup);
                self.expr(f, key)?;
                f.emit(Op::Get);
                f.emit(Op::Swap);
            }
            Expr::Root(name) => {
                f.emit(Op::Root);
                f.emit(Op::Dup);
                let k = f.name(name);
                f.emit(Op::GetK(k));
                f.emit(Op::Swap);
            }
            other => {
                self.expr(f, other)?;
                f.emit(Op::Local(0));
            }
        }
        for a in args {
            self.expr(f, a)?;
        }
        Ok(())
    }

    /// `thread f(a)` and friends: as in the game's compiler, a call of the
    /// root table's `SpinOff` (`SpinOffAndWaitFor`, `SpinOffAndWaitForSolo`,
    /// `SpinOffDelayed`) with the function and its arguments; `a.f()` passes
    /// `a` as `this` (the new thread's `this`). `delaythread(d)` passes the
    /// delay first.
    fn thread_call(
        &mut self,
        f: &mut Func,
        native: &str,
        delay: Option<&Expr>,
        call: &Expr,
    ) -> Result<()> {
        let Expr::Call(callee, args) = call else {
            return self.err(f, "expected a function call after thread");
        };
        f.emit(Op::Root);
        let k = f.name(native);
        f.emit(Op::GetK(k));
        match &**callee {
            Expr::Field(obj, name) => {
                self.expr(f, obj)?;
                f.emit(Op::Dup);
                let k = f.name(name);
                f.emit(Op::GetK(k));
            }
            Expr::Index(obj, key) => {
                self.expr(f, obj)?;
                f.emit(Op::Dup);
                self.expr(f, key)?;
                f.emit(Op::Get);
            }
            Expr::Root(name) => {
                f.emit(Op::Root);
                f.emit(Op::Dup);
                let k = f.name(name);
                f.emit(Op::GetK(k));
            }
            other => {
                f.emit(Op::Local(0));
                self.expr(f, other)?;
            }
        }
        if let Some(d) = delay {
            self.expr(f, d)?;
            f.emit(Op::Swap);
        }
        for a in args {
            self.expr(f, a)?;
        }
        let n = args.len() + 1 + delay.is_some() as usize;
        f.emit(Op::Call(n as u16));
        Ok(())
    }

    fn closure(&mut self, f: &mut Func, def: &FuncDef) -> Result<()> {
        // Default values and free variables are worked out now, in the
        // enclosing function.
        for p in &def.params {
            if let Some(d) = &p.default {
                self.expr(f, d)?;
            }
        }
        for name in &def.free_vars {
            self.name_value(f, name)?;
        }
        let proto = self.function(def)?;
        f.proto.protos.push(Rc::new(proto));
        f.emit(Op::Closure((f.proto.protos.len() - 1) as u32));
        Ok(())
    }

    fn class(&mut self, f: &mut Func, def: &ClassDef) -> Result<()> {
        if let Some(base) = &def.base {
            self.expr(f, base)?;
        }
        f.emit(Op::NewClass(def.base.is_some()));
        for (key, value, _is_static) in &def.members {
            self.expr(f, key)?;
            self.expr(f, value)?;
            f.emit(Op::ClassMember);
        }
        Ok(())
    }
}
