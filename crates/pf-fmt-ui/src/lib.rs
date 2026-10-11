//! Readers for the game's UI files.
//!
//! - [`kv`]: Valve KeyValues text, the format of `.res` layouts, schemes,
//!   localisation and weapon scripts.
//! - [`vfont`]: the lightly scrambled `.vfont` font files.
//! - [`layout`]: where VGUI panels end up on screen (positions, sizes, pinning).
//!
//! No Bevy and no file system here: callers hand in bytes or text.

pub mod kv;
pub mod layout;
pub mod vfont;
