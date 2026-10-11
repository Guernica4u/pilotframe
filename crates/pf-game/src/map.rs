//! The greybox test map, built in code (ours, not from the game): a place to
//! try every move and, later, every weapon. North of the spawn: crates, a
//! speed lane and wall-run walls; east: ramps and stairs; west: ledges, a
//! tower and a tall wall to hang on; south-west: jump gaps; south: a shooting
//! range with boards at measured distances.
//!
//! Everything is a box, so one list of [`Piece`]s gives both the visible meshes
//! and the collision world. Ramps are tilted slabs whose low end is sunk into the
//! floor. Units are game units (~1 inch), Y up. The player spawns at the origin
//! facing -Z.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use pf_movement::StaticWorld;

use crate::level::LevelEntity;

/// Where the player starts and resets to.
pub const SPAWN: Vec3 = Vec3::new(0.0, 1.0, 0.0);

/// One tile of the measuring grid, units: thick lines every 512, thin every 64.
const GRID_TILE: f32 = 512.0;
/// Grid texture size, pixels per tile.
const GRID_PIXELS: u32 = 256;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Floor,
    Wall,
    Ramp,
    Step,
    Crate,
    Marker,
    Target,
}

impl Kind {
    fn color(self) -> Color {
        match self {
            Kind::Floor => Color::srgb(0.55, 0.55, 0.58),
            Kind::Wall => Color::srgb(0.38, 0.48, 0.68),
            Kind::Ramp => Color::srgb(0.85, 0.55, 0.28),
            Kind::Step => Color::srgb(0.42, 0.66, 0.42),
            Kind::Crate => Color::srgb(0.72, 0.66, 0.50),
            Kind::Marker => Color::srgb(0.92, 0.85, 0.20),
            Kind::Target => Color::srgb(0.85, 0.30, 0.25),
        }
    }
}

struct Piece {
    center: Vec3,
    half: Vec3,
    rotation: Quat,
    kind: Kind,
}

/// An axis-aligned box from corner `min` to corner `max`.
fn block(min: Vec3, max: Vec3, kind: Kind) -> Piece {
    Piece {
        center: (min + max) * 0.5,
        half: (max - min) * 0.5,
        rotation: Quat::IDENTITY,
        kind,
    }
}

/// A ramp `width` wide centred on `x`, rising towards -Z from the floor at `z0`
/// over `run` units at `degrees`, with a platform at the top.
fn ramp(pieces: &mut Vec<Piece>, x: f32, z0: f32, run: f32, degrees: f32, width: f32) {
    const THICKNESS: f32 = 16.0;
    const SUNK: f32 = 32.0; // extra slab length hidden below the floor
    let angle = degrees.to_radians();
    let rise = run * angle.tan();
    // The slab's top face runs from below the floor up to (z0 - run, rise).
    let low = Vec3::new(x, -SUNK * angle.sin(), z0 + SUNK * angle.cos());
    let high = Vec3::new(x, rise, z0 - run);
    let rotation = Quat::from_rotation_x(angle);
    let up = rotation * Vec3::Y;
    pieces.push(Piece {
        center: (low + high) * 0.5 - up * (THICKNESS * 0.5),
        half: Vec3::new(width * 0.5, THICKNESS * 0.5, low.distance(high) * 0.5),
        rotation,
        kind: Kind::Ramp,
    });
    let half_w = width * 0.5;
    pieces.push(block(
        Vec3::new(x - half_w, 0.0, z0 - run - 240.0),
        Vec3::new(x + half_w, rise, z0 - run),
        Kind::Ramp,
    ));
}

/// `count` steps of `rise` each, 32 deep, `width` wide, climbing towards -Z from
/// `z0`, with a landing at the top.
fn stairs(pieces: &mut Vec<Piece>, x: f32, z0: f32, rise: f32, count: usize, width: f32) {
    const DEPTH: f32 = 32.0;
    let back = z0 - DEPTH * count as f32 - 160.0;
    let half_w = width * 0.5;
    for k in 0..count {
        let front = z0 - DEPTH * k as f32;
        let top = rise * (k + 1) as f32;
        pieces.push(block(
            Vec3::new(x - half_w, 0.0, back),
            Vec3::new(x + half_w, top, front),
            Kind::Step,
        ));
    }
}

fn layout() -> Vec<Piece> {
    let mut p = Vec::new();

    // Floor, top at y = 0.
    p.push(block(
        Vec3::new(-4000.0, -16.0, -4000.0),
        Vec3::new(4000.0, 0.0, 4000.0),
        Kind::Floor,
    ));
    // Spawn pad.
    p.push(block(
        Vec3::new(-96.0, 0.0, -96.0),
        Vec3::new(96.0, 1.0, 96.0),
        Kind::Marker,
    ));

    // North (ahead of the spawn): crates to mantle, a speed lane, and walls to
    // wall-run on.
    for (x, z, size) in [
        (-150.0, -650.0, 48.0),
        (60.0, -700.0, 64.0),
        (-40.0, -900.0, 96.0),
        (180.0, -1000.0, 128.0),
    ] {
        let h = size * 0.5;
        p.push(block(
            Vec3::new(x - h, 0.0, z - h),
            Vec3::new(x + h, size, z + h),
            Kind::Crate,
        ));
    }
    // Speed lane: a marker post every 100 units along x = 260.
    for k in 0..14 {
        let z = -100.0 * k as f32;
        p.push(block(
            Vec3::new(258.0, 0.0, z - 2.0),
            Vec3::new(262.0, 40.0, z + 2.0),
            Kind::Marker,
        ));
    }
    // A corridor 300 wide, a lone wall, and a staggered pair for chaining runs.
    p.push(wall_z(-158.0, -1400.0, -2900.0, 320.0));
    p.push(wall_z(158.0, -1400.0, -2900.0, 320.0));
    p.push(wall_z(700.0, -1400.0, -2900.0, 400.0));
    p.push(wall_z(-900.0, -1400.0, -2000.0, 320.0));
    p.push(wall_z(-1300.0, -2100.0, -2700.0, 320.0));
    // A zig-zag of short walls, alternating sides, for wall-run to wall-run.
    for k in 0..5 {
        let x = if k % 2 == 0 { -500.0 } else { -260.0 };
        let z = -3100.0 - 420.0 * k as f32;
        p.push(wall_z(x, z, z - 360.0, 320.0));
    }

    // East: ramps (walkable 15°, 30°, 40°; then around and past the 45° limit
    // in movement.ron) and stairs.
    for (i, degrees) in [15.0, 30.0, 40.0, 45.0, 50.0, 55.0].into_iter().enumerate() {
        ramp(
            &mut p,
            450.0 + 200.0 * i as f32,
            -300.0,
            400.0,
            degrees,
            160.0,
        );
    }
    for (i, rise) in [8.0, 12.0, 16.0, 18.0].into_iter().enumerate() {
        stairs(&mut p, 1100.0 + 200.0 * i as f32, 700.0, rise, 8, 160.0);
    }

    // West: ledges from walk-up (step height 18) to jump, mantle and too high,
    // a tower ringed by walls, and a very tall wall to hang on.
    for (i, height) in [
        12.0, 18.0, 20.0, 24.0, 32.0, 48.0, 64.0, 80.0, 96.0, 112.0, 128.0, 160.0,
    ]
    .into_iter()
    .enumerate()
    {
        let x = -500.0 - 128.0 * i as f32;
        p.push(block(
            Vec3::new(x - 48.0, 0.0, -200.0),
            Vec3::new(x + 48.0, height, -100.0),
            Kind::Step,
        ));
    }
    p.push(block(
        Vec3::new(-2000.0, 0.0, -1100.0),
        Vec3::new(-1616.0, 512.0, -716.0),
        Kind::Wall,
    ));
    p.push(wall_z(-1450.0, -500.0, -1400.0, 448.0));
    p.push(wall_x(-1150.0, -1800.0, -2400.0, 448.0));
    p.push(wall_z(-2700.0, -300.0, -1600.0, 1024.0));

    // South-west: platforms 64 high with growing gaps (128 to 448), to measure
    // how far a jump and a double jump carry.
    let mut x = -400.0;
    for gap in [0.0, 128.0, 192.0, 256.0, 320.0, 384.0, 448.0] {
        x -= gap;
        p.push(block(
            Vec3::new(x - 128.0, 0.0, 600.0),
            Vec3::new(x, 64.0, 728.0),
            Kind::Step,
        ));
        x -= 128.0;
    }

    // South (behind the spawn): a shooting range. Firing line at z = 500,
    // pilot-sized boards (32 × 72) at 256, 512, 1024, 2048 and 3072 units with a
    // distance post beside each row, and a backstop.
    p.push(block(
        Vec3::new(-400.0, 0.0, 496.0),
        Vec3::new(400.0, 2.0, 504.0),
        Kind::Marker,
    ));
    for distance in [256.0, 512.0, 1024.0, 2048.0, 3072.0] {
        let z = 500.0 + distance;
        for x in [-200.0, 0.0, 200.0] {
            p.push(block(
                Vec3::new(x - 16.0, 0.0, z - 2.0),
                Vec3::new(x + 16.0, 72.0, z + 2.0),
                Kind::Target,
            ));
        }
        p.push(block(
            Vec3::new(-424.0, 0.0, z - 4.0),
            Vec3::new(-416.0, 120.0, z + 4.0),
            Kind::Marker,
        ));
    }
    p.push(block(
        Vec3::new(-800.0, 0.0, 3700.0),
        Vec3::new(800.0, 400.0, 3732.0),
        Kind::Wall,
    ));

    p
}

/// A wall 16 thick along Z at `x`, from `z_from` to `z_to`.
fn wall_z(x: f32, z_from: f32, z_to: f32, height: f32) -> Piece {
    block(
        Vec3::new(x - 8.0, 0.0, z_from.min(z_to)),
        Vec3::new(x + 8.0, height, z_from.max(z_to)),
        Kind::Wall,
    )
}

/// A wall 16 thick along X at `z`, from `x_from` to `x_to`.
fn wall_x(z: f32, x_from: f32, x_to: f32, height: f32) -> Piece {
    block(
        Vec3::new(x_from.min(x_to), 0.0, z - 8.0),
        Vec3::new(x_from.max(x_to), height, z + 8.0),
        Kind::Wall,
    )
}

/// The test map's collision, read by the movement tick.
pub fn test_map_collision() -> StaticWorld {
    let mut world = StaticWorld::new();
    for piece in &layout() {
        world.add_box(piece.center, piece.half, piece.rotation);
    }
    world
}

pub fn spawn_test_map(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    commands.insert_resource(ClearColor(Color::srgb(0.62, 0.72, 0.85)));
    // Fill light so faces turned away from the sun aren't black.
    commands.insert_resource(GlobalAmbientLight {
        brightness: 2500.0,
        ..default()
    });
    let grid = images.add(grid_image());
    let mut material_for = std::collections::HashMap::new();

    for piece in layout() {
        let material = material_for
            .entry(piece.kind)
            .or_insert_with(|| {
                materials.add(StandardMaterial {
                    base_color: piece.kind.color(),
                    base_color_texture: Some(grid.clone()),
                    perceptual_roughness: 0.9,
                    ..default()
                })
            })
            .clone();
        // Bevy note: an entity is just an ID; what it *is* comes from the
        // components spawned on it. Mesh + material + transform = a visible object.
        commands.spawn((
            LevelEntity,
            Mesh3d(meshes.add(box_mesh(piece.half))),
            MeshMaterial3d(material),
            Transform::from_translation(piece.center).with_rotation(piece.rotation),
        ));
    }

    commands.spawn((
        LevelEntity,
        DirectionalLight {
            illuminance: 8_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 0.0).looking_to(Vec3::new(-0.4, -1.0, -0.6), Vec3::Y),
        // Default shadow distances assume 1 unit = 1 metre; ours are inches.
        bevy::light::CascadeShadowConfigBuilder {
            first_cascade_far_bound: 600.0,
            maximum_distance: 6000.0,
            ..default()
        }
        .build(),
    ));
}

/// A box mesh whose UVs are in world units, so the grid is the same size on
/// every surface and shows distances and how fast you're moving.
fn box_mesh(half: Vec3) -> Mesh {
    let mut mesh = Mesh::from(Cuboid::from_size(half * 2.0));
    let (
        Some(VertexAttributeValues::Float32x3(positions)),
        Some(VertexAttributeValues::Float32x3(normals)),
    ) = (
        mesh.attribute(Mesh::ATTRIBUTE_POSITION),
        mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
    )
    else {
        return mesh;
    };
    let uvs: Vec<[f32; 2]> = positions
        .iter()
        .zip(normals)
        .map(|(p, n)| {
            let n = Vec3::from(*n).abs();
            let uv = if n.x > 0.5 {
                [p[2], p[1]]
            } else if n.y > 0.5 {
                [p[0], p[2]]
            } else {
                [p[0], p[1]]
            };
            [uv[0] / GRID_TILE, uv[1] / GRID_TILE]
        })
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh
}

/// The measuring grid, tiled: a thin line every 64 units and a thick one every
/// 512, multiplied by each material's colour. It carries its own smaller
/// copies (mipmaps), so distant lines blend to grey instead of shimmering.
fn grid_image() -> Image {
    let n = GRID_PIXELS as usize;
    let minor = n / (GRID_TILE / 64.0) as usize;
    let shade = |x: usize, y: usize| -> u8 {
        let edge = |v: usize| v < 2 || v >= n - 1;
        let line = |v: usize| v.is_multiple_of(minor);
        if edge(x) || edge(y) {
            150
        } else if line(x) || line(y) {
            200
        } else {
            245
        }
    };
    let mut level: Vec<u8> = (0..n * n).map(|i| shade(i % n, i / n)).collect();
    let mut data = Vec::new();
    let mut size = n;
    let mut levels = 0;
    loop {
        data.extend(level.iter().flat_map(|&v| [v, v, v, 255]));
        levels += 1;
        if size == 1 {
            break;
        }
        // Each pixel of the next level averages a 2×2 block of this one.
        let half = size / 2;
        level = (0..half * half)
            .map(|i| {
                let (x, y) = (i % half * 2, i / half * 2);
                let sum: u32 = [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)]
                    .iter()
                    .map(|&(a, b)| level[b * size + a] as u32)
                    .sum();
                (sum / 4) as u8
            })
            .collect();
        size = half;
    }
    let mut image = Image::new(
        Extent3d {
            width: GRID_PIXELS,
            height: GRID_PIXELS,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data[..n * n * 4].to_vec(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    // Bevy note: `Image::new` takes only the full-size level; the smaller ones
    // are appended after it, with the level count set to match.
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        anisotropy_clamp: 16,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

/// The parkour moves, run with the shipped movement.ron on this map's real
/// pieces, so a tuning change that breaks the greybox shows up in `cargo test`.
#[cfg(test)]
mod tests {
    use super::*;
    use pf_movement::{Mode, MoveInput, MovementState, MovementTuning, step};

    const DT: f32 = 1.0 / 60.0;

    fn tuning() -> MovementTuning {
        ron::from_str(include_str!("../assets/movement.ron")).unwrap()
    }

    /// Run `input` for `seconds`, returning the end state and every mode seen.
    fn run(
        world: &StaticWorld,
        mut state: MovementState,
        input: MoveInput,
        seconds: f32,
    ) -> (MovementState, Vec<Mode>) {
        let t = tuning();
        let mut modes = Vec::new();
        for _ in 0..(seconds / DT).round() as usize {
            state = step(&state, &input, &t, world, DT);
            modes.push(state.mode);
        }
        (state, modes)
    }

    fn sprint(yaw: f32) -> MoveInput {
        MoveInput {
            wish: Vec2::Y,
            yaw,
            sprint: true,
            ..default()
        }
    }

    fn jump(input: MoveInput) -> MoveInput {
        MoveInput {
            jump: true,
            ..input
        }
    }

    #[test]
    fn wall_run_then_wall_jump_off_the_corridor_wall() {
        let world = test_map_collision();
        // Inside the corridor, facing down it (-Z), angling into the right wall
        // as you have to: the game only starts a wall-run when you hit the wall.
        let angled = MoveInput {
            wish: Vec2::new(0.3, 1.0).normalize(),
            ..sprint(0.0)
        };
        let (state, _) = run(
            &world,
            MovementState::new(Vec3::new(115.0, 1.0, -1300.0)),
            sprint(0.0),
            0.6,
        );
        let (state, _) = run(&world, state, angled, 0.2);
        let (state, _) = run(&world, state, jump(angled), DT);
        let (state, modes) = run(&world, state, sprint(0.0), 0.3);
        assert!(matches!(state.mode, Mode::WallRun { .. }), "{modes:?}");
        // Jump off while steering at the left wall: the wall jump adds speed in
        // the input direction, so this carries us across.
        let left = MoveInput {
            wish: Vec2::new(-1.0, 0.3),
            ..sprint(0.0)
        };
        let (state, _) = run(&world, state, jump(left), DT);
        // At least 150 away from the wall (input towards it only adds beyond that).
        assert!(state.velocity.x < -140.0, "{state:?}");
        assert!(state.velocity.y > 200.0, "{state:?}");
    }

    #[test]
    fn mantles_the_96_unit_crate() {
        let world = test_map_collision();
        // The 96 crate spans x = -88..8, z = -948..-852. Run at it facing -Z.
        let start = MovementState::new(Vec3::new(-40.0, 1.0, -700.0));
        let (state, _) = run(&world, start, sprint(0.0), 0.4);
        let (state, _) = run(&world, state, jump(sprint(0.0)), DT);
        // An eye-level ledge takes the game's 1 s "level" mantle.
        let (state, modes) = run(&world, state, sprint(0.0), 1.5);
        assert!(
            modes.iter().any(|m| matches!(m, Mode::Mantle { .. })),
            "{modes:?}"
        );
        assert!((state.position.y - 96.0).abs() < 1.0, "{state:?}");
    }
}
