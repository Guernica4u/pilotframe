# Research notes: formats, tools, and sources

Research snapshot: **2026-10-04**. Anything marked **(unverified)** has to be checked against the real disc in Milestone 0 before we rely on it.

> Unofficial project. Not affiliated with, endorsed by, or connected to Electronic Arts or Respawn Entertainment. "Titanfall" is a trademark of Electronic Arts Inc. It's named here only to say what this project is compatible with.

---

> **Update after M0 (2026-10-04):** the real disc answers several unknowns below. Maps and most assets are inside **Bluepoint `XBAR` packs** (`r1/bp_packdata/*.bpk`), not VPKs. Textures are in **Bluepoint streaming texture sets** (`.st1/.st2`). **Audio is Audiokinetic Wwise** (`AKPK` `.pck` files), which the community documents well (vgmstream, wwiser). That's better news than expected. Details: [formats/overview.md](formats/overview.md).

## 1. Short version

| Area | PC version (r1, 2014) | Xbox 360 version (Bluepoint port) | Our status |
|---|---|---|---|
| Disc container | n/a | XGD3 DVD, XDVDFS filesystem (well documented) | **Documented.** Open-source readers exist |
| Archives | Respawn VPK v2.3, chunked, LZHAM-compressed | **Unknown.** Could be VPK, Source-360-style `.360.*` files, or Bluepoint's own packing | **Needs our own research** |
| Maps (BSP) | `rBSP` v29, 128 lumps, geometry as "meshes" (not brush faces) | v29, **big-endian**. Header looks like Orange Box 360's. Grid, LevelInfo and static-prop gamelump layouts differ | PC: mostly documented (bsp_tool). 360: **stub only, never tested on real files** |
| Models | Studio MDL v52 (Respawn variant) + VTX/VVD | Probably big-endian `.360.mdl`-style variants **(unverified)** | PC: partly documented. 360: **undocumented** |
| Animations | Inside MDL (v52 animdesc) | Same, big-endian **(unverified)** | **Partly documented** (PC) |
| Textures | VTF (DXT/BC) | Probably VTFX (Source's 360 texture container) with Xenos tiling and big-endian DXT blocks **(unverified)** | VTFX header documented on VDC. Xenos untiling is in open-source code (Xenia, BSD). TF1 specifics **unknown** |
| Materials | VMT text keyvalues | Probably VMT or a compiled binary variant **(unverified)** | **Unknown** for 360 |
| Audio | ~35 GB of uncompressed audio (a deliberate CPU-saving choice) | Almost certainly XMA/XMA2, since 360 discs can't hold uncompressed audio **(unverified)** | XMA is decodable with FFmpeg (LGPL). The container/bank format is **unknown** |
| Scripts/settings | Squirrel `.nut` scripts, keyvalue `.set`/`.txt` settings | Probably present, maybe compiled **(unverified)** | Using these is a **policy question** (see PLAN.md §10) |

**The main takeaway:** almost all existing Titanfall tooling targets **PC Titanfall 2 and Apex**. Some covers PC Titanfall 1. **Almost nothing covers the Xbox 360 build.** The only 360-specific public code I found is a big-endian BSP branch in bsp_tool, and its author lists it as "lacks .bsps to test". **Expect to reverse-engineer most of the 360 data formats ourselves.** Milestones 0–3 are mostly format research, not engine work.

---

## 2. Disc and container layer (documented)

- **XGD3**: the 360 disc format used from 2011 on. ~8.3 GB usable per disc. Titanfall came out in 2014, so it's XGD3. Number of discs: **check in M0** (I couldn't confirm it).
- **XDVDFS**: the game partition's filesystem. Simple and fully documented. Data files inside are stored **plain (not encrypted)**. Only `default.xex` is encrypted/signed, and we never touch it.
- **Output formats** you'll run into:
  - **ISO** (raw disc image, ~7.8–8.5 GB). Redump-style dumps include the video partition and security sectors. "Game partition only" ISOs (XGD3 trimmed) are smaller.
  - **Extracted folder**: the XDVDFS contents written to normal files. Easiest for our tools.
  - **GOD (Games on Demand)**: an ISO repacked into STFS/SVOD container files for modded consoles (`iso2god`). Readable, but an extra layer. **We'll accept it but not recommend it.**
- **Tools** (we only need to *read*):
  - `xdvdfs` (Rust, MIT/Apache-dual per its repo. Check before depending on it): a modern XDVDFS reader. Good candidate to depend on or study.
  - `extract-xiso`: old C tool, works.
  - Velocity / wxPirs / `iso2god-rs`: STFS/SVOD/GOD tools (only needed if you dump as GOD).

## 3. BSP maps

- Source of truth: **bsp_tool** (Python, **GPL-3.0**, snake-biscuits). Branches: `titanfall.py` (PC + beta, supported), `titanfall_x360.py` (exists, **untested on real data**), `titanfall2.py`, `apex_legends*.py`.
- PC Titanfall: magic `rBSP`, version **29**, **128 lumps**. Geometry is in a MESHES/vertex/index lump family (closer to Quake 3/CoD mesh rendering than to Source brushes). Static props are in a gamelump. Some lumps are in external `.bsp_lump` files in later games. **Check whether TF1/360 does this.** Entities are in the entity lump plus separate `.ent` files (TF2/Apex do this. **Check for TF1**).
- 360 branch (from reading the bsp_tool source description, not copied): big-endian. Lump header layout inherited from Orange Box 360. **Grid**, **LevelInfo** and **static props gamelump v12** have different field layouts. **PakFile and PhysicsCollide are TODO** (unknown).
- Valve Developer Community pages "BSP (Source)" and "BSP (Source)/Game-Specific" cover the rBSP header. (VDC blocked automated fetches during this session. Read them manually.)

## 4. Models / animations

- PC TF1 uses studiomdl **v52** (a Respawn fork of Source's v48/49). `rmdlconv` (r-ex) converts between MDL versions. Its source documents struct differences.
- **RSX** (r-ex, **AGPL-3.0**) parses TF2 MDL and Apex RMDL. **Its docs don't mention TF1 support.**
- **Legion+** (r-ex, **GPL-3.0**, **archived June 2025**, replaced by RSX): RPak/MBNK for Apex/TF2. **Not relevant to TF1**, which has no RPaks (those started with TF2).
- **360 models: undocumented.** Expect byte-swapped structs, possibly different vertex compression, and VTX strip layouts tuned for Xenos.

## 5. Textures

- Source's 360 texture container is **VTFX** (VDC page "VTFX file format". An open-source `vtfx_reader` exists, rob5300. **Check its license**).
- Xenos GPU textures are usually **tiled** (a swizzled memory layout) and DXT blocks are **16-bit-word byte-swapped**. Untiling is implemented in open source in **Xenia** (BSD-3-Clause). We read it to understand the algorithm, then write our own. The algorithm is a well-known public hardware fact, so this is fine.
- Formats to expect: DXT1/3/5, DXN (BC5) normal maps, possibly CTX1 (a 360-only 2-channel format with no PC equivalent. We'd convert it to BC5).
- Mip tails on Xenos are packed oddly (small mips share a tile). This is a classic source of bugs.

## 6. Audio

- PC TF1 shipped uncompressed audio (Respawn: to save CPU on low-end PCs). **The 360 can't do that**, so expect **XMA2** in some bank/stream container **(unverified)**.
- XMA decoding: **FFmpeg** (`xma1`/`xma2` decoders, LGPL-2.1+). Only the native setup tool uses it, to transcode to Ogg/Opus in the cache. The runtime never needs XMA.
- The bank/event format (how sounds are grouped, randomised, attenuated) is **undocumented** for TF1 360. Miles Sound System was used in later Respawn games (MBNK in RSX). **TF1 360: unknown.**

## 7. Community projects and their licenses

| Project | What it is | License | Use for us |
|---|---|---|---|
| [SK8-ENGINE/skate-3-rust-engine](https://github.com/SK8-ENGINE/skate-3-rust-engine) | Rust/Bevy Skate 3 rewrite from a 360 ISO. Our template | GPL-3.0-only | Structure, setup flow, legal framing |
| [snake-biscuits/bsp_tool](https://github.com/snake-biscuits/bsp_tool) | BSP parser, many games incl. rBSP and a TF 360 stub | GPL-3.0 | **Primary BSP reference**. Compatible license (confirm "or later" wording) |
| [r-ex/rsx](https://github.com/r-ex/rsx) | Respawn asset extractor (TF2/Apex) | AGPL-3.0 | Reference for MDL family. Read, don't vendor |
| [r-ex/LegionPlus](https://github.com/r-ex/LegionPlus) | Older TF2/Apex extractor, archived 2025 | GPL-3.0 | Mostly irrelevant (RPak) |
| r-ex/rmdlconv | MDL version converter | check | v52 struct reference |
| [pg9182/tf2vpk](https://pkg.go.dev/github.com/pg9182/tf2vpk) | Respawn VPK read/write (Go) | MIT | VPK reference. Says r1 should work |
| [r1delta/r1delta](https://github.com/r1delta/r1delta) | TF1 PC dedicated-server/mod framework | GPL-3.0 + mixed (incl. Source SDK code) | **Behaviour reference only.** Needs the PC game. Contains Valve SDK-derived code. **Don't copy** Its `cfg/server/listenserver.cfg` and `cfg/autoexec_delta.cfg` gave us the multiplayer server movement values (captured from the real servers), which we confirmed live |
| Northstar (R2Northstar) | TF2 PC server/mod framework + docs | mixed (check per repo) | TF2 docs on VPK, VTF, maps. Useful background |
| Xenia | 360 emulator | BSD-3-Clause | Xenos texture tiling/format reference |
| xdvdfs | XDVDFS tool in Rust | check (likely MIT/Apache) | Possible dependency for disc reading |
| FFmpeg | XMA decoding | LGPL-2.1+ | Setup tool only, dynamically linked or external binary |
| Momentum Mod | Source movement game with a "parkour" mode tuned with TF2 speedrunners | check | Movement-feel reference, not TF1-accurate |

## 8. Movement numbers found so far (all unverified)

Found in secondary sources (a 2026 university student project's "Titanfall constants" file, plus press coverage of 2014 datamined weapon stats). **These are hypotheses to test, not ground truth.** They may also be TF2 values.

| Quantity | Value (Source units, u ≈ 1 inch) |
|---|---|
| Run speed | ~180.6 u/s |
| Sprint speed | ~269.85 u/s |
| Jump launch velocity | 380 u/s |
| Double-jump velocity | 340 u/s |
| Slide-jump velocity | 280 u/s |
| Max wall-run speed | 630 u/s |
| Max time on one wall | 1.75 s |
| Wall-run zero-gravity "grip" phase | 0.5 s |

See PLAN.md §6 for how we'll measure the real values.

## 9. Things nobody has publicly documented (we'll have to research them ourselves)

1. The 360 archive/packaging layout (top priority, blocks everything).
2. 360 BSP lumps beyond the header (PakFile, PhysicsCollide; lightmap format; whether Bluepoint stripped or rebaked anything).
3. 360 MDL/VTX/VVD variants.
4. 360 texture container details for TF1 (VTFX version, flags, CTX1 use).
5. 360 material format.
6. 360 audio banks/streams.
7. Collision data (PhysicsCollide is a TODO even in bsp_tool's 360 branch). **We may have to build collision from render meshes or another lump** (the PC version has collision-ish lumps such as CM_* grid/brush data **(check)**).
8. Precise movement physics (it lives in engine code, which we won't read).

## 10. Community research sweep (2026-10-04)

Done with an external research assistant. Claims marked *(reported)* still need checking against our own files before we rely on them.

- **XBAR `.bpk`:** cra0 said on XeNTaX (Oct 2014) that he'd "figured out most of" the header, and his closed-source Titanfall VPKTool 3.4 (2015) shipped "experimental 360 archive" support. **He never published details, the name hash, or file lists.** Our decoder in `pf-fmt-bpk` already goes further. Not used: closed-source binary.
- **Related, but a different format:** Bluepoint's MGS HD Collection (360) `.xbarc` is PSARC-like. aluigi's QuickBMS script for Quake Arena Arcade's `.bpk` (by Pi Studios, not Bluepoint) handles XMemCompress. Both are archived ZenHAX threads, and neither documents the hash.
- **bsp_tool** (GPL-3.0): `bsp_tool/branches/respawn/titanfall_x360.py` is now in `master`. The old branch is gone. Big-endian `PSBr`, version 29. It reuses `orange_box_x360` lump classes, and PakFile/PhysicsCollide are TODO. **Allowed to port with attribution** (decision log).
- **Textures:** Xenia `src/xenia/gpu/xenos.h` (BSD-3-Clause) has the Xenos format list (CTX1 = 60, DXN) and the tiling logic used as our reference. *(reported)* Source-engine 360 VTFX data is linear (untiled) with per-block DXT byte swapping, so verify this on our own VTFX entries in M3. `rob5300/vtfx_reader` (Rust) and the Noesis `inc_xbox360_untile.py` have **no license**: read for understanding only, never copy.
- **Audio:** vgmstream (permissive license, in COPYING) parses Wwise `.pck`/`.bnk` and decodes XMA1/XMA2. This is the likely route for `r1/wwise/_360/*.pck`.
- **Movement anchors:**
  - Wall-run lasts **1.75 s** (3.5 s with Enhanced Parkour Kit) at "about double sprint speed" (Titanfall Wiki, Fandom, method not stated).
  - A Respawn designer said traversal reaches "up to 30 mph, well over double a pilot's sprint speed" (Game Developer interview, 2014).
  - These are behaviour-level figures, not engine units.
- **Nobody has publicly documented:** the `.st1/.st2` streaming texture sets, `BPAN` and the other in-pack magics, 360 MDL/VVD/VTX internals, the XBAR name hash, or engine-unit TF1 movement values.
- **Lead for later:** the ResHax "XeNTaX backup index" and the archive.org `xentax.7z` attachment dump might hold cra0's unpublished 2014 notes.

## Sources

- https://github.com/SK8-ENGINE/skate-3-rust-engine
- https://github.com/snake-biscuits/bsp_tool and its `SUPPORTED_GAMES.md`, `bsp_tool/branches/respawn/titanfall_x360.py`
- https://github.com/snake-biscuits/bsp_tool/wiki/Etymology:-Lump-Names-&-Iteration-on-the-%60.bsp%60-Format
- https://developer.valvesoftware.com/wiki/BSP_(Source)/Game-Specific
- https://developer.valvesoftware.com/wiki/VTFX_file_format
- https://github.com/r-ex/rsx, https://github.com/r-ex/LegionPlus
- https://pkg.go.dev/github.com/pg9182/tf2vpk
- https://github.com/r1delta/r1delta
- https://r2northstar.readthedocs.io/en/latest/mapping/info-dump.html
- https://consolemods.org/wiki/Xbox_360:Game_Extraction (blocked automated fetch. Read manually)
- https://www.tomshardware.com/video-games/console-gaming/modern-blu-ray-drives-can-now-rip-gamecube-wii-and-xbox-360-games-to-pc-third-party-firmware-unlocks-game-rips-from-physical-media-on-select-players
- https://www.recalbox.com/blog/2026-08-16-omnidrive-wii-gamecube-xbox-360-retrogaming-preservation-emulation/
- https://pcgamer.com/titanfall-install-audio
- https://www.gamingnexus.com/News/31971/Bluepoint-Games-developing-Titanfall-for-the-Xbox-360
- https://cse125.ucsd.edu/2026/cse125g2/docs/TitanfallConstants_8hpp_source.html (student project. Low confidence)
- https://titanfall.wiki.gg/wiki/Slide_Hop
- https://learn.microsoft.com/en-us/previous-versions/windows/xna/bb976036(v=xnagamestudio.30) (tiled vs linear texture memory)
