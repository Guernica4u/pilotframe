//! Source-engine studio models as used by Titanfall PC: `.mdl` (v52) for the
//! structure and material names, `.vvd` (v4) for vertices, `.vtx` (v7) for
//! triangles. Only the static mesh of LOD 0 is read: no bones or animation yet.
//!
//! Layouts are from public documentation (Valve Developer Community wiki pages
//! "MDL", "VVD", "VTX" and the Source SDK `studio.h`/`optimize.h` struct
//! definitions as documented there), checked against the owner's own files.
//!
//! How the three files fit together, for each body part → model → mesh:
//! the VTX mesh's strip groups list vertices (`orig_mesh_vert_id`) and u16
//! triangle indices into that list. The VVD vertex is
//! `model.vertex_index / 48 + mesh.vertex_offset + orig_mesh_vert_id`.

use std::io;

const VVD_VERTEX_LEN: usize = 48;

/// One drawable mesh of a model, in model space (Source units, Z up).
#[derive(Debug, Clone, PartialEq)]
pub struct ModelMesh {
    /// Candidate material paths without extension, e.g.
    /// `models/colony/colony_panel_a`, best guess first (one per `$cdmaterials`).
    pub materials: Vec<String>,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub name: String,
    pub meshes: Vec<ModelMesh>,
}

/// Read a model from its three files.
pub fn load(mdl: &[u8], vvd: &[u8], vtx: &[u8]) -> io::Result<Model> {
    let m = Reader::new(mdl, "mdl");
    if m.bytes(0, 4)? != b"IDST" {
        return Err(invalid("not an MDL file"));
    }
    let version = m.i32(4)?;
    if !(44..=53).contains(&version) {
        return Err(invalid(&format!("unsupported MDL version {version}")));
    }
    let name = m.cstr(12, 64)?;

    // Material names: each texture name joined with each $cdmaterials folder.
    let (num_textures, texture_index) = (m.count(204)?, m.u32(208)? as usize);
    let (num_cd, cd_index) = (m.count(212)?, m.u32(216)? as usize);
    let mut cd_paths = Vec::new();
    for i in 0..num_cd {
        let at = m.u32(cd_index + i * 4)? as usize;
        cd_paths.push(clean_path(&m.cstr(at, 256)?));
    }
    if cd_paths.is_empty() {
        cd_paths.push(String::new());
    }
    let mut texture_names = Vec::new();
    for i in 0..num_textures {
        let tex = texture_index + i * 64; // mstudiotexture_t is 64 bytes
        let name_at = tex as i64 + m.i32(tex)? as i64;
        texture_names.push(clean_path(&m.cstr(name_at as usize, 256)?));
    }
    // Skin family 0 maps a mesh's material number to a texture.
    let (num_skinref, skin_index) = (m.count(220)?, m.u32(228)? as usize);
    let skin = |material: usize| -> usize {
        if material < num_skinref {
            m.u16(skin_index + material * 2)
                .map(|t| t as usize)
                .unwrap_or(material)
        } else {
            material
        }
    };

    let vertices = vvd_lod0(vvd)?;
    let x = Reader::new(vtx, "vtx");
    if x.i32(0)? != 7 {
        return Err(invalid("unsupported VTX version"));
    }
    let (vtx_parts, vtx_part_offset) = (x.count(28)?, x.u32(32)? as usize);

    let mut meshes = Vec::new();
    let (num_parts, part_index) = (m.count(232)?, m.u32(236)? as usize);
    if num_parts != vtx_parts {
        return Err(invalid("MDL and VTX disagree on body part count"));
    }
    for p in 0..num_parts {
        let part = part_index + p * 16; // mstudiobodyparts_t
        let (num_models, model_index) = (m.count(part + 4)?, m.i32(part + 12)? as i64);
        let vpart = vtx_part_offset + p * 8;
        let (vtx_models, vtx_model_off) = (x.count(vpart)?, x.u32(vpart + 4)? as usize);
        // Only the first model of each body part (the default "submodel").
        if num_models == 0 || vtx_models == 0 {
            continue;
        }
        let model = (part as i64 + model_index) as usize; // mstudiomodel_t, 148 bytes
        let model_vertex_base = m.i32(model + 84)? as usize / VVD_VERTEX_LEN;
        let (num_meshes, mesh_index) = (m.count(model + 72)?, m.i32(model + 76)? as i64);

        let vmodel = vpart + vtx_model_off;
        let lod0 = vmodel + x.u32(vmodel + 4)? as usize; // ModelLODHeader of LOD 0
        let (vtx_meshes, vtx_mesh_off) = (x.count(lod0)?, x.u32(lod0 + 4)? as usize);
        if vtx_meshes != num_meshes {
            return Err(invalid("MDL and VTX disagree on mesh count"));
        }

        for k in 0..num_meshes {
            let mesh = (model as i64 + mesh_index) as usize + k * 116; // mstudiomesh_t
            let material = m.i32(mesh)?.max(0) as usize;
            let mesh_vertex_base = model_vertex_base + m.i32(mesh + 12)?.max(0) as usize;
            let tex = texture_names
                .get(skin(material))
                .cloned()
                .unwrap_or_default();
            let materials = cd_paths.iter().map(|cd| format!("{cd}{tex}")).collect();

            let vmesh = lod0 + vtx_mesh_off + k * 9; // MeshHeader, 9 bytes packed
            let (groups, group_off) = (x.count(vmesh)?, x.u32(vmesh + 4)? as usize);
            let mut out = ModelMesh {
                materials,
                positions: Vec::new(),
                normals: Vec::new(),
                uvs: Vec::new(),
                indices: Vec::new(),
            };
            for g in 0..groups {
                let group = vmesh + group_off + g * STRIP_GROUP_LEN;
                let (num_verts, vert_off) = (x.count(group)?, x.u32(group + 4)? as usize);
                let (num_indices, index_off) = (x.count(group + 8)?, x.u32(group + 12)? as usize);
                let base = out.positions.len() as u32;
                for v in 0..num_verts {
                    // Vertex: 3 bone weight indices, bone count, orig_mesh_vert_id u16, 3 bone ids.
                    let id = x.u16(group + vert_off + v * 9 + 4)? as usize;
                    let vert = vertices
                        .get(mesh_vertex_base + id)
                        .ok_or_else(|| invalid("model vertex index out of range"))?;
                    out.positions.push(vert.0);
                    out.normals.push(vert.1);
                    out.uvs.push(vert.2);
                }
                for i in 0..num_indices {
                    let idx = x.u16(group + index_off + i * 2)? as u32;
                    if idx >= num_verts as u32 {
                        return Err(invalid("model triangle index out of range"));
                    }
                    out.indices.push(base + idx);
                }
            }
            if !out.indices.is_empty() {
                meshes.push(out);
            }
        }
    }
    Ok(Model { name, meshes })
}

/// StripGroupHeader size for VTX v7 as written by newer compilers (with the
/// topology fields): 33 bytes. Titanfall's `.dx11.vtx` files use this.
const STRIP_GROUP_LEN: usize = 33;

type Vertex = ([f32; 3], [f32; 3], [f32; 2]);

/// LOD 0 vertices from a `.vvd`, applying the fixup table if present.
fn vvd_lod0(vvd: &[u8]) -> io::Result<Vec<Vertex>> {
    let r = Reader::new(vvd, "vvd");
    if r.bytes(0, 4)? != b"IDSV" {
        return Err(invalid("not a VVD file"));
    }
    let lod0_count = r.count(16)?;
    let (num_fixups, fixup_start) = (r.count(48)?, r.u32(52)? as usize);
    let data_start = r.u32(56)? as usize;
    let read = |i: usize| -> io::Result<Vertex> {
        let at = data_start + i * VVD_VERTEX_LEN;
        Ok((
            [r.f32(at + 16)?, r.f32(at + 20)?, r.f32(at + 24)?],
            [r.f32(at + 28)?, r.f32(at + 32)?, r.f32(at + 36)?],
            [r.f32(at + 40)?, r.f32(at + 44)?],
        ))
    };
    if num_fixups == 0 {
        return (0..lod0_count).map(read).collect();
    }
    // Fixups: (lod i32, source vertex i32, count i32). Every fixup applies to LOD 0.
    let mut out = Vec::with_capacity(lod0_count);
    for f in 0..num_fixups {
        let at = fixup_start + f * 12;
        let (src, count) = (r.count(at + 4)?, r.count(at + 8)?);
        for i in src..src + count {
            out.push(read(i)?);
        }
    }
    Ok(out)
}

fn clean_path(s: &str) -> String {
    let mut p = s.replace('\\', "/").to_ascii_lowercase();
    if let Some(stripped) = p.strip_suffix(".vmt") {
        p = stripped.to_string();
    }
    p.trim_start_matches('/').to_string()
}

/// Bounds-checked little-endian reads.
struct Reader<'a> {
    data: &'a [u8],
    what: &'static str,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8], what: &'static str) -> Self {
        Self { data, what }
    }
    fn bytes(&self, at: usize, n: usize) -> io::Result<&'a [u8]> {
        self.data
            .get(
                at..at
                    .checked_add(n)
                    .ok_or_else(|| invalid("offset overflow"))?,
            )
            .ok_or_else(|| invalid(&format!("{} file is cut off at {at:#x}", self.what)))
    }
    fn u16(&self, at: usize) -> io::Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(at, 2)?.try_into().unwrap()))
    }
    fn u32(&self, at: usize) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(at, 4)?.try_into().unwrap()))
    }
    fn i32(&self, at: usize) -> io::Result<i32> {
        Ok(self.u32(at)? as i32)
    }
    fn f32(&self, at: usize) -> io::Result<f32> {
        Ok(f32::from_bits(self.u32(at)?))
    }
    /// A non-negative count, sanity-limited so garbage can't allocate gigabytes.
    fn count(&self, at: usize) -> io::Result<usize> {
        let n = self.i32(at)?;
        if !(0..=1_000_000).contains(&n) {
            return Err(invalid(&format!("bad count {n} in {} file", self.what)));
        }
        Ok(n as usize)
    }
    fn cstr(&self, at: usize, max: usize) -> io::Result<String> {
        let rest = self
            .data
            .get(at..)
            .ok_or_else(|| invalid("string offset out of range"))?;
        let rest = &rest[..rest.len().min(max)];
        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        Ok(String::from_utf8_lossy(&rest[..end]).into_owned())
    }
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}
