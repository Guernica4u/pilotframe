//! Valve KeyValues text: nested `key value` / `key { ... }` pairs.
//!
//! What the game's UI files use, all handled here:
//! - keys and values quoted or bare, `//` comments;
//! - a platform condition after an entry, e.g. `"tall" "20" [$X360]`, which keeps
//!   or drops it (we are the PC, English build);
//! - `#base "file.res"` lines at the top, which name a file to merge under this
//!   one ([`Node::merge`]).

/// A key with either a text value or a block of children.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub key: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Text(String),
    Block(Vec<Node>),
}

/// A parsed file: its `#base` includes and its top-level entries.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct File {
    pub bases: Vec<String>,
    pub nodes: Vec<Node>,
}

impl Node {
    /// The first child called `key` (case-insensitive), if this is a block.
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.children()
            .iter()
            .find(|n| n.key.eq_ignore_ascii_case(key))
    }

    /// The text of the first child called `key`.
    pub fn text(&self, key: &str) -> Option<&str> {
        match &self.get(key)?.value {
            Value::Text(t) => Some(t),
            Value::Block(_) => None,
        }
    }

    /// The children, or nothing if this is a text value.
    pub fn children(&self) -> &[Node] {
        match &self.value {
            Value::Block(c) => c,
            Value::Text(_) => &[],
        }
    }

    /// Lay `overlay` over this node: text values are replaced, blocks with the
    /// same key are merged in turn, and new keys are added. This is how a file's
    /// `#base` is combined with the file itself (the file is the overlay).
    pub fn merge(&mut self, overlay: &Node) {
        match (&mut self.value, &overlay.value) {
            (Value::Block(mine), Value::Block(theirs)) => {
                for t in theirs {
                    match mine.iter_mut().find(|m| m.key.eq_ignore_ascii_case(&t.key)) {
                        Some(m) => m.merge(t),
                        None => mine.push(t.clone()),
                    }
                }
            }
            (mine, theirs) => *mine = theirs.clone(),
        }
    }
}

/// Parse a file, keeping entries whose platform condition holds for the PC
/// English build.
pub fn parse(text: &str) -> File {
    parse_with(text, pc_english)
}

/// Parse a file, keeping entries for which `keep(condition)` is true.
pub fn parse_with(text: &str, keep: impl Fn(&str) -> bool) -> File {
    let tokens = tokenize(text);
    let mut file = File::default();
    let mut i = 0;
    // `#base "x"` / `#include "x"` lines come first.
    while let Some(Token::Text(t)) = tokens.get(i) {
        if !t.starts_with('#') {
            break;
        }
        if let Some(Token::Text(path)) = tokens.get(i + 1) {
            if t.eq_ignore_ascii_case("#base") || t.eq_ignore_ascii_case("#include") {
                file.bases.push(path.clone());
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    file.nodes = block(&tokens, &mut i, &keep);
    file
}

/// Whether a condition like `$X360 || $PS3` or `!$GAMECONSOLE` holds for us.
pub fn pc_english(condition: &str) -> bool {
    condition.split("||").any(|any| {
        any.split("&&").all(|term| {
            let term = term.trim();
            let (negate, name) = match term.strip_prefix('!') {
                Some(rest) => (true, rest.trim()),
                None => (false, term),
            };
            let on = matches!(
                name.to_ascii_uppercase().as_str(),
                "$WIN32" | "$WIN64" | "$WINDOWS" | "$PC" | "$ENGLISH"
            );
            on != negate
        })
    })
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Open,
    Close,
    Text(String),
    Condition(String),
}

/// Entries until a closing brace (or the end), dropping those whose condition fails.
fn block(tokens: &[Token], i: &mut usize, keep: &impl Fn(&str) -> bool) -> Vec<Node> {
    let mut out = Vec::new();
    while let Some(token) = tokens.get(*i) {
        *i += 1;
        let key = match token {
            Token::Close => break,
            Token::Text(k) => k.clone(),
            // Stray braces or conditions: skip.
            Token::Open | Token::Condition(_) => continue,
        };
        // A condition may sit between a key and its block, too.
        let mut cond = take_condition(tokens, i);
        let value = match tokens.get(*i) {
            Some(Token::Open) => {
                *i += 1;
                Value::Block(block(tokens, i, keep))
            }
            Some(Token::Text(v)) => {
                *i += 1;
                Value::Text(v.clone())
            }
            _ => continue,
        };
        if let Some(c) = take_condition(tokens, i) {
            cond = Some(c);
        }
        if cond.as_deref().is_none_or(keep) {
            out.push(Node { key, value });
        }
    }
    out
}

fn take_condition(tokens: &[Token], i: &mut usize) -> Option<String> {
    match tokens.get(*i) {
        Some(Token::Condition(c)) => {
            *i += 1;
            Some(c.clone())
        }
        _ => None,
    }
}

fn tokenize(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            _ if c.is_whitespace() || c == '\u{feff}' => {
                chars.next();
            }
            '{' => {
                chars.next();
                out.push(Token::Open);
            }
            '}' => {
                chars.next();
                out.push(Token::Close);
            }
            '/' => {
                chars.next();
                if chars.peek() == Some(&'/') {
                    for c in chars.by_ref() {
                        if c == '\n' {
                            break;
                        }
                    }
                } else {
                    out.push(Token::Text(bare(&mut chars, "/")));
                }
            }
            '"' => {
                chars.next();
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(other) => s.push(other),
                            None => break,
                        },
                        _ => s.push(c),
                    }
                }
                out.push(Token::Text(s));
            }
            '[' => {
                chars.next();
                let mut s = String::new();
                for c in chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                    s.push(c);
                }
                out.push(Token::Condition(s.trim().to_string()));
            }
            _ => out.push(Token::Text(bare(&mut chars, ""))),
        }
    }
    out
}

/// A bare word: up to whitespace, a brace, a quote or a condition.
fn bare(chars: &mut std::iter::Peekable<std::str::Chars>, start: &str) -> String {
    let mut s = start.to_string();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() || matches!(c, '{' | '}' | '"' | '[') {
            break;
        }
        s.push(c);
        chars.next();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const RES: &str = r#"#base "parent.res"
        "Resource/UI/Test.res"
        {
            // a comment
            Label
            {
                ControlName   Label
                "xpos"        "c-64"
                tall          30          [$X360]
                tall          20          [!$X360]
                labelText     "say \"hi\""
            }
            Hidden [$GAMECONSOLE]
            {
                visible 1
            }
        }"#;

    #[test]
    fn parses_blocks_values_and_conditions() {
        let file = parse(RES);
        assert_eq!(file.bases, ["parent.res"]);
        let root = &file.nodes[0];
        assert_eq!(root.key, "Resource/UI/Test.res");
        let label = root.get("label").unwrap();
        assert_eq!(label.text("XPOS"), Some("c-64"));
        assert_eq!(label.text("tall"), Some("20"));
        assert_eq!(label.text("labelText"), Some("say \"hi\""));
        assert!(root.get("Hidden").is_none());
    }

    #[test]
    fn merge_overlays_and_adds() {
        let mut base = parse("r { a { x 1 y 2 } b { z 3 } }").nodes.remove(0);
        let over = parse("r { a { y 5 } c { w 6 } }").nodes.remove(0);
        base.merge(&over);
        let a = base.get("a").unwrap();
        assert_eq!((a.text("x"), a.text("y")), (Some("1"), Some("5")));
        assert!(base.get("b").is_some() && base.get("c").is_some());
    }

    #[test]
    fn conditions() {
        assert!(pc_english("$WIN32"));
        assert!(!pc_english("$X360 || $PS3"));
        assert!(pc_english("!$GAMECONSOLE"));
        assert!(!pc_english("$ENGLISH && $X360"));
    }
}
