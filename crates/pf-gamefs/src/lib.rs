//! The game's files, read straight from the player's install: one lookup over
//! a mod folder (loose files, checked first) and the install's VPK packs.
//!
//! The shared pack (`englishclient_mp_common.bsp.pak000_dir.vpk`) is mounted
//! at start; a map's own pack is mounted when the map loads, in front of the
//! shared one. Paths are the game's own (`materials/...`, `scripts/...`), any
//! case, either slash. Files the packs keep under `depot/r1dev/game/r1/` (the
//! maps) are found without that prefix too.
//!
//! It's a global (behind a lock), so plain loading functions anywhere can read
//! files without being handed it. No Bevy here: tools and tests use it too.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

use pf_fmt_vpk::{VpkDir, VpkFile};
use pf_vfs::{FileSource, ReadAt};

/// Where the packs keep some files (the maps), dropped for lookups.
const DEPOT_PREFIX: &str = "depot/r1dev/game/r1/";
/// The language of the packs we mount.
const LANGUAGE: &str = "english";

static FS: RwLock<GameFs> = RwLock::new(GameFs {
    install: None,
    mods: None,
    packs: Vec::new(),
});

struct GameFs {
    install: Option<PathBuf>,
    mods: Option<PathBuf>,
    /// Mounted packs, searched last to first.
    packs: Vec<Pack>,
}

struct Pack {
    name: String,
    /// The dir file's bytes (preload data lives in it).
    dir_data: Vec<u8>,
    files: HashMap<String, VpkFile>,
    /// `.../client_<name>.bsp.pak000`, to which `_NNN.vpk` is added.
    archive_base: PathBuf,
    archives: Mutex<HashMap<u16, FileSource>>,
}

/// A path as the packs store it: lower case, forward slashes, no leading
/// slash, `.` and `..` resolved (HUD images use paths like `vgui/../ui/x`).
fn normalise(path: &str) -> String {
    let lower = path.replace('\\', "/").to_ascii_lowercase();
    let mut parts: Vec<&str> = Vec::new();
    for part in lower.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

impl Pack {
    /// Mount `<vpk>/englishclient_<name>.bsp.pak000_dir.vpk`.
    fn open(vpk_dir: &Path, name: &str) -> io::Result<Self> {
        let dir_path = vpk_dir.join(format!("{LANGUAGE}client_{name}.bsp.pak000_dir.vpk"));
        let dir_data = std::fs::read(&dir_path)
            .map_err(|e| io::Error::other(format!("{}: {e}", dir_path.display())))?;
        let dir = VpkDir::parse(&dir_data)?;
        let mut files = HashMap::with_capacity(dir.files.len() * 2);
        for f in dir.files {
            let path = normalise(&f.path);
            if let Some(short) = path.strip_prefix(DEPOT_PREFIX) {
                files.insert(short.to_string(), f.clone());
            }
            files.insert(path, f);
        }
        Ok(Self {
            name: name.to_string(),
            dir_data,
            files,
            archive_base: vpk_dir.join(format!("client_{name}.bsp.pak000")),
            archives: Mutex::new(HashMap::new()),
        })
    }

    fn read(&self, f: &VpkFile) -> io::Result<Vec<u8>> {
        let (start, len) = f.preload;
        let mut data = self
            .dir_data
            .get(start..start + len)
            .ok_or_else(|| io::Error::other("preload out of range"))?
            .to_vec();
        let mut archives = self.archives.lock().unwrap_or_else(|e| e.into_inner());
        for c in &f.chunks {
            let archive = match archives.entry(c.archive) {
                std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                std::collections::hash_map::Entry::Vacant(e) => {
                    let path = PathBuf::from(format!(
                        "{}_{:03}.vpk",
                        self.archive_base.display(),
                        c.archive
                    ));
                    e.insert(FileSource::open(&path)?)
                }
            };
            let mut raw = vec![0u8; c.compressed_size as usize];
            archive.read_exact_at(c.offset, &mut raw)?;
            if c.is_compressed() {
                raw = pf_lzham::decompress(&raw, c.size as usize)?;
            }
            data.extend_from_slice(&raw);
        }
        Ok(data)
    }
}

/// Mount an install (the folder holding `vpk/`): its shared pack, plus the
/// mod folder `mods` if one is given. Replaces anything mounted before.
pub fn mount_install(install: &Path, mods: Option<&Path>) -> io::Result<()> {
    let common = Pack::open(&install.join("vpk"), "mp_common")?;
    let mut fs = FS.write().unwrap_or_else(|e| e.into_inner());
    fs.install = Some(install.to_path_buf());
    fs.mods = mods.map(Path::to_path_buf);
    fs.packs = vec![common];
    Ok(())
}

/// Mount a map's pack (e.g. `mp_colony`) in front of the shared one, replacing
/// any other map's.
pub fn mount_map(name: &str) -> io::Result<()> {
    let mut fs = FS.write().unwrap_or_else(|e| e.into_inner());
    let install = fs
        .install
        .clone()
        .ok_or_else(|| io::Error::other("no game install mounted"))?;
    if fs.packs.iter().any(|p| p.name == name) {
        return Ok(());
    }
    let pack = Pack::open(&install.join("vpk"), name)?;
    fs.packs.truncate(1);
    fs.packs.push(pack);
    Ok(())
}

/// Whether an install is mounted.
pub fn mounted() -> bool {
    FS.read().map(|fs| fs.install.is_some()).unwrap_or(false)
}

/// A file's bytes: from the mod folder if it's there, else the newest pack
/// that has it.
pub fn read(path: &str) -> Option<Vec<u8>> {
    let path = normalise(path);
    let fs = FS.read().unwrap_or_else(|e| e.into_inner());
    if let Some(mods) = &fs.mods
        && let Ok(data) = std::fs::read(mods.join(&path))
    {
        return Some(data);
    }
    fs.packs.iter().rev().find_map(|pack| {
        let f = pack.files.get(&path)?;
        pack.read(f)
            .map_err(|e| eprintln!("can't read {path} from {}: {e}", pack.name))
            .ok()
    })
}

/// A text file: UTF-16 with a byte-order mark (the localisation and playlists
/// are) or UTF-8.
pub fn read_text(path: &str) -> Option<String> {
    let bytes = read(path)?;
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_le_bytes(c))
            .collect();
        return Some(String::from_utf16_lossy(&units));
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    Some(String::from_utf8_lossy(bytes).into_owned())
}

/// Every file in the mounted packs whose path starts with `prefix`, sorted
/// (mod-folder files aren't listed).
pub fn list(prefix: &str) -> Vec<String> {
    let prefix = normalise(prefix);
    let fs = FS.read().unwrap_or_else(|e| e.into_inner());
    let mut out: Vec<String> = fs
        .packs
        .iter()
        .flat_map(|p| p.files.keys())
        .filter(|k| k.starts_with(&prefix) && !k.starts_with(DEPOT_PREFIX))
        .cloned()
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Whether a file exists (in the mod folder or a mounted pack).
pub fn exists(path: &str) -> bool {
    let path = normalise(path);
    let fs = FS.read().unwrap_or_else(|e| e.into_inner());
    fs.mods.as_ref().is_some_and(|m| m.join(&path).is_file())
        || fs.packs.iter().any(|p| p.files.contains_key(&path))
}

/// Find the install: `PF_GAME_DIR`, else a folder under `search` (up to three
/// levels deep) holding `vpk/englishclient_mp_common.bsp.pak000_dir.vpk`.
pub fn find_install(search: &Path) -> Option<PathBuf> {
    let is_install = |p: &Path| {
        p.join("vpk")
            .join(format!("{LANGUAGE}client_mp_common.bsp.pak000_dir.vpk"))
            .is_file()
    };
    if let Some(dir) = std::env::var_os("PF_GAME_DIR").map(PathBuf::from) {
        return is_install(&dir).then_some(dir);
    }
    let mut level = vec![search.to_path_buf()];
    for _ in 0..=3 {
        if let Some(found) = level.iter().find(|p| is_install(p)) {
            return Some(found.clone());
        }
        level = level
            .iter()
            .filter_map(|p| std::fs::read_dir(p).ok())
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_normalise() {
        assert_eq!(
            normalise("\\Materials\\VGUI\\HUD\\x.vmt"),
            "materials/vgui/hud/x.vmt"
        );
        assert_eq!(
            normalise("materials/vgui/../ui/scoreboard_mcorp_logo.vmt"),
            "materials/ui/scoreboard_mcorp_logo.vmt"
        );
    }
}
