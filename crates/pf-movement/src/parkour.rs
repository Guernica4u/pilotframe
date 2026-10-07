//! Pilot parkour: wall-run, wall jump, wall-hang and mantle.
//!
//! Each mode is a small function that takes this tick's state and returns the next.
//! `try_start` runs at the end of every airborne free-move tick and decides whether
//! one of them begins. How each move behaves follows the engine's own settings and
//! their descriptions, written up in docs/movement/r1-engine-values.md.

use glam::Vec3;

use super::{
    Body, Mode, MoveInput, MovementState, accelerate, friction, horizontal, wish_velocity,
};
use crate::{CollisionWorld, MovementTuning, SKIN};

/// How far beyond the skin gap a wall-run or wall-hang stays attached. Units.
const WALL_PROBE: f32 = 6.0;
/// How far away a wall can be and still catch you; you're pulled onto it. Units.
const WALL_CATCH: f32 = 20.0;
/// How far ahead a mantle looks for the ledge's wall (`automantle_searchdist`).
const MANTLE_SEARCH: f32 = 5.0;
/// A surface counts as a wall if its normal's up component is within this of zero
/// (about 72° to 108° from flat).
const WALL_MAX_NORMAL_Y: f32 = 0.3;
/// Speed pressing into the wall while running on it, so curved walls stay attached.
/// Units/s.
const WALL_STICK: f32 = 20.0;
/// Steering at least this much away from the wall (dot product) lets go of it.
const STEER_AWAY: f32 = 0.5;
/// Forward stick/key must be held at least this much to keep wall-running.
const HOLD_FORWARD: f32 = 0.3;
/// A mantle needs the input pushing within this angle of straight into the wall
/// (`automantle_maxangle_push`, 50°), as a cosine.
const MANTLE_PUSH_COS: f32 = 0.643;
/// "Only high walls": the wall must still be there this far above the feet, so
/// you can't wall-run something you could see over. Units.
const HIGH_WALL_CHECK: f32 = 64.0;

/// Facing direction on the ground plane for a yaw (0 faces -Z, positive turns left).
pub(crate) fn facing(yaw: f32) -> Vec3 {
    let (sin, cos) = yaw.sin_cos();
    Vec3::new(-sin, 0.0, -cos)
}

/// How much of the upward boost is left after `wallruns` wall-runs without
/// landing: full at first, fading to nothing between the two thresholds.
fn strength(wallruns: u8, tuning: &MovementTuning) -> f32 {
    let span = (tuning.wallrun_strength_loss_end - tuning.wallrun_strength_loss_start).max(1e-3);
    1.0 - ((f32::from(wallruns) - tuning.wallrun_strength_loss_start) / span).clamp(0.0, 1.0)
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
    if try_mantle(s, input, tuning.mantle_max_view_angle, tuning, body, world) {
        return;
    }
    let face = facing(input.yaw);

    if input.zoom
        && let Some((normal, dist)) = nearest_wall(s.position, s.velocity, face, body, world)
    {
        s.position -= normal * (dist - SKIN).max(0.0);
        s.mode = Mode::WallHang { normal, time: 0.0 };
        s.air_jumps = 0;
        return;
    }

    let flat = horizontal(s.velocity);
    if flat.length() < tuning.wallrun_min_speed {
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
    // Only walls you can't see over.
    if wall_in(
        s.position + Vec3::Y * HIGH_WALL_CHECK,
        -normal,
        WALL_CATCH,
        body,
        world,
    )
    .is_none()
    {
        return;
    }
    s.position -= normal * (dist - SKIN).max(0.0);
    s.mode = Mode::WallRun { normal, time: 0.0 };
    let boost = tuning.wallrun_up_boost * strength(s.wallruns, tuning);
    s.velocity = along + Vec3::Y * s.velocity.y.max(boost);
    s.air_jumps = 0;
    s.wallruns = s.wallruns.saturating_add(1);
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
        return detach(s, normal, tuning);
    };
    if input.jump {
        return wall_jump(s, normal, input, tuning, body, world, dt);
    }
    if input.zoom {
        s.mode = Mode::WallHang { normal, time: 0.0 };
        return s;
    }
    // Running along a wall into a ledge, or turning onto the top of the wall.
    if try_mantle(
        &mut s,
        input,
        tuning.mantle_max_view_angle_wallrun,
        tuning,
        body,
        world,
    ) {
        return s;
    }
    let slipping = time > tuning.wallrun_time_limit;
    let wish = wish_velocity(input, tuning, false);
    let steering_away = wish.normalize_or_zero().dot(normal) > STEER_AWAY;
    // The training mission: "HOLD forward to run along the wall".
    let holding_forward = input.wish.y > HOLD_FORWARD;
    if input.crouch
        || time > tuning.wallrun_time_limit + tuning.wallrun_slip_time
        || (!slipping && (steering_away || !holding_forward))
    {
        return detach(s, normal, tuning);
    }

    let flat = horizontal(s.velocity);
    let mut along = flat - normal * flat.dot(normal);
    let mut vy = s.velocity.y;
    if slipping {
        // The moment the time runs out: a kick down and half the speed gone.
        if time - dt <= tuning.wallrun_time_limit {
            vy -= tuning.wallrun_slip_speed;
            along *= 1.0 - tuning.wallrun_slip_slowdown;
        }
    } else {
        // Along the wall: friction, then accelerate towards your input's direction
        // along the wall, up to the wall-run top speed.
        along = friction(along, tuning.wallrun_friction, 0.0, dt);
        let on_wall = wish - normal * wish.dot(normal);
        if let Some(dir) = on_wall.try_normalize() {
            let step = tuning.wallrun_accelerate * tuning.wallrun_max_speed * dt;
            along = accelerate(along, dir * tuning.wallrun_max_speed, step);
        }
        // Pushing forward gives some upward push that fights gravity.
        vy += tuning.wallrun_accelerate_vertical * tuning.wallrun_upward_push * dt;
    }
    // Gravity starts at nothing and ramps up to full; tired runs ramp faster.
    let ramp = tuning.wallrun_gravity_ramp_time * strength(s.wallruns.saturating_sub(1), tuning);
    let gravity_scale = if slipping || ramp <= 1e-3 {
        1.0
    } else {
        (time / ramp).min(1.0)
    };
    vy -= tuning.gravity * gravity_scale * dt;
    vy = vy.clamp(
        -tuning.wallrun_max_speed_vertical,
        tuning.wallrun_max_speed_vertical,
    );

    let vel = along + Vec3::Y * vy - normal * WALL_STICK;
    let moved = body.slide(world, s.position, vel, dt, false);
    s.position = body.push_out(world, moved.pos);
    // Keep only the motion along the wall; the stick-in speed is re-added each tick.
    let flat = horizontal(moved.vel);
    s.velocity = flat - normal * flat.dot(normal) + Vec3::Y * moved.vel.y;
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
        return detach(s, normal, tuning);
    };
    if input.jump {
        return wall_jump(s, normal, input, tuning, body, world, dt);
    }
    if !input.zoom || time > tuning.wall_hang_time_limit + tuning.wall_hang_slip_time {
        return detach(s, normal, tuning);
    }
    if time > tuning.wall_hang_time_limit {
        // Slipping off: a kick down, then gravity.
        if time - dt <= tuning.wall_hang_time_limit {
            s.velocity = Vec3::NEG_Y * tuning.wallrun_slip_speed;
        }
        s.velocity.y -= tuning.gravity * dt;
    } else {
        // Come to a stop over `wall_hang_stop_time`, evenly.
        let left = (tuning.wall_hang_stop_time - (time - dt)).max(dt);
        s.velocity *= (1.0 - dt / left).max(0.0);
    }
    let moved = body.slide(
        world,
        s.position,
        s.velocity - normal * WALL_STICK,
        dt,
        false,
    );
    s.position = body.push_out(world, moved.pos);
    let flat = horizontal(moved.vel);
    s.velocity = flat - normal * flat.dot(normal) + Vec3::Y * moved.vel.y;
    s.mode = Mode::WallHang { normal, time };
    land_if_grounded(s, body, world)
}

pub(crate) fn mantle(
    mut s: MovementState,
    from: Vec3,
    to: Vec3,
    time: f32,
    duration: f32,
    tuning: &MovementTuning,
) -> MovementState {
    let duration = duration.max(1e-3);
    let t = (time / duration).min(1.0);
    // First half: pull up level with the ledge. Second half: over it.
    let up = (t * 2.0).min(1.0);
    let over = (t * 2.0 - 1.0).max(0.0);
    let next = Vec3::new(
        from.x + (to.x - from.x) * over,
        from.y + (to.y - from.y) * up,
        from.z + (to.z - from.z) * over,
    );
    s.velocity = Vec3::ZERO;
    s.position = next;
    if t >= 1.0 {
        s.mode = Mode::Free;
        s.on_ground = true;
        s.velocity = horizontal(to - from).normalize_or_zero() * tuning.walk_speed * 0.5;
        s.air_jumps = 0;
        s.last_wall = None;
        s.wallruns = 0;
    } else {
        s.mode = Mode::Mantle {
            from,
            to,
            time,
            duration,
        };
        s.on_ground = false;
    }
    s
}

/// Pushing forward at a ledge in reach: start climbing onto it.
fn try_mantle(
    s: &mut MovementState,
    input: &MoveInput,
    max_view_deg: f32,
    tuning: &MovementTuning,
    body: &Body,
    world: &impl CollisionWorld,
) -> bool {
    let Some(push) = wish_velocity(input, tuning, false).try_normalize() else {
        return false;
    };
    let face = facing(input.yaw);
    let Some((to, normal)) = find_ledge(s.position, face, tuning, body, world) else {
        return false;
    };
    let view_ok = -normal.dot(face) >= max_view_deg.to_radians().cos();
    let push_ok = -normal.dot(push) >= MANTLE_PUSH_COS;
    if !view_ok || !push_ok {
        return false;
    }
    let eye_above_ledge = s.position.y + tuning.eye_height - to.y;
    s.mode = Mode::Mantle {
        from: s.position,
        to,
        time: 0.0,
        duration: tuning.mantle_time(eye_above_ledge),
    };
    s.velocity = Vec3::ZERO;
    true
}

/// Jump off the wall: keep the speed along it, and add speed up, away from the
/// wall and towards your input.
fn wall_jump(
    mut s: MovementState,
    normal: Vec3,
    input: &MoveInput,
    tuning: &MovementTuning,
    body: &Body,
    world: &impl CollisionWorld,
    dt: f32,
) -> MovementState {
    let flat = horizontal(s.velocity);
    let along = flat - normal * flat.dot(normal);
    let input_dir = wish_velocity(input, tuning, false).normalize_or_zero();
    let up = tuning.wall_jump_up_speed * strength(s.wallruns.saturating_sub(1), tuning);
    s.velocity = along
        + normal * tuning.wall_jump_outward_speed
        + input_dir * tuning.wall_jump_input_speed
        + Vec3::Y * up;
    s.mode = Mode::Free;
    s.last_wall = Some(normal);
    s.on_ground = false;
    let moved = body.slide(world, s.position, s.velocity, dt, false);
    s.position = body.push_out(world, moved.pos);
    s.velocity = moved.vel;
    s
}

/// Let go of the wall and fall.
fn detach(mut s: MovementState, normal: Vec3, tuning: &MovementTuning) -> MovementState {
    s.mode = Mode::Free;
    s.last_wall = Some(normal);
    s.velocity += normal * tuning.wallrun_fall_away_speed;
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
        s.wallruns = 0;
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

/// Feet position on top of a ledge in front of us that we can climb onto, and the
/// wall's normal.
///
/// Feel forward for a wall, look for headroom straight up, then forward over the
/// top, then down onto something walkable, and check we'd fit standing there.
fn find_ledge(
    pos: Vec3,
    face: Vec3,
    tuning: &MovementTuning,
    body: &Body,
    world: &impl CollisionWorld,
) -> Option<(Vec3, Vec3)> {
    let (normal, dist) = wall_in(pos, face, MANTLE_SEARCH, body, world)?;
    let lift = body.sweep_distance(world, pos, Vec3::Y, tuning.mantle_height);
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
    Some((feet, normal))
}
