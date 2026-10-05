# `XBAR` packs (`r1/bp_packdata/*.bpk`)

Bluepoint's container for each map (plus a shared `engine.bpk`) in the Xbox 360 build. Worked out on 2026-10-04 from the owner's own dump by inspecting headers and testing hypotheses. Implemented in [`crates/pf-fmt-bpk`](../../crates/pf-fmt-bpk/src/lib.rs). **Status: fully decodes all 18 packs on the disc. Zero failures.**

All integers are **big-endian**.

## Header (0x30 bytes)

| Offset | Type | Meaning | Confidence |
|---|---|---|---|
| 0x00 | char[4] | `XBAR` | certain |
| 0x04 | u32 | version, always 6 | high |
| 0x08 | u32 | entry count | certain |
| 0x0C | u32 | unknown (0x1E or 0x20) | — |
| 0x10 | u32 | LZX window size (0x20000 = 128 KB) | certain (decoding works) |
| 0x14 | u32 | id space: every entry id is below it | certain |
| 0x18 | u32 | data offset (end of the header area) | certain: `0x18 + 0x1C == file size` in all 18 packs |
| 0x1C | u32 | data size | certain |
| 0x20 | u32 | unknown, always 0 | — |
| 0x24 | u32 | unknown. The same value in most packs, different in `engine.bpk` | — |
| 0x28 | u32 | unknown, hash-like. Same pattern as 0x24 | — |
| 0x2C | u32 | unknown, always 0 | — |

## Entry table (from 0x30, `count` x 28 bytes)

`[id] [unpacked size] [start] [end] [hash hi] [hash lo] [flags]`. Each is a u32.

- The table is **sorted by the 64-bit hash** (hi:lo), which suggests the game looks entries up by binary search on a hashed name. The final record breaks the sort order in at least one pack (cause unknown).
- `start`/`end` are absolute byte offsets. Entries are packed back to back with no gaps, in increasing `id` order. `id` is sparse (not 0..count).
- `end - start == size` means the entry is **raw**. Otherwise it's **compressed**.
- `flags`: mostly 0, sometimes 1 or 3. One odd value per pack. Meaning unknown.
- **Names aren't stored in the table.** The common 32/64-bit hashes (FNV-1/1a, CRC32, CRC64, djb2, sdbm, MD5/SHA1 prefixes) didn't match obvious paths like `maps/mp_lobby.bsp`. Recovering names is open research. It isn't needed to identify content, because magics and BSP structure do that.
- The area between the table and `data offset` holds further u32 arrays (not yet understood). Maybe a name-lookup structure.

## Compression

XMemCompress-style **LZX** (decoded with the `lzxd` crate, MIT/Apache-2.0):

1. The stored data is a series of chunks. Each chunk is preceded by either
   - `0xFF, u16 frame size, u16 chunk size` (a frame that isn't a full 32 KB, or sometimes a full one), or
   - `u16 chunk size` (a full 32 KB frame).
2. Every 128 KB (one window) of output, the stream **ends and a new independent LZX stream begins**. The boundary is marked by **five zero bytes**.
3. If the final partial window wouldn't compress, it follows the last separator **raw** (stored bytes left == output bytes left).

## What's inside (lobby pack)

Content is identified by leading bytes. The Source-family formats are **byte-swapped**, as expected for a big-endian console:

| Magic | Count (lobby) | Likely meaning |
|---|---|---|
| `PSBr` | 1 | rBSP header, `rBSP` byte-swapped. **Exactly 2,064 bytes = 16-byte header + 128 x 16-byte lump records.** The lump data lives in **other entries** of the pack |
| `TSDI` | 18 | Studio models (`IDST` swapped) |
| `VTFX` | 13 | Source's Xbox 360 texture container |
| `BPAN` | 88 | Bluepoint animation data? (biggest category by size) |
| `ENTI` | 5 | Entity data (12-byte stubs in the lobby) |
| `MODL`, `SMAP`, `AS01`, `ST12`, `STP1`, `PROJ`, `TOOL` | few | Unknown Bluepoint/engine formats |
| text | many | KeyValues settings/materials, configs. **Game scripts (`.nut`-like) are also present and are off-limits under the clean-room rule. Tools must not display them.** |

## Next (Milestone 2)

Parse the 2,064-byte `PSBr` header (version, 128 lump offsets/sizes). Then work out how lump records point to other pack entries (by id? by offset into a combined stream?) and pull out the vertex/index/mesh lumps for `mp_lobby`.
