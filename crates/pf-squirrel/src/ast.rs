//! The parsed form of a script.

use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<str>),
    /// A plain name: a local, an outer variable, or a field of `this`/the root.
    Ident(Rc<str>),
    /// `::name`: a field of the root table.
    Root(Rc<str>),
    This,
    Vargc,
    /// `vargv[i]`.
    Vargv(Box<Expr>),
    Field(Box<Expr>, Rc<str>),
    Index(Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    Unary(UnOp, Box<Expr>),
    /// `++x` / `--x` (`post` false) or `x++` / `x--` (`post` true).
    IncDec {
        target: Box<Expr>,
        delta: i64,
        post: bool,
    },
    Binary(BinOp, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `target op= value` (`op` None for plain `=`).
    Assign {
        target: Box<Expr>,
        op: Option<BinOp>,
        value: Box<Expr>,
    },
    /// `target <- value`.
    NewSlot(Box<Expr>, Box<Expr>),
    Table(Vec<(Expr, Expr)>),
    Array(Vec<Expr>),
    Function(Rc<FuncDef>),
    Class(Rc<ClassDef>),
    Delete(Box<Expr>),
    Clone(Box<Expr>),
    Typeof(Box<Expr>),
    Resume(Box<Expr>),
    /// `delegate parent : table`: `table` with `parent` as its delegate.
    Delegate(Box<Expr>, Box<Expr>),
    /// `thread f(...)`: run the call as a new script thread.
    Thread(Box<Expr>),
    /// `waitthread f(...)`: run it as a thread and wait for it to finish.
    WaitThread(Box<Expr>),
    /// `waitthreadsolo f(...)`: like `waitthread` (Respawn; its exact
    /// difference is still to be read from launcher.dll).
    WaitThreadSolo(Box<Expr>),
    /// `delaythread(delay) f(...)`.
    DelayThread(Box<Expr>, Box<Expr>),
    /// The comma operator (only in `for` headers).
    Comma(Vec<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
    BitNot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Shl,
    Shr,
    UShr,
    BitAnd,
    BitOr,
    BitXor,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    In,
    InstanceOf,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub line: u32,
    pub kind: StmtKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    Expr(Expr),
    /// `local a = 1, b` (no value means null).
    Local(Vec<(Rc<str>, Option<Expr>)>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    DoWhile(Box<Stmt>, Expr),
    For {
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        step: Option<Expr>,
        body: Box<Stmt>,
    },
    Foreach {
        key: Option<Rc<str>>,
        value: Rc<str>,
        container: Expr,
        body: Box<Stmt>,
    },
    Switch(Expr, Vec<(Option<Expr>, Vec<Stmt>)>),
    Break,
    Continue,
    Return(Option<Expr>),
    Block(Vec<Stmt>),
    /// `function a::b::c(...)`: a new slot `c` in `a.b` (or `c` in the
    /// current table for a plain name).
    Function(Vec<Rc<str>>, Rc<FuncDef>),
    /// `class a.b extends c { ... }`.
    Class(Expr, Rc<ClassDef>),
    Const(Rc<str>, Expr),
    Enum(Rc<str>, Vec<(Rc<str>, Expr)>),
    Try(Box<Stmt>, Rc<str>, Box<Stmt>),
    Throw(Expr),
    /// `wait expr`: suspend this script thread for that many seconds.
    Wait(Expr),
    Empty,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: Rc<str>,
    pub default: Option<Expr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FuncDef {
    pub name: Rc<str>,
    pub params: Vec<Param>,
    pub varargs: bool,
    /// Squirrel 2 free variables, `function(...) : (a, b)`: outer values
    /// copied in when the function is created.
    pub free_vars: Vec<Rc<str>>,
    pub body: Vec<Stmt>,
    pub line: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClassDef {
    pub base: Option<Expr>,
    /// Members in order: name, value, static.
    pub members: Vec<(Expr, Expr, bool)>,
}

/// Call `f` on every expression in the statements, in source order (an
/// expression before the ones inside it). For tools that read scripts.
pub fn visit_exprs(stmts: &[Stmt], f: &mut impl FnMut(&Expr)) {
    for s in stmts {
        visit_stmt(s, f);
    }
}

fn visit_stmt(s: &Stmt, f: &mut impl FnMut(&Expr)) {
    match &s.kind {
        StmtKind::Expr(e) | StmtKind::Throw(e) | StmtKind::Wait(e) | StmtKind::Const(_, e) => {
            visit_expr(e, f)
        }
        StmtKind::Return(e) => e.iter().for_each(|e| visit_expr(e, f)),
        StmtKind::Local(vars) => vars
            .iter()
            .filter_map(|(_, e)| e.as_ref())
            .for_each(|e| visit_expr(e, f)),
        StmtKind::If(c, a, b) => {
            visit_expr(c, f);
            visit_stmt(a, f);
            b.iter().for_each(|b| visit_stmt(b, f));
        }
        StmtKind::While(c, body) => {
            visit_expr(c, f);
            visit_stmt(body, f);
        }
        StmtKind::DoWhile(body, c) => {
            visit_stmt(body, f);
            visit_expr(c, f);
        }
        StmtKind::For {
            init,
            cond,
            step,
            body,
        } => {
            init.iter().for_each(|s| visit_stmt(s, f));
            cond.iter().for_each(|e| visit_expr(e, f));
            step.iter().for_each(|e| visit_expr(e, f));
            visit_stmt(body, f);
        }
        StmtKind::Foreach {
            container, body, ..
        } => {
            visit_expr(container, f);
            visit_stmt(body, f);
        }
        StmtKind::Switch(e, cases) => {
            visit_expr(e, f);
            for (label, body) in cases {
                label.iter().for_each(|e| visit_expr(e, f));
                visit_exprs(body, f);
            }
        }
        StmtKind::Block(body) => visit_exprs(body, f),
        StmtKind::Function(_, def) => visit_exprs(&def.body, f),
        StmtKind::Class(e, def) => {
            visit_expr(e, f);
            visit_class(def, f);
        }
        StmtKind::Enum(_, items) => items.iter().for_each(|(_, e)| visit_expr(e, f)),
        StmtKind::Try(a, _, b) => {
            visit_stmt(a, f);
            visit_stmt(b, f);
        }
        StmtKind::Break | StmtKind::Continue | StmtKind::Empty => {}
    }
}

fn visit_class(def: &ClassDef, f: &mut impl FnMut(&Expr)) {
    def.base.iter().for_each(|e| visit_expr(e, f));
    for (k, v, _) in &def.members {
        visit_expr(k, f);
        visit_expr(v, f);
    }
}

fn visit_expr(e: &Expr, f: &mut impl FnMut(&Expr)) {
    f(e);
    let mut go = |e: &Expr| visit_expr(e, f);
    match e {
        Expr::Null
        | Expr::Bool(_)
        | Expr::Int(_)
        | Expr::Float(_)
        | Expr::Str(_)
        | Expr::Ident(_)
        | Expr::Root(_)
        | Expr::This
        | Expr::Vargc => {}
        Expr::Vargv(a)
        | Expr::Field(a, _)
        | Expr::Unary(_, a)
        | Expr::Delete(a)
        | Expr::Clone(a)
        | Expr::Typeof(a)
        | Expr::Resume(a)
        | Expr::Thread(a)
        | Expr::WaitThread(a)
        | Expr::WaitThreadSolo(a) => go(a),
        Expr::IncDec { target, .. } => go(target),
        Expr::Index(a, b)
        | Expr::Binary(_, a, b)
        | Expr::And(a, b)
        | Expr::Or(a, b)
        | Expr::NewSlot(a, b)
        | Expr::Delegate(a, b)
        | Expr::DelayThread(a, b) => {
            go(a);
            go(b);
        }
        Expr::Call(callee, args) => {
            go(callee);
            args.iter().for_each(go);
        }
        Expr::Ternary(a, b, c) => {
            go(a);
            go(b);
            go(c);
        }
        Expr::Assign { target, value, .. } => {
            go(target);
            go(value);
        }
        Expr::Table(items) => items.iter().for_each(|(k, v)| {
            visit_expr(k, f);
            visit_expr(v, f);
        }),
        Expr::Array(items) | Expr::Comma(items) => items.iter().for_each(go),
        Expr::Function(def) => visit_exprs(&def.body, f),
        Expr::Class(def) => visit_class(def, f),
    }
}
