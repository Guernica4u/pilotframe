# Next session: start here

State on 2026-10-04: **M0 and M1 done** (disc reader, all 18 XBAR packs decode). Pushed to `Guernica4u/pilotframe`. Friend may start Track B ([TRACK_B.md](TRACK_B.md)).

## M2 plan: lobby geometry on screen (Track A)

1. **Find lump data.** Parse the 2,064-byte `PSBr` entry in `mp_lobby.bpk`: 16-byte header (magic, version 29, ...) plus 128 × 16-byte lump records, big-endian. Check whether lump offsets/sizes point into one combined stream or match other pack entries' sizes or ids. Start by comparing lump sizes against the entry sizes in `Resources/extracted/mp_lobby.list.txt`.
2. **Port struct layouts** from bsp_tool `branches/respawn/titanfall.py` + `titanfall_x360.py` (GPL-3.0, credit in the file header). New crate `crates/pf-fmt-bsp` (Layer 0, `binrw` or hand-rolled BE readers like `pf-fmt-bpk`).
3. **Decode geometry lumps:** vertices, vertex normals, mesh indices, meshes, and vertex-format lumps. Sanity check: bounding box size and index ranges stay within vertex count.
4. **Add Bevy** (pin the latest stable in workspace deps). New crate `crates/pf-viewer`: load ISO → pack → BSP → one `Mesh`. Fly camera (WASD + mouse, controller later). Flat shading with normals. Source Z-up → Bevy Y-up.
5. **Tests:** synthetic BSP with 1 triangle (CI). Real-data test: lobby has >0 meshes and all indices are in range.
6. Repeat for `mp_colony`. Update `docs/formats/bsp.md`, PLAN.md and the roadmap page.

**Done when:** `cargo run -p pf-viewer -- Resources/TitanFall-XBOX360.iso mp_lobby` opens a window with the lobby's shapes and you can fly around.

## Pending research (optional, user runs in Mistral)

- 360 gameplay footage for movement measurement (prompt given in chat on 2026-10-04).
- Gameplay data/rules: weapons, titans, Attrition, burn cards, tick rate.
- Paste results to Claude. They get checked and added to [RESEARCH.md](RESEARCH.md).

## Reminders

- Claude runs git/builds/pushes. The user prefers fewer terminal steps and pop-up questions (AskUserQuestion) for decisions.
- Local test data: set `PF_GAME_DUMP` to `Resources\TitanFall-XBOX360.iso`.
