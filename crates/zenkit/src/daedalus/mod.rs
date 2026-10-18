//! Immutable Daedalus DAT storage and checked symbol/bytecode introspection.
//!
//! Loading preserves the version, sort table, every symbol word, string delimiter
//! and bytecode byte. It does not execute code or infer runtime initial values for
//! function/instance variables. Malformed relationships, sort entries and code are
//! inspectable until a corresponding checked query is requested. Unknown symbol
//! types fail because their payload extent is unknown.
//!
//! Names remain bytes. [`Script::find_raw_name`] is exact; [`Script::find_name`]
//! folds ASCII on both sides and maps a leading generated-name FF byte to `$`.
//! Both queries return the last matching symbol;
//! the matching iterators retain all duplicates in original symbol order.
//!
//! Numeric initializers are endian-aware borrowing iterators, not references to
//! possibly unaligned native integer/float arrays. Float bits remain exact;
//! [`SymbolRef::float_at`] provides the corresponding f32 value explicitly.
//!
//! Raw instruction decoding is distinct from a checked linear instruction index
//! and from executable validation. None of these APIs is a VM or a proof of stack
//! types, stack depth, termination, host bindings or arithmetic safety.
// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT

pub mod addon;
mod inspect;
mod instruction;
mod read;
pub mod vm;

pub use inspect::Instances;
pub use instruction::{
    DecodeLimits, DecodedCode, Executable, Instruction, Instructions, Opcode, Operand,
};

use crate::{Error, ErrorKind, Result};
use std::{
    fmt,
    iter::FusedIterator,
    ops::Range,
    sync::atomic::{AtomicU64, Ordering},
};

const CTX: &str = "Daedalus DAT";
static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
fn at(kind: ErrorKind, offset: usize) -> Error {
    Error::new(kind, offset, CTX)
}
fn bad(kind: ErrorKind) -> Error {
    Error::operation(kind, CTX)
}
fn reserve<T>(v: &mut Vec<T>, count: usize, offset: usize) -> Result<()> {
    v.try_reserve(count)
        .map_err(|_| at(ErrorKind::LimitExceeded, offset))
}

/// Finite logical storage limits; capacities and allocator overhead are not quotas.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Maximum consumed DAT extent. A prefix read may leave a larger suffix.
    pub bytes: usize,
    /// Retained original DAT bytes (one complete copy, including metadata and code).
    pub copied_bytes: usize,
    /// Symbol descriptors and sort-table entries, each bounded by this count.
    pub symbols: usize,
    /// Aggregate nonmember initializer elements; class offsets/addresses count one.
    pub values: usize,
    /// Maximum raw byte length of each name or string, excluding its delimiter.
    pub string_bytes: usize,
    /// Bytecode payload length.
    pub code_bytes: usize,
    /// Aggregate raw-name, folded-name and address lookup index entries.
    pub index_items: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            bytes: 128 * 1024 * 1024,
            copied_bytes: 128 * 1024 * 1024,
            symbols: 1_000_000,
            values: 4_000_000,
            string_bytes: 1024 * 1024,
            code_bytes: 64 * 1024 * 1024,
            index_items: 3_000_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(u8)]
pub enum DataType {
    Void = 0,
    Float = 1,
    Int = 2,
    String = 3,
    Class = 4,
    Function = 5,
    Prototype = 6,
    Instance = 7,
}
impl DataType {
    pub fn from_raw(raw: u32) -> Option<Self> {
        Some(match raw {
            0 => Self::Void,
            1 => Self::Float,
            2 => Self::Int,
            3 => Self::String,
            4 => Self::Class,
            5 => Self::Function,
            6 => Self::Prototype,
            7 => Self::Instance,
            _ => return None,
        })
    }
    pub fn raw(self) -> u32 {
        self as u32
    }
}

/// The five stored source words, preserved without masking.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceLocation {
    words: [u32; 5],
}
impl SourceLocation {
    pub fn raw_words(self) -> [u32; 5] {
        self.words
    }
    pub fn file_index(self) -> u32 {
        self.words[0] & 0x7ffff
    }
    pub fn line_start(self) -> u32 {
        self.words[1] & 0x7ffff
    }
    pub fn line_count(self) -> u32 {
        self.words[2] & 0x7ffff
    }
    pub fn char_start(self) -> u32 {
        self.words[3] & 0xffffff
    }
    pub fn char_count(self) -> u32 {
        self.words[4] & 0xffffff
    }
}

/// Script-owned identity; moving a Script preserves it, another Script rejects it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SymbolId {
    owner: u64,
    index: u32,
}
impl SymbolId {
    pub fn index(self) -> u32 {
        self.index
    }
}

#[derive(Clone, Copy)]
struct Span {
    start: usize,
    end: usize,
}
impl Span {
    fn range(self) -> Range<usize> {
        self.start..self.end
    }
}
struct Symbol {
    offset: usize,
    name_marker: u32,
    name: Option<Span>,
    vary: u32,
    properties: u32,
    source: SourceLocation,
    data: Span,
    strings: Range<usize>,
    parent: i32,
    parent_offset: usize,
}
impl Symbol {
    fn data_type(&self) -> DataType {
        DataType::from_raw((self.properties >> 12) & 15).unwrap()
    }
    fn count(&self) -> usize {
        (self.properties & 0xfff) as usize
    }
    fn member(&self) -> bool {
        self.properties & (4 << 16) != 0
    }
    fn constant(&self) -> bool {
        self.properties & (1 << 16) != 0
    }
}

/// One immutable DAT document. No Clone duplicates its owner identity.
pub struct Script {
    owner: u64,
    offset: usize,
    bytes: Vec<u8>,
    version: u8,
    sort: Vec<u32>,
    symbols: Vec<Symbol>,
    strings: Vec<Span>,
    code: Span,
    raw_names: Vec<u32>,
    names: Vec<u32>,
    addresses: Vec<u32>,
}
impl fmt::Debug for Script {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Script")
            .field("version", &self.version)
            .field("symbols", &self.symbols.len())
            .field("bytes", &self.bytes.len())
            .field("code_bytes", &self.code().len())
            .finish()
    }
}
impl Script {
    pub fn version(&self) -> u8 {
        self.version
    }
    /// Absolute offset at which this document began in its input Reader.
    pub fn offset(&self) -> usize {
        self.offset
    }
    pub fn byte_len(&self) -> usize {
        self.bytes.len()
    }
    pub fn original_bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn sort_table(&self) -> &[u32] {
        &self.sort
    }
    pub fn code(&self) -> &[u8] {
        &self.bytes[self.code.range()]
    }
    pub fn code_offset(&self) -> usize {
        self.offset + self.code.start
    }
    pub fn len(&self) -> usize {
        self.symbols.len()
    }
    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }
    pub fn symbol(&self, index: u32) -> Option<SymbolRef<'_>> {
        ((index as usize) < self.len()).then_some(SymbolRef {
            script: self,
            index,
        })
    }
    pub fn get(&self, id: SymbolId) -> Result<SymbolRef<'_>> {
        if id.owner != self.owner {
            return Err(bad(ErrorKind::InvalidHandle));
        }
        self.symbol(id.index)
            .ok_or_else(|| bad(ErrorKind::InvalidHandle))
    }
    pub fn symbols(
        &self,
    ) -> impl ExactSizeIterator<Item = SymbolRef<'_>> + DoubleEndedIterator + FusedIterator {
        (0..self.len() as u32).map(|index| SymbolRef {
            script: self,
            index,
        })
    }
    fn string(&self, span: Span) -> DatString<'_> {
        DatString {
            raw: &self.bytes[span.range()],
            delimiter: self.bytes[span.end],
            offset: self.offset + span.start,
        }
    }
    fn words(&self, span: Span) -> Words<'_> {
        Words {
            chunks: self.bytes[span.range()].as_chunks::<4>().0.iter(),
        }
    }
}

/// Borrowed complete symbol view; storage is immutable and stays with its Script.
#[derive(Clone, Copy)]
pub struct SymbolRef<'a> {
    script: &'a Script,
    index: u32,
}
impl fmt::Debug for SymbolRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SymbolRef")
            .field("index", &self.index)
            .field("data_type", &self.data_type())
            .field("count", &self.count())
            .field("name_bytes", &self.raw_name().map(|s| s.raw().len()))
            .finish()
    }
}
impl<'a> SymbolRef<'a> {
    fn inner(self) -> &'a Symbol {
        &self.script.symbols[self.index as usize]
    }
    pub fn id(self) -> SymbolId {
        SymbolId {
            owner: self.script.owner,
            index: self.index,
        }
    }
    pub fn index(self) -> u32 {
        self.index
    }
    pub fn offset(self) -> usize {
        self.script.offset + self.inner().offset
    }
    pub fn name_marker(self) -> u32 {
        self.inner().name_marker
    }
    pub fn raw_name(self) -> Option<DatString<'a>> {
        self.inner().name.map(|s| self.script.string(s))
    }
    /// Byte-preserving generated-name view, replacing only a first FF byte by `$`.
    /// Missing names and present empty names both have an empty logical view.
    pub fn logical_name(
        self,
    ) -> impl ExactSizeIterator<Item = u8> + DoubleEndedIterator + FusedIterator + 'a {
        self.raw_name()
            .map_or(&[][..], |s| s.raw())
            .iter()
            .enumerate()
            .map(|(i, &b)| if i == 0 && b == 255 { b'$' } else { b })
    }
    pub fn is_generated(self) -> bool {
        self.raw_name()
            .is_some_and(|s| s.raw().first() == Some(&255))
    }
    pub fn vary(self) -> u32 {
        self.inner().vary
    }
    pub fn properties(self) -> u32 {
        self.inner().properties
    }
    pub fn data_type(self) -> DataType {
        self.inner().data_type()
    }
    pub fn count(self) -> u16 {
        self.inner().count() as u16
    }
    /// All six serialized flag bits; runtime-only access-trap/local bits are absent.
    pub fn flags(self) -> u8 {
        ((self.properties() >> 16) & 0x3f) as u8
    }
    pub fn is_const(self) -> bool {
        self.inner().constant()
    }
    pub fn has_return(self) -> bool {
        self.flags() & 2 != 0
    }
    pub fn is_member(self) -> bool {
        self.inner().member()
    }
    pub fn is_external(self) -> bool {
        self.flags() & 8 != 0
    }
    pub fn is_merged(self) -> bool {
        self.flags() & 16 != 0
    }
    pub fn source_location(self) -> SourceLocation {
        self.inner().source
    }
    pub fn parent(self) -> i32 {
        self.inner().parent
    }
    pub fn parent_offset(self) -> usize {
        self.script.offset + self.inner().parent_offset
    }
    pub fn data_offset(self) -> usize {
        self.script.offset + self.inner().data.start
    }
    pub fn member_offset(self) -> Option<u32> {
        self.is_member().then_some(self.vary())
    }
    pub fn class_size(self) -> Option<u32> {
        (!self.is_member() && self.data_type() == DataType::Class).then_some(self.vary())
    }
    /// Raw return-type word; unknown values remain inspectable.
    pub fn return_type_raw(self) -> Option<u32> {
        (!self.is_member() && self.data_type() == DataType::Function).then_some(self.vary())
    }
    pub fn return_type(self) -> Result<DataType> {
        let raw = self
            .return_type_raw()
            .ok_or_else(|| at(ErrorKind::WrongObjectType, self.offset()))?;
        DataType::from_raw(raw).ok_or_else(|| {
            at(
                ErrorKind::Unsupported,
                self.offset()
                    + if let Some(s) = self.raw_name() {
                        5 + s.raw().len()
                    } else {
                        4
                    },
            )
        })
    }
    pub fn initial_value(self) -> InitialValue<'a> {
        let sym = self.inner();
        if self.is_member() {
            return InitialValue::None;
        }
        let words = || self.script.words(sym.data);
        match self.data_type() {
            DataType::Void => InitialValue::None,
            DataType::Float => InitialValue::FloatBits(words()),
            DataType::Int => InitialValue::Integers(IntValues(words())),
            DataType::String => InitialValue::Strings(Strings {
                script: self.script,
                spans: self.script.strings[sym.strings.clone()].iter(),
            }),
            DataType::Class => InitialValue::ClassOffset(words().next().unwrap() as i32),
            DataType::Function | DataType::Prototype | DataType::Instance => {
                InitialValue::Address(words().next().unwrap() as i32)
            }
        }
    }
    pub fn address(self) -> Option<i32> {
        match self.initial_value() {
            InitialValue::Address(n) => Some(n),
            _ => None,
        }
    }
    pub fn class_offset(self) -> Option<i32> {
        match self.initial_value() {
            InitialValue::ClassOffset(n) => Some(n),
            _ => None,
        }
    }
    pub fn int_at(self, index: usize) -> Result<i32> {
        match self.initial_value() {
            InitialValue::Integers(mut v) => v
                .nth(index)
                .ok_or_else(|| at(ErrorKind::InvalidData, self.data_offset())),
            _ => Err(at(ErrorKind::WrongObjectType, self.offset())),
        }
    }
    pub fn float_bits_at(self, index: usize) -> Result<u32> {
        match self.initial_value() {
            InitialValue::FloatBits(mut v) => v
                .nth(index)
                .ok_or_else(|| at(ErrorKind::InvalidData, self.data_offset())),
            _ => Err(at(ErrorKind::WrongObjectType, self.offset())),
        }
    }
    pub fn float_at(self, index: usize) -> Result<f32> {
        self.float_bits_at(index).map(f32::from_bits)
    }
    pub fn string_at(self, index: usize) -> Result<DatString<'a>> {
        match self.initial_value() {
            InitialValue::Strings(mut v) => v
                .nth(index)
                .ok_or_else(|| at(ErrorKind::InvalidData, self.data_offset())),
            _ => Err(at(ErrorKind::WrongObjectType, self.offset())),
        }
    }
}

/// Borrowed initializer values. Member fields have no serialized initializer;
/// FUNCTION/PROTOTYPE/INSTANCE expose the stored address, not VM runtime state.
#[derive(Clone)]
pub enum InitialValue<'a> {
    None,
    Integers(IntValues<'a>),
    FloatBits(Words<'a>),
    Strings(Strings<'a>),
    ClassOffset(i32),
    Address(i32),
}
impl fmt::Debug for InitialValue<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Integers(v) => f.debug_tuple("Integers").field(&v.len()).finish(),
            Self::FloatBits(v) => f.debug_tuple("FloatBits").field(&v.len()).finish(),
            Self::Strings(v) => f.debug_tuple("Strings").field(&v.len()).finish(),
            Self::ClassOffset(v) => f.debug_tuple("ClassOffset").field(v).finish(),
            Self::Address(v) => f.debug_tuple("Address").field(v).finish(),
        }
    }
}

/// Raw terminated string; delimiter is exactly one NUL, CR or LF byte, never CRLF.
#[derive(Clone, Copy)]
pub struct DatString<'a> {
    raw: &'a [u8],
    delimiter: u8,
    offset: usize,
}
impl<'a> DatString<'a> {
    pub fn raw(self) -> &'a [u8] {
        self.raw
    }
    pub fn delimiter(self) -> u8 {
        self.delimiter
    }
    pub fn offset(self) -> usize {
        self.offset
    }
    /// Replaces only backslash-n/backslash-t pairs. Unknown escapes and a final
    /// backslash remain literal; output bytes are not decoded as text.
    pub fn decoded(self) -> DecodedString<'a> {
        DecodedString {
            bytes: self.raw.iter().copied(),
        }
    }
}
impl fmt::Debug for DatString<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DatString")
            .field("bytes", &self.raw.len())
            .field("delimiter", &self.delimiter)
            .field("offset", &self.offset)
            .finish()
    }
}
#[derive(Clone)]
pub struct DecodedString<'a> {
    bytes: std::iter::Copied<std::slice::Iter<'a, u8>>,
}
impl Iterator for DecodedString<'_> {
    type Item = u8;
    fn next(&mut self) -> Option<u8> {
        let b = self.bytes.next()?;
        if b == b'\\' {
            match self.bytes.clone().next() {
                Some(b'n') => {
                    self.bytes.next();
                    return Some(b'\n');
                }
                Some(b't') => {
                    self.bytes.next();
                    return Some(b'\t');
                }
                _ => {}
            }
        }
        Some(b)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.bytes.len().div_ceil(2), Some(self.bytes.len()))
    }
}
impl FusedIterator for DecodedString<'_> {}
/// Exact little-endian four-byte values, loaded without alignment assumptions.
#[derive(Clone)]
pub struct Words<'a> {
    chunks: std::slice::Iter<'a, [u8; 4]>,
}
impl Iterator for Words<'_> {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        self.chunks.next().map(|b| u32::from_le_bytes(*b))
    }
    fn nth(&mut self, n: usize) -> Option<u32> {
        self.chunks.nth(n).map(|b| u32::from_le_bytes(*b))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.chunks.size_hint()
    }
}
impl DoubleEndedIterator for Words<'_> {
    fn next_back(&mut self) -> Option<u32> {
        self.chunks.next_back().map(|b| u32::from_le_bytes(*b))
    }
}
impl ExactSizeIterator for Words<'_> {}
impl FusedIterator for Words<'_> {}
#[derive(Clone)]
pub struct IntValues<'a>(Words<'a>);
impl Iterator for IntValues<'_> {
    type Item = i32;
    fn next(&mut self) -> Option<i32> {
        self.0.next().map(|n| n as i32)
    }
    fn nth(&mut self, n: usize) -> Option<i32> {
        self.0.nth(n).map(|n| n as i32)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}
impl DoubleEndedIterator for IntValues<'_> {
    fn next_back(&mut self) -> Option<i32> {
        self.0.next_back().map(|n| n as i32)
    }
}
impl ExactSizeIterator for IntValues<'_> {}
impl FusedIterator for IntValues<'_> {}
#[derive(Clone)]
pub struct Strings<'a> {
    script: &'a Script,
    spans: std::slice::Iter<'a, Span>,
}
impl<'a> Iterator for Strings<'a> {
    type Item = DatString<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        self.spans.next().map(|s| self.script.string(*s))
    }
    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        self.spans.nth(n).map(|s| self.script.string(*s))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.spans.size_hint()
    }
}
impl DoubleEndedIterator for Strings<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.spans.next_back().map(|s| self.script.string(*s))
    }
}
impl ExactSizeIterator for Strings<'_> {}
impl FusedIterator for Strings<'_> {}
