// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;
use crate::io::{LineEnding, Writer as Bytes};
use std::{collections::HashMap, io::SeekFrom};

/// An owned writer requiring fallible finalization through [`Writer::finish`].
/// Record operations leave output/state unchanged on error. Finalization consumes
/// the writer and patches counts, lengths and the table. Pending BIN_SAFE table
/// bytes count against the output budget. Output is independent of the clock,
/// environment, locale and filesystem metadata.
///
/// ASCII uses reader-compatible `int` for byte/word, hex `raw` for matrices,
/// and Rust's round-trip float formatting. NaN payloads cannot survive decimal
/// text; use a raw field when individual NaN bits matter. BINARY enums above
/// 255 are rejected. BIN_SAFE byte strings may contain any byte; BINARY strings
/// cannot contain NUL, and ASCII strings cannot contain CR/LF/NUL.
pub struct Writer {
    output: Bytes,
    format: Format,
    limits: Limits,
    count: u32,
    count_at: usize,
    table_at: Option<usize>,
    pending_table_bytes: usize,
    frames: Vec<usize>,
    records: usize,
    keys: HashMap<Vec<u8>, u32>,
    save_game: bool,
    wide_g1: bool,
    metadata_bytes: usize,
}
impl std::fmt::Debug for Writer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Writer")
            .field("format", &self.format)
            .field("bytes", &self.output.as_ref().len())
            .field("depth", &self.frames.len())
            .field("objects", &self.count)
            .field("keys", &self.keys.len())
            .finish_non_exhaustive()
    }
}
fn clean_line(v: &[u8], limit: usize) -> Result<()> {
    if v.len() > limit {
        return Err(output_error(ErrorKind::LimitExceeded));
    }
    if v.iter().any(|b| b"\r\n\0".contains(b)) {
        return Err(output_error(ErrorKind::InvalidData));
    }
    Ok(())
}
fn out_line(w: &mut Bytes, v: &[u8], limit: usize) -> Result<()> {
    clean_line(v, limit)?;
    w.line(v, LineEnding::Lf, limit, CTX)
}
fn out_text(w: &mut Bytes, v: impl std::fmt::Display) -> Result<()> {
    use std::io::Write;
    write!(w, "{v}").map_err(|_| output_error(ErrorKind::LimitExceeded))
}
impl Writer {
    pub fn new(header: Header<'_>, limits: Limits) -> Result<Self> {
        if header.version != 1 {
            return Err(output_error(ErrorKind::Unsupported));
        }
        if header.archiver.is_empty() {
            return Err(output_error(ErrorKind::InvalidData));
        }
        let mut output = Bytes::new(limits.bytes);
        out_line(&mut output, b"ZenGin Archive", limits.field_bytes)?;
        out_line(&mut output, b"ver 1", limits.field_bytes)?;
        out_line(&mut output, header.archiver, limits.field_bytes)?;
        out_line(
            &mut output,
            match header.format {
                Format::Ascii => b"ASCII",
                Format::Binary => b"BINARY",
                Format::BinSafe => b"BIN_SAFE",
            },
            limits.field_bytes,
        )?;
        out_line(
            &mut output,
            if header.save_game {
                b"saveGame 1"
            } else {
                b"saveGame 0"
            },
            limits.field_bytes,
        )?;
        for (prefix, value) in [
            (b"date ".as_slice(), header.date),
            (b"user ".as_slice(), header.user),
        ] {
            if let Some(value) = value {
                clean_line(value, limits.field_bytes.saturating_sub(prefix.len()))?;
                output.bytes(prefix, CTX)?;
                out_line(&mut output, value, limits.field_bytes)?;
            }
        }
        out_line(&mut output, b"END", limits.field_bytes)?;
        let count_at;
        let table_at;
        if header.format == Format::BinSafe {
            output.u32(2, CTX)?;
            count_at = output.position();
            output.u32(0, CTX)?;
            table_at = Some(output.position());
            output.u32(0, CTX)?;
        } else {
            clean_line(b"objects 0000000000", limits.field_bytes)?;
            output.bytes(b"objects ", CTX)?;
            count_at = output.position();
            output.bytes(b"0000000000\nEND\n\n", CTX)?;
            table_at = None;
        }
        let pending_table_bytes = if header.format == Format::BinSafe {
            4
        } else {
            0
        };
        if pending_table_bytes > limits.bytes - output.as_ref().len() {
            return Err(output_error(ErrorKind::LimitExceeded));
        }
        Ok(Self {
            output,
            format: header.format,
            limits,
            count: 0,
            count_at,
            table_at,
            pending_table_bytes,
            frames: Vec::new(),
            records: 0,
            keys: HashMap::new(),
            save_game: header.save_game,
            wide_g1: header.user == Some(b"XZEN"),
            metadata_bytes: [Some(header.archiver), header.date, header.user]
                .into_iter()
                .flatten()
                .try_fold(0usize, |n, part| n.checked_add(part.len()))
                .ok_or_else(|| output_error(ErrorKind::LimitExceeded))?,
        })
    }
    pub(crate) fn session_header(&self) -> (bool, bool, usize, Limits) {
        (
            self.save_game,
            self.wide_g1,
            self.metadata_bytes,
            self.limits,
        )
    }
    pub fn format(&self) -> Format {
        self.format
    }
    /// Whether this writer was opened for savegame schemas.
    pub fn is_save_game(&self) -> bool {
        self.save_game
    }
    pub fn depth(&self) -> usize {
        self.frames.len()
    }
    /// Current staged bytes, including headers whose counts await finalization.
    pub fn len(&self) -> usize {
        self.output.as_ref().len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn temporary(&self) -> Bytes {
        Bytes::new(self.available())
    }
    fn available(&self) -> usize {
        self.limits.bytes - self.len() - self.pending_table_bytes
    }
    fn check_record(&self) -> Result<()> {
        if self.frames.len() >= self.limits.depth || self.records >= self.limits.objects {
            return Err(output_error(ErrorKind::LimitExceeded));
        }
        Ok(())
    }
    fn object(
        &self,
        w: &mut Bytes,
        name: &[u8],
        class: &[u8],
        version: u16,
        index: u32,
    ) -> Result<()> {
        if name.len() > self.limits.field_bytes || class.len() > self.limits.field_bytes {
            return Err(output_error(ErrorKind::LimitExceeded));
        }
        if self.format != Format::Binary && (!token(name) || !token(class)) {
            return Err(output_error(ErrorKind::InvalidData));
        }
        if self.format == Format::Binary {
            w.u32(0, CTX)?;
            w.u16(version, CTX)?;
            w.u32(index, CTX)?;
            w.terminated(name, 0, self.limits.field_bytes, CTX)?;
            w.terminated(class, 0, self.limits.field_bytes, CTX)?;
        } else {
            let mut line = Bytes::new(if self.format == Format::BinSafe {
                self.limits.field_bytes.min(u16::MAX as usize)
            } else {
                self.limits.field_bytes
            });
            line.bytes(b"[", CTX)?;
            line.bytes(name, CTX)?;
            line.bytes(b" ", CTX)?;
            line.bytes(class, CTX)?;
            line.bytes(b" ", CTX)?;
            out_text(&mut line, version)?;
            line.bytes(b" ", CTX)?;
            out_text(&mut line, index)?;
            line.bytes(b"]", CTX)?;
            if self.format == Format::Ascii {
                w.bytes(line.as_ref(), CTX)?;
                w.u8(b'\n', CTX)?;
            } else {
                w.u8(1, CTX)?;
                w.u16(line.as_ref().len() as u16, CTX)?;
                w.bytes(line.as_ref(), CTX)?;
            }
        }
        Ok(())
    }
    /// Start a record, allocating an index for ordinary classes. Empty classes
    /// become `%` with index zero. Use `write_reference` for raw reference records.
    pub fn begin_object(&mut self, name: &[u8], class: &[u8], version: u16) -> Result<u32> {
        self.check_record()?;
        if class == b"\xa7" {
            return Err(output_error(ErrorKind::InvalidData));
        }
        let class = if class.is_empty() { b"%" } else { class };
        let empty = class == b"%";
        let index = if empty { 0 } else { self.count };
        let next = if empty {
            self.count
        } else {
            self.count
                .checked_add(1)
                .ok_or_else(|| output_error(ErrorKind::LimitExceeded))?
        };
        let mut record = self.temporary();
        self.object(&mut record, name, class, version, index)?;
        self.frames
            .try_reserve(1)
            .map_err(|_| output_error(ErrorKind::LimitExceeded))?;
        let start = self.len();
        self.output.bytes(record.as_ref(), CTX)?;
        self.frames.push(start);
        self.count = next;
        self.records += 1;
        Ok(index)
    }
    /// Anonymous world grammar frame: preserve metadata without allocating identity.
    pub(crate) fn begin_anonymous(
        &mut self,
        name: &[u8],
        class: &[u8],
        version: u16,
        index: u32,
    ) -> Result<()> {
        self.check_record()?;
        if !(class.is_empty() || class == b"%") {
            return Err(output_error(ErrorKind::Unsupported));
        }
        if class.is_empty() && self.format != Format::Binary {
            return Err(output_error(ErrorKind::Unsupported));
        }
        let mut record = self.temporary();
        self.object(&mut record, name, class, version, index)?;
        self.frames
            .try_reserve(1)
            .map_err(|_| output_error(ErrorKind::LimitExceeded))?;
        let start = self.len();
        self.output.bytes(record.as_ref(), CTX)?;
        self.frames.push(start);
        self.records += 1;
        Ok(())
    }
    /// Append a caller-defined raw span without archive tags, names or conversion.
    /// The total output limit includes pending BIN_SAFE table bytes; `field_bytes`
    /// does not cap this span. Failure leaves output and archive state unchanged.
    /// BINARY object lengths include the appended bytes when the object closes.
    ///
    /// No record/key counts or graph identities are registered; use object
    /// sessions for shared identity. Unlike `write(name, Value::Raw(...))`, this
    /// adds no field framing. Supply any separators required by the raw grammar.
    /// Prepare complete raw bytes before appending them.
    pub fn unframed(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > self.available() {
            return Err(output_error(ErrorKind::LimitExceeded));
        }
        self.output.bytes(bytes, CTX)
    }
    /// Close the current record. BINARY patches its complete byte extent.
    pub fn end_object(&mut self) -> Result<()> {
        let start = *self
            .frames
            .last()
            .ok_or_else(|| output_error(ErrorKind::InvalidData))?;
        if self.format == Format::Binary {
            let length = u32::try_from(self.len() - start)
                .map_err(|_| output_error(ErrorKind::LimitExceeded))?;
            let end = self.len();
            self.output.seek_to(SeekFrom::Start(start as u64), CTX)?;
            self.output.u32(length, CTX)?;
            self.output.seek_to(SeekFrom::Start(end as u64), CTX)?;
        } else {
            let ending: &[u8] = if self.format == Format::Ascii {
                b"[]\n"
            } else {
                b"\x01\x02\x00[]"
            };
            if ending.len() > self.available() {
                return Err(output_error(ErrorKind::LimitExceeded));
            }
            self.output.bytes(ending, CTX)?;
        }
        self.frames.pop();
        Ok(())
    }
    /// Write a complete raw reference record. It does not resolve, validate or
    /// allocate the referenced index, so forward references remain representable.
    pub fn write_reference(&mut self, name: &[u8], index: u32) -> Result<()> {
        self.check_record()?;
        let mut record = self.temporary();
        self.object(&mut record, name, b"\xa7", 0, index)?;
        if self.format == Format::Binary {
            let length = u32::try_from(record.as_ref().len())
                .map_err(|_| output_error(ErrorKind::LimitExceeded))?;
            record.seek_to(SeekFrom::Start(0), CTX)?;
            record.u32(length, CTX)?;
        } else {
            record.bytes(
                if self.format == Format::Ascii {
                    b"[]\n"
                } else {
                    b"\x01\x02\x00[]"
                },
                CTX,
            )?;
        }
        self.output.bytes(record.as_ref(), CTX)?;
        self.records += 1;
        Ok(())
    }
    /// Serialize one primitive. Names are ignored in BINARY, where none exist on
    /// the wire. BIN_SAFE interns raw names; ASCII requires a nonempty field name
    /// without whitespace, `=`, `:`, NUL or brackets.
    pub fn write(&mut self, name: &[u8], value: Value<'_>) -> Result<()> {
        let mut record = self.temporary();
        let mut new_key = None;
        let mut new_table_bytes = 0;
        match self.format {
            Format::Ascii => self.ascii(&mut record, name, &value)?,
            Format::Binary => binary(&mut record, &value, self.limits.field_bytes, false)?,
            Format::BinSafe => {
                if name.len() > self.limits.field_bytes || name.len() > u16::MAX as usize {
                    return Err(output_error(ErrorKind::LimitExceeded));
                }
                let index = if let Some(index) = self.keys.get(name) {
                    *index
                } else {
                    if self.keys.len() >= self.limits.keys || self.keys.len() >= 65_536 {
                        return Err(output_error(ErrorKind::LimitExceeded));
                    }
                    new_table_bytes = name.len() + 8;
                    let remaining = self
                        .available()
                        .checked_sub(new_table_bytes)
                        .ok_or_else(|| output_error(ErrorKind::LimitExceeded))?;
                    if remaining < 6 {
                        return Err(output_error(ErrorKind::LimitExceeded));
                    }
                    record = Bytes::new(remaining);
                    let mut owned = Vec::new();
                    owned
                        .try_reserve_exact(name.len())
                        .map_err(|_| output_error(ErrorKind::LimitExceeded))?;
                    owned.extend_from_slice(name);
                    new_key = Some(owned);
                    self.keys.len() as u32
                };
                record.u8(18, CTX)?;
                record.u32(index, CTX)?;
                record.u8(tag(value.value_type()), CTX)?;
                if matches!(tag(value.value_type()), 1 | 9 | 16) {
                    let mut payload = Bytes::new(self.limits.field_bytes.min(u16::MAX as usize));
                    binary(&mut payload, &value, self.limits.field_bytes, true)?;
                    record.u16(payload.as_ref().len() as u16, CTX)?;
                    record.bytes(payload.as_ref(), CTX)?;
                } else {
                    binary(&mut record, &value, self.limits.field_bytes, true)?;
                }
            }
        }
        if new_key.is_some() {
            self.keys
                .try_reserve(1)
                .map_err(|_| output_error(ErrorKind::LimitExceeded))?;
        }
        self.output.bytes(record.as_ref(), CTX)?;
        if let Some(key) = new_key {
            let index = self.keys.len() as u32;
            self.keys.insert(key, index);
            self.pending_table_bytes += new_table_bytes;
        }
        Ok(())
    }
    fn ascii(&self, w: &mut Bytes, name: &[u8], value: &Value<'_>) -> Result<()> {
        if !token(name) || name.contains(&b'=') || name.contains(&b':') {
            return Err(output_error(ErrorKind::InvalidData));
        }
        let mut line = Bytes::new(self.limits.field_bytes);
        line.bytes(name, CTX)?;
        line.u8(b'=', CTX)?;
        let ty: &[u8] = match value {
            Value::String(_) => b"string",
            Value::Int(_) | Value::Byte(_) | Value::Word(_) => b"int",
            Value::Float(_) => b"float",
            Value::Enum(_) => b"enum",
            Value::Bool(_) => b"bool",
            Value::Color(_) => b"color",
            Value::Vec3(_) => b"vec3",
            Value::Raw(_) | Value::Mat3(_) => b"raw",
            _ => b"rawFloat",
        };
        line.bytes(ty, CTX)?;
        line.u8(b':', CTX)?;
        match value {
            Value::String(v) => {
                clean_line(v, self.limits.field_bytes)?;
                line.bytes(v, CTX)?;
            }
            Value::Int(v) => out_text(&mut line, v)?,
            Value::Byte(v) => out_text(&mut line, v)?,
            Value::Word(v) => out_text(&mut line, v)?,
            Value::Enum(v) => out_text(&mut line, v)?,
            Value::Bool(v) => out_text(&mut line, u8::from(*v))?,
            Value::Float(v) => out_text(&mut line, v)?,
            Value::Color(v) => {
                for (i, b) in v.to_rgba().iter().enumerate() {
                    if i != 0 {
                        line.u8(b' ', CTX)?;
                    }
                    out_text(&mut line, b)?;
                }
            }
            Value::Vec2(v) => text_floats(&mut line, &v.to_array())?,
            Value::Vec3(v) => text_floats(&mut line, &v.to_array())?,
            Value::Aabb(v) => text_floats(
                &mut line,
                &[v.min.x, v.min.y, v.min.z, v.max.x, v.max.y, v.max.z],
            )?,
            Value::RawFloat(v) => text_floats(&mut line, v)?,
            Value::Raw(v) => hex(&mut line, v)?,
            Value::Mat3(v) => {
                let mut raw = Bytes::new(36);
                v.write(&mut raw, CTX)?;
                hex(&mut line, raw.as_ref())?;
            }
        }
        w.bytes(line.as_ref(), CTX)?;
        w.u8(b'\n', CTX)?;
        Ok(())
    }
    /// Complete the archive. Unclosed records and final table/output overflow
    /// fail; this consumes the writer even on failure. Returned bytes stand alone.
    pub fn finish(mut self) -> Result<Vec<u8>> {
        if !self.frames.is_empty() {
            return Err(output_error(ErrorKind::InvalidData));
        }
        if let Some(table_at) = self.table_at {
            let offset =
                u32::try_from(self.len()).map_err(|_| output_error(ErrorKind::LimitExceeded))?;
            self.output.u32(self.keys.len() as u32, CTX)?;
            let mut keys = Vec::new();
            keys.try_reserve_exact(self.keys.len())
                .map_err(|_| output_error(ErrorKind::LimitExceeded))?;
            keys.extend(self.keys.iter());
            keys.sort_unstable_by_key(|(_, index)| **index);
            for (name, index) in keys {
                self.output.u16(name.len() as u16, CTX)?;
                self.output.u16(*index as u16, CTX)?;
                // Hash bytes as signed values for the BIN_SAFE wire format. Readers
                // use insertion indices; stored hashes are not used as key IDs.
                let hash = name.iter().fold(0u32, |hash, b| {
                    hash.wrapping_mul(33).wrapping_add((*b as i8 as i32) as u32)
                }) % 97;
                self.output.u32(hash, CTX)?;
                self.output.bytes(name, CTX)?;
            }
            self.output.seek_to(SeekFrom::Start(table_at as u64), CTX)?;
            self.output.u32(offset, CTX)?;
            self.output
                .seek_to(SeekFrom::Start(self.count_at as u64), CTX)?;
            self.output.u32(self.count, CTX)?;
        } else {
            self.output
                .seek_to(SeekFrom::Start(self.count_at as u64), CTX)?;
            use std::io::Write;
            write!(self.output, "{:010}", self.count)
                .map_err(|_| output_error(ErrorKind::LimitExceeded))?;
        }
        Ok(self.output.into_inner())
    }
}
fn text_floats(w: &mut Bytes, values: &[f32]) -> Result<()> {
    for (i, v) in values.iter().enumerate() {
        if i != 0 {
            w.u8(b' ', CTX)?;
        }
        out_text(w, v)?;
    }
    Ok(())
}
fn hex(w: &mut Bytes, values: &[u8]) -> Result<()> {
    const DIGITS: &[u8] = b"0123456789abcdef";
    for v in values {
        w.bytes(&[DIGITS[(v >> 4) as usize], DIGITS[(v & 15) as usize]], CTX)?;
    }
    Ok(())
}
fn binary(w: &mut Bytes, value: &Value<'_>, limit: usize, binsafe: bool) -> Result<()> {
    match value {
        Value::String(v) => {
            if v.len() > limit {
                return Err(output_error(ErrorKind::LimitExceeded));
            }
            if binsafe {
                w.bytes(v, CTX)?;
            } else {
                w.terminated(v, 0, limit, CTX)?;
            }
        }
        Value::Int(v) => w.i32(*v, CTX)?,
        Value::Float(v) => w.f32(*v, CTX)?,
        Value::Byte(v) => w.u8(*v, CTX)?,
        Value::Word(v) => w.u16(*v, CTX)?,
        Value::Enum(v) => {
            if binsafe {
                w.u32(*v, CTX)?;
            } else {
                w.u8(
                    u8::try_from(*v).map_err(|_| output_error(ErrorKind::Unsupported))?,
                    CTX,
                )?;
            }
        }
        Value::Bool(v) => {
            if binsafe {
                w.u32(u32::from(*v), CTX)?;
            } else {
                w.u8(u8::from(*v), CTX)?;
            }
        }
        Value::Color(v) => w.bytes(&v.to_bgra(), CTX)?,
        Value::Vec2(v) => v.write(w, CTX)?,
        Value::Vec3(v) => v.write(w, CTX)?,
        Value::Aabb(v) => v.write(w, CTX)?,
        Value::Mat3(v) => v.write(w, CTX)?,
        Value::Raw(v) => {
            if v.len() > limit {
                return Err(output_error(ErrorKind::LimitExceeded));
            }
            w.bytes(v, CTX)?;
        }
        Value::RawFloat(v) => {
            if v.len() > limit / 4 {
                return Err(output_error(ErrorKind::LimitExceeded));
            }
            for v in v.iter() {
                w.f32(*v, CTX)?;
            }
        }
    }
    Ok(())
}
