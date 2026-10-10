// Respawn's UnlitTwoTexture as the sight reticles use it: the base texture
// times a second texture (each through its own 2D transform), times the
// colour (`$color` × `$color2`), with alpha from both textures and `$alpha`.
// See docs/weapons.md, "The HCOG reticle".

#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_bindings,
}

// rgb: $color × $color2; a: $alpha.
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> color: vec4<f32>;
// Each 2D transform as two rows of (a, b, translate).
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<uniform> base_row0: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var<uniform> base_row1: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var<uniform> second_row0: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var<uniform> second_row1: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var second_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var second_sampler: sampler;

fn apply(row0: vec4<f32>, row1: vec4<f32>, uv: vec2<f32>) -> vec2<f32> {
    let p = vec3<f32>(uv, 1.0);
    return vec2<f32>(dot(row0.xyz, p), dot(row1.xyz, p));
}

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    var out: FragmentOutput;
#ifdef VERTEX_UVS_A
    let uv = in.uv;
#else
    let uv = vec2<f32>(0.0);
#endif
    let a = textureSample(pbr_bindings::base_color_texture, pbr_bindings::base_color_sampler, apply(base_row0, base_row1, uv));
    let b = textureSample(second_texture, second_sampler, apply(second_row0, second_row1, uv));
    out.color = vec4<f32>(a.rgb * b.rgb * color.rgb, a.a * b.a * color.a);
    return out;
}
