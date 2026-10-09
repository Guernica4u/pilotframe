# Pilot HUD

How TF1 builds the pilot HUD, read from the owner's game files (PC retail disc, `englishclient_mp_common` pack) and `client.dll`. `pf-game/src/hud.rs` draws it from these same files at run time; nothing is copied into the repo.

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

The game's R1Delta copy of the common pack points some entries at R1Delta's own pack (archive number 0x1337), so HUD files are read from the **retail disc** instead.

## The helmet screen

`Create_PilotHud` (in `cl_main_hud.nut`) places an 852 × 480 VGUI screen 208 units in front of the cockpit camera, 426 × 240 units in size, and bends it ("hudwarp": `xWarp` 45, `xScale` 1.3, `yWarp` 30, `yScale` 1.1, `viewDist` 1). **We draw it flat, scaled to fit the window.** The curve is still to do.

**Layout rules** (`pf-fmt-ui::layout`):
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
| `weaponMagLabel` + `weaponAmmoMag` | text + icon | spare magazines, white at alpha 140 |

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

**Check:** R1Delta screenshot at FOV ≈ 90, standing, R-201 → arm centres ≈ 40 px from centre at 1125 px tall. Pilotframe at FOV 90 → ≈ 24 px at 720 tall (= 25.6 scaled).

## Still to match

- **Hudwarp.** The screen's curve.
- **Additive blending.** `standard_plus`, `hud_bar_small` and `weaponinfo_clip` are `$additive`; we alpha-blend them.
- **`shadowglow` on fonts.**
- **The rest of the screen:** minimap, Titan meter, offhand ability icons, objectives, scores, damage indicators.
