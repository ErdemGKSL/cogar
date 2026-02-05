//! Shared protocol crate for native-ogar.
//!
//! This crate contains:
//! - Binary reading/writing utilities
//! - Packet definitions and builders
//! - Shared types (Color, Position, etc.)

mod binary;
mod error;
pub mod packets;

pub use binary::{BinaryReader, BinaryWriter};
pub use error::ProtocolError;

/// RGB color used for cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// Represents a 2D position using glam's Vec2.
pub type Position = glam::Vec2;
