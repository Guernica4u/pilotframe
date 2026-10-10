//! The player: feeds actions into `pf_movement::step` every fixed tick, and places
//! the first-person camera every rendered frame.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};
use leafwing_input_manager::prelude::*;
use pf_movement::{Mode, MoveInput, MovementState, step};

use crate::Screen;
use crate::input::{self, Action, look};
use crate::level::{ChosenMap, LevelEntity, MapCollision, SpawnPoint};
use crate::menu::playing;
use crate::settings::Settings;
use crate::tuning::{Tuning, TuningAsset};
use crate::view;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        // Bevy note: `FixedUpdate` runs a fixed number of times per second (see
        // TICK_HZ in main.rs), however fast frames are drawn: movement behaves the
        // same at 30 or 240 fps. `Update` runs once per frame, for anything visual.
        // `.chain()` runs the systems in the listed order.
        app.add_systems(OnEnter(Screen::InGame), (spawn_player, spawn_hud))
            .add_systems(FixedUpdate, tick_movement.run_if(playing))
            .add_systems(
                Update,
                (
                    aim.run_if(playing),
                    (place_camera, update_hud, toggle_debug_text, apply_settings)
                        .run_if(in_state(Screen::InGame)),
                )
                    .chain(),
            );
    }
}

/// Fall this far below the spawn and you're put back at it.
const KILL_DEPTH: f32 = 4000.0;
/// Inches to metres, for the HUD.
const METRES_PER_UNIT: f32 = 0.0254;
/// How far the view tilts away from the wall during a wall-run, degrees
/// (`wallrun_maxViewTilt`).
const WALLRUN_ROLL_DEG: f32 = 15.0;
/// How quickly the wall-run tilt follows, per second (`wallrun_viewTiltSpeed`).
const WALLRUN_TILT_SPEED: f32 = 6.0;
/// How quickly the eye height catches up when crouching, per second.
const CROUCH_EASE: f32 = 12.0;
/// The landing kick, from the pilot settings: full strength for a fall from
/// `viewkickFallDistMin` (10 ft), growing to `viewkickFallDistMaxScale` (12) times
/// that from `viewkickFallDistMax` (70 ft); weaker below the minimum. Feet.
const LAND_KICK_FALL_MIN_FT: f32 = 10.0;
const LAND_KICK_FALL_MAX_FT: f32 = 70.0;
const LAND_KICK_MAX_SCALE: f32 = 12.0;
/// Most roll when turning while sprinting, degrees (pilot `sprinttiltMaxRoll`).
const SPRINT_TILT_MAX_ROLL: f32 = 2.0;

#[derive(Component)]
pub struct Player {
    state: MovementState,
    /// Position at the previous tick. The camera blends between the two so motion
    /// looks smooth even when the screen refreshes faster than the tick rate.
    previous: Vec3,
    /// Radians. Yaw 0 faces -Z; positive turns left. Pitch positive looks up.
    yaw: f32,
    pitch: f32,
    sprint_latched: bool,
    /// Smoothed camera height above the feet and wall-run tilt (radians).
    eye: f32,
    roll: f32,
    /// View punches and sprint tilt.
    view: view::ViewEffects,
    /// Yaw at the last frame, to measure how fast we're turning.
    last_yaw: f32,
    /// Movement keys (W, A, S, D as bits) held at the last tick, to spot new presses.
    keys: u8,
}

fn spawn_player(
    mut commands: Commands,
    spawn: Res<SpawnPoint>,
    settings: Res<Settings>,
    chosen: Res<ChosenMap>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let mut player = commands.spawn((
        LevelEntity,
        Player {
            state: MovementState::new(spawn.feet),
            previous: spawn.feet,
            yaw: spawn.yaw,
            pitch: 0.0,
            sprint_latched: false,
            eye: pf_world::EYE_HEIGHT,
            roll: 0.0,
            view: default(),
            last_yaw: spawn.yaw,
            keys: 0,
        },
        // leafwing adds the matching `ActionState<Action>` for us.
        input::default_bindings(),
        Camera3d::default(),
        // Default near/far planes assume metres; our units are inches.
        Projection::Perspective(PerspectiveProjection {
            fov: view::vertical_fov(settings.fov),
            near: 1.0,
            far: 100_000.0,
            ..default()
        }),
        Transform::from_translation(spawn.feet),
    ));
    if *chosen == ChosenMap::Colony {
        player.insert(pf_world::colony_fog());
    }
    let camera = player.id();
    crate::viewmodel::spawn(
        &mut commands,
        camera,
        &mut meshes,
        &mut materials,
        &mut images,
    );
}

/// Settings can change in the pause menu; keep the camera in step.
fn apply_settings(settings: Res<Settings>, mut projection: Single<&mut Projection, With<Player>>) {
    if !settings.is_changed() {
        return;
    }
    if let Projection::Perspective(p) = projection.as_mut() {
        p.fov = view::vertical_fov(settings.fov);
    }
}

/// Mouse and right stick turn the view every frame, for responsive aim.
fn aim(
    time: Res<Time>,
    settings: Res<Settings>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    player: Single<(&mut Player, &ActionState<Action>)>,
) {
    let (mut player, actions) = player.into_inner();
    let mut turn = Vec2::ZERO; // x = yaw, y = pitch
    if input::cursor_captured(&cursor) {
        // Raw mouse deltas: +x is right, +y is down.
        let mouse = actions.axis_pair(&Action::Look);
        turn -= mouse * look::MOUSE_SENSITIVITY * settings.mouse_sensitivity;
    }
    let stick = shape_stick(actions.axis_pair(&Action::LookStick), look::STICK_CURVE);
    turn += Vec2::new(
        -stick.x * look::STICK_YAW_SPEED,
        stick.y * look::STICK_PITCH_SPEED,
    ) * time.delta_secs();
    // Dev: `PF_DEV_TURN=<degrees per second>` turns the view by itself (to see
    // turn-dependent effects in screenshots).
    if let Some(rate) = std::env::var("PF_DEV_TURN")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
    {
        turn.x += rate.to_radians() * time.delta_secs();
    }

    player.yaw = (player.yaw + turn.x).rem_euclid(TAU);
    player.pitch = (player.pitch + turn.y).clamp(-FRAC_PI_2 + 0.01, FRAC_PI_2 - 0.01);
}

/// One simulation tick.
fn tick_movement(
    time: Res<Time>,
    tuning: Res<Tuning>,
    tunings: Res<Assets<TuningAsset>>,
    map: Res<MapCollision>,
    spawn: Res<SpawnPoint>,
    keyboard: Res<ButtonInput<KeyCode>>,
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
        crouch: actions.pressed(&Action::Crouch),
        zoom: actions.pressed(&Action::Zoom),
        pitch: player.pitch,
        key_pressed: false,
    };
    // A newly pressed movement key (keyboard only) can turn a fresh jump.
    let keys = [KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD]
        .iter()
        .enumerate()
        .fold(0u8, |bits, (i, k)| {
            bits | (u8::from(keyboard.pressed(*k)) << i)
        });
    let input = MoveInput {
        key_pressed: keys & !player.keys != 0,
        ..input
    };
    player.keys = keys;

    // `time` is the fixed clock here, so this is exactly one tick.
    let next = step(&player.state, &input, tuning, &map.0, time.delta_secs());
    kick_view(&mut player, &next, tuning.gravity);
    // Ducking or standing in the air moves the feet, not the head: shift the
    // eased eye height and the blend start by the same amount so the view holds still.
    let airborne = !player.state.on_ground && player.state.mode == Mode::Free;
    let tuck = tuning.capsule_height - tuning.crouch_height;
    let shift = match (player.state.crouched, next.crouched) {
        (false, true) if airborne => tuck,
        (true, false) if airborne => -tuck,
        _ => 0.0,
    };
    player.eye -= shift;
    player.previous = player.state.position + Vec3::Y * shift;
    player.state = next;

    if actions.just_pressed(&Action::Reset) || player.state.position.y < spawn.feet.y - KILL_DEPTH {
        player.state = MovementState::new(spawn.feet);
        player.previous = spawn.feet;
        player.yaw = spawn.yaw;
    }
}

impl Player {
    /// The movement state after the latest tick.
    pub fn movement(&self) -> &MovementState {
        &self.state
    }

    /// The view's (yaw, pitch) in radians: yaw 0 faces -Z and turns left,
    /// pitch looks up.
    pub fn view_angles(&self) -> (f32, f32) {
        (self.yaw, self.pitch)
    }

    /// The view punch and weapon kick springs.
    pub fn view_effects(&mut self) -> &mut view::ViewEffects {
        &mut self.view
    }

    /// Move the aim itself by (pitch down, yaw left) degrees, the game's
    /// convention (recoil's permanent kick).
    pub fn add_aim(&mut self, pitch_down: f32, yaw_left: f32) {
        self.yaw = (self.yaw + yaw_left.to_radians()).rem_euclid(TAU);
        self.pitch =
            (self.pitch - pitch_down.to_radians()).clamp(-FRAC_PI_2 + 0.01, FRAC_PI_2 - 0.01);
    }

    /// How far the gun is kicked relative to the view (radians: pitch up, yaw
    /// left, roll left).
    pub fn weapon_kick(&self) -> Vec3 {
        self.view.weapon_offsets()
    }
}

/// View punches for what just happened this tick.
fn kick_view(player: &mut Player, next: &MovementState, gravity: f32) {
    let before = player.state;
    let right = Quat::from_rotation_y(player.yaw) * Vec3::X;
    // How much a wall is to one side: -1 straight to the right, 1 to the left.
    let side = |normal: Vec3| normal.dot(right);
    let view = &mut player.view;
    if let Mode::WallRun { normal, .. } = next.mode
        && !matches!(before.mode, Mode::WallRun { .. })
    {
        // The game scales the wall-run kick by how side-on the wall is, and
        // leaves the gun's roll out of it.
        view.punch(&view::WALLRUN, side(normal), 1.0, false);
    }
    if let Mode::WallHang { normal, .. } = next.mode
        && !matches!(before.mode, Mode::WallHang { .. })
    {
        view.punch(&view::WALLHANG, side(normal).signum(), 1.0, true);
    }
    if next.air_jumps > before.air_jumps {
        view.punch(&view::DOUBLE_JUMP, 1.0, 1.0, true);
    } else if before.on_ground && !next.on_ground && next.velocity.y > 1.0 {
        view.punch(&view::JUMP, 1.0, 1.0, true);
    }
    if next.on_ground && !before.on_ground && before.velocity.y < 0.0 {
        view.punch(
            &view::LAND,
            1.0,
            land_kick_scale(-before.velocity.y, gravity),
            true,
        );
    }
}

/// How hard a landing at `speed` (units/s, downwards) kicks the view: the game
/// turns the pilot's fall distances (feet) into landing speeds with v² = 2gh.
fn land_kick_scale(speed: f32, gravity: f32) -> f32 {
    let at = |feet: f32| (2.0 * gravity * 12.0 * feet).sqrt();
    let (min, max) = (at(LAND_KICK_FALL_MIN_FT), at(LAND_KICK_FALL_MAX_FT));
    if speed < min {
        speed / min
    } else if speed <= max {
        1.0 + (speed - min) / (max - min) * (LAND_KICK_MAX_SCALE - 1.0)
    } else {
        LAND_KICK_MAX_SCALE
    }
}

pub fn place_camera(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    tuning: Res<Tuning>,
    tunings: Res<Assets<TuningAsset>>,
    player: Single<(&mut Player, &mut Transform)>,
) {
    let Some(tuning) = tunings.get(&tuning.0) else {
        return;
    };
    let (mut player, mut transform) = player.into_inner();
    // How far we are between the last tick and the next one, 0..1.
    let blend = fixed.overstep_fraction();
    let feet = player.previous.lerp(player.state.position, blend);

    // Ease the eye height and tilt instead of snapping them. `1 - e^(-k·dt)` gives
    // the same feel at any frame rate.
    let dt = time.delta_secs();
    let eye = tuning.eye(player.state.crouched);
    player.eye += (eye - player.eye) * (1.0 - (-CROUCH_EASE * dt).exp());
    // Wall on the right: tilt left, and the other way round.
    let roll = match player.state.mode {
        Mode::WallRun { normal, .. } => {
            let right = Quat::from_rotation_y(player.yaw) * Vec3::X;
            -normal.dot(right) * WALLRUN_ROLL_DEG.to_radians()
        }
        _ => 0.0,
    };
    player.roll += (roll - player.roll) * (1.0 - (-WALLRUN_TILT_SPEED * dt).exp());

    // Punch springs and sprint tilt.
    let turn_rate = if dt > 0.0 {
        let mut d = player.yaw - player.last_yaw;
        // Yaw wraps at a full turn; take the short way round.
        d = (d + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI;
        d.to_degrees() / dt
    } else {
        0.0
    };
    player.last_yaw = player.yaw;
    let v = player.state.velocity;
    let sprinting =
        player.state.on_ground && Vec2::new(v.x, v.z).length() > tuning.walk_speed * 1.05;
    player.view.update(dt, turn_rate, sprinting);
    let kick = player.view.offsets(SPRINT_TILT_MAX_ROLL);

    transform.translation = feet + Vec3::Y * player.eye;
    transform.rotation = Quat::from_euler(
        EulerRot::YXZ,
        player.yaw + kick.y,
        player.pitch + kick.x,
        player.roll + kick.z,
    );
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
        LevelEntity,
        Hud,
        Visibility::Hidden,
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

const CONTROLS: &str = "WASD move   mouse look   Space jump (again in the air)   Shift sprint   Ctrl/C crouch   Right mouse aim (wall-hang)   Left mouse fire   R reload   Backspace reset   Esc menu\n\
                        Controller: sticks   A jump   L3 sprint   B crouch   LT aim   RT fire   X reload   Back reset\n\
                        Jump alongside a wall to wall-run. Jump at a ledge holding forward to mantle.   F3 hides this.";

/// F3 shows or hides the debug text (speed, movement mode, controls).
fn toggle_debug_text(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut text: Single<&mut Visibility, With<Hud>>,
) {
    if keyboard.just_pressed(KeyCode::F3) {
        **text = match **text {
            Visibility::Hidden => Visibility::Inherited,
            _ => Visibility::Hidden,
        };
    }
}

fn update_hud(
    player: Single<&Player>,
    weapon: Option<Single<&crate::viewmodel::Viewmodel>>,
    mut text: Single<&mut Text, With<Hud>>,
) {
    let v = player.state.velocity;
    let speed = Vec2::new(v.x, v.z).length();
    let ground = match player.state.mode {
        Mode::WallRun { time, .. } => format!("wall-run {time:.2}s"),
        Mode::WallHang { time, .. } => format!("wall-hang {time:.2}s"),
        Mode::Mantle { .. } => "mantle".into(),
        Mode::Free if player.state.on_ground => "on ground".into(),
        Mode::Free => "in air".into(),
    };
    let ammo = weapon.map_or(String::new(), |w| format!("   ammo {}", w.ammo()));
    text.0 = format!(
        "speed {speed:4.0} u/s ({:4.1} m/s)   vertical {:+5.0} u/s   {ground}{ammo}\n\n{CONTROLS}",
        speed * METRES_PER_UNIT,
        v.y,
    );
}
