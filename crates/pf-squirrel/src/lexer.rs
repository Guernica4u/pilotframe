//! Script text to tokens.
//!
//! Squirrel 2.2's lexical rules (from its public reference manual): `//`, `/*
//! */` and `#` comments; identifiers; integers (decimal, `0x` hex, `'c'`
//! character literals); floats (with `.` and/or an exponent); `"..."` strings
//! with C escapes and `@"..."` verbatim strings; and the operators below.
//! Respawn added the keywords `thread`, `wait`, `waitthread`, `waitthreadsolo`
//! and `delaythread`
//! (docs/port/scripts.md). Squirrel 2's `delegate` keyword is in; `base` is
//! not a keyword in the game's version (scripts use it as a variable name).

use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),
    Kw(Kw),
    Op(&'static str),
    Eof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kw {
    Break,
    Case,
    Catch,
    Class,
    Clone,
    Const,
    Constructor,
    Continue,
    Default,
    Delaythread,
    Delegate,
    Delete,
    Do,
    Else,
    Enum,
    Extends,
    False,
    For,
    Foreach,
    Function,
    If,
    In,
    Instanceof,
    Local,
    Null,
    Resume,
    Return,
    Static,
    Switch,
    This,
    Thread,
    Throw,
    True,
    Try,
    Typeof,
    Vargc,
    Vargv,
    Wait,
    Waitthread,
    Waitthreadsolo,
    While,
    Yield,
}

fn keyword(s: &str) -> Option<Kw> {
    use Kw::*;
    Some(match s {
        "break" => Break,
        "case" => Case,
        "catch" => Catch,
        "class" => Class,
        "clone" => Clone,
        "const" => Const,
        "constructor" => Constructor,
        "continue" => Continue,
        "default" => Default,
        "delaythread" => Delaythread,
        "delegate" => Delegate,
        "delete" => Delete,
        "do" => Do,
        "else" => Else,
        "enum" => Enum,
        "extends" => Extends,
        "false" => False,
        "for" => For,
        "foreach" => Foreach,
        "function" => Function,
        "if" => If,
        "in" => In,
        "instanceof" => Instanceof,
        "local" => Local,
        "null" => Null,
        "resume" => Resume,
        "return" => Return,
        "static" => Static,
        "switch" => Switch,
        "this" => This,
        "thread" => Thread,
        "throw" => Throw,
        "true" => True,
        "try" => Try,
        "typeof" => Typeof,
        "vargc" => Vargc,
        "vargv" => Vargv,
        "wait" => Wait,
        "waitthread" => Waitthread,
        "waitthreadsolo" => Waitthreadsolo,
        "while" => While,
        "yield" => Yield,
        _ => return None,
    })
}

/// Operators, longest first so `<<=` wins over `<<` and `<`.
const OPS: &[&str] = &[
    ">>>=", "<=>", ">>>", "<<=", ">>=", "...", "<-", "::", "==", "!=", "<=", ">=", "&&", "||",
    "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<", ">>", "+", "-", "*", "/",
    "%", "<", ">", "=", "!", "~", "&", "|", "^", "?", ":", ".", ",", ";", "(", ")", "[", "]", "{",
    "}", "@",
];

/// A token and the line it starts on (1-based).
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LexError {
    pub line: u32,
    pub message: String,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

pub fn lex(src: &str) -> Result<Vec<Token>, LexError> {
    let b = src.as_bytes();
    let mut i = 0;
    let mut line = 1u32;
    let mut out = Vec::new();
    let err = |line, message: String| LexError { line, message };
    while i < b.len() {
        let c = b[i];
        // Whitespace and comments.
        if c == b'\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'/') || c == b'#' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            loop {
                if i + 1 >= b.len() {
                    return Err(err(line, "unterminated comment".into()));
                }
                if b[i] == b'\n' {
                    line += 1;
                }
                if b[i] == b'*' && b[i + 1] == b'/' {
                    i += 2;
                    break;
                }
                i += 1;
            }
            continue;
        }
        let start_line = line;
        let push = |out: &mut Vec<Token>, tok| {
            out.push(Token {
                tok,
                line: start_line,
            })
        };
        // Identifiers and keywords.
        if c.is_ascii_alphabetic() || c == b'_' {
            let s = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let word = &src[s..i];
            push(
                &mut out,
                keyword(word).map_or_else(|| Tok::Ident(word.to_string()), Tok::Kw),
            );
            continue;
        }
        // Numbers.
        if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            if c == b'0' && matches!(b.get(i + 1), Some(b'x' | b'X')) {
                let s = i + 2;
                i = s;
                while i < b.len() && b[i].is_ascii_hexdigit() {
                    i += 1;
                }
                let v = i64::from_str_radix(&src[s..i], 16)
                    .map_err(|e| err(line, format!("bad hex number: {e}")))?;
                push(&mut out, Tok::Int(v));
                continue;
            }
            let s = i;
            let mut float = false;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            let mut end = None;
            if i < b.len() && b[i] == b'.' && b.get(i + 1) != Some(&b'.') {
                float = true;
                i += 1;
                // Like Squirrel's reader, keep taking digits and dots; only
                // the part before a second dot counts (a game script has
                // `100.0.0`, read as 100.0).
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                    if b[i] == b'.' && end.is_none() {
                        end = Some(i);
                    }
                    i += 1;
                }
            }
            if i < b.len() && matches!(b[i], b'e' | b'E') {
                let mut j = i + 1;
                if j < b.len() && matches!(b[j], b'+' | b'-') {
                    j += 1;
                }
                if j < b.len() && b[j].is_ascii_digit() {
                    float = true;
                    i = j;
                    while i < b.len() && b[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let text = &src[s..end.unwrap_or(i)];
            let tok = if float {
                Tok::Float(
                    text.parse()
                        .map_err(|e| err(line, format!("bad number {text}: {e}")))?,
                )
            } else {
                Tok::Int(
                    text.parse()
                        .map_err(|e| err(line, format!("bad number {text}: {e}")))?,
                )
            };
            push(&mut out, tok);
            continue;
        }
        // Strings.
        if c == b'"' || (c == b'@' && b.get(i + 1) == Some(&b'"')) {
            let verbatim = c == b'@';
            i += if verbatim { 2 } else { 1 };
            let mut s = String::new();
            loop {
                let Some(&ch) = b.get(i) else {
                    return Err(err(start_line, "unterminated string".into()));
                };
                if ch == b'"' {
                    // In a verbatim string `""` is a quote.
                    if verbatim && b.get(i + 1) == Some(&b'"') {
                        s.push('"');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                if ch == b'\n' {
                    if !verbatim {
                        return Err(err(line, "newline in string".into()));
                    }
                    line += 1;
                }
                if ch == b'\\' && !verbatim {
                    let (decoded, used) = escape(&b[i + 1..])
                        .ok_or_else(|| err(line, "bad escape in string".into()))?;
                    s.push(decoded);
                    i += 1 + used;
                    continue;
                }
                // Copy one UTF-8 character.
                let len = utf8_len(ch);
                s.push_str(&src[i..i + len]);
                i += len;
            }
            push(&mut out, Tok::Str(s));
            continue;
        }
        // Character literals are integers.
        if c == b'\'' {
            let (ch, used) = if b.get(i + 1) == Some(&b'\\') {
                let (d, u) = escape(&b[i + 2..]).ok_or_else(|| err(line, "bad escape".into()))?;
                (d, u + 1)
            } else {
                let ch = src[i + 1..]
                    .chars()
                    .next()
                    .ok_or_else(|| err(line, "bad character literal".into()))?;
                (ch, ch.len_utf8())
            };
            if b.get(i + 1 + used) != Some(&b'\'') {
                return Err(err(line, "unterminated character literal".into()));
            }
            i += used + 2;
            push(&mut out, Tok::Int(ch as i64));
            continue;
        }
        // Operators.
        if let Some(op) = OPS.iter().find(|op| src[i..].starts_with(*op)) {
            i += op.len();
            push(&mut out, Tok::Op(op));
            continue;
        }
        return Err(err(line, format!("unexpected character {:?}", c as char)));
    }
    out.push(Token {
        tok: Tok::Eof,
        line,
    });
    Ok(out)
}

fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

/// A backslash escape (the bytes after the `\`): the character and how many
/// bytes it used.
fn escape(b: &[u8]) -> Option<(char, usize)> {
    Some(match *b.first()? {
        b'n' => ('\n', 1),
        b't' => ('\t', 1),
        b'r' => ('\r', 1),
        b'a' => ('\x07', 1),
        b'b' => ('\x08', 1),
        b'f' => ('\x0C', 1),
        b'v' => ('\x0B', 1),
        b'0' => ('\0', 1),
        b'\\' => ('\\', 1),
        b'"' => ('"', 1),
        b'\'' => ('\'', 1),
        b'x' => {
            let n = b[1..]
                .iter()
                .take(4)
                .take_while(|c| c.is_ascii_hexdigit())
                .count();
            let v = u32::from_str_radix(std::str::from_utf8(&b[1..1 + n]).ok()?, 16).ok()?;
            (char::from_u32(v)?, 1 + n)
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<Tok> {
        lex(s).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn lexes_the_basics() {
        assert_eq!(
            toks("local x <- 0x1F + 2.5e1 // hi\n'a' \"s\\n\" thread wait"),
            vec![
                Tok::Kw(Kw::Local),
                Tok::Ident("x".into()),
                Tok::Op("<-"),
                Tok::Int(31),
                Tok::Op("+"),
                Tok::Float(25.0),
                Tok::Int(97),
                Tok::Str("s\n".into()),
                Tok::Kw(Kw::Thread),
                Tok::Kw(Kw::Wait),
                Tok::Eof,
            ]
        );
        assert_eq!(
            toks("a::b ... 1.0"),
            vec![
                Tok::Ident("a".into()),
                Tok::Op("::"),
                Tok::Ident("b".into()),
                Tok::Op("..."),
                Tok::Float(1.0),
                Tok::Eof
            ]
        );
    }

    #[test]
    fn counts_lines() {
        let t = lex("a\n/* x\n y */ b\n\"c\"").unwrap();
        assert_eq!(
            t.iter().map(|t| t.line).collect::<Vec<_>>(),
            vec![1, 3, 4, 4]
        );
    }
}
