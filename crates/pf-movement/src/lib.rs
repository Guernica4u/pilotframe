//! Pilot movement as a pure fixed-tick function: `(state, input, world) -> state`.
//!
//! Layer 0: no Bevy, no `std::fs`, so it runs in plain unit tests, in the sandbox,
//! and later on a server or in a browser. Same inputs give the same result, which is
//! what replays and network prediction (M10) need.
//!
//! Units are original game units (~1 inch) and seconds. Y is up, matching the
//! converted cache (PLAN §2). Yaw 0 faces -Z and positive yaw turns left, like a
//! Bevy camera.
//!
//! The collision approach is the general "collide and slide" technique: sweep the
//! capsule, stop just short of what it hits, remove the velocity going into that
//! surface, and sweep again with what's left of the tick.

pub mod collision;
mod parkour;
mod tuning;

use glam::{Vec2, Vec3};

pub use collision::{Capsule, CollisionWorld, Hit, StaticWorld};
pub use tuning::MovementTuning;

/// Gap kept between the capsule and every surface, so the next sweep doesn't start
/// already touching. Units.
const SKIN: f32 = 0.1;
/// How far below the feet we look for ground. Must be bigger than `SKIN`.
const GROUND_PROBE: f32 = 2.0;
/// Most surfaces one sweep can slide off before giving up for this tick.
const MAX_BUMPS: usize = 4;
/// Rising faster than this (units/s) means we have left the ground.
const LIFTOFF_SPEED: f32 = 1.0;
/// A surface this close to level counts as flat (step treads, floors).
const FLAT_NORMAL_Y: f32 = 0.999;
/// A flat surface at least this far above the feet is a step to climb, units.
const STEP_LIP: f32 = 0.5;
/// An edge contact counts as possible ground only if its normal is at least this
/// upward. Below that, the edge is beside the capsule rather than under it.
const EDGE_MIN_NORMAL_Y: f32 = 0.1;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovementState {
    /// Bottom of the capsule.
    pub position: Vec3,
    /// Units/s.
    pub velocity: Vec3,
    pub on_ground: bool,
    /// Ducked: shorter capsule, slower walk. Stays true under a low ceiling even
    /// after crouch is let go.
    pub crouched: bool,
    pub mode: Mode,
    /// Mid-air jumps used. Refilled by landing or touching a wall.
    pub air_jumps: u8,
    /// The wall we last ran on: its normal and where we left it. Re-grabbing it is
    /// only allowed lower down, and gives no boost.
    pub last_wall: Option<(Vec3, Vec3)>,
    /// This wall-run is on the same wall as the last: no boost, full gravity.
    pub same_wall: bool,
    /// Wall-runs since last on the ground; later ones get less upward boost.
    pub wallruns: u8,
    /// Seconds left in which a ground jump still works after walking off a ledge.
    pub jump_grace: f32,
    /// Seconds since the last jump (for the keyboard jump turn).
    pub since_jump: f32,
    /// Seconds since last on the ground.
    pub air_time: f32,
}

/// What the pilot is doing beyond plain walking, falling and jumping.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    /// On the ground or in the air.
    Free,
    /// Running along a wall. `normal` points out of the wall, flat. Past the time
    /// limit the run turns into a slip, then you fall off.
    WallRun { normal: Vec3, time: f32 },
    /// Hanging on a wall (aim down sights while on it).
    WallHang { normal: Vec3, time: f32 },
    /// Climbing over a ledge from `from` to `to` (feet positions) over `duration`.
    Mantle {
        from: Vec3,
        to: Vec3,
        time: f32,
        duration: f32,
    },
}

impl MovementState {
    /// Standing still at `position`. Ground is detected on the first tick.
    pub fn new(position: Vec3) -> Self {
        Self {
            position,
            velocity: Vec3::ZERO,
            on_ground: false,
            crouched: false,
            mode: Mode::Free,
            air_jumps: 0,
            last_wall: None,
            same_wall: false,
            wallruns: 0,
            jump_grace: 0.0,
            since_jump: 10.0,
            air_time: 0.0,
        }
    }
}

/// One tick of player intent. The input layer (keyboard, controller, replay, network)
/// builds this; movement never sees raw buttons.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MoveInput {
    /// x = strafe right, y = forwards. Length up to 1 (a half-tilted stick gives less).
    pub wish: Vec2,
    /// Facing around the up axis, radians.
    pub yaw: f32,
    /// Looking up (positive) or down, radians. Steers wall-runs up and down.
    pub pitch: f32,
    /// A movement key was newly pressed this tick (keyboard only; drives the jump turn).
    pub key_pressed: bool,
    /// The jump button went down since the last tick.
    pub jump: bool,
    /// Sprint is held. Only counts while moving forwards.
    pub sprint: bool,
    /// Crouch is held.
    pub crouch: bool,
    /// Aim down sights is held. On a wall, this hangs on it.
    pub zoom: bool,
}

/// Advance one fixed tick of `dt` seconds.
pub fn step(
    state: &MovementState,
    input: &MoveInput,
    tuning: &MovementTuning,
    world: &impl CollisionWorld,
    dt: f32,
) -> MovementState {
    // Garbage in (a zero tick, NaN from a broken controller) shouldn't corrupt the state.
    if dt.is_nan() || dt <= 0.0 || !input.wish.is_finite() || !input.yaw.is_finite() {
        return *state;
    }
    let mut s = *state;
    duck(&mut s, input.crouch, tuning, world);
    let body = Body::new(tuning, s.crouched);
    s.position = body.push_out(world, s.position);

    match s.mode {
        Mode::Free => free_move(s, input, tuning, &body, world, dt),
        Mode::WallRun { normal, time } => {
            parkour::wall_run(s, normal, time + dt, input, tuning, &body, world, dt)
        }
        Mode::WallHang { normal, time } => {
            parkour::wall_hang(s, normal, time + dt, input, tuning, &body, world, dt)
        }
        Mode::Mantle {
            from,
            to,
            time,
            duration,
        } => parkour::mantle(s, from, to, time + dt, duration, tuning),
    }
}

/// Crouch and stand, the Source way. On the ground the feet stay put and the head
/// moves. In the air the head stays put and the feet move: ducking tucks them up,
/// standing lowers them again, which needs that much room below (otherwise you
/// stay ducked until you land). So ducking can't be used to climb.
fn duck(s: &mut MovementState, want: bool, tuning: &MovementTuning, world: &impl CollisionWorld) {
    let airborne = !s.on_ground && s.mode == Mode::Free;
    let tuck = (tuning.capsule_height - tuning.crouch_height).max(0.0);
    if want && !s.crouched {
        s.crouched = true;
        if airborne {
            let body = Body::new(tuning, true);
            s.position.y += body.sweep_distance(world, s.position, Vec3::Y, tuck);
        }
    } else if !want && s.crouched {
        let standing = Body::new(tuning, false);
        let target = if airborne {
            s.position - Vec3::Y * tuck
        } else {
            s.position
        };
        if standing.fits(world, target) {
            s.position = target;
            s.crouched = false;
        }
    }
}

/// Walking, jumping and falling, plus the checks that start wall-runs, wall-hangs
/// and mantles.
///
/// Ground and air control follow the game's own code (docs/movement/
/// r1-engine-values.md, "Behaviour"): a linear slowdown of whatever you aren't
/// pushing for, then accelerate towards the wished-for velocity.
fn free_move(
    mut s: MovementState,
    input: &MoveInput,
    tuning: &MovementTuning,
    body: &Body,
    world: &impl CollisionWorld,
    dt: f32,
) -> MovementState {
    let mut pos = s.position;
    let mut vel = s.velocity;
    let wish = wish_velocity(input, tuning, s.crouched);

    // Ground is re-checked every tick rather than trusted from the last one.
    let mut ground = body.find_ground(world, pos, vel);
    let mut jumped = false;
    // Velocity before this tick's collisions clip it: what a wall was hit with.
    let mut approach = vel;
    // Just walked off a ledge: a jump still counts as a ground jump for a moment.
    let can_ground_jump = ground.is_some() || (s.jump_grace > 0.0 && vel.y <= 0.0);
    if can_ground_jump && input.jump {
        vel.y = tuning.jump_speed();
        ground = None;
        jumped = true;
        s.jump_grace = 0.0;
        s.since_jump = 0.0;
    }

    let was_grounded = ground.is_some();
    if let Some(normal) = ground {
        // Keep what you're pushing for, bleed off the rest, then accelerate.
        let flat = decelerate(horizontal(vel), wish, tuning.deceleration * dt);
        let flat = accelerate(flat, wish, tuning.acceleration * dt);
        let intended = along_ground(flat, normal);

        let start = pos;
        let plain = body.slide(world, start, intended, dt, true);
        (pos, vel) = (plain.pos, plain.vel);
        // Blocked by something steep: maybe it's a step we can walk up. Keep
        // whichever attempt got further.
        if plain.hit_wall
            && let Some(stepped) = body.step_up(world, start, intended, dt, tuning.step_height)
            && horizontal(stepped.pos - start).length_squared()
                > horizontal(plain.pos - start).length_squared() + 1e-3
        {
            (pos, vel) = (stepped.pos, stepped.vel);
        }
        // Stay glued to the ground walking down ramps and stairs.
        pos = body
            .snap_down(world, pos, tuning.step_height)
            .unwrap_or(pos);
    } else {
        // The double jump replaces the fall speed rather than adding to it, so it
        // always gives the same lift.
        if input.jump && !jumped && s.air_jumps < 1 {
            vel.y = tuning.launch_speed(tuning.double_jump_height);
            s.air_jumps += 1;
            s.since_jump = 0.0;
        }
        // Air control: steer, but only add speed up to `air_speed` in the wished
        // direction. Momentum above that is kept, never added to; a tiny extra
        // push still applies.
        let mut flat = horizontal(vel);
        if let Some(dir) = wish.try_normalize() {
            let air_wish = dir * (input.wish.length().min(1.0) * tuning.air_speed);
            if flat.dot(dir) < air_wish.length() {
                flat = accelerate(flat, air_wish, tuning.air_acceleration * dt);
            } else {
                flat += dir * tuning.air_extra_acceleration * dt;
            }
            if input.key_pressed {
                flat = jump_turn(flat, dir, s.since_jump, tuning);
            }
        }
        vel = Vec3::new(flat.x, vel.y, flat.z);
        // Half the gravity before the move and half after puts every tick exactly on
        // the true parabola, so jump height doesn't depend on the tick rate.
        vel.y -= tuning.gravity * dt * 0.5;
        approach = vel;
        let air = body.slide(world, pos, vel, dt, false);
        (pos, vel) = (air.pos, air.vel);
        vel.y -= tuning.gravity * dt * 0.5;
    }

    pos = body.push_out(world, pos);
    // A ground move that followed a slope up isn't a take-off: only a jump is.
    let walked = if was_grounded { horizontal(vel) } else { vel };
    let ground = body.find_ground(world, pos, walked);
    if ground.is_some() {
        // On the ground the stored velocity stays flat (as in Source): following a
        // slope is done by the move itself, so a step's edge can't launch you.
        vel = horizontal(vel);
        s.air_jumps = 0;
        s.last_wall = None;
        s.same_wall = false;
        s.wallruns = 0;
        s.jump_grace = tuning.jump_grace_time;
    } else {
        s.jump_grace = (s.jump_grace - dt).max(0.0);
        if jumped {
            s.jump_grace = 0.0;
        }
    }
    if !jumped {
        s.since_jump += dt;
    }
    s.position = pos;
    s.velocity = vel;
    s.on_ground = ground.is_some();
    s.air_time = if s.on_ground { 0.0 } else { s.air_time + dt };
    if !s.on_ground && !jumped {
        parkour::try_start(&mut s, approach, input, tuning, body, world);
    }
    s
}

/// Collision shape plus what counts as ground, shared by all the helpers.
struct Body {
    capsule: Capsule,
    min_ground_y: f32,
}

struct Ground {
    normal: Vec3,
    /// Height of the surface itself (not the feet).
    height: f32,
}

struct Slide {
    pos: Vec3,
    vel: Vec3,
    /// Hit something too steep to stand on.
    hit_wall: bool,
}

impl Body {
    fn new(tuning: &MovementTuning, crouched: bool) -> Self {
        Self {
            capsule: tuning.capsule(crouched),
            min_ground_y: tuning.min_ground_normal_y(),
        }
    }

    /// The capsule isn't overlapping anything at `pos`.
    fn fits(&self, world: &impl CollisionWorld, pos: Vec3) -> bool {
        world.push_out(&self.capsule, pos, 0.0).length_squared() < 1e-4
    }

    fn is_ground(&self, normal: Vec3) -> bool {
        normal.y >= self.min_ground_y
    }

    /// The walkable surface just below the feet, unless we're moving away from it.
    fn find_ground(&self, world: &impl CollisionWorld, pos: Vec3, vel: Vec3) -> Option<Vec3> {
        let hit = world.cast_capsule(&self.capsule, pos, Vec3::NEG_Y * GROUND_PROBE)?;
        let ground = self.ground_under(world, &hit)?;
        // Only rising counts as leaving the ground (as in Source): walking downhill
        // with flat velocity isn't taking off.
        (vel.y <= LIFTOFF_SPEED).then_some(ground.normal)
    }

    /// The walkable surface the capsule touched, if it touched one.
    ///
    /// On an edge (a step lip, a ledge we're walking off) the rounded capsule's
    /// contact normal tilts even though the surface is flat, so we look straight
    /// down just past the contact, on the solid side, to see the real surface.
    fn ground_under(&self, world: &impl CollisionWorld, hit: &Hit) -> Option<Ground> {
        if hit.normal.y < EDGE_MIN_NORMAL_Y {
            return None;
        }
        let solid_side = -horizontal(hit.normal).normalize_or_zero();
        match world.cast_ray(hit.point + solid_side * 0.25 + Vec3::Y, Vec3::NEG_Y * 2.0) {
            Some(ray) if self.is_ground(ray.normal) => Some(Ground {
                normal: ray.normal,
                height: ray.point.y,
            }),
            // Only an edge under us (a step's lip): stand on it, but as if flat,
            // so moving doesn't follow the rounded contact up and off the lip.
            _ if self.is_ground(hit.normal) => Some(Ground {
                normal: Vec3::Y,
                height: hit.point.y,
            }),
            _ => None,
        }
    }

    /// Restore the SKIN gap if rounding or a bad spawn left us touching something.
    fn push_out(&self, world: &impl CollisionWorld, pos: Vec3) -> Vec3 {
        pos + world.push_out(&self.capsule, pos, SKIN)
    }

    /// Move for `dt` seconds, sliding along whatever is in the way.
    ///
    /// With `grounded`, surfaces too steep to stand on act as vertical walls, so
    /// walking into a steep slope blocks you instead of sliding you up it.
    fn slide(
        &self,
        world: &impl CollisionWorld,
        mut pos: Vec3,
        mut vel: Vec3,
        dt: f32,
        grounded: bool,
    ) -> Slide {
        let mut remaining = dt;
        let mut planes = Vec::with_capacity(MAX_BUMPS);
        let mut hit_wall = false;

        for _ in 0..MAX_BUMPS {
            let motion = vel * remaining;
            let len = motion.length();
            if len < 1e-4 {
                break;
            }
            let Some(hit) = world.cast_capsule(&self.capsule, pos, motion) else {
                pos += motion;
                break;
            };
            let dir = motion / len;
            // Stop SKIN short of the surface, measured along its normal. At a grazing
            // angle that can mean not moving at all this bump, which is fine: the
            // velocity gets clipped and the next bump slides along the surface.
            let into = -dir.dot(hit.normal);
            let back_off = if into > 1e-3 { SKIN / into } else { SKIN };
            let travel = (hit.fraction * len - back_off).max(0.0);
            pos += dir * travel;
            remaining *= 1.0 - travel / len;

            let mut normal = hit.normal;
            // The capsule's rounded bottom touching a step's lip gives a tilted,
            // ramp-like contact. If the real surface there is flat and higher than
            // our feet, it's a step: treat it as a wall so step-up handles it
            // (Source's flat-bottomed box never sees this).
            let step_lip = grounded
                && self.is_ground(normal)
                && normal.y < FLAT_NORMAL_Y
                && self
                    .ground_under(world, &hit)
                    .is_some_and(|g| g.normal.y >= FLAT_NORMAL_Y && g.height > pos.y + STEP_LIP);
            if !self.is_ground(normal) || step_lip {
                hit_wall = true;
                if grounded {
                    normal = horizontal(normal).try_normalize().unwrap_or(normal);
                }
            }
            planes.push(normal);
            vel = clip_to_planes(vel, &planes);
            if vel == Vec3::ZERO {
                break;
            }
        }
        Slide { pos, vel, hit_wall }
    }

    /// Try the move again from `step_height` higher, then drop back down. Returns
    /// `None` if there's no headroom or nothing walkable to land on.
    fn step_up(
        &self,
        world: &impl CollisionWorld,
        pos: Vec3,
        vel: Vec3,
        dt: f32,
        step_height: f32,
    ) -> Option<Slide> {
        let lift = self.sweep_distance(world, pos, Vec3::Y, step_height);
        if lift < 1.0 {
            return None;
        }
        let across = self.slide(world, pos + Vec3::Y * lift, horizontal(vel), dt, true);

        let hit = world.cast_capsule(&self.capsule, across.pos, Vec3::NEG_Y * (lift + SKIN))?;
        // Check the surface, not the feet: landing on the lip of something taller
        // would otherwise ratchet us up it a step_height at a time.
        if self.ground_under(world, &hit)?.height > pos.y + step_height {
            return None;
        }
        let drop = (hit.fraction * (lift + SKIN) - SKIN).max(0.0);
        Some(Slide {
            pos: across.pos - Vec3::Y * drop,
            vel: across.vel,
            hit_wall: false,
        })
    }

    /// If there's ground within `max_drop` below, move down onto it.
    fn snap_down(&self, world: &impl CollisionWorld, pos: Vec3, max_drop: f32) -> Option<Vec3> {
        let hit = world.cast_capsule(&self.capsule, pos, Vec3::NEG_Y * max_drop)?;
        self.ground_under(world, &hit)?;
        Some(pos - Vec3::Y * (hit.fraction * max_drop - SKIN).max(0.0))
    }

    /// How far we can move in `dir` (unit vector), up to `max`, keeping SKIN clear.
    fn sweep_distance(&self, world: &impl CollisionWorld, pos: Vec3, dir: Vec3, max: f32) -> f32 {
        match world.cast_capsule(&self.capsule, pos, dir * max) {
            Some(hit) => (hit.fraction * max - SKIN).max(0.0),
            None => max,
        }
    }
}

/// Direction and speed the player is asking for, flat on the ground plane.
fn wish_velocity(input: &MoveInput, tuning: &MovementTuning, crouched: bool) -> Vec3 {
    let wish = input.wish.clamp_length_max(1.0);
    let forward = parkour::facing(input.yaw);
    let right = Vec3::new(-forward.z, 0.0, forward.x);
    let speed = if crouched {
        tuning.crouch_speed
    } else if input.sprint && wish.y > 0.0 {
        tuning.sprint_speed
    } else {
        tuning.walk_speed
    };
    (right * wish.x + forward * wish.y) * speed
}

/// Ground slowdown: the part of the velocity along the wished direction, up to the
/// wished speed, is kept; the rest (drift, excess speed, everything when there's no
/// input) shrinks by `step`.
fn decelerate(flat: Vec3, wish: Vec3, step: f32) -> Vec3 {
    let speed_sq = flat.length_squared();
    let keep = match wish.try_normalize() {
        Some(dir) if flat.dot(dir) > 0.0 && speed_sq > 1e-6 => {
            let along = flat.dot(dir);
            dir * along * (along * wish.length() / speed_sq).min(1.0)
        }
        _ => Vec3::ZERO,
    };
    let rest = flat - keep;
    let len = rest.length();
    if len < 1e-6 {
        return keep;
    }
    keep + rest * ((len - step).max(0.0) / len)
}

/// Keyboard jump turn: just after a jump, a newly pressed key turns the horizontal
/// velocity towards `dir` while keeping its speed.
fn jump_turn(flat: Vec3, dir: Vec3, since_jump: f32, tuning: &MovementTuning) -> Vec3 {
    let (min, max) = (tuning.jump_turn_time_min, tuning.jump_turn_time_max);
    let speed = flat.length();
    if since_jump >= max || speed < 1e-3 {
        return flat;
    }
    let fade = ((since_jump - max) / (min - max).min(-1e-3)).clamp(0.0, 1.0);
    let current = flat / speed;
    let turned = current + (dir - current) * fade * tuning.jump_turn_strength;
    let Some(turned) = turned.try_normalize() else {
        return flat;
    };
    let change =
        (turned * speed - flat).clamp_length_max(tuning.jump_turn_max * tuning.sprint_speed);
    flat + change
}

/// Push towards `wish`'s direction by at most `max_step` (acceleration × dt: the
/// game's accelerations are in units/s², not Source's speed-scaled factor), and
/// only until the speed in that direction reaches `wish`'s length. Speed you
/// already have in other directions is left alone.
fn accelerate(current: Vec3, wish: Vec3, max_step: f32) -> Vec3 {
    let wish_speed = wish.length();
    if wish_speed < 1e-3 {
        return current;
    }
    let dir = wish / wish_speed;
    let add = wish_speed - current.dot(dir);
    if add <= 0.0 {
        return current;
    }
    current + dir * add.min(max_step)
}

fn horizontal(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// Tilt a flat velocity to run along a slope, keeping its horizontal part, so
/// ramps don't change how fast you cover ground.
fn along_ground(flat: Vec3, normal: Vec3) -> Vec3 {
    flat - Vec3::Y * (flat.dot(normal) / normal.y)
}

/// Remove only the part of `v` that pushes into `normal`.
fn clip(v: Vec3, normal: Vec3) -> Vec3 {
    v - normal * v.dot(normal).min(0.0)
}

/// Slide along the surfaces hit this tick. If one surface works, use it; if we're
/// wedged between two (a corner, a wall meeting a ramp), follow their crease; if
/// three or more box us in, stop.
fn clip_to_planes(vel: Vec3, planes: &[Vec3]) -> Vec3 {
    let pushes_into = |v: Vec3, skip: &[usize]| {
        planes
            .iter()
            .enumerate()
            .any(|(k, p)| !skip.contains(&k) && v.dot(*p) < -1e-3)
    };
    for (i, &plane) in planes.iter().enumerate().rev() {
        let v = clip(vel, plane);
        if !pushes_into(v, &[i]) {
            return v;
        }
    }
    for i in 0..planes.len() {
        for j in i + 1..planes.len() {
            let crease = planes[i].cross(planes[j]).normalize_or_zero();
            let v = crease * vel.dot(crease);
            if !pushes_into(v, &[i, j]) {
                return v;
            }
        }
    }
    Vec3::ZERO
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn along_ground_keeps_horizontal_speed_and_follows_the_slope() {
        let ramp = Vec3::new(0.0, 1.0, 1.0).normalize();
        let v = along_ground(Vec3::new(0.0, 0.0, -100.0), ramp);
        assert!(v.dot(ramp).abs() < 1e-4);
        assert!((horizontal(v).length() - 100.0).abs() < 1e-3);
        assert!(v.y > 0.0, "walking up the ramp should climb: {v}");
    }

    #[test]
    fn crease_between_two_walls() {
        let a = Vec3::X;
        let b = Vec3::Z;
        // Pushing diagonally into a corner: nothing left once both walls are clipped.
        assert_eq!(
            clip_to_planes(Vec3::new(-1.0, 0.0, -1.0), &[a, b]),
            Vec3::ZERO
        );
        // Pushing into the corner while moving up slides up the crease.
        let v = clip_to_planes(Vec3::new(-1.0, 1.0, -1.0), &[a, b]);
        assert!(v.abs_diff_eq(Vec3::Y, 1e-4), "{v}");
    }

    #[test]
    fn accelerate_never_pushes_past_the_wished_speed() {
        let v = accelerate(
            Vec3::new(300.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, -60.0),
            500.0,
        );
        // Sideways momentum is kept; the new direction only gets up to 60.
        assert!(
            (v.x - 300.0).abs() < 1e-3 && (v.z + 60.0).abs() < 1e-3,
            "{v}"
        );
        let v = accelerate(
            Vec3::new(300.0, 0.0, 0.0),
            Vec3::new(200.0, 0.0, 0.0),
            500.0,
        );
        assert_eq!(v.x, 300.0);
    }

    #[test]
    fn decelerate_keeps_what_you_push_for_and_bleeds_the_rest() {
        let step = 1500.0 / 60.0;
        // Pushing the way you move, below the wished speed: nothing lost.
        let v = decelerate(Vec3::new(100.0, 0.0, 0.0), Vec3::new(200.0, 0.0, 0.0), step);
        assert!((v.x - 100.0).abs() < 1e-3, "{v}");
        // Over the wished speed: only the excess shrinks.
        let v = decelerate(Vec3::new(300.0, 0.0, 0.0), Vec3::new(200.0, 0.0, 0.0), step);
        assert!((v.x - (300.0 - step)).abs() < 1e-3, "{v}");
        // No input: a straight-line stop at 1500 u/s², about 0.18 s from sprint.
        let mut v = Vec3::new(270.0, 0.0, 0.0);
        let mut ticks = 0;
        while v.x > 0.0 {
            v = decelerate(v, Vec3::ZERO, step);
            ticks += 1;
        }
        assert_eq!(ticks, 11);
    }
}
