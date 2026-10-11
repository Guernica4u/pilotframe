//! Our own interpreter for Titanfall's Squirrel scripts: Squirrel 2.2.3 as
//! Respawn modified it (`launcher.dll`), written from the language's public
//! reference manual and from what the game's scripts and DLLs show
//! (docs/port/scripts.md). The scripts themselves are read from the player's
//! install at run time; none are in this repository.
//!
//! Layer 0: no Bevy, no file access.

pub mod ast;
pub mod lexer;
pub mod parser;

pub use parser::{ParseError, parse};
