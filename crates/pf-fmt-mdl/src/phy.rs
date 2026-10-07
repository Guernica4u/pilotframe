//! `.phy`: a model's collision shape, as convex pieces.
//!
//! Layout from the public Valve Developer Community "PHY" page and the
//! public Source SDK 2013 `phyfile.h`/`vcollide_parse` documentation, checked
//! against the owner's files. The file is:
//!
//! - a 16-byte header: header size, id, solid count, checksum;
//! - each solid: its byte size (u32), then a 28-byte header starting with the tag "VPHY", then a
//!   48-byte "compact surface" header (offset +28 = byte size and deviation
//!   packed into one u32; +32 = offset of the ledge tree, which follows the
//!   pieces);
//! - the pieces ("ledges"), one after another, each a 16-byte header (point
//!   array offset, unused, flags with size/16 in the top 24 bits, triangle count)
//!   and 16-byte triangles of three 4-byte edges whose low 16 bits are a point
//!   index. Points are 16 bytes: x, y, z, w floats.
//!
//! Points are in the physics engine's space: metres, with Source (x, y, z) stored
//! as (x, -z, y). We convert back to Source inches.

use std::io;

/// Inches per metre.
const UNITS_PER_METRE: f32 = 39.370_08;
const SOLID_HEADER: usize = 28;
const SURFACE_HEADER: usize = 48;
const LEDGE_HEADER: usize = 16;
const TRIANGLE: usize = 16;

/// Collision triangles for a whole model, in model space (Source units, Z up).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Collision {
    /// How many separate solids the file had. Static props have one; ragdolls
    /// have one per bone, each in that bone's own space.
    pub solids: u32,
    pub points: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

/// Read every convex piece of every solid in a `.phy` file.
pub fn load_phy(data: &[u8]) -> io::Result<Collision> {
    let header_size = u32_at(data, 0)? as usize;
    let solids = u32_at(data, 8)?;
    let mut out = Collision {
        solids,
        ..Default::default()
    };
    let mut at = header_size;
    for _ in 0..solids {
        let size = u32_at(data, at)? as usize;
        let solid = at + 4;
        if data.get(solid..solid + 4) != Some(b"VPHY".as_slice()) {
            // Older "legacy" solids have no VPHY header. Not seen in Titanfall.
            return Err(invalid("solid without VPHY header"));
        }
        read_surface(data, solid + SOLID_HEADER, &mut out)?;
        at = solid + size;
    }
    Ok(out)
}

fn read_surface(data: &[u8], surface: usize, out: &mut Collision) -> io::Result<()> {
    let tree = surface + u32_at(data, surface + 32)? as usize;
    let mut ledge = surface + SURFACE_HEADER;
    // Points can sit right after each piece's triangles or in one shared block
    // after all the pieces; either way pieces stop before the first points.
    let mut first_points = tree;
    while ledge + LEDGE_HEADER <= tree.min(first_points) {
        let points = ledge
            .checked_add_signed(i32_at(data, ledge)? as isize)
            .ok_or_else(|| invalid("bad point offset"))?;
        let size = (u32_at(data, ledge + 8)? >> 8) as usize * 16;
        let count = u16_at(data, ledge + 12)? as usize;
        if count == 0 || size == 0 {
            break;
        }
        let base = out.points.len() as u32;
        let mut used = 0usize;
        for t in 0..count {
            let tri = ledge + LEDGE_HEADER + t * TRIANGLE;
            let mut corner = [0u32; 3];
            for (k, c) in corner.iter_mut().enumerate() {
                let index = u16_at(data, tri + 4 + k * 4)? as usize;
                used = used.max(index + 1);
                *c = base + index as u32;
            }
            // The physics engine winds the other way round from what we want
            // facing outwards; collision is two-sided, but keep it consistent.
            out.triangles.push([corner[0], corner[2], corner[1]]);
        }
        for i in 0..used {
            let p = points + i * 16;
            let [x, y, z] = [f32_at(data, p)?, f32_at(data, p + 4)?, f32_at(data, p + 8)?];
            out.points.push([
                x * UNITS_PER_METRE,
                z * UNITS_PER_METRE,
                -y * UNITS_PER_METRE,
            ]);
        }
        let after_triangles = ledge + LEDGE_HEADER + count * TRIANGLE;
        if points >= after_triangles {
            first_points = first_points.min(points);
        }
        // A piece's size covers its own points when they follow it directly.
        ledge = if points == after_triangles {
            ledge + size
        } else {
            after_triangles
        };
    }
    Ok(())
}

fn bytes<const N: usize>(data: &[u8], at: usize) -> io::Result<[u8; N]> {
    data.get(at..at + N)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| invalid("phy file is truncated"))
}

fn u16_at(data: &[u8], at: usize) -> io::Result<u16> {
    bytes(data, at).map(u16::from_le_bytes)
}

fn u32_at(data: &[u8], at: usize) -> io::Result<u32> {
    bytes(data, at).map(u32::from_le_bytes)
}

fn i32_at(data: &[u8], at: usize) -> io::Result<i32> {
    bytes(data, at).map(i32::from_le_bytes)
}

fn f32_at(data: &[u8], at: usize) -> io::Result<f32> {
    bytes(data, at).map(f32::from_le_bytes)
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-solid file with a single tetrahedron, built by hand.
    fn tetrahedron() -> Vec<u8> {
        let points = [
            [0.0f32, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        let tris = [[0u16, 1, 2], [0, 3, 1], [0, 2, 3], [1, 3, 2]];
        let ledge_len = LEDGE_HEADER + tris.len() * TRIANGLE + points.len() * 16;
        let surface_len = SURFACE_HEADER + ledge_len;

        let mut d = Vec::new();
        for v in [16u32, 0, 1, 0] {
            d.extend(v.to_le_bytes());
        }
        d.extend(((SOLID_HEADER + surface_len) as u32).to_le_bytes());
        // VPHY header: tag, version, type, surface size, drag areas, axis map size.
        d.extend(b"VPHY");
        d.extend(0x100u16.to_le_bytes());
        d.extend(0u16.to_le_bytes());
        d.extend((surface_len as u32).to_le_bytes());
        d.resize(d.len() + 16, 0);
        // Compact surface header.
        let surface = d.len();
        d.resize(surface + SURFACE_HEADER, 0);
        d[surface + 32..surface + 36].copy_from_slice(&(surface_len as u32).to_le_bytes());
        // The piece.
        d.extend(((LEDGE_HEADER + tris.len() * TRIANGLE) as i32).to_le_bytes());
        d.extend(0u32.to_le_bytes());
        d.extend((((ledge_len / 16) as u32) << 8 | 4).to_le_bytes());
        d.extend((tris.len() as u16).to_le_bytes());
        d.extend(0u16.to_le_bytes());
        for t in tris {
            d.extend(0u32.to_le_bytes());
            for i in t {
                d.extend(u32::from(i).to_le_bytes());
            }
        }
        for p in points {
            for v in p {
                d.extend(v.to_le_bytes());
            }
            d.extend(0f32.to_le_bytes());
        }
        d
    }

    #[test]
    fn reads_a_tetrahedron_and_converts_to_source_inches() {
        let c = load_phy(&tetrahedron()).unwrap();
        assert_eq!(c.triangles.len(), 4);
        assert_eq!(c.points.len(), 4);
        // Physics (0, 1, 0) is Source (0, 0, -1) metre.
        let p = c.points[2];
        assert!(p[0].abs() < 1e-4 && p[1].abs() < 1e-4 && (p[2] + UNITS_PER_METRE).abs() < 1e-3);
        assert!(c.triangles.iter().flatten().all(|&i| i < 4));
    }

    #[test]
    fn rejects_garbage() {
        assert!(load_phy(&[1, 2, 3]).is_err());
        let mut bad = tetrahedron();
        bad[20..24].copy_from_slice(b"XXXX");
        assert!(load_phy(&bad).is_err());
    }
}
