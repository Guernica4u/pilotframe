//! Tokens to a syntax tree.
//!
//! Squirrel 2.2's grammar from its reference manual, with Respawn's additions
//! found in the game's scripts: default parameter values, and `thread`,
//! `waitthread`, `delaythread(t)` and `wait`. Statements end at `;`, `}` or a
//! line break, as in Squirrel.
//!
//! Operator precedence, loosest first: assignment (`= <- += ...`), `?:`,
//! `||`, `&&`, `|`, `^`, `&`, `== !=`, `< <= > >= in instanceof`,
//! `<< >> >>>`, `+ -`, `* / %`, prefix (`- ! ~ ++ -- typeof clone delete
//! resume delegate thread waitthread delaythread`), postfix (`. [] () ++ --`).
//! Functions may list Squirrel 2 free variables: `function(a) : (x, y) { }`.

use std::fmt;
use std::rc::Rc;

use crate::ast::*;
use crate::lexer::{Kw, Tok, Token};

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub line: u32,
    /// The message as the game's compiler words it (where we know it).
    pub message: String,
    /// The token the error was found at (ours, for debugging).
    pub near: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)?;
        if !self.near.is_empty() {
            write!(f, " (at {})", self.near)?;
        }
        Ok(())
    }
}

type Result<T> = std::result::Result<T, ParseError>;

pub struct Parser {
    toks: Vec<Token>,
    pos: usize,
}

/// Parse a whole script.
pub fn parse(src: &str) -> Result<Vec<Stmt>> {
    let toks = crate::lexer::lex(src).map_err(|e| ParseError {
        line: e.line,
        message: e.message,
        near: String::new(),
    })?;
    let mut p = Parser { toks, pos: 0 };
    let mut out = Vec::new();
    while !p.at_eof() {
        out.push(p.statement()?);
    }
    Ok(out)
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, n: usize) -> &Tok {
        &self.toks[(self.pos + n).min(self.toks.len() - 1)].tok
    }

    fn line(&self) -> u32 {
        self.toks[self.pos].line
    }

    fn prev_line(&self) -> u32 {
        self.toks[self.pos.saturating_sub(1)].line
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek(), Tok::Eof)
    }

    fn next(&mut self) -> Tok {
        let t = self.toks[self.pos].tok.clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn err<T>(&self, message: impl Into<String>) -> Result<T> {
        Err(ParseError {
            line: self.line(),
            message: message.into(),
            near: format!("{:?}", self.peek()),
        })
    }

    fn is_op(&self, op: &str) -> bool {
        matches!(self.peek(), Tok::Op(o) if *o == op)
    }

    fn is_kw(&self, kw: Kw) -> bool {
        matches!(self.peek(), Tok::Kw(k) if *k == kw)
    }

    fn eat_op(&mut self, op: &str) -> bool {
        if self.is_op(op) {
            self.next();
            true
        } else {
            false
        }
    }

    fn eat_kw(&mut self, kw: Kw) -> bool {
        if self.is_kw(kw) {
            self.next();
            true
        } else {
            false
        }
    }

    fn expect_op(&mut self, op: &str) -> Result<()> {
        if self.eat_op(op) {
            Ok(())
        } else {
            self.err(format!("expected `{op}`"))
        }
    }

    fn ident(&mut self) -> Result<Rc<str>> {
        match self.peek().clone() {
            Tok::Ident(s) => {
                self.next();
                Ok(s.into())
            }
            // `constructor` is a plain name outside class bodies too.
            Tok::Kw(Kw::Constructor) => {
                self.next();
                Ok("constructor".into())
            }
            _ => self.err("expected a name"),
        }
    }

    /// The end of a statement: `;`, or `}`/end of file/a new line next.
    /// The end of a statement. As in the game's compiler, a statement whose
    /// last token is `}` takes no `;`: a `;` after it is an empty statement of
    /// its own, which then needs `;` or a line break after it, so
    /// `local t = {}; f()` on one line is an error (oracle O001).
    fn end_statement(&mut self) -> Result<()> {
        if matches!(self.toks[self.pos.saturating_sub(1)].tok, Tok::Op("}")) {
            return Ok(());
        }
        if self.eat_op(";") {
            return Ok(());
        }
        self.end_without_semicolon()
    }

    /// `}`, the end of the file or a line break must come next.
    fn end_without_semicolon(&mut self) -> Result<()> {
        if self.is_op(";") || self.is_op("}") || self.at_eof() || self.line() > self.prev_line() {
            return Ok(());
        }
        self.err("end of statement expected (; or lf)")
    }

    // ---- Statements ----

    pub fn statement(&mut self) -> Result<Stmt> {
        let line = self.line();
        let kind = self.statement_kind()?;
        Ok(Stmt { line, kind })
    }

    fn statement_kind(&mut self) -> Result<StmtKind> {
        Ok(match self.peek().clone() {
            Tok::Op(";") => {
                self.next();
                self.end_without_semicolon()?;
                StmtKind::Empty
            }
            Tok::Op("{") => StmtKind::Block(self.block()?),
            Tok::Kw(Kw::Local) => {
                self.next();
                if self.eat_kw(Kw::Function) {
                    let name = self.ident()?;
                    let f = self.function_rest(name.clone())?;
                    return Ok(StmtKind::Local(vec![(name, Some(Expr::Function(f)))]));
                }
                let mut vars = Vec::new();
                loop {
                    let name = self.ident()?;
                    let value = if self.eat_op("=") {
                        Some(self.expr_no_comma()?)
                    } else {
                        None
                    };
                    vars.push((name, value));
                    if !self.eat_op(",") {
                        break;
                    }
                }
                self.end_statement()?;
                StmtKind::Local(vars)
            }
            Tok::Kw(Kw::If) => {
                self.next();
                self.expect_op("(")?;
                let cond = self.expr()?;
                self.expect_op(")")?;
                let then = Box::new(self.statement()?);
                // `else` may follow an optional `;`.
                let save = self.pos;
                self.eat_op(";");
                let other = if self.eat_kw(Kw::Else) {
                    Some(Box::new(self.statement()?))
                } else {
                    self.pos = save;
                    None
                };
                StmtKind::If(cond, then, other)
            }
            Tok::Kw(Kw::While) => {
                self.next();
                self.expect_op("(")?;
                let cond = self.expr()?;
                self.expect_op(")")?;
                StmtKind::While(cond, Box::new(self.statement()?))
            }
            Tok::Kw(Kw::Do) => {
                self.next();
                let body = Box::new(self.statement()?);
                if !self.eat_kw(Kw::While) {
                    return self.err("expected `while` after `do` body");
                }
                self.expect_op("(")?;
                let cond = self.expr()?;
                self.expect_op(")")?;
                self.end_statement()?;
                StmtKind::DoWhile(body, cond)
            }
            Tok::Kw(Kw::For) => {
                self.next();
                self.expect_op("(")?;
                let init = if self.is_op(";") {
                    None
                } else if self.is_kw(Kw::Local) {
                    let line = self.line();
                    self.next();
                    let mut vars = Vec::new();
                    loop {
                        let name = self.ident()?;
                        let value = if self.eat_op("=") {
                            Some(self.expr_no_comma()?)
                        } else {
                            None
                        };
                        vars.push((name, value));
                        if !self.eat_op(",") {
                            break;
                        }
                    }
                    Some(Box::new(Stmt {
                        line,
                        kind: StmtKind::Local(vars),
                    }))
                } else {
                    let line = self.line();
                    Some(Box::new(Stmt {
                        line,
                        kind: StmtKind::Expr(self.expr()?),
                    }))
                };
                self.expect_op(";")?;
                let cond = if self.is_op(";") {
                    None
                } else {
                    Some(self.expr()?)
                };
                self.expect_op(";")?;
                let step = if self.is_op(")") {
                    None
                } else {
                    Some(self.expr()?)
                };
                self.expect_op(")")?;
                StmtKind::For {
                    init,
                    cond,
                    step,
                    body: Box::new(self.statement()?),
                }
            }
            Tok::Kw(Kw::Foreach) => {
                self.next();
                self.expect_op("(")?;
                let first = self.ident()?;
                let (key, value) = if self.eat_op(",") {
                    (Some(first), self.ident()?)
                } else {
                    (None, first)
                };
                if !self.eat_kw(Kw::In) {
                    return self.err("expected `in`");
                }
                let container = self.expr()?;
                self.expect_op(")")?;
                StmtKind::Foreach {
                    key,
                    value,
                    container,
                    body: Box::new(self.statement()?),
                }
            }
            Tok::Kw(Kw::Switch) => {
                self.next();
                self.expect_op("(")?;
                let subject = self.expr()?;
                self.expect_op(")")?;
                self.expect_op("{")?;
                let mut cases = Vec::new();
                while !self.eat_op("}") {
                    let label = if self.eat_kw(Kw::Case) {
                        Some(self.expr()?)
                    } else if self.eat_kw(Kw::Default) {
                        None
                    } else {
                        return self.err("expected `case` or `default`");
                    };
                    self.expect_op(":")?;
                    let mut body = Vec::new();
                    while !self.is_kw(Kw::Case) && !self.is_kw(Kw::Default) && !self.is_op("}") {
                        body.push(self.statement()?);
                    }
                    cases.push((label, body));
                }
                StmtKind::Switch(subject, cases)
            }
            Tok::Kw(Kw::Break) => {
                self.next();
                self.end_statement()?;
                StmtKind::Break
            }
            Tok::Kw(Kw::Continue) => {
                self.next();
                self.end_statement()?;
                StmtKind::Continue
            }
            Tok::Kw(Kw::Return) => {
                let line = self.line();
                self.next();
                let value =
                    if self.is_op(";") || self.is_op("}") || self.at_eof() || self.line() > line {
                        None
                    } else {
                        Some(self.expr()?)
                    };
                self.end_statement()?;
                StmtKind::Return(value)
            }
            Tok::Kw(Kw::Function) => {
                self.next();
                let mut path = vec![self.ident()?];
                while self.eat_op("::") {
                    path.push(self.ident()?);
                }
                let name = path.last().cloned().unwrap_or_else(|| "".into());
                let f = self.function_rest(name)?;
                StmtKind::Function(path, f)
            }
            Tok::Kw(Kw::Class) => {
                self.next();
                let target = self.prefix_chain_names()?;
                let class = self.class_rest()?;
                StmtKind::Class(target, class)
            }
            Tok::Kw(Kw::Const) => {
                self.next();
                let name = self.ident()?;
                self.expect_op("=")?;
                let value = self.constant()?;
                self.end_statement()?;
                StmtKind::Const(name, value)
            }
            Tok::Kw(Kw::Enum) => {
                self.next();
                let name = self.ident()?;
                self.expect_op("{")?;
                let mut items = Vec::new();
                let mut next_value = 0i64;
                while !self.eat_op("}") {
                    let item = self.ident()?;
                    let value = if self.eat_op("=") {
                        let v = self.constant()?;
                        if let Expr::Int(n) = v {
                            next_value = n + 1;
                        }
                        v
                    } else {
                        let v = Expr::Int(next_value);
                        next_value += 1;
                        v
                    };
                    items.push((item, value));
                    self.eat_op(",");
                }
                StmtKind::Enum(name, items)
            }
            Tok::Kw(Kw::Try) => {
                self.next();
                let body = Box::new(self.statement()?);
                if !self.eat_kw(Kw::Catch) {
                    return self.err("expected `catch`");
                }
                self.expect_op("(")?;
                let name = self.ident()?;
                self.expect_op(")")?;
                StmtKind::Try(body, name, Box::new(self.statement()?))
            }
            Tok::Kw(Kw::Throw) => {
                self.next();
                let e = self.expr()?;
                self.end_statement()?;
                StmtKind::Throw(e)
            }
            // `Assert` is a keyword in the game's compiler, and an `Assert(
            // ... )` statement compiles to nothing: it skips to the matching
            // `)` (launcher.dll; the shipped game has asserts compiled out).
            Tok::Ident(ref name) if name == "Assert" && self.peek2_is_op("(") => {
                self.next();
                self.next();
                let mut depth = 0;
                loop {
                    match self.next() {
                        Tok::Op("(") => depth += 1,
                        Tok::Op(")") if depth == 0 => break,
                        Tok::Op(")") => depth -= 1,
                        Tok::Eof => return self.err("expected ')'"),
                        _ => {}
                    }
                }
                StmtKind::Empty
            }
            Tok::Kw(Kw::Wait) => {
                self.next();
                let e = self.expr()?;
                self.end_statement()?;
                StmtKind::Wait(e)
            }
            _ => {
                let e = self.expr()?;
                self.end_statement()?;
                StmtKind::Expr(e)
            }
        })
    }

    fn block(&mut self) -> Result<Vec<Stmt>> {
        self.expect_op("{")?;
        let mut out = Vec::new();
        while !self.eat_op("}") {
            if self.at_eof() {
                return self.err("unclosed `{`");
            }
            out.push(self.statement()?);
        }
        Ok(out)
    }

    /// A constant's value: a literal, optionally negated.
    fn constant(&mut self) -> Result<Expr> {
        let neg = self.eat_op("-");
        let v = match self.next() {
            Tok::Int(n) => Expr::Int(if neg { -n } else { n }),
            Tok::Float(f) => Expr::Float(if neg { -f } else { f }),
            Tok::Str(s) if !neg => Expr::Str(s.into()),
            Tok::Kw(Kw::True) if !neg => Expr::Bool(true),
            Tok::Kw(Kw::False) if !neg => Expr::Bool(false),
            Tok::Kw(Kw::Null) if !neg => Expr::Null,
            _ => {
                self.pos -= 1;
                return self.err("expected a constant");
            }
        };
        Ok(v)
    }

    /// `a.b.c` for a class statement's target.
    fn prefix_chain_names(&mut self) -> Result<Expr> {
        let mut e = if self.eat_op("::") {
            Expr::Root(self.ident()?)
        } else {
            Expr::Ident(self.ident()?)
        };
        while self.eat_op(".") || self.eat_op("::") {
            e = Expr::Field(Box::new(e), self.ident()?);
        }
        Ok(e)
    }

    /// After `function name`: `(params) body`.
    fn function_rest(&mut self, name: Rc<str>) -> Result<Rc<FuncDef>> {
        let line = self.line();
        self.expect_op("(")?;
        let mut params = Vec::new();
        let mut varargs = false;
        while !self.eat_op(")") {
            if self.eat_op("...") {
                if params.iter().any(|p: &Param| p.default.is_some()) {
                    return self.err(
                        "function with default parameters cannot have variable number of parameters",
                    );
                }
                varargs = true;
                self.expect_op(")")?;
                break;
            }
            let pname = self.ident()?;
            let default = if self.eat_op("=") {
                Some(self.expr_no_comma()?)
            } else {
                None
            };
            params.push(Param {
                name: pname,
                default,
            });
            if !self.eat_op(",") {
                self.expect_op(")")?;
                break;
            }
        }
        let mut free_vars = Vec::new();
        if self.eat_op(":") {
            self.expect_op("(")?;
            while !self.eat_op(")") {
                free_vars.push(self.ident()?);
                if !self.eat_op(",") {
                    self.expect_op(")")?;
                    break;
                }
            }
        }
        let body = if self.is_op("{") {
            self.block()?
        } else {
            vec![self.statement()?]
        };
        Ok(Rc::new(FuncDef {
            name,
            params,
            varargs,
            free_vars,
            body,
            line,
        }))
    }

    /// After `class name`: `[extends base] { members }`.
    fn class_rest(&mut self) -> Result<Rc<ClassDef>> {
        let base = if self.eat_kw(Kw::Extends) {
            Some(self.prefix_chain_names()?)
        } else {
            None
        };
        self.expect_op("{")?;
        let mut members = Vec::new();
        while !self.eat_op("}") {
            let is_static = self.eat_kw(Kw::Static);
            if self.eat_kw(Kw::Function) {
                let name = self.ident()?;
                let f = self.function_rest(name.clone())?;
                members.push((Expr::Str(name), Expr::Function(f), is_static));
            } else if self.eat_kw(Kw::Constructor) {
                let f = self.function_rest("constructor".into())?;
                members.push((
                    Expr::Str("constructor".into()),
                    Expr::Function(f),
                    is_static,
                ));
            } else if self.eat_op("[") {
                let key = self.expr()?;
                self.expect_op("]")?;
                self.expect_op("=")?;
                let value = self.expr_no_comma()?;
                members.push((key, value, is_static));
            } else {
                let name = self.ident()?;
                self.expect_op("=")?;
                let value = self.expr_no_comma()?;
                members.push((Expr::Str(name), value, is_static));
            }
            self.eat_op(";");
            self.eat_op(",");
        }
        Ok(Rc::new(ClassDef { base, members }))
    }

    // ---- Expressions ----

    pub fn expr(&mut self) -> Result<Expr> {
        let first = self.assignment()?;
        if self.is_op(",") {
            let mut all = vec![first];
            while self.eat_op(",") {
                all.push(self.assignment()?);
            }
            return Ok(Expr::Comma(all));
        }
        Ok(first)
    }

    fn expr_no_comma(&mut self) -> Result<Expr> {
        self.assignment()
    }

    fn assignment(&mut self) -> Result<Expr> {
        let lhs = self.ternary()?;
        let op = match self.peek() {
            Tok::Op("=") => None,
            Tok::Op("<-") => {
                self.next();
                let value = self.assignment()?;
                return Ok(Expr::NewSlot(Box::new(lhs), Box::new(value)));
            }
            Tok::Op("+=") => Some(BinOp::Add),
            Tok::Op("-=") => Some(BinOp::Sub),
            Tok::Op("*=") => Some(BinOp::Mul),
            Tok::Op("/=") => Some(BinOp::Div),
            Tok::Op("%=") => Some(BinOp::Mod),
            Tok::Op("&=") => Some(BinOp::BitAnd),
            Tok::Op("|=") => Some(BinOp::BitOr),
            Tok::Op("^=") => Some(BinOp::BitXor),
            Tok::Op("<<=") => Some(BinOp::Shl),
            Tok::Op(">>=") => Some(BinOp::Shr),
            Tok::Op(">>>=") => Some(BinOp::UShr),
            _ => return Ok(lhs),
        };
        self.next();
        let value = self.assignment()?;
        Ok(Expr::Assign {
            target: Box::new(lhs),
            op,
            value: Box::new(value),
        })
    }

    fn ternary(&mut self) -> Result<Expr> {
        let cond = self.or()?;
        if self.eat_op("?") {
            let a = self.assignment()?;
            self.expect_op(":")?;
            let b = self.assignment()?;
            return Ok(Expr::Ternary(Box::new(cond), Box::new(a), Box::new(b)));
        }
        Ok(cond)
    }

    fn or(&mut self) -> Result<Expr> {
        let mut e = self.and()?;
        while self.eat_op("||") {
            e = Expr::Or(Box::new(e), Box::new(self.and()?));
        }
        Ok(e)
    }

    fn and(&mut self) -> Result<Expr> {
        let mut e = self.binary(0)?;
        while self.eat_op("&&") {
            e = Expr::And(Box::new(e), Box::new(self.binary(0)?));
        }
        Ok(e)
    }

    /// Binary operators by level, loosest first.
    fn binary(&mut self, level: usize) -> Result<Expr> {
        const LEVELS: &[&[(&str, BinOp)]] = &[
            &[("|", BinOp::BitOr)],
            &[("^", BinOp::BitXor)],
            &[("&", BinOp::BitAnd)],
            &[("==", BinOp::Eq), ("!=", BinOp::Ne)],
            &[
                ("<", BinOp::Lt),
                ("<=", BinOp::Le),
                (">", BinOp::Gt),
                (">=", BinOp::Ge),
            ],
            &[("<<", BinOp::Shl), (">>", BinOp::Shr), (">>>", BinOp::UShr)],
            &[("+", BinOp::Add), ("-", BinOp::Sub)],
            &[("*", BinOp::Mul), ("/", BinOp::Div), ("%", BinOp::Mod)],
        ];
        if level == LEVELS.len() {
            return self.prefix();
        }
        let mut e = self.binary(level + 1)?;
        loop {
            let op = match self.peek() {
                Tok::Op(o) => LEVELS[level]
                    .iter()
                    .find(|(s, _)| s == o)
                    .map(|&(_, op)| op),
                // `in` and `instanceof` sit with the comparisons.
                Tok::Kw(Kw::In) if level == 4 => Some(BinOp::In),
                Tok::Kw(Kw::Instanceof) if level == 4 => Some(BinOp::InstanceOf),
                _ => None,
            };
            let Some(op) = op else { break };
            self.next();
            let rhs = self.binary(level + 1)?;
            e = Expr::Binary(op, Box::new(e), Box::new(rhs));
        }
        Ok(e)
    }

    fn prefix(&mut self) -> Result<Expr> {
        let unary = |op, e| Ok(Expr::Unary(op, Box::new(e)));
        match self.peek().clone() {
            Tok::Op("-") => {
                self.next();
                // Fold literal negatives so `-1` is a constant.
                match self.prefix()? {
                    Expr::Int(n) => Ok(Expr::Int(-n)),
                    Expr::Float(f) => Ok(Expr::Float(-f)),
                    e => unary(UnOp::Neg, e),
                }
            }
            Tok::Op("!") => {
                self.next();
                let e = self.prefix()?;
                unary(UnOp::Not, e)
            }
            Tok::Op("~") => {
                self.next();
                let e = self.prefix()?;
                unary(UnOp::BitNot, e)
            }
            Tok::Op(op @ ("++" | "--")) => {
                self.next();
                let target = self.prefix()?;
                Ok(Expr::IncDec {
                    target: Box::new(target),
                    delta: if op == "++" { 1 } else { -1 },
                    post: false,
                })
            }
            Tok::Kw(Kw::Typeof) => {
                self.next();
                Ok(Expr::Typeof(Box::new(self.prefix()?)))
            }
            Tok::Kw(Kw::Clone) => {
                self.next();
                Ok(Expr::Clone(Box::new(self.prefix()?)))
            }
            Tok::Kw(Kw::Delete) => {
                self.next();
                Ok(Expr::Delete(Box::new(self.prefix()?)))
            }
            Tok::Kw(Kw::Resume) => {
                self.next();
                Ok(Expr::Resume(Box::new(self.prefix()?)))
            }
            Tok::Kw(Kw::Delegate) => {
                self.next();
                let parent = self.prefix()?;
                self.expect_op(":")?;
                let table = self.prefix()?;
                Ok(Expr::Delegate(Box::new(parent), Box::new(table)))
            }
            Tok::Kw(Kw::Thread) => {
                self.next();
                Ok(Expr::Thread(Box::new(self.prefix()?)))
            }
            Tok::Kw(Kw::Waitthread) => {
                self.next();
                Ok(Expr::WaitThread(Box::new(self.prefix()?)))
            }
            Tok::Kw(Kw::Waitthreadsolo) => {
                self.next();
                Ok(Expr::WaitThreadSolo(Box::new(self.prefix()?)))
            }
            Tok::Kw(Kw::Delaythread) => {
                self.next();
                self.expect_op("(")?;
                let delay = self.expr()?;
                self.expect_op(")")?;
                let call = self.prefix()?;
                Ok(Expr::DelayThread(Box::new(delay), Box::new(call)))
            }
            _ => self.postfix(),
        }
    }

    fn postfix(&mut self) -> Result<Expr> {
        let mut e = self.primary()?;
        loop {
            match self.peek() {
                Tok::Op(".") => {
                    self.next();
                    let name = match self.next() {
                        Tok::Ident(s) => s.into(),
                        // Keywords are allowed as field names after `.`.
                        Tok::Kw(k) => format!("{k:?}").to_ascii_lowercase().into(),
                        _ => {
                            self.pos -= 1;
                            return self.err("expected a field name");
                        }
                    };
                    e = Expr::Field(Box::new(e), name);
                }
                Tok::Op("[") => {
                    // A `[` on a new line starts a new statement.
                    if self.line() > self.prev_line() {
                        break;
                    }
                    self.next();
                    let idx = self.expr()?;
                    self.expect_op("]")?;
                    e = Expr::Index(Box::new(e), Box::new(idx));
                }
                Tok::Op("(") => {
                    self.next();
                    let mut args = Vec::new();
                    while !self.eat_op(")") {
                        args.push(self.expr_no_comma()?);
                        if !self.eat_op(",") {
                            self.expect_op(")")?;
                            break;
                        }
                    }
                    e = Expr::Call(Box::new(e), args);
                }
                Tok::Op(op @ ("++" | "--")) => {
                    // Postfix only on the same line.
                    if self.line() > self.prev_line() {
                        break;
                    }
                    let delta = if *op == "++" { 1 } else { -1 };
                    self.next();
                    e = Expr::IncDec {
                        target: Box::new(e),
                        delta,
                        post: true,
                    };
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn primary(&mut self) -> Result<Expr> {
        let t = self.next();
        Ok(match t {
            Tok::Int(n) => Expr::Int(n),
            Tok::Float(f) => Expr::Float(f),
            Tok::Str(s) => Expr::Str(s.into()),
            Tok::Ident(s) => Expr::Ident(s.into()),
            Tok::Kw(Kw::Constructor) => Expr::Ident("constructor".into()),
            Tok::Kw(Kw::Null) => Expr::Null,
            Tok::Kw(Kw::True) => Expr::Bool(true),
            Tok::Kw(Kw::False) => Expr::Bool(false),
            Tok::Kw(Kw::This) => Expr::This,
            Tok::Kw(Kw::Vargc) => Expr::Vargc,
            Tok::Kw(Kw::Vargv) => {
                self.expect_op("[")?;
                let i = self.expr()?;
                self.expect_op("]")?;
                Expr::Vargv(Box::new(i))
            }
            Tok::Op("::") => Expr::Root(self.ident()?),
            Tok::Op("(") => {
                let e = self.expr()?;
                self.expect_op(")")?;
                e
            }
            Tok::Op("[") => {
                let mut items = Vec::new();
                while !self.eat_op("]") {
                    items.push(self.expr_no_comma()?);
                    self.eat_op(",");
                }
                Expr::Array(items)
            }
            Tok::Op("{") => {
                let mut entries = Vec::new();
                while !self.eat_op("}") {
                    let key = if self.eat_op("[") {
                        let k = self.expr()?;
                        self.expect_op("]")?;
                        self.expect_op("=")?;
                        k
                    } else if self.eat_kw(Kw::Function) {
                        let name = self.ident()?;
                        let f = self.function_rest(name.clone())?;
                        entries.push((Expr::Str(name), Expr::Function(f)));
                        self.eat_op(",");
                        continue;
                    } else {
                        let name = match self.next() {
                            Tok::Ident(s) => s,
                            Tok::Str(s) => s,
                            Tok::Kw(k) => format!("{k:?}").to_ascii_lowercase(),
                            _ => {
                                self.pos -= 1;
                                return self.err("expected a table key");
                            }
                        };
                        // `key = value` or JSON-style `"key": value`.
                        if !self.eat_op("=") {
                            self.expect_op(":")?;
                        }
                        Expr::Str(name.into())
                    };
                    let value = self.expr_no_comma()?;
                    entries.push((key, value));
                    self.eat_op(",");
                }
                Expr::Table(entries)
            }
            Tok::Kw(Kw::Function) => {
                // `function(...)` expression (an optional name is ignored).
                let name = if matches!(self.peek(), Tok::Ident(_)) {
                    self.ident()?
                } else {
                    "".into()
                };
                Expr::Function(self.function_rest(name)?)
            }
            Tok::Kw(Kw::Class) => Expr::Class(self.class_rest()?),
            // `@` only starts a verbatim string (`@(...)` lambdas don't exist
            // in the game's version: "string expected", oracle X003).
            Tok::Op("@") => return self.err("string expected"),
            _ => {
                self.pos -= 1;
                return self.err("expression expected");
            }
        })
    }

    fn peek2_is_op(&self, op: &str) -> bool {
        matches!(self.peek_at(1), Tok::Op(o) if *o == op)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_respawn_forms() {
        let src = r#"
            const FOO = -2
            enum E { A, B = 5, C }
            function A::B( x, y = null ) { return x }
            function V( a, ... ) { return vargc }
            class C { constructor( a ) { this.a <- a } a = 0 }
            function Go()
            {
                thread Think( 1 )
                waitthread Think( 2 )
                delaythread( 0.5 ) Think( 3 )
                wait 0.1
                local t = { a = 1, [2] = "b", function f() { return 3 } }
                foreach ( k, v in t ) printl( k )
                if ( "a" in t && !( 3 in t ) ) ::x <- t.a++
                else x = [1, 2,]
                switch ( x ) { case 1: break; default: x += 1 }
            }
        "#;
        let stmts = parse(src).unwrap();
        assert_eq!(stmts.len(), 6);
        assert!(matches!(stmts[1].kind, StmtKind::Enum(_, ref v) if v[2].1 == Expr::Int(6)));
    }

    #[test]
    fn statements_end_at_line_breaks() {
        // `(` on a new line still calls (as in Squirrel); `[` starts anew.
        let stmts = parse("local a = b\n(c).d()\nx = y\n[1].len()").unwrap();
        assert_eq!(stmts.len(), 3);
    }
}

#[cfg(test)]
mod game_rules {
    use super::*;

    /// Grammar rules measured in the real game (oracle run 1).
    #[test]
    fn matches_the_game() {
        let fails = |src: &str| parse(src).unwrap_err().message;
        assert!(fails("local t = {}; f()").starts_with("end of statement expected (; or lf)"));
        assert!(parse("local t = {}\nf()").is_ok());
        assert!(parse("local a = [1]; f()").is_ok());
        assert!(fails("local f = @(a) a").starts_with("string expected"));
        assert!(parse("local s = @\"v\"").is_ok());
        assert!(fails("local f = function(a = 1, ...) {}").starts_with("function with default"));
        assert!(fails("local x = 10 <=> 3").starts_with("expression expected"));
        // `yield` is a plain name: `yield 1` is two expressions on one line.
        assert!(fails("function g() { yield 1 }").starts_with("end of statement expected"));
    }
}
