// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2023-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Word,
    Number,
    String,
    Left,
    Right,
    Open,
    Close,
    Colon,
    Eof,
}
#[derive(Clone, Copy)]
struct Token<'a> {
    kind: Kind,
    raw: &'a [u8],
    offset: usize,
    after_close: bool,
}
struct Parser<'a, 'b, 'input> {
    bytes: &'a [u8],
    base: usize,
    pos: usize,
    look: Option<Token<'a>>,
    mode: SyntaxMode,
    limits: Limits,
    b: &'b mut Budget<'input>,
    diagnostics: Vec<Diagnostic>,
    tokens: usize,
    depth: usize,
}
impl<'a> Parser<'a, '_, '_> {
    fn diagnostic(&mut self, offset: usize, recovery: Recovery) -> Result<()> {
        push(
            &mut self.diagnostics,
            Diagnostic { offset, recovery },
            self.b,
            offset,
        )?;
        crate::logging::emit(
            crate::logging::Level::Warn,
            "zenkit::model_script",
            format_args!("source recovery at byte {offset}: {recovery:?}"),
        );
        Ok(())
    }
    fn lex(&mut self) -> Result<Token<'a>> {
        let mut after_close = false;
        loop {
            while self.pos < self.bytes.len()
                && (space(self.bytes[self.pos])
                    || self.bytes[self.pos] == b';'
                    || self.mode == SyntaxMode::Legacy
                        && matches!(self.bytes[self.pos], b'(' | b')'))
            {
                if self.bytes[self.pos] == b')' {
                    after_close = true;
                }
                if matches!(self.bytes[self.pos], b'(' | b')') {
                    if self.tokens >= self.limits.tokens {
                        return Err(at(ErrorKind::LimitExceeded, self.base + self.pos));
                    }
                    self.tokens += 1;
                }
                self.pos += 1;
            }
            if self.pos == self.bytes.len() {
                return Ok(Token {
                    kind: Kind::Eof,
                    raw: &[],
                    offset: self.base + self.pos,
                    after_close,
                });
            }
            let start = self.pos;
            let offset = self.base + start;
            let c = self.bytes[self.pos];
            self.pos += 1;
            if c == b'/' {
                if self.bytes.get(self.pos) == Some(&b'/') {
                    self.pos += 1;
                } else if self.mode == SyntaxMode::Legacy {
                    self.diagnostic(offset, Recovery::SingleSlashComment)?;
                } else {
                    return Err(at(ErrorKind::InvalidData, offset));
                }
                while self.pos < self.bytes.len() && !matches!(self.bytes[self.pos], b'\r' | b'\n')
                {
                    self.pos += 1;
                }
                continue;
            }
            if self.tokens >= self.limits.tokens {
                return Err(at(ErrorKind::LimitExceeded, offset));
            }
            self.tokens += 1;
            let mut raw_start = start;
            let mut raw_end = self.pos;
            let kind = match c {
                b'{' => Kind::Left,
                b'}' => Kind::Right,
                b'(' => Kind::Open,
                b')' => Kind::Close,
                b':' => Kind::Colon,
                b'"' => {
                    raw_start = self.pos;
                    while self.pos < self.bytes.len()
                        && !matches!(self.bytes[self.pos], b'"' | b'\r' | b'\n')
                    {
                        self.pos += 1;
                    }
                    raw_end = self.pos;
                    if self.bytes.get(self.pos) == Some(&b'"') {
                        self.pos += 1;
                    } else {
                        if self.mode == SyntaxMode::Strict {
                            return Err(at(ErrorKind::InvalidData, offset));
                        }
                        self.diagnostic(offset, Recovery::MissingQuote)?;
                        let last = self.bytes[raw_start..raw_end]
                            .iter()
                            .rposition(|&v| !space(v))
                            .map(|i| raw_start + i);
                        if last.is_some_and(|i| self.bytes[i] == b')') {
                            raw_end = last.unwrap();
                            self.diagnostic(offset, Recovery::ClosingParenthesisInUnclosedString)?;
                        }
                    }
                    Kind::String
                }
                b'0'..=b'9' | b'-' | b'+' => {
                    while self.pos < self.bytes.len()
                        && (self.bytes[self.pos].is_ascii_alphanumeric()
                            || matches!(self.bytes[self.pos], b'.' | b'+' | b'-'))
                    {
                        self.pos += 1;
                    }
                    raw_end = self.pos;
                    Kind::Number
                }
                b'.' if self.bytes.get(self.pos).is_some_and(u8::is_ascii_digit) => {
                    while self.pos < self.bytes.len()
                        && (self.bytes[self.pos].is_ascii_alphanumeric()
                            || matches!(self.bytes[self.pos], b'.' | b'+' | b'-'))
                    {
                        self.pos += 1;
                    }
                    raw_end = self.pos;
                    Kind::Number
                }
                _ if c.is_ascii_alphabetic() || matches!(c, b'*' | b'_' | b'.') => {
                    while self.pos < self.bytes.len()
                        && (self.bytes[self.pos].is_ascii_alphanumeric()
                            || matches!(self.bytes[self.pos], b'_' | b'-' | b'.'))
                    {
                        self.pos += 1;
                    }
                    raw_end = self.pos;
                    Kind::Word
                }
                _ => return Err(at(ErrorKind::InvalidData, offset)),
            };
            if raw_end - raw_start > self.limits.string_bytes {
                return Err(at(ErrorKind::LimitExceeded, offset));
            }
            return Ok(Token {
                kind,
                raw: &self.bytes[raw_start..raw_end],
                offset,
                after_close,
            });
        }
    }
    fn peek(&mut self) -> Result<Token<'a>> {
        if let Some(t) = self.look {
            return Ok(t);
        }
        let t = self.lex()?;
        self.look = Some(t);
        Ok(t)
    }
    fn next(&mut self) -> Result<Token<'a>> {
        if let Some(t) = self.look.take() {
            Ok(t)
        } else {
            self.lex()
        }
    }
    fn expect(&mut self, kind: Kind) -> Result<Token<'a>> {
        let t = self.next()?;
        if t.kind != kind {
            return Err(at(
                if t.kind == Kind::Eof {
                    ErrorKind::UnexpectedEof
                } else {
                    ErrorKind::InvalidData
                },
                t.offset,
            ));
        }
        Ok(t)
    }
    fn maybe(&mut self, kind: Kind) -> Result<bool> {
        if self.peek()?.kind == kind {
            self.next()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    fn keyword(&mut self, value: &[u8]) -> Result<bool> {
        let t = self.peek()?;
        if t.kind == Kind::Word && t.raw.eq_ignore_ascii_case(value) {
            self.next()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    fn expect_keyword(&mut self, value: &[u8]) -> Result<()> {
        let t = self.expect(Kind::Word)?;
        if !t.raw.eq_ignore_ascii_case(value) {
            return Err(at(ErrorKind::Unsupported, t.offset));
        }
        Ok(())
    }
    fn string(&mut self) -> Result<Vec<u8>> {
        let t = self.expect(Kind::String)?;
        self.b.copy(t.raw, Some(t.offset + 1))
    }
    fn optional_string(&mut self) -> Result<Option<Vec<u8>>> {
        if self.peek()?.kind == Kind::String {
            Ok(Some(self.string()?))
        } else {
            Ok(None)
        }
    }
    fn integer(&mut self) -> Result<i32> {
        let t = self.expect(Kind::Number)?;
        parse_int(t.raw).map_err(|_| at(ErrorKind::InvalidData, t.offset))
    }
    fn layer(&mut self) -> Result<u32> {
        let t = self.expect(Kind::Number)?;
        std::str::from_utf8(t.raw)
            .ok()
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| at(ErrorKind::InvalidData, t.offset))
    }
    fn optional_int(&mut self) -> Result<Option<i32>> {
        if self.peek()?.kind == Kind::Number {
            Ok(Some(self.integer()?))
        } else {
            Ok(None)
        }
    }
    fn number(&mut self) -> Result<f32> {
        let t = self.expect(Kind::Number)?;
        let value = std::str::from_utf8(t.raw)
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|v| v.is_finite());
        value.ok_or_else(|| at(ErrorKind::InvalidData, t.offset))
    }
    fn optional_number(&mut self) -> Result<Option<f32>> {
        if self.peek()?.kind == Kind::Number {
            Ok(Some(self.number()?))
        } else {
            Ok(None)
        }
    }
    fn named(&mut self, name: &[u8]) -> Result<Option<f32>> {
        if self.keyword(name)? {
            self.expect(Kind::Colon)?;
            Ok(Some(self.number()?))
        } else {
            Ok(None)
        }
    }
    fn flags(&mut self) -> Result<Flags> {
        let t = self.expect(Kind::Word)?;
        let raw = self.b.copy(t.raw, Some(t.offset))?;
        self.maybe(Kind::Colon)?;
        Ok(Flags(raw))
    }
    fn direction(&mut self, optional: bool) -> Result<Direction> {
        let t = self.peek()?;
        if t.kind == Kind::Word
            && (t.raw.eq_ignore_ascii_case(b"F") || t.raw.eq_ignore_ascii_case(b"R"))
        {
            self.next()?;
            Ok(Direction(self.b.copy(t.raw, Some(t.offset))?))
        } else if optional {
            Ok(Direction(self.b.copy(b"F", None)?))
        } else {
            Err(at(ErrorKind::InvalidData, t.offset))
        }
    }
    fn open(&mut self) -> Result<()> {
        if self.mode == SyntaxMode::Strict {
            self.expect(Kind::Open)?;
        }
        Ok(())
    }
    fn close(&mut self) -> Result<()> {
        if self.mode == SyntaxMode::Strict {
            self.expect(Kind::Close)?;
        }
        Ok(())
    }
    fn enter(&mut self) -> Result<()> {
        let t = self.expect(Kind::Left)?;
        if self.depth >= self.limits.depth {
            return Err(at(ErrorKind::LimitExceeded, t.offset));
        }
        self.depth += 1;
        Ok(())
    }
    fn done(&mut self) -> Result<bool> {
        let t = self.peek()?;
        if t.kind == Kind::Right {
            self.next()?;
            self.depth -= 1;
            return Ok(true);
        }
        if t.kind == Kind::Eof {
            if self.mode == SyntaxMode::Strict {
                return Err(at(ErrorKind::UnexpectedEof, t.offset));
            }
            self.diagnostic(t.offset, Recovery::MissingBrace)?;
            self.depth -= 1;
            return Ok(true);
        }
        Ok(false)
    }
    fn common(&mut self) -> Result<Common> {
        Ok(Common {
            name: self.string()?,
            layer: self.layer()?,
            next: self.string()?,
            blend_in: self.number()?,
            blend_out: self.number()?,
            flags: self.flags()?,
        })
    }
    fn events(&mut self) -> Result<Vec<Event>> {
        let braced = self.peek()?.kind == Kind::Left;
        let mut events = vec![];
        if braced {
            self.enter()?;
        } else if self.mode == SyntaxMode::Strict {
            return Ok(events);
        } else if self.peek()?.kind == Kind::Word && self.peek()?.raw.starts_with(b"*") {
            let p = self.peek()?.offset;
            self.diagnostic(p, Recovery::UnbracedEvents)?;
        }
        loop {
            if braced && self.done()? {
                break;
            }
            let t = self.peek()?;
            if !braced && (t.kind != Kind::Word || !t.raw.starts_with(b"*")) {
                break;
            }
            let kw = self.expect(Kind::Word)?;
            let event = self.event(kw)?;
            push(&mut events, event, self.b, kw.offset)?;
        }
        Ok(events)
    }
    fn event(&mut self, kw: Token<'a>) -> Result<Event> {
        self.open()?;
        let event = if kw.raw.eq_ignore_ascii_case(b"*eventTag") {
            let frame = self.optional_int()?.unwrap_or(0);
            let name = self.string()?;
            let p = self.peek()?.offset + 1;
            let mut arguments = vec![];
            while self.peek()?.kind == Kind::String {
                let at = self.peek()?.offset;
                let v = self.string()?;
                push(&mut arguments, v, self.b, at)?;
            }
            let attached = self.keyword(b"ATTACH")?;
            let tag = Tag {
                frame,
                name,
                arguments,
                attached,
            };
            validate_tag(&tag, p)?;
            Event::Tag(tag)
        } else if kw.raw.eq_ignore_ascii_case(b"*eventSFX")
            || kw.raw.eq_ignore_ascii_case(b"*eventSFXGrnd")
        {
            let ground = kw.raw.eq_ignore_ascii_case(b"*eventSFXGrnd");
            let frame = self.integer()?;
            let name = self.string()?;
            let range = if ground {
                1000.
            } else {
                self.named(b"R")?.unwrap_or(1000.)
            };
            let mut empty_slot = self.keyword(b"EMPTY_SLOT")?;
            if !empty_slot {
                let p = self.peek()?.offset;
                if self.keyword(b"EMTPY_SLOT")? {
                    if self.mode == SyntaxMode::Strict {
                        return Err(at(ErrorKind::InvalidData, p));
                    }
                    self.diagnostic(p, Recovery::MisspelledEmptySlot)?;
                    empty_slot = true;
                }
            }
            let s = Sound {
                frame,
                name,
                range,
                empty_slot: Bool32(u32::from(empty_slot)),
            };
            if ground {
                Event::GroundSound(s)
            } else {
                Event::Sound(s)
            }
        } else if kw.raw.eq_ignore_ascii_case(b"*eventPFX") {
            let frame = self.integer()?;
            let index = self.optional_int()?.unwrap_or(0);
            let name = self.string()?;
            let position = self.string()?;
            let mut attached = self.keyword(b"ATTACH")?;
            let mut attachment_literal = None;
            if !attached && self.peek()?.kind == Kind::String {
                let p = self.peek()?.offset;
                let v = self.string()?;
                if self.mode == SyntaxMode::Strict && !v.eq_ignore_ascii_case(b"ATTACH") {
                    return Err(at(ErrorKind::InvalidData, p));
                }
                self.diagnostic(p, Recovery::QuotedAttachment)?;
                attached = true;
                attachment_literal = Some(v);
            }
            Event::Particle(Particle {
                frame,
                index,
                name,
                position,
                attached: Bool32(u32::from(attached)),
                attachment_literal,
            })
        } else if kw.raw.eq_ignore_ascii_case(b"*eventPFXStop") {
            Event::ParticleStop(ParticleStop {
                frame: self.integer()?,
                index: self.integer()?,
            })
        } else if kw.raw.eq_ignore_ascii_case(b"*eventMMStartAni") {
            Event::Morph(Morph {
                frame: self.integer()?,
                animation: self.string()?,
                node: self.optional_string()?.unwrap_or_default(),
                compiled_values: None,
            })
        } else if kw.raw.eq_ignore_ascii_case(b"*eventCamTremor") {
            Event::CameraTremor(CameraTremor {
                frame: self.integer()?,
                fields: [
                    self.integer()?,
                    self.integer()?,
                    self.integer()?,
                    self.integer()?,
                ],
            })
        } else {
            return Err(at(ErrorKind::Unsupported, kw.offset));
        };
        self.close()?;
        Ok(event)
    }
    fn enumeration(&mut self, s: &mut ModelScript) -> Result<()> {
        self.enter()?;
        while !self.done()? {
            let kw = self.expect(Kind::Word)?;
            self.open()?;
            if kw.raw.eq_ignore_ascii_case(b"ani") {
                let common = self.common()?;
                let model = self.string()?;
                let direction = self.direction(false)?;
                let first_frame = self.integer()?;
                let last_frame = self.integer()?;
                let mut fps = None;
                let mut cvs = None;
                loop {
                    let p = self.peek()?.offset;
                    if self.keyword(b"FPS")? {
                        if fps.is_some() {
                            return Err(at(ErrorKind::InvalidData, p));
                        }
                        self.expect(Kind::Colon)?;
                        fps = Some(self.number()?);
                    } else if self.keyword(b"CVS")? {
                        if cvs.is_some() {
                            return Err(at(ErrorKind::InvalidData, p));
                        }
                        self.expect(Kind::Colon)?;
                        cvs = Some(self.number()?);
                    } else {
                        break;
                    }
                }
                self.close()?;
                let events = self.events()?;
                push(
                    &mut s.animations,
                    Animation {
                        common,
                        model,
                        direction,
                        first_frame,
                        last_frame,
                        fps: fps.unwrap_or(25.),
                        speed: 0.,
                        collision_volume_scale: cvs.unwrap_or(1.),
                        events,
                    },
                    self.b,
                    kw.offset,
                )?;
            } else if kw.raw.eq_ignore_ascii_case(b"aniAlias") {
                let common = self.common()?;
                let alias = self.string()?;
                let direction = self.direction(true)?;
                self.close()?;
                push(
                    &mut s.aliases,
                    Alias {
                        common,
                        alias,
                        direction,
                    },
                    self.b,
                    kw.offset,
                )?;
            } else if kw.raw.eq_ignore_ascii_case(b"aniBlend") {
                let name = self.string()?;
                let layer = self.optional_int()?;
                let next = self.string()?;
                let blend_in = self.optional_number()?.unwrap_or(0.);
                let blend_out = self.optional_number()?.unwrap_or(0.);
                let t = self.peek()?;
                let flags = if t.kind == Kind::Word
                    && !t.after_close
                    && !t.raw.starts_with(b"*")
                    && (self.mode == SyntaxMode::Strict
                        || t.raw.iter().all(|c| b"MREFIP.".contains(c)))
                {
                    Some(self.flags()?)
                } else {
                    None
                };
                self.close()?;
                let events = self.events()?;
                push(
                    &mut s.blends,
                    Blend {
                        name,
                        next,
                        blend_in,
                        blend_out,
                        layer,
                        flags,
                        events,
                    },
                    self.b,
                    kw.offset,
                )?;
            } else if kw.raw.eq_ignore_ascii_case(b"aniComb") {
                let common = self.common()?;
                let model = self.string()?;
                let last_frame = self.integer()?;
                self.close()?;
                let events = self.events()?;
                push(
                    &mut s.combinations,
                    Combination {
                        common,
                        model,
                        last_frame,
                        events,
                    },
                    self.b,
                    kw.offset,
                )?;
            } else if kw.raw.eq_ignore_ascii_case(b"aniDisable") {
                let v = self.string()?;
                self.close()?;
                push(&mut s.disabled_animations, v, self.b, kw.offset)?;
            } else if kw.raw.eq_ignore_ascii_case(b"modelTag") {
                let name = self.string()?;
                let bone = self.string()?;
                self.close()?;
                push(
                    &mut s.model_tags,
                    ModelTag {
                        frame: None,
                        name,
                        bone,
                    },
                    self.b,
                    kw.offset,
                )?;
            } else {
                return Err(at(ErrorKind::Unsupported, kw.offset));
            }
        }
        Ok(())
    }
    fn script(&mut self) -> Result<ModelScript> {
        let mut s = ModelScript::default();
        self.expect_keyword(b"Model")?;
        self.open()?;
        s.model_name = Some(self.string()?);
        self.close()?;
        self.enter()?;
        while !self.done()? {
            let kw = self.expect(Kind::Word)?;
            if kw.raw.eq_ignore_ascii_case(b"meshAndTree") {
                if s.skeleton.is_some() {
                    return Err(at(ErrorKind::InvalidData, kw.offset));
                }
                self.open()?;
                let name = self.string()?;
                let disable_mesh = Bool32(u32::from(self.keyword(b"DONT_USE_MESH")?));
                self.close()?;
                s.skeleton = Some(Skeleton { name, disable_mesh });
            } else if kw.raw.eq_ignore_ascii_case(b"registerMesh") {
                self.open()?;
                let v = self.string()?;
                self.close()?;
                push(&mut s.meshes, v, self.b, kw.offset)?;
            } else if kw.raw.eq_ignore_ascii_case(b"aniEnum") {
                self.enumeration(&mut s)?;
            } else {
                return Err(at(ErrorKind::Unsupported, kw.offset));
            }
        }
        let t = self.next()?;
        if t.kind != Kind::Eof {
            return Err(at(ErrorKind::Unsupported, t.offset));
        }
        s.diagnostics = std::mem::take(&mut self.diagnostics);
        Ok(s)
    }
}
pub(super) fn read(
    bytes: &[u8],
    base: usize,
    mode: SyntaxMode,
    limits: Limits,
    b: &mut Budget,
) -> Result<ModelScript> {
    Parser {
        bytes,
        base,
        pos: 0,
        look: None,
        mode,
        limits,
        b,
        diagnostics: vec![],
        tokens: 0,
        depth: 0,
    }
    .script()
}
