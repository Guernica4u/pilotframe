# Tools

## In the repo

- **`pf-game`**, the game. `Tools/play.cmd` builds it in release mode and starts it from the project folder (the desktop shortcut runs it).
- **Dev environment variables** (`pf-game`):

  | Variable | Does |
  |---|---|
  | `PF_MAP=colony` | start on Colony instead of the greybox test map (the default) |
  | `PF_WINDOW=1920x1080` | window size |
  | `PF_SANDBOX_SCREENSHOT=out.png` | save a screenshot after `PF_SCREENSHOT_DELAY` seconds (default 2), then quit |
  | `PF_DEV_ADS=1` | hold aim |
  | `PF_DEV_FIRE=1` | hold fire |
  | `PF_DEV_TURN=<deg/s>` | turn by itself |
  | `PF_DEV_PITCH=<deg>` | hold the view at a pitch (up is positive) |

  Running the exe directly needs `BEVY_ASSET_ROOT=crates/pf-game`.
- **`pf-viewer <map.bsp> [materials...]`**: fly through a map with no collision.
- **`pf-vpk list|extract`**: VPK contents (`Tools/pf-vpk`).

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

**Reference shots** (taken once in R1Delta): 1920×1080, FOV 70, default pilot loadout, looking at the sky or a plain wall. Poses: `hip`, `ads`, `fired` (after 5 shots).
