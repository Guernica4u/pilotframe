# Gameplay data: community sources (cross-check only)

Collected 2026-10-04 via an external research assistant. **Most numbers were datamined from PC game files in 2014.** Our source of truth will be the **360 disc's own settings files** (KeyValues text inside the XBAR packs, allowed as data). Use the values below to cross-check, and note any 360 differences.

## Pilot movement (Denkirson boards via Gameranx, Mar 2014, datamined PC)
- Run 180.6 u/s, sprint 269.85 u/s. Wall-run max is about 2× sprint (Wiki: 1.75 s per wall, 3.5 s with Enhanced Parkour Kit).

## Titans (same source)
| Chassis | HP | Walk / sprint (u/s) | Dashes | Dash recharge | Melee |
|---|---|---|---|---|---|
| Atlas | 8000 | 280 / 420 | 2 | 4.17 s | 1000 |
| Ogre | 10500 | 265 / 400 | 1 | 4.55 s | 1875 |
| Stryder | 5500 | 300 / 435 | 3 | 3.30 s | 1125 |
- Titanfall drop lasts 6 s (3 s with Warpfall). The first Titan is ready about 4 min after match start, sooner with kills. Core charges in 200 s.
- **Unknown:** shield values/regen, embark/disembark timings.

## Weapons (Dissentia sheet + Symthic, datamined PC, patched Mar 21 / Apr 11 / May 14 2014)
- Sheet: docs.google.com/spreadsheets/d/1ScR34IxxUN9sFw_XBxB0sGhqeaqi7-ATE9aro7HZp04. Symthic archived at web.archive.org (symthic.com/titanfall-stats).
- Examples (damage near→far): R-101C 50→40, R-97 40→30, Hemlok 60→45, G2A4 99→66, Longbow 120 (no falloff), Smart Pistol 67 (no falloff). Spectre 200 HP. Grunts die to 1 Smart Pistol hit.
- Smart Pistol lock: 0.37–0.65 s on pilots, 0.1–0.2 s on minions (wiki.gg).

## Attrition (Fandom/IGN wikis)
- Points: Grunt 1, Spectre 1, Pilot 5, Titan 5, Pilot+Titan 10. Score limit 300 (raised in the Mar 21 2014 update). Time limit 10 min.
- Epilogue: no respawns. The losers run to an evac dropship and the winners hunt them. The dropship can be destroyed.
- Minions keep spawning until the Epilogue (Mar 21 2014 change). Drop pods carry squads of about 5. **Unknown:** per-team counts, spawn cadence.
- Burn cards: 3 slots, one per life. Titan cards last until that Titan dies. Full list on the Fandom wiki and gamepressure.

## Engine / platform
- **Xbox 360 (Digital Foundry, Apr 2014):** 1040×600 with 2× MSAA. Unlocked frame rate up to 60 fps (tearing), with an optional 30 fps lock. Textures streamed from DVD+HDD. Reduced textures, near-identical geometry, "feature-complete." **360 footage may be up to 60 fps, which helps movement measurement.**
- PC FOV 70–90° (internal pilot FOV 70). Console TF1 had no FOV slider.
- **Unknown:** TF1 tick rate (TF2 measured 60 Hz server / 20 Hz client) and console aim-assist values.
- Networking architecture: Jon Shiring, GDC 2014, "The Online Tech of Titanfall."
