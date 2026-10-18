//! Rust archive, asset, world/savegame and Daedalus APIs.
//!
//! [`io`] provides byte readers and writers; [`values`] provides math and date
//! records. [`archive`] handles ASCII, BINARY and BIN_SAFE encodings, and
//! [`objects`] manages shared object identity.
//!
//! Start with [`vfs::Vdf`] to read or rewrite an ordinary or Union archive.
//! [`vfs::Vfs`] combines archives, host snapshots and inserted files; see its
//! [composition example](vfs::Vfs#compose-save-and-reopen).
//!
//! Names and paths remain raw bytes; callers choose text decoding. Parsing uses
//! finite [`Limits`]. [`Error`] retains available offsets, paths and I/O sources.
//! [`ContextError`] adds owned runtime labels while preserving the typed error.
//! Compression is a runtime format choice, available without a feature flag.
#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

pub mod animation;
pub mod archive;
pub mod cutscene;
pub mod daedalus;
pub mod model_script;
pub mod morph;

mod compression;
mod error;
pub mod font;
pub mod io;
pub mod logging;
pub mod material;
pub mod mesh;
pub mod objects;
pub mod savegame;
pub mod texture;
pub mod values;
pub mod vfs;
pub mod vob;
pub mod world;

pub use error::{ContextError, Error, ErrorKind, Result};

#[cfg(test)]
mod tests;

/// Game variant; this is distinct from an individual file's format version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameVersion {
    Gothic1,
    Gothic2,
}

/// Finite limits checked before allocating from file-controlled counts.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub input_bytes: usize,
    pub entries: usize,
    pub nesting: usize,
    /// Sum of file payload lengths, including overlapping source ranges.
    pub payload_bytes: usize,
    /// Maximum decoded payload length of one file (ordinary or compressed).
    pub file_payload_bytes: usize,
    /// Total compressed blocks visited, including aliased streams.
    pub compression_blocks: usize,
    /// Maximum decoded block size and compressed block length.
    pub compression_block_bytes: usize,
    /// Total encoded stream bytes visited, including headers and aliased streams.
    pub compression_input_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            input_bytes: 256 * 1024 * 1024,
            entries: 100_000,
            nesting: 128,
            payload_bytes: 256 * 1024 * 1024,
            file_payload_bytes: 64 * 1024 * 1024,
            compression_blocks: 100_000,
            compression_block_bytes: 1024 * 1024,
            compression_input_bytes: 256 * 1024 * 1024,
        }
    }
}
