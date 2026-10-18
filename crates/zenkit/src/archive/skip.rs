// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;

/// Per-operation work limits for explicit framing-only skipping.
#[derive(Clone, Copy, Debug)]
pub struct SkipLimits {
    pub bytes: usize,
    /// Tagged records and ASCII lines (including empty lines) inspected.
    pub steps: usize,
}
impl Default for SkipLimits {
    fn default() -> Self {
        Self {
            bytes: 256 * 1024 * 1024,
            steps: 1_000_000,
        }
    }
}
enum Token<'a> {
    Begin(ObjectHeader<'a>),
    End,
    Field,
}

fn object_line(v: &[u8], p: usize) -> Result<Option<ObjectHeader<'_>>> {
    if !v.starts_with(b"[") || v == b"[]" {
        return Ok(None);
    }
    let v = v
        .strip_prefix(b"[")
        .and_then(|v| v.strip_suffix(b"]"))
        .ok_or_else(|| invalid(p))?;
    let mut fields = parts(v);
    let (_, name) = fields.next().ok_or_else(|| invalid(p))?;
    let (_, class) = fields.next().ok_or_else(|| invalid(p))?;
    let (offset, value) = fields.next().ok_or_else(|| invalid(p))?;
    let version = number(value, p + offset + 1)?;
    let (offset, value) = fields.next().ok_or_else(|| invalid(p))?;
    let index = number(value, p + offset + 1)?;
    if fields.next().is_some() || !token(name) || !token(class) {
        return Err(invalid(p));
    }
    Ok(Some(ObjectHeader {
        object_name: name,
        class_name: class,
        version,
        index,
    }))
}
impl<'a> Reader<'a> {
    fn skip_token(
        &self,
        r: &mut Bytes<'a>,
        steps: &mut usize,
        limit: usize,
        allow_unterminated: bool,
        only_begin: bool,
    ) -> Result<Token<'a>> {
        loop {
            let p = r.position();
            if only_begin && allow_unterminated && r.remaining() == 0 {
                return Ok(Token::Field);
            }
            if *steps >= limit {
                return Err(limited(p));
            }
            *steps += 1;
            let (v, p) = if self.header.format == Format::Ascii {
                let raw = match line(r, self.limits.field_bytes) {
                    Ok(v) => v,
                    Err(e)
                        if allow_unterminated
                            && e.kind == ErrorKind::UnexpectedEof
                            && r.remaining() != 0 =>
                    {
                        r.fixed_bytes(r.remaining(), self.limits.field_bytes, CTX)?
                    }
                    Err(e) => return Err(e),
                };
                let v = raw.trim_ascii_start();
                if v.is_empty() {
                    continue;
                }
                if !v.starts_with(b"[") {
                    if only_begin {
                        return Ok(Token::Field);
                    }
                    let equal = v
                        .iter()
                        .position(|b| *b == b'=')
                        .ok_or_else(|| invalid(p))?;
                    let colon = v[equal + 1..]
                        .iter()
                        .position(|b| *b == b':')
                        .ok_or_else(|| invalid(p + equal + 1))?;
                    if equal == 0 || colon == 0 {
                        return Err(invalid(p));
                    }
                    return Ok(Token::Field);
                }
                (v, p + raw.len() - v.len())
            } else {
                let tag = r.u8(CTX)?;
                if only_begin && tag != 1 {
                    return Ok(Token::Field);
                }
                match tag {
                    18 => {
                        let key_at = r.position();
                        let key = r.u32(CTX)? as usize;
                        if key >= self.keys.len() {
                            return Err(invalid(key_at));
                        }
                        let tag_at = r.position();
                        let n = match r.u8(CTX)? {
                            1 | 9 | 16 => {
                                let at = r.position();
                                let n = r.u16(CTX)? as usize;
                                if n > self.limits.field_bytes {
                                    return Err(limited(at));
                                }
                                n
                            }
                            2 | 3 | 6 | 8 | 17 => 4,
                            4 => 1,
                            5 => 2,
                            7 => 12,
                            _ => {
                                return Err(Error::new(
                                    ErrorKind::Unsupported,
                                    tag_at,
                                    "unknown skipped value tag",
                                ));
                            }
                        };
                        r.take_bytes(n, CTX)?;
                        return Ok(Token::Field);
                    }
                    1 => {
                        let n = r.u16(CTX)? as usize;
                        let at = r.position();
                        (r.fixed_bytes(n, self.limits.field_bytes, CTX)?, at)
                    }
                    _ => return Err(invalid(p)),
                }
            };
            if v == b"[]" {
                return Ok(Token::End);
            }
            return object_line(v, p)?
                .map(Token::Begin)
                .ok_or_else(|| invalid(p));
        }
    }
    fn skip_window(&self, limit: usize) -> Result<(Bytes<'a>, bool)> {
        let mut candidate = self.candidate()?;
        let capped = candidate.remaining() > limit;
        let n = candidate.remaining().min(limit);
        Ok((candidate.substream(n, CTX)?, capped))
    }
    fn skip_error(mut error: Error, capped: bool) -> Error {
        if capped && error.kind == ErrorKind::UnexpectedEof {
            error.kind = ErrorKind::LimitExceeded;
            error
        } else {
            error
        }
    }
    /// Skip one framed tagged field without decoding its payload. Unknown ASCII
    /// type names are permitted; unknown BIN_SAFE type tags are not frameable.
    /// BINARY fields have no type/length tags and return Unsupported unchanged.
    pub fn skip_entry(&mut self, limits: SkipLimits) -> Result<()> {
        if self.header.format == Format::Binary {
            return Err(Error::new(
                ErrorKind::Unsupported,
                self.position(),
                "BINARY field skipping",
            ));
        }
        let (mut r, capped) = self.skip_window(limits.bytes)?;
        match self
            .skip_token(&mut r, &mut 0, limits.steps, !capped, false)
            .map_err(|e| Self::skip_error(e, capped))?
        {
            Token::Field => self.commit(&r),
            _ => Err(invalid(self.position())),
        }
    }
    /// Skip the next complete object. Tagged fields/end markers return None
    /// without advancing. BINARY requires the caller to know an object follows.
    /// Opaque BINARY descendants cannot be counted or schema-validated.
    pub fn skip_next_object(&mut self, limits: SkipLimits) -> Result<Option<ObjectHeader<'a>>> {
        self.skip_object_impl(false, limits)
    }
    /// Skip the remainder of the current object and close it. Errors preserve
    /// cursor, frame stack and record count. No graph identities are registered.
    pub fn skip_current_object(&mut self, limits: SkipLimits) -> Result<()> {
        if self.ends.is_empty() {
            return Err(invalid(self.position()));
        }
        self.skip_object_impl(true, limits).map(|_| ())
    }
    fn skip_object_impl(
        &mut self,
        current: bool,
        limits: SkipLimits,
    ) -> Result<Option<ObjectHeader<'a>>> {
        let p = self.position();
        if limits.steps == 0 {
            return Err(limited(p));
        }
        if self.header.format == Format::Binary {
            let mut r = self.candidate()?;
            let mut header = None;
            if !current {
                if r.remaining() == 0 {
                    return Ok(None);
                }
                let n = r.u32(CTX)? as usize;
                if n < 12 || n - 4 > r.remaining() {
                    return Err(invalid(p));
                }
                if n > limits.bytes
                    || self.records >= self.limits.objects
                    || self.ends.len() >= self.limits.depth
                {
                    return Err(limited(p));
                }
                let mut body = r.substream(n - 4, CTX)?;
                let version = body.u16(CTX)?;
                let index = body.u32(CTX)?;
                let object_name = body.terminated(0, self.limits.field_bytes, CTX)?;
                let class_name = body.terminated(0, self.limits.field_bytes, CTX)?;
                header = Some(ObjectHeader {
                    version,
                    index,
                    object_name,
                    class_name,
                });
            } else {
                if r.remaining() > limits.bytes {
                    return Err(limited(p));
                }
                r.take_bytes(r.remaining(), CTX)?;
            }
            self.commit(&r)?;
            if current {
                self.ends.pop();
            } else {
                self.records += 1;
            }
            return Ok(header);
        }
        if !current && self.remaining_bytes() == 0 {
            return Ok(None);
        }
        let (mut r, capped) = self.skip_window(limits.bytes)?;
        let mut steps = 0;
        let mut depth = usize::from(current);
        let mut records = self.records;
        let mut first = None;
        loop {
            let at = r.position();
            let token = self
                .skip_token(
                    &mut r,
                    &mut steps,
                    limits.steps,
                    !capped,
                    !current && depth == 0,
                )
                .map_err(|e| Self::skip_error(e, capped))?;
            if !current && depth == 0 && !matches!(token, Token::Begin(_)) {
                return Ok(None);
            }
            match token {
                Token::Begin(h) => {
                    let existing = self.ends.len() - usize::from(current);
                    if existing + depth >= self.limits.depth || records >= self.limits.objects {
                        return Err(limited(at));
                    }
                    if !current && depth == 0 {
                        first = Some(h);
                    }
                    depth += 1;
                    records += 1;
                }
                Token::Field => {}
                Token::End => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
        }
        self.commit(&r)?;
        self.records = records;
        if current {
            self.ends.pop();
        }
        Ok(first)
    }
}
