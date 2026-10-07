// World surface shader: the original's baked lighting instead of Bevy's lights.
//
//   albedo = $basetexture, mixed with $basetexture2 by vertex alpha (blend materials)
//   light  = sky lightmap (colour x alpha brightness) + sun colour x sun shadow mask x N.L
//   colour = albedo x light
//
// Meshes without a lightmap are drawn fullbright.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::main_pass_post_lighting_processing,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var base_texture2: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var base_sampler2: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var lightmap_sky: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var lightmap_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var lightmap_realtime: texture_2d<f32>;
// x: has texture2, y: has lightmap, z: sky scale, w: sun scale
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var<uniform> settings: vec4<f32>;
// Direction the sunlight travels (Bevy space).
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var<uniform> sun_direction: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(107) var<uniform> sun_color: vec4<f32>;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    var albedo = pbr_input.material.base_color;

    if settings.x > 0.5 {
#ifdef VERTEX_COLORS
        let amount = in.color.a;
#else
        let amount = 0.0;
#endif
        // The standard path multiplied in the vertex colour, which is
        // (1, 1, 1, blend) for us; undo the alpha part.
        let second = textureSample(base_texture2, base_sampler2, in.uv);
        albedo = vec4(mix(albedo.rgb, second.rgb, amount), 1.0);
    }
    pbr_input.material.base_color = albedo;
    albedo = alpha_discard(pbr_input.material, albedo);

    var light = vec3(1.0);
#ifdef VERTEX_UVS_B
    if settings.y > 0.5 && settings.y < 1.5 {
        let sky = textureSample(lightmap_sky, lightmap_sampler, in.uv_b);
        let mask = textureSample(lightmap_realtime, lightmap_sampler, in.uv_b).r;
        // The map's own normals are authored correctly; Source winds triangles
        // the other way from Bevy, so is_front can't be trusted to flip them.
        let n = normalize(in.world_normal);
        let n_dot_l = max(dot(n, -sun_direction.xyz), 0.0);
        light = sky.rgb * sky.a * settings.z + sun_color.rgb * mask * n_dot_l * settings.w;
        // Debug views (PF_LM_DEBUG): 1 = sun mask, 2 = N.L
        if sun_color.w > 0.5 && sun_color.w < 1.5 { light = vec3(mask); }
        if sun_color.w > 1.5 && sun_color.w < 2.5 { light = vec3(n_dot_l); }
    }
#endif
    // Props (y = 2) have no lightmap: flat ambient + sun x N.L, no shadows yet.
    if settings.y > 1.5 {
        let n = normalize(in.world_normal);
        let n_dot_l = max(dot(n, -sun_direction.xyz), 0.0);
        light = vec3(settings.z) + sun_color.rgb * n_dot_l * settings.w;
    }

    var out: FragmentOutput;
    out.color = vec4(albedo.rgb * light, albedo.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
