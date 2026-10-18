//! Bounded byte-buffer I/O with exact little-endian primitives and raw strings.
//!
//! [`Reader`] borrows input; [`Writer`] owns a finite output buffer. Exact helper
//! operations are atomic on error. Standard [`Read`] operations may consume a
//! partial input. Text decoding is strict; whitespace skipping requires
//! [`Reader::line_then_ignore`].
//!
//! ```
//! use std::io::SeekFrom;
//! use zenkit::io::{Reader, Writer, LineEnding, TextEncoding};
//! let mut output = Writer::new(128);
//! output.u32(0, "length")?;
//! output.line(b"raw\xff", LineEnding::CrLf, 16, "name")?;
//! let end = output.position();
//! output.seek_to(SeekFrom::Start(0), "patch")?;
//! output.u32((end - 4) as u32, "length")?;
//! let mut input = Reader::new(output.as_ref());
//! let length = input.u32("length")? as usize;
//! let mut chunk = input.substream(length, "chunk")?;
//! assert_eq!(chunk.line(LineEnding::CrLf, 16, "name")?, b"raw\xff");
//! assert_eq!(chunk.position(), end);
//! let mut text = Reader::new(b"ZenKit");
//! assert_eq!(text.text(6, 16, TextEncoding::Ascii, "name")?, "ZenKit");
//! # Ok::<(), zenkit::Error>(())
//! ```
// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2022-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use crate::{Error, ErrorKind, Result};
use std::{
    fmt,
    io::{self, BufRead, Read, Seek, SeekFrom, Write},
};
pub(crate) mod names;
pub use names::{HostNameCodec, HostNameEncoding};

/// Exact byte sequence ending a line. No automatic newline normalization occurs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LineEnding {
    /// One LF byte. A preceding CR remains part of the payload.
    Lf,
    /// A CR followed by LF. A lone LF is payload, not an ending.
    CrLf,
}
impl LineEnding {
    fn bytes(self) -> &'static [u8] {
        match self {
            Self::Lf => b"\n",
            Self::CrLf => b"\r\n",
        }
    }
}

/// Strict decoding policies. Legacy data can always remain raw byte slices.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextEncoding {
    /// Valid UTF-8 only; invalid or incomplete sequences are rejected.
    Utf8,
    /// Bytes in 0..=127 only; controls and NUL are preserved.
    Ascii,
}

/// A borrowed, bounded reader with absolute diagnostic offsets.
///
/// Exact helpers leave the cursor unchanged on failure and return slices tied to
/// the original input lifetime. [`Reader::position`] is absolute in the original
/// input; [`Seek`] and [`Reader::seek_to`] use positions relative to this reader's
/// own substream. Seeking outside 0..=length is rejected, including past EOF.
/// No input allocation occurs; apply [`read_bounded`] before loading a file.
#[derive(Clone)]
pub struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
    base: usize,
}

macro_rules! read_primitive {
    ($name:ident, $ty:ty, $size:literal) => {
        #[doc = concat!("Read an exact little-endian `", stringify!($ty), "`. Short input fails without advancing.")]
        pub fn $name(&mut self, context: &'static str) -> Result<$ty> {
            let mut data = [0; $size];
            data.copy_from_slice(self.take_bytes($size, context)?);
            Ok(<$ty>::from_le_bytes(data))
        }
    }
}

impl<'a> Reader<'a> {
    /// Borrow the complete input with initial position zero.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            position: 0,
            base: 0,
        }
    }
    pub(crate) fn with_origin(bytes: &'a [u8], base: usize) -> Result<Self> {
        base.checked_add(bytes.len())
            .ok_or_else(|| Error::operation(ErrorKind::InvalidData, "input origin"))?;
        Ok(Self {
            bytes,
            position: 0,
            base,
        })
    }
    pub(crate) fn view(&self) -> (&'a [u8], usize) {
        (self.bytes, self.base)
    }
    /// Current absolute byte offset, including nested substream bases.
    pub fn position(&self) -> usize {
        self.base + self.position
    }
    /// Current byte offset relative to this reader's start, as used by [`Seek`].
    pub fn relative_position(&self) -> usize {
        self.position
    }
    /// Number of bytes remaining in this reader's bounded input.
    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }
    /// Internal provenance for a borrowed payload; unrelated buffers have no offset.
    pub(crate) fn offset_of(&self, bytes: &[u8]) -> Option<usize> {
        let offset = bytes
            .as_ptr()
            .addr()
            .checked_sub(self.bytes.as_ptr().addr())?;
        (offset.checked_add(bytes.len())? <= self.bytes.len()).then(|| self.base + offset)
    }
    /// Read exactly `length` raw bytes, with no allocation or text decoding.
    ///
    /// Overflow and short input fail without advancing. `context` labels the
    /// failed field in the error; the offset points to its start.
    pub fn take_bytes(&mut self, length: usize, context: &'static str) -> Result<&'a [u8]> {
        let start = self.position;
        let end = start
            .checked_add(length)
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, self.position(), context))?;
        let bytes = self
            .bytes
            .get(start..end)
            .ok_or_else(|| Error::new(ErrorKind::UnexpectedEof, self.position(), context))?;
        self.position = end;
        Ok(bytes)
    }
    /// Read an exact raw byte string, rejecting a length greater than `limit`.
    /// The limit counts payload bytes; errors leave the cursor unchanged.
    pub fn fixed_bytes(
        &mut self,
        length: usize,
        limit: usize,
        context: &'static str,
    ) -> Result<&'a [u8]> {
        if length > limit {
            return Err(Error::new(
                ErrorKind::LimitExceeded,
                self.position(),
                context,
            ));
        }
        self.take_bytes(length, context)
    }
    /// Read an exact string and decode strictly using the explicit policy.
    ///
    /// Length is measured in bytes. Errors leave the cursor unchanged; a decoding
    /// error points to the first invalid byte (or start of an incomplete UTF-8
    /// sequence). Returned text borrows input, without allocation or replacement.
    pub fn text(
        &mut self,
        length: usize,
        limit: usize,
        encoding: TextEncoding,
        context: &'static str,
    ) -> Result<&'a str> {
        let mut candidate = self.clone();
        let offset = self.position();
        let bytes = candidate.fixed_bytes(length, limit, context)?;
        if encoding == TextEncoding::Ascii
            && let Some(index) = bytes.iter().position(|b| !b.is_ascii())
        {
            return Err(Error::new(ErrorKind::InvalidData, offset + index, context));
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|e| Error::new(ErrorKind::InvalidData, offset + e.valid_up_to(), context))?;
        self.position = candidate.position;
        Ok(text)
    }
    /// Read raw bytes through a required delimiter, excluding it from the result.
    ///
    /// `limit` excludes the delimiter, which is consumed on success. At most
    /// `limit + 1` bytes are inspected. EOF before a delimiter within that window
    /// gives `UnexpectedEof`; a non-delimiter beyond `limit` gives `LimitExceeded`.
    /// Either failure leaves the cursor unchanged. Zero permits an empty string.
    pub fn terminated(
        &mut self,
        delimiter: u8,
        limit: usize,
        context: &'static str,
    ) -> Result<&'a [u8]> {
        self.until_sequence(&[delimiter], limit, context)
    }
    /// Read a raw line with the exact required ending, excluding the ending.
    ///
    /// `limit` excludes the ending; whitespace, embedded NULs and other newline
    /// bytes are preserved. EOF without that ending is an error, including empty
    /// input. At most `limit + ending length` bytes are inspected. On failure the
    /// cursor is unchanged.
    pub fn line(
        &mut self,
        ending: LineEnding,
        limit: usize,
        context: &'static str,
    ) -> Result<&'a [u8]> {
        self.until_sequence(ending.bytes(), limit, context)
    }
    /// Read through the first NUL, CR, LF or bounded EOF, then optionally skip bytes.
    ///
    /// This is the legacy **borrowed-memory** line convention. The returned raw
    /// slice excludes the first terminator, which is consumed. EOF without a
    /// terminator succeeds; empty input returns an empty slice without advancing.
    /// Check [`Self::remaining`] when looping, since repeated EOF calls also succeed.
    /// A NUL terminator disables skipping. After CR/LF, only bytes present in
    /// `ignored_after_newline` are skipped, without trimming the returned payload.
    /// An empty set stops immediately after the first terminator, so CRLF consumes
    /// only CR unless LF is selected for skipping.
    ///
    /// A NUL encountered during skipping always stops it, even if selected. A
    /// nonterminal NUL stays unread; a NUL at the exact end of this reader's
    /// bounded input is consumed. Use [`Self::line`] or [`Self::terminated`] for
    /// an exact delimiter without skipping.
    ///
    /// `payload_limit` counts returned bytes. `ignored_limit` counts every byte
    /// consumed after the first CR/LF, including a terminal NUL. The first
    /// terminator counts against neither limit. Exceeding either limit returns
    /// `LimitExceeded` at the line's absolute start and leaves the cursor unchanged.
    /// At most `payload_limit + 1` payload/terminator bytes and `ignored_limit + 1`
    /// suffix bytes are inspected. No allocation occurs. A zero limit allows an
    /// empty payload or no consumed suffix.
    ///
    /// ```
    /// use zenkit::io::Reader;
    /// let mut input = Reader::new(b" name\r\n \tnext\0 tail");
    /// assert_eq!(input.line_then_ignore(b" \t\r\n", 5, 3, "name")?, b" name");
    /// assert_eq!(input.line_then_ignore(b" \t\r\n", 4, 0, "next")?, b"next");
    /// assert_eq!(input.take_bytes(5, "suffix")?, b" tail");
    /// # Ok::<(), zenkit::Error>(())
    /// ```
    pub fn line_then_ignore(
        &mut self,
        ignored_after_newline: &[u8],
        payload_limit: usize,
        ignored_limit: usize,
        context: &'static str,
    ) -> Result<&'a [u8]> {
        let start = self.position;
        let rest = &self.bytes[start..];
        let inspected = rest.len().min(payload_limit.saturating_add(1));
        let length = rest[..inspected]
            .iter()
            .position(|byte| matches!(byte, 0 | b'\r' | b'\n'))
            .unwrap_or(inspected);
        let limited = || Error::new(ErrorKind::LimitExceeded, self.base + start, context);
        if length > payload_limit {
            return Err(limited());
        }
        let mut consumed = length;
        if let Some(&terminator) = rest.get(length) {
            consumed += 1;
            if terminator != 0 && !ignored_after_newline.is_empty() {
                let mut ignored = [false; 256];
                for &byte in ignored_after_newline {
                    ignored[usize::from(byte)] = true;
                }
                let suffix_start = consumed;
                while let Some(&byte) = rest.get(consumed) {
                    // Lookahead leaves an internal NUL unread but
                    // consumes it at bounded EOF. NUL never joins an ignored run.
                    let consume = if byte == 0 {
                        consumed == rest.len() - 1
                    } else {
                        ignored[usize::from(byte)]
                    };
                    if !consume {
                        break;
                    }
                    if consumed - suffix_start == ignored_limit {
                        return Err(limited());
                    }
                    consumed += 1;
                }
            }
        }
        self.position += consumed;
        Ok(&rest[..length])
    }
    fn until_sequence(
        &mut self,
        ending: &[u8],
        limit: usize,
        context: &'static str,
    ) -> Result<&'a [u8]> {
        let rest = &self.bytes[self.position..];
        let inspected = rest.len().min(limit.saturating_add(ending.len()));
        if let Some(length) = rest[..inspected]
            .windows(ending.len())
            .position(|v| v == ending)
        {
            let start = self.position;
            self.position += length + ending.len();
            return Ok(&self.bytes[start..start + length]);
        }
        // A suffix that can still become the ending is not proven payload.
        let partial = (1..ending.len())
            .rev()
            .find(|&n| rest[..inspected].ends_with(&ending[..n]))
            .unwrap_or(0);
        let kind = if inspected - partial > limit {
            ErrorKind::LimitExceeded
        } else {
            ErrorKind::UnexpectedEof
        };
        Err(Error::new(kind, self.position(), context))
    }
    read_primitive!(u8, u8, 1);
    read_primitive!(i8, i8, 1);
    read_primitive!(u16, u16, 2);
    read_primitive!(i16, i16, 2);
    read_primitive!(u32, u32, 4);
    read_primitive!(i32, i32, 4);
    read_primitive!(u64, u64, 8);
    read_primitive!(i64, i64, 8);
    read_primitive!(f32, f32, 4);
    read_primitive!(f64, f64, 8);
    /// Advance the parent by `length` and return an independently positioned child.
    /// Child errors retain absolute offsets. Failure leaves the parent unchanged.
    pub fn substream(&mut self, length: usize, context: &'static str) -> Result<Reader<'a>> {
        let base = self.position();
        Ok(Self {
            bytes: self.take_bytes(length, context)?,
            position: 0,
            base,
        })
    }
    /// Seek within this reader, returning its new *relative* position.
    /// Invalid positions fail without advancing; errors report the old absolute offset.
    pub fn seek_to(&mut self, from: SeekFrom, context: &'static str) -> Result<usize> {
        let position = checked_seek(self.position, self.bytes.len(), from)
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, self.position(), context))?;
        self.position = position;
        Ok(position)
    }
}
impl fmt::Debug for Reader<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reader")
            .field("length", &self.bytes.len())
            .field("position", &self.position)
            .field("base", &self.base)
            .finish()
    }
}
impl Read for Reader<'_> {
    /// Read up to the remaining byte count. Unlike exact helpers, short reads succeed.
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = buffer.len().min(self.remaining());
        buffer[..count].copy_from_slice(&self.bytes[self.position..self.position + count]);
        self.position += count;
        Ok(count)
    }
}
impl BufRead for Reader<'_> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        Ok(&self.bytes[self.position..])
    }
    fn consume(&mut self, amount: usize) {
        self.position += amount.min(self.remaining());
    }
}
impl Seek for Reader<'_> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.seek_to(from, "seek input")
            .map(|v| v as u64)
            .map_err(std_error)
    }
}

/// An owned byte writer with atomic writes and an explicit finite output limit.
///
/// Supports patching existing bytes; seeks past the end are rejected. Limits
/// count output length, excluding allocator overhead and spare capacity.
/// [`Write`] accepts the whole slice or fails unchanged; compound operations such
/// as `write_fmt` may commit fragments before failing. External writes after
/// [`Writer::into_inner`] may also fail partially. Errors have no input offset.
/// Default limit: 256 MiB. [`Debug`] never prints payload bytes.
#[derive(Clone)]
pub struct Writer {
    bytes: Vec<u8>,
    position: usize,
    limit: usize,
}
macro_rules! write_primitive {
    ($name:ident, $ty:ty) => {
        #[doc = concat!("Write the exact little-endian representation of `", stringify!($ty), "`, atomically on error.")]
        pub fn $name(&mut self, value: $ty, context: &'static str) -> Result<()> { self.bytes(&value.to_le_bytes(), context) }
    }
}
impl Writer {
    /// Default maximum logical output size: 256 MiB.
    pub const DEFAULT_LIMIT: usize = 256 * 1024 * 1024;
    /// Start an empty writer with `limit` maximum logical output bytes. Zero is valid.
    pub fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            position: 0,
            limit,
        }
    }
    /// Current output byte offset.
    pub fn position(&self) -> usize {
        self.position
    }
    /// Configured maximum logical output size.
    pub fn limit(&self) -> usize {
        self.limit
    }
    /// Consume the writer and return its complete output, regardless of cursor position.
    pub fn into_inner(self) -> Vec<u8> {
        self.bytes
    }
    /// Borrow complete output, including bytes after the current cursor.
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }
    /// Write raw bytes at the cursor, overwriting or extending the buffer.
    /// Overflow, resource limit or reservation failure leaves bytes and cursor unchanged.
    pub fn bytes(&mut self, bytes: &[u8], context: &'static str) -> Result<()> {
        self.parts(bytes, &[], context)
    }
    fn parts(&mut self, payload: &[u8], suffix: &[u8], context: &'static str) -> Result<()> {
        let end = self
            .position
            .checked_add(payload.len())
            .and_then(|v| v.checked_add(suffix.len()))
            .filter(|&v| v <= self.limit)
            .ok_or_else(|| Error::operation(ErrorKind::LimitExceeded, context))?;
        let extra = end.saturating_sub(self.bytes.len());
        self.bytes
            .try_reserve(extra)
            .map_err(|_| Error::operation(ErrorKind::LimitExceeded, context))?;
        if end > self.bytes.len() {
            self.bytes.resize(end, 0);
        }
        let middle = self.position + payload.len();
        self.bytes[self.position..middle].copy_from_slice(payload);
        self.bytes[middle..end].copy_from_slice(suffix);
        self.position = end;
        Ok(())
    }
    /// Write payload plus delimiter, rejecting embedded delimiters or payload over `limit`.
    /// The writer's output limit includes the delimiter. Failure is atomic.
    pub fn terminated(
        &mut self,
        bytes: &[u8],
        delimiter: u8,
        limit: usize,
        context: &'static str,
    ) -> Result<()> {
        self.string(bytes, &[delimiter], limit, context)
    }
    /// Write payload plus the chosen exact line ending.
    ///
    /// Rejects the chosen ending within the payload and payload lengths over
    /// `limit`. Other newline bytes, whitespace and NULs are preserved.
    /// Failure leaves output and cursor unchanged.
    pub fn line(
        &mut self,
        bytes: &[u8],
        ending: LineEnding,
        limit: usize,
        context: &'static str,
    ) -> Result<()> {
        self.string(bytes, ending.bytes(), limit, context)
    }
    fn string(
        &mut self,
        bytes: &[u8],
        ending: &[u8],
        limit: usize,
        context: &'static str,
    ) -> Result<()> {
        if bytes.len() > limit {
            return Err(Error::operation(ErrorKind::LimitExceeded, context));
        }
        if bytes.windows(ending.len()).any(|v| v == ending) {
            return Err(Error::operation(ErrorKind::InvalidData, context));
        }
        self.parts(bytes, ending, context)
    }
    write_primitive!(u8, u8);
    write_primitive!(i8, i8);
    write_primitive!(u16, u16);
    write_primitive!(i16, i16);
    write_primitive!(u32, u32);
    write_primitive!(i32, i32);
    write_primitive!(u64, u64);
    write_primitive!(i64, i64);
    write_primitive!(f32, f32);
    write_primitive!(f64, f64);
    /// Seek within existing output, returning its new position. Failure is atomic.
    pub fn seek_to(&mut self, from: SeekFrom, context: &'static str) -> Result<usize> {
        let position = checked_seek(self.position, self.bytes.len(), from)
            .ok_or_else(|| Error::operation(ErrorKind::InvalidData, context))?;
        self.position = position;
        Ok(position)
    }
}
impl Default for Writer {
    fn default() -> Self {
        Self::new(Self::DEFAULT_LIMIT)
    }
}
impl AsRef<[u8]> for Writer {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}
impl fmt::Debug for Writer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Writer")
            .field("length", &self.bytes.len())
            .field("position", &self.position)
            .field("limit", &self.limit)
            .finish()
    }
}
impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes(bytes, "write output")
            .map(|()| bytes.len())
            .map_err(std_error)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Seek for Writer {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.seek_to(from, "seek output")
            .map(|v| v as u64)
            .map_err(std_error)
    }
}
fn checked_seek(position: usize, length: usize, from: SeekFrom) -> Option<usize> {
    let target = match from {
        SeekFrom::Start(value) => i128::from(value),
        SeekFrom::Current(value) => position as i128 + i128::from(value),
        SeekFrom::End(value) => length as i128 + i128::from(value),
    };
    usize::try_from(target).ok().filter(|&v| v <= length)
}
fn std_error(error: Error) -> io::Error {
    let kind = match error.kind {
        ErrorKind::UnexpectedEof => io::ErrorKind::UnexpectedEof,
        ErrorKind::InvalidData => io::ErrorKind::InvalidInput,
        ErrorKind::LimitExceeded => io::ErrorKind::Other,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, error)
}

/// Read without trusting a file's reported size; an extra byte detects excess input.
///
/// The input may be partially consumed on failure, as with ordinary [`Read`].
/// Short reads are retried; I/O errors retain their source. The limit counts
/// input bytes, excluding allocator overhead. Reads start at the current device
/// position; the resulting buffer is independent of the device.
///
/// For framed input, wrap the borrowed device in [`Read::take`] with the known
/// frame length to preserve its suffix. Without that wrapper, detecting an excess
/// consumes up to `limit + 1` bytes. A short frame is still ordinary EOF: validate
/// the required frame length or parse its exact fields separately.
///
/// ```
/// use std::io::{Cursor, Read, Seek, SeekFrom};
/// use zenkit::io::{read_bounded, Reader};
/// let mut device = Cursor::new(b"xx\x78\x56\x34\x12suffix");
/// device.seek(SeekFrom::Start(2))?;
/// let owned = read_bounded((&mut device).take(4), 4)?;
/// assert_eq!(Reader::new(&owned).u32("value")?, 0x12345678);
/// assert_eq!(device.position(), 6);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// A borrowed reader cannot outlive its snapshot:
///
/// ```compile_fail
/// use zenkit::io::Reader;
/// fn dangling() -> Reader<'static> {
///     let owned = vec![1, 2, 3];
///     Reader::new(&owned)
/// }
/// ```
pub fn read_bounded(input: impl Read, limit: usize) -> Result<Vec<u8>> {
    read_bounded_hint(input, limit, 0)
}

// A regular file's size is an allocation hint, never a trusted extent. Keep the
// same limit+1 read and actual-length check when a file grows/shrinks after stat.
pub(crate) fn read_bounded_hint(
    input: impl Read,
    limit: usize,
    size_hint: usize,
) -> Result<Vec<u8>> {
    let cap = u64::try_from(limit)
        .ok()
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| Error::operation(ErrorKind::LimitExceeded, "input limit"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size_hint.min(limit.saturating_add(1)))
        .map_err(|_| Error::operation(ErrorKind::LimitExceeded, "input allocation"))?;
    input
        .take(cap)
        .read_to_end(&mut bytes)
        .map_err(|e| Error::io(e, "read input"))?;
    if bytes.len() > limit {
        return Err(Error::operation(ErrorKind::LimitExceeded, "input bytes"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod snapshot_hint_tests {
    use super::*;

    #[test]
    fn stale_or_large_hints_neither_truncate_input_nor_relax_the_byte_limit() {
        for hint in [0, 1, 4, 32, usize::MAX] {
            for limit in [0, 3, 4, 8] {
                let mut source = io::Cursor::new(b"data");
                let result = read_bounded_hint(&mut source, limit, hint);
                assert_eq!(source.position(), 4.min(limit as u64 + 1));
                if limit >= 4 {
                    assert_eq!(result.unwrap(), b"data");
                } else {
                    let error = result.unwrap_err();
                    assert_eq!(error.kind, ErrorKind::LimitExceeded);
                    assert_eq!(error.offset, None);
                }
            }
        }
    }

    #[test]
    fn hinted_snapshots_preserve_io_failures_and_reject_unrepresentable_limits() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "reader"))
            }
        }
        let error = read_bounded_hint(Broken, 8, usize::MAX).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Io);
        assert_eq!(error.offset, None);
        assert!(std::error::Error::source(&error).is_some());
        if usize::BITS == 64 {
            let error = read_bounded_hint(Broken, usize::MAX, 1).unwrap_err();
            assert_eq!(error.kind, ErrorKind::LimitExceeded);
        }
    }
}
