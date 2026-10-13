# Movement values

Every number in [`crates/pf-game/assets/movement.ron`](../../../crates/pf-game/assets/movement.ron) is tagged with its source there. Units: game units (~1 inch), seconds, degrees.

**Sources, best first:**
- **set**: the pilot settings files, `scripts/players/mp/pilot_*.set` (KeyValues data; every standard pilot class agrees). Extract with `pf-vpk extract <englishclient_mp_common…_dir.vpk> scripts/players/ <out>`.
- **engine**: the engine's own defaults, read from the owner's copy of `client.dll` (R1Delta install), clean-room. Names, values and the game's descriptions are in [r1-engine-values.md](r1-engine-values.md).
- **wiki**: the Titanfall fandom wiki ([Enhanced Parkour Kit](https://titanfall.fandom.com/wiki/Enhanced_Parkour_Kit)): wall-run 1.75 s, wall-hang 11 s (double with the kit). **Wrong:** the live game uses 2.0 s / 9.0 s.
- **mp**: multiplayer server values, which replace several client defaults (see `r1-engine-values.md`, "Multiplayer server values").
- **guess**: still to measure.

## Still guessed

| Value | Current | Notes |
|---|---|---|
| `double_jump_height` | 60 | Not a stored setting; probably computed in code. Next place to look: the double-jump code in `client.dll`. |
| `wallrun_min_speed` | 100 | The engine allows any approach angle (`wallrun_minAngle_air` 180); we still need some speed along the wall to pick a direction. |
| `max_slope_deg` | 45 | Steepest walkable slope. |

## How the model works

Read from the game's own movement code; the full plain-language write-up is in [r1-engine-values.md](r1-engine-values.md) under "Behaviour". In short:

- **Ground:** the speed you're pushing for is kept, everything else bleeds off linearly at 1500 u/s² (0.6 × acceleration), then you accelerate at 2500 u/s². From a sprint you stop in about 0.2 s.
- **Air:** you can only add speed up to 60 u/s in the direction you steer, at 500 u/s². Just after a jump, a newly pressed movement key turns your velocity towards it, keeping your speed (strong for 0.2 s, gone by 0.5 s).
- **Wall-run:** you have to hit the wall moving into it. The controls are rebuilt on the wall: look up to run up, strafe only goes up, holding forward adds some "up". Wall friction (4/s) acts on the whole velocity, and the push along the wall is 1500 u/s² (towards up to 450 u/s). Together they settle at about 375 u/s, so a sprint onto a wall speeds up (measured live). Up/down the wall the push is 450 u/s², towards up to 300 u/s. Gravity ramps in over 0.7 s. The run slips for its last 0.7 s and ends at 2.0 s. Re-grabbing the same wall only works lower down and gets no boost.
- **Wall jump:** adds to your speed: 100 towards your input, at least 150 away from the wall, and up to 250 up.
- **Wall-hang:** aim down sights on a wall: speed and gravity fade to nothing over 0.5 s; slips in the last second of 11.
- **Mantle:** still built from the settings (ledges up to 80, 0.35 to 1.11 s); its code is next to read.

Other unused values from the settings files: `fov 70`, `health 200`, `ziplineSpeed 600`, `hardFallDist 20`, `impactSpeed 380`, `meleeLungeSpeed 200`, `sprinttiltMaxRoll 2`.

Camera (`pf-game/src/view.rs`, `player.rs`): view punches on jump, double jump, landing, wall-run and wall-hang on the engine's spring; sprint turn tilt; wall-run tilt 15° at speed 6; field of view in Source's convention (70 = 4:3 horizontal). Values in [r1-engine-values.md](r1-engine-values.md#view-camera-feel).

## Recorded in the game (2026-10-12)

One minute of play in R1Delta (stand, strafe, walk, sprint, jumps, double jumps, one short wall-run), recorded read-only with `telemetry.py record`, and replayed through our code with `PF_REPLAY` (docs/tools.md).

**Buttons** (`telemetry.py keys`): attack 1, jump 2, forward 8, back 16, left 512, right 1024, sprint 1<<17, aim 1<<19, crouch 1<<27. Crouch and aim are not Source's usual bits (4 and 2048). Use 32, reload 1<<13 and 1<<14 also showed up.

**The gun slows you.** The game's top ground speed is the settings speed × the held weapon's `move_speed_modifier` × a scale that reads 1.0 live (client.dll's max-speed function; it also returns 0 while a "frozen until" time hasn't passed). Most guns have 0.95238, including our default R-101C; a few (R-97, Car, autopistol) have 1.0. Ported: `MoveInput::weapon_speed`.

**Walk 180.5 and sprint 270.0, not 180.6 and 269.85.** The live `m_flMaxspeed` reads exactly 180.5. The steady recorded speeds are 171.905 (= 180.5 × 0.95238) and 257.143 (= 270.0 × 0.95238). A few frames show 172.0 and 257.0 (the file values), so something rounds the speed to steps of 0.5, maybe the networked value. Not found in the code yet. Crouch speed still needs a recording.

**Gravity 750**, confirmed (the vertical speed falls 750 u/s per second in the air).

**The client steps at the frame rate, not a fixed tick.** Frames were ~5 ms apart, each with its own dt, and position and speed fit half-gravity-before, half-after exactly. Our replay still uses a fixed tick, so it can't match frame for frame yet.

**Jump: about 1% lower than the formula.** The code (client.dll's jump function) adds `surface jumpfactor × up × sqrt(2 × sv_gravity × height)`; jumpfactor is 1.0 on normal surfaces. With height 80 that's 346.4 u/s, but from the first airborne frames the launch speed is 344.5–344.6 (apex ≈ 79.1 above the ground, every standing and running jump alike). The height comes from the player settings block (`client.dll+0xBF6EB0 + index × 0x2d40`, index from the player at +0x2ac0); the code picks one of two heights (block +0x88 or +0x90) depending on the speed and a convar. Next step: read those two fields live to see which value it really uses.
