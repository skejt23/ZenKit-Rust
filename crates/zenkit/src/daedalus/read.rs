// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;
use crate::io::Reader;

impl Script {
    /// Read exactly one DAT and reject trailing bytes. Version is retained opaque.
    pub fn from_bytes(bytes: &[u8], limits: Limits) -> Result<Self> {
        if bytes.len() > limits.bytes || bytes.len() > limits.copied_bytes {
            return Err(at(ErrorKind::LimitExceeded, 0));
        }
        let mut r = Reader::new(bytes);
        let value = Self::read(&mut r, limits)?;
        if r.remaining() != 0 {
            return Err(at(ErrorKind::InvalidData, r.position()));
        }
        Ok(value)
    }
    /// Read one prefix, committing the input cursor only after complete success.
    /// All errors preserve the caller's cursor and retain absolute input offsets.
    /// Const-string parent words use the documented grammar, without
    /// heuristic whitespace shifting or fallback extra-word consumption.
    pub fn read(reader: &mut Reader<'_>, limits: Limits) -> Result<Self> {
        let offset = reader.position();
        let cap = limits.bytes.min(limits.copied_bytes);
        let mut probe = reader.clone();
        let span = probe.take_bytes(probe.remaining().min(cap), CTX)?;
        let mut r = reader.clone().substream(span.len(), CTX)?;
        let result = Self::parse(&mut r, span, offset, limits);
        let value = result.map_err(|e| {
            if e.kind == ErrorKind::UnexpectedEof && reader.remaining() > cap {
                at(ErrorKind::LimitExceeded, e.offset.unwrap_or(offset))
            } else {
                e
            }
        })?;
        reader.take_bytes(value.byte_len(), CTX)?;
        Ok(value)
    }
    fn parse(r: &mut Reader<'_>, input: &[u8], offset: usize, limits: Limits) -> Result<Self> {
        let version = r.u8(CTX)?;
        let count_offset = r.position();
        let count = r.u32(CTX)? as usize;
        if count > limits.symbols {
            return Err(at(ErrorKind::LimitExceeded, count_offset));
        }
        let minimum = count
            .checked_mul(40)
            .and_then(|n| n.checked_add(4))
            .ok_or_else(|| at(ErrorKind::LimitExceeded, count_offset))?;
        if minimum > r.remaining() {
            return Err(at(ErrorKind::UnexpectedEof, count_offset));
        }
        let mut sort = Vec::new();
        reserve(&mut sort, count, count_offset)?;
        for _ in 0..count {
            sort.push(r.u32(CTX)?);
        }
        let mut symbols = Vec::new();
        reserve(&mut symbols, count, count_offset)?;
        let mut strings = Vec::new();
        let mut values = 0usize;
        let mut indexes = 0usize;
        for _ in 0..count {
            let start = r.position() - offset;
            let name_marker = r.u32(CTX)?;
            let name = if name_marker == 0 {
                None
            } else {
                Some(read_string(r, input, offset, limits.string_bytes)?)
            };
            let vary = r.u32(CTX)?;
            let property_offset = r.position();
            let properties = r.u32(CTX)?;
            let ty = DataType::from_raw((properties >> 12) & 15)
                .ok_or_else(|| at(ErrorKind::Unsupported, property_offset))?;
            let mut source = [0; 5];
            for word in &mut source {
                *word = r.u32(CTX)?;
            }
            let member = properties & (4 << 16) != 0;
            let n = if member {
                0
            } else {
                match ty {
                    DataType::Void => 0,
                    DataType::Float | DataType::Int | DataType::String => {
                        (properties & 0xfff) as usize
                    }
                    _ => 1,
                }
            };
            values = values
                .checked_add(n)
                .filter(|n| *n <= limits.values)
                .ok_or_else(|| at(ErrorKind::LimitExceeded, property_offset))?;
            let data_start = r.position() - offset;
            let first_string = strings.len();
            if !member && ty == DataType::String {
                if n.checked_add(4).is_none_or(|n| n > r.remaining()) {
                    return Err(at(ErrorKind::UnexpectedEof, r.position()));
                }
                reserve(&mut strings, n, r.position())?;
                for _ in 0..n {
                    strings.push(read_string(r, input, offset, limits.string_bytes)?);
                }
            } else {
                r.take_bytes(
                    n.checked_mul(4)
                        .ok_or_else(|| at(ErrorKind::LimitExceeded, property_offset))?,
                    CTX,
                )?;
            }
            let data = Span {
                start: data_start,
                end: r.position() - offset,
            };
            let parent_offset = r.position() - offset;
            let parent = r.i32(CTX)?;
            let address = !member
                && (matches!(ty, DataType::Prototype | DataType::Instance)
                    || ty == DataType::Function && properties & (1 << 16) != 0);
            indexes = indexes
                .checked_add(usize::from(name.is_some()) * 2 + usize::from(address))
                .filter(|n| *n <= limits.index_items)
                .ok_or_else(|| at(ErrorKind::LimitExceeded, offset + start))?;
            symbols.push(Symbol {
                offset: start,
                name_marker,
                name,
                vary,
                properties,
                source: SourceLocation { words: source },
                data,
                strings: first_string..strings.len(),
                parent,
                parent_offset,
            });
        }
        let code_len_offset = r.position();
        let code_length = r.u32(CTX)? as usize;
        if code_length > limits.code_bytes {
            return Err(at(ErrorKind::LimitExceeded, code_len_offset));
        }
        let code_start = r.position() - offset;
        r.take_bytes(code_length, CTX)?;
        let code = Span {
            start: code_start,
            end: r.position() - offset,
        };
        let consumed = r.position() - offset;
        let mut bytes = Vec::new();
        reserve(&mut bytes, consumed, offset)?;
        bytes.extend_from_slice(&input[..consumed]);
        let owner = NEXT_OWNER
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| at(ErrorKind::LimitExceeded, offset))?;
        let mut script = Self {
            owner,
            offset,
            bytes,
            version,
            sort,
            symbols,
            strings,
            code,
            raw_names: Vec::new(),
            names: Vec::new(),
            addresses: Vec::new(),
        };
        script.build_indexes()?;
        Ok(script)
    }
}
fn read_string(r: &mut Reader<'_>, input: &[u8], offset: usize, limit: usize) -> Result<Span> {
    let start = r.position() - offset;
    let remaining = &input[start..];
    let search = &remaining[..remaining.len().min(limit.saturating_add(1))];
    let len = search
        .iter()
        .position(|b| matches!(b, 0 | 10 | 13))
        .ok_or_else(|| {
            at(
                if remaining.len() > limit {
                    ErrorKind::LimitExceeded
                } else {
                    ErrorKind::UnexpectedEof
                },
                offset + start,
            )
        })?;
    r.take_bytes(len + 1, CTX)?;
    Ok(Span {
        start,
        end: start + len,
    })
}
