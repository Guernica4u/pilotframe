# Track B: movement sandbox

Brief for the second developer's Claude Code session. Read [CLAUDE.md](../CLAUDE.md) and [PLAN.md](../PLAN.md) first.

## Setup

1. Check for Rust (rustup/cargo), Git and GitHub CLI (`gh`). On Windows, Rust also needs the Visual Studio C++ Build Tools. **Ask before installing anything.** The developer runs `gh auth login` themselves.
2. Set git name/email for this repo only (ask what to use).
3. Run `cargo test --workspace`. Tests needing game data skip themselves, which is expected.

## Goal: Milestone 4, then Milestone 5

No game files are needed for any of this.

- **`crates/pf-movement`**: Layer 0, no Bevy, no `std::fs`. A pure fixed-tick function: (state, input, collision queries) → new state. Original game units (~1 inch). Collision goes through a trait, backed by the test map now and real map data later.
- **`crates/pf-sandbox`**: a Bevy app with a homemade greybox map (floor, ramps, steps, boxes, long walls for wall-running), a first-person camera, and a capsule controller driven by `pf-movement` on `FixedUpdate`. Pin the latest stable Bevy and record the version.
- **Input:** keyboard/mouse and controller both first-class, through named actions (e.g. `leafwing-input-manager` if its license and Bevy version fit).
- **Order:** walk, sprint, jump, gravity, slopes, steps. Then (M5) double jump, wall-run, wall-hang, crouch, mantle, air control. (No slide: that's Titanfall 2.)
- **Tuning values** go in a `.ron` file, never hard-coded. Placeholders for now. Real values come from 360 footage later, each logged with its source in `docs/movement/`.
- **Tests:** unit tests for `pf-movement` without Bevy (jump apex height, can't pass through walls, step-up height).

## Workflow

- Branch `track-b/movement-sandbox`. Small commits. Open a PR to `main` when it's playable. Never push to `main` directly.
- Track A (map decoding) runs in parallel. Avoid editing `crates/pf-fmt-*`, `crates/pf-xdvdfs` and `Tools/`. If you must, explain why in the PR.
- `cargo fmt`, `cargo clippy --workspace --all-targets` (clean) and `cargo test --workspace` before every commit.
- The developer is newer to Rust and Bevy: briefly explain Rust/Bevy-specific choices. **Show a short plan and ask questions before writing code.**
