//! Our own interpreter for Titanfall's Squirrel scripts: Squirrel 2.2.3 as
//! Respawn modified it (`launcher.dll`), written from the language's public
//! reference manual and from what the game's scripts and DLLs show
//! (docs/port/scripts.md). The scripts themselves are read from the player's
//! install at run time; none are in this repository.
//!
//! Layer 0: no Bevy, no file access.

pub mod ast;
pub mod compiler;
pub mod fmt;
pub mod lexer;
pub mod parser;
mod stdlib;
pub mod table;
mod threads;
pub mod value;
pub mod vector;
pub mod vm;

pub use parser::{ParseError, parse};
pub use value::Value;
pub use vm::{NativeError, NativeResult, Vm, compile_typemask, native, native_checked, throw};
