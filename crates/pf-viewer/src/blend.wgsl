// Two-texture blend for Source "WorldVertexTransition" materials (grass/dirt,
// rock/mud...). The vertex colour's alpha picks how much of the second texture
// shows: 0 = only $basetexture, 1 = only $basetexture2.
//
// Built on Bevy's standard PBR fragment path; only the base colour changes.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var base_texture2: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var base_sampler2: sampler;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

#ifdef VERTEX_COLORS
    let amount = in.color.a;
#else
    let amount = 0.0;
#endif
    // The standard path multiplied base colour by the vertex colour (alpha
    // included). Our vertex colour is (1, 1, 1, blend), so undo the alpha.
    let first = vec4(pbr_input.material.base_color.rgb, 1.0);
    let second = textureSample(base_texture2, base_sampler2, in.uv);
    pbr_input.material.base_color = vec4(mix(first.rgb, second.rgb, amount), 1.0);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
