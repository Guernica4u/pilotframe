# MDL v52 skeleton and animation (Titanfall PC)

Titanfall's `.mdl` files are version 52, but the skeleton and animation parts keep the Source 2007-era layouts documented in the public Source SDK 2013 `studio.h`. Checked against the owner's `ptpov_rspn101.mdl` (R-201 first-person model) and `pov_mcor_pilot_male_br.mdl` (pilot arms). Reader: `crates/pf-fmt-mdl/src/anim.rs`.

## Header fields used

| Offset | Field |
|---|---|
| 156 / 160 | bone count / offset (216-byte `mstudiobone_t`) |
| 180 / 184 | animation count / offset (100-byte `mstudioanimdesc_t`) |
| 188 / 192 | sequence count / offset (212-byte `mstudioseqdesc_t`) |

## Bone (216 bytes)

name offset (relative to the bone) +0, parent +4, rest position +32, rest quaternion (x, y, z, w) +44, rest Euler angles +60, position scale +72, rotation scale +84, `poseToBone` 3×4 matrix (row-major) +96.

## Animation description (100 bytes)

name offset +4, fps +8, flags +12 (0x1 looping, 0x4 delta), frame count +16, external block +52 (always 0 here), data offset +56, section table offset +80, frames per section +84 (long animations are split into sections; each table entry is (block, data offset), 8 bytes).

## Sequence (212 bytes)

label offset +4, activity name offset +8, flags +12, blend count +56, animation index list offset +60 (i16 per blend).

## Per-bone animation stream

A chain of records: bone (u8), flags (u8), offset to the next record (i16, 0 = last), then the data. Rotation comes first, then position:

- rotation: raw 48-bit quaternion (flag 0x02), raw 64-bit quaternion (0x20), or a compressed Euler stream (0x08, three i16 offsets to run-length value streams, scaled by the bone's rotation scale);
- position: raw 3 half floats (0x01), or a compressed stream (0x04, scaled by the position scale).

Non-delta animations add the bone's rest Euler/position to compressed values; delta animations don't. Run-length streams are (valid count u8, total frames u8) headers followed by `valid` i16 values; frames past the valid ones repeat the last value.

## First-person rig (what the game data shows)

- 98 bones in the R-201 model: root `jx_c_delta`, camera `jx_c_camera` (at eye height under `jx_c_pov`), spine, both arms to the fingers, and the gun bone `ja_c_propGun`. In the camera bone's space x is left, y up, z forward.
- The arms model has the same bone names; it follows the weapon's animated bones by name.
- 54 sequences / 70 animations: aim in/out (full poses: their last frames are the aim and hip poses), idle and attack (deltas, each with a hip and an aim version: 2-way blends), sprint (looping full pose), reload (normal / empty, with "late" variants), draw, holster, raise, lower, melee. All 30 fps.
