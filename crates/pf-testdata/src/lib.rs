//! Real-data tests call [`dump`] first and return early when it gives `None`.
//! CI never sets `PF_GAME_DUMP`, so those tests pass there without game data.

use std::path::PathBuf;

pub const ENV_VAR: &str = "PF_GAME_DUMP";

/// Path to the local disc image or extracted folder, if configured.
pub fn dump() -> Option<PathBuf> {
    match std::env::var_os(ENV_VAR) {
        Some(p) if !p.is_empty() => Some(PathBuf::from(p)),
        _ => {
            eprintln!("skipped: {ENV_VAR} not set");
            None
        }
    }
}
