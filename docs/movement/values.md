# Movement values

Every number in [`crates/pf-game/assets/movement.ron`](../../crates/pf-game/assets/movement.ron) is tagged with its source there. Units: game units (~1 inch), seconds, degrees.

**Sources, best first:**
- **set**: the pilot settings files on the owner's PC discs, `scripts/players/mp/pilot_*.set` (KeyValues data; every standard pilot class agrees). Extract with `pf-pc <disc1.iso> common-extract scripts/players/ <out>`.
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
