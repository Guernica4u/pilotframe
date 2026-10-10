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

**Drawn so far:** unlit, translucent or additive, tinted by `$color2`. **Not yet:** the noise layer, the proxies, the holo offset, and overbright (our gun camera isn't HDR, so the 8× tint clips to full red).

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
