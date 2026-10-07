# Pilotframe: notes for Claude Code sessions

Unofficial Rust + Bevy rebuild of Titanfall (2014), using assets from each player's own Xbox 360 disc. Not affiliated with EA or Respawn. Read [PLAN.md](PLAN.md) (decisions + milestones) and [docs/formats/](docs/formats/) before changing format code.

## Non-negotiable rules

- **All project code is ours, in Rust.** What goes into the repo and releases is written by us. Never paste, or translate line by line, code from anywhere (decompiled game code, open-source tools, mods). Normal crates.io dependencies are fine (license rule below).
- **The owner's own copy of the game may be studied any way, including decompiling or disassembling its executables** (owner's decision, 2026-10-07). Work clean-room style: findings (values, formulas, struct layouts, behaviour) go into notes in `docs/`, and the Rust is written from those notes. No decompiled code, disassembly listings or pseudo-code are committed.
- **Anything legal may be used as reference** (rules relaxed 2026-10-06): open-source projects (bsp_tool, Northstar, R1Delta, public Source SDK 2013, etc.), wikis, docs, videos, and the owner's own game files, including the Squirrel game scripts (read them to learn values and behaviour; never copy them into the repo or ship them). Credit what we learned from in `docs/RESEARCH.md`.
- **Still banned (illegal whoever owns the game):** leaked source (Valve/Respawn leaks, and any files in other projects that come from them), leaked Xbox SDK (XDK) docs, and any DRM bypass (e.g. the Xbox One disc, or unpacking a copy-protected executable).
- **No game data in git.** Nothing from `Resources/` (ISO, extracted packs, inventory JSON) is ever committed, pasted into issues, or used as a test fixture. Docs may describe struct layouts, but no hex dumps longer than a header.
- **Engine never assumes original assets exist.** Missing assets fall back to placeholders.
- **Branding (name, logo, icon) is the owners' call.** Don't flag it. The project logo is a deliberate choice. Keep the "unofficial / not affiliated with EA or Respawn" notice in the README and title screen.
- **License:** GPL-3.0-only. Only add dependencies with GPL-compatible licenses (MIT, Apache-2.0, BSD, ISC, zlib, MPL-2.0, LGPL, GPL-3.0). Check before adding.

## Layout

- `crates/`: libraries. Layer 0 crates (`pf-vfs`, `pf-xdvdfs`, `pf-fmt-*`, later `pf-movement`, `pf-sim`) have **no Bevy dependency and no `std::fs`** outside native-only code, so they compile to wasm32.
- `Tools/`: research CLIs (`pf-inventory`, `pf-probe`, `pf-bpk`).
- Bevy crates: `pf-world` (draws a map, gives its collision), `pf-viewer` (fly-through tool), `pf-game` (the game: menus, map select, player). Bevy is pinned workspace-wide (0.19.1).

## Conventions

- Rust 2024 edition, stable toolchain. Run `cargo fmt`, `cargo clippy --workspace --all-targets` (must be clean), `cargo test --workspace` before committing.
- **Tests:** CI-safe tests use synthetic data built in the test. Real-data tests call `pf_testdata::dump()` and return early when `PF_GAME_DUMP` isn't set.
- **Simulation:** fixed timestep. Movement is a pure function of (state, input, world queries), so it can be replayed and network-predicted later. Simulation uses original game units (~1 inch). Convert Z-up to Bevy's Y-up once, at the asset-conversion boundary.
- **Gameplay values live in data files** (`.ron`), each with its source and confidence noted in `docs/movement/`.
- Match the surrounding code's style and comment density. Explain Rust/Bevy-specific choices briefly; the owners are newer to Rust.

## Workflow

- Two tracks: **Track A** (game files to maps, needs a disc dump) and **Track B** (movement sandbox, needs no game files). Work on a branch, merge through pull requests to `main`.
- Commit messages end with a `Co-Authored-By` line for Claude.
