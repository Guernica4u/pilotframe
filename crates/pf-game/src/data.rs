//! Where the player's own unpacked game files are.

use std::path::PathBuf;

/// The shared `mp_common` files (UI, weapons, models, materials) from the EA
/// install, unpacked with `pf-vpk extract`. If they're not there, the game runs
/// without those assets.
// TODO (R1): read the install's VPKs directly instead.
pub fn common_dir() -> PathBuf {
    PathBuf::from("Resources/ea_extracted/common")
}
