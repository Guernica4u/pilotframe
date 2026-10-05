# Xbox 360 disc contents: overview (Milestone 0)

Our own notes from running `pf-inventory` on an owner-made dump (build string `R1X360_CL108433`, single XGD3 disc). Only facts about structure are recorded here. No game data.

## Disc layout

- XGD3 image. The game partition starts at `0x2080000`. 220 files / 13 dirs, ~6.0 GB of game data.
- `$SystemUpdate/`, root `PIRS` packages (`AvatarAssetPack`, `nxeart`): console system files. **Ignored.**
- `default.xex`, `*.lex` (XEX2 modules): executables. **Never read.**

## Game data (`r1/`)

| Path / type | Magic | Size | What it is | Documented? |
|---|---|---|---|---|
| `r1/bp_packdata/<map>.bpk` (18) | `XBAR` | 20–81 MB each | **Bluepoint per-map pack**: one per level + `engine.bpk` (shared) | **No. Our research (M1)** |
| `r1/materials/StreamingTextureSets/{main,ui}.st1/.st2` | none | 0.05–1.4 GB | **Bluepoint streaming texture sets** (two tiers/mip ranges?) | **No. Our research (M3)** |
| `r1/wwise/_360/*.pck` | `AKPK` | 5 KB–324 MB | **Audiokinetic Wwise** file packages (banks + streams, XMA expected) | **Yes.** Community docs/tools (vgmstream, wwiser) |
| `r1/scripts/game_wwise_manifest_<lang>.txt` | KeyValues text | ~2.4 MB | Wwise event/file manifest per language | Text, readable |
| `r1/media/*.bik` | `BIKi` | 1–163 MB | Bink videos (intro, menu, logos) | Bink 1. FFmpeg decodes it. Low priority |
| `r1/gameinfo.xtx` | KeyValues text | 1.5 KB | Source-style game info | Text |
| `r1/ea.bfp` | text-ish | 258 B | unknown, tiny | — |
| `r1/resource/*.ttf`, `platform/vgui/fonts/*.ttf` | TrueType | — | UI fonts (commercially licensed). **Never redistribute. Use our own fonts.** | n/a |

**There are no `.bsp`, `.vpk`, `.mdl` or `.vtf` files on the disc.** Everything the PC version keeps in VPKs is inside the Bluepoint `XBAR` packs and streaming texture sets. So PC tooling (VPK readers) doesn't apply directly. The question is whether the *contents* of the packs are still Source-family formats (rBSP lumps, MDL, VTF-like headers). bsp_tool's 360 branch suggests the BSP part is.

## Maps on the disc (pack size)

| Map pack | MB | | Map pack | MB |
|---|---|---|---|---|
| mp_lobby | 19.95 | | mp_relic | 68.54 |
| mp_npe (training) | 51.49 | | mp_corporate | 69.86 |
| **mp_colony** | **58.52** | | mp_o2 (Demeter) | 69.85 |
| mp_smugglers_cove | 60.86 | | mp_airbase | 72.85 |
| mp_rise | 61.94 | | mp_lagoon | 74.21 |
| mp_overlook | 62.74 | | mp_fracture | 75.65 |
| mp_training_ground | 63.12 | | mp_boneyard | 81.40 |
| mp_nexus | 67.45 | | engine (shared) | 33.43 |
| mp_angel_city | 68.06 | | | |
| mp_outpost_207 | 68.23 | | | |

## `XBAR` pack: first look (header only, big-endian)

From hex-viewing the start of `mp_lobby.bpk` with `pf-probe`. **All interpretations are guesses to verify in M1.**

- `0x00` magic `XBAR`. `0x04` u32 = 6 (**version?**).
- `0x08` u32 = 828. Matches the number of 28-byte records that follow (**entry count?**).
- `0x14` u32 = 1077. Every record's first field is below this (**string/name table count?**).
- `0x18` u32 = 0x8000 (**32 KB block size: suggests chunked compression**, likely Xbox LZX/XMemCompress).
- `0x1C` u32 ≈ the file size (**data size**). `0x24` u32 ≈ 256 KB (**header/TOC size?**).
- From `0x30`: **28-byte records** of 7 big-endian u32s: `[small index] [size?] [offset A] [offset B > A] [value sorted ascending] [random-looking] [0/1 flag]`. The ascending field strongly suggests a **name hash table sorted for binary search**. That means file names may not be stored in plain text. If so, we'll need a name list and to identify the hash function.

M1 tasks, in order: confirm the header fields across all 18 packs, test the compression hypothesis (entropy per block, LZX trial-decode with an open-source decoder such as the `lzxd` crate), find where names live (string table vs hashes only), then extract `mp_lobby`'s entries and look for `rBSP`/`IDST`/VTF-like magics inside.
