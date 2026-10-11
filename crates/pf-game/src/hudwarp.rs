//! The helmet screen's curve ("hudwarp"), ported from the game's UI renderer
//! (`vguimatsurface.dll`). The game draws the HUD flat, cuts every shape into
//! cells of `hudwarp_chopsize` (60) pixels, and moves each corner as if the
//! screen were wrapped onto a sphere seen from a little way behind its centre.
//! Notes: docs/port/hud.md ("Hudwarp").
//!
//! The helmet screen is a panel in front of the camera, so it covers a bit more
//! than the window (it's seen with a 70° view) and its edges fall off screen.
//!
//! We draw the HUD flat into two images (one for normal textures, one for the
//! game's `$additive` ones) and show them on a mesh whose grid corners are moved
//! by the same formula. Additive textures then add light over the scene as in
//! the game, instead of covering it.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{RenderTarget, ScalingMode};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use crate::level::LevelEntity;

/// The helmet screen's size in its own units.
pub const SCREEN: (f32, f32) = (852.0, 480.0);
/// Pixels per helmet-screen unit in the flat HUD images.
pub const PIXELS_PER_UNIT: f32 = 2.0;
/// The render layer the warped HUD is drawn on.
const LAYER: usize = 2;
/// `hudwarp_chopsize`: the largest piece the game warps as one, pixels.
const CHOP: f32 = 60.0;
/// `Create_PilotHud` puts the helmet screen 208 units in front of the camera,
/// 426 units wide (852 screen units, so 2 per world unit).
pub const PANEL_DISTANCE: f32 = 208.0;
pub const UNITS_PER_WORLD_UNIT: f32 = 2.0;
/// The field of view the helmet screen is seen with: the cockpit's
/// `cockpit_default_fov` (75), in the Source convention (4:3 horizontal).
/// At 16:9 it shows exactly the 426 × 240 screen at 208 units, so the 852 × 480
/// layout fills the window (docs/port/hud.md, "The cockpit and its FOV").
const PANEL_FOV_DEG: f32 = 75.0;

/// The warp's settings (`WarpGlobalSettings`).
#[derive(Clone, Copy, Debug)]
pub struct Warp {
    /// Degrees of sphere arc across and up/down.
    pub x_warp_deg: f32,
    pub y_warp_deg: f32,
    /// Final scale after projecting back to 2D.
    pub x_scale: f32,
    pub y_scale: f32,
    /// Distance back from the sphere's centre the projection is seen from.
    pub view_dist: f32,
}

/// The pilot helmet screen's settings, once it has finished turning on
/// (`Create_PilotHud` and `MainHud_TurnOn` in `cl_main_hud.nut`).
pub const PILOT: Warp = Warp {
    x_warp_deg: 45.0,
    y_warp_deg: 30.0,
    x_scale: 1.3,
    y_scale: 1.1,
    view_dist: 1.0,
};

impl Warp {
    /// Where the game draws the flat point (x, y) of a `w` × `h` screen.
    pub fn point(&self, x: f32, y: f32, w: f32, h: f32) -> (f32, f32) {
        // The game clamps the arcs to 1..89 degrees.
        let xw = self.x_warp_deg.clamp(1.0, 89.0).to_radians();
        let yw = self.y_warp_deg.clamp(1.0, 89.0).to_radians();
        // Across the screen from +arc (left/top) to -arc (right/bottom).
        let ax = xw - 2.0 * xw * x / w;
        let ay = yw - 2.0 * yw * y / h;
        let d = self.view_dist;
        let depth = (ax.cos() * ay.cos() + d) / (d + 1.0);
        (
            (1.0 - self.x_scale * ax.sin() * ay.cos() / (depth * xw.tan())) * 0.5 * w,
            (1.0 - self.y_scale * ay.sin() / (depth * yw.tan())) * 0.5 * h,
        )
    }
}

/// The warped helmet-screen meshes; they move with the cockpit's sway.
#[derive(Component)]
pub struct HudPanel;

/// The two flat HUD layers' UI roots.
pub struct Layers {
    pub normal: Entity,
    pub additive: Entity,
}

/// A grid over the helmet screen in `CHOP`-pixel cells (as the game cuts it),
/// each corner moved by `warp`; positions centred, Y up, UVs flat.
fn warp_mesh(warp: &Warp) -> Mesh {
    let (w, h) = SCREEN;
    let chop = CHOP / PIXELS_PER_UNIT;
    let lines = |extent: f32| -> Vec<f32> {
        let mut v: Vec<f32> = (0..)
            .map(|i| i as f32 * chop)
            .take_while(|&p| p < extent)
            .collect();
        v.push(extent);
        v
    };
    let (xs, ys) = (lines(w), lines(h));
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    for &y in &ys {
        for &x in &xs {
            let (wx, wy) = warp.point(x, y, w, h);
            positions.push([wx - w / 2.0, h / 2.0 - wy, 0.0]);
            uvs.push([x / w, y / h]);
        }
    }
    let row = xs.len() as u32;
    let mut indices = Vec::new();
    for j in 0..ys.len() as u32 - 1 {
        for i in 0..row - 1 {
            let a = j * row + i;
            indices.extend([a, a + row, a + 1, a + 1, a + row, a + row + 1]);
        }
    }
    let normals = vec![[0.0, 0.0, 1.0]; positions.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

/// Set up the two flat HUD images, the cameras that draw UI into them, and the
/// warped mesh that shows them over the game. Returns the UI roots to put HUD
/// elements under.
pub fn spawn(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    warp: &Warp,
) -> Layers {
    let size = (
        (SCREEN.0 * PIXELS_PER_UNIT) as u32,
        (SCREEN.1 * PIXELS_PER_UNIT) as u32,
    );
    let mut layer = |order: isize, alpha_mode: AlphaMode| {
        let image = images.add(Image::new_target_texture(
            size.0,
            size.1,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
        let camera = commands
            .spawn((
                LevelEntity,
                Camera2d,
                Camera {
                    order,
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    ..default()
                },
                RenderTarget::from(image.clone()),
            ))
            .id();
        let root = commands
            .spawn((
                LevelEntity,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                UiTargetCamera(camera),
                Pickable::IGNORE,
            ))
            .id();
        let material = materials.add(StandardMaterial {
            base_color_texture: Some(image),
            unlit: true,
            alpha_mode,
            cull_mode: None,
            ..default()
        });
        (root, material)
    };
    // UI drawn over a clear image leaves colours already multiplied by alpha.
    let (normal, normal_material) = layer(-2, AlphaMode::Premultiplied);
    let (additive, additive_material) = layer(-1, AlphaMode::Add);

    let mesh = meshes.add(warp_mesh(warp));
    // An orthographic camera over the game, the helmet screen fitted to it.
    commands.spawn((
        LevelEntity,
        Camera3d::default(),
        Camera {
            order: 2,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        // How much of the helmet screen fits on screen: its height at that
        // distance and field of view, in screen units.
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: 2.0
                    * PANEL_DISTANCE
                    * ((PANEL_FOV_DEG.to_radians() * 0.5).tan() * 0.75)
                    * UNITS_PER_WORLD_UNIT,
            },
            ..OrthographicProjection::default_3d()
        }),
        Tonemapping::None,
        RenderLayers::layer(LAYER),
        // The window's own UI (the crosshair, debug text) draws over the HUD.
        IsDefaultUiCamera,
        Transform::IDENTITY,
    ));
    for (z, material) in [(-10.0, normal_material), (-9.0, additive_material)] {
        commands.spawn((
            LevelEntity,
            HudPanel,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material),
            RenderLayers::layer(LAYER),
            Transform::from_xyz(0.0, 0.0, z),
        ));
    }
    Layers { normal, additive }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centre_stays_and_edges_bend() {
        let (w, h) = SCREEN;
        let (cx, cy) = PILOT.point(w / 2.0, h / 2.0, w, h);
        assert!((cx - w / 2.0).abs() < 1e-3 && (cy - h / 2.0).abs() < 1e-3);
        // Mirror symmetric.
        let (lx, ly) = PILOT.point(100.0, 50.0, w, h);
        let (rx, ry) = PILOT.point(w - 100.0, h - 50.0, w, h);
        assert!((lx + rx - w).abs() < 1e-3 && (ly + ry - h).abs() < 1e-3);
        // Corners pulled in towards the middle more than the edge centres.
        let (corner_x, _) = PILOT.point(0.0, 0.0, w, h);
        let (edge_x, _) = PILOT.point(0.0, h / 2.0, w, h);
        assert!(corner_x > edge_x, "{corner_x} vs {edge_x}");
    }
}
