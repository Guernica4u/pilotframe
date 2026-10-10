//! Where the player's own unpacked game files are.

use std::path::PathBuf;

/// The shared `mp_common` files (UI, weapons, models, materials). Prefers the
/// final PC patch (v1.0.10.1, unpacked from an installed copy with
/// `pf-pc vpk-extract`), then the launch build from the retail discs
/// (`pf-pc common-extract`). If neither is there, the folder simply doesn't
/// exist and the game runs without those assets.
// TODO: replace with the first-launch setup's `data/` folder.
pub fn common_dir() -> PathBuf {
    [
        "Resources/ea_extracted/common",
        "Resources/pc_extracted/common",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| p.is_dir())
    .unwrap_or_else(|| PathBuf::from("Resources/ea_extracted/common"))
}
