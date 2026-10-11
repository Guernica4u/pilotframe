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

| Step | What | Port from | Status |
|---|---|---|---|
| R0 | Reset: one source, cleanup, new plan and docs | — | done |
| R1 | Read the install directly: install path config, VPK mounting (no extraction step), mod-override folder | file formats | done |
| R2 | Colony from the EA files: geometry, textures, lightmaps, props, collision, sky | BSP + materials | mostly exists (from older files), re-check |
| R3 | Pilot movement: ground, air, double jump, wall-run, wall-hang, mantle, crouch, view punches. Values are nearly all from the game already; check the behaviour code against the decompile (`max_slope_deg` is still a guess). | client.dll movement prediction | partly ported |
| R4 | Viewmodel: animations, sway, recoil, ADS, sight proxies, gun position (ours sits lower and further right than the reference) | client.dll + weapon scripts | partly ported |
| R5 | **Script test (go/no-go).** Our own Squirrel interpreter, in Rust, runs the game's `cl_main_hud.nut` from the install far enough to build the pilot HUD with a few native functions. **Pass:** the HUD is built by the game's script and matches the reference shots, within 2 sessions. Then scripts drive the HUD and game rules from here on. **Fail:** keep porting each script by hand (today's `hud.rs`). | the game's `.nut` scripts | |
| R6 | Shooting: bullet traces, impact effects and decals per surface, damage on targets | client.dll + effect tables | |
| R7 | Sound: weapons, footsteps, ambience. First find where the EA install keeps working audio (its VPK sound entries fail their CRC; an installer rebuilds them). | sound scripts + files | |
| R8 | First-person animations: sprint, wall-run, hang, mantle, reload variants | MDL sequences + client.dll | |
| R9 | Menus from the game's own UI files (`.res`, and the scripts if R5 passed) | UI files | |
| R10+ | More weapons, Titans, more maps, AI, multiplayer | — | |

**Public:** the repo stays private until you can start the game, pick a map and walk it with real pilot movement.

## Rules

See [CLAUDE.md](CLAUDE.md) (non-negotiable rules, layout, conventions). In short: our code only, clean-room, no game data in git, no leaks, no DRM bypass, GPL-3.0, Rust 2024 + Bevy 0.19.1.
