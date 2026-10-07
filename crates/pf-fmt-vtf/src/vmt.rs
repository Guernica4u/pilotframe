//! `.vmt` material files: a shader name and a block of `"$key" "value"` pairs
//! (Valve KeyValues text). We only need the top-level pairs, e.g. `$basetexture`.

use std::collections::HashMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Vmt {
    /// e.g. `LightmappedGeneric`.
    pub shader: String,
    /// Lower-cased keys (`$basetexture`) to raw values.
    pub params: HashMap<String, String>,
}

impl Vmt {
    pub fn parse(text: &str) -> Self {
        let mut vmt = Vmt::default();
        let mut depth = 0usize;
        let mut pending_key: Option<String> = None;
        for token in tokens(text) {
            match token {
                Token::Open => {
                    depth += 1;
                    pending_key = None;
                }
                Token::Close => depth = depth.saturating_sub(1),
                Token::Text(s) => {
                    if depth == 0 {
                        if vmt.shader.is_empty() {
                            vmt.shader = s;
                        }
                    } else if depth == 1 {
                        match pending_key.take() {
                            None => pending_key = Some(s.to_ascii_lowercase()),
                            Some(k) => {
                                vmt.params.insert(k, s);
                            }
                        }
                    }
                }
            }
        }
        vmt
    }

    /// The base texture path, `/`-separated, without `.vtf`.
    pub fn base_texture(&self) -> Option<String> {
        let raw = self.params.get("$basetexture")?;
        let path = raw.replace('\\', "/").to_ascii_lowercase();
        Some(path.trim_end_matches(".vtf").to_string())
    }
}

enum Token {
    Open,
    Close,
    Text(String),
}

/// Quoted or bare words, braces, and `//` comments.
fn tokens(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            '{' => {
                chars.next();
                out.push(Token::Open);
            }
            '}' => {
                chars.next();
                out.push(Token::Close);
            }
            '"' => {
                chars.next();
                let s: String = chars.by_ref().take_while(|&c| c != '"').collect();
                out.push(Token::Text(s));
            }
            '/' if text_starts_comment(&mut chars) => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            c if c.is_whitespace() => {
                chars.next();
            }
            _ => {
                let mut s = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || c == '{' || c == '}' || c == '"' {
                        break;
                    }
                    s.push(c);
                    chars.next();
                }
                out.push(Token::Text(s));
            }
        }
    }
    out
}

fn text_starts_comment(chars: &mut std::iter::Peekable<std::str::Chars>) -> bool {
    let mut look = chars.clone();
    look.next();
    look.peek() == Some(&'/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_shader_and_params() {
        let v = Vmt::parse(
            "// comment\n\"LightmappedGeneric\"\n{\n\t\"$basetexture\" \"world\\concrete\\wall_col\"\n\t$surfaceprop concrete\n\t\"Proxies\" { \"x\" \"y\" }\n}\n",
        );
        assert_eq!(v.shader, "LightmappedGeneric");
        assert_eq!(v.base_texture().as_deref(), Some("world/concrete/wall_col"));
        assert_eq!(
            v.params.get("$surfaceprop").map(String::as_str),
            Some("concrete")
        );
        assert!(!v.params.contains_key("x"));
    }
}
