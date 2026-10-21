# Pilotframe

An unofficial rebuild of **Titanfall (2014, PC)** in Rust and [Bevy](https://bevyengine.org), aiming to be a 1:1 copy of the original. It runs from **your own EA install** of the game: maps, models, textures and scripts are read from it at run time, and none of them are in this repository or its releases.

> **Pilotframe is unofficial and not affiliated with, endorsed by, or connected to Electronic Arts or Respawn Entertainment.** "Titanfall" is a trademark of Electronic Arts Inc.

**Progress page:** https://guernica4u.github.io/pilotframe/

## Status

> **On hold (21 Oct 2026).** Development is paused until the owner restarts it. The code here is the latest state.


**The only system perfected so far is pilot movement** (ground, air, double jump, wall-run, wall-hang, mantle), ported from the game's own code and values and checked against recordings of the real game.

Everything else is work in progress. You can walk the Colony map with real pilot movement; there is no shooting, sound, working HUD, Titans, AI or multiplayer yet. The script engine (our own Squirrel interpreter) runs the game's client scripts, and the HUD comes next. See [PLAN.md](PLAN.md) for the roadmap.

## Running it

You need Titanfall installed through the EA app (final patch, v1.0.10.1). Without it, only a grey test map runs.

**From a release (Windows):** unzip it, then start `pf-game.exe` with `PF_GAME_DIR` set to your install (the folder holding `vpk/`) and `PF_MAP=colony`:

```bat
set PF_GAME_DIR=C:\Program Files\EA Games\Titanfall
set PF_MAP=colony
pf-game.exe
```

**From source:** install stable Rust, then:

```bash
cargo run -p pf-game --release
```

The game looks for the install under `Resources/` in the working folder, or wherever `PF_GAME_DIR` points. Other settings are in [docs/tools.md](docs/tools.md).

## How it's built

- **All code is ours.** The game's `client.dll` is studied for reference, clean-room: what it does (values, formulas, layouts) is written up in [docs/port/](docs/port/), and the Rust is written from those notes. No decompiled code is ever committed.
- **No game data in git**, and the engine never assumes the game's files exist.
- No leaked source, no DRM bypass. `Titanfall.exe` and EA's activation files are never touched.

Rules and layout: [CLAUDE.md](CLAUDE.md). Credits for what we learned from: [docs/RESEARCH.md](docs/RESEARCH.md).

## License

GPL-3.0-only. See [LICENSE](LICENSE).
