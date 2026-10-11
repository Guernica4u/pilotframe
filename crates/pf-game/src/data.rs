//! The game's files: the player's EA install, read through `pf_world::gamefs`.

use std::path::Path;

/// Find and mount the install (`PF_GAME_DIR`, else searched for under
/// `Resources/`), with `mods/` in the working folder as the mod folder: files
/// there override the game's. Without an install the game still runs, on
/// placeholders.
pub fn mount() {
    let Some(install) = pf_world::gamefs::find_install(Path::new("Resources")) else {
        eprintln!(
            "no EA install found under Resources/ (set PF_GAME_DIR); running without game files"
        );
        return;
    };
    match pf_world::gamefs::mount_install(&install, Some(Path::new("mods"))) {
        Ok(()) => eprintln!("game files: {}", install.display()),
        Err(e) => eprintln!("can't read the install at {}: {e}", install.display()),
    }
}

/// A file's bytes (`scripts/...`, `materials/...`).
pub fn read(path: &str) -> Option<Vec<u8>> {
    pf_world::gamefs::read(path)
}

/// A text file (UTF-8, or UTF-16 with a byte-order mark).
pub fn read_text(path: &str) -> Option<String> {
    pf_world::gamefs::read_text(path)
}
