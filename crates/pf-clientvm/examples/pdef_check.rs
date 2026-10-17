//! Check the persistence definition we build from the scripts
//! (`pdef::build`) against independent sources in the install, and list
//! every enum with where it came from.
//!
//! - titanDecals: the Titan models' skin families hold the decals in enum
//!   order: Militia/IMC skin `2 + index` and `1 + index + count`
//!   (`_items.nut`), so each decal's two skins must use a texture of that
//!   decal, and the family count must be `1 + 2 * count`.
//!
//! Usage: `cargo run -p pf-clientvm --example pdef_check`.

use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Resources");
    let Some(install) = pf_gamefs::find_install(&root) else {
        eprintln!("no install found under {}", root.display());
        return;
    };
    pf_gamefs::mount_install(&install, None).expect("mount install");
    let (def, problems) = pf_clientvm::pdef::build(&pf_gamefs::read_text);
    for p in &problems {
        println!("PROBLEM: {p}");
    }
    for e in &def.enums {
        let mark = if e.placeholder { " (placeholder)" } else { "" };
        println!(
            "{:24} {:4} items{mark}  from {}",
            e.name,
            e.items.len(),
            e.source
        );
    }

    let Some(decals) = def.find_enum("titanDecals") else {
        return;
    };
    let count = decals.items.len();
    for model in [
        "models/titans/atlas/atlas_titan.mdl",
        "models/titans/ogre/ogre_titan.mdl",
        "models/titans/stryder/stryder_titan.mdl",
    ] {
        let Some(mdl) = pf_gamefs::read(model) else {
            println!("{model}: missing");
            continue;
        };
        let families = pf_fmt_mdl::skin_families(&mdl).expect("skins");
        let base = &families[0];
        // The decal texture a family adds (a slot that differs from skin 0,
        // or the base decal itself).
        let decal_of = |f: usize| -> Option<String> {
            let fam = families.get(f)?;
            fam.iter()
                .zip(base)
                .filter(|(t, b)| t != b || t.contains("decal"))
                .map(|(t, _)| t.rsplit('/').next().unwrap_or(t).to_string())
                .find(|t| t.to_lowercase().contains("decal"))
        };
        let (mut by_name, mut same_pair) = (0, 0);
        for (i, item) in decals.items.iter().enumerate().skip(1) {
            let (imc, mcor) = (decal_of(2 + i), decal_of(1 + i + count));
            if imc.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(item)) {
                by_name += 1;
            } else {
                println!("  {model}: decal {i} {item}: skin uses {imc:?}");
            }
            if imc.is_some() && imc == mcor {
                same_pair += 1;
            }
        }
        println!(
            "{model}: {} families (expected {}), {by_name}/{} decals match by name, {same_pair} with the same texture on both teams",
            families.len(),
            1 + 2 * count,
            count - 1
        );
    }
}
