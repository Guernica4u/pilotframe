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

**Not done yet:**
- How the spread (an angle) becomes the arms' push in pixels. Needs the drawing code; comes with weapon spread.
- Fade timing. We fade at 8 per second; this is our guess.
- The hit indicator (`hit_indicator`, 64 × 64).
- Colour by target (enemy and friendly colours).

## Still to match

- **Hudwarp.** The screen's curve.
- **Additive blending.** `standard_plus`, `hud_bar_small` and `weaponinfo_clip` are `$additive`; we alpha-blend them.
- **`shadowglow` on fonts.**
- **The rest of the screen:** minimap, Titan meter, offhand ability icons, objectives, scores, damage indicators.
