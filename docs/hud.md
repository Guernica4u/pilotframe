# Pilot HUD

How TF1 builds the pilot HUD, read from the owner's game files (the `englishclient_mp_common` pack of the final PC patch, v1.0.10.1) and `client.dll`. `pf-game/src/hud.rs` draws it from these same files at run time; nothing is copied into the repo.

## Where things are

| What | File |
|---|---|
| Helmet screen layout | `scripts/screens/vgui_fullscreen_pilot.res`, which `#base`s `vgui_fullscreen.res` |
| Screen definition (852 × 480) | `scripts/vgui_screens.txt`, entry `vgui_fullscreen_pilot` |
| Fonts (names, sizes) | `resource/hudscriptedscheme.res`, section `Fonts`; font files in `resource/clientscheme.res` → `CustomFontFiles` |
| Font files | `resource/*.vfont` (see below) |
| Textures | `materials/vgui/HUD/*.vmt` / `.vtf` |
| Weapon names | `resource/r1_english.txt` (UTF-16), e.g. `WPN_RSPN101_SHORT` = "R-101C Carbine" |
| Crosshair per weapon | the weapon script's `CrosshairData` block |
| HUD scripts (reference only) | `scripts/vscripts/client/cl_main_hud.nut` (weapon panel: `MainHud_InitWeapons`) |

**Which copy.** Files come from the owner's EA install (final patch v1.0.10.1), unpacked with `pf-pc vpk-extract` into `Resources/ea_extracted/common`; the game falls back to the launch build from the retail discs. The R1Delta copy of the common pack isn't used: it points about 28,000 entries at R1Delta's own pack (archive number 0x1337). The game DLLs (`client.dll`, `vguimatsurface.dll`, `engine.dll`) are byte-identical in the EA and R1Delta copies, so addresses in these notes hold for both. The final patch changed some HUD data from the launch build, e.g. the R-201's clip is 24 (240 total) instead of 30 (300).

## The helmet screen

`Create_PilotHud` (in `cl_main_hud.nut`) places an 852 × 480 VGUI screen 208 units in front of the cockpit camera, 426 × 240 units in size, and bends it ("hudwarp": `xWarp` 45, `xScale` 1.3, `yWarp` 30, `yScale` 1.1, `viewDist` 1). **We draw it flat, scaled to fit the window.** The curve is still to do.

**Layout rules** (`pf-fmt-ui::layout`):
- **Pin offsets point outward from the sibling's corner:** left of a left corner, above a top corner, right/below otherwise. The left and right panels use the same numbers on mirrored corners (`GameInfoBG` and `dpadIconBG` both `xpos 20`; `Offhand0Icon` and `Offhand1Icon` both `xpos -200`). Checked against an R1Delta screenshot: the weapon name lands within 5 px.
- `xpos`/`ypos` take a number, `c±n` (from centre), `r±n` (from the right/bottom) or `%n`.
- `wide`/`tall` take a number, `%n` or `f n`.
- `pin_to_sibling` puts our corner `pin_corner_to_sibling` on the sibling's corner `pin_to_sibling_corner`, then offsets by `xpos`/`ypos`. Corners: 0 TL, 1 TR, 2 BL, 3 BR, 4 top centre, 5 right centre, 6 bottom centre, 7 left centre, 8 centre.

**Resolved weapon panel:**
- Hex icon at (739, 366), 72 × 66.
- Side background to its left at (631, 366), 108 × 66.
- Ammo count right-aligned against the side background's right centre.
- Ammo bar 65 × 6.
- Weapon name above the bar.
- Spare-magazine count and clip icon above the panel.

**Weapon panel elements:**

| Element | Type | Draws |
|---|---|---|
| `dpadIconBG`, `dpadIcon` | image | hex background and frame |
| `dpadSideBG` | image | the panel background |
| `weaponLabel` | text | `HudFontSmall` (Conduit ITC Pro Medium, 12, glow 3), white |
| `weaponAmmoCount` | text | rounds in the clip, `HudFontMedPlain` (18), right-aligned |
| `weaponAmmoBar` | bar | clip fraction: `hud_bar_small` tinted 128 grey over black at alpha 123 |
| `weaponMagLabel` + `weaponAmmoMag` | text + icon | spare magazines, white at alpha 140, formatted by the localisation's `HudAutoText_ActiveWeaponMagazineCount` ("x%s1"). The weapon's `ammo_default_total` includes the loaded magazine: an R1Delta screenshot shows 24 in the clip and "x9" for the R-201's 240 (final patch). |

## Offhand slots and the d-pad (`InitOffhandWeaponHud`, `MainHud_UpdateWeapons`)

What the HUD script shows for the default loadout (see [weapons.md](weapons.md), "Loadout"):

| Slot | Weapon | Elements | Shows |
|---|---|---|---|
| Offhand 0 (`OFFHAND_RIGHT`, right of centre) | ordnance: frag grenade | `Offhand0BG/Icon/Hint/Count` | `hud_icon` (`HUD/dpad_grenade_icon`), key of `+offhand1` (F), count "x 3" |
| Offhand 1 (`OFFHAND_LEFT`, left of centre) | special: cloak | `Offhand1BG/Icon/Hint/Bar` | `hud_icon` (`HUD/dpad_cloak`), key of `+offhand2` (Q), ready bar |
| D-pad left | anti-Titan weapon: Archer | `dpadLeftIcon/IconHint` | `hud_icon` (`HUD/dpad_archer`), key of `weaponSelectOrdnance` (3); its title and description are drawn at alpha 0 |

- **Count or bar** (`GetOffhandProgressSource`): `offhand` and `offhand_hybrid` weapons show a bar only if they have a `charge_time`; any other fire mode (the cloak is `offhand_instant`) always shows a bar.
- **Count format:** `HudAutoText_PilotOffhandWeaponAmmo0` is "x %s1", `...Ammo1` is "%s1". The frag's 2 grenades plus `pas_ordnance_pack` make 3, as in the R1Delta screenshot.
- **Key hints:** labels like `%offhand1%` show the key bound to `+offhand1` (else `offhand1`). Defaults come from `cfg/config_default_pc.cfg`: F `+offhand1`, Q `+offhand2`, 3 `weaponSelectOrdnance`, V `+ability 1` (Titan call-in, shown by the hex).
- **Not yet:** the box drawn around the key name. It's code (`EnableKeyBindingIcons`), not a texture, so it needs reading from the DLLs. We show the bare key name.
- **Label alignment:** a VGUI label places its text by `textAlignment` (default `west`): `west`/`east`/`center` are vertically centred; `north*` top, `south*` bottom.

**Open: the safe area** (measured against R1Delta, so possibly R1Delta's curve rather than the layout). Offhand icons are pinned to `SafeAreaCenter` (730 × 420 in the file). `MainHud_InitAspectRatio` resizes it to 90% of the screen width and 95% of the height when the safe-area setting is off (`level.safeAreaScale != 1`). Against the R1Delta screenshot, with our 70° panel view:
- the file's 730 puts the icons about 13 px (at 1280 wide) too close to the centre;
- 766.8 puts them about 18 px too far out, and too low;
- the screenshot fits about 745, at the file's height.

So either the setting's value on PC or the panel's view angle (measured, see "Hudwarp") is off. We keep the file's size until `SafeareaSettingIsEnabled` is read from code.

## The hex: Titan meter (`UpdateTitanModeHUD`, `cl_replacement_titan_hud.nut`)

While the first Titan builds, the d-pad's down slot shows:
- `dpadDownIcon`: `HUD/titan_build_icon_5` (60 × 60, centred on the hex); `HUD/titan_stance_ready` once ready.
- `dpadDownIconHint`: the key of `+ability 1` (V), `HudFontMedPlain`, centred, 28 left of and 11 above the hex's bottom centre.
- `dpadDownDesc`: a countdown to the Titan (`HudAutoText_CountdownTime`, "%s1:%s2"), bottom-aligned with `textinsety` -6; blank once ready.
- `dpadDownIconBG`: a circular bar (`HUD/hud_hex_progress_timer`, black at 220, counter-clockwise). `SetDpadCooldown` sets it to the fraction still to build and runs it down to 0, so it's a dark cover that shrinks as the Titan builds, leaving a revealed wedge clockwise from 12 o'clock.
- `dpadDownIconProgressBar`: fill colour 0 (not drawn), background `HUD/hud_hex_progress_hollow_round_bg` at black 160 while building, clear once ready.

**Build time:** the playlist's `titan_build_time` (`playlists.txt`, UTF-16; 240 in every MP playlist; the script default is 340). The R1Delta screenshot shows a 14:44 match clock and 3:44 on the Titan, so both started together. Kills take time off in the game; not here yet.

**Countdown format, ours:** minutes, then two-digit seconds, rounded up. The game's formatting of `HATT_COUNTDOWN_TIME` isn't read yet.

### Circular progress bars (`client.dll`)

The progress bar's paint (`0x18041d1a0`) hands circular bars to `0x18041cdb0`:
- **Value:** the bar's value goes through its remap: (v − in_lo) × in_scale, clamped to 0..1, then × out_scale + out_lo, clamped. The angle is value × 2π.
- **Wedges:** 8 triangles of 45°, each from the centre (0.5, 0.5) to two points on the image's square edge. A table at `0x180b243b0` gives each wedge's start angle and edge points, clockwise from 12 o'clock: (0.5, 0) → (1, 0) → (1, 0.5) → … → (0, 0) → (0.5, 0). Each is drawn with the bar's texture and colour, UVs as the points.
- **The partial wedge** ends on its edge at the tangent of the angle into it (measured from the edge's centre or corner), so the drawn shape is exactly the angular sector in the image's square space.
- **`CircularClockwise`** (byte +0x2af): set, the sector runs clockwise from 12 o'clock; clear, counter-clockwise.

We draw the same sector with a UI shader (`pf-game/assets/shaders/hud_circular_bar.wgsl`).


## Score panel (`MainHud_InitScoreBars`)

Bottom left, pinned to `SafeArea`'s bottom-left corner:
- `GameInfoBG`/`FG` (the hex) with `GameInfoTeamIcon`: `TEAM_ICON_MILITIA` (`../ui/scoreboard_mcorp_logo`) or `TEAM_ICON_IMC`.
- `GameInfoLabel` above it: the time left (`HATT_COUNTDOWN_TIME` to the match's end); `GameModeLabel` beside it: `GAMETYPE_TEXT[mode]` (Pilot Hunter is mode `tdm`, `#GAMEMODE_PILOT_HUNTER`), all caps.
- Per team, `ScoresFriendly`/`ScoresEnemy` bars, remapped from 0..score limit to 0.011..0.96 of the strip, plus the score (`HudAutoText_FriendlyTeamScore`, "%s1") and team name (`#TEAM_MCOR` "Militia", `#TEAM_IMC` "IMC").
- `Player_Status_BG` (`HUD/hud_hex_8v8_bg_left`). The Titan counts draw one icon per Titan, so nothing at the start.
- **Ours:** Pilotframe has no modes or teams, so the HUD shows Pilot Hunter on the Militia with 0–0 and the playlist's `TDM_timelimit` (15 min) running down from the start.

**Label insets** (Source's `Label` paint): `textinsety` is added to the text's y whatever the alignment; `textinsetx` moves the text in from the side it's aligned to (added for west, subtracted for east, ignored when centred).

## Fonts (`.vfont`)

A TrueType/OpenType file followed by a trailer and the tag `VFONT1`. The byte before the tag, n, is the trailer's length, counting that byte. The rest of the trailer is the salt. To decode:
- The key starts at 0xA7 and is xor-ed with each (salt byte + 0xA7).
- Each data byte is xor-ed with the key; the key then becomes that scrambled byte + 0xA7.

This was read from the font loader in `vguimatsurface.dll`. The HUD fonts decode to OpenType CFF (Conduit ITC Pro, PF DIN Text Cond Pro) and TrueType (Titanfall).

## Crosshair

Each weapon's `CrosshairData` lists elements. The R-201 has a `spread` element with `standard_plus` (plus `standard_plus_shadow`):
- 44 × 44 on a 480-tall screen, `rotation` 90;
- drawn while hip-firing only, half size while aiming (`scale_ads` 0.5);
- fully faded halfway into aiming (`fade_into_zooming_end` 0.5);
- faded while sprinting and while reloading.

**From the element loader in `client.dll` (`0x180404160`):**
- A `spread` element draws its texture (one arm) several times around the centre. It starts at `spread_initial_angle` and steps by `spread_delta_angle`, giving 360 / step icons (at most 8), pushed out by `spread_initial_offset` plus the current spread.
- Defaults when a weapon doesn't set them: offset 0, start angle 0, **step 90°, so 4 arms**.
- Other keys: `offset_x/y`, `size_fov`, `fade_*`, `draw_while_*`, `stationary`, `default_color` / `fixed_color`.

**Per-frame drawing** (`0x180402750`; arm setup at `0x1804022b0`):
- Arm k's angle is `spread_initial_angle` + k × step. Its push direction is (cos, −sin) of that angle, and its image is turned by `rotation` + k × step.
- Each frame, every arm is placed at the crosshair centre + push × its direction. The push is the scaled `spread_initial_offset` plus the spread in pixels.
- **Spread in pixels** (`0x18007c090`) = tan(spread / 2) / tan(fov / 2) × (screen size) × 0.5. That's the true projection of the spread cone. Whether "screen size" is the width or the height isn't settled yet (it's the second value from the screen-size call, likely the height).
- R-201 standing hip spread is 5.5°. In the owner's R1Delta screenshot (2000 × 1125) each arm's centre sits about 40 px from the screen centre. That matches the height version at FOV 90; the owner's FOV setting needs confirming.
- Alpha is the element colour's alpha × fade. The sprint fade (when `fade_while_sprinting` is set) and the reload fade are values kept by the HUD; their rates aren't read yet.

**Still to read:** the hit indicator (`hit_indicator`, 64 × 64) and the target colours (enemy and friendly).
- The fade rates. Our current 8 per second is a stand-in, to be replaced by the game's.

## Weapon spread

Ported from `client.dll`; `pf-game/src/spread.rs` follows it.

**Spread now = base + kick.** The getter (`0x180487ed0`, weapon vtable slot 0x958) returns:
- base (weapon +0x252C), plus
- lerp(hip kick +0x2530, ADS kick +0x2534, by zoom fraction).

A setting looked up by name at run time (pointer at `0x1817eb1d8`) switches this to max(base, kick) instead. Its name and value aren't read yet. **We use base + kick (unconfirmed)**, until it can be read from the live game.

**Base, each player tick** (`0x180490e30`, slot 0x960; called from the player's think with speed, run speed, sprinting, crouch, zoom and on ground):
- Sprinting: `spread_stand_hip_sprint`.
- In the air: lerp(`air_hip`, `air_ads`, zoom).
- On the ground or a wall: first stand = lerp(`stand_hip`, `stand_hip_run`, clamp(speed / run speed)). Then a blend over crouch c and zoom a: (1−a)(1−c)·stand + a(1−c)·`stand_ads` + (1−a)c·`crouch_hip` + ac·`crouch_ads`.
- The base moves towards that target by at most `spread_lerp_speed` (default **1.25**) per tick.
- "Run speed" is the pilot's stand-block setting at +0x44, read here as `speed` (180.6). That's inferred from its position, not confirmed.

**Kick on each shot** (`0x18048f400`):
- The hip kick is added only while less than halfway aimed in; the ADS kick always.
- Kick sizes: `spread_kick_on_fire_[stand|crouch|air]_[hip|ads]`, capped at `spread_max_kick_*`. The stance uses the ground entity only, so wall-running counts as air.

**Decay** (weapon think `0x18049bff0`): `spread_decay_delay` after the last shot, both kicks shrink by `spread_decay_rate` × frame time, down to 0.

**Unset values** (`0x1802e5750`): a value below 0 means "unset" and falls back to `spread_stand_hip`; sprint falls back to run first. Table defaults: `stand_hip` 5, the others −1, `lerp_speed` 1.25, `npc` 3.

**Ours, not yet the game's:**
- Crouch is 0 or 1 here; the game passes a duck fraction.
- The tick is our 60 Hz.

**The arm texture** (`standard_plus`, 128 × 128 DXT5) is one 4-pixel-wide line running from the tile's centre to a quarter of the tile out. So an arm's inner end sits exactly at the push. The game's UI angles turn the opposite way to Bevy's, so we negate them.

**Check against R1Delta** (2000 × 1125, FOV ≈ 90, standing, R-201):
- TF1: arms run from 28 to 53 px from the centre, 25 long, 3 thick.
- Formula: push = tan(2.75°) / tan(45°) × 562.5 = 27 px; arm = 44 × 1125/480 / 4 = 26 px long. That matches.
- Pilotframe at FOV 70 (720 tall): push 24.7 px, which is where the arms are.

## Hudwarp (the helmet screen's curve)

Ported from `vguimatsurface.dll` (December 2014 build); `pf-game/src/hudwarp.rs` follows it.

**Settings** (`hudwarp_*` defaults; the scripts pass their own through `WarpGlobalSettings`):
- Arc: `xWarp` 45, `yWarp` 30, both clamped to 1..89°.
- Final scale: `xScale` 1.2 (the pilot HUD passes 1.3), `yScale` 1.1.
- `viewDist` 1.0.
- `chopsize` 60 px.
- `MainHud_TurnOn` animates the scales up from 0 when the HUD switches on (not ported yet).

**Setup** (`0x180015a30`) stores the chop size and 1/chop, the screen size, tan(xWarp) and tan(yWarp), the arcs in radians with their scales, and `viewDist`.

**Drawing:** each shape is scaled by 1/chop and split at whole-number lines until every piece fits in one 60 px cell (`0x180011420`). Then every corner is moved by `0x18000bac0`. That function uses π/2, 0.5, and a 4-wide sine:
- ax = xWarp·(1 − 2x/W), ay = yWarp·(1 − 2y/H)
- depth = (cos ax · cos ay + d) / (d + 1)
- x′ = W/2 · (1 − xScale · sin ax · cos ay / (depth · tan xWarp))
- y′ = H/2 · (1 − yScale · sin ay / (depth · tan yWarp))

**Ours:** the HUD is drawn flat into two images, one for normal textures and one for `$additive` ones. They're shown on a grid mesh in the same 60 px cells, with every corner moved by that formula. The additive image adds light; the normal one blends as premultiplied alpha.

**Where the screen sits:** `Create_PilotHud` puts it 208 units in front of the cockpit's `CAMERA_BASE` attachment, 426 units wide.

**The cockpit and its FOV.** Every pilot class (`scripts/players/mp/pilot_*.set`) has a cockpit model, `models/weapons/arms/human_pov_cockpit.mdl`, with its own sway settings (`cockpitSway*`, below). The HUD screen is parented to it. The cockpit is a `C_Titan_Cockpit` (client.dll):
- Its constructor (`0x1803f58f0`) sets its FOV (+0x2364) from `cockpit_default_fov`, default **75**. No script calls `SetCockpitFOV`, and `cockpit_forced_fov` defaults to 0.
- Its per-frame cockpit-screen code (`0x1803f7930`) overwrites the FOV with **70.0** in its normal path (when a player float at +0x2a3c is 0; the field isn't identified yet).
- So we view the screen at FOV 70, which covers about 1.1× the screen width. The R1Delta screenshot measured the same, though it can't be trusted for this (see below). Still to read: the code that renders the cockpit with this FOV, to confirm it's the Source 4:3 horizontal convention like the other FOVs.

**Cockpit sway (not ported yet).** The pilot classes set `cockpitSwayTurnAngleFactor` -0.6, `TurnAngleRollFactor` 0.2, `TurnOriginFactor` -0.15, `MoveAngleFactor` -0.4, `MoveAngleRollFactor` -0.2, `MoveOriginFactor` -0.15, `cockpitSwayGain` 5.0, plus min/max limits. The HUD moves with the cockpit, so it drifts as you turn and move.

**R1Delta draws its own curve.** R1Delta's `tier0.dll` has `client/rendering/hudwarp_hooks.cpp` with settings `hudwarp_use_gpu` and `hudwarp_disable`: it replaces the game's warp with its own GPU version. Against an R1Delta screenshot, no settings of the game's formula fit (16 px RMS over 9 elements). Dropping the depth term fits to 5.6 px, so R1Delta's version evidently differs from the original. **We keep the original's formula** (from `vguimatsurface.dll`) and don't fit warp-dependent positions to R1Delta screenshots. The game also has `hudwarp_override` (default off) to take the five values from `hudwarp_*` ConVars instead of the script.

## Still to match

- **Additive blending of the crosshair.** `standard_plus` is `$additive`, but the crosshair isn't on the helmet screen yet, so it's still alpha-blended.
- **`shadowglow` on fonts.**
- **The rest of the screen:** minimap, objectives, damage indicators.
