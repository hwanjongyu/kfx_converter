//! Amazon KFX Binary Packager
//!
//! Assembles `BookIR` into KFX entity records (Metadata, DocumentData, Storylines, Content, Resources)
//! and serializes them into a valid `.kfx` container file.

pub mod accumulator;
pub mod core;
pub mod error;
pub mod locators;
pub mod metadata;
pub mod navigation;
pub mod resources;
pub mod storylines;
pub mod styles;
pub mod symbols;
pub mod types;

#[cfg(test)]
mod tests;

pub use accumulator::*;
pub use core::*;
pub use error::*;
pub use styles::*;
pub use symbols::*;
pub use types::*;
