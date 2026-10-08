//! Pilot parkour: wall-run, wall jump, wall-hang and mantle.
//!
//! Each mode is a small function that takes this tick's state and returns the next.
//! `try_start` runs at the end of every airborne free-move tick and decides whether
//! one of them begins. How the wall moves work follows the game's own code, written
//! up in plain words in docs/movement/r1-engine-values.md ("Behaviour"); the mantle
//! still follows the engine's settings and is due the same treatment.

use glam::Vec3;

use super::{Body, Mode, MoveInput, MovementState, accelerate, horizontal, wish_velocity};
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
/// On a wall, if the view's right vector is less aligned with the wall normal than
/// this you're facing the wall (forward climbs); otherwise you're running along it.
const FACING_WALL: f32 = 0.7;
/// Slower than this into the wall doesn't count as hitting it, units/s.
const INTO_WALL: f32 = 1.0;
/// Same wall: normals at least this aligned (`wallrun_sameWallDot`).
const SAME_WALL_DOT: f32 = 0.9;
/// A wall jump while pushing into the wall loses this much of its outward push
/// per unit of "into the wall" input.
const JUMP_INTO_WALL: f32 = 0.8;
/// The wall jump's upward part is applied this many times over, then capped.
const JUMP_UP_MULTIPLIER: f32 = 1.5;
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

/// The view's forward (with pitch) and right (flat) vectors.
fn view(input: &MoveInput) -> (Vec3, Vec3) {
    let (sy, cy) = input.yaw.sin_cos();
    let (sp, cp) = input.pitch.sin_cos();
    (Vec3::new(-sy * cp, sp, -cy * cp), Vec3::new(cy, 0.0, -sy))
}

/// How much upward help is left after `count` wall-runs without landing (0 on a
/// same-wall run). This is the game's formula as written: full up to the start
/// count, nothing from the end count, and in between it *rises* from 0 to 1, so
/// the 4th run gets half and the 3rd full.
fn strength(count: u8, same_wall: bool, t: &MovementTuning) -> f32 {
    let (start, end) = (t.wallrun_strength_loss_start, t.wallrun_strength_loss_end);
    let c = f32::from(count);
    if same_wall || c >= end {
        0.0
    } else if c <= start {
        1.0
    } else {
        (c - start) / (end - start).max(1e-3)
    }
}

/// While hanging, speed and gravity fade out over `wall_hang_stop_time`.
fn hang_factor(hang: Option<f32>, t: &MovementTuning) -> f32 {
    hang.map_or(1.0, |h| {
        (1.0 - h / t.wall_hang_stop_time.max(1e-3)).max(0.0)
    })
}

/// Gravity on the wall: ramps from (almost) nothing to full over the ramp time,
/// less so for tired or same-wall runs.
fn gravity_scale(s: &MovementState, run_time: f32, hang: Option<f32>, t: &MovementTuning) -> f32 {
    let ramp = t.wallrun_gravity_ramp_time;
    let base = if run_time >= ramp || ramp <= 0.0 {
        1.0
    } else {
        1.0 - (1.0 - run_time / ramp) * strength(s.wallruns, s.same_wall, t)
    };
    base * hang_factor(hang, t)
}

/// The wished movement on a wall: the controls are rebuilt in the wall's plane, so
/// looking up runs up the wall, strafing moves up the wall (never down), and
/// holding forward adds some automatic "up".
fn wall_wish(input: &MoveInput, normal: Vec3, t: &MovementTuning) -> (Vec3, Vec3) {
    let (look, right) = view(input);
    let (fwd, side) = if right.dot(normal).abs() < FACING_WALL {
        let fwd = normal.cross(right).normalize_or_zero();
        (fwd, fwd.cross(normal))
    } else {
        let side = look.cross(normal).normalize_or_zero();
        (normal.cross(side), side)
    };
    let forward_in = input.wish.y;
    let along = normal.cross(Vec3::Y).normalize_or_zero();
    let mut side_in = input.wish.x + forward_in.max(0.0) * fwd.dot(along) * t.wallrun_upward_push;
    // Strafing may only take you up the wall.
    if (side.y < -0.01 && side_in > 0.0) || (side.y > 0.01 && side_in < 0.0) {
        side_in = 0.0;
    }
    let wish = fwd * forward_in + side * side_in;
    (wish - normal * wish.dot(normal), look)
}

/// In the air after a free move: start a mantle, wall-hang or wall-run if the
/// situation calls for one. Checked in that order.
pub(crate) fn try_start(
    s: &mut MovementState,
    approach: Vec3,
    input: &MoveInput,
    tuning: &MovementTuning,
    body: &Body,
    world: &impl CollisionWorld,
) {
    if try_mantle(s, input, tuning.mantle_max_view_angle, tuning, body, world) {
        return;
    }
    let face = facing(input.yaw);
    let Some((normal, dist)) = nearest_wall(s.position, s.velocity, face, body, world) else {
        return;
    };
    // You have to hit the wall moving into it (any angle will do).
    if approach.dot(normal) > -INTO_WALL {
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
    // The same wall again: only lower down than where you left it.
    let same_wall = match s.last_wall {
        Some((last, left_at)) if last.dot(normal) > SAME_WALL_DOT => {
            if s.position.y >= left_at.y {
                return;
            }
            true
        }
        _ => false,
    };
    s.position -= normal * (dist - SKIN).max(0.0);
    s.velocity -= normal * s.velocity.dot(normal).min(0.0);
    s.same_wall = same_wall;
    s.wallruns = s.wallruns.saturating_add(1);
    s.air_jumps = 0;
    // The upward boost adds to your vertical speed, capped at the boost.
    let boost = strength(s.wallruns, same_wall, tuning) * tuning.wallrun_up_boost;
    if s.velocity.y < boost {
        s.velocity.y = (s.velocity.y + boost).min(boost);
    }
    s.mode = if input.zoom {
        Mode::WallHang { normal, time: 0.0 }
    } else {
        Mode::WallRun { normal, time: 0.0 }
    };
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn wall_run(
    s: MovementState,
    normal: Vec3,
    time: f32,
    input: &MoveInput,
    tuning: &MovementTuning,
    body: &Body,
    world: &impl CollisionWorld,
    dt: f32,
) -> MovementState {
    on_wall(s, normal, time, None, input, tuning, body, world, dt)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn wall_hang(
    s: MovementState,
    normal: Vec3,
    time: f32,
    input: &MoveInput,
    tuning: &MovementTuning,
    body: &Body,
    world: &impl CollisionWorld,
    dt: f32,
) -> MovementState {
    on_wall(s, normal, time, Some(time), input, tuning, body, world, dt)
}

/// One tick on a wall, running (`hang` = None) or hanging (`hang` = seconds hung).
#[allow(clippy::too_many_arguments)]
fn on_wall(
    mut s: MovementState,
    normal: Vec3,
    time: f32,
    hang: Option<f32>,
    input: &MoveInput,
    t: &MovementTuning,
    body: &Body,
    world: &impl CollisionWorld,
    dt: f32,
) -> MovementState {
    // Follow the wall as it curves; drop off where it ends.
    let Some(normal) = wall_at(s.position, normal, body, world) else {
        return detach(s, normal, t);
    };
    if input.jump {
        return wall_jump(s, normal, hang.is_some(), input, t, body, world, dt);
    }
    // Running along a wall into a ledge, or turning onto the top of the wall.
    if try_mantle(
        &mut s,
        input,
        t.mantle_max_view_angle_wallrun,
        t,
        body,
        world,
    ) {
        return s;
    }
    match hang {
        // Aim down sights to hang; let go to run again.
        None if input.zoom => {
            s.mode = Mode::WallHang { normal, time: 0.0 };
            return s;
        }
        Some(_) if !input.zoom => {
            s.mode = Mode::WallRun {
                normal,
                time: t.wallrun_gravity_ramp_time,
            };
            return s;
        }
        _ => {}
    }
    let (limit, slip_time, slip_speed) = match hang {
        None => (
            t.wallrun_time_limit,
            t.wallrun_slip_time,
            t.wallrun_slip_speed,
        ),
        Some(_) => (
            t.wall_hang_time_limit,
            t.wall_hang_slip_time,
            t.wallrun_slip_speed,
        ),
    };
    if input.crouch || time >= limit {
        return detach(s, normal, t);
    }

    let factor = hang_factor(hang, t);
    let slip_start = limit - slip_time;
    let slipping = time > slip_start;
    let mut v = s.velocity;
    // The slip: a kick down, and (running) half the horizontal speed gone.
    if slipping && time - dt <= slip_start {
        v.y -= slip_speed;
        if hang.is_none() {
            let keep = 1.0 - t.wallrun_slip_slowdown;
            v.x *= keep;
            v.z *= keep;
        }
    }

    // Friction on the whole velocity; while slipping, falling isn't slowed.
    let speed = v.length();
    if speed > 1e-4 {
        let old_vy = v.y;
        let new = ((speed - t.wallrun_friction * speed * dt) * factor).max(0.0);
        v *= new / speed;
        if slipping && old_vy < 0.0 {
            v.y = old_vy;
        }
    }

    let (wish, look) = wall_wish(input, normal, t);
    if wish.y.abs() > 1e-4 {
        let target = wish.y.abs().min(1.0) * t.wallrun_max_speed_vertical * factor;
        v = accelerate(
            v,
            Vec3::Y * wish.y.signum() * target,
            t.wallrun_accelerate_vertical * dt,
        );
    }
    let flat = horizontal(wish);
    if let Some(dir) = flat.try_normalize() {
        let top = if dir.dot(look) > 0.0 {
            t.wallrun_max_speed
        } else {
            t.wallrun_max_speed_backward
        };
        let target = flat.length().min(1.0) * top * factor;
        v = accelerate(v, dir * target, t.wallrun_accelerate * dt);
    }
    let run_time = if hang.is_some() { f32::INFINITY } else { time };
    v.y -= t.gravity * gravity_scale(&s, run_time, hang, t) * dt;

    let moved = body.slide(world, s.position, v - normal * WALL_STICK, dt, false);
    s.position = body.push_out(world, moved.pos);
    // Keep only the motion along the wall; the stick-in speed is re-added each tick.
    s.velocity = moved.vel - normal * moved.vel.dot(normal);
    s.mode = match hang {
        None => Mode::WallRun { normal, time },
        Some(_) => Mode::WallHang { normal, time },
    };
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
        s.same_wall = false;
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

/// Jump off the wall. Adds to your velocity (so speed along the wall is kept): a
/// push towards your input, at least `wall_jump_outward_speed` away from the wall
/// (less when pushing into it), and an upward kick capped at `wall_jump_up_speed`.
#[allow(clippy::too_many_arguments)]
fn wall_jump(
    mut s: MovementState,
    normal: Vec3,
    hanging: bool,
    input: &MoveInput,
    t: &MovementTuning,
    body: &Body,
    world: &impl CollisionWorld,
    dt: f32,
) -> MovementState {
    let mut dir = wish_velocity(input, t, false).normalize_or_zero() * input.wish.length().min(1.0);
    if hanging && dir.length() < 1.0 {
        // From a hang, top the direction up with where you're looking.
        dir += view(input).0 * (1.0 - dir.length());
    }
    let mut push = dir * t.wall_jump_input_speed;
    let into = dir.dot(normal);
    let outward = if into < 0.0 {
        t.wall_jump_outward_speed * (1.0 + JUMP_INTO_WALL * into)
    } else {
        t.wall_jump_outward_speed
    };
    let away = push.dot(normal);
    if away < outward {
        push += normal * (outward - away);
    }
    s.velocity += push;
    s.same_wall = false;
    let up = strength(s.wallruns, false, t) * t.wall_jump_up_speed;
    if s.velocity.y < up {
        s.velocity.y = (s.velocity.y + JUMP_UP_MULTIPLIER * up).min(up);
    }
    s.mode = Mode::Free;
    s.last_wall = Some((normal, s.position));
    s.on_ground = false;
    s.since_jump = 0.0;
    let moved = body.slide(world, s.position, s.velocity, dt, false);
    s.position = body.push_out(world, moved.pos);
    s.velocity = moved.vel;
    s
}

/// Let go of the wall and fall, pushed a little away from it.
fn detach(mut s: MovementState, normal: Vec3, t: &MovementTuning) -> MovementState {
    s.mode = Mode::Free;
    s.last_wall = Some((normal, s.position));
    s.velocity += normal * t.wallrun_fall_away_speed;
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
        s.same_wall = false;
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
