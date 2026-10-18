//! Schema-directed ZenGin archives: ASCII, BINARY, and BIN_SAFE.
//!
//! This layer handles wire metadata, primitives and nested records. It does not
//! instantiate game classes or resolve reference identity. Class `%` denotes an
//! empty object (also an empty class string in BINARY), and raw byte `0xa7`
//! denotes a reference; both remain visible.
//! BINARY stores neither field names nor types, so callers supply [`ValueType`].
//! Strings are bytes; use `std::str::from_utf8` for explicit strict decoding.
//!
//! ```
//! use zenkit::archive::{Format, Header, Limits, Reader, Value, ValueType, Writer};
//! let mut writer = Writer::new(Header::new(Format::BinSafe), Limits::default())?;
//! writer.begin_object(b"root", b"Example", 1)?;
//! writer.write(b"answer", Value::Int(42))?;
//! writer.end_object()?;
//! let bytes = writer.finish()?;
//! let mut reader = Reader::new(&bytes, Limits::default())?;
//! assert_eq!(reader.begin_object()?.unwrap().class_name, b"Example");
//! assert_eq!(reader.read(ValueType::Int)?.value, Value::Int(42));
//! assert!(reader.end_object()?);
//! reader.finish()?;
//! # Ok::<(), zenkit::Error>(())
//! ```
// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
mod input;
pub(crate) use input::{AddressSpace, AsciiSeparator, TableBudget};
mod read;
mod write;
use crate::{
    Error, ErrorKind, Result,
    values::{Aabb, Color, Mat3, Vec2, Vec3},
};
pub use read::{Reader, SkipLimits};
use std::borrow::Cow;
pub use write::Writer;

/// Wire encoding. BINARY requires the caller's object schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    Ascii,
    Binary,
    BinSafe,
}

/// Raw archive metadata. Version 1 is supported; date/user are uninterpreted bytes.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct Header<'a> {
    pub version: u32,
    pub archiver: &'a [u8],
    pub format: Format,
    pub save_game: bool,
    pub date: Option<&'a [u8]>,
    pub user: Option<&'a [u8]>,
}
impl Header<'static> {
    /// Deterministic defaults: version 1, canonical archiver, no user or date.
    pub const fn new(format: Format) -> Self {
        Self {
            version: 1,
            archiver: match format {
                Format::BinSafe => b"zCArchiverBinSafe",
                _ => b"zCArchiverGeneric",
            },
            format,
            save_game: false,
            date: None,
            user: None,
        }
    }
}

/// Wire object identity. Indices are numbers, not validated graph handles.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ObjectHeader<'a> {
    pub object_name: &'a [u8],
    pub class_name: &'a [u8],
    pub version: u16,
    pub index: u32,
}
impl ObjectHeader<'_> {
    pub fn is_reference(&self) -> bool {
        self.class_name == b"\xa7"
    }
    pub fn is_empty(&self) -> bool {
        self.class_name.is_empty() || self.class_name == b"%"
    }
}

/// Logical size/work bounds, not an allocator quota. All records (including
/// empty/reference records) count towards `objects`; root depth is one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    /// Complete input/output bytes, including headers and the BIN_SAFE key table.
    pub bytes: usize,
    /// Full ASCII/header line bytes (excluding its ending), and individual
    /// binary variable payload/key bytes. Fixed-size binary scalars are exempt.
    pub field_bytes: usize,
    pub objects: usize,
    pub depth: usize,
    pub keys: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            bytes: 256 * 1024 * 1024,
            field_bytes: 1024 * 1024,
            objects: 1_000_000,
            depth: 128,
            keys: 65_536,
        }
    }
}

/// Expected primitive. Raw counts are exact for untagged BINARY and minimums
/// for tagged ASCII/BIN_SAFE; their complete payloads are returned.
/// Fixed typed values require exact shapes. A raw/rawFloat wire tag permits a
/// raw fallback retaining every decoded byte/element; a different typed wire tag
/// cannot be reinterpreted as raw. Use explicit unframed capture or skipping for
/// that case. Decoded values do not retain ASCII spelling or table layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueType {
    String,
    Int,
    Float,
    Byte,
    Word,
    Enum,
    Bool,
    Color,
    Vec2,
    Vec3,
    Aabb,
    Mat3,
    Raw(usize),
    RawFloat(usize),
}

/// A typed archive primitive. ASCII hex and float arrays need owned decoding;
/// binary strings/raw data borrow input. No implicit legacy text conversion occurs.
#[derive(Clone, PartialEq)]
pub enum Value<'a> {
    String(Cow<'a, [u8]>),
    Int(i32),
    Float(f32),
    Byte(u8),
    Word(u16),
    Enum(u32),
    Bool(bool),
    Color(Color),
    Vec2(Vec2),
    Vec3(Vec3),
    Aabb(Aabb),
    Mat3(Mat3),
    Raw(Cow<'a, [u8]>),
    RawFloat(Cow<'a, [f32]>),
}
impl Value<'_> {
    pub fn value_type(&self) -> ValueType {
        match self {
            Self::String(_) => ValueType::String,
            Self::Int(_) => ValueType::Int,
            Self::Float(_) => ValueType::Float,
            Self::Byte(_) => ValueType::Byte,
            Self::Word(_) => ValueType::Word,
            Self::Enum(_) => ValueType::Enum,
            Self::Bool(_) => ValueType::Bool,
            Self::Color(_) => ValueType::Color,
            Self::Vec2(_) => ValueType::Vec2,
            Self::Vec3(_) => ValueType::Vec3,
            Self::Aabb(_) => ValueType::Aabb,
            Self::Mat3(_) => ValueType::Mat3,
            Self::Raw(v) => ValueType::Raw(v.len()),
            Self::RawFloat(v) => ValueType::RawFloat(v.len()),
        }
    }
}
/// Field name is absent in BINARY. ASCII/BIN_SAFE retain raw names.
#[derive(Clone, PartialEq)]
pub struct Entry<'a> {
    pub name: Option<&'a [u8]>,
    pub value: Value<'a>,
}

impl std::fmt::Debug for Header<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Header")
            .field("version", &self.version)
            .field("format", &self.format)
            .field("save_game", &self.save_game)
            .field("archiver_bytes", &self.archiver.len())
            .field("date_bytes", &self.date.map(<[u8]>::len))
            .field("user_bytes", &self.user.map(<[u8]>::len))
            .finish()
    }
}
impl std::fmt::Debug for ObjectHeader<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObjectHeader")
            .field("name_bytes", &self.object_name.len())
            .field("class_bytes", &self.class_name.len())
            .field("version", &self.version)
            .field("index", &self.index)
            .field("reference", &self.is_reference())
            .field("empty", &self.is_empty())
            .finish()
    }
}
impl std::fmt::Debug for Value<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String(v) => f.debug_struct("String").field("bytes", &v.len()).finish(),
            Self::Raw(v) => f.debug_struct("Raw").field("bytes", &v.len()).finish(),
            Self::RawFloat(v) => f
                .debug_struct("RawFloat")
                .field("elements", &v.len())
                .finish(),
            Self::Int(v) => f.debug_tuple("Int").field(v).finish(),
            Self::Float(v) => f.debug_tuple("Float").field(v).finish(),
            Self::Byte(v) => f.debug_tuple("Byte").field(v).finish(),
            Self::Word(v) => f.debug_tuple("Word").field(v).finish(),
            Self::Enum(v) => f.debug_tuple("Enum").field(v).finish(),
            Self::Bool(v) => f.debug_tuple("Bool").field(v).finish(),
            Self::Color(v) => f.debug_tuple("Color").field(v).finish(),
            Self::Vec2(v) => f.debug_tuple("Vec2").field(v).finish(),
            Self::Vec3(v) => f.debug_tuple("Vec3").field(v).finish(),
            Self::Aabb(v) => f.debug_tuple("Aabb").field(v).finish(),
            Self::Mat3(v) => f.debug_tuple("Mat3").field(v).finish(),
        }
    }
}
impl std::fmt::Debug for Entry<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entry")
            .field("name_bytes", &self.name.map(<[u8]>::len))
            .field("value", &self.value)
            .finish()
    }
}

const CTX: &str = "archive";
fn invalid(offset: usize) -> Error {
    Error::new(ErrorKind::InvalidData, offset, CTX)
}
fn limited(offset: usize) -> Error {
    Error::new(ErrorKind::LimitExceeded, offset, CTX)
}
fn output_error(kind: ErrorKind) -> Error {
    Error::operation(kind, CTX)
}
fn number<T: std::str::FromStr>(bytes: &[u8], offset: usize) -> Result<T> {
    std::str::from_utf8(bytes)
        .map_err(|e| invalid(offset + e.valid_up_to()))?
        .parse()
        .map_err(|_| invalid(offset))
}
fn trim(bytes: &[u8]) -> &[u8] {
    bytes.trim_ascii()
}
fn parts(bytes: &[u8]) -> impl Iterator<Item = (usize, &[u8])> {
    bytes
        .split(|b| b.is_ascii_whitespace())
        .scan(0usize, |offset, part| {
            let start = *offset;
            *offset += part.len() + 1;
            Some((start, part))
        })
        .filter(|(_, part)| !part.is_empty())
}
fn token(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && !bytes
            .iter()
            .any(|b| b.is_ascii_whitespace() || b"[]\0".contains(b))
}
fn reserve<T>(v: &mut Vec<T>, additional: usize, offset: usize) -> Result<()> {
    v.try_reserve(additional).map_err(|_| limited(offset))
}
fn tag(kind: ValueType) -> u8 {
    match kind {
        ValueType::String => 1,
        ValueType::Int => 2,
        ValueType::Float => 3,
        ValueType::Byte => 4,
        ValueType::Word => 5,
        ValueType::Bool => 6,
        ValueType::Vec3 => 7,
        ValueType::Color => 8,
        ValueType::Raw(_) | ValueType::Mat3 => 9,
        ValueType::Vec2 | ValueType::Aabb | ValueType::RawFloat(_) => 16,
        ValueType::Enum => 17,
    }
}
