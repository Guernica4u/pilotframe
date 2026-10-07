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
/// Moving away from the ground faster than this (units/s) means we've left it.
const LIFTOFF_SPEED: f32 = 1.0;
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
    /// Normal of the wall we last came off. That wall can't be wall-run again until
    /// we land or touch a different one, so you can't climb a wall by re-grabbing it.
    pub last_wall: Option<Vec3>,
}

/// What the pilot is doing beyond plain walking, falling and jumping.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    /// On the ground or in the air.
    Free,
    /// Running along a wall. `normal` points out of the wall, flat.
    WallRun { normal: Vec3, time: f32 },
    /// Holding still on a wall.
    WallHang { normal: Vec3, time: f32 },
    /// Climbing over a ledge from `from` to `to` (feet positions).
    Mantle { from: Vec3, to: Vec3, time: f32 },
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
    /// The jump button went down since the last tick.
    pub jump: bool,
    /// Sprint is held. Only counts while moving forwards.
    pub sprint: bool,
    /// Crouch is held. In the air next to a wall, this hangs on it.
    pub crouch: bool,
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
    // Duck at once; stand back up only when there's room overhead.
    s.crouched = input.crouch || (s.crouched && !Body::new(tuning, false).fits(world, s.position));
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
        Mode::Mantle { from, to, time } => parkour::mantle(s, from, to, time + dt, tuning),
    }
}

/// Walking, jumping and falling, plus the checks that start wall-runs, wall-hangs
/// and mantles.
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
    if ground.is_some() && input.jump {
        vel.y = tuning.jump_speed();
        ground = None;
        jumped = true;
    }

    if let Some(normal) = ground {
        let rate = if wish == Vec3::ZERO {
            tuning.ground_friction
        } else {
            tuning.ground_accel
        };
        let intended = along_ground(move_towards(horizontal(vel), wish, rate * dt), normal);

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
        }
        let flat = air_steer(horizontal(vel), wish, tuning.air_accel * dt);
        vel = Vec3::new(flat.x, vel.y, flat.z);
        // Half the gravity before the move and half after puts every tick exactly on
        // the true parabola, so jump height doesn't depend on the tick rate.
        vel.y -= tuning.gravity * dt * 0.5;
        let air = body.slide(world, pos, vel, dt, false);
        (pos, vel) = (air.pos, air.vel);
        vel.y -= tuning.gravity * dt * 0.5;
    }

    pos = body.push_out(world, pos);
    let ground = body.find_ground(world, pos, vel);
    if let Some(normal) = ground {
        vel = along_ground(horizontal(vel), normal);
        s.air_jumps = 0;
        s.last_wall = None;
    }
    s.position = pos;
    s.velocity = vel;
    s.on_ground = ground.is_some();
    if !s.on_ground && !jumped {
        parkour::try_start(&mut s, input, tuning, body, world);
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
        (vel.dot(ground.normal) <= LIFTOFF_SPEED).then_some(ground.normal)
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
            _ if self.is_ground(hit.normal) => Some(Ground {
                normal: hit.normal,
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
            if !self.is_ground(normal) {
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

/// Air control: steer towards `wish` without ever gaining speed beyond the faster
/// of the current speed and the wished-for speed. Placeholder until M5.
fn air_steer(current: Vec3, wish: Vec3, accel: f32) -> Vec3 {
    if wish == Vec3::ZERO {
        return current;
    }
    let cap = current.length().max(wish.length());
    (current + wish.normalize() * accel).clamp_length_max(cap)
}

fn horizontal(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// Tilt a flat velocity to run along a slope, keeping its horizontal part, so
/// ramps don't change how fast you cover ground.
fn along_ground(flat: Vec3, normal: Vec3) -> Vec3 {
    flat - Vec3::Y * (flat.dot(normal) / normal.y)
}

fn move_towards(current: Vec3, target: Vec3, max_delta: f32) -> Vec3 {
    let delta = target - current;
    let dist = delta.length();
    if dist <= max_delta || dist < 1e-6 {
        target
    } else {
        current + delta / dist * max_delta
    }
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
    fn air_steer_never_adds_speed() {
        let v = air_steer(
            Vec3::new(300.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, -100.0),
            50.0,
        );
        assert!(v.length() <= 300.0 + 1e-3);
        assert!(v.z < 0.0);
    }
}
