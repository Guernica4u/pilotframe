//! Circular HUD progress bars (`CHudProgressBar` with `CircularEnabled`), as
//! the game paints them (`client.dll`, notes in docs/hud.md): the bar's image
//! is cut to a circle sector starting at 12 o'clock and covering progress ×
//! 360°, clockwise or counter-clockwise (`CircularClockwise`).
//!
//! Bevy note: a `UiMaterial` is a UI node drawn by our own shader
//! (`assets/shaders/hud_circular_bar.wgsl`); its fields become the shader's
//! inputs.

use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

#[derive(AsBindGroup, Asset, TypePath, Debug, Clone)]
pub struct CircularBar {
    #[uniform(0)]
    pub color: LinearRgba,
    /// x: progress (0..1), y: 1 for clockwise.
    #[uniform(1)]
    pub params: Vec4,
    #[texture(2)]
    #[sampler(3)]
    pub texture: Handle<Image>,
}

impl CircularBar {
    pub fn new(texture: Handle<Image>, color: Color, clockwise: bool, progress: f32) -> Self {
        Self {
            color: color.into(),
            params: Vec4::new(progress, if clockwise { 1.0 } else { 0.0 }, 0.0, 0.0),
            texture,
        }
    }

    pub fn set_progress(&mut self, progress: f32) {
        self.params.x = progress.clamp(0.0, 1.0);
    }
}

impl UiMaterial for CircularBar {
    fn fragment_shader() -> ShaderRef {
        "shaders/hud_circular_bar.wgsl".into()
    }
}
