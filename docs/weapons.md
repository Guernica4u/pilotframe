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
