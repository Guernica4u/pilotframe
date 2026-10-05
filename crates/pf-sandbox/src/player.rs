//! The player: feeds actions into `pf_movement::step` every fixed tick, and places
//! the first-person camera every rendered frame.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};
use leafwing_input_manager::prelude::*;
use pf_movement::{MoveInput, MovementState, step};

use crate::input::{self, Action, look};
use crate::map::{MapCollision, SPAWN};
use crate::tuning::{Tuning, TuningAsset};

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        // Bevy note: `FixedUpdate` runs a fixed number of times per second (see
        // TICK_HZ in main.rs), however fast frames are drawn: movement behaves the
        // same at 30 or 240 fps. `Update` runs once per frame, for anything visual.
        // `.chain()` runs the systems in the listed order.
        app.add_systems(Startup, (spawn_player, spawn_hud))
            .add_systems(FixedUpdate, tick_movement)
            .add_systems(Update, (aim, place_camera, update_hud).chain());
    }
}

/// Vertical field of view, degrees. Placeholder until we check the original's.
const FOV_DEGREES: f32 = 60.0;
/// Fall this far and you're put back at the spawn.
const KILL_HEIGHT: f32 = -2000.0;
/// Inches to metres, for the HUD.
const METRES_PER_UNIT: f32 = 0.0254;

#[derive(Component)]
struct Player {
    state: MovementState,
    /// Position at the previous tick. The camera blends between the two so motion
    /// looks smooth even when the screen refreshes faster than the tick rate.
    previous: Vec3,
    /// Radians. Yaw 0 faces -Z; positive turns left. Pitch positive looks up.
    yaw: f32,
    pitch: f32,
    sprint_latched: bool,
}

fn spawn_player(mut commands: Commands) {
    commands.spawn((
        Player {
            state: MovementState::new(SPAWN),
            previous: SPAWN,
            yaw: 0.0,
            pitch: 0.0,
            sprint_latched: false,
        },
        // leafwing adds the matching `ActionState<Action>` for us.
        input::default_bindings(),
        Camera3d::default(),
        // Default near/far planes assume metres; our units are inches.
        Projection::Perspective(PerspectiveProjection {
            fov: FOV_DEGREES.to_radians(),
            near: 1.0,
            far: 20_000.0,
            ..default()
        }),
        Transform::from_translation(SPAWN),
    ));
}

/// Mouse and right stick turn the view every frame, for responsive aim.
fn aim(
    time: Res<Time>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    player: Single<(&mut Player, &ActionState<Action>)>,
) {
    let (mut player, actions) = player.into_inner();
    let mut turn = Vec2::ZERO; // x = yaw, y = pitch
    if input::cursor_captured(&cursor) {
        // Raw mouse deltas: +x is right, +y is down.
        let mouse = actions.axis_pair(&Action::Look);
        turn -= mouse * look::MOUSE_SENSITIVITY;
    }
    let stick = shape_stick(actions.axis_pair(&Action::LookStick), look::STICK_CURVE);
    turn += Vec2::new(
        -stick.x * look::STICK_YAW_SPEED,
        stick.y * look::STICK_PITCH_SPEED,
    ) * time.delta_secs();

    player.yaw = (player.yaw + turn.x).rem_euclid(TAU);
    player.pitch = (player.pitch + turn.y).clamp(-FRAC_PI_2 + 0.01, FRAC_PI_2 - 0.01);
}

/// One simulation tick.
fn tick_movement(
    time: Res<Time>,
    tuning: Res<Tuning>,
    tunings: Res<Assets<TuningAsset>>,
    map: Res<MapCollision>,
    player: Single<(&mut Player, &ActionState<Action>)>,
) {
    // `let ... else` returns early if movement.ron hasn't finished loading.
    let Some(tuning) = tunings.get(&tuning.0) else {
        return;
    };
    let (mut player, actions) = player.into_inner();

    let wish = shape_stick(actions.clamped_axis_pair(&Action::Move), 1.0);
    if actions.just_pressed(&Action::SprintToggle) {
        player.sprint_latched = true;
    }
    if wish.y <= 0.1 {
        player.sprint_latched = false;
    }
    let input = MoveInput {
        wish,
        yaw: player.yaw,
        // leafwing keeps a separate action state for FixedUpdate, so a press is
        // seen by exactly one tick even when a frame runs zero or several ticks.
        jump: actions.just_pressed(&Action::Jump),
        sprint: actions.pressed(&Action::Sprint) || player.sprint_latched,
    };

    // `time` is the fixed clock here, so this is exactly one tick.
    let next = step(&player.state, &input, tuning, &map.0, time.delta_secs());
    player.previous = player.state.position;
    player.state = next;

    if actions.just_pressed(&Action::Reset) || player.state.position.y < KILL_HEIGHT {
        player.state = MovementState::new(SPAWN);
        player.previous = SPAWN;
    }
}

fn place_camera(
    fixed: Res<Time<Fixed>>,
    tuning: Res<Tuning>,
    tunings: Res<Assets<TuningAsset>>,
    player: Single<(&Player, &mut Transform)>,
) {
    let Some(tuning) = tunings.get(&tuning.0) else {
        return;
    };
    let (player, mut transform) = player.into_inner();
    // How far we are between the last tick and the next one, 0..1.
    let blend = fixed.overstep_fraction();
    let feet = player.previous.lerp(player.state.position, blend);
    transform.translation = feet + Vec3::Y * tuning.eye_height;
    transform.rotation = Quat::from_euler(EulerRot::YXZ, player.yaw, player.pitch, 0.0);
}

/// Deadzone, then a response curve. Keeps the direction, reshapes the length.
fn shape_stick(v: Vec2, curve: f32) -> Vec2 {
    let len = v.length();
    if len <= look::STICK_DEADZONE {
        return Vec2::ZERO;
    }
    let scaled = ((len - look::STICK_DEADZONE) / (1.0 - look::STICK_DEADZONE)).min(1.0);
    v / len * scaled.powf(curve)
}

#[derive(Component)]
struct Hud;

fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(10.0),
            left: Val::Px(12.0),
            ..default()
        },
    ));
}

const CONTROLS: &str = "WASD move   mouse look   Space jump   Shift sprint   R reset   Esc free mouse\n\
                        Controller: sticks   A jump   L3 sprint   Back reset\n\
                        Edit assets/movement.ron while playing; it reloads on save.";

fn update_hud(
    player: Single<&Player>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    mut text: Single<&mut Text, With<Hud>>,
) {
    let v = player.state.velocity;
    let speed = Vec2::new(v.x, v.z).length();
    let ground = if player.state.on_ground {
        "on ground"
    } else {
        "in air"
    };
    let hint = if input::cursor_captured(&cursor) {
        CONTROLS
    } else {
        "Click to capture the mouse"
    };
    text.0 = format!(
        "speed {speed:4.0} u/s ({:4.1} m/s)   vertical {:+5.0} u/s   {ground}\n\n{hint}",
        speed * METRES_PER_UNIT,
        v.y,
    );
}
