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
//! `Mesh` (0x50) → its `MaterialSort` (0x52) gives a vertex offset and a
//! `TextureData` (0x02) → its name is in the string table (0x2C/0x2B).
//! The mesh's triangles are `num_triangles * 3` u16 entries of `MESH_INDICES`
//! (0x4F) from `first_mesh_index`; each index + the sort's vertex offset picks a
//! vertex in the lump chosen by the mesh flags; that vertex's first u32 indexes
//! `VERTICES` (0x03, f32 x3 positions).

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
    pub const VERTEX_UNLIT: usize = 0x47;
    pub const VERTEX_LIT_FLAT: usize = 0x48;
    pub const VERTEX_LIT_BUMP: usize = 0x49;
    pub const VERTEX_UNLIT_TS: usize = 0x4A;
    pub const MESH_INDICES: usize = 0x4F;
    pub const MESHES: usize = 0x50;
    pub const MATERIAL_SORT: usize = 0x52;
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

/// One drawable piece of the world.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldMesh {
    /// Index into [`Geometry::materials`].
    pub material: u32,
    pub kind: VertexKind,
    /// Triangles as indices into [`Geometry::positions`].
    pub triangles: Vec<[u32; 3]>,
}

/// The map's static world geometry, in original units (Z up).
#[derive(Debug, Clone, PartialEq)]
pub struct Geometry {
    pub positions: Vec<[f32; 3]>,
    /// Material names, e.g. `world/dev/dev_white_512`.
    pub materials: Vec<String>,
    pub meshes: Vec<WorldMesh>,
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
        let materials = self.material_names()?;

        // TEXTURE_DATA: 36 bytes; we only need it to map sorts to names, and the
        // texture data index equals the string table index in practice, so the
        // name index (u32 at +12) is followed through the table.
        let tex_name: Vec<u32> = records(self.lump(lump::TEXTURE_DATA)?, 36, "TEXTURE_DATA")?
            .map(|r| u32_at(r, 12))
            .collect();
        // MATERIAL_SORT: texture_data u16, lightmap u16, cubemap u16, unknown u16, vertex_offset u32.
        let sorts: Vec<(u16, u32)> = records(self.lump(lump::MATERIAL_SORT)?, 12, "MATERIAL_SORT")?
            .map(|r| (u16_at(r, 0), u32_at(r, 8)))
            .collect();
        let indices: Vec<u16> = records(self.lump(lump::MESH_INDICES)?, 2, "MESH_INDICES")?
            .map(|r| u16_at(r, 0))
            .collect();

        let mut meshes = Vec::new();
        // MESHES: first_mesh_index u32, num_triangles u16, 8 x i16 unknown,
        // material_sort u16 (+22), flags u32 (+24).
        for (n, r) in records(self.lump(lump::MESHES)?, 28, "MESHES")?.enumerate() {
            let (first, tris) = (u32_at(r, 0) as usize, u16_at(r, 4) as usize);
            let (sort_id, flags) = (u16_at(r, 22) as usize, u32_at(r, 24));
            let bad = |what: &str| invalid(format!("mesh {n}: {what}"));
            let &(tex, vertex_offset) = sorts
                .get(sort_id)
                .ok_or_else(|| bad("material sort out of range"))?;
            let kind = VertexKind::from_flags(flags);
            let (vlump, stride) = kind.lump();
            let verts = self.lump(vlump)?;

            let position_of = |i: u16| -> io::Result<u32> {
                let v = (vertex_offset as usize + i as usize) * stride;
                let rec = verts
                    .get(v..v + 4)
                    .ok_or_else(|| bad("vertex index out of range"))?;
                let p = u32_at(rec, 0);
                if p as usize >= positions.len() {
                    return Err(bad("position index out of range"));
                }
                Ok(p)
            };
            let tri_indices = indices
                .get(first..first + tris * 3)
                .ok_or_else(|| bad("mesh indices out of range"))?;
            let triangles = tri_indices
                .as_chunks::<3>()
                .0
                .iter()
                .map(|t| Ok([position_of(t[0])?, position_of(t[1])?, position_of(t[2])?]))
                .collect::<io::Result<Vec<_>>>()?;

            let material = tex_name
                .get(tex as usize)
                .copied()
                .filter(|&m| (m as usize) < materials.len())
                .unwrap_or(u32::MAX);
            meshes.push(WorldMesh {
                material,
                kind,
                triangles,
            });
        }
        Ok(Geometry {
            positions,
            materials,
            meshes,
        })
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
