// A HUD progress bar drawn as a circle sector (`CHudProgressBar` with
// `CircularEnabled`): the texture shows only within `progress` x 360 degrees of
// 12 o'clock, turning clockwise or counter-clockwise. Angles are measured in
// the texture's own square space, as the game's wedges are. See docs/port/hud.md.

#import bevy_ui::ui_vertex_output::UiVertexOutput

@group(1) @binding(0) var<uniform> color: vec4<f32>;
// x: progress (0..1), y: 1 for clockwise.
@group(1) @binding(1) var<uniform> params: vec4<f32>;
@group(1) @binding(2) var bar_texture: texture_2d<f32>;
@group(1) @binding(3) var bar_sampler: sampler;

const TAU: f32 = 6.2831853;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let d = in.uv - vec2<f32>(0.5, 0.5);
    // From 12 o'clock, clockwise on screen (texture y points down).
    var angle = atan2(d.x, -d.y);
    if angle < 0.0 {
        angle += TAU;
    }
    if params.y < 0.5 {
        angle = TAU - angle;
    }
    if angle >= params.x * TAU {
        discard;
    }
    return textureSample(bar_texture, bar_sampler, in.uv) * color;
}
