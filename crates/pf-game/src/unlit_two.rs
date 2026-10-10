//! Respawn's `UnlitTwoTexture` material (the sight reticles), drawn by our
//! shader `assets/shaders/unlit_two_texture.wgsl`: base texture × second
//! texture × colour, each texture through its own 2D transform, which the
//! material's proxies animate ([`crate::matproxy`]).
//!
//! Bevy note: it's a standard material "extended" with our own fragment
//! shader, which replaces the lit output entirely.

use bevy::math::Affine2;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

pub type UnlitTwoTexture = ExtendedMaterial<StandardMaterial, UnlitTwo>;

#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct UnlitTwo {
    /// rgb: `$color` × `$color2`; a: `$alpha`.
    #[uniform(100)]
    pub color: Vec4,
    /// The two 2D transforms, each as two rows of (a, b, translate).
    #[uniform(101)]
    pub base_row0: Vec4,
    #[uniform(102)]
    pub base_row1: Vec4,
    #[uniform(103)]
    pub second_row0: Vec4,
    #[uniform(104)]
    pub second_row1: Vec4,
    #[texture(105)]
    #[sampler(106)]
    pub second: Handle<Image>,
}

/// A 2D transform as the shader's two rows.
fn rows(t: Affine2) -> (Vec4, Vec4) {
    let m = t.matrix2;
    (
        Vec4::new(m.x_axis.x, m.y_axis.x, t.translation.x, 0.0),
        Vec4::new(m.x_axis.y, m.y_axis.y, t.translation.y, 0.0),
    )
}

impl UnlitTwo {
    pub fn new(second: Handle<Image>) -> Self {
        let (r0, r1) = rows(Affine2::IDENTITY);
        Self {
            color: Vec4::ONE,
            base_row0: r0,
            base_row1: r1,
            second_row0: r0,
            second_row1: r1,
            second,
        }
    }

    pub fn set(&mut self, color: Vec3, alpha: f32, base: Affine2, second: Affine2) {
        self.color = color.extend(alpha);
        (self.base_row0, self.base_row1) = rows(base);
        (self.second_row0, self.second_row1) = rows(second);
    }
}

impl MaterialExtension for UnlitTwo {
    fn fragment_shader() -> ShaderRef {
        "shaders/unlit_two_texture.wgsl".into()
    }
}

/// The material for a reticle: `base` and `second` textures, blended or
/// additive.
pub fn material(base: Handle<Image>, second: Handle<Image>, additive: bool) -> UnlitTwoTexture {
    ExtendedMaterial {
        base: StandardMaterial {
            base_color_texture: Some(base),
            unlit: true,
            cull_mode: None,
            alpha_mode: if additive {
                AlphaMode::Add
            } else {
                AlphaMode::Blend
            },
            ..default()
        },
        extension: UnlitTwo::new(second),
    }
}
