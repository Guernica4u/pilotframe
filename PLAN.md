# PLAN: an unofficial Rust/Bevy reimplementation of Titanfall (2014)

> **Working codename: `Pilotframe`** (crate prefix `pf-`). This is a placeholder until you choose a name (see Q7).
>
> **Unofficial.** Not affiliated with, endorsed by, or connected to Electronic Arts or Respawn Entertainment. "Titanfall" is a trademark of Electronic Arts Inc. and is used only to describe compatibility. No game code, data, or assets are included or distributed.

Status: **Milestones 0 and 1 done. Milestone 2 (raw map geometry) next.** Format research and sources are in [docs/RESEARCH.md](docs/RESEARCH.md).

## Why this exists

**An open-source version of Titanfall that stays playable on PC forever, is fully moddable, and belongs to its community.** It doesn't depend on EA's executable, DRM, stores or servers, and doesn't need a pre-existing PC install (unlike projects that patch the original game). Every design choice should serve that:
- **Independent:** own engine, runs from the player's own discs (PC or 360).
- **Moddable:** gameplay values in data files, a documented cache/content format, mod folder support (mods override original assets), and community maps (M11).
- **Ours:** clean-room GPL code, so nobody can take it away.

## Decisions (2026-10-04)

| Topic | Decision |
|---|---|
| Asset source | **Both, from the owner's own discs** (in gitignored `Resources/`): the **Xbox 360 ISO**, plus the **PC retail discs (3 ISOs)**. The PC data is a plain, unencrypted split ZIP (538 files, about 50 GB, mostly Respawn VPKs), so no DRM is involved. We never touch the PC `.exe`/`.dll` or game scripts. **The PC (documented formats) is a reference for decoding the 360 (Bluepoint formats). Setup will accept either disc set, and PC gives HD assets.** (Decided 2026-10-05.) |
| Movement reference | **PC values**, read from both versions' settings files. If they match the 360 (likely), there's nothing to decide. (Changed 2026-10-05.) |
| **Main source** | **PC first** for everything (2026-10-05). The 360 stays supported at lower priority, so players with only the 360 disc can play later. |
| First launch | Like the Skate 3 project: a setup screen with a **Windows file picker**. The player picks the 360 ISO, PC Disc 1 ISO (discs 2–3 found next to it), or a PC install folder. The version is detected automatically. Converts once into a cache. |
| Cache location | A `data/` folder **next to the game exe**. |
| Menu | **Rebuild the original menu** at runtime from the player's own files (art and fonts are never in the repo). Plain fallback menu if the files are missing. |
| Graphics | **Match the original PC look** (baked lighting, original textures). Modern extras only as optional settings later, if ever. |
| Audio | Comes in **right after the first textured map (M3)**: map ambience, footsteps, menu music. |
| First map | **Colony** (PC files). |
| Public goal | Repo goes public when it **boots to the menu and you can pick a map and walk it with real pilot movement** (incl. wall-running). No enemies needed. |
| DLC | **Only DLC the owner downloaded themselves** (e.g. an old PC install or the Xbox 360's hard drive). Never from other people. The stores are closed. Setup will pick DLC up if it's present. Base game first. |
| (old) DLC | Base game only (15 maps + lobby + training). DLC maps aren't on any disc. They were downloads, and the stores are closed, so it's out of scope for now. |
| Reference line | (Relaxed 2026-10-06.) **Anything legal may be read as reference**, including the owner's own game scripts. **All shipped code is written by us in Rust.** Leaks, decompiling the executables, and DRM bypass stay banned. |
| Third-party code | Open-source tools (bsp_tool, RSX, Northstar, R1Delta…) are **reference only**: we learn layouts and behaviour from them and write our own Rust, crediting them in `docs/RESEARCH.md`. Skip any files that come from leaked code. |
| Measurement footage | No 360 console and no capture card, so we'll use **YouTube footage of the 360 version** that the owner picks, analysed frame by frame. Lower precision (360 ran ~30 fps at sub-720p), so we average over many clips. |
| Codename | `Pilotframe` (`pf-*`) until a real name is chosen. |
| Hosting | **Private GitHub repo**. Owner + one friend, each with their own Claude. **Stays private until it boots to a menu and loads a map** (no enemies needed). When it goes public, describe it as an "unofficial reimplementation" (not a "port" or "remake"). |
| Setup UX | CLI first, GUI file picker later. |
| Pace | Lots of time available, two people working in parallel. **Timeline is compressed** (see §5). |
| Toolchain | Rust 1.99 stable (MSVC), VS 2022 C++ Build Tools, gh CLI. **Bevy 0.19.1** (latest stable on 2026-10-04, first used by `pf-sandbox`), pinned workspace-wide in the root `Cargo.toml` together with leafwing-input-manager 0.21.0. |

---

## 0. Ground rules (the project's constitution)

These go into `CONTRIBUTING.md` and `LEGAL.md` word for word once the repo exists.

1. **Our code, in Rust.** Every line of project code is written by us. We never copy, paste, or line-by-line translate code from any source. Anything **legal** may be used as reference: the owner's own game files (data and scripts), open-source projects, public SDKs, docs, wikis, videos. (Relaxed 2026-10-06.)
2. **Never, even as reference:** leaked source (Valve/Respawn leaks, or files in other projects derived from them), leaked XDK docs, decompiling/disassembling the game executables (`.exe`, `.dll`, `.xex`), and DRM circumvention.
3. **Bring your own assets.** The repo, releases, CI artifacts, issues, wiki, and any website hold **zero** game-derived bytes: no textures, meshes, audio, maps, screenshots of converted assets in test fixtures, or hex dumps longer than a struct header. Only the player's local setup tool turns their own disc dump into a local cache next to the exe.
4. **The engine never assumes original assets exist.** Every asset lookup goes through a content layer that can be satisfied by the original-derived cache, a community pack, or built-in placeholders.
5. **Dependencies** (crates) must have GPL-3.0-compatible licenses. Other projects' code is read, never vendored.
6. **Branding:** name, logo and icon are the owners' call (relaxed 2026-10-05). README and title screen carry the "unofficial / not affiliated" notice.
7. **License:** `GPL-3.0-only`, plus this notice: *"This license covers only the code in this repository. It grants no rights to Electronic Arts' or Respawn Entertainment's code, data, assets, or trademarks."*

---

## 1. Getting your Xbox 360 disc onto your PC

We only need the **data files** on the game partition. On 360 discs those files are stored in plain form (only the XEX is encrypted, and we ignore it). The hard part is that **a normal PC DVD/Blu-ray drive refuses to read the game partition** of an Xbox disc. Every dumping method gets around that somehow.

### Options

| Method | Hardware | Output | Notes |
|---|---|---|---|
| **A. OmniDrive firmware on a PC Blu-ray drive** *(technically best)* | Specific MediaTek MT1959-based drives (e.g. LG WP50NB40, some Asus/Buffalo/Verbatim models). About $60–120 | Full ISO via `redumper` (Redump-verifiable) | Out of beta since Feb 2026. Redump's recommended method. You flash custom firmware onto a drive you own. |
| **B. Old Kreon-firmware DVD drive** (Lite-On/Samsung/Philips drives taken out of 360 consoles or sold modded) | Modded DVD drive + SATA | ISO via Xbox Backup Creator / DiscImageCreator | Older and more fiddly. XGD3 needs extra steps (some old firmware can't read XGD3 fully). |
| **C. Modded 360 console (RGH/JTAG)** copying the disc over FTP | A hacked 360 | Extracted folder or GOD | **Not recommended.** It requires defeating the console's own security, which is the same kind of thing you've already ruled out for the Xbox One. |
| **D. Someone else's dump / download** | — | — | **No.** Getting a copy from someone else is plain copyright infringement, whatever discs you own. Our docs will say so. |

### File formats our setup tool will accept (in order of preference)

1. **Redump-style `.iso`.** We read XDVDFS directly from the image and never extract it to disk.
2. **Extracted folder**: the XDVDFS tree as normal files (made with `xdvdfs` or `extract-xiso`).
3. **GOD container** (an STFS/SVOD folder from `iso2god`). Supported later, as a convenience.

### ⚠️ Legal gray areas, stated plainly (I'm not a lawyer)

- **Making a personal copy of a game you own** is a gray area under US copyright law in general. Personal-use backup isn't clearly protected for games.
- **The bigger issue is DMCA §1201.** Methods A and B work by making the drive ignore the Xbox disc's access protection, which normally tells non-Xbox drives not to read the game partition. One reading is that this is "circumventing a technological measure that controls access." The opposite argument is that the disc isn't encrypted and the firmware only removes the drive's own refusal. **This has never been tested in court. The Copyright Office's §1201 exemptions for video game preservation apply to libraries/archives, not to individuals.** OmniDrive's marketing calls it "legal". Treat that as their position, not settled law.
- Method C involves defeating the console's security, so it's more clearly §1201 territory.
- **Honest summary:** dumping your own 360 disc is the same *category* of question as the Xbox One disc, just milder: no decryption keys are involved, only a drive-level read lock. You've drawn a firm line at the Xbox One. **Where you draw it for the 360 is your decision (Q1).** The project itself only ever reads an image the user already has. It ships no dumping tool, no firmware, and no instructions for bypassing anything. Like the Skate 3 project, it links to neutral preservation docs (Redump) and doesn't host anything.
- Outside the US (EU and others), private-copy rules differ but anti-circumvention laws exist too.

---

## 2. Workspace layout

**Rust note:** a *workspace* is one repo containing many *crates* (Rust libraries/binaries) built together. Splitting by responsibility keeps compile times down (only changed crates rebuild), forces clean dependency direction, and lets us keep Bevy out of the parser crates. That makes them faster to test and WASM-friendly.

```
pilotframe/
├─ Cargo.toml                # [workspace], shared dependency versions, lints
├─ LICENSE                   # GPL-3.0-only text
├─ LEGAL.md                  # BYOA rules, trademarks, no-affiliation, dumping stance
├─ README.md / CONTRIBUTING.md / PLAN.md
├─ docs/
│  ├─ RESEARCH.md            # sources, tool licenses
│  ├─ formats/               # OUR OWN write-ups of each format (struct tables, no game bytes)
│  ├─ movement/              # measurement method, measured values with error bars
│  └─ decisions/             # short ADRs: "why Avian not Rapier", etc.
├─ crates/
│  │  ── Layer 0: no Bevy, no std::fs, WASM-safe ──────────────
│  ├─ pf-binread/            # endian-aware struct reading helpers (wraps `binrw`)
│  ├─ pf-vfs/                # `ReadAt` byte-source trait + read-only virtual filesystem trait
│  ├─ pf-xdvdfs/             # 360 ISO / extracted folder / (later) GOD+STFS → VFS
│  ├─ pf-fmt-archive/        # whatever the 360 package format turns out to be (VPK? .360 zip?)
│  ├─ pf-fmt-bsp/            # rBSP v29 (big + little endian), lumps → typed structs
│  ├─ pf-fmt-mdl/            # MDL/VVD/VTX (360 variant) + animations
│  ├─ pf-fmt-texture/        # VTFX/VTF header, Xenos untile, endian swap, CTX1/DXN decode
│  ├─ pf-fmt-material/       # VMT keyvalues (or compiled variant)
│  ├─ pf-fmt-audio/          # bank/stream container parsing (XMA payloads passed through)
│  ├─ pf-fmt-kv/             # Valve KeyValues text/binary (entities, settings, materials)
│  ├─ pf-cache/              # OUR cache format: spec, writer, reader, versioning
│  ├─ pf-content/            # logical asset IDs, content packs, fallbacks/placeholders
│  ├─ pf-sim/                # fixed-tick simulation core: math, collision queries trait
│  ├─ pf-movement/           # pilot movement, pure functions over (state, input, world) → state
│  │  ── Layer 1: native-only tooling ─────────────────────────
│  ├─ pf-convert/            # parsed formats → cache (meshes, KTX2 textures, Ogg audio)
│  ├─ pf-setup/              # binary: pick ISO/folder → validate → convert → write cache
│  │  ── Layer 2: Bevy plugins ────────────────────────────────
│  ├─ pf-assets/             # Bevy AssetLoaders for cache files (via AssetServer, not fs)
│  ├─ pf-world/              # map → ECS entities, collision world, spawn points
│  ├─ pf-render/             # map/prop/lightmap rendering, sky, scalable quality settings
│  ├─ pf-input/              # action mapping, KBM + gamepad, sensitivity/deadzones
│  ├─ pf-player/             # Bevy glue around pf-movement, camera, viewmodel
│  ├─ pf-titan/              # titan movement, embark/disembark, dash, core abilities
│  ├─ pf-ai/                 # grunts/spectres, navmesh, behaviour
│  ├─ pf-weapons/            # hitscan/projectile, ADS, recoil, ammo, data-driven defs
│  ├─ pf-gamemode/           # rules: attrition scoring, spawning, match flow, evac
│  ├─ pf-net/                # client/server, prediction, reconciliation (M10)
│  ├─ pf-audio/              # runtime audio (spatial, mixing)
│  ├─ pf-ui/                 # HUD, menus, settings
│  └─ pf-game/               # the actual binary: assembles plugins, CLI flags
├─ tools/
│  ├─ pf-inventory/          # M0: walk a dump, emit a manifest (paths, sizes, magic, hashes)
│  ├─ pf-probe/              # format exploration helpers: struct guessing, histograms
│  └─ pf-motion/             # movement measurement tooling (video frame stepping, plots)
└─ .github/workflows/        # fmt, clippy, tests (no game data), wasm32 check of Layer 0
```

Key decisions (with short explanations):

- **Layer 0 crates don't depend on Bevy and never call `std::fs`.** They take a `&[u8]` or a `ReadAt` trait object. That gives three wins: fast unit tests, reuse in the setup tool, and the same parsers compiling to WASM (§3).
- **`binrw`** for parsing: a Rust crate where you declare a struct plus attributes like `#[br(big)]` and it generates the reading code. It handles 360-vs-PC endianness with one flag. It's like C# `BinaryReader` plus attributes, but checked at compile time.
- **Physics/collision:** I recommend a **custom kinematic character controller** in `pf-sim` that uses `parry3d` (a collision-query library) for shape casts, instead of driving the player with a rigid-body engine. Source-style movement (wall-runs, wall-hangs) is easier to make faithful when you control every step. Rigid-body physics (ragdolls, debris) can come later from **Avian**, which is Bevy-native. Recorded as an ADR in docs/decisions.
- **Units and axes:** the simulation uses **original game units** (≈1 inch) internally, so values copy straight from measurements. The converter switches Source's Z-up to Bevy's Y-up once, when writing the cache.
- **Fixed-timestep simulation from day one** (Bevy's `FixedUpdate` schedule, e.g. 60 Hz to start. **Check what tick the original used**). Movement is a pure function of (state, input, world). That's what makes later multiplayer prediction and replays possible without a rewrite.
- **Bevy version:** pin the newest stable release when we start, and only upgrade between milestones. Bevy breaks APIs every ~3–4 months, and AI coding tools often produce code for older Bevy versions. Each upgrade gets its own small PR.
- **Input:** `leafwing-input-manager` (a popular Bevy crate that maps physical buttons to named *actions* like `Jump`, `Sprint`), so KBM and controller are equal from the start. Controller aim gets its own aim-assist-free response curves and deadzones.

---

## 3. Asset pipeline

```
 your disc dump (ISO / folder / GOD)
        │   pf-xdvdfs  (read-only VFS over the image)
        ▼
 pf-fmt-* parsers  ──►  pf-convert  ──►  ./cache/original/   (next to the exe, gitignored)
                                           ├─ manifest.toml   (cache version, source hash, tool version)
                                           ├─ maps/<map>.pfmap
                                           ├─ meshes/*.glb    (or a custom mesh format)
                                           ├─ textures/*.ktx2 (BC1/BC3/BC5/BC7, mips)
                                           ├─ audio/*.ogg
                                           └─ data/*.ron      (weapon/titan/movement defs extracted as data)
                                        ▲
 ./cache/community/<pack>/  ───────────┘  (same layout, made by people, no EA content)
 built-in placeholders (in the exe)  ────  greybox meshes, checker textures, beeps
```

- **The cache format is ours and documented**, so community packs can target it without ever touching EA data. Every cache file is versioned. Bumping the converter version invalidates the cache and setup reconverts.
- **Setup is a separate native binary** (`pf-setup`), at first a CLI with a simple GUI added later. It checks the dump against a list of known file hashes (shipped as hashes only, not data), converts, and writes `manifest.toml`. Conversion is restartable per asset.
- **`pf-content` resolves logical IDs**, e.g. `map:mp_xxx`, `mesh:titan/atlas`, `sfx:weapon/r101/fire`, by checking packs in priority order: user mods → community → original → built-in placeholder. **Missing assets warn, they never crash.** That's priority #6 built in from day 1: placeholder mode should simply work with no cache.
- **Gameplay values are data, not code.** Movement, weapon, and titan tuning live in `.ron` files (RON is a Rust-flavoured JSON-like format). Default values we've measured ourselves are committed to the repo (they're our own measurements, facts not assets). If original settings files turn out to be usable (Q3), the converter can produce overrides locally.

### No-disc mode (M11) fits naturally
A community pack = new maps (a custom map format or a Blender export pipeline into `.pfmap`), models, and sounds that use the same logical IDs. We'll ship a tiny **greybox test map** made by us from day one. It's also our CI smoke-test map.

### Web build (M12) fits naturally
- Layer 0 compiles to `wasm32-unknown-unknown`. CI runs `cargo check --target wasm32-unknown-unknown` on those crates from M1, so native-only code can't creep in.
- The runtime loads only through Bevy's `AssetServer` (which can fetch over HTTP on web), never through `std::fs`.
- Heavy work goes through Bevy task pools, which run single-threaded on WASM. No `std::thread::spawn` in core crates.
- **Legal constraint for web:** we can never host a converted cache. Possible approaches (decided later): (a) the browser converts the user's ISO locally using the File API plus the WASM parsers, then stores the result in IndexedDB/OPFS. Audio transcoding is the hard part, since FFmpeg-in-WASM is heavy. (b) The user picks a local cache folder that the desktop setup already made. (c) Community packs only, which can be hosted.

---

## 4. Choosing the first map

**Decided after the M0 inventory (see [docs/formats/overview.md](docs/formats/overview.md)):**
- **Parser smoke test: `mp_lobby`** (20 MB pack, the smallest by far).
- **First real map: `mp_colony`** (58.5 MB, the smallest real multiplayer map pack). It's compact and mostly built-up, with short sightlines, so missing sky/far-LOD matters less. Training (`mp_npe`, 51 MB) is smaller but is a scripted tutorial space, a worse test of normal map content.

Original criteria (still used if Colony turns out to use unusual data):

1. **Smallest BSP and fewest lump types in use**, especially few/no displacement-like data, few static props, simple skybox. Every unimplemented lump is something that can block us.
2. **Mostly enclosed**, so a missing 3D skybox or far-LOD doesn't make it unreadable.
3. **Few unique materials**, so fewer texture/material edge cases at once.

Plan:
- **First smoke test: `mp_lobby`** (if it exists as a BSP on the 360 disc). It's the tiny menu/lobby scene: minimal geometry, minimal entities. It proves the parser before we take on a real map.
- **First real map: probably the smallest enclosed multiplayer map by file size.** From memory **Colony** or **Rise** are likely candidates, compact and mostly built-up. **The M0 inventory script will rank every map** by BSP size, lump count, static-prop count, and unique material count. I'll report the numbers and we'll pick from data.

---

## 5. Milestones

Each milestone has a **definition of done** and is small enough to show something.

**Revised pace (2026-10-04):** two people with lots of time, both using Claude. The "Rough effort" column below was a cautious first guess. **Writing the code is fast. What takes time is reverse-engineering formats nobody has documented** (the Bluepoint `XBAR` packs and streaming texture sets), and that can't be sped up the same way. Realistic targets now:

| Milestone | Best case | If the formats fight back |
|---|---|---|
| **M0 dump + inventory** | ✅ **Done 2026-10-04** | — |
| M1 list pack contents | ✅ **Done 2026-10-04**: all 18 packs fully decode ([bpk.md](docs/formats/bpk.md)). Entry *names* are still unknown (hashed) | — |
| M2 raw map geometry | 1–3 days after M1 | 1–2 weeks |
| M3 textured fly-through | 1–2 weeks | 1–2 months (streaming texture sets) |
| M4 walking + collision | days | 1–2 weeks |
| M5 faithful movement | 2–4 weeks | ongoing tuning |
| M6–M9 titans → match vs bots | 2–4 months | 6+ months |

**Splitting work between two people:** the original-asset work (M1–M3) and the movement sandbox (M4–M5 on a homemade greybox map) don't depend on each other. One person per track, merging through PRs.

The original estimates, kept for reference:

| # | Milestone | Done when | Rough effort |
|---|---|---|---|
| **0** | **Dump + inventory** | You have an ISO. `pf-inventory` prints a manifest: every file path, size, extension, first 16 bytes (magic), SHA-256. We write `docs/formats/overview.md` describing what the 360 build contains. **No game data is committed. The manifest itself stays local** (file names are probably fine to discuss, but hashes and listings stay out of the repo by default). | 1–2 weeks (hardware-dependent) |
| **1** | **Archive/package format: list contents** | `pf-fmt-archive` lists and extracts every entry from the 360 packages. This decides how much of the PC tooling knowledge transfers. Tests pass against a local dump. CI passes with synthetic fixtures. | 2–6 weeks (more if it's a custom Bluepoint format) |
| **2** | **Raw map geometry** | One map's vertices/indices (meshes lump) appear in a Bevy window with a fly camera, flat-shaded, no textures. Endianness confirmed. | 2–4 weeks |
| **3** | **Fly through a full textured map** | Textures untiled and correct, materials mapped, lightmaps (or vertex lighting) applied, static props placed, sky. Runs smoothly on a 3060 Ti. Setup converts the map into the cache. The game loads **only** from the cache. | 1–3 months |
| **4** | **Walk the map with collision** | Capsule-shaped player, gravity, ground movement, steps/slopes, can't walk through walls. Collision source decided (collision lump vs render mesh). KBM + controller. | 3–6 weeks |
| **5** | **Faithful pilot movement** | Sprint, jump, double jump, wall-run, wall-hang, crouch, air control, mantle (TF1 has no slide). Each parameter is backed by a measurement in `docs/movement/` with an error bar. Side-by-side video comparison passes our tolerance (§6). | 2–4 months, then ongoing tuning |
| **6** | **One titan + embark/disembark** | Titan movement (walk/dash), camera, embark animation, disembark, titanfall drop-in. Uses original model + animations from the cache, or a greybox stand-in. | 2–3 months |
| **7** | **Grunts / AI** | Navmesh generated per map at setup/load time (we can't rely on the original's nav data being parseable), grunts spawn, path, take cover, shoot. | 2–4 months |
| **8** | **Weapons** | Data-driven weapon defs, hitscan + projectiles, ADS, recoil, damage, a few signature weapons, anti-titan weapon, smart pistol. | 2–4 months |
| **9** | **Match vs bots** | Offline Attrition: scoring, spawns, titan timers, bot pilots, epilogue/evac. | 2–4 months |
| **10** | **Multiplayer** | Dedicated/listen server, client prediction of movement (that's why movement is pure from M4), lag compensation for hitscan. Library chosen then (lightyear vs custom). | 4–8 months |
| **11** | **No-disc mode** | Community pack format documented. Greybox map + placeholder models playable end to end. Map-making path (e.g. Blender → `.pfmap`). | 2–3 months |
| **12** | **Web build** | Placeholder mode runs in a browser. Original-asset path per §3. | 2+ months |

**Total: realistically 2–4 years** part-time to reach M9/M10, with the early milestones dominated by format research.

---

## 6. Measuring the original movement

Movement code lives in the engine, inside the XEX we won't touch. So we treat the original as a **black box** and measure it, the way speedrunners and physicists would.

**Step 1 — collect hypotheses (week 1 of M5):** community-documented values (RESEARCH.md §8), the Titanfall wiki, the speedrun communities for TF1/TF2. **If Q3 allows**, numeric settings in the disc's data files (keyvalue settings like player class settings). Every value goes into `docs/movement/values.md` with source + confidence.

**Step 2 — record controlled clips:**
- Best source: the **Xbox One version running on your Xbox One/Series console** (higher, steadier frame rate than the 360) captured with a **60 fps capture card** (Elgato-class, ~$100–180). Recording yourself playing your own copy raises no legal issues. If you have a Series X/S, use it: smoother frames give better data. The 360 version is the fallback.
- Fixed test routines in a known spot (training/gauntlet if still accessible, see Q2): standing jump, sprint from rest, double jump at apex, wall-run along a long flat wall, wall-hang duration, fall from known heights.
- Reduce noise: fixed FOV setting, aim at a fixed marker, a controller with consistent inputs (or KBM on the 360? No. Controller).

**Step 3 — extract numbers (`tools/pf-motion`):**
- Frame-step video (60 fps → ~16.7 ms resolution, averaged over many trials).
- **Our biggest advantage:** we have the **real map geometry** from M3. We can render the same spot in our engine and match camera position to video frames (known wall lengths, door heights, prop sizes). That gives **position over time**, from which velocity and acceleration follow. Plain stopwatch measurements can't do that.
- Timings (wall-run max duration, double-jump window, wall-hang duration) come from frame counts on HUD/animation cues.
- Fit curves: e.g. jump height vs time → launch velocity + gravity. Sprint ramp-up → acceleration.

**Step 4 — compare:** a side-by-side tool plays our engine's replay of the same inputs next to the recorded clip. Tolerances per quantity (e.g. ±3% speeds, ±1 frame timings) are agreed before tuning.

**Honest ceiling:** we can probably get speeds, heights, and timings to within a few percent. Subtle things (exact air-strafe acceleration curves, wall-run attach angle rules, edge-case mantle behaviour) will be approximations that we tune until they *feel* right to people who played a lot. See §8.

---

## 7. Testing parsers without committing game data

- **Env var:** `PF_GAME_DUMP=/path/to/titanfall.iso` (or a folder). A small helper in a `pf-testdata` dev-crate:
  ```rust
  // returns None (and prints "skipped: PF_GAME_DUMP not set") when the env var is missing
  let Some(dump) = pf_testdata::dump() else { return; };
  ```
  Real-data tests live in `tests/real_data.rs` per crate. They run locally whenever the env var is set and skip in CI, where it never is. (Rust note: Rust's test harness has no built-in "skip". The convention is an early `return`, plus an `#[ignore]` variant for slow tests, run with `cargo test -- --ignored`.)
- **Asserted values are facts, not data:** counts, struct sizes, bounding boxes, "every lump parses with no leftover bytes", "round-trip re-serialise == original bytes". No snippets of asset bytes in test source.
- **Synthetic fixtures for CI:** test-only *writers* generate tiny valid files (a 2-triangle BSP, a 4×4 tiled texture) from our own understanding of the format. Parser → writer → parser round-trips run in CI.
- **Fuzzing:** `cargo-fuzz` on every parser. Malformed input must return an error, never panic. Parsers will eventually read user-supplied files, so this is a real robustness requirement.
- **Guardrails against accidental commits:**
  - `.gitignore`: `cache/`, `*.iso`, `dumps/`, `*.bsp`, `*.vpk`, `*.vtf*`, `*.mdl`, `*.xma`, `*.wav`, `local/`, inventory outputs.
  - CI job that fails on any file >1 MB, or with a blocked extension, or with a known game-file magic number.
  - Optional pre-commit hook doing the same.

---

## 8. Risks and unknowns (honest assessment)

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| **360 archive format is a custom Bluepoint format, undocumented** | Medium–high | Blocks M1+ | Entropy/magic analysis, compare file sizes with known PC structures, look for Source 360 conventions (`.360.` suffixes, VTFX, zip-based paks). Weeks of research may be needed. |
| **360 assets differ from PC** (lower-res textures, simplified meshes, rebaked lighting, stripped LODs) | Very likely | Visual quality is below the PC game | Accept 360 quality as the baseline (Q6). Community HD packs later through the same content system. |
| **Collision data unreadable** (PhysicsCollide is TODO even in bsp_tool's 360 branch) | Medium | Delays M4 | Fall back to collision built from render meshes plus a hand-tagged nonsolid list. |
| **Audio bank format unknown + XMA needs FFmpeg** | High | Delays sound, not walking | Audio isn't on the critical path until M8/M9. Placeholder sounds until then. |
| **Movement fidelity has a ceiling** without engine code | Certain | "Feels slightly off" to veterans | Measurement pipeline (§6), community feedback rounds, values in data files so tuning doesn't need code changes. |
| **Original game no longer playable for measurement** (servers, offline modes) | Unknown | Harder to measure | See Q2. Existing YouTube footage as a weaker fallback. |
| **Scope** (a full AAA multiplayer shooter) | Certain | Burnout | Milestones that are each fun on their own. M3 ("walk around the original maps") is a satisfying stopping point. |
| **Bevy churn + you're new to Rust** | High | Slow, frustrating upgrades | Pin versions, keep Bevy out of Layer 0 (most code doesn't care about upgrades), clippy + tests in CI. |
| **AI-generated code drifts toward "remembered" or copied code** | Low–medium | Code isn't ours | Rule in CONTRIBUTING: write fresh Rust from our docs and notes. Never paste leaked or decompiled code into a session. |
| **Legal: EA takedown** | Low–medium (EA has tolerated BYOA projects but isn't obliged to) | Project hosting lost | Strict BYOA, no game assets anywhere, clean-room code, a clear notice, mirror-ready repo. Branding is easy to swap if ever needed. |

---

## 9. What M0 will actually involve (once you've answered the questions)

1. You get the dump (method per Q1) as an ISO.
2. I write `pf-vfs` + `pf-xdvdfs` (or wrap the `xdvdfs` crate after a license check) + `tools/pf-inventory`.
3. You run `pf-inventory path/to/your.iso > local/inventory.json` and paste me a **summary**: extension histogram, top-level folders, magic numbers of the big files.
4. We write `docs/formats/overview.md` together, choose the first map from the data, and decide M1's target format.

---

## 10. Questions for you

Your answers change the plan. I've marked what each one affects.

**Q1 (legal line for the 360 dump, affects §1/M0).** Given the §1201 gray area for drive-firmware dumping: are you comfortable dumping your 360 disc with an **OmniDrive-flashed Blu-ray drive** (method A)? Do you already own a compatible drive, or a Kreon drive? If you'd rather not use any read-lock bypass, the project basically can't use original assets. We'd pivot to no-disc mode first (M11 becomes M1).

**Q2 (can you still play the original? affects §6).** Do you have an Xbox One or Series X|S, and can you still get into **training/the gauntlet or a match** on the 360 and/or Xbox One version (servers, offline modes)? Do you have a capture card, or are you willing to buy one (~$100–180)?

**Q3 (clean-room strictness for data/scripts, affects §6, §3, pf-convert).** The disc probably contains **settings files** (plain-text numbers like speeds and timings) and **Squirrel game scripts** (`.nut`). Which do you want?
- (a) Settings files OK as data. Scripts off-limits entirely.
- (b) Both readable to *understand behaviour*, no translation. Code is written from our own notes.
- (c) Neither. Measure everything from video only.
My recommendation is **(a)**. It boosts movement accuracy a lot and the risk stays low.

**Q4 (third-party code reuse, affects speed of M1–M3).** May we **port struct definitions from GPL-3.0 tools** like bsp_tool into Rust (license-compatible, with attribution)? Or should we only read their docs/notes and re-derive everything ourselves (cleaner provenance, slower)?

**Q5 (asset quality baseline, affects M3, priorities).** The 360 port likely has lower-res textures and simplified assets. Is "360 quality" acceptable as the target for original-asset mode, with HD only from community packs later?

**Q6 (movement reference, affects §6).** Should movement match the **Xbox One/PC tuning** (if they turn out identical) or specifically the **360 build** if they differ? I'd suggest "Xbox One/PC is canonical".

**Q7 (name + hosting, affects repo setup).** Do you have a project name in mind? It can't contain "Titanfall" or anything confusable. Will the repo be **public on GitHub from day one**, or private until M3? (Private-first lets us discuss file listings more freely while researching.)

**Q8 (pace, affects milestone sizing).** Roughly how many hours/week, and solo or open to contributors early? That decides whether I break milestones into ~1-evening tasks or ~1-week tasks.

**Q9 (setup UX, minor).** OK to start `pf-setup` as a **command-line tool** and add a GUI file picker (like the Skate 3 project's) around M3?
