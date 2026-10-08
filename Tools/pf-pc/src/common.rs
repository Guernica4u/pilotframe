//! `materials-for`: pull the materials a map needs out of the shared
//! `mp_common` VPK, straight from the discs, without unpacking all 35 GB.
//!
//! Each `mp_common` archive is one Deflate stream inside the disc ZIP, so we
//! can't jump to a file. Instead we work out which byte ranges we need from
//! each archive and stream it once, keeping only those ranges.

use pf_fmt_vpk::{VpkDir, VpkFile};
use pf_fmt_vtf::vmt::Vmt;
use pf_zip::{RangeCapture, SplitZip};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::{fs, io};

const COMMON_DIR: &str = "vpk/englishclient_mp_common.bsp.pak000_dir.vpk";

pub fn materials_for(zip: &SplitZip, bsp_path: &Path, have: &Path, out: &Path) -> io::Result<()> {
    let bsp_data = fs::read(bsp_path)?;
    let bsp = pf_fmt_bsp::Bsp::parse(&bsp_data)?;
    let missing: Vec<String> = bsp
        .material_names()?
        .into_iter()
        .map(|n| n.to_ascii_lowercase())
        .filter(|n| !n.starts_with("tools/"))
        .filter(|n| !have.join(format!("{n}.vmt")).exists())
        .collect();
    println!(
        "{} materials missing from {}",
        missing.len(),
        have.display()
    );

    let entry = zip
        .find(COMMON_DIR)
        .ok_or_else(|| io::Error::other("mp_common VPK not found on the discs"))?;
    let mut dir_data = Vec::new();
    zip.extract(entry, &mut dir_data)?;
    let dir = VpkDir::parse(&dir_data)?;
    let by_path: BTreeMap<&str, &VpkFile> =
        dir.files.iter().map(|f| (f.path.as_str(), f)).collect();

    // Pass 0: prop models that aren't in the map's own pack.
    let have_models = have.parent().unwrap_or(have).join("models");
    let (prop_models, _) = bsp.static_props()?;
    let mut model_paths = Vec::new();
    for name in &prop_models {
        let name = name.to_ascii_lowercase();
        if !have_models
            .join(name.trim_start_matches("models/"))
            .exists()
        {
            for ext in ["mdl", "vvd", "dx11.vtx"] {
                model_paths.push(format!("{}.{ext}", name.trim_end_matches(".mdl")));
            }
        }
    }
    let picked_models = pick(zip, &dir_data, &by_path, &model_paths, out)?;

    // Materials every prop model needs that we don't have yet.
    let mut model_materials = BTreeSet::new();
    let picked: BTreeMap<&str, &[u8]> = picked_models
        .iter()
        .map(|(p, d)| (p.as_str(), d.as_slice()))
        .collect();
    for name in &prop_models {
        let base = name.to_ascii_lowercase();
        let base = base.trim_end_matches(".mdl");
        let read = |ext: &str| -> Option<Vec<u8>> {
            let rel = format!("{base}.{ext}");
            picked
                .get(rel.as_str())
                .map(|d| d.to_vec())
                .or_else(|| fs::read(have_models.join(rel.trim_start_matches("models/"))).ok())
        };
        let (Some(mdl), Some(vvd), Some(vtx)) = (read("mdl"), read("vvd"), read("dx11.vtx")) else {
            continue;
        };
        let Ok(model) = pf_fmt_mdl::load(&mdl, &vvd, &vtx) else {
            continue;
        };
        for mesh in model.meshes {
            if !mesh
                .materials
                .iter()
                .any(|m| have.join(format!("{m}.vmt")).exists())
            {
                model_materials.extend(mesh.materials);
            }
        }
    }

    // Pass 1: the missing .vmt files (world and props).
    let vmt_paths: Vec<String> = missing
        .iter()
        .chain(&model_materials)
        .map(|n| format!("materials/{n}.vmt"))
        .collect();
    let vmts = pick(zip, &dir_data, &by_path, &vmt_paths, out)?;

    // Pass 2: textures they (and the map's own materials) use that we don't have.
    let mut textures = BTreeSet::new();
    let mut scan = |text: &str| {
        let vmt = Vmt::parse(text);
        for key in ["$basetexture", "$basetexture2"] {
            if let Some(t) = vmt.params.get(key) {
                let t = t.replace('\\', "/").to_ascii_lowercase();
                let t = t.trim_end_matches(".vtf");
                if !have.join(format!("{t}.vtf")).exists() {
                    textures.insert(format!("materials/{t}.vtf"));
                }
            }
        }
    };
    for (_, data) in &vmts {
        scan(&String::from_utf8_lossy(data));
    }
    for name in bsp.material_names()? {
        if let Ok(text) = fs::read(have.join(format!("{}.vmt", name.to_ascii_lowercase()))) {
            scan(&String::from_utf8_lossy(&text));
        }
    }
    let textures: Vec<String> = textures.into_iter().collect();
    pick(zip, &dir_data, &by_path, &textures, out)?;
    Ok(())
}

/// `model-materials`: the materials and textures that the given local models
/// use, pulled from `mp_common` into `out` (skipping ones `out` already has).
pub fn model_materials(zip: &SplitZip, models: &[&Path], out: &Path) -> io::Result<()> {
    let entry = zip
        .find(COMMON_DIR)
        .ok_or_else(|| io::Error::other("mp_common VPK not found on the discs"))?;
    let mut dir_data = Vec::new();
    zip.extract(entry, &mut dir_data)?;
    let dir = VpkDir::parse(&dir_data)?;
    let by_path: BTreeMap<&str, &VpkFile> =
        dir.files.iter().map(|f| (f.path.as_str(), f)).collect();
    let have = out.join("materials");

    let mut vmt_paths = BTreeSet::new();
    for mdl in models {
        let read = |ext: &str| fs::read(mdl.with_extension(ext));
        let model = pf_fmt_mdl::load(&read("mdl")?, &read("vvd")?, &read("dx11.vtx")?)?;
        for mesh in model.meshes {
            // Every $cdmaterials candidate the VPK actually has.
            for m in mesh.materials {
                let path = format!(
                    "materials/{}.vmt",
                    m.to_ascii_lowercase().replace('\\', "/")
                );
                if by_path.contains_key(path.as_str()) && !out.join(&path).exists() {
                    vmt_paths.insert(path);
                }
            }
        }
    }
    let vmt_paths: Vec<String> = vmt_paths.into_iter().collect();
    let vmts = pick(zip, &dir_data, &by_path, &vmt_paths, out)?;

    let mut textures = BTreeSet::new();
    for (_, data) in &vmts {
        let vmt = Vmt::parse(&String::from_utf8_lossy(data));
        for key in ["$basetexture", "$basetexture2", "$bumpmap"] {
            if let Some(t) = vmt.params.get(key) {
                let t = t.replace('\\', "/").to_ascii_lowercase();
                let t = t.trim_end_matches(".vtf");
                if !have.join(format!("{t}.vtf")).exists() {
                    textures.insert(format!("materials/{t}.vtf"));
                }
            }
        }
    }
    let textures: Vec<String> = textures.into_iter().collect();
    pick(zip, &dir_data, &by_path, &textures, out)?;
    Ok(())
}

/// `common-extract`: every `mp_common` file whose path contains `filter`
/// (lowercase), apart from the blocked types.
pub fn extract_matching(zip: &SplitZip, filter: &str, out: &Path) -> io::Result<()> {
    let entry = zip
        .find(COMMON_DIR)
        .ok_or_else(|| io::Error::other("mp_common VPK not found on the discs"))?;
    let mut dir_data = Vec::new();
    zip.extract(entry, &mut dir_data)?;
    let dir = VpkDir::parse(&dir_data)?;
    let by_path: BTreeMap<&str, &VpkFile> =
        dir.files.iter().map(|f| (f.path.as_str(), f)).collect();
    let paths: Vec<String> = dir
        .files
        .iter()
        .map(|f| f.path.clone())
        .filter(|p| p.contains(filter))
        .filter(|p| !crate::blocked(p))
        .collect();
    pick(zip, &dir_data, &by_path, &paths, out)?;
    Ok(())
}

/// Extract the named files (those that exist in the VPK) into `out/<path>`,
/// streaming each needed archive once. Returns the files' contents.
fn pick(
    zip: &SplitZip,
    dir_data: &[u8],
    by_path: &BTreeMap<&str, &VpkFile>,
    paths: &[String],
    out: &Path,
) -> io::Result<Vec<(String, Vec<u8>)>> {
    let files: Vec<&VpkFile> = paths
        .iter()
        .filter_map(|p| by_path.get(p.as_str()).copied())
        .collect();
    println!(
        "picking {} of {} files from mp_common",
        files.len(),
        paths.len()
    );

    // archive index -> list of (file index, chunk index, offset, compressed size)
    let mut wanted: BTreeMap<u16, Vec<(usize, usize, u64, u64)>> = BTreeMap::new();
    for (fi, f) in files.iter().enumerate() {
        for (ci, c) in f.chunks.iter().enumerate() {
            wanted
                .entry(c.archive)
                .or_default()
                .push((fi, ci, c.offset, c.compressed_size));
        }
    }

    let mut raw: BTreeMap<(usize, usize), Vec<u8>> = BTreeMap::new();
    for (archive, list) in &wanted {
        let name = format!("vpk/client_mp_common.bsp.pak000_{archive:03}.vpk");
        let entry = zip
            .find(&name)
            .ok_or_else(|| io::Error::other(format!("{name} not found on the discs")))?;
        println!(
            "  streaming {name} ({:.1} GB) for {} chunks",
            entry.size as f64 / 1e9,
            list.len()
        );
        let mut capture = RangeCapture::new(list.iter().map(|&(_, _, o, n)| (o, n)).collect());
        zip.extract(entry, &mut capture)?;
        for (&(fi, ci, _, _), data) in list.iter().zip(capture.data) {
            raw.insert((fi, ci), data);
        }
    }

    let mut result = Vec::new();
    for (fi, f) in files.iter().enumerate() {
        let mut data = dir_data[f.preload.0..f.preload.0 + f.preload.1].to_vec();
        for (ci, c) in f.chunks.iter().enumerate() {
            let bytes = raw.remove(&(fi, ci)).unwrap_or_default();
            if c.is_compressed() {
                data.extend(pf_lzham::decompress(&bytes, c.size as usize)?);
            } else {
                data.extend(bytes);
            }
        }
        if crate::crc32(&data) != f.crc32 {
            return Err(io::Error::other(format!("{}: CRC-32 mismatch", f.path)));
        }
        let dest = out.join(&f.path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&dest, &data)?;

        result.push((f.path.clone(), data));
    }
    Ok(result)
}
