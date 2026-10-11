//! Weapon spread (accuracy cone), ported from the game: the per-tick update the
//! player runs on its weapon, the kick on each shot, its decay, and the value the
//! crosshair and bullets read. Notes: docs/port/hud.md ("Weapon spread").
//!
//! Spread is an angle in degrees. It is a base (from how you're moving) plus a
//! firing kick (separate for hip fire and aiming, blended by how far you're aimed).

/// The weapon script's spread settings (`spread_*`). Unset values (negative, the
/// game's default) are filled in the way the game does when it reads them.
#[derive(Clone, Debug, PartialEq)]
pub struct SpreadData {
    stand_hip: f32,
    stand_hip_run: f32,
    stand_hip_sprint: f32,
    stand_ads: f32,
    crouch_hip: f32,
    crouch_ads: f32,
    air_hip: f32,
    air_ads: f32,
    /// Most the base spread moves towards its target per tick.
    lerp_speed: f32,
    /// Kick per shot, and its cap: [stand, crouch, air] for hip and for aiming.
    kick_hip: [f32; 3],
    kick_ads: [f32; 3],
    max_kick_hip: [f32; 3],
    max_kick_ads: [f32; 3],
    /// Degrees per second the kick shrinks, after `decay_delay` seconds of not firing.
    decay_rate: f32,
    decay_delay: f32,
}

impl SpreadData {
    /// From the script's values; `get(key)` returns the number if the key is set.
    /// Defaults are the game's own (its weapon setting table).
    pub fn from_script(get: impl Fn(&str) -> Option<f32>) -> Self {
        let unset = |k: &str| get(k).unwrap_or(-1.0);
        let zero = |k: &str| get(k).unwrap_or(0.0);
        let stand_hip = get("spread_stand_hip").unwrap_or(5.0);
        let or_stand = |v: f32| if v < 0.0 { stand_hip } else { v };
        let run = unset("spread_stand_hip_run");
        let sprint = unset("spread_stand_hip_sprint");
        let three = |prefix: &str, suffix: &str| {
            ["stand", "crouch", "air"].map(|s| zero(&format!("{prefix}_{s}_{suffix}")))
        };
        Self {
            stand_hip,
            stand_hip_run: or_stand(run),
            // An unset sprint spread falls back to the run spread, then standing.
            stand_hip_sprint: if sprint >= 0.0 { sprint } else { or_stand(run) },
            stand_ads: or_stand(unset("spread_stand_ads")),
            crouch_hip: or_stand(unset("spread_crouch_hip")),
            crouch_ads: or_stand(unset("spread_crouch_ads")),
            air_hip: or_stand(unset("spread_air_hip")),
            air_ads: or_stand(unset("spread_air_ads")),
            lerp_speed: get("spread_lerp_speed").unwrap_or(1.25),
            kick_hip: three("spread_kick_on_fire", "hip"),
            kick_ads: three("spread_kick_on_fire", "ads"),
            max_kick_hip: three("spread_max_kick", "hip"),
            max_kick_ads: three("spread_max_kick", "ads"),
            decay_rate: zero("spread_decay_rate"),
            decay_delay: zero("spread_decay_delay"),
        }
    }
}

/// What the player is doing, as the game hands it to the weapon each tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct SpreadInput {
    /// Horizontal speed, units/s.
    pub speed: f32,
    /// The pilot's walking speed: the run spread applies fully from here up.
    pub run_speed: f32,
    pub sprinting: bool,
    /// How far crouched and how far aimed in, 0..1.
    pub crouch: f32,
    pub ads: f32,
    /// On the ground, or on a wall (wall-running counts as ground here).
    pub grounded: bool,
    /// On the ground itself (for the firing kick: stand / crouch / air).
    pub on_ground: bool,
}

/// A weapon's spread state.
#[derive(Clone, Debug, Default)]
pub struct Spread {
    /// The movement part, easing towards its target.
    base: f32,
    /// Firing kicks, hip and aiming.
    kick_hip: f32,
    kick_ads: f32,
    /// When the last shot was fired, seconds of game time.
    last_shot: f32,
}

impl Spread {
    /// The spread right now, degrees: the base plus the kick for how far aimed in.
    pub fn degrees(&self, ads: f32) -> f32 {
        self.base + (self.kick_ads - self.kick_hip) * ads + self.kick_hip
    }

    /// One player tick: the base eases towards the spread for what you're doing.
    pub fn tick(&mut self, d: &SpreadData, i: &SpreadInput) {
        let target = if i.sprinting {
            d.stand_hip_sprint
        } else if !i.grounded {
            d.air_hip + (d.air_ads - d.air_hip) * i.ads
        } else {
            // Standing: from the still spread to the run spread with speed.
            let stand = if i.run_speed == 0.0 {
                if i.speed >= 0.0 {
                    d.stand_hip_run
                } else {
                    d.stand_hip
                }
            } else {
                let f = (i.speed / i.run_speed).clamp(0.0, 1.0);
                d.stand_hip + (d.stand_hip_run - d.stand_hip) * f
            };
            let (a, c) = (i.ads, i.crouch);
            (1.0 - a) * (1.0 - c) * stand
                + (1.0 - c) * a * d.stand_ads
                + (1.0 - a) * c * d.crouch_hip
                + c * a * d.crouch_ads
        };
        self.base = target.clamp(self.base - d.lerp_speed, self.base + d.lerp_speed);
    }

    /// A shot fired at time `now`: kick the spread (the hip kick only while less
    /// than halfway aimed in).
    pub fn fire(&mut self, d: &SpreadData, i: &SpreadInput, now: f32) {
        self.last_shot = now;
        let stance = if !i.on_ground {
            2
        } else if i.crouch > 0.0 {
            1
        } else {
            0
        };
        if i.ads < 0.5 {
            self.kick_hip = (self.kick_hip + d.kick_hip[stance]).min(d.max_kick_hip[stance]);
        }
        self.kick_ads = (self.kick_ads + d.kick_ads[stance]).min(d.max_kick_ads[stance]);
    }

    /// Kicks shrink once you've stopped firing for a moment.
    pub fn decay(&mut self, d: &SpreadData, now: f32, dt: f32) {
        if self.last_shot + d.decay_delay <= now {
            self.kick_hip = (self.kick_hip - d.decay_rate * dt).max(0.0);
            self.kick_ads = (self.kick_ads - d.decay_rate * dt).max(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The R-201's values (its script).
    fn r201() -> SpreadData {
        SpreadData::from_script(|k| {
            Some(match k {
                "spread_stand_hip" => 5.5,
                "spread_stand_hip_run" => 8.0,
                "spread_stand_hip_sprint" => 11.0,
                "spread_crouch_hip" => 4.5,
                "spread_air_hip" => 11.0,
                "spread_stand_ads" | "spread_crouch_ads" => 0.35,
                "spread_air_ads" => 0.5,
                "spread_kick_on_fire_stand_hip" => 0.5,
                "spread_max_kick_stand_hip" => 8.0,
                "spread_decay_delay" => 0.1,
                "spread_decay_rate" => 4.0,
                _ => return None,
            })
        })
    }

    fn settle(s: &mut Spread, d: &SpreadData, i: &SpreadInput) {
        for _ in 0..60 {
            s.tick(d, i);
        }
    }

    #[test]
    fn movement_sets_the_base() {
        let d = r201();
        let mut s = Spread::default();
        let still = SpreadInput {
            run_speed: 180.6,
            grounded: true,
            on_ground: true,
            ..Default::default()
        };
        settle(&mut s, &d, &still);
        assert_eq!(s.degrees(0.0), 5.5);
        settle(
            &mut s,
            &d,
            &SpreadInput {
                speed: 180.6,
                ..still
            },
        );
        assert_eq!(s.degrees(0.0), 8.0);
        settle(
            &mut s,
            &d,
            &SpreadInput {
                sprinting: true,
                ..still
            },
        );
        assert_eq!(s.degrees(0.0), 11.0);
        settle(
            &mut s,
            &d,
            &SpreadInput {
                crouch: 1.0,
                ..still
            },
        );
        assert_eq!(s.degrees(0.0), 4.5);
        settle(
            &mut s,
            &d,
            &SpreadInput {
                grounded: false,
                ..still
            },
        );
        assert_eq!(s.degrees(0.0), 11.0);
    }

    #[test]
    fn base_eases_at_lerp_speed() {
        let d = r201();
        let mut s = Spread::default();
        s.tick(
            &d,
            &SpreadInput {
                grounded: true,
                ..Default::default()
            },
        );
        assert_eq!(s.degrees(0.0), 1.25);
    }

    #[test]
    fn firing_kicks_then_decays() {
        let d = r201();
        let mut s = Spread::default();
        let i = SpreadInput {
            on_ground: true,
            ..Default::default()
        };
        for shot in 0..40 {
            s.fire(&d, &i, shot as f32 * 0.1);
        }
        assert_eq!(s.kick_hip, 8.0, "capped at the max kick");
        // The last shot was at 3.9 s; the decay waits 0.1 s.
        s.decay(&d, 3.95, 1.0);
        assert_eq!(s.kick_hip, 8.0, "not before the delay");
        s.decay(&d, 4.1, 1.0);
        assert_eq!(s.kick_hip, 4.0);
    }
}
