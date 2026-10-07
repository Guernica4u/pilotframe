# Next session: start here

State on 2026-10-04: **M0 and M1 done** (disc reader, all 18 XBAR packs decode). Pushed to `Guernica4u/pilotframe`. Friend may start Track B ([TRACK_B.md](TRACK_B.md)).

## Full state snapshot (2026-10-05, written before a context compact)

**Mission:** an open-source Titanfall that's always playable on PC, moddable, and ours (see PLAN.md "Why this exists"). Repo `Guernica4u/pilotframe` stays **private until it boots to the menu and you can pick a map and walk it with real pilot movement**.

**Local files** (all in gitignored `Resources/`):
- `TitanFall-XBOX360.iso`: the owner's 360 dump. Single XGD3 disc, build `R1X360_CL108433`.
- `pc/Titanfall (USA) (Disc 1-3).iso` + `[LOGS]` (DiscImageCreator, 2022): the owner's PC retail discs. Plain split ZIP, 538 entries, about 50 GB, 214 VPKs, no encryption.
- `r1_titanfall_DLCS/Titanfall DLCS/`: the owner's own PC DLC download (checked 2026-10-05). All 3 packs: DropPod1 = Expedition (mp_runoff, mp_swampland, mp_wargames), DropPod2 = Frontier's Edge (mp_harmony_mines, mp_haven, mp_switchback), DropPod3 = IMC Rising (mp_backwater, mp_sandtrap, mp_zone_18). That's 9 `vpk/client_mp_*.bsp.pak000_000.vpk` data VPKs (~6.8 GB) plus installer exes (never open them). **The `englishclient_mp_<map>.bsp.pak000_dir.vpk` index files are missing.** Those came with a base-game patch, and the retail disc only has dir VPKs for launch maps. **Resolved 2026-10-06:** the owner's 9 English dir VPKs were copied from their own install (`C:\Documents 2.0\r1 delta\vpk`) into the DLC `vpk` folder, so all 9 DLC maps are readable.
- `extracted/`: decoded 360 packs + `*.list.txt`. `inventory.json`: 360 inventory.
- `reference/`: Colony overhead/minimap, a town screenshot, `menu/main_menu_console.png`, `links.txt` (executions, terminations, animations, movement-basics videos, Fandom wiki), `hud/` (pilot HUD shot), `video/training_full_r1delta_1080p60.mp4` (the owner's full training-mission recording in R1Delta, 1080p60, 577 MB, added 2026-10-06; main footage for measuring movement in M5), `video/colony_footage.mp4`, and PC menu screenshots in `menu/`. All are described in [reference-ui.md](reference-ui.md).

**Decisions:** PC is the main source, 360 supported at lower priority. Movement follows PC values. First map: Colony. Match the original graphics. Audio right after M3. Rebuild the original menu at runtime from the player's files. Setup screen with a Windows file picker accepts a 360 ISO, PC Disc 1 ISO, or PC install folder (auto-detected). Cache goes in `data/` next to the exe. DLC only from the owner's own download. Branding/logo is the owner's call (rule relaxed; owner keeps their logo). **Still non-negotiable:** no game files in git, clean-room code (no decompiled/leaked code, never read .xex/.lex/.exe/.dll), no game scripts.

**R1Delta:** the owner installed it (it needed a Defender exclusion; the installer downloads files via curl). It may be used to *play* the original PC game as a side-by-side reference, e.g. OBS 60 fps recordings for M5. Never use its code. The owner's install files are theirs to use (owner's decision, 2026-10-06).

**Friend / Track B:** branch `track-b/movement-sandbox` adds `pf-movement` + `pf-sandbox` (Bevy 0.19.1, leafwing 0.21). The owner tested it, and it builds and runs, but they found it un-Titanfall-like (no parkour yet) and don't care about it for now. A parkour prompt was drafted for the friend. Local worktree: `C:\Documents 2.0\Vibecoding\pilotframe-trackb` (run with `CARGO_TARGET_DIR` set to the main repo's `target`, then `cargo run -p pf-sandbox`). PR not opened yet. The friend should merge `main` first.

**Owner preferences:** Claude runs git/builds/pushes itself (minimal terminal for the owner), uses AskUserQuestion pop-ups for decisions, keeps replies short, and watches the weekly usage limit. Commit identity: `guernica4u <Guernica4u@proton.me>`. Commit messages via a temp file (PowerShell here-strings break `git commit -F -`).

**Next action: Thursday, "start M2"**: PC Colony geometry on screen (steps below).

## Progress 2026-10-06 (M2 started early, branch m2/pc-reader)

- **Done:** `pf-iso9660` (ISO 9660 + multi-extent), `pf-zip` (split ZIP64 over the 3 discs, Deflate stream extract + CRC check), `pf-fmt-vpk` (Respawn VPK v2.3 dir: names + chunks), `Tools/pf-pc` (`list`, `extract`, `vpk`). Colony VPKs extract in ~6 s to `Resources/pc_extracted/vpk/` (CRC OK). The Colony dir lists 5,608 named files: `depot/r1dev/game/r1/maps/mp_colony.bsp` + 134 `.bsp_lump` files, 1,971 `.vtf`, 702 `.mdl`, etc. List: `Resources/pc_extracted/colony_files.txt`.
- **Also done 2026-10-06:** `pf-lzham` (MIT lzham_alpha C lib via `lzham-alpha-sys`, dict 2^20). Needs CMake: set `CMAKE` to VS Build Tools' cmake.exe. `pf-pc vpk-extract` unpacks all 5,608 Colony files (1.5 GB, CRCs OK) to `Resources/pc_extracted/colony/`. `pf-fmt-bsp` reads rBSP v29 world geometry (layouts from bsp_tool, credited; LitBump stride 44 and Unlit stride 20 confirmed). **`pf-viewer` shows Colony** (188k triangles, 309 materials, flat colour per material, fly camera): `cargo run -p pf-viewer --release -- Resources/pc_extracted/colony/depot/r1dev/game/r1/maps/mp_colony.bsp`. Bevy 0.19.1 is pinned on main now.
- **M3 textures done 2026-10-06:** `pf-fmt-vtf` (VTF 7.5 + VMT). The viewer uploads DXT1/3/5 straight to the GPU (BC1/2/3) with repeat + anisotropic sampling. Colony: 277 materials textured, 10 flat (14 VMTs live in `mp_common`, not the Colony VPK; 5 VTFs use unknown format 34). Viewer finds `materials/` by walking up from the map path.
- **Blend + common done 2026-10-06:** `blend.wgsl` (ExtendedMaterial, embedded in the exe) mixes `$basetexture2` by vertex alpha (vertex colour = RGBA8 at +16 in every vertex kind). `pf-pc <disc1> materials-for <bsp> <map materials> <out>` streams `mp_common` archive 000 (12.7 GB, about 13 s) once and keeps only the missing VMTs/VTFs, written to `Resources/pc_extracted/common/`. The viewer searches `materials` and `common/materials`. Colony: 280 textured, 7 blended, 0 flat.`n- **Lightmaps done 2026-10-06:** lump 0x53 = 8-byte page headers; 0x62 = per page 2 RGBA8 layers (layer 0 = baked light with brightness in alpha; layer 1 unknown, probably bump direction); 0x69 = per page 1 RGBA8 layer, red = sun shadow mask. The sun is the type-3 record in WORLD_LIGHTS (0x36, 100-byte records; direction at +24, colour at +12); type 5 = sky ambient. `world.wgsl` does albedo x (sky x a x PF_SKY_SCALE + sun x mask x N.L x PF_SUN_SCALE). Scales are tuned by eye (1.5 / 0.3); the true decode is still unknown. PF_LM_DEBUG=1/2 shows mask / N.L. Source winding is CW: don't trust is_front for normals.
- **Fog/decals/spawn done 2026-10-06:** decals ($decal 1, 36 on Colony) get `depth_bias` 100 (fixes posters flickering in walls). `colony_fog()` approximates env_fog_controller from `mp_colony_env.ent` (hard-coded for now). The camera starts at the first `info_spawnpoint_human_start` from `<map>_spawn.ent`. Grey clear colour until the skybox. Reference = the owner's gameplay video `Resources/reference/video/colony_footage.mp4`, not the marketing render.
- **Not done yet, so it still looks flat:** normal maps (`` _nml), specular/cubemaps, static props (most of the map's detail: trees, grass, rubble, wires), detail sprites/grass, skybox (3D sky_camera), particles/smoke, exact lightmap decode.
- **Static props done 2026-10-06:** `pf-fmt-mdl` reads MDL v52 + VVD v4 (fixups) + VTX v7 (33-byte strip groups, `.dx11.vtx`) LOD 0; all 702 Colony models parse (`examples/check_models.rs`). `Bsp::static_props()` reads game lump `sprp` v12 (84-byte props: origin, angles, model u16 at +24, scale at +72). The viewer places all 16,891 props from 703 models (`props.rs`), using the Source yaw/pitch/roll to Bevy rotation. Props are lit flat ambient + sun x N.L (no shadows; prop vertex lighting is not found yet). `materials-for` now also pulls missing prop models and their materials from mp_common.
- **Next (M3 rest):** skybox (sky_camera 3D sky), normal maps, prop lighting, particles. Then (game lump 0x23 + .mdl); confirm winding then enable culling. Also: turn the pf-pc steps into the first-launch setup (pick Disc 1, extract to `data/`).

## Update 2026-10-05

- **PC discs are usable:** `Resources/pc/Titanfall (USA) (Disc 1-3).iso`. Each holds part of one split ZIP (`Titanfall.cd` = central directory, `Titanfall.z01`… = data), with no encryption flags. Contents: `vpk/client_mp_*.bsp.pak000_*.vpk` (Respawn VPK v2.3, LZHAM) and more. Never open `.exe`/`.dll`/scripts.
- **Found 2026-10-06:** inside the PC ZIP, 210 of 214 VPKs use Deflate (method 8), 4 are stored. Deflate means no random access, so `pf-zip` should stream-extract (e.g. `miniz_oxide`, MIT/Apache/zlib) into the `data/` cache on first launch rather than reading in place.
- **New M2 order:** (0) write `pf-zip` (read-only multi-disc ZIP over the 3 ISOs via ISO9660/UDF + pf-vfs) and `pf-fmt-vpk` (Respawn VPK + LZHAM; check that the LZHAM crate's license is GPL-compatible). (1) Draw **PC** `mp_lobby`/`mp_colony` geometry (rBSP v29 LE, bsp_tool `titanfall.py`). (2) Use the same map from the 360 (`PSBr`, BE) to map 360 lumps onto PC lumps.
- **Track B** (friend) pushed `pf-movement` + `pf-sandbox` on `track-b/movement-sandbox`. Shared-file changes reviewed OK (Bevy 0.19.1 pinned). `main` is now rustfmt-clean. Map collision must implement `pf_movement::CollisionWorld` (parry TriMesh, Z-up→Y-up at conversion).

## Agreed path to going public (2026-10-05)

M2 Colony geometry (PC) → M3 textured Colony → **audio** (ambience/footsteps/menu music) → M4 walk with collision (`pf-movement` in the real map) → M5 parkour → **setup screen with file picker + rebuilt original menu + map select** → go public. Details in the PLAN.md decisions table.

## M2 plan: lobby geometry on screen (Track A). Superseded: now PC Colony first, see above

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
