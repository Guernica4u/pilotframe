//! Behaviour tests: run the real `step` function tick by tick in small hand-built
//! worlds. No Bevy, no game data.

use glam::{Quat, Vec2, Vec3};
use pf_movement::{Mode, MoveInput, MovementState, MovementTuning, StaticWorld, step};

const DT: f32 = 1.0 / 60.0;
const RADIUS: f32 = 16.0;

/// Test values, not the sandbox's placeholders, so tests don't break when tuning
/// changes. Written as RON to also check the format parses.
fn tuning() -> MovementTuning {
    ron::from_str(
        "(
            capsule_radius: 16.0, capsule_height: 72.0, eye_height: 64.0,
            crouch_height: 48.0, crouch_eye_height: 40.0,
            walk_speed: 200.0, sprint_speed: 300.0, crouch_speed: 90.0,
            ground_accel: 2000.0, ground_friction: 2000.0, air_accel: 400.0,
            gravity: 800.0, jump_height: 50.0, double_jump_height: 40.0,
            wallrun_speed: 350.0, wallrun_accel: 500.0, wallrun_min_speed: 120.0,
            wallrun_gravity: 100.0, wallrun_max_lift: 150.0, wallrun_max_fall_speed: 500.0,
            wallrun_time_limit: 1.5, wall_jump_height: 40.0, wall_jump_push: 250.0,
            wall_hang_time_limit: 3.0, mantle_reach: 80.0, mantle_time: 0.3,
            max_slope_deg: 45.0, step_height: 18.0,
        )",
    )
    .unwrap()
}

/// Flat floor with its top at y = 0.
fn floor() -> StaticWorld {
    let mut world = StaticWorld::new();
    world.add_box(
        Vec3::new(0.0, -10.0, 0.0),
        Vec3::new(5000.0, 10.0, 5000.0),
        Quat::IDENTITY,
    );
    world
}

/// Box from `min` to `max` corners.
fn add_block(world: &mut StaticWorld, min: Vec3, max: Vec3) {
    world.add_box((min + max) * 0.5, (max - min) * 0.5, Quat::IDENTITY);
}

/// A wedge rising towards -Z, starting at `z_start`, `run` long, at `degrees`.
fn add_ramp(world: &mut StaticWorld, z_start: f32, run: f32, degrees: f32) {
    let rise = run * degrees.to_radians().tan();
    let z_end = z_start - run;
    let pts = [
        Vec3::new(-300.0, 0.0, z_start),
        Vec3::new(300.0, 0.0, z_start),
        Vec3::new(-300.0, 0.0, z_end),
        Vec3::new(300.0, 0.0, z_end),
        Vec3::new(-300.0, rise, z_end),
        Vec3::new(300.0, rise, z_end),
    ];
    assert!(world.add_convex(&pts));
}

const FORWARD: MoveInput = MoveInput {
    wish: Vec2::new(0.0, 1.0),
    yaw: 0.0,
    jump: false,
    sprint: false,
    crouch: false,
};

/// Run `seconds` of the same input, calling `each` after every tick.
fn run(
    world: &StaticWorld,
    mut state: MovementState,
    input: MoveInput,
    seconds: f32,
    mut each: impl FnMut(&MovementState),
) -> MovementState {
    let t = tuning();
    for _ in 0..(seconds / DT).round() as usize {
        state = step(&state, &input, &t, world, DT);
        each(&state);
    }
    state
}

fn settle(world: &StaticWorld, at: Vec3) -> MovementState {
    let state = run(
        world,
        MovementState::new(at),
        MoveInput::default(),
        0.5,
        |_| {},
    );
    assert!(state.on_ground, "didn't land: {state:?}");
    state
}

fn speed(state: &MovementState) -> f32 {
    Vec2::new(state.velocity.x, state.velocity.z).length()
}

// ---- walk / sprint ----

#[test]
fn walk_and_sprint_reach_their_top_speeds() {
    let world = floor();
    let start = settle(&world, Vec3::ZERO);

    let walked = run(&world, start, FORWARD, 1.0, |_| {});
    assert!((speed(&walked) - 200.0).abs() < 0.01, "{walked:?}");

    let sprinted = run(
        &world,
        start,
        MoveInput {
            sprint: true,
            ..FORWARD
        },
        1.0,
        |_| {},
    );
    assert!((speed(&sprinted) - 300.0).abs() < 0.01, "{sprinted:?}");

    // Sprint only counts going forwards.
    let back = MoveInput {
        wish: Vec2::new(0.0, -1.0),
        sprint: true,
        ..FORWARD
    };
    let backed = run(&world, start, back, 1.0, |_| {});
    assert!((speed(&backed) - 200.0).abs() < 0.01, "{backed:?}");
}

#[test]
fn friction_stops_you() {
    let world = floor();
    let moving = run(&world, settle(&world, Vec3::ZERO), FORWARD, 1.0, |_| {});
    let stopped = run(&world, moving, MoveInput::default(), 0.5, |_| {});
    assert_eq!(stopped.velocity, Vec3::ZERO);
}

#[test]
fn yaw_turns_the_wish_direction() {
    let world = floor();
    let left = MoveInput {
        yaw: std::f32::consts::FRAC_PI_2,
        ..FORWARD
    };
    let state = run(&world, settle(&world, Vec3::ZERO), left, 1.0, |_| {});
    // Facing -Z and turning left by 90° faces -X.
    assert!(
        state.position.x < -100.0 && state.position.z.abs() < 1e-3,
        "{state:?}"
    );
}

// ---- jump / gravity ----

fn jump_apex(dt: f32) -> f32 {
    let world = floor();
    let t = tuning();
    let mut state = settle(&world, Vec3::ZERO);
    let ground_y = state.position.y;
    state = step(
        &state,
        &MoveInput {
            jump: true,
            ..Default::default()
        },
        &t,
        &world,
        dt,
    );
    let mut apex = state.position.y;
    for _ in 0..(2.0 / dt) as usize {
        state = step(&state, &MoveInput::default(), &t, &world, dt);
        apex = apex.max(state.position.y);
    }
    assert!(state.on_ground, "should land again: {state:?}");
    assert!((state.position.y - ground_y).abs() < 0.2, "{state:?}");
    apex - ground_y
}

#[test]
fn jump_apex_height_matches_tuning() {
    let apex = jump_apex(DT);
    assert!((apex - 50.0).abs() < 0.2, "apex {apex}");
}

#[test]
fn jump_height_does_not_depend_on_tick_rate() {
    for hz in [30.0, 60.0, 120.0, 144.0] {
        let apex = jump_apex(1.0 / hz);
        assert!((apex - 50.0).abs() < 0.2, "{hz} Hz: apex {apex}");
    }
}

#[test]
fn falls_under_gravity() {
    let world = floor();
    let t = tuning();
    let mut state = MovementState::new(Vec3::new(0.0, 500.0, 0.0));
    for _ in 0..30 {
        state = step(&state, &MoveInput::default(), &t, &world, DT);
    }
    // After 0.5 s: fallen g t² / 2 = 100 units, moving g t = 400 units/s.
    assert!((state.position.y - 400.0).abs() < 0.01, "{state:?}");
    assert!((state.velocity.y + 400.0).abs() < 0.01, "{state:?}");
    assert!(!state.on_ground);
}

#[test]
fn one_double_jump_per_time_in_the_air() {
    let world = floor();
    let t = tuning();
    let jump = MoveInput {
        jump: true,
        ..Default::default()
    };
    let mut state = MovementState::new(Vec3::new(0.0, 500.0, 0.0));
    state = step(&state, &MoveInput::default(), &t, &world, DT);
    assert!(state.velocity.y < 0.0);
    // The mid-air jump replaces the fall speed with a fresh upward one.
    state = step(&state, &jump, &t, &world, DT);
    let lift = (2.0 * 800.0 * 40.0f32).sqrt();
    assert!((state.velocity.y - lift).abs() < 20.0, "{state:?}");
    // A second one does nothing.
    state = step(&state, &jump, &t, &world, DT);
    assert!(state.velocity.y < lift - 5.0, "{state:?}");
    // Landing gives it back.
    state = run(&world, state, MoveInput::default(), 3.0, |_| {});
    assert!(state.on_ground && state.air_jumps == 0, "{state:?}");
}

// ---- walls ----

/// A thin wall whose front face is at z = -199.
fn wall() -> StaticWorld {
    let mut world = floor();
    add_block(
        &mut world,
        Vec3::new(-500.0, 0.0, -201.0),
        Vec3::new(500.0, 300.0, -199.0),
    );
    world
}
const WALL_LIMIT: f32 = -199.0 + RADIUS;

#[test]
fn walking_into_a_wall_stops_at_it() {
    let world = wall();
    let state = run(
        &world,
        settle(&world, Vec3::ZERO),
        MoveInput {
            sprint: true,
            ..FORWARD
        },
        3.0,
        |s| {
            assert!(s.position.z >= WALL_LIMIT, "went into the wall: {s:?}");
        },
    );
    assert!(
        state.position.z < WALL_LIMIT + 1.0,
        "should be touching the wall: {state:?}"
    );
}

#[test]
fn very_fast_movement_cannot_tunnel_through_a_wall() {
    let world = wall();
    // 50,000 units/s covers 833 units in one tick, far past the 2-unit-thick wall.
    let mut state = MovementState::new(Vec3::new(0.0, 50.0, 0.0));
    state.velocity = Vec3::new(0.0, 0.0, -50_000.0);
    let state = step(&state, &MoveInput::default(), &tuning(), &world, DT);
    assert!(state.position.z >= WALL_LIMIT, "{state:?}");
    assert!(
        state.velocity.z.abs() < 1e-3,
        "speed into the wall should be removed: {state:?}"
    );
}

#[test]
fn moving_diagonally_into_a_wall_slides_along_it() {
    let world = wall();
    let diagonal = MoveInput {
        wish: Vec2::new(1.0, 1.0).normalize(),
        ..FORWARD
    };
    let state = run(&world, settle(&world, Vec3::ZERO), diagonal, 3.0, |s| {
        assert!(s.position.z >= WALL_LIMIT, "{s:?}");
    });
    assert!(
        state.position.x > 250.0,
        "should keep sliding sideways: {state:?}"
    );
}

#[test]
fn corners_stop_you_without_jitter() {
    let mut world = wall();
    // Second wall on the right, face at x = 99.
    add_block(
        &mut world,
        Vec3::new(99.0, 0.0, -500.0),
        Vec3::new(101.0, 300.0, 500.0),
    );
    let into_corner = MoveInput {
        wish: Vec2::new(1.0, 1.0).normalize(),
        ..FORWARD
    };
    let wedged = run(&world, settle(&world, Vec3::ZERO), into_corner, 3.0, |s| {
        assert!(
            s.position.z >= WALL_LIMIT && s.position.x <= 99.0 - RADIUS,
            "{s:?}"
        );
    });
    // Keep pushing for another second: we shouldn't move or vibrate.
    run(&world, wedged, into_corner, 1.0, |s| {
        assert!(
            s.position.distance(wedged.position) < 0.01,
            "{s:?} vs {wedged:?}"
        );
    });
}

// ---- slopes ----

#[test]
fn walks_up_a_gentle_ramp() {
    let mut world = floor();
    add_ramp(&mut world, -100.0, 400.0, 30.0);
    let state = run(&world, settle(&world, Vec3::ZERO), FORWARD, 2.0, |_| {});
    assert!(state.on_ground, "{state:?}");
    assert!(state.position.y > 100.0, "should have climbed: {state:?}");
}

#[test]
fn cannot_walk_up_a_steep_ramp() {
    let mut world = floor();
    add_ramp(&mut world, -100.0, 200.0, 60.0);
    let mut max_y = 0.0f32;
    run(
        &world,
        settle(&world, Vec3::ZERO),
        MoveInput {
            sprint: true,
            ..FORWARD
        },
        3.0,
        |s| {
            max_y = max_y.max(s.position.y);
        },
    );
    assert!(max_y < 20.0, "climbed a 60° slope to {max_y}");
}

#[test]
fn slides_down_a_steep_ramp() {
    let mut world = floor();
    add_ramp(&mut world, -100.0, 200.0, 60.0);
    // Dropped onto the middle of the slope with no input.
    let state = run(
        &world,
        MovementState::new(Vec3::new(0.0, 250.0, -200.0)),
        MoveInput::default(),
        3.0,
        |_| {},
    );
    assert!(state.on_ground && state.position.y < 1.0, "{state:?}");
    assert!(
        state.position.z > -100.0,
        "should end up off the bottom of the slope: {state:?}"
    );
}

// ---- steps ----

fn walk_at_step(height: f32) -> MovementState {
    let mut world = floor();
    add_block(
        &mut world,
        Vec3::new(-500.0, 0.0, -600.0),
        Vec3::new(500.0, height, -100.0),
    );
    run(&world, settle(&world, Vec3::ZERO), FORWARD, 2.0, |_| {})
}

#[test]
fn walks_up_a_step_below_step_height() {
    let state = walk_at_step(12.0);
    assert!(state.on_ground, "{state:?}");
    assert!((state.position.y - 12.0).abs() < 0.5, "{state:?}");
    assert!(state.position.z < -200.0, "{state:?}");
}

#[test]
fn walks_up_a_step_just_under_step_height() {
    let state = walk_at_step(17.5);
    assert!((state.position.y - 17.5).abs() < 0.5, "{state:?}");
}

#[test]
fn blocked_by_a_step_above_step_height() {
    let state = walk_at_step(24.0);
    assert!(state.position.y < 1.0, "{state:?}");
    assert!(state.position.z >= -100.0 + RADIUS - 0.5, "{state:?}");
}

#[test]
fn walks_down_stairs_without_falling() {
    // Four 12-unit steps down, each 32 deep. Player starts on the top one.
    let mut world = floor();
    for k in 0..4 {
        let top = 12.0 * (4 - k) as f32;
        add_block(
            &mut world,
            Vec3::new(-500.0, 0.0, -32.0 * k as f32),
            Vec3::new(500.0, top, 300.0),
        );
    }
    let mut longest_airborne = 0;
    let mut airborne = 0;
    let state = run(
        &world,
        settle(&world, Vec3::new(0.0, 48.0, 100.0)),
        FORWARD,
        2.0,
        |s| {
            airborne = if s.on_ground { 0 } else { airborne + 1 };
            longest_airborne = longest_airborne.max(airborne);
        },
    );
    assert!(state.on_ground && state.position.y < 0.5, "{state:?}");
    assert!(
        longest_airborne <= 2,
        "airborne for {longest_airborne} ticks in a row"
    );
}

#[test]
fn walking_off_a_ledge_falls() {
    let mut world = floor();
    add_block(
        &mut world,
        Vec3::new(-500.0, 0.0, -100.0),
        Vec3::new(500.0, 200.0, 300.0),
    );
    let mut was_airborne = false;
    let state = run(
        &world,
        settle(&world, Vec3::new(0.0, 200.0, 0.0)),
        FORWARD,
        3.0,
        |s| {
            was_airborne |= !s.on_ground;
        },
    );
    assert!(was_airborne);
    assert!(state.on_ground && state.position.y < 0.5, "{state:?}");
}

/// A floor made of two triangles (like map geometry) behaves like a box floor:
/// you land on it, stand on it, and can walk across it.
#[test]
fn lands_and_walks_on_a_triangle_mesh_floor() {
    let mut world = StaticWorld::new();
    let s = 2000.0;
    let points = [
        Vec3::new(-s, 0.0, -s),
        Vec3::new(s, 0.0, -s),
        Vec3::new(s, 0.0, s),
        Vec3::new(-s, 0.0, s),
    ];
    assert!(world.add_trimesh(&points, &[[0, 2, 1], [0, 3, 2]]));
    assert!(!StaticWorld::new().add_trimesh(&[], &[]));

    let dropped = run(
        &world,
        MovementState::new(Vec3::new(0.0, 100.0, 0.0)),
        MoveInput::default(),
        2.0,
        |_| {},
    );
    assert!(dropped.on_ground, "should land on the mesh");
    assert!(
        dropped.position.y.abs() < 1.0,
        "feet at the floor, got {}",
        dropped.position.y
    );

    let walked = run(&world, dropped, FORWARD, 1.0, |s| {
        assert!(s.position.y > -1.0, "fell through")
    });
    assert!(walked.position.z < -100.0, "should have walked forwards");
}

// ---- crouch ----

#[test]
fn crouch_is_slower_and_stays_down_under_a_low_ceiling() {
    let mut world = floor();
    // A 60-unit-high gap: fits crouched (48), not standing (72).
    add_block(
        &mut world,
        Vec3::new(-300.0, 60.0, -400.0),
        Vec3::new(300.0, 80.0, -200.0),
    );
    let crouch = MoveInput {
        crouch: true,
        ..FORWARD
    };
    let start = settle(&world, Vec3::ZERO);
    let state = run(&world, start, crouch, 1.0, |_| {});
    assert!((speed(&state) - 90.0).abs() < 1.0, "{state:?}");
    // Keep going under the ceiling, then let go of crouch: still ducked.
    let state = run(&world, state, crouch, 2.0, |_| {});
    assert!(state.position.z < -200.0, "{state:?}");
    let state = run(&world, state, MoveInput::default(), 0.2, |_| {});
    assert!(state.crouched, "stood up into the ceiling: {state:?}");
    assert!(state.position.y < 1.0);
}

// ---- wall-run / wall-hang / mantle ----

/// A long, tall wall along the X axis with its face at z = -40.
fn long_wall() -> StaticWorld {
    let mut world = floor();
    add_block(
        &mut world,
        Vec3::new(-2000.0, 0.0, -60.0),
        Vec3::new(2000.0, 600.0, -40.0),
    );
    world
}

/// Sprint along the wall (towards -X, yaw 90° left), jump, then run `input`.
fn wallrun_from_jump(world: &StaticWorld, input: MoveInput, seconds: f32) -> (MovementState, f32) {
    let yaw = std::f32::consts::FRAC_PI_2;
    let sprint = MoveInput {
        wish: Vec2::new(0.0, 1.0),
        yaw,
        sprint: true,
        ..Default::default()
    };
    let start = settle(world, Vec3::new(0.0, 0.0, -40.0 + RADIUS + 1.0));
    let mut state = run(world, start, sprint, 1.0, |_| {});
    state = step(
        &state,
        &MoveInput {
            jump: true,
            ..sprint
        },
        &tuning(),
        world,
        DT,
    );
    let mut ran = 0.0;
    let state = run(world, state, MoveInput { yaw, ..input }, seconds, |s| {
        if matches!(s.mode, Mode::WallRun { .. }) {
            ran += DT;
        }
    });
    (state, ran)
}

const HOLD_FORWARD: MoveInput = MoveInput {
    wish: Vec2::new(0.0, 1.0),
    yaw: 0.0,
    jump: false,
    sprint: false,
    crouch: false,
};

#[test]
fn wall_run_starts_beside_a_wall_and_ends_at_the_time_limit() {
    let world = long_wall();
    let (state, ran) = wallrun_from_jump(&world, HOLD_FORWARD, 3.0);
    assert!(ran > 1.4 && ran < 1.6, "wall-ran for {ran} s");
    assert!(state.on_ground, "should fall and land after: {state:?}");
}

#[test]
fn wall_run_falls_slower_than_a_jump() {
    let world = long_wall();
    // A plain jump lands after about 0.7 s; a wall-run is still up after 1 s.
    let (state, _) = wallrun_from_jump(&world, HOLD_FORWARD, 1.0);
    assert!(matches!(state.mode, Mode::WallRun { .. }), "{state:?}");
    assert!(state.position.y > 10.0, "{state:?}");
    assert!(
        speed(&state) > 300.0,
        "should speed up along the wall: {state:?}"
    );
}

#[test]
fn wall_jump_pushes_away_and_up() {
    let world = long_wall();
    let (state, _) = wallrun_from_jump(&world, HOLD_FORWARD, 0.4);
    assert!(matches!(state.mode, Mode::WallRun { .. }), "{state:?}");
    let state = step(
        &state,
        &MoveInput {
            jump: true,
            yaw: std::f32::consts::FRAC_PI_2,
            ..HOLD_FORWARD
        },
        &tuning(),
        &world,
        DT,
    );
    assert_eq!(state.mode, Mode::Free);
    assert!(state.velocity.z > 200.0, "away from the wall: {state:?}");
    assert!(state.velocity.y > 200.0, "{state:?}");
    assert!(state.last_wall.is_some());
}

#[test]
fn wall_hang_holds_still_until_crouch_is_released() {
    let world = long_wall();
    let hang = MoveInput {
        crouch: true,
        ..Default::default()
    };
    let (state, _) = wallrun_from_jump(&world, hang, 0.3);
    assert!(matches!(state.mode, Mode::WallHang { .. }), "{state:?}");
    let y = state.position.y;
    let state = run(&world, state, hang, 1.0, |_| {});
    assert!((state.position.y - y).abs() < 0.01, "{state:?}");
    let state = run(&world, state, MoveInput::default(), 0.1, |_| {});
    assert_eq!(state.mode, Mode::Free);
    assert!(state.velocity.y < 0.0);
}

#[test]
fn mantles_onto_a_ledge_within_reach() {
    let mut world = floor();
    // A 64-high block whose face is at z = -100.
    add_block(
        &mut world,
        Vec3::new(-300.0, 0.0, -400.0),
        Vec3::new(300.0, 64.0, -100.0),
    );
    let start = settle(&world, Vec3::ZERO);
    let jump = MoveInput {
        jump: true,
        ..FORWARD
    };
    let state = step(&start, &jump, &tuning(), &world, DT);
    let mut mantled = false;
    let state = run(&world, state, FORWARD, 2.0, |s| {
        mantled |= matches!(s.mode, Mode::Mantle { .. });
    });
    assert!(mantled, "never mantled: {state:?}");
    assert!(
        state.on_ground && (state.position.y - 64.0).abs() < 1.0,
        "{state:?}"
    );
    assert!(state.position.z < -100.0, "{state:?}");
}

#[test]
fn cannot_mantle_a_wall_that_is_too_tall() {
    let world = long_wall();
    let start = settle(&world, Vec3::ZERO);
    let jump = MoveInput {
        jump: true,
        ..FORWARD
    };
    let state = step(&start, &jump, &tuning(), &world, DT);
    let state = run(&world, state, FORWARD, 2.0, |s| {
        assert!(!matches!(s.mode, Mode::Mantle { .. }), "{s:?}");
    });
    assert!(state.position.y < 1.0);
}

#[test]
fn letting_go_of_forward_ends_a_wall_run() {
    let world = long_wall();
    let (state, _) = wallrun_from_jump(&world, HOLD_FORWARD, 0.4);
    assert!(matches!(state.mode, Mode::WallRun { .. }), "{state:?}");
    let state = run(&world, state, MoveInput::default(), DT, |_| {});
    assert_eq!(state.mode, Mode::Free);
}

#[test]
fn mantles_off_a_wall_run_by_turning_onto_the_ledge() {
    // A long wall only 110 high: too tall to jump onto, low enough to mantle.
    let mut world = floor();
    add_block(
        &mut world,
        Vec3::new(-2000.0, 0.0, -400.0),
        Vec3::new(2000.0, 110.0, -40.0),
    );
    let (state, _) = wallrun_from_jump(&world, HOLD_FORWARD, 0.2);
    assert!(matches!(state.mode, Mode::WallRun { .. }), "{state:?}");
    // Turn to face the wall (-Z) and keep pushing forward.
    let mut mantled = false;
    let state = run(&world, state, HOLD_FORWARD, 1.0, |s| {
        mantled |= matches!(s.mode, Mode::Mantle { .. });
    });
    assert!(mantled, "{state:?}");
    assert!((state.position.y - 110.0).abs() < 1.0, "{state:?}");
}
