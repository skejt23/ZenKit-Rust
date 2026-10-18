// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;
use crate::io::{LineEnding, Reader as Bytes};
use std::io::SeekFrom;
#[path = "skip.rs"]
mod skip;
pub use skip::SkipLimits;

/// A bounded schema-directed reader over an archive byte slice.
///
/// Offsets are relative to the supplied slice, starting at archive magic.
/// BIN_SAFE table offsets use that origin.
/// Failed operations preserve the cursor, object stack, and record count.
/// EOF inside an object is an error, never an implicit object terminator.
pub struct Reader<'a> {
    input: Bytes<'a>,
    header: Header<'a>,
    limits: Limits,
    count: u32,
    body_end: usize,
    keys: Vec<&'a [u8]>,
    ends: Vec<usize>,
    records: usize,
    space: AddressSpace<'a>,
    table_range: Option<std::ops::Range<usize>>,
}
impl std::fmt::Debug for Reader<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reader")
            .field("format", &self.header.format)
            .field("position", &self.position())
            .field("depth", &self.ends.len())
            .field("declared_objects", &self.count)
            .finish_non_exhaustive()
    }
}
fn line<'a>(r: &mut Bytes<'a>, limit: usize) -> Result<&'a [u8]> {
    let start = r.position();
    let v = r.line(LineEnding::Lf, limit.saturating_add(1), CTX)?;
    let v = v.strip_suffix(b"\r").unwrap_or(v);
    if v.len() > limit {
        return Err(limited(start));
    }
    Ok(v)
}
fn prefix<'a>(line: &'a [u8], prefix: &[u8], offset: usize) -> Result<&'a [u8]> {
    line.strip_prefix(prefix).ok_or_else(|| invalid(offset))
}
impl<'a> Header<'a> {
    /// Read only the generic header, through the line ending after its `END`.
    /// No backend header, separator, object count, body or BIN_SAFE table is read.
    /// Returned metadata borrows the original input; errors leave its cursor unchanged.
    ///
    /// `limits.bytes` bounds the header window inspected from the current cursor,
    /// including line endings. Trailing body bytes do not count toward that limit.
    /// `limits.field_bytes` bounds each line excluding its LF or CRLF ending;
    /// the object, depth and key limits do not apply to this operation. Exhausting
    /// the byte window reports `LimitExceeded`; physically truncated input reports
    /// `UnexpectedEof`. Diagnostic offsets retain the input's absolute origin.
    pub(crate) fn read(input: &mut Bytes<'a>, limits: Limits) -> Result<Self> {
        let start = input.position();
        let bounded = input.remaining() > limits.bytes;
        let mut candidate = input
            .clone()
            .substream(input.remaining().min(limits.bytes), CTX)?;
        let header = Self::parse(&mut candidate, limits.field_bytes).map_err(|mut error| {
            if bounded && error.kind == ErrorKind::UnexpectedEof {
                error.kind = ErrorKind::LimitExceeded;
            }
            error
        })?;
        input.take_bytes(candidate.position() - start, CTX)?;
        Ok(header)
    }

    fn parse(input: &mut Bytes<'a>, field_bytes: usize) -> Result<Self> {
        let mut head_line = || {
            let pos = input.position();
            Ok::<_, Error>((pos, line(input, field_bytes)?))
        };
        let (p, v) = head_line()?;
        if v != b"ZenGin Archive" {
            return Err(invalid(p));
        }
        let (p, v) = head_line()?;
        let version: u32 = number(prefix(v, b"ver ", p)?, p + 4)?;
        if version != 1 {
            return Err(Error::new(ErrorKind::Unsupported, p + 4, CTX));
        }
        let (p, archiver) = head_line()?;
        if archiver.is_empty() || archiver.contains(&0) {
            return Err(invalid(p));
        }
        let (p, v) = head_line()?;
        let format = match v {
            b"ASCII" => Format::Ascii,
            b"BINARY" => Format::Binary,
            b"BIN_SAFE" => Format::BinSafe,
            _ => return Err(Error::new(ErrorKind::Unsupported, p, CTX)),
        };
        let (p, v) = head_line()?;
        let save: u32 = number(prefix(v, b"saveGame ", p)?, p + 9)?;
        let (mut p, mut v) = head_line()?;
        let date = if let Some(date) = v.strip_prefix(b"date ") {
            (p, v) = head_line()?;
            Some(date)
        } else {
            None
        };
        let user = if let Some(user) = v.strip_prefix(b"user ") {
            (p, v) = head_line()?;
            Some(user)
        } else {
            None
        };
        if v != b"END" {
            return Err(invalid(p));
        }
        Ok(Header {
            version,
            archiver,
            format,
            save_game: save != 0,
            date,
            user,
        })
    }
}
fn ascii_line<'a>(r: &mut Bytes<'a>, limit: usize) -> Result<(usize, &'a [u8])> {
    loop {
        let p = r.position();
        let value = match line(r, limit) {
            Ok(v) => v,
            Err(error) if error.kind == ErrorKind::UnexpectedEof && r.remaining() != 0 => {
                r.fixed_bytes(r.remaining(), limit, CTX)?
            }
            Err(error) => return Err(error),
        };
        if !value.trim_ascii().is_empty() {
            return Ok((p, value));
        }
    }
}

#[cfg(test)]
mod name_lookahead_tests {
    use super::*;
    #[test]
    fn tagged_field_name_peek_never_consumes_a_record() {
        for format in [Format::Ascii, Format::BinSafe] {
            let limits = Limits::default();
            let mut w = Writer::new(Header::new(format), limits).unwrap();
            w.write(b"shortKey7", Value::Int(-3)).unwrap();
            w.write(b"next", Value::Bool(true)).unwrap();
            let bytes = w.finish().unwrap();
            let mut r = Reader::new(&bytes, limits).unwrap();
            let p = r.position();
            assert!(!r.has_field_name(b"shortKey").unwrap());
            assert!(!r.has_field_name(b"shortKey70").unwrap());
            for _ in 0..10 {
                assert!(r.has_field_name(b"shortKey7").unwrap());
                assert_eq!(r.position(), p);
            }
            assert_eq!(r.read_int().unwrap(), -3);
            assert!(r.has_field_name(b"next").unwrap());
            assert!(r.read_bool().unwrap());
            r.finish().unwrap();
        }
    }
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8], limits: Limits) -> Result<Self> {
        let space = AddressSpace::new(bytes, 0)?;
        let mut tables = TableBudget::new(limits.keys, limits.bytes);
        Self::parse(
            space,
            0..bytes.len(),
            AsciiSeparator::Optional,
            limits,
            &mut tables,
            true,
        )
    }
    /// Read an archive header inside an explicit body window. Table pointers use
    /// physical AddressSpace offsets. Only body reads advance position(); table
    /// bytes are inspected out of band. The work budget is unchanged on failure.
    pub(crate) fn embedded(
        space: AddressSpace<'a>,
        body: std::ops::Range<usize>,
        separator: AsciiSeparator,
        limits: Limits,
        tables: &mut TableBudget,
    ) -> Result<Self> {
        let mut candidate = *tables;
        let value = Self::parse(space, body, separator, limits, &mut candidate, false)?;
        *tables = candidate;
        Ok(value)
    }
    fn parse(
        space: AddressSpace<'a>,
        body: std::ops::Range<usize>,
        separator: AsciiSeparator,
        limits: Limits,
        tables: &mut TableBudget,
        standalone: bool,
    ) -> Result<Self> {
        if space.bytes.len() > limits.bytes {
            return Err(limited(body.start));
        }
        let mut input = space.reader(body.clone())?;
        let mut table_range = None;
        let header = Header::read(&mut input, limits)?;
        let format = header.format;
        let count;
        let body_end;
        let mut keys = Vec::new();
        if format == Format::BinSafe {
            let p = input.position();
            if input.u32(CTX)? != 2 {
                return Err(Error::new(ErrorKind::Unsupported, p, CTX));
            }
            count = input.u32(CTX)?;
            let p = input.position();
            let pointer_at = p;
            let table_start = input.u32(CTX)? as usize;
            if table_start < space.origin || table_start > space.range().end {
                return Err(invalid(p));
            }
            if standalone && table_start < input.position() {
                return Err(invalid(p));
            }
            let mut table = space.reader(table_start..space.range().end)?;
            let p = table.position();
            let length = table.u32(CTX)? as usize;
            if length > limits.keys || length > 65_536 {
                return Err(limited(p));
            }
            if length > table.remaining() / 8 {
                return Err(Error::new(ErrorKind::UnexpectedEof, table.position(), CTX));
            }
            let minimum = length
                .checked_mul(8)
                .and_then(|n| n.checked_add(4))
                .ok_or_else(|| limited(p))?;
            tables.charge(length, minimum, p)?;
            reserve(&mut keys, length, p)?;
            keys.resize(length, None);
            for _ in 0..length {
                let length = table.u16(CTX)? as usize;
                let p = table.position();
                let index = table.u16(CTX)? as usize;
                let _stored_hash = table.u32(CTX)?;
                tables.charge(0, length, table.position())?;
                let name = table.fixed_bytes(length, limits.field_bytes, CTX)?;
                let slot = keys.get_mut(index).ok_or_else(|| invalid(p))?;
                if slot.replace(name).is_some() {
                    return Err(invalid(p));
                }
            }
            if standalone && table.remaining() != 0 {
                return Err(invalid(table.position()));
            }
            let extent = table_start..table.position();
            if extent.start < input.position() && extent.end > body.start {
                return Err(invalid(pointer_at));
            }
            body_end = if table_start >= input.position() {
                body.end.min(table_start)
            } else {
                body.end
            };
            table_range = Some(extent);
        } else {
            let p = input.position();
            let v = line(&mut input, limits.field_bytes)?;
            count = number(trim(prefix(v, b"objects ", p)?), p + 8)?;
            let p = input.position();
            if line(&mut input, limits.field_bytes)? != b"END" {
                return Err(invalid(p));
            }
            // BINARY requires exactly one blank separator; arbitrary whitespace
            // skipping would consume payload. ASCII also accepts no separator.
            let p = input.position();
            let mut probe = input.clone();
            if format == Format::Binary || separator == AsciiSeparator::Required {
                if !line(&mut probe, limits.field_bytes)?.is_empty() {
                    return Err(invalid(p));
                }
                input = probe;
            } else if separator == AsciiSeparator::Optional
                && probe.remaining() != 0
                && matches!(line(&mut probe,limits.field_bytes),Ok(v) if v.is_empty())
            {
                input = probe;
            }
            body_end = body.end;
        }
        if count as usize > limits.objects {
            return Err(limited(input.position()));
        }
        // A permutation of 0..length was checked above, so every slot is filled.
        let mut final_keys = Vec::new();
        reserve(&mut final_keys, keys.len(), input.position())?;
        final_keys.extend(keys.into_iter().map(Option::unwrap));
        let keys = final_keys;
        Ok(Self {
            input,
            header,
            limits,
            count,
            body_end,
            keys,
            ends: Vec::new(),
            records: 0,
            space,
            table_range,
        })
    }
    /// Validated physical table extent, independent from the sequential cursor.
    pub(crate) fn table_range(&self) -> Option<std::ops::Range<usize>> {
        self.table_range.clone()
    }
    pub(crate) fn address_space(&self) -> AddressSpace<'a> {
        self.space
    }
    /// Check only that all opened records are closed, without consuming a parent suffix.
    pub(crate) fn finish_records(&self) -> Result<()> {
        if self.ends.is_empty() {
            Ok(())
        } else {
            Err(invalid(self.position()))
        }
    }
    pub fn header(&self) -> Header<'a> {
        self.header
    }
    pub(crate) fn configured_limits(&self) -> Limits {
        self.limits
    }
    /// Header metadata, not the total number of records: empty/reference records
    /// do not allocate indices. No graph count/identity validation is implied.
    pub fn object_count(&self) -> u32 {
        self.count
    }
    pub fn position(&self) -> usize {
        self.input.position()
    }
    pub(crate) fn remaining_bytes(&self) -> usize {
        self.ends.last().copied().unwrap_or(self.body_end) - self.position()
    }
    pub(crate) fn offset_of(&self, bytes: &[u8]) -> Option<usize> {
        self.input.offset_of(bytes)
    }
    pub fn depth(&self) -> usize {
        self.ends.len()
    }
    /// Take an unframed raw span as a bounded IO reader with absolute offsets.
    /// Success advances the archive by `length`; errors in the returned reader
    /// do not rewind it. Failure leaves archive state unchanged.
    ///
    /// BINARY spans stop at the current object's end; tagged spans are bounded
    /// by the body/table without inspecting object-end delimiters. The caller
    /// supplies the exact length. Input limits apply; `field_bytes` does not.
    /// No records, keys or graph identities are registered. Use object sessions
    /// for shared identity, or [`Reader::read_raw`] for a framed archive field.
    pub fn unframed(&mut self, length: usize) -> Result<Bytes<'a>> {
        let mut r = self.candidate()?;
        let bytes = r.substream(length, CTX)?;
        self.commit(&r)?;
        Ok(bytes)
    }
    pub(crate) fn has_object_end(&self) -> Result<bool> {
        if self.ends.is_empty() {
            return Err(invalid(self.position()));
        }
        let mut r = self.candidate()?;
        if self.header.format == Format::Binary {
            return Ok(r.remaining() == 0);
        }
        if self.header.format == Format::BinSafe && r.clone().u8(CTX)? != 1 {
            return Ok(false);
        }
        Ok(self.record_line(&mut r)?.0 == b"[]")
    }
    fn candidate(&self) -> Result<Bytes<'a>> {
        let mut candidate = self.input.clone();
        candidate.substream(
            self.ends.last().copied().unwrap_or(self.body_end) - self.position(),
            CTX,
        )
    }
    /// Inspect a known schema's fixed suffix inside the current BINARY frame.
    /// The cursor and frames are unchanged. The caller must still parse the
    /// entire schema and require an exact end; this is not generic field framing.
    pub(crate) fn binary_object_tail(&self, length: usize) -> Result<&'a [u8]> {
        if self.header.format != Format::Binary || self.ends.is_empty() {
            return Err(invalid(self.position()));
        }
        let mut tail = self.candidate()?;
        let skip = tail.remaining().checked_sub(length).ok_or_else(|| {
            Error::new(
                ErrorKind::UnexpectedEof,
                self.position(),
                "BINARY object suffix",
            )
        })?;
        tail.seek_to(SeekFrom::Start(skip as u64), CTX)?;
        tail.take_bytes(length, CTX)
    }
    fn commit(&mut self, r: &Bytes<'a>) -> Result<()> {
        self.input
            .seek_to(
                SeekFrom::Start((r.position() - self.input.view().1) as u64),
                CTX,
            )
            .map(|_| ())
    }
    fn record_line(&self, r: &mut Bytes<'a>) -> Result<(&'a [u8], usize)> {
        if self.header.format == Format::Ascii {
            let (p, whole) = ascii_line(r, self.limits.field_bytes)?;
            let value = whole.trim_ascii_start();
            Ok((value, p + whole.len() - value.len()))
        } else {
            let p = r.position();
            if r.u8(CTX)? != 1 {
                return Err(invalid(p));
            }
            let len = r.u16(CTX)? as usize;
            let p = r.position();
            Ok((r.fixed_bytes(len, self.limits.field_bytes, CTX)?, p))
        }
    }
    /// Bounded, allocation-free tagged name lookahead for dependent schemas.
    /// This does not consume a record or validate its value; the following read
    /// still checks its tag, payload, record budget and exact field name.
    pub(crate) fn has_field_name(&self, name: &[u8]) -> Result<bool> {
        let mut r = self.candidate()?;
        if r.remaining() == 0 {
            return Ok(false);
        }
        match self.header.format {
            Format::Ascii => {
                let (line, _) = self.record_line(&mut r)?;
                Ok(line
                    .iter()
                    .position(|b| *b == b'=')
                    .is_some_and(|i| &line[..i] == name))
            }
            Format::BinSafe => {
                if r.u8(CTX)? != 18 {
                    return Ok(false);
                }
                let p = r.position();
                let index = r.u32(CTX)? as usize;
                Ok(*self.keys.get(index).ok_or_else(|| invalid(p))? == name)
            }
            Format::Binary => Err(Error::new(
                ErrorKind::Unsupported,
                self.position(),
                "BINARY has no field names",
            )),
        }
    }
    /// Begin the next record. In tagged encodings a field/end returns `None`
    /// without advancing; BINARY requires the caller to know an object follows.
    pub fn begin_object(&mut self) -> Result<Option<ObjectHeader<'a>>> {
        let mut r = self.candidate()?;
        if r.remaining() == 0 {
            return Ok(None);
        }
        if self.header.format == Format::Ascii
            && r.clone()
                .take_bytes(r.remaining(), CTX)?
                .trim_ascii_start()
                .is_empty()
        {
            return Ok(None);
        }
        let p = r.position();
        let (object, end) = if self.header.format == Format::Binary {
            let length = r.u32(CTX)? as usize;
            let end = p.checked_add(length).ok_or_else(|| invalid(p))?;
            if length < 12 || end > p + r.remaining() + 4 {
                return Err(invalid(p));
            }
            let mut object = r.substream(length - 4, CTX)?;
            let version = object.u16(CTX)?;
            let index = object.u32(CTX)?;
            let object_name = object.terminated(0, self.limits.field_bytes, CTX)?;
            let class_name = object.terminated(0, self.limits.field_bytes, CTX)?;
            r = object;
            (
                ObjectHeader {
                    object_name,
                    class_name,
                    version,
                    index,
                },
                end,
            )
        } else {
            if self.header.format == Format::BinSafe && r.clone().u8(CTX)? != 1 {
                return Ok(None);
            }
            let (v, p) = self.record_line(&mut r)?;
            if !v.starts_with(b"[") || v == b"[]" {
                return Ok(None);
            }
            let v = v
                .strip_prefix(b"[")
                .and_then(|v| v.strip_suffix(b"]"))
                .ok_or_else(|| invalid(p))?;
            let mut parts = parts(v);
            let (_, object_name) = parts.next().ok_or_else(|| invalid(p))?;
            let (_, class_name) = parts.next().ok_or_else(|| invalid(p))?;
            let (offset, part) = parts.next().ok_or_else(|| invalid(p))?;
            let version = number(part, p + 1 + offset)?;
            let (offset, part) = parts.next().ok_or_else(|| invalid(p))?;
            let index = number(part, p + 1 + offset)?;
            if parts.next().is_some() || !token(object_name) || !token(class_name) {
                return Err(invalid(p));
            }
            (
                ObjectHeader {
                    object_name,
                    class_name,
                    version,
                    index,
                },
                self.body_end,
            )
        };
        if self.ends.len() >= self.limits.depth || self.records >= self.limits.objects {
            return Err(limited(p));
        }
        reserve(&mut self.ends, 1, p)?;
        self.commit(&r)?;
        self.ends.push(end);
        self.records += 1;
        Ok(Some(object))
    }
    /// Consume a matching end, or return false without consuming the next field.
    /// Calling outside an object is an error. Missing/truncated ends are errors.
    pub fn end_object(&mut self) -> Result<bool> {
        if self.ends.is_empty() {
            return Err(invalid(self.position()));
        }
        let mut r = self.candidate()?;
        if self.header.format == Format::Binary {
            if r.remaining() != 0 {
                return Ok(false);
            }
        } else {
            if self.header.format == Format::BinSafe && r.clone().u8(CTX)? != 1 {
                return Ok(false);
            }
            if self.record_line(&mut r)?.0 != b"[]" {
                return Ok(false);
            }
        }
        self.commit(&r)?;
        self.ends.pop();
        Ok(true)
    }
    /// Read one field, preserving its name where the encoding stores one.
    /// `Raw(n)`/`RawFloat(n)` require at least n bytes/floats in tagged formats
    /// and return the complete payload. BINARY consumes exactly n. Typed fixed
    /// values require exact length. Raw fallback requires a compatible raw or
    /// rawFloat wire tag; other typed tags are not reinterpreted. Exact encoded
    /// spelling/layout requires retaining original bytes or an unframed span.
    pub fn read(&mut self, kind: ValueType) -> Result<Entry<'a>> {
        let mut r = self.candidate()?;
        let entry = match self.header.format {
            Format::Ascii => self.ascii(&mut r, kind)?,
            Format::Binary => Entry {
                name: None,
                value: binary(&mut r, kind, self.limits.field_bytes)?,
            },
            Format::BinSafe => {
                let p = r.position();
                if r.u8(CTX)? != 18 {
                    return Err(invalid(p));
                }
                let p = r.position();
                let index = r.u32(CTX)? as usize;
                let name = *self.keys.get(index).ok_or_else(|| invalid(p))?;
                let p = r.position();
                if r.u8(CTX)? != tag(kind) {
                    return Err(invalid(p));
                }
                let value = if matches!(tag(kind), 1 | 9 | 16) {
                    let p = r.position();
                    let length = r.u16(CTX)? as usize;
                    if length > self.limits.field_bytes {
                        return Err(limited(p));
                    }
                    let mut payload = r.substream(length, CTX)?;
                    let actual = match kind {
                        ValueType::String => {
                            Value::String(Cow::Borrowed(payload.take_bytes(length, CTX)?))
                        }
                        ValueType::Raw(min) => {
                            if length < min {
                                return Err(invalid(p));
                            }
                            Value::Raw(Cow::Borrowed(payload.take_bytes(length, CTX)?))
                        }
                        ValueType::RawFloat(min) => {
                            if !length.is_multiple_of(4) || length / 4 < min {
                                return Err(invalid(p));
                            }
                            floats(&mut payload, length / 4, self.limits.field_bytes)?
                        }
                        _ => binary(&mut payload, kind, self.limits.field_bytes)?,
                    };
                    if payload.remaining() != 0 {
                        return Err(Error::new(
                            ErrorKind::Unsupported,
                            payload.position(),
                            "padded archive value; use raw field",
                        ));
                    }
                    actual
                } else if kind == ValueType::Bool {
                    Value::Bool(r.u32(CTX)? != 0)
                } else if kind == ValueType::Enum {
                    Value::Enum(r.u32(CTX)?)
                } else {
                    binary(&mut r, kind, self.limits.field_bytes)?
                };
                Entry {
                    name: Some(name),
                    value,
                }
            }
        };
        self.commit(&r)?;
        Ok(entry)
    }
    fn ascii(&self, r: &mut Bytes<'a>, kind: ValueType) -> Result<Entry<'a>> {
        let (start, whole) = ascii_line(r, self.limits.field_bytes)?;
        let v = whole.trim_ascii_start();
        let p = start + whole.len() - v.len();
        let equal = v
            .iter()
            .position(|b| *b == b'=')
            .ok_or_else(|| invalid(p))?;
        let name = &v[..equal];
        if name.is_empty() {
            return Err(invalid(p));
        }
        let rest = &v[equal + 1..];
        let colon = rest
            .iter()
            .position(|b| *b == b':')
            .ok_or_else(|| invalid(p + equal + 1))?;
        let ty = &rest[..colon];
        let payload = &rest[colon + 1..];
        let p = p + equal + 1 + colon + 1;
        let accepted = match kind {
            ValueType::String => ty == b"string",
            ValueType::Int => ty == b"int",
            ValueType::Byte => ty == b"int" || ty == b"byte",
            ValueType::Word => ty == b"int" || ty == b"word",
            ValueType::Float => ty == b"float",
            ValueType::Enum => ty == b"enum",
            ValueType::Bool => ty == b"bool",
            ValueType::Color => ty == b"color",
            ValueType::Vec3 => ty == b"vec3",
            ValueType::Vec2 | ValueType::Aabb | ValueType::RawFloat(_) => ty == b"rawFloat",
            ValueType::Mat3 => ty == b"raw" || ty == b"rawFloat",
            ValueType::Raw(_) => ty == b"raw",
        };
        if !accepted {
            return Err(invalid(p - colon - 1));
        }
        let scalar = trim(payload);
        let scalar_p = p + payload.len() - payload.trim_ascii_start().len();
        let value = match kind {
            ValueType::String => Value::String(Cow::Borrowed(payload)),
            ValueType::Int => Value::Int(number(scalar, scalar_p)?),
            ValueType::Byte => Value::Byte(number(scalar, scalar_p)?),
            ValueType::Word => Value::Word(number(scalar, scalar_p)?),
            ValueType::Enum => Value::Enum(number(scalar, scalar_p)?),
            ValueType::Bool => Value::Bool(number::<u32>(scalar, scalar_p)? != 0),
            ValueType::Float => Value::Float(number(scalar, scalar_p)?),
            ValueType::Color => {
                let a = components::<u8, 4>(payload, p)?;
                Value::Color(Color::from_rgba(a))
            }
            ValueType::Vec2 => Value::Vec2(Vec2::from(components::<f32, 2>(payload, p)?)),
            ValueType::Vec3 => Value::Vec3(Vec3::from(components::<f32, 3>(payload, p)?)),
            ValueType::Aabb => {
                let a = components::<f32, 6>(payload, p)?;
                Value::Aabb(Aabb {
                    min: Vec3::from([a[0], a[1], a[2]]),
                    max: Vec3::from([a[3], a[4], a[5]]),
                })
            }
            ValueType::Mat3 if ty == b"rawFloat" => {
                let a = components::<f32, 9>(payload, p)?;
                Value::Mat3(Mat3::from_rows([
                    [a[0], a[1], a[2]].into(),
                    [a[3], a[4], a[5]].into(),
                    [a[6], a[7], a[8]].into(),
                ]))
            }
            ValueType::Mat3 | ValueType::Raw(_) => {
                let decoded = unhex(payload, p)?;
                if kind == ValueType::Mat3 {
                    if decoded.len() > 36 {
                        return Err(Error::new(
                            ErrorKind::Unsupported,
                            p,
                            "padded archive value; use raw field",
                        ));
                    }
                    if decoded.len() != 36 {
                        return Err(invalid(p));
                    }
                    Value::Mat3(Mat3::read(&mut Bytes::new(&decoded), CTX)?)
                } else {
                    let ValueType::Raw(min) = kind else {
                        unreachable!()
                    };
                    if decoded.len() < min {
                        return Err(invalid(p));
                    }
                    Value::Raw(Cow::Owned(decoded))
                }
            }
            ValueType::RawFloat(min) => {
                let mut values = Vec::new();
                for (offset, v) in parts(payload) {
                    reserve(&mut values, 1, p + offset)?;
                    values.push(number(v, p + offset)?);
                }
                if values.len() < min {
                    return Err(invalid(p));
                }
                Value::RawFloat(Cow::Owned(values))
            }
        };
        Ok(Entry {
            name: Some(name),
            value,
        })
    }
    /// Require all records closed and every body byte consumed, allowing trailing
    /// ASCII whitespace. BIN_SAFE's table
    /// was already validated at construction. Object graph identity is deferred.
    pub fn finish(self) -> Result<()> {
        if !self.ends.is_empty() {
            return Err(invalid(self.position()));
        }
        if self.position() != self.body_end {
            let mut candidate = self.candidate()?;
            if self.header.format != Format::Ascii
                || !candidate
                    .take_bytes(candidate.remaining(), CTX)?
                    .trim_ascii()
                    .is_empty()
            {
                return Err(invalid(self.position()));
            }
        }
        Ok(())
    }
    /// Read all raw bytes. `minimum` is exact for BINARY and a lower bound in
    /// tagged formats; no surplus encoded bytes are discarded.
    pub fn read_raw(&mut self, minimum: usize) -> Result<Cow<'a, [u8]>> {
        match self.read(ValueType::Raw(minimum))?.value {
            Value::Raw(v) => Ok(v),
            _ => unreachable!("matching decoder"),
        }
    }
    /// Read all raw floats with the same minimum/exact policy as `read_raw`.
    pub fn read_raw_float(&mut self, minimum: usize) -> Result<Cow<'a, [f32]>> {
        match self.read(ValueType::RawFloat(minimum))?.value {
            Value::RawFloat(v) => Ok(v),
            _ => unreachable!("matching decoder"),
        }
    }
}
fn components<T: std::str::FromStr + Copy + Default, const N: usize>(
    bytes: &[u8],
    p: usize,
) -> Result<[T; N]> {
    let mut parts = parts(bytes);
    let mut out = [T::default(); N];
    for slot in &mut out {
        let (offset, part) = parts.next().ok_or_else(|| invalid(p + bytes.len()))?;
        *slot = number(part, p + offset)?;
    }
    if parts.next().is_some() {
        return Err(Error::new(
            ErrorKind::Unsupported,
            p,
            "padded archive value; use raw field",
        ));
    }
    Ok(out)
}
fn unhex(bytes: &[u8], p: usize) -> Result<Vec<u8>> {
    if !bytes.len().is_multiple_of(2) {
        return Err(invalid(p + bytes.len() - 1));
    }
    let mut out = Vec::new();
    reserve(&mut out, bytes.len() / 2, p)?;
    for (i, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
        let mut value = 0;
        for (j, b) in pair.iter().enumerate() {
            let digit = match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' => b - b'a' + 10,
                b'A'..=b'F' => b - b'A' + 10,
                _ => return Err(invalid(p + i * 2 + j)),
            };
            value = value * 16 + digit;
        }
        out.push(value);
    }
    Ok(out)
}
fn floats<'a>(r: &mut Bytes<'a>, count: usize, limit: usize) -> Result<Value<'a>> {
    let length = count.checked_mul(4).ok_or_else(|| limited(r.position()))?;
    if length > limit {
        return Err(limited(r.position()));
    }
    let mut payload = r.substream(length, CTX)?;
    let mut values = Vec::new();
    reserve(&mut values, count, payload.position())?;
    for _ in 0..count {
        values.push(payload.f32(CTX)?);
    }
    Ok(Value::RawFloat(Cow::Owned(values)))
}
fn binary<'a>(r: &mut Bytes<'a>, kind: ValueType, limit: usize) -> Result<Value<'a>> {
    Ok(match kind {
        ValueType::String => Value::String(Cow::Borrowed(r.terminated(0, limit, CTX)?)),
        ValueType::Int => Value::Int(r.i32(CTX)?),
        ValueType::Float => Value::Float(r.f32(CTX)?),
        ValueType::Byte => Value::Byte(r.u8(CTX)?),
        ValueType::Word => Value::Word(r.u16(CTX)?),
        ValueType::Enum => Value::Enum(r.u8(CTX)? as u32),
        ValueType::Bool => Value::Bool(r.u8(CTX)? != 0),
        ValueType::Color => Value::Color(Color::from_bgra(
            r.take_bytes(4, CTX)?.try_into().expect("four bytes"),
        )),
        ValueType::Vec2 => Value::Vec2(Vec2::read(r, CTX)?),
        ValueType::Vec3 => Value::Vec3(Vec3::read(r, CTX)?),
        ValueType::Aabb => Value::Aabb(Aabb::read(r, CTX)?),
        ValueType::Mat3 => Value::Mat3(Mat3::read(r, CTX)?),
        ValueType::Raw(n) => Value::Raw(Cow::Borrowed(r.fixed_bytes(n, limit, CTX)?)),
        ValueType::RawFloat(n) => floats(r, n, limit)?,
    })
}
macro_rules! typed { ($($method:ident,$kind:ident,$ty:ty);* $(;)?)=>{$(impl<'a> Reader<'a>{
    #[doc=concat!("Read a `",stringify!($kind),"` value, discarding only its field name. Use `read` to retain metadata.")]
    pub fn $method(&mut self)->Result<$ty>{match self.read(ValueType::$kind)?.value{Value::$kind(v)=>Ok(v),_=>unreachable!("matching decoder")}}
})*}; }
typed! {read_string,String,Cow<'a,[u8]>;read_int,Int,i32;read_float,Float,f32;read_byte,Byte,u8;read_word,Word,u16;read_enum,Enum,u32;read_bool,Bool,bool;read_color,Color,Color;read_vec2,Vec2,Vec2;read_vec3,Vec3,Vec3;read_aabb,Aabb,Aabb;read_mat3,Mat3,Mat3;}
