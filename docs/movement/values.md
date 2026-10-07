# Movement values

Every number in [`crates/pf-game/assets/movement.ron`](../../crates/pf-game/assets/movement.ron) is tagged with its source there. Units: game units (~1 inch), seconds, degrees.

**Sources, best first:**
- **set**: the pilot settings files on the owner's PC discs, `scripts/players/mp/pilot_*.set` (KeyValues data; every standard pilot class agrees). Extract with `pf-pc <disc1.iso> common-extract scripts/players/ <out>`.
- **engine**: the engine's own defaults, read from the owner's copy of `client.dll` (R1Delta install), clean-room. Names, values and the game's descriptions are in [r1-engine-values.md](r1-engine-values.md).
- **wiki**: the Titanfall fandom wiki ([Enhanced Parkour Kit](https://titanfall.fandom.com/wiki/Enhanced_Parkour_Kit)): wall-run 1.75 s, wall-hang 11 s (double with the kit).
- **guess**: still to measure.

## Still guessed

| Value | Current | Notes |
|---|---|---|
| `double_jump_height` | 60 | Not a stored setting; probably computed in code. Next place to look: the double-jump code in `client.dll`. |
| `wallrun_min_speed` | 100 | The engine allows any approach angle (`wallrun_minAngle_air` 180); we still need some speed along the wall to pick a direction. |
| `max_slope_deg` | 45 | Steepest walkable slope. |

## How the model works (from the engine settings' descriptions)

- **Ground:** Source-style: friction (`sv_friction` 4, `sv_stopspeed` 100) every tick, then accelerate (2500 u/s²) towards the wished speed. `deceleration` is unset (-1), so friction does all the stopping: from sprint you slide to a stop over about half a second.
- **Air:** you can only add speed up to `airSpeed` (60) in the direction you steer, at `airAcceleration` (500 u/s²). Momentum above that is kept, never added to. That's what makes jumps commit you.
- **Jump grace:** 0.2 s after walking off a ledge you can still jump (`jump_graceperiod`).
- **Wall-run:** starts with an upward boost (250); gravity ramps from 0 to full over 0.7 s; holding forward gives upward push (450 × 0.65); along the wall, friction 4 and acceleration 7 settle you at 240 u/s. After the time limit you slip for 0.7 s (a 70 u/s kick down, half your speed lost), then fall off with 100 u/s away from the wall. After 3 wall-runs without landing, the boost fades out by the 5th.
- **Wall jump:** 250 up, 150 away from the wall, 100 towards your input, plus the speed you had along the wall.
- **Wall-hang:** aim down sights on a wall. You stop over 0.5 s, hang up to 11 s, then slip for 1 s.
- **Mantle:** ledges up to 80 above the feet, only while jumping, facing within 65° (45° when wall-running). It takes 0.35 to 1.11 s depending on how high the ledge is compared to your eyes.

Other unused values from the settings files: `fov 70`, `health 200`, `ziplineSpeed 600`, `hardFallDist 20`, `impactSpeed 380`, `meleeLungeSpeed 200`, `sprinttiltMaxRoll 2`.

Camera, in `pf-game/src/player.rs`: wall-run view tilt 15° (`wallrun_maxViewTilt`), plus our own landing dip and head bob (to be replaced by the engine's viewkick values).
