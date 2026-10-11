//! Print where a `.res` layout puts its elements (following `#base` includes).
//!
//! `cargo run -p pf-fmt-ui --example res_layout -- <file.res> <wide> <tall> [name...]`

use pf_fmt_ui::{kv, layout};
use std::path::Path;

fn load(path: &Path) -> kv::Node {
    let text = std::fs::read_to_string(path).expect("read .res");
    let file = kv::parse(&text);
    let mut root = file.nodes.into_iter().next().expect("a root block");
    // Bases go underneath: start from the base, lay this file over it.
    for base in file.bases.iter().rev() {
        let mut merged = load(&path.with_file_name(base));
        merged.merge(&root);
        root = merged;
    }
    root
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = load(Path::new(&args[0]));
    let screen = (args[1].parse().unwrap(), args[2].parse().unwrap());
    let rects = layout::layout(&root, screen, |_| None);
    let wanted: Vec<String> = args[3..].iter().map(|s| s.to_ascii_lowercase()).collect();
    let mut names: Vec<_> = rects.keys().collect();
    names.sort();
    for name in names {
        if wanted.is_empty() || wanted.contains(name) {
            let r = rects[name];
            println!(
                "{name:28} x {:6.1} y {:6.1} w {:5.1} h {:5.1}",
                r.x, r.y, r.w, r.h
            );
        }
    }
}
