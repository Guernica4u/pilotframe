# Menu and HUD reference

Notes from the owner's PC screenshots (R1Delta, 2560×1440, 2026-10-06). The images live in gitignored `Resources/reference/menu/` and `hud/`. This is a layout guide for rebuilding the menus. Art and fonts come from the player's own files at runtime.

## Shared look

- Big title top left (light blue, condensed caps) over a long dashed bar that spans the screen.
- Left column of white caps menu items. The selected item gets a soft blue glow bar. Locked items are greyed out with a red padlock.
- "TITANFALL" wordmark bottom right. A bottom bar holds BACK / LEAVE on the left.
- Lobby countdown top right: "STARTING IN" plus a big number.
- Backgrounds: the IMC fleet in the clouds (lobby screens), blurred dropship interior (burn cards), blurred lobby scene (loadouts).

## Screens

| Screen | File | Contents |
|---|---|---|
| Main menu (PC) | `main_menu_pc_r1delta.png` | Huge TITANFALL logo over a live map scene. PLAY, INTRO, OPTIONS, CREDITS, QUIT. Player name + version bottom left. Respawn logo + © line at the bottom. **ADDONS / EDIT USERNAME / DISCORD and the news box are R1Delta additions, not original.** |
| Main menu (360) | `main_menu_console.png` | Console version. |
| Private lobby | `lobby_private.png` | PLAY TITANFALL: PRIVATE MATCH, TRAINING. Then PILOT LOADOUTS, TITAN LOADOUTS, BURN CARDS (n OPEN SLOT). Then PERSONAL STATS, CHALLENGES. Then OPTIONS. Chat box with "Say:" in the middle, player list on the right (mute icon, gen/level badge, name). Bottom: LEAVE, PROGRESSION: ENABLED. |
| Private match | `private_match.png` | STOP COUNTDOWN, SET MAP, SET GAME MODE, MATCH SETTINGS (greyed during countdown), loadouts, burn cards, OPTIONS. Map preview image with map name + mode (e.g. COLONY / PILOT HUNTER). MILITIA (teal rows) vs IMC (brown rows) team lists. |
| Pilot loadouts | `pilot_loadouts.png` | Presets on the left (RIFLEMAN, ASSASSIN, locked CQB). 3D pilot model in the middle. Cards: PRIMARY WEAPON (+ 2 attachment slots), ANTI-TITAN WEAPON, SIDEARM, TACTICAL ABILITY, ORDNANCE, KIT ×2. Each card has a name + one-line description. |
| Titan loadouts | `titan_loadouts.png` | Presets (ASSAULT, TANK, locked ARTILLERY). 3D titan model with chassis name + "CORE ABILITY: …" plate (e.g. ATLAS / DAMAGE CORE). OS VOICE portrait. Cards: WEAPON (+ attachment), TACTICAL ABILITY, ORDNANCE, KIT ×2. |
| Burn cards | `burn_cards.png` | 3 card slots (later ones "UNLOCKED AT LEVEL 9 / 11"), deck count "0/100" top centre, checkboxes "Autofill empty slots" and "Reverse sort order". |

Card header colours: blue = weapons, teal = tactical/ordnance, gold = kits.

## HUD

See [tf1-not-tf2.md](tf1-not-tf2.md#hud-from-r1delta-pc). Shots in `hud/`.

## Videos (`Resources/reference/video/`)

- `training_full_r1delta_1080p60.mp4`: full pilot training mission. Main source for measuring movement (M5).
- `colony_footage.mp4`: Colony gameplay. Used to check the map in M3/M4.
