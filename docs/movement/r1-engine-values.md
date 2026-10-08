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

## Multiplayer server values (override the defaults below)

The defaults in the tables below are the **client.dll** defaults. In multiplayer the server sends its own values to every player, and these replace the defaults. They come from Respawn's `cfg/server/listenserver.cfg` (the `if_mp` column), which R1Delta ships with a note that the values were captured from the real servers' `NET_SetConVar` messages. On 2026-10-08 they were confirmed by reading the running game (R1Delta, local, read-only):

| Name | MP value | Client default | Campaign (`if_sp`) |
|---|---|---|---|
| `wallrun_maxSpeedHorizontal` | **450** | 240 | 280 |
| `wallrun_accelerateHorizontal` | **1500** u/s² | 7 | 1500 |
| `wallrun_jumpUpSpeed` | **320** | 250 | 160 |
| `wallrun_jumpOutwardSpeed` | **250** | 150 | 200 |
| `wallrun_fallAwaySpeed` | **70** | 100 | 70 |
| `wallrun_strengthLossStart` / `End` | **1000 / 1000** (upward help never fades) | 3 / 5 | 3 / 5 |
| `wallrun_gravityRampUpTime` | 0.7 | 0.7 | 2.5 |
| `wallrun_maxSpeedVertical` / `accelerateVertical` | 300 / 450 | 300 / 450 | 120 / 100 |
| `automantle_maxangle_view` | **50** | 65 | 50 |
| `superjump_min_height` = `max_height` | **80** (double jump) | | 0 / 114 |
| `superjump_horzspeed` | 240 | | 60 |
| `superjump_limit` | 1 | | 3 |

The player's own limits, read live from the player object: **wall-run 2.0 s**, **wall-hang 9.0 s** (the wiki's 1.75 s / 11 s are wrong).

**Measured wall-run (live recording, 10 runs):** on contact only the into-the-wall part of the velocity is removed, so hitting a wall head-on stops you nearly dead, in the real game too. Speed along the wall then rises with rate ≈ 4/s towards ≈ 370 u/s (1500 / 4 = 375, where the push balances friction): 303 → 337 → 353 → 363 u/s at 0.1 / 0.3 / 0.5 / 1.0 s. Vertical speed rises by ~150–200 u/s on contact, then sinks to about −55 u/s by 1 s.

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
| `wallrun_timeLimit` / `wallrun_hangTimeLimit` | player settings: **2.0 s / 9.0 s** read live (the wiki's 1.75 s / 11 s are wrong; the Enhanced Parkour Kit mod adds to both) |

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

## View (camera feel)

Defaults are loaded in the instruction just before each setting's name where it's registered, which is how these were read.

| Setting | Default | Meaning |
|---|---|---|
| `viewpunch_base_springConstant{X,Y,Z}` | 65 | "Bigger number increases the speed at which the view corrects" |
| `viewpunch_base_springDamping{X,Y,Z}` | 9 | "Bigger number makes the response more damped" |
| `viewpunch_jump_*` | base pitch 2, yaw 0, roll 0.9; random 1, 1, 0.5 | Kick on a ground jump (`viewpunch_jump` 1 enables it) |
| `viewpunch_doublejump_*` | base 4.5, 0, 0; random 1, 1, 2 | Double jump |
| `viewpunch_wallrun_*` | base -1.5, 0, 5; random 0.5 each | Starting a wall-run |
| `viewpunch_wallhangStart_*` | base 4, 2, 4; random 1, 1, 0.5 | Starting a wall-hang |
| `viewpunch_wallhangEndWarning_*` | base -1, 0, -2; random 0.5 each | Hang about to end (not used yet) |
| `viewpunch_land_*` | base 1.5, 0, 0; random 0.5, 0, 1.0 | Landing |
| `viewpunch_zipline_*` | base 10, 0, 0; random 3, 10, 10 | Zipline (not used yet) |
| `sprinttilt_accel` / `maxvel` / `turnrange` | 35 / 2 / 120 | Sprint turn tilt: fraction acceleration, max speed, turn rate (deg/s) for full tilt |
| pilot `fov` | 70 | Source convention: horizontal degrees on a 4:3 screen (≈55° vertical) |

Our reading (not stated by the game): a punch is an angular kick on that spring, with its starting speed = punch × √65. Landing scales the land punch from 0 at a 10-unit fall to full at 70 (pilot `viewkickFallDistMin/Max`); what `viewkickFallDistMaxScale` (12) multiplies is still unknown.

## Still unknown

- Double jump height (not a stored setting; probably computed in code).
- The exact order of friction and acceleration inside a tick, and how `acceleration` is applied (we use the Source order: friction, then accelerate up to the wished speed).

## Behaviour (from reading `client.dll`, 2026-10-07)

Plain-language notes on what the game's movement code does. Our Rust is written from these notes, not from the disassembly. Addresses are in the owner's `client.dll` (R1Delta build) for re-checking.

### Ground (`0x180339e49` slowdown, `0x180340b80` walk, `0x1801654e0`/`0x1801655b0` accel/decel getters)

- **Acceleration** comes from the stance settings: `sprintAcceleration` if set and you're faster than `speed`, else `lowAcceleration` if set and slower than `lowSpeed`, else `acceleration` (2500). Scaled by a per-player multiplier (normally 1) and a footstep factor (`footstepAccelReduce`, 1).
- **Deceleration**: `sprintDeceleration`/`deceleration` if set, else **0.6 × acceleration** (1500 for pilots). It's linear, not Source friction.
- Each tick: the part of your velocity along the wished direction, up to the wished speed, is kept; everything else (sideways drift, excess speed) is slowed by `deceleration × dt`. Then `Accelerate` adds up to `acceleration × dt` towards the wished direction, never past the wished speed.
- **`Accelerate` is not Source's**: the step is `accel × dt × surface friction`. It is *not* multiplied by the wished speed, so accelerations are in units/s².

### Air (`0x18033f250` air move, `0x18033f020` air accelerate)

- Wished speed = min(input length, 1) × `airSpeed` (60) × speed scale; acceleration = `airAcceleration` (500), using the same units/s² `Accelerate`. When already at or above the wished speed in that direction, a tiny extra push of `player_extraairaccelleration` × dt (0.2) is added instead.
- **Keyboard jump grace**: on the tick a movement key is newly pressed, within `jump_keyboardgraceperiodmax` (0.5 s) of the last jump, the horizontal velocity is turned towards the held keys' direction while keeping its speed. Strength is `jump_keyboardgrace_strength` (0.7) up to `jump_keyboardgraceperiodmin` (0.2 s), fading to 0 at 0.5 s. The change is capped at `jump_keyboardgrace_max` (0.7) × sprint speed.

### Wall-run (`0x18034129f` input and physics, `0x180165c01` start test, `0x18016c2b0` begin, `0x18016b990` wall jump, `0x18016b3b0` slip/fall, `0x180160d10` gravity scale)

- **Controls are rebuilt on the wall** (n = wall normal). If the view's right vector points mostly along the wall (|right·n| < 0.7, i.e. facing the wall or away): forward' = normalize(n × right), right' = forward' × n, so forward climbs and strafe slides along. Otherwise (running along the wall): right' = normalize(forward × n), forward' = n × right'. Forward' follows your pitch, so **looking up runs up the wall**.
- **Auto push**: side input += max(forward input, 0) × (forward' · t) × `wallrun_upwardAutoPush` (0.65), with t = n × up. Strafing can only go up the wall: side input moving down the wall is zeroed.
- Wish = forward' × forward input + right' × side input, flattened onto the wall plane.
- **Friction** on the whole 3D velocity: speed × (1 − `wallrun_friction` (4) × dt), times the hang factor. While slipping, downward speed isn't reduced.
- **Vertical**: if the wish has a vertical part, accelerate towards min(|wish_v|, 1) × `wallrun_maxSpeedVertical` (300) in that direction at `wallrun_accelerateVertical` (450 u/s²).
- **Horizontal**: accelerate towards min(|wish_h|, 1) × (`wallrun_maxSpeedHorizontal` 240 if forward, `wallrun_maxSpeedHorizontalBackward` 50 if backward), at `wallrun_accelerateHorizontal` (MP 1500 u/s²; client default 7). Both are scaled by a move-speed factor S = (scale − 1) × `wallrun_moveSpeedScaleScale` (0.57) + 1, which is 1 unless a script changes the player's speed.
- **Gravity scale**: before `wallrun_gravityRampUpTime` (0.7 s) it is 1 − (1 − t/0.7) × L, where L is 0 on a same-wall run, else 1 for runs ≤ `strengthLossStart` (3), 0 for ≥ `strengthLossEnd` (5), and (count − 3)/(5 − 3) in between (as written; it jumps at 3). Times the hang factor.
- **Start test** (`0x180165aa0`): not on the ground; the hit surface's normal z (up) must be between −0.35 and 0.7; the 3D velocity direction · wall normal < −cos(`wallrun_minAngle_air` / 2) (180 → any angle into the wall). The wall must be there for rays from the body's centre line at **10 and 36 units** above the feet, reaching half the hull width. When the view is within the mantle angle of the wall, also at the top of the hull, so low walls get mantled rather than run on. Same wall (normals within `wallrun_sameWallDot` 0.9; `wallrun_sameWallAllowed` 1): rejected if not lower than where you left it and within `wallrun_sameWallDist` (100) horizontally, or if (distance − 100) × `wallrun_sameWallSlope` (0) ≤ the height gained. There is also a 0.5 s lockout after a rarely set player timestamp (field 0x3560, not yet identified; it isn't time since leaving the ground). There is no minimum air time.
- **Contact:** measured live, you keep moving towards the wall at full speed until you touch it; the run starts on contact, and you are never pulled onto the wall.
- **Begin**: wall-run count += 1, then the upward boost B = L × `wallrun_upWallBoost` (250, 0 on a same-wall run). If vertical speed < B: vertical = min(vertical + B, B).
- **Slip**: starts at `timeLimit − wallrun_sliptime` (0.7 s): vertical −= `wallrun_slipvel` (70), horizontal × (1 − `wallrun_slipslowdown` 0.5). Hang: at `hangTimeLimit − wallrun_hangsliptime` (1.0), vertical −= `wallrun_hangslipvel` (70).
- **Fall off**: velocity += n × `wallrun_fallAwaySpeed` (100); the wall is marked as "same wall" for the next grab.
- **Wall jump**: d = input direction (when hanging, topped up to length 1 with the view direction). Velocity += d × `wallrun_jumpInputDirSpeed` (100); then the outward part is raised to at least O = `wallrun_jumpOutwardSpeed` (150) × (1 + 0.8 × (d·n) if d·n < 0). Up: U = L × `wallrun_jumpUpSpeed` (250); if vertical < U: vertical = min(vertical + 1.5 × U, U).
- **Hang** (aim down sights on the wall): speed and gravity scale by 1 − hang time / `wallrun_hangStopTime` (0.5), reaching 0.
