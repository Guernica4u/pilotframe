# Tools

## In the repo

- **`pf-game`**, the game. `Tools/play.cmd` builds it in release mode and starts it from the project folder (the desktop shortcut runs it).
- **Dev environment variables** (`pf-game`):

  | Variable | Does |
  |---|---|
  | `PF_MAP=colony` | start on Colony instead of the greybox test map (the default) |
  | `PF_GAME_DIR=<folder>` | the EA install (the folder holding `vpk/`); otherwise it's searched for under `Resources/` |
  | `PF_WINDOW=1920x1080` | window size |
  | `PF_SANDBOX_SCREENSHOT=out.png` | save a screenshot after `PF_SCREENSHOT_DELAY` seconds (default 2), then quit |
  | `PF_DEV_ADS=1` | hold aim |
  | `PF_DEV_FIRE=1` | hold fire |
  | `PF_DEV_TURN=<deg/s>` | turn by itself |
  | `PF_DEV_PITCH=<deg>` | hold the view at a pitch (up is positive) |
  | `PF_REPLAY=game.csv` | headless: run a real-game movement recording's inputs through our movement (test map, or Colony with `PF_MAP=colony`) and write `game.ours.csv`, then exit |

  Running the exe directly needs `BEVY_ASSET_ROOT=crates/pf-game`.
- **Game files** are read straight from the install's VPKs (`pf_world::gamefs`): the shared `mp_common` pack at start, a map's pack when it loads. Loose files in `mods/` (in the working folder, gitignored) override them, using the game's own paths, e.g. `mods/scripts/weapons/mp_weapon_rspn101.txt`.
- **`pf-viewer <map>`** (e.g. `mp_colony`): fly through a map with no collision.
- **`pf-vpk list|extract`**: VPK contents (`Tools/pf-vpk`), for reading files by hand; the game doesn't need anything extracted.

## Local only (never committed)

These read decompiled code or game data, so they live in gitignored `Resources/re/scripts/`:

- **`DecompileAll.java`** (Ghidra headless): decompiles every function of a DLL into `Resources/re/full/<dll>/functions_NNN.c` with an `index.tsv`. client.dll (38,284 functions) takes about 15 minutes.
- **`fn.py`**: looks things up in that decompile, in about a second.
  - `python -I fn.py client <addr>` prints the function at an address.
  - `-s "<text>"` lists the functions containing a string.
  - `-c FUN_x` lists the callers of a function.
- **`capture_ours.sh [WxH]`**: builds the game and screenshots it in set poses (hip, ads; looking at the sky) into `Resources/reference/ours/`. It then runs `compare.py` against the reference shots in `Resources/reference/r1delta/<pose>.*`.
- **`compare.py REF OURS OUT`**: writes three files:
  - an overlay (reference red, ours green);
  - a sheet of reference-vs-ours crops per HUD region;
  - a report of each region's offset in pixels, with a match score.

- **`native.py <Name>`**: explains one native function: its signature and help text, the engine code behind it (plus what it calls), the struct offsets it touches, and every game script that calls it.
- **`datamaps.py [filter]`**: every field the game names in its data tables (name, type, offset in its object), e.g. `m_nButtons` at player+0x2cdc.
- **The script oracle (`oracle.py`):**
  - `oracle.py make` writes `Resources/reference/oracle/pf_oracle.nut` from `crates/pf-squirrel/oracle/cases.txt` (138 cases).
  - Run that file in the real game's client VM through R1Delta's console, and save the log.
  - `oracle.py compare game.log ours.log` then compares the results line by line.
- **Movement replay:**
  - `telemetry.py keys` (in R1Delta): shows which button bits each key sets.
  - `telemetry.py record [secs]`: logs the real pilot's position, velocity, flags, buttons and angles each frame (read-only) into `Resources/reference/moves/`.
  - `PF_REPLAY=<that csv>` runs the same inputs through our movement.
  - `movement_compare.py <csv>` prints the position gap, jump heights and air times, and top speed, and draws game-versus-ours plots.
  - Self-test: replaying our own output gives a gap of 0.00.
- **`helpers/`**: 38 older one-off research scripts (string and caller finders, cvar readers, recording analysis), indexed in its README.
- **Script research** (see docs/port/scripts.md):
  - `natives.py`: every native function the scripts can call, from all three DLLs.
  - `sqreg.py`: launcher.dll's static Squirrel tables.
  - `script_calls.py`: calls the scripts make that aren't found yet, per VM.
  - `include_graph.py`: which scripts each VM loads, in order.
  - `lang_survey.py`: which language features the scripts use.
- **The script corpus:** `Resources/re/corpus/` (extracted with `PF_READ_SCRIPTS=1 pf-vpk extract … scripts/`), for these tools only.
- **Full decompiles:** client.dll, engine.dll (33,277 functions) and launcher.dll (3,377), each under `Resources/re/full/<dll>/`.

**Reference shots** (taken once in R1Delta): 1920×1080, FOV 70, default pilot loadout, looking at the sky or a plain wall. Poses: `hip`, `ads`, `fired` (after 5 shots).
