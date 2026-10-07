//! Pilot parkour: wall-run, wall jump, wall-hang and mantle.
//!
//! Each mode is a small function that takes this tick's state and returns the next.
//! `try_start` runs at the end of every airborne free-move tick and decides whether
//! one of them begins. The time limits come from tuning; how the moves are triggered
//! follows what we see in the PC version's training mission and gameplay footage.

use glam::Vec3;

use super::{Body, Mode, MoveInput, MovementState, horizontal, move_towards, wish_velocity};
use crate::{CollisionWorld, MovementTuning, SKIN};

/// How far beyond the skin gap a wall-run or wall-hang stays attached. Units.
const WALL_PROBE: f32 = 6.0;
/// How far away a wall can be and still catch you; you're pulled onto it. Units.
const WALL_CATCH: f32 = 20.0;
/// A surface counts as a wall if its normal's up component is within this of zero
/// (about 72° to 108° from flat).
const WALL_MAX_NORMAL_Y: f32 = 0.3;
/// Speed pressing into the wall while running on it, so curved walls stay attached.
/// Units/s.
const WALL_STICK: f32 = 20.0;
/// Speed away from the wall when you let go of it. Units/s.
const DETACH_PUSH: f32 = 40.0;
/// Steering at least this much away from the wall (dot product) lets go of it.
const STEER_AWAY: f32 = 0.5;
/// Forward stick/key must be held at least this much to keep wall-running.
const HOLD_FORWARD: f32 = 0.3;
/// A mantle only starts when pushing forward at least this much and facing the wall
/// at least this squarely (dot products).
const MANTLE_WISH: f32 = 0.5;
const MANTLE_FACING: f32 = 0.6;

/// Facing direction on the ground plane for a yaw (0 faces -Z, positive turns left).
pub(crate) fn facing(yaw: f32) -> Vec3 {
    let (sin, cos) = yaw.sin_cos();
    Vec3::new(-sin, 0.0, -cos)
}

/// In the air after a free move: start a mantle, wall-hang or wall-run if the
/// situation calls for one. Checked in that order.
pub(crate) fn try_start(
    s: &mut MovementState,
    input: &MoveInput,
    tuning: &MovementTuning,
    body: &Body,
    world: &impl CollisionWorld,
) {
    let face = facing(input.yaw);
    if try_mantle(s, input, tuning, body, world) {
        return;
    }

    if input.crouch
        && let Some((normal, dist)) = nearest_wall(s.position, s.velocity, face, body, world)
    {
        s.position -= normal * (dist - SKIN).max(0.0);
        s.mode = Mode::WallHang { normal, time: 0.0 };
        s.velocity = Vec3::ZERO;
        s.air_jumps = 0;
        return;
    }

    let flat = horizontal(s.velocity);
    if flat.length() < tuning.wallrun_min_speed || s.velocity.y < -tuning.wallrun_max_fall_speed {
        return;
    }
    let Some((normal, dist)) = nearest_wall(s.position, s.velocity, face, body, world) else {
        return;
    };
    if s.last_wall.is_some_and(|last| last.dot(normal) > 0.9) {
        return;
    }
    // Running straight into a wall isn't a wall-run: there has to be speed along it.
    let along = flat - normal * flat.dot(normal);
    if along.length() < tuning.wallrun_min_speed {
        return;
    }
    s.position -= normal * (dist - SKIN).max(0.0);
    s.mode = Mode::WallRun { normal, time: 0.0 };
    s.velocity = along + Vec3::Y * s.velocity.y.clamp(0.0, tuning.wallrun_max_lift);
    s.air_jumps = 0;
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn wall_run(
    mut s: MovementState,
    normal: Vec3,
    time: f32,
    input: &MoveInput,
    tuning: &MovementTuning,
    body: &Body,
    world: &impl CollisionWorld,
    dt: f32,
) -> MovementState {
    // Follow the wall as it curves; drop off where it ends.
    let Some(normal) = wall_at(s.position, normal, body, world) else {
        return detach(s, normal);
    };
    if input.jump {
        return wall_jump(s, normal, tuning, body, world, dt);
    }
    // Running along a wall into a ledge, or turning onto the top of the wall.
    if try_mantle(&mut s, input, tuning, body, world) {
        return s;
    }
    if input.crouch {
        s.mode = Mode::WallHang { normal, time: 0.0 };
        s.velocity = Vec3::ZERO;
        return s;
    }
    let wish = wish_velocity(input, tuning, false);
    let steering_away = wish.normalize_or_zero().dot(normal) > STEER_AWAY;
    // The training mission: "HOLD forward to run along the wall".
    let holding_forward = input.wish.y > HOLD_FORWARD;
    if time > tuning.wallrun_time_limit || steering_away || !holding_forward {
        return detach(s, normal);
    }

    let flat = horizontal(s.velocity);
    let along = flat - normal * flat.dot(normal);
    let dir = along.normalize_or_zero();
    // Pushing roughly along the run builds speed up to wallrun_speed; otherwise you
    // keep what you have.
    let target = if wish.dot(dir) > 0.0 {
        tuning.wallrun_speed
    } else {
        along.length()
    };
    let speed = move_towards(along, dir * target, tuning.wallrun_accel * dt).length();
    if speed < tuning.wallrun_min_speed {
        return detach(s, normal);
    }

    let mut vy = s.velocity.y - tuning.wallrun_gravity * dt * 0.5;
    let vel = dir * speed + Vec3::Y * vy - normal * WALL_STICK;
    let moved = body.slide(world, s.position, vel, dt, false);
    vy = moved.vel.y - tuning.wallrun_gravity * dt * 0.5;
    s.position = body.push_out(world, moved.pos);
    // Keep only the motion along the wall; the stick-in speed is re-added each tick.
    let flat = horizontal(moved.vel);
    s.velocity = flat - normal * flat.dot(normal) + Vec3::Y * vy;
    s.mode = Mode::WallRun { normal, time };
    land_if_grounded(s, body, world)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn wall_hang(
    mut s: MovementState,
    normal: Vec3,
    time: f32,
    input: &MoveInput,
    tuning: &MovementTuning,
    body: &Body,
    world: &impl CollisionWorld,
    dt: f32,
) -> MovementState {
    let Some(normal) = wall_at(s.position, normal, body, world) else {
        return detach(s, normal);
    };
    if input.jump {
        return wall_jump(s, normal, tuning, body, world, dt);
    }
    if try_mantle(&mut s, input, tuning, body, world) {
        return s;
    }
    if !input.crouch || time > tuning.wall_hang_time_limit {
        return detach(s, normal);
    }
    s.velocity = Vec3::ZERO;
    s.mode = Mode::WallHang { normal, time };
    s
}

pub(crate) fn mantle(
    mut s: MovementState,
    from: Vec3,
    to: Vec3,
    time: f32,
    tuning: &MovementTuning,
) -> MovementState {
    let t = (time / tuning.mantle_time.max(1e-3)).min(1.0);
    // First half: pull up level with the ledge. Second half: over it.
    let up = (t * 2.0).min(1.0);
    let over = (t * 2.0 - 1.0).max(0.0);
    let next = Vec3::new(
        from.x + (to.x - from.x) * over,
        from.y + (to.y - from.y) * up,
        from.z + (to.z - from.z) * over,
    );
    s.velocity = (next - s.position) / tuning.mantle_time.max(1e-3) * 0.5;
    s.position = next;
    if t >= 1.0 {
        s.mode = Mode::Free;
        s.on_ground = true;
        s.velocity = horizontal(to - from).normalize_or_zero() * tuning.walk_speed * 0.5;
        s.air_jumps = 0;
        s.last_wall = None;
    } else {
        s.mode = Mode::Mantle { from, to, time };
        s.on_ground = false;
    }
    s
}

/// Pushing forward at a ledge in reach: start climbing onto it.
fn try_mantle(
    s: &mut MovementState,
    input: &MoveInput,
    tuning: &MovementTuning,
    body: &Body,
    world: &impl CollisionWorld,
) -> bool {
    if input.wish.y < MANTLE_WISH {
        return false;
    }
    let Some(to) = find_ledge(s.position, facing(input.yaw), tuning, body, world) else {
        return false;
    };
    s.mode = Mode::Mantle {
        from: s.position,
        to,
        time: 0.0,
    };
    s.velocity = Vec3::ZERO;
    true
}

/// Jump off the wall: keep the speed along it, add a push away and a fresh jump.
fn wall_jump(
    mut s: MovementState,
    normal: Vec3,
    tuning: &MovementTuning,
    body: &Body,
    world: &impl CollisionWorld,
    dt: f32,
) -> MovementState {
    let flat = horizontal(s.velocity);
    let along = flat - normal * flat.dot(normal);
    s.velocity = along
        + normal * tuning.wall_jump_push
        + Vec3::Y * tuning.launch_speed(tuning.wall_jump_height);
    s.mode = Mode::Free;
    s.last_wall = Some(normal);
    s.on_ground = false;
    let moved = body.slide(world, s.position, s.velocity, dt, false);
    s.position = body.push_out(world, moved.pos);
    s.velocity = moved.vel;
    s
}

/// Let go of the wall and fall.
fn detach(mut s: MovementState, normal: Vec3) -> MovementState {
    s.mode = Mode::Free;
    s.last_wall = Some(normal);
    s.velocity += normal * DETACH_PUSH;
    s
}

fn land_if_grounded(
    mut s: MovementState,
    body: &Body,
    world: &impl CollisionWorld,
) -> MovementState {
    if body.find_ground(world, s.position, s.velocity).is_some() {
        s.mode = Mode::Free;
        s.on_ground = true;
        s.velocity.y = 0.0;
        s.air_jumps = 0;
        s.last_wall = None;
    }
    s
}

/// Is there a wall right next to us on the side `normal` points away from? Returns
/// its (flattened) normal.
fn wall_at(pos: Vec3, normal: Vec3, body: &Body, world: &impl CollisionWorld) -> Option<Vec3> {
    wall_in(pos, -normal, WALL_PROBE, body, world).map(|(n, _)| n)
}

/// The closest wall within catching distance, and how far it is: checked straight
/// ahead of our motion, to each side and the way we're facing.
fn nearest_wall(
    pos: Vec3,
    vel: Vec3,
    face: Vec3,
    body: &Body,
    world: &impl CollisionWorld,
) -> Option<(Vec3, f32)> {
    let ahead = horizontal(vel).try_normalize().unwrap_or(face);
    let side = Vec3::Y.cross(ahead);
    [
        ahead,
        side,
        -side,
        (ahead + side).normalize(),
        (ahead - side).normalize(),
        face,
    ]
    .into_iter()
    .filter_map(|dir| wall_in(pos, dir, WALL_CATCH, body, world))
    .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// A wall within `probe` (plus the skin gap) in direction `dir`: its flat normal
/// and distance.
fn wall_in(
    pos: Vec3,
    dir: Vec3,
    probe: f32,
    body: &Body,
    world: &impl CollisionWorld,
) -> Option<(Vec3, f32)> {
    let reach = SKIN + probe;
    let hit = world.cast_capsule(&body.capsule, pos, dir * reach)?;
    if hit.normal.y.abs() > WALL_MAX_NORMAL_Y {
        return None;
    }
    let normal = horizontal(hit.normal).try_normalize()?;
    Some((normal, hit.fraction * reach))
}

/// Feet position on top of a ledge in front of us that we can climb onto, if any.
///
/// Feel forward for a wall, look for headroom straight up, then forward over the
/// top, then down onto something walkable, and check we'd fit standing there.
fn find_ledge(
    pos: Vec3,
    face: Vec3,
    tuning: &MovementTuning,
    body: &Body,
    world: &impl CollisionWorld,
) -> Option<Vec3> {
    let (normal, dist) = wall_in(pos, face, WALL_PROBE, body, world)?;
    if -normal.dot(face) < MANTLE_FACING {
        return None;
    }
    let lift = body.sweep_distance(world, pos, Vec3::Y, tuning.mantle_reach);
    let forward = dist + body.capsule.radius + 2.0;
    let top = pos + Vec3::Y * lift;
    if body.sweep_distance(world, top, -normal, forward) < forward - 0.5 {
        return None;
    }
    let over = top - normal * forward;
    let hit = world.cast_capsule(&body.capsule, over, Vec3::NEG_Y * lift)?;
    body.ground_under(world, &hit)?;
    let feet = over - Vec3::Y * (hit.fraction * lift - SKIN).max(0.0);
    // Must actually be a climb, and we must fit standing up on top.
    if feet.y <= pos.y + 1.0 || !Body::new(tuning, false).fits(world, feet) {
        return None;
    }
    Some(feet)
}
