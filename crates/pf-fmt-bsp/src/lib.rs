//! Titanfall (PC) map files: `rBSP` version 29, little-endian.
//!
//! Struct layouts follow bsp_tool's `branches/respawn/titanfall.py` by
//! Jared "Snake-Biscuits" Ketterer (GPL-3.0, https://github.com/snake-biscuits/bsp_tool),
//! and were checked against the owner's own `mp_colony.bsp` (every mesh index
//! lands inside its vertex lump). Only the lumps needed to draw the world are here.
//!
//! - Header: `rBSP`, version u32 (29), map revision u32, 127 u32, then 128 lump
//!   headers of 16 bytes: offset, length, version, fourCC.
//! - Lumps can also live in `<map>.bsp.<id:04x>.bsp_lump` files next to the map.
//!   The PC VPKs ship both; this reader uses the copy inside the `.bsp`.
//!
//! How world geometry hangs together:
//! `Mesh` (0x50) → its `MaterialSort` (0x52) gives a vertex offset, a
//! `TextureData` (0x02, whose name is in the string table 0x2C/0x2B) and a
//! lightmap page (0x53). The mesh's triangles are `num_triangles * 3` u16
//! entries of `MESH_INDICES` (0x4F) from `first_mesh_index`; each index + the
//! sort's vertex offset picks a vertex in the lump chosen by the mesh flags.
//!
//! Lightmaps (checked on Colony): `LIGHTMAP_HEADERS` (0x53) is one 8-byte
//! record per page (u32 count, u16 width, u16 height). `LIGHTMAP_DATA_SKY`
//! (0x62) holds, per page, two RGBA8 layers of width x height. The first is
//! the baked colour with a brightness factor in alpha; the second looks like
//! bump/direction data. `LIGHTMAP_DATA_REAL_TIME_LIGHTS` (0x69) holds one RGBA8
//! layer per page; its red channel is the sun's shadow mask.

use std::io;

pub const MAGIC: &[u8; 4] = b"rBSP";
pub const VERSION: u32 = 29;
pub const LUMP_COUNT: usize = 128;
const HEADER_LEN: usize = 16 + LUMP_COUNT * 16;

pub mod lump {
    pub const ENTITIES: usize = 0x00;
    pub const TEXTURE_DATA: usize = 0x02;
    pub const VERTICES: usize = 0x03;
    pub const VERTEX_NORMALS: usize = 0x1E;
    pub const TEXTURE_DATA_STRING_DATA: usize = 0x2B;
    pub const TEXTURE_DATA_STRING_TABLE: usize = 0x2C;
    pub const GAME_LUMP: usize = 0x23;
    pub const WORLD_LIGHTS: usize = 0x36;
    pub const VERTEX_UNLIT: usize = 0x47;
    pub const VERTEX_LIT_FLAT: usize = 0x48;
    pub const VERTEX_LIT_BUMP: usize = 0x49;
    pub const VERTEX_UNLIT_TS: usize = 0x4A;
    pub const MESH_INDICES: usize = 0x4F;
    pub const MESHES: usize = 0x50;
    pub const MATERIAL_SORT: usize = 0x52;
    pub const LIGHTMAP_HEADERS: usize = 0x53;
    pub const LIGHTMAP_DATA_SKY: usize = 0x62;
    pub const LIGHTMAP_DATA_REAL_TIME_LIGHTS: usize = 0x69;
}

/// The vertex lump a mesh draws from, from mesh flags & 0x600.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VertexKind {
    LitFlat,
    LitBump,
    Unlit,
    UnlitTs,
}

impl VertexKind {
    fn from_flags(flags: u32) -> Self {
        match flags & 0x600 {
            0x000 => Self::LitFlat,
            0x200 => Self::LitBump,
            0x400 => Self::Unlit,
            _ => Self::UnlitTs,
        }
    }

    /// (lump id, bytes per vertex). LitBump and Unlit are confirmed on Colony;
    /// LitFlat and UnlitTs are from bsp_tool and not yet seen in our data.
    fn lump(self) -> (usize, usize) {
        match self {
            Self::LitFlat => (lump::VERTEX_LIT_FLAT, 36),
            Self::LitBump => (lump::VERTEX_LIT_BUMP, 44),
            Self::Unlit => (lump::VERTEX_UNLIT, 20),
            Self::UnlitTs => (lump::VERTEX_UNLIT_TS, 28),
        }
    }

    /// Lit vertices carry a lightmap UV (2 x f32) at byte 20.
    fn has_lightmap_uv(self) -> bool {
        matches!(self, Self::LitFlat | Self::LitBump)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LumpHeader {
    pub offset: u32,
    pub length: u32,
    pub version: u32,
    pub fourcc: u32,
}

pub struct Bsp<'a> {
    data: &'a [u8],
    pub revision: u32,
    pub lumps: [LumpHeader; LUMP_COUNT],
}

/// One triangle corner. Every vertex kind starts with position index u32,
/// normal index u32, texture UV 2 x f32, RGBA8 colour; lit kinds then have
/// the lightmap UV.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Corner {
    /// Index into [`Geometry::positions`].
    pub position: u32,
    pub normal: [f32; 3],
    /// In texture repeats (1.0 = one texture width), V pointing down the image.
    pub uv: [f32; 2],
    /// 0..1 within the mesh's lightmap page; zero for unlit vertices.
    pub lightmap_uv: [f32; 2],
    /// Blend materials use the alpha to mix their two textures.
    pub color: [u8; 4],
}

/// One drawable piece of the world.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldMesh {
    /// Index into [`Geometry::materials`].
    pub material: u32,
    pub kind: VertexKind,
    /// Index into [`Geometry::lightmaps`], if the mesh is lightmapped.
    pub lightmap: Option<u16>,
    pub triangles: Vec<[Corner; 3]>,
}

/// One lightmap page. Each layer is width x height RGBA8.
#[derive(Debug, Clone, PartialEq)]
pub struct LightmapPage {
    pub width: u32,
    pub height: u32,
    /// Baked light colour, brightness factor in alpha.
    pub sky: Vec<u8>,
    /// Second sky layer (bump/direction data, not used yet).
    pub sky_direction: Vec<u8>,
    /// Red channel = sun shadow mask (255 = fully lit by the sun).
    pub realtime: Vec<u8>,
}

/// The map's sun and sky light, from the world lights lump.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sun {
    /// Direction the light travels (Z up, pointing down for a sun overhead).
    pub direction: [f32; 3],
    /// Linear colour x intensity.
    pub color: [f32; 3],
    /// Sky ambient colour.
    pub ambient: [f32; 3],
}

/// One placed model (crate, tree, fence...).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StaticProp {
    pub origin: [f32; 3],
    /// Pitch, yaw, roll in degrees (Source convention).
    pub angles: [f32; 3],
    /// Index into the model name list.
    pub model: u16,
    pub scale: f32,
}

/// The map's static world geometry, in original units (Z up).
#[derive(Debug, Clone, PartialEq)]
pub struct Geometry {
    pub positions: Vec<[f32; 3]>,
    /// Material names, e.g. `world/dev/dev_white_512`.
    pub materials: Vec<String>,
    pub meshes: Vec<WorldMesh>,
    pub lightmaps: Vec<LightmapPage>,
    pub sun: Option<Sun>,
}

impl Geometry {
    pub fn triangle_count(&self) -> usize {
        self.meshes.iter().map(|m| m.triangles.len()).sum()
    }

    /// Axis-aligned bounds of every position, (min, max).
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for p in &self.positions {
            for a in 0..3 {
                min[a] = min[a].min(p[a]);
                max[a] = max[a].max(p[a]);
            }
        }
        (min, max)
    }
}

impl<'a> Bsp<'a> {
    pub fn parse(data: &'a [u8]) -> io::Result<Self> {
        let header = data
            .get(..HEADER_LEN)
            .ok_or_else(|| invalid("file is too short for an rBSP header".into()))?;
        if &header[0..4] != MAGIC {
            return Err(invalid("not an rBSP file".into()));
        }
        let version = u32_at(header, 4);
        if version != VERSION {
            return Err(invalid(format!(
                "rBSP version {version}, expected {VERSION} (Titanfall)"
            )));
        }
        let mut lumps = [LumpHeader {
            offset: 0,
            length: 0,
            version: 0,
            fourcc: 0,
        }; LUMP_COUNT];
        for (i, l) in lumps.iter_mut().enumerate() {
            let at = 16 + i * 16;
            *l = LumpHeader {
                offset: u32_at(header, at),
                length: u32_at(header, at + 4),
                version: u32_at(header, at + 8),
                fourcc: u32_at(header, at + 12),
            };
        }
        Ok(Self {
            data,
            revision: u32_at(header, 8),
            lumps,
        })
    }

    /// Raw bytes of one lump (empty if the map doesn't have it).
    pub fn lump(&self, id: usize) -> io::Result<&'a [u8]> {
        let h = self.lumps[id];
        let start = h.offset as usize;
        self.data
            .get(start..start + h.length as usize)
            .ok_or_else(|| invalid(format!("lump {id:#04x} runs past the end of the file")))
    }

    /// The entity lump as text (KeyValues-style blocks).
    pub fn entities(&self) -> io::Result<String> {
        let raw = self.lump(lump::ENTITIES)?;
        Ok(String::from_utf8_lossy(raw.strip_suffix(&[0]).unwrap_or(raw)).into_owned())
    }

    pub fn geometry(&self) -> io::Result<Geometry> {
        let positions: Vec<[f32; 3]> = records(self.lump(lump::VERTICES)?, 12, "VERTICES")?
            .map(|r| [f32_at(r, 0), f32_at(r, 4), f32_at(r, 8)])
            .collect();
        let normals: Vec<[f32; 3]> =
            records(self.lump(lump::VERTEX_NORMALS)?, 12, "VERTEX_NORMALS")?
                .map(|r| [f32_at(r, 0), f32_at(r, 4), f32_at(r, 8)])
                .collect();
        let materials = self.material_names()?;

        // TEXTURE_DATA: 36 bytes; the name index (u32 at +12) goes through the
        // string table.
        let tex_name: Vec<u32> = records(self.lump(lump::TEXTURE_DATA)?, 36, "TEXTURE_DATA")?
            .map(|r| u32_at(r, 12))
            .collect();
        // MATERIAL_SORT: texture_data u16, lightmap page u16 (0xFFFF = none),
        // cubemap u16, unknown u16, vertex_offset u32.
        let sorts: Vec<(u16, u16, u32)> =
            records(self.lump(lump::MATERIAL_SORT)?, 12, "MATERIAL_SORT")?
                .map(|r| (u16_at(r, 0), u16_at(r, 2), u32_at(r, 8)))
                .collect();
        let indices: Vec<u16> = records(self.lump(lump::MESH_INDICES)?, 2, "MESH_INDICES")?
            .map(|r| u16_at(r, 0))
            .collect();
        let lightmaps = self.lightmap_pages()?;

        let mut meshes = Vec::new();
        // MESHES: first_mesh_index u32, num_triangles u16, 8 x i16 unknown,
        // material_sort u16 (+22), flags u32 (+24).
        for (n, r) in records(self.lump(lump::MESHES)?, 28, "MESHES")?.enumerate() {
            let (first, tris) = (u32_at(r, 0) as usize, u16_at(r, 4) as usize);
            let (sort_id, flags) = (u16_at(r, 22) as usize, u32_at(r, 24));
            let bad = |what: &str| invalid(format!("mesh {n}: {what}"));
            let &(tex, lightmap, vertex_offset) = sorts
                .get(sort_id)
                .ok_or_else(|| bad("material sort out of range"))?;
            let kind = VertexKind::from_flags(flags);
            let (vlump, stride) = kind.lump();
            let verts = self.lump(vlump)?;

            let corner = |i: u16| -> io::Result<Corner> {
                let v = (vertex_offset as usize + i as usize) * stride;
                let rec = verts
                    .get(v..v + stride)
                    .ok_or_else(|| bad("vertex index out of range"))?;
                let position = u32_at(rec, 0);
                if position as usize >= positions.len() {
                    return Err(bad("position index out of range"));
                }
                let normal = normals
                    .get(u32_at(rec, 4) as usize)
                    .copied()
                    .unwrap_or([0.0, 0.0, 1.0]);
                let lightmap_uv = if kind.has_lightmap_uv() && stride >= 28 {
                    [f32_at(rec, 20), f32_at(rec, 24)]
                } else {
                    [0.0, 0.0]
                };
                Ok(Corner {
                    position,
                    normal,
                    uv: [f32_at(rec, 8), f32_at(rec, 12)],
                    lightmap_uv,
                    color: [rec[16], rec[17], rec[18], rec[19]],
                })
            };
            let tri_indices = indices
                .get(first..first + tris * 3)
                .ok_or_else(|| bad("mesh indices out of range"))?;
            let triangles = tri_indices
                .as_chunks::<3>()
                .0
                .iter()
                .map(|t| Ok([corner(t[0])?, corner(t[1])?, corner(t[2])?]))
                .collect::<io::Result<Vec<_>>>()?;

            let material = tex_name
                .get(tex as usize)
                .copied()
                .filter(|&m| (m as usize) < materials.len())
                .unwrap_or(u32::MAX);
            let lightmap = Some(lightmap).filter(|&l| (l as usize) < lightmaps.len());
            meshes.push(WorldMesh {
                material,
                kind,
                lightmap: lightmap.filter(|_| kind.has_lightmap_uv()),
                triangles,
            });
        }
        Ok(Geometry {
            positions,
            materials,
            meshes,
            lightmaps,
            sun: self.sun()?,
        })
    }

    /// Lightmap pages with their sky and real-time-light layers.
    pub fn lightmap_pages(&self) -> io::Result<Vec<LightmapPage>> {
        let sky = self.lump(lump::LIGHTMAP_DATA_SKY)?;
        let realtime = self.lump(lump::LIGHTMAP_DATA_REAL_TIME_LIGHTS)?;
        let (mut sky_at, mut rt_at) = (0usize, 0usize);
        let mut pages = Vec::new();
        for r in records(self.lump(lump::LIGHTMAP_HEADERS)?, 8, "LIGHTMAP_HEADERS")? {
            let (width, height) = (u16_at(r, 4) as u32, u16_at(r, 6) as u32);
            let layer = (width * height * 4) as usize;
            let take = |data: &[u8], at: usize, what: &str| -> io::Result<Vec<u8>> {
                data.get(at..at + layer)
                    .map(<[u8]>::to_vec)
                    .ok_or_else(|| invalid(format!("{what} lightmap data is cut off")))
            };
            pages.push(LightmapPage {
                width,
                height,
                sky: take(sky, sky_at, "sky")?,
                sky_direction: take(sky, sky_at + layer, "sky")?,
                realtime: take(realtime, rt_at, "real-time")?,
            });
            sky_at += layer * 2;
            rt_at += layer;
        }
        Ok(pages)
    }

    /// The sun (`emit_skylight`, type 3) and sky ambient (`emit_skyambient`,
    /// type 5) from `WORLD_LIGHTS`: 100-byte records with origin (3 x f32),
    /// intensity (3 x f32, +12), normal (3 x f32, +24) and type (i32, +52).
    pub fn sun(&self) -> io::Result<Option<Sun>> {
        let lights = self.lump(lump::WORLD_LIGHTS)?;
        if lights.is_empty() || !lights.len().is_multiple_of(100) {
            return Ok(None);
        }
        let vec3 = |r: &[u8], at: usize| [f32_at(r, at), f32_at(r, at + 4), f32_at(r, at + 8)];
        let (mut sun, mut ambient) = (None, [0.0; 3]);
        for r in lights.as_chunks::<100>().0 {
            match u32_at(r, 52) {
                3 => sun = Some((vec3(r, 24), vec3(r, 12))),
                5 => ambient = vec3(r, 12),
                _ => {}
            }
        }
        Ok(sun.map(|(direction, color)| Sun {
            direction,
            color,
            ambient,
        }))
    }

    /// Static props from the game lump (0x23). Its directory is a count then
    /// 16-byte entries (id `sprp` reversed, flags u16, version u16, absolute
    /// file offset i32, length i32). `sprp` v12 (checked on Colony): model
    /// name count + 128-byte names, leaf count (+ u16 leaves), prop count,
    /// two more u32s, then 84-byte props: origin (3 x f32), angles (pitch, yaw,
    /// roll in degrees, +12), model index u16 (+24), scale f32 (+72).
    pub fn static_props(&self) -> io::Result<(Vec<String>, Vec<StaticProp>)> {
        let lump = self.lump(lump::GAME_LUMP)?;
        if lump.len() < 4 {
            return Ok((Vec::new(), Vec::new()));
        }
        let count = u32_at(lump, 0) as usize;
        for i in 0..count.min(64) {
            let entry = lump
                .get(4 + i * 16..4 + i * 16 + 16)
                .ok_or_else(|| invalid("game lump directory is cut off".into()))?;
            if &entry[0..4] != b"prps" || u16_at(entry, 6) != 12 {
                continue;
            }
            let (offset, length) = (u32_at(entry, 8) as usize, u32_at(entry, 12) as usize);
            let s = self
                .data
                .get(offset..offset + length)
                .ok_or_else(|| invalid("static prop lump runs past the end of the file".into()))?;
            let cut = || invalid("static prop lump is cut off".into());
            let num_names = u32_at(s.get(..4).ok_or_else(cut)?, 0) as usize;
            let mut names = Vec::with_capacity(num_names);
            for n in 0..num_names {
                let raw = s.get(4 + n * 128..4 + (n + 1) * 128).ok_or_else(cut)?;
                let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
                names.push(String::from_utf8_lossy(&raw[..end]).replace('\\', "/"));
            }
            let mut at = 4 + num_names * 128;
            let leaves = u32_at(s.get(at..at + 4).ok_or_else(cut)?, 0) as usize;
            at += 4 + leaves * 2;
            let num_props = u32_at(s.get(at..at + 4).ok_or_else(cut)?, 0) as usize;
            at += 12;
            let data = s.get(at..at + num_props * 84).ok_or_else(cut)?;
            let props = data
                .as_chunks::<84>()
                .0
                .iter()
                .map(|r| StaticProp {
                    origin: [f32_at(r, 0), f32_at(r, 4), f32_at(r, 8)],
                    angles: [f32_at(r, 12), f32_at(r, 16), f32_at(r, 20)],
                    model: u16_at(r, 24),
                    scale: f32_at(r, 72),
                })
                .collect();
            return Ok((names, props));
        }
        Ok((Vec::new(), Vec::new()))
    }

    /// Material names from the texture string table.
    pub fn material_names(&self) -> io::Result<Vec<String>> {
        let data = self.lump(lump::TEXTURE_DATA_STRING_DATA)?;
        records(
            self.lump(lump::TEXTURE_DATA_STRING_TABLE)?,
            4,
            "TEXTURE_DATA_STRING_TABLE",
        )?
        .map(|r| {
            let start = u32_at(r, 0) as usize;
            let rest = data
                .get(start..)
                .ok_or_else(|| invalid("material name offset out of range".into()))?;
            let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
            Ok(String::from_utf8_lossy(&rest[..end]).replace('\\', "/"))
        })
        .collect()
    }
}

/// Split a lump into fixed-size records, failing if it isn't a whole number of them.
fn records<'a>(
    lump: &'a [u8],
    size: usize,
    name: &str,
) -> io::Result<impl Iterator<Item = &'a [u8]>> {
    if !lump.len().is_multiple_of(size) {
        return Err(invalid(format!(
            "{name} lump is {} bytes, not a multiple of {size}",
            lump.len()
        )));
    }
    Ok(lump.chunks_exact(size))
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_bits(u32_at(b, at))
}

fn invalid(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}
