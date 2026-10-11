// Source's `$selfillum` on a lit model: the usual lit colour, plus the base
// colour × the self-illumination mask × `$selfillumtint`.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var mask_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var mask_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var<uniform> tint: vec4<f32>;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
#ifdef VERTEX_UVS_A
    let mask = textureSample(mask_texture, mask_sampler, in.uv).r;
#else
    let mask = 0.0;
#endif
    out.color = vec4<f32>(out.color.rgb + pbr_input.material.base_color.rgb * mask * tint.rgb, out.color.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
