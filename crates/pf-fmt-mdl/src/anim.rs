//! Skeleton and animations from an `.mdl`.
//!
//! Titanfall's v52 models keep Source's 2007-era layouts here: 216-byte bones,
//! 100-byte animation descriptions, 212-byte sequences, and Source's compressed
//! per-bone animation streams. The layouts and the meaning of each field are from
//! the public Source SDK 2013 `studio.h` documentation, checked against the
//! owner's files (`docs/formats/mdl-anim.md`); the code is our own.
//!
//! Space: everything is in the model's own space (Source units, Z up for most
//! models; the first-person rigs are authored Y up and rotated by their root).

use std::io;

use glam::{Affine3A, Mat3A, Quat, Vec3, Vec3A};

use crate::{Reader, invalid};

const BONE_LEN: usize = 216;
const ANIM_DESC_LEN: usize = 100;
const SEQ_LEN: usize = 212;

/// Animation-wide flags (`mstudioanimdesc_t::flags`).
pub const LOOPING: u32 = 0x1;
/// Added on top of another pose instead of replacing it.
pub const DELTA: u32 = 0x4;

// Per-bone stream flags (`mstudio_rle_anim_t::flags`).
const RAW_POS: u8 = 0x01;
const RAW_ROT: u8 = 0x02;
const ANIM_POS: u8 = 0x04;
const ANIM_ROT: u8 = 0x08;
const RAW_ROT2: u8 = 0x20;

#[derive(Debug, Clone, PartialEq)]
pub struct Bone {
    pub name: String,
    pub parent: Option<usize>,
    /// Rest pose, relative to the parent.
    pub pos: Vec3,
    pub quat: Quat,
    /// The rest rotation as Euler angles (radians), which compressed animations
    /// are stored relative to.
    pub rot: Vec3,
    /// Scales for the compressed position and rotation values.
    pub pos_scale: Vec3,
    pub rot_scale: Vec3,
    /// From model space (the mesh's bind pose) into this bone's space.
    pub pose_to_bone: Affine3A,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    pub name: String,
    pub fps: f32,
    pub frames: usize,
    pub flags: u32,
    /// Offset of this description in the file; data offsets are relative to it.
    at: usize,
    data: i32,
    section_index: i32,
    section_frames: usize,
}

impl Animation {
    pub fn is_delta(&self) -> bool {
        self.flags & DELTA != 0
    }
    pub fn is_looping(&self) -> bool {
        self.flags & LOOPING != 0
    }
    /// Length in seconds.
    pub fn duration(&self) -> f32 {
        (self.frames.saturating_sub(1)) as f32 / self.fps.max(1.0)
    }
}

/// A named sequence: what the game plays (`idle_seq` etc.). Blended sequences
/// list several animations; `animations[0]` is the first.
#[derive(Debug, Clone, PartialEq)]
pub struct Sequence {
    pub name: String,
    /// The game's activity name, e.g. `ACT_VM_IDLE`.
    pub activity: String,
    pub flags: u32,
    pub animations: Vec<usize>,
}

/// A pose: each bone's position and rotation relative to its parent.
pub type Pose = Vec<(Vec3, Quat)>;

pub fn skeleton(mdl: &[u8]) -> io::Result<Vec<Bone>> {
    let r = Reader::new(mdl, "mdl");
    let (count, start) = (r.count(156)?, r.u32(160)? as usize);
    (0..count)
        .map(|i| {
            let b = start + i * BONE_LEN;
            let v = |at: usize| -> io::Result<Vec3> {
                Ok(Vec3::new(
                    r.f32(b + at)?,
                    r.f32(b + at + 4)?,
                    r.f32(b + at + 8)?,
                ))
            };
            let parent = r.i32(b + 4)?;
            // matrix3x4_t: three rows of (x, y, z, translation).
            let m = |row: usize, col: usize| r.f32(b + 96 + row * 16 + col * 4);
            let pose_to_bone = Affine3A {
                matrix3: Mat3A::from_cols(
                    Vec3A::new(m(0, 0)?, m(1, 0)?, m(2, 0)?),
                    Vec3A::new(m(0, 1)?, m(1, 1)?, m(2, 1)?),
                    Vec3A::new(m(0, 2)?, m(1, 2)?, m(2, 2)?),
                ),
                translation: Vec3A::new(m(0, 3)?, m(1, 3)?, m(2, 3)?),
            };
            Ok(Bone {
                name: r.cstr((b as i64 + r.i32(b)? as i64) as usize, 128)?,
                parent: (parent >= 0 && (parent as usize) < count).then_some(parent as usize),
                pos: v(32)?,
                quat: Quat::from_xyzw(
                    r.f32(b + 44)?,
                    r.f32(b + 48)?,
                    r.f32(b + 52)?,
                    r.f32(b + 56)?,
                ),
                rot: v(60)?,
                pos_scale: v(72)?,
                rot_scale: v(84)?,
                pose_to_bone,
            })
        })
        .collect()
}

pub fn animations(mdl: &[u8]) -> io::Result<Vec<Animation>> {
    let r = Reader::new(mdl, "mdl");
    let (count, start) = (r.count(180)?, r.u32(184)? as usize);
    (0..count)
        .map(|i| {
            let a = start + i * ANIM_DESC_LEN;
            if r.i32(a + 52)? != 0 {
                return Err(invalid(
                    "animation stored in an external block (not supported)",
                ));
            }
            Ok(Animation {
                name: r.cstr((a as i64 + r.i32(a + 4)? as i64) as usize, 128)?,
                fps: r.f32(a + 8)?,
                flags: r.u32(a + 12)?,
                frames: r.count(a + 16)?,
                at: a,
                data: r.i32(a + 56)?,
                section_index: r.i32(a + 80)?,
                section_frames: r.count(a + 84)?,
            })
        })
        .collect()
}

pub fn sequences(mdl: &[u8]) -> io::Result<Vec<Sequence>> {
    let r = Reader::new(mdl, "mdl");
    let (count, start) = (r.count(188)?, r.u32(192)? as usize);
    (0..count)
        .map(|i| {
            let s = start + i * SEQ_LEN;
            let name_at = |off: usize| -> io::Result<String> {
                r.cstr((s as i64 + r.i32(s + off)? as i64) as usize, 128)
            };
            let blends = r.count(s + 56)?.min(64);
            let list = (s as i64 + r.i32(s + 60)? as i64) as usize;
            let animations = (0..blends)
                .map(|k| r.i16(list + k * 2).map(|v| v.max(0) as usize))
                .collect::<io::Result<_>>()?;
            Ok(Sequence {
                name: name_at(4)?,
                activity: name_at(8)?,
                flags: r.u32(s + 12)?,
                animations,
            })
        })
        .collect()
}

/// The rest pose (or, for delta animations, "no change").
pub fn rest_pose(bones: &[Bone]) -> Pose {
    bones.iter().map(|b| (b.pos, b.quat)).collect()
}

/// The pose at `time` seconds into `anim`, blending between the two nearest
/// frames. Looping animations wrap; others hold their last frame.
pub fn sample(mdl: &[u8], bones: &[Bone], anim: &Animation, time: f32) -> io::Result<Pose> {
    let last = anim.frames.saturating_sub(1) as f32;
    let mut frame = (time * anim.fps).max(0.0);
    frame = if anim.is_looping() && last > 0.0 {
        frame % last
    } else {
        frame.min(last)
    };
    let (f0, t) = (frame.floor(), frame.fract());
    let a = sample_frame(mdl, bones, anim, f0 as usize)?;
    if t < 1e-4 {
        return Ok(a);
    }
    let b = sample_frame(mdl, bones, anim, (f0 as usize + 1).min(last as usize))?;
    Ok(a.iter()
        .zip(&b)
        .map(|(&(pa, qa), &(pb, qb))| (pa.lerp(pb, t), qa.slerp(qb, t)))
        .collect())
}

/// Add a delta pose on top of `base`, scaled by `weight` (0..1).
pub fn add_delta(base: &mut Pose, delta: &Pose, weight: f32) {
    for ((pos, rot), &(dp, dq)) in base.iter_mut().zip(delta) {
        *pos += dp * weight;
        *rot = (*rot * Quat::IDENTITY.slerp(dq, weight)).normalize();
    }
}

/// Model-space transform of every bone for a pose.
pub fn world_transforms(bones: &[Bone], pose: &Pose) -> Vec<Affine3A> {
    let mut out: Vec<Affine3A> = Vec::with_capacity(bones.len());
    for (i, b) in bones.iter().enumerate() {
        let (p, q) = pose.get(i).copied().unwrap_or((b.pos, b.quat));
        let local = Affine3A::from_rotation_translation(q, p);
        // Parents always come before their children in Source models.
        let world = match b.parent {
            Some(parent) if parent < i => out[parent] * local,
            _ => local,
        };
        out.push(world);
    }
    out
}

/// The matrices that move bind-pose vertices to the posed skeleton.
pub fn skinning(bones: &[Bone], world: &[Affine3A]) -> Vec<Affine3A> {
    bones
        .iter()
        .zip(world)
        .map(|(b, w)| *w * b.pose_to_bone)
        .collect()
}

fn sample_frame(mdl: &[u8], bones: &[Bone], anim: &Animation, frame: usize) -> io::Result<Pose> {
    let r = Reader::new(mdl, "mdl");
    let delta = anim.is_delta();
    let mut pose: Pose = if delta {
        vec![(Vec3::ZERO, Quat::IDENTITY); bones.len()]
    } else {
        rest_pose(bones)
    };

    // Long animations are cut into sections of `section_frames` frames.
    let (mut at, mut frame) = (anim.at as i64 + anim.data as i64, frame);
    if let Some(section) = frame.checked_div(anim.section_frames) {
        let entry = (anim.at as i64 + anim.section_index as i64) as usize + section * 8;
        at = anim.at as i64 + r.i32(entry + 4)? as i64;
        frame -= section * anim.section_frames;
    }
    if anim.data == 0 && anim.section_frames == 0 {
        return Ok(pose);
    }

    let mut p = at as usize;
    for _ in 0..bones.len().max(1) * 2 {
        let bone = r.bytes(p, 1)?[0] as usize;
        let flags = r.bytes(p + 1, 1)?[0];
        let next = r.i16(p + 2)?;
        let data = p + 4;
        if let Some(b) = bones.get(bone) {
            let rot_size = if flags & RAW_ROT != 0 {
                6
            } else if flags & RAW_ROT2 != 0 {
                8
            } else if flags & ANIM_ROT != 0 {
                6
            } else {
                0
            };
            let pos_at = data + rot_size;

            if flags & RAW_ROT != 0 {
                pose[bone].1 = quat48(r.bytes(data, 6)?);
            } else if flags & RAW_ROT2 != 0 {
                pose[bone].1 = quat64(r.bytes(data, 8)?);
            } else if flags & ANIM_ROT != 0 {
                let mut e = Vec3::ZERO;
                for axis in 0..3 {
                    e[axis] = anim_value(&r, data, axis, frame, b.rot_scale[axis])?;
                }
                if !delta {
                    e += b.rot;
                }
                pose[bone].1 = euler_quat(e);
            }

            if flags & RAW_POS != 0 {
                let v = r.bytes(pos_at, 6)?;
                pose[bone].0 = Vec3::new(half(v, 0), half(v, 2), half(v, 4));
            } else if flags & ANIM_POS != 0 {
                let mut v = Vec3::ZERO;
                for axis in 0..3 {
                    v[axis] = anim_value(&r, pos_at, axis, frame, b.pos_scale[axis])?;
                }
                if !delta {
                    v += b.pos;
                }
                pose[bone].0 = v;
            }
        }
        if next == 0 {
            break;
        }
        p = (p as i64 + next as i64) as usize;
    }
    Ok(pose)
}

/// One channel of a compressed (run-length) value stream at `frame`.
/// `ptr` is an `mstudioanim_valueptr_t`: three i16 offsets, relative to itself.
fn anim_value(r: &Reader, ptr: usize, axis: usize, frame: usize, scale: f32) -> io::Result<f32> {
    let offset = r.i16(ptr + axis * 2)?;
    if offset == 0 {
        return Ok(0.0);
    }
    // Runs of (valid count, total frames) headers, each followed by `valid` values;
    // frames past the valid ones repeat the last value.
    let mut at = (ptr as i64 + offset as i64) as usize;
    let mut k = frame;
    loop {
        let valid = r.bytes(at, 1)?[0] as usize;
        let total = r.bytes(at + 1, 1)?[0] as usize;
        if total == 0 {
            return Ok(0.0);
        }
        if total > k {
            let index = if valid > k { k + 1 } else { valid };
            return Ok(r.i16(at + index * 2)? as f32 * scale);
        }
        k -= total;
        at += (valid + 1) * 2;
    }
}

/// Source's `AngleQuaternion` for radian Euler angles (x roll, y pitch, z yaw).
fn euler_quat(e: Vec3) -> Quat {
    let (sr, cr) = (e.x * 0.5).sin_cos();
    let (sp, cp) = (e.y * 0.5).sin_cos();
    let (sy, cy) = (e.z * 0.5).sin_cos();
    Quat::from_xyzw(
        sr * cp * cy - cr * sp * sy,
        cr * sp * cy + sr * cp * sy,
        cr * cp * sy - sr * sp * cy,
        cr * cp * cy + sr * sp * sy,
    )
}

/// 48-bit quaternion: x and y as 16 bits, z as 15 bits, then the sign of w.
fn quat48(b: &[u8]) -> Quat {
    let x = u16::from_le_bytes([b[0], b[1]]) as f32;
    let y = u16::from_le_bytes([b[2], b[3]]) as f32;
    let zw = u16::from_le_bytes([b[4], b[5]]);
    let z = (zw & 0x7fff) as f32;
    let (x, y, z) = (
        (x - 32768.0) / 32768.0,
        (y - 32768.0) / 32768.0,
        (z - 16384.0) / 16384.0,
    );
    let w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
    Quat::from_xyzw(x, y, z, if zw & 0x8000 != 0 { -w } else { w })
}

/// 64-bit quaternion: x, y, z as 21 bits each, then the sign of w.
fn quat64(b: &[u8]) -> Quat {
    let v = u64::from_le_bytes(b.try_into().unwrap_or([0; 8]));
    let c = |shift: u32| ((v >> shift) & 0x1f_ffff) as f32;
    let (x, y, z) = (
        (c(0) - 1_048_576.0) / 1_048_576.5,
        (c(21) - 1_048_576.0) / 1_048_576.5,
        (c(42) - 1_048_576.0) / 1_048_576.5,
    );
    let w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
    Quat::from_xyzw(x, y, z, if v >> 63 != 0 { -w } else { w })
}

/// IEEE half float at `at`.
fn half(b: &[u8], at: usize) -> f32 {
    let h = u16::from_le_bytes([b[at], b[at + 1]]);
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = ((h >> 10) & 0x1f) as i32;
    let man = (h & 0x3ff) as f32;
    sign * match exp {
        0 => man / 1024.0 * 2f32.powi(-14),
        31 => f32::INFINITY,
        _ => (1.0 + man / 1024.0) * 2f32.powi(exp - 15),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_floats() {
        assert_eq!(half(&0x3c00u16.to_le_bytes(), 0), 1.0);
        assert_eq!(half(&0xc000u16.to_le_bytes(), 0), -2.0);
        assert_eq!(half(&0u16.to_le_bytes(), 0), 0.0);
    }

    #[test]
    fn packed_quaternions_unpack_to_identity() {
        // x = y = z = 0 (biased), w positive.
        let q = quat48(&[0x00, 0x80, 0x00, 0x80, 0x00, 0x40]);
        assert!(q.abs_diff_eq(Quat::IDENTITY, 1e-4), "{q}");
        let v: u64 = 1_048_576 | (1_048_576 << 21) | (1_048_576 << 42);
        let q = quat64(&v.to_le_bytes());
        assert!(q.abs_diff_eq(Quat::IDENTITY, 1e-4), "{q}");
    }

    #[test]
    fn euler_matches_axis_rotation() {
        let q = euler_quat(Vec3::new(0.0, 0.0, 1.0));
        assert!(q.abs_diff_eq(Quat::from_rotation_z(1.0), 1e-5));
    }

    #[test]
    fn run_length_values() {
        // One run: 2 valid values (10, 20) covering 4 frames; frames 2 and 3 repeat 20.
        let mut d = vec![6u8, 0, 0, 0, 0, 0]; // value pointer: x at +6, y/z none
        d.extend([2u8, 4]);
        d.extend(10i16.to_le_bytes());
        d.extend(20i16.to_le_bytes());
        let r = Reader::new(&d, "test");
        let at = |f| anim_value(&r, 0, 0, f, 0.5).unwrap();
        assert_eq!((at(0), at(1), at(2), at(3)), (5.0, 10.0, 10.0, 10.0));
        assert_eq!(anim_value(&r, 0, 1, 0, 1.0).unwrap(), 0.0);
    }
}
