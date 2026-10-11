//! Source's `$selfillum` for lit models (the HCOG sight's glowing glass): the
//! normal lit colour plus base colour × `$selfillummask` × `$selfillumtint`,
//! drawn by our shader `assets/shaders/self_illum.wgsl`.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

pub type SelfIllumMaterial = ExtendedMaterial<StandardMaterial, SelfIllum>;

#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct SelfIllum {
    #[texture(100)]
    #[sampler(101)]
    pub mask: Handle<Image>,
    #[uniform(102)]
    pub tint: Vec4,
}

impl MaterialExtension for SelfIllum {
    fn fragment_shader() -> ShaderRef {
        "shaders/self_illum.wgsl".into()
    }
}

/// `$selfillum`, its mask texture path and tint from a `.vmt`'s text, if the
/// material glows. Read raw: paths use backslashes.
pub fn settings(vmt: &str) -> Option<(String, [f32; 3])> {
    let value = |key: &str| {
        vmt.lines()
            .find(|l| {
                l.trim_start()
                    .trim_start_matches('"')
                    .to_ascii_lowercase()
                    .starts_with(key)
            })
            .and_then(|l| {
                let parts: Vec<&str> = l.split('"').collect();
                parts.get(3).map(|s| s.to_string())
            })
    };
    if value("$selfillum\"").is_none_or(|v| v.trim() != "1") {
        return None;
    }
    let mask = value("$selfillummask\"")?
        .replace('\\', "/")
        .to_ascii_lowercase();
    let tint = value("$selfillumtint\"").map_or([1.0; 3], |t| {
        let n: Vec<f32> = t
            .trim_matches(|c| c == '[' || c == ']' || c == ' ')
            .split_whitespace()
            .filter_map(|x| x.parse().ok())
            .collect();
        [
            n.first().copied().unwrap_or(1.0),
            n.get(1).copied().unwrap_or(1.0),
            n.get(2).copied().unwrap_or(1.0),
        ]
    });
    Some((mask, tint))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_hcog_glow() {
        let vmt = "\"VertexLitGeneric\"\n{\n\t\"$selfillum\" \"1\"\n\t\"$selfillummask\" \"models\\weapons\\attachments\\hcog_ilm\"\n\t\"$selfillumtint\" \"[10 10 10]\"\n}";
        let (mask, tint) = settings(vmt).unwrap();
        assert_eq!(mask, "models/weapons/attachments/hcog_ilm");
        assert_eq!(tint, [10.0; 3]);
        assert!(settings("\"VertexLitGeneric\"\n{\n\t\"$selfillummask\" \"x\"\n}").is_none());
    }
}
