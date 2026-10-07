# Titanfall 1 engine movement values (clean-room notes)

Read on 2026-10-07 from the owner's own copy of the PC game (R1Delta install, `r1/bin/x64_retail/client.dll`), as allowed by CLAUDE.md. These are **facts**: setting names, default values and the game's own one-line descriptions, read from the setting registrations and the player-settings field table. No code was copied; our Rust is written from these notes.

The DLL isn't packed or encrypted; nothing was circumvented.

## Ground and air (engine console variables and player settings defaults)

| Setting | Default | Game's description / what it means for us |
|---|---|---|
| `sv_gravity` | 750 | World gravity, units/s² |
| `sv_friction` | 4 | World friction (Source-style: speed lost per second = max(speed, stopspeed) × friction) |
| `sv_stopspeed` | 100 | Minimum stopping speed when on ground |
| `acceleration` (stand) | 2500 | "How quickly the player can get to full speed (units/sec/sec)" |
| `deceleration` (stand) | -1 | "How quickly the player can come to a stop". -1 means unset, so friction does the stopping |
| `airSpeed` | 60 | "Max speed player can adjust to in the air" (Source-style air control cap) |
| `airAcceleration` | 500 | "Acceleration when in the air (units/sec/sec)" |
| `player_extraairaccelleration` | 0.2 | Extra air acceleration even at max speed (not used yet) |
| `speedScaleSide` / `speedScaleBack` | 1 / 1 | Strafe and backward speed scales |
| `jump_graceperiod` | 0.2 | "Extra time during which a player can jump after falling off a ledge" |
| `jump_keyboardgraceperiodmin/max` | 0.2 / 0.5 | Time after a jump during which keyboard input can still change direction (full strength, then fading to 0) |
| `jump_keyboardgrace_strength` / `_max` | 0.7 / 0.7 | How strongly, and how much (fraction of sprint speed) |
| `sprinttiltMaxRoll` | 10 (pilot `.set`: 2) | Degrees of view tilt when turning while sprinting |
| `viewkickFallDistMin/Max/MaxScale` | 10 / 30 / 6 (pilot `.set`: 10 / 70 / 12) | Landing view kick range |

Pilot `.set` files override some of these (speeds, jump height 80, step height 18, hull; see `values.md`).

## Wall-run (`wallrun_*` console variables)

| Setting | Default | Meaning |
|---|---|---|
| `wallrun_upWallBoost` | 250 | Initial upward speed when a wall-run starts |
| `wallrun_gravityRampUpTime` | 0.7 | Gravity starts at 0 and ramps to full over this many seconds |
| `wallrun_maxSpeedHorizontal` | 240 | Max horizontal speed while wall-running |
| `wallrun_maxSpeedHorizontalBackward` | 50 | Max reverse speed |
| `wallrun_accelerateHorizontal` | 7 | Horizontal acceleration (Source-style factor) |
| `wallrun_friction` | 4 | Friction while wall-running |
| `wallrun_maxSpeedVertical` | 300 | Max vertical speed |
| `wallrun_accelerateVertical` | 450 | Vertical acceleration; "fights with gravity" |
| `wallrun_upwardAutoPush` | 0.65 | Automatic up-the-wall input when pushing forward along the wall |
| `wallrun_sliptime` | 0.7 | Time you slip before falling off |
| `wallrun_slipvel` | 70 | Downward impulse when slipping starts |
| `wallrun_slipslowdown` | 0.5 | Fraction of velocity lost when slipping starts |
| `wallrun_fallAwaySpeed` | 100 | Speed away from the wall when falling off |
| `wallrun_jumpUpSpeed` | 250 | Vertical speed when jumping out of a wall-run |
| `wallrun_jumpOutwardSpeed` | 150 | Speed away from the wall when jumping out |
| `wallrun_jumpInputDirSpeed` | 100 | Speed in the input direction when jumping out |
| `wallrun_strengthLossStart` / `End` | 3 / 5 | After this many wall-runs (without landing) the upward boost, jump-up speed and gravity ramp start / finish fading to 0 |
| `wallrun_sameWallDot` | 0.9 | Same-wall test (normal dot product) |
| `wallrun_sameWallDist` | 100 | Within this distance of the previous run on the same wall, no higher re-grab |
| `wallrun_minAngle_air` | 180 | Any approach angle can start a wall-run |
| `wallrun_onlyHighWalls` | 1 | No wall-run on walls low enough to see over |
| `wallrun_angleChangeMinCos` | 0.8 | Wall turning away by more than this ends the run |
| `wallrun_maxViewTilt` / `viewTiltSpeed` | 15 / 6 | View roll in degrees, and its speed |
| `wallrun_repelTimeMin/Max` | 0.2 / 0.4 | After a wall jump, pushed away from that wall for this long |
| `wallrun_hangStopTime` | 0.5 | "Length of time to come to a stop when **zooming**": wall-hang is triggered by aiming down sights |
| `wallrun_hangsliptime` / `hangslipvel` | 1.0 / 70 | Slip time and impulse at the end of a wall-hang |
| `wallrun_timeLimit` / `wallrun_hangTimeLimit` | player settings; no default found in the table. The Titanfall wiki gives 1.75 s / 11 s (3.5 / 22 with the Enhanced Parkour Kit, whose `.set` mod adds to both) |

## Auto-mantle (`automantle_*`)

| Setting | Default | Meaning |
|---|---|---|
| `automantle_height` | 80 | Highest ledge (above the feet) that can be mantled |
| `automantle_searchdist` | 5 | Forward distance to look for a ledge |
| `automantle_forwarddist` | 30 | Forward distance for the ground check on top |
| `automantle_mindist` | 18 | Minimum forward distance moved |
| `automantle_maxangle_push` | 50 | Max angle between input and the wall's inward normal |
| `automantle_maxangle_view` | 65 | Max angle between view and the wall (45 while wall-running) |
| `automantle_cooldown` | 0.5 | Seconds between mantles |
| `automantle_height_below/level/above` | -10 / 10 / 30 | Eye height relative to the ledge picks the animation: below -10 "below", up to 10 "level", up to 30 "above", higher "high" |
| `automantle_duration_below/level/above/high` | 1.11 / 1.0 / 0.5 / 0.35 | Mantle durations for those cases |

## Still unknown

- Double jump height (not a stored setting; probably computed in code).
- The exact order of friction and acceleration inside a tick, and how `acceleration` is applied (we use the Source order: friction, then accelerate up to the wished speed).
