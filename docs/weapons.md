# Weapons

What the game does with a weapon, read from the owner's game files and `client.dll`. Spread and the crosshair are in [hud.md](hud.md).

## Loadout

- **Preset.** A new pilot gets `pilot_preset_loadout_1` from `scripts/vscripts/_loadouts.nut`:
  - primary `mp_weapon_rspn101` (R-201) with attachment `hcog`;
  - secondary (anti-Titan) `mp_weapon_rocket_launcher` (Archer);
  - sidearm `mp_weapon_semipistol` (Hammond P2011);
  - special `mp_ability_cloak`, ordnance `mp_weapon_frag_grenade`;
  - passives `pas_ordnance_pack` and `pas_longer_bubble`.
- **Passives as mods.** `_passives.nut` calls `GiveExtraWeaponMod(<passive>)`, which applies the passive to every weapon that defines a mod by that name. The frag's `pas_ordnance_pack` sets `ammo_clip_size` "1++".
- **Relative mod values.** Mods write `*n` (times), `/n`, `n++` (plus) or `n--` (minus); any other value replaces the base one. Counts in the final patch's scripts: 263 `*n`, 11 `/n`, 13 `n++`, 4 `n--`.
- `pf-game/src/loadout.rs` reads both. The viewmodel uses the primary; the HUD uses the offhands and the secondary.

## Attachments (mods) and body groups

- **How a mod applies.** A mod is a block under `Mods` in the weapon script. Its values replace the base ones. For the R-201's `hcog`:
  - `bodygroup1_set`/`2_set` 0 (iron sights off), `bodygroup4_set` 1 (rear-sight-off part), `bodygroup6_set` 1 (`sight_hcog`);
  - `zoom_fov` 45;
  - `anim_alt_idleAttack` 1.
- **Body groups.** `bodygroupN_name` names one of the model's body parts and `bodygroupN_set` picks its sub-model, where 0 is usually blank. The R-201's are `sight_aog04`, `sight_hcog`, `sight_holo`, two suppressors, `r101_sight_front_on/off`, `r101_sight_rear_on/off` and `r101_magazine` (the clip shows at 0 and hides at 1). `pf-fmt-mdl::load_with_body` reads a chosen sub-model; the `body_groups` example lists them.
- **`anim_alt_idleAttack` n** switches the gun to its `_alt{n}` animations: idle, aimed idle, attack, aimed attack, and the aim in/out sequences.

## The HCOG reticle

The reticle meshes use `hcog_reticle_front`, `hcog_reticle_rear` and `hcog_reticle_field_emitter` (`materials/models/weapons/attachments`). These use Respawn's `UnlitTwoTexture` shader:
- translucent (the field emitter also `$additive`), unlit, no fog;
- `$color2` tint: front [8 1 1], emitter [2.5 1 0], i.e. overbright red;
- a second texture `dev/noise_post` scrolled by noise proxies;
- `$holoOffset` (front [0.1 0]).

**How the reticle shows** (material proxies in `client.dll`):
- **`HoloSight`** sets `$alpha` from a value the viewmodel keeps (`0x18009e320`).
  - While zoomed, it rises each frame by frame time ÷ the aim-in time (or at the aim-in animation's rate, when the weapon has one). Otherwise it falls at the aim-out rate. Clamped 0..1.
  - So the reticle fades in as you aim and is invisible from the hip.
  - With a ConVar set, it's simply the zoom fraction.
  - `HoloSight` runs last, so it overwrites the front chevron's random-alpha `UniformNoise`.
- **`UniformNoise`**: a random value in [min, max] each frame. The front reticle's `$scroll` gets one per axis, so its noise texture jumps around every frame (shimmer). The field emitter's `$color` flickers 0.8–1.
- **`TextureTransform`** (Source's): uv′ = T(center + translate)·R(rotate)·S(scale)·T(−center)·uv, center 0.5. Used for `$texture2transform` (the noise) and the front reticle's `$holoOffset`.
- **`UnlitTwoTexture`**: the base texture × the second texture (`dev/noise_post`) × `$color` × `$color2`, alpha × `$alpha`.

**Sight body:** `hcog.vmt` is `VertexLitGeneric` with `$selfillum` and mask `hcog_ilm` at `$selfillumtint` [10 10 10]. That's the glowing blue glass.

## On-gun ammo counter

The R-201 viewmodel has an ammo screen built from meshes: two clip digits, a clip meter, magazine ticks (hip and aimed versions) and a background. These use `Basic` materials with proxies (`client.dll`, factories registered by name; each proxy's bind is vtable slot 1):

| Proxy | Value |
|---|---|
| `ClipAmmo` | rounds in the clip (spare rounds if the weapon has no clip) |
| `MaxClipAmmo` | clip size, at least 1 |
| `RemainingAmmo` | spare rounds |
| `MaxCarryAmmo` | `ammo_stockpile_max` |
| `RemainingClipCount` | spare ÷ clip size, rounded **up** |
| `NumericCounterAtlasFrame` | with p = 10^digitIndex: frame 0 (blank) if value < p and digitIndex > 0, else (value ÷ p mod 10) + 1. The atlas is blank, then 0–9 |
| `TickCounterAtlasFrames` | ticks = a full row if (row+1)·length ≤ count, 0 if row·length ≥ count, else count mod length (length 0 = the atlas size); frame start = atlas size − ticks, frame count = length |
| `TextureAtlas` | uv′ = (count/size)·uv + start/size per axis (count defaults to 1) |
| `ProgressBar` | uv shifted by 1 − value/max (0 if max is 0) |
| `ClipAmmoColor` | red (1, 0.29, 0.26) if clip ≤ 10% of its size; blue (0.52, 0.905, 1) if ≥ 40%; else orange (1, 0.5, 0.25) |
| `RemainingAmmoColor` | red if spare ≤ one clip, orange if ≤ three clips, else blue |
| `BackgroundAmmoColor` | (1, 0.69, 0.65) if clip ≤ 10%, else white |
| `Divide` | Source's: src1 ÷ src2 |

The R-201's tick row length is `MaxCarryAmmo` ÷ `MaxClipAmmo` (240 ÷ 24 = 10). The counter's `_ilm` masks aren't in the game files.

**Ours:**
- `pf-game/src/matproxy.rs` runs these proxies each frame; `pf-game/src/unlit_two.rs` with `assets/shaders/unlit_two_texture.wgsl` draws `UnlitTwoTexture`.
- `Basic` is drawn unlit. `$decal` meshes get a small depth bias (ours).
- The holo value ramps over the zoom times; the game uses the aim-in/out animations' rates when the weapon has them.
- **Overbright:** the reticle tints are up to 8×, and our gun camera clips at white, so the reticle looks dimmer than the game's. That needs HDR rendering.
- **Not yet:** the sight body's `$selfillum` glow.

## Gun field of view

`viewmodelfov` (pilot settings, offset 0x28) is used as-is: `0x18003d260` returns it as an integer, and the FOV slider doesn't scale it. The world FOV is the pilot `fov` (offset 0x24) × `cl_fovScale`, rounded (`0x18003d1e0`).

## Sway and bob

Ported from `client.dll`; `pf-game/src/sway.rs` follows it, and the cockpit uses the same code (see [hud.md](hud.md), "Cockpit sway"). `weapon_sway` (default 1) turns it on.

**Settings** (loader `0x18047ddd0`):
- Two blocks of the same layout: hip, and aimed (keys with `_zoomed`, 0x1a0 further on).
- `sway_rotate_attach` names the pivot attachment, with `sway_rotate_attach_blend_time`.
- `sway_min/max_{x,y,z,pitch,yaw,roll}` are the limits; `sway_translate_gain` and `sway_rotate_gain` the easing rates.
- `bob_cycle_time`, `bob_min/max_speed`, `bob_vert/horz_dist`, `bob_pitch/yaw/roll`, `bob_gain`.
- Factors `sway_{move_forward,move_back,move_left,move_right,move_up,move_down,turn_left,turn_right,turn_up,turn_down}_{translate_x,y,z,rotate_pitch,yaw,roll}`.
- `sway_enable_zoomed` (default 1).
- **Defaults:** missing keys are 0, except the gains (12 hip, 24 aimed), `bob_gain` 12, `bob_min_speed` 0.1 and `bob_max_speed` 0.2. A missing limit is 0, so that axis doesn't move. The R-201's aimed block only frees yaw ±0.03° and pitch ±0.01°.

**Update** (`0x18047f1e0`), each frame:
1. **Block.** Use the aimed block if zoomed (the player's zoom state, on while aiming) and `sway_enable_zoomed`; otherwise the hip one.
2. **Bob.**
   - **When it runs.** Planar speed in view space is remapped to 0..1 between the bob speeds. With `bob_cycle_time` > 0, a positive speed fraction and on the ground, the phase advances by fraction × dt, wrapping at 2 × cycle.
   - **The waves.** Two sines, sin(2π·(phase mod cycle)/cycle) and sin(2π·phase/(2·cycle)), eased at `bob_gain`. The first gives the vertical distance and pitch, the second the horizontal distance, yaw and roll.
   - **Stopping.** Otherwise the phase eases to the nearest whole step (0, cycle or 2 × cycle) and the waves ease to 0.
3. **Turn rate.** The per-frame view-angle change ÷ dt, kept in a ring of 5 and averaged.
4. **Targets.** Each input adds |input| × its direction's six factors:
   - forward/back/left/right/up/down velocity × dt;
   - yaw rate (left/right) and pitch rate (up/down), each × a frame time.

   Offsets are clamped to their limits; angles are normalised, then clamped.
5. **Easing.** Offset: x = (1 − e)·target + e·x with e = exp(−`sway_translate_gain`·dt). Angles the same way with `sway_rotate_gain`, through the angle difference.
6. **Aimed limits.** With `sway_enable_zoomed`, the angles are clamped to limits lerped from hip to aimed by clamp(zoom fraction × 20, 0, 1).

**Apply** (`0x18047fed0`): rotate about the pivot by sway + bob angles, then move by sway + bob offset, in the gun model's space; then on to the view.

**Pivot** (`0x18009f740`):
- The hip and aimed `sway_rotate_attach` attachments (R-201: `SWAY_ROTATE`, `SWAY_ROTATE_ZOOMED`), read from the gun's current pose in model space.
- A blend moves towards 1 while zoomed (0 otherwise) over that block's blend time (R-201: 0.2 s into aim, instant out).
- The pivot is lerped and slerped between the two.

**Ours:**
- The turn rate's frame time (an engine global at +0x28) is taken to be this frame's.
- The first frame has no previous view, so it counts as no turn.
- The aim fraction is our zoom ramp, not the engine's.

## Recoil (view kick)

Ported from `client.dll`; `pf-game/src/recoil.rs` and the kick spring in `pf-game/src/view.rs` follow it. `viewkick_*` settings sit at 0x134–0x1c8 in the weapon settings. Table defaults: the four `viewkick_scale_min/max_*` and `viewkick_duck_scale` are 1, everything else 0.

**Each shot** (`0x180491190`, called from the fire code `0x18049c4c0`):
- **Pitch and yaw:** `base` + a random size between `random_innerexclude`/2 and `random`/2, with a random sign. The game uses named shared random streams ("weaponKickX", "weaponKickXFlip"…) so client and server agree.
- **Roll:** `base` + random between `randomMin` and `randomMax`, randomly signed.
- **Scale.** Pitch and yaw are scaled by the kick scale × ((`viewkick_duck_scale` − 1) × crouch fraction + 1) × a shot count. Roll is scaled by a separate factor; we take both multipliers as 1.
- **Soft and hard.** The kick × `softScale` goes to the kick spring's speed (× 20), × `hardScale` to its angle (`0x18003c840`).
- **Permanent kick.** `viewkick_perm_pitch/yaw` (same random form) × the kick scale is added to the player's real aim angles, for the predicting local player.
- **Burst accumulator.**
  - **Reading** (`0x180488060`): it holds for `valueDecayDelay` after the last shot, then falls at `valueDecayRate`/s, down to 0.
  - **Each shot:** after the kick is computed, it becomes min(value now + `valuePerShot`, `valueLerpEnd`).

**Kick scale** (`0x18048b940`):
- The first shot of a trigger pull uses lerp(`scale_firstshot_hipfire`, `_ads`, zoom) if that's non-zero.
- Otherwise: lerp(min, max, t), with t the burst value from `valueLerpStart` to `valueLerpEnd`, clamped. Min and max are each lerped hip → ADS by zoom.

**Kick spring** (`0x180165190`):
- **Two springs.** The game runs the view-punch spring (player +0x26d0/+0x26dc) and the weapon-kick spring (+0x26e8/+0x26f4), both moved by the same code with per-axis constants: solved exactly over each frame (see r1-engine-values.md, "View"). Stiff weapon springs need the exact solution; a fixed Euler step blows up on the R-201's roll spring.
- **The kick spring's constants** come from the weapon's `viewkick_spring` set in `scripts/weapons/springs.txt`, lerped from the `hipfire_*` to the `ADS_*` values by the zoom fraction (`0x180487f50`). Without a set it uses the base constants (65 / 9; the pilot's `punch_pilot` set is the same).
- **Movement punches** also add 0.2 × to the kick spring.

**View and gun split:**
- **Fractions.** wf = lerp(`viewkick_hipfire_weaponFraction`, `viewkick_ads_weaponFraction`, zoom), and vms the same for the `_vmScale` values.
- **Camera** (`0x18003ba40`): view punch + kick pitch/yaw × (1 − wf) + all of the kick's roll.
- **Gun, relative to the view** (`0x1800378d0`): kick pitch/yaw × wf × vms, and the kick's roll undone.
- **R-201:** hip fire splits the kick, the camera taking half and the gun another quarter; aimed, the camera takes all of it.

**Ours:**
- Crouch is 0 or 1; the game eases it over 400 ms.
- Our own random generator replaces the named streams.
- `PF_DEV_FIRE=1` holds the trigger and `PF_DEV_ADS=1` holds aim, for screenshots.

