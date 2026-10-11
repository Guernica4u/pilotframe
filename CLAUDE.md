# Pilotframe: notes for Claude Code sessions

Unofficial Rust + Bevy rebuild of Titanfall (2014, PC), a **1:1 copy** of the original, running from the player's own EA install. Not affiliated with EA or Respawn. Read [PLAN.md](PLAN.md) first (source, method, roadmap); tools are in [docs/tools.md](docs/tools.md).

## Non-negotiable rules

- **All project code is ours, in Rust.** What goes into the repo and releases is written by us. Never paste, or translate line by line, code from anywhere (decompiled game code, open-source tools, mods). Normal crates.io dependencies are fine (license rule below).
- **One source: the owner's EA install** (final patch v1.0.10.1, under gitignored `Resources/`). Its data files and scripts are read for values and behaviour. `client.dll` and the other plain DLLs may be decompiled (owner's decision, 2026-10-07), clean-room: findings (values, formulas, struct layouts, behaviour) go into `docs/port/`, the Rust is written from those notes, and no decompiled code, disassembly or pseudo-code is committed. Never touch `Titanfall.exe` (DRM) or `Core/` (EA activation), and never modify the install.
- **Port, don't invent.** Every system comes from the game's code and data. Anything not found yet is a marked placeholder, and we say so. Label work "ported" or "ours".
- **R1Delta** is for playing side by side and capturing reference shots and video only. Never modify the owner's R1Delta folders.
- **Still banned (illegal whoever owns the game):** leaked source (Valve/Respawn leaks, and any files in other projects that come from them) and any DRM bypass.
- **No game data in git.** Nothing from `Resources/` is ever committed, pasted into issues, or used as a test fixture. Docs may describe struct layouts, but no hex dumps longer than a header. Game scripts are never copied into the repo or shipped.
- **Engine never assumes original assets exist.** Missing assets fall back to placeholders.
- **Branding (name, logo, icon) is the owners' call.** Keep the "unofficial / not affiliated with EA or Respawn" notice in the README and title screen.
- **License:** GPL-3.0-only. Only add dependencies with GPL-compatible licenses (MIT, Apache-2.0, BSD, ISC, zlib, MPL-2.0, LGPL, GPL-3.0). Check before adding. Credit what we learned from in `docs/RESEARCH.md`.

## Layout

- `crates/`: libraries.
  - Layer 0 (`pf-vfs`, `pf-fmt-*`, `pf-lzham`, `pf-movement`) has **no Bevy dependency and no `std::fs`** outside native-only code, so it compiles to wasm32.
  - Bevy crates: `pf-world` (draws a map, gives its collision), `pf-viewer` (fly-through tool) and `pf-game` (the game). Bevy is pinned workspace-wide (0.19.1).
- `Tools/pf-vpk`: list and extract VPK contents.
- `docs/port/`: one note per ported system (HUD, weapons, movement…).
- `docs/formats/`: file format notes.

## Conventions

- Rust 2024 edition, stable toolchain. Run `cargo fmt`, `cargo clippy --workspace --all-targets` (must be clean) and `cargo test --workspace` before committing.
- **Tests:** CI-safe tests use synthetic data built in the test. Tests on real game files return early when those files aren't present.
- **Simulation:** fixed timestep. Movement is a pure function of (state, input, world queries). It uses the game's units (~1 inch). Convert Z-up to Bevy's Y-up once, at the asset boundary.
- **Gameplay values live in data files** (`.ron`) or are read from the game's own files, each with its source noted in `docs/port/`.
- Match the surrounding code's style and comment density. Explain Rust/Bevy-specific choices briefly; the owners are newer to Rust.

## Workflow

- One roadmap step per branch, merged to `main` by pull request with the check result (compare report or video frames) attached.
- Before building, check `pf-game` isn't running (the owner may be playing).
- Commit messages end with a `Co-Authored-By` line for Claude.
