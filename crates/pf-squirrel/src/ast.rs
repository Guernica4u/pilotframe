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
