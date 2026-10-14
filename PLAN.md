# Pilotframe: plan

An unofficial Rust + Bevy rebuild of **Titanfall (2014, PC)** that runs from the player's own install, stays playable forever, and is fully ours (GPL-3.0). Not affiliated with, endorsed by, or connected to Electronic Arts or Respawn Entertainment. "Titanfall" is a trademark of Electronic Arts Inc.

**Goal: a 1:1 copy of the original game.** Every system is ported from the game itself (its data files and its code), never made up. The code is all ours, in Rust.

Reset on 2026-10-10. The work before it (Xbox 360 disc, PC retail discs, R1Delta copies, guessed values) is kept under the git tag `archive-v0`.

## The one source

The **EA install, final patch v1.0.10.1** (locally `Resources/titan fall/.../titanfall_pc_patch…`, gitignored):
- **Data** (maps, models, textures, materials, UI layouts, fonts, scripts, settings) is read from its VPKs and loose files. Scripts and settings files are read for values and behaviour, and never copied into the repo or shipped.
- **Code:** `client.dll` (and `vguimatsurface.dll` / `engine.dll` if needed) is decompiled for reference, clean-room. Findings go into notes in `docs/port/`, and our Rust is written from the notes. `Titanfall.exe` (DRM) and `Core/` (EA activation) are never touched.
- **R1Delta** is only for playing the real game side by side, and for taking reference screenshots and video. It's never a source of data or code.

## The method, for every system

1. **Find** the code in the full client.dll decompile (`docs/tools.md`) and the values in the game files.
2. **Write it up** in `docs/port/<system>.md`: values, formulas, struct layouts, behaviour. No decompiled code.
3. **Write the Rust** from the notes.
4. **Check** it against the real game: reference shots with the compare tool, or video frame by frame. Attach the result to the PR.
5. **Label it:** "ported" (from the game) or "ours" (our own stand-in, e.g. the dev shell). Nothing is guessed silently. A value we can't find yet is a marked placeholder, listed under the step it belongs to.

## Roadmap

Order set by the owner (2026-10-14): **gameplay perfected first**, then maps, then menus, then the complicated systems. Step numbers are names, not order (branches use them); the table is in working order.

**Why the script engine leads phase 1:** in Titanfall the HUD, minimap, menus and what each weapon does when fired are Squirrel scripts, not engine code. Hand-building those would be thrown away once the scripts run. The engine-side parts (bullet traces, sound playback, animation, effects) are ported from client.dll alongside.

| Step | What | Port from | Status |
|---|---|---|---|
| R0 | Reset: one source, cleanup, new plan and docs | — | done |
| R1 | Read the install directly: install path config, VPK mounting (no extraction step), mod-override folder | file formats | done |
| **Phase 1** | **Gameplay, perfected** (on the dev map and Colony) | | |
| R5 | **Script engine.** Our own Squirrel interpreter, in Rust, runs the game's scripts from the install. Groundwork done: native inventory, load order, language survey, a parser for all 481 scripts, checked against the game's own compiler (146/146 cases). Next: the VM; first milestone is the R-101C's weapon script and the pilot HUD script running (docs/port/scripts.md). | the game's `.nut` scripts + launcher.dll | in progress |
| R3 | Pilot movement: ground, air, double jump, wall-run, wall-hang, mantle, crouch, view punches. Values recorded from the game (2026-10-12); open: jump ~1% low, frame-rate stepping, crouch and long wall-run recordings, `max_slope_deg`. | client.dll movement prediction | partly ported |
| R4 | Viewmodel: sway, recoil, ADS, sight proxies, gun position (ours sits lower and further right than the reference) | client.dll + weapon scripts | partly ported |
| R6 | Shooting: the weapon scripts' fire callbacks, bullet traces, impact effects and decals per surface, damage on targets | weapon scripts + client.dll + effect tables | |
| R7 | Sound: weapons, reloads, footsteps, impacts, HUD sounds, ambience. First find where the EA install keeps working audio (its VPK sound entries fail their CRC; an installer rebuilds them). | sound scripts + files | |
| R8 | First-person animations: fire, reload variants, sprint, wall-run, hang, mantle | MDL sequences + client.dll | |
| R11 | HUD and minimap, run by the game's own HUD scripts (the hand-built HUD is replaced) | `cl_main_hud.nut` and friends | frozen until R5 |
| **Phase 2** | **Maps** | | |
| R2 | Colony checked against the real game: geometry, textures, lightmaps, props, collision, sky | BSP + materials | mostly exists (from older files), re-check |
| R12 | Every multiplayer map, DLC included, to the same standard | BSP + materials | |
| **Phase 3** | **Menus and front end** | | |
| R9 | Main menu, loadouts, settings, lobby, loading screens, from the game's own UI files and scripts | `.res` + UI scripts | |
| **Phase 4** | **The complicated systems** | | |
| R10+ | Titans, AI (grunts, spectres), game modes, multiplayer, campaign | — | |

**Public:** the repo stays private until you can start the game, pick a map and walk it with real pilot movement.

## Rules

See [CLAUDE.md](CLAUDE.md) (non-negotiable rules, layout, conventions). In short: our code only, clean-room, no game data in git, no leaks, no DRM bypass, GPL-3.0, Rust 2024 + Bevy 0.19.1.
