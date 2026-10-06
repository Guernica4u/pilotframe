//! A hand-built rBSP with one triangle, wired up the same way as a real map:
//! mesh -> material sort -> unlit vertices -> positions, plus one material name.

use pf_fmt_bsp::{Bsp, VertexKind, lump};

fn build() -> Vec<u8> {
    let mut lumps: Vec<(usize, Vec<u8>)> = Vec::new();

    let mut positions = Vec::new();
    for p in [[0f32, 0., 0.], [64., 0., 0.], [0., 64., 0.]] {
        for v in p {
            positions.extend_from_slice(&v.to_le_bytes());
        }
    }
    lumps.push((lump::VERTICES, positions));

    // VertexUnlit: position index, normal index, uv (2 x f32), colour.
    let mut unlit = Vec::new();
    for i in 0..3u32 {
        unlit.extend_from_slice(&i.to_le_bytes());
        unlit.extend_from_slice(&[0u8; 16]);
    }
    lumps.push((lump::VERTEX_UNLIT, unlit));
    lumps.push((
        lump::MESH_INDICES,
        [0u16, 1, 2].iter().flat_map(|i| i.to_le_bytes()).collect(),
    ));

    let mut mesh = vec![0u8; 28];
    mesh[4..6].copy_from_slice(&1u16.to_le_bytes()); // one triangle
    mesh[24..28].copy_from_slice(&0x400u32.to_le_bytes()); // unlit vertices
    lumps.push((lump::MESHES, mesh));
    lumps.push((lump::MATERIAL_SORT, vec![0u8; 12])); // texture 0, vertex offset 0

    let mut tex = vec![0u8; 36];
    tex[12..16].copy_from_slice(&0u32.to_le_bytes()); // name index 0
    lumps.push((lump::TEXTURE_DATA, tex));
    lumps.push((lump::TEXTURE_DATA_STRING_TABLE, 0u32.to_le_bytes().to_vec()));
    lumps.push((
        lump::TEXTURE_DATA_STRING_DATA,
        b"world\\test\\wall\0".to_vec(),
    ));

    let mut out = vec![0u8; 16 + 128 * 16];
    out[0..4].copy_from_slice(b"rBSP");
    out[4..8].copy_from_slice(&29u32.to_le_bytes());
    for (id, data) in lumps {
        let at = 16 + id * 16;
        let offset = out.len() as u32;
        out[at..at + 4].copy_from_slice(&offset.to_le_bytes());
        out[at + 4..at + 8].copy_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
    }
    out
}

#[test]
fn reads_one_triangle() {
    let data = build();
    let g = Bsp::parse(&data).unwrap().geometry().unwrap();
    assert_eq!(g.positions.len(), 3);
    assert_eq!(g.materials, ["world/test/wall"]);
    assert_eq!(g.meshes.len(), 1);
    assert_eq!(g.meshes[0].kind, VertexKind::Unlit);
    assert_eq!(g.meshes[0].triangles, [[0, 1, 2]]);
    assert_eq!(g.meshes[0].material, 0);
    assert_eq!(g.bounds(), ([0., 0., 0.], [64., 64., 0.]));
}

#[test]
fn rejects_bad_data() {
    assert!(Bsp::parse(b"rBSP").is_err());
    let mut wrong_version = build();
    wrong_version[4] = 47;
    assert!(Bsp::parse(&wrong_version).is_err());

    // A mesh index that points past the vertex lump must be an error, not a panic.
    let mut bad = build();
    let idx_at = 16 + lump::MESH_INDICES * 16;
    let off = u32::from_le_bytes(bad[idx_at..idx_at + 4].try_into().unwrap()) as usize;
    bad[off..off + 2].copy_from_slice(&900u16.to_le_bytes());
    assert!(Bsp::parse(&bad).unwrap().geometry().is_err());
}
