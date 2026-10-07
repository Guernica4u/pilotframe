# Movement values

Every number in [`crates/pf-game/assets/movement.ron`](../../crates/pf-game/assets/movement.ron), where it came from, and how sure we are. Units: game units (~1 inch), seconds, degrees.

**Sources:**
- **set**: the pilot settings files on the owner's PC discs, `scripts/players/mp/pilot_*.set` in the `mp_common` VPK (KeyValues data). Every standard pilot class has the same movement values. The "fastest" and "quickdraw" variants differ slightly and aren't used. To extract them, run `pf-pc <disc1.iso> common-extract scripts/players/ <out>`.
- **guess**: a placeholder picked to feel right in the greybox. These still need measuring. The engine's own defaults (gravity, wall-run speed and timing, air control) live in the executable, which we don't touch, so they have to come from footage: the training mission and Colony clips in `Resources/reference/video/` (PC, 1080p60). The task is written up in [docs/tasks/measure-movement.md](../tasks/measure-movement.md).

| Value | Current | Source | Confidence | How to measure / notes |
|---|---|---|---|---|
| `capsule_radius` | 16 | set: `hull_min/max` ±16 | high | |
| `capsule_height` | 72 | set: stand `hull_max` z | high | |
| `eye_height` | 60 | set: stand `viewheight` | high | |
| `crouch_height` | 47 | set: crouch `hull_max` z | high | |
| `crouch_eye_height` | 38 | set: crouch `viewheight` | high | |
| `walk_speed` | 180.6 | set: stand `speed` | high | |
| `sprint_speed` | 269.85 | set: stand `sprintspeed` | high | |
| `crouch_speed` | 89.25 | set: crouch `speed` | high | |
| `jump_height` | 80 | set: `jumpheight` | medium | We assume it means the apex height. Check against a standing jump next to known geometry. |
| `step_height` | 18 | set: `stepheight` | high | |
| `ground_accel` | 1400 | guess | none | Frames from standstill to top speed |
| `ground_friction` | 1600 | guess | none | Frames from top speed to a stop after releasing the stick |
| `air_accel` | 300 | guess | none | How fast direction changes mid-air |
| `gravity` | 750 | guess | none | Fit a parabola to jump height over time |
| `double_jump_height` | 60 | guess | none | Height gained by the second jump |
| `wallrun_speed` | 340 | guess | none | Speed along a long wall after ~1 s of running |
| `wallrun_accel` | 600 | guess | none | Frames to reach that speed |
| `wallrun_min_speed` | 150 | guess | none | Slowest approach that still catches a wall |
| `wallrun_gravity` | 150 | guess | none | Height lost over a full wall-run |
| `wallrun_max_lift` | 150 | guess | none | Rise at the start of a wall-run entered while jumping up |
| `wallrun_max_fall_speed` | 450 | guess | none | Falling past a wall fast: does it still catch you? |
| `wallrun_time_limit` | 1.75 | wiki: Enhanced Parkour Kit page (3.5 s with the kit) | medium | The kit's settings mod is `pas_wall_runner`. Confirm by frame count on a long wall. |
| `wall_jump_height` | 60 | guess | none | Height gained jumping off a wall |
| `wall_jump_push` | 280 | guess | none | Sideways distance covered after a wall jump |
| `wall_hang_time_limit` | 11 | wiki: Enhanced Parkour Kit page (22 s with the kit) | medium | |
| `mantle_reach` | 70 | guess | none | Tallest ledge mantled from a jump |
| `mantle_time` | 0.35 | guess | none | Frames from ledge grab to standing |
| `max_slope_deg` | 45 | guess | none | Steepest ramp in the maps you can walk up |

Other values from the settings files that aren't used yet: `fov 70`, `health 200`, `ziplineSpeed 600`, `hardFallDist 20`, `impactSpeed 380`, `meleeLungeSpeed 200`, `sprinttiltMaxRoll 2` (a camera roll while sprinting).

The training mission's prompts say: "SPRINT, then JUMP at an angle toward a wall to start wallrunning. HOLD [forward] to run along the wall" (so letting go of forward drops you off), and for mantling, jump and hold forward. The training video's wall-run lessons all end at the wall's end or with a jump-off, so it can't show the time limit.

How the moves are triggered (crouch in the air next to a wall to hang, steering away to drop off a wall-run) follows the training mission's prompts. Check it against the training video too.

Not movement tuning, but also unmeasured: the simulation tick rate (60 Hz, `TICK_HZ` in `pf-game`) and the wall-run camera tilt (12°, `WALLRUN_ROLL_DEG` in `pf-game/src/player.rs`).
