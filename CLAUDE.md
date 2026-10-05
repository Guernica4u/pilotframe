# Pilotframe: notes for Claude Code sessions

Unofficial Rust + Bevy rebuild of Titanfall (2014), using assets from each player's own Xbox 360 disc. Not affiliated with EA or Respawn. Read [PLAN.md](PLAN.md) (decisions + milestones) and [docs/formats/](docs/formats/) before changing format code.

## Non-negotiable rules

- **Clean room.** Never use decompiled/disassembled game code, leaked source (Source engine or Respawn leaks), or leaked Xbox SDK (XDK) docs. Never read, decrypt, or run `.xex`/`.lex` files. We study data formats from the owner's own files and write original code.
- **Game scripts are off-limits.** The packs contain Squirrel game scripts. Don't open, print, or summarise them. Numeric settings files (KeyValues data) may be read as data.
- **No game data in git.** Nothing from `Resources/` (ISO, extracted packs, inventory JSON) is ever committed, pasted into issues, or used as a test fixture. Docs may describe struct layouts, but no hex dumps longer than a header.
- **Engine never assumes original assets exist.** Missing assets fall back to placeholders.
- **No EA/Respawn trademarks or art** in names, UI, or assets.
- **License:** GPL-3.0-only. Only add dependencies with GPL-compatible licenses (MIT, Apache-2.0, BSD, ISC, zlib, MPL-2.0, LGPL, GPL-3.0). Check before adding.

## Layout

- `crates/`: libraries. Layer 0 crates (`pf-vfs`, `pf-xdvdfs`, `pf-fmt-*`, later `pf-movement`, `pf-sim`) have **no Bevy dependency and no `std::fs`** outside native-only code, so they compile to wasm32.
- `Tools/`: research CLIs (`pf-inventory`, `pf-probe`, `pf-bpk`).
- Bevy crates come later (`pf-world`, `pf-render`, `pf-player`, `pf-game`). Pin one Bevy version workspace-wide.

## Conventions

- Rust 2024 edition, stable toolchain. Run `cargo fmt`, `cargo clippy --workspace --all-targets` (must be clean), `cargo test --workspace` before committing.
- **Tests:** CI-safe tests use synthetic data built in the test. Real-data tests call `pf_testdata::dump()` and return early when `PF_GAME_DUMP` isn't set.
- **Simulation:** fixed timestep. Movement is a pure function of (state, input, world queries), so it can be replayed and network-predicted later. Simulation uses original game units (~1 inch). Convert Z-up to Bevy's Y-up once, at the asset-conversion boundary.
- **Gameplay values live in data files** (`.ron`), each with its source and confidence noted in `docs/movement/`.
- Match the surrounding code's style and comment density. Explain Rust/Bevy-specific choices briefly; the owners are newer to Rust.

## Workflow

- Two tracks: **Track A** (game files to maps, needs a disc dump) and **Track B** (movement sandbox, needs no game files). Work on a branch, merge through pull requests to `main`.
- Commit messages end with a `Co-Authored-By` line for Claude.
