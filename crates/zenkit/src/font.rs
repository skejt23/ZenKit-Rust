//! Bounded version-1 FNT definitions. Glyph UVs remain raw floating-point values.
//!
//! The explicit glyph count is retained, including counts other than 256.
//! Invalid/out-of-range UVs, NaNs, infinities and signed zero are retained exactly.
//! No font texture decoding or character-code conversion is implied.
//!
//! ```
//! use zenkit::font::{Font, Glyph};
//! let font = Font { name: b"FONT.TGA".to_vec(), height: 17,
//!     glyphs: vec![Glyph::default(); 256] };
//! let bytes = font.to_bytes(Default::default())?;
//! assert_eq!(Font::from_bytes(&bytes, Default::default())?, font);
//! # Ok::<(), zenkit::Error>(())
//! ```
// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use crate::{
    Error, ErrorKind, Result,
    io::{LineEnding, Reader, Writer},
    values::Vec2,
};
use std::fmt;
const CTX: &str = "font";
fn at(kind: ErrorKind, p: usize) -> Error {
    Error::new(kind, p, CTX)
}
fn bad(kind: ErrorKind) -> Error {
    Error::operation(kind, CTX)
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Glyph {
    pub width: u8,
    pub uv: [Vec2; 2],
}
#[derive(Clone, Default, PartialEq)]
pub struct Font {
    pub name: Vec<u8>,
    pub height: u32,
    pub glyphs: Vec<Glyph>,
}
impl fmt::Debug for Font {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Font")
            .field("name_bytes", &self.name.len())
            .field("height", &self.height)
            .field("glyphs", &self.glyphs.len())
            .finish()
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Complete font size, including header and all width/UV planes.
    pub bytes: usize,
    /// Maximum glyph count; 256 is an engine expectation, not a parser restriction.
    pub glyphs: usize,
    /// Raw font name bytes, excluding LF or CRLF delimiter.
    pub name_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            bytes: 256 * 1024 * 1024,
            glyphs: 65536,
            name_bytes: 1024 * 1024,
        }
    }
}
fn line<'a>(r: &mut Reader<'a>, limit: usize) -> Result<&'a [u8]> {
    let p = r.position();
    let bytes = r.line(LineEnding::Lf, limit.saturating_add(1), CTX)?;
    let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
    if bytes.len() > limit {
        return Err(at(ErrorKind::LimitExceeded, p));
    }
    if bytes.iter().any(|v| matches!(*v, 0 | b'\r')) {
        return Err(at(ErrorKind::InvalidData, p));
    }
    Ok(bytes)
}
impl Font {
    pub fn from_bytes(bytes: &[u8], limits: Limits) -> Result<Self> {
        if bytes.len() > limits.bytes {
            return Err(at(ErrorKind::LimitExceeded, 0));
        }
        let mut r = Reader::new(bytes);
        let font = Self::read(&mut r, limits)?;
        if r.remaining() != 0 {
            return Err(at(ErrorKind::InvalidData, r.position()));
        }
        Ok(font)
    }
    /// Read one font transactionally, leaving any following enclosing-file bytes.
    /// Accept LF/CRLF explicitly; never skip whitespace after either header line.
    pub fn read(input: &mut Reader<'_>, limits: Limits) -> Result<Self> {
        let start = input.position();
        let mut r = input.clone();
        if line(&mut r, 1)? != b"1" {
            return Err(at(ErrorKind::Unsupported, start));
        }
        let name_pos = r.position();
        let name = line(&mut r, limits.name_bytes)?;
        let height = r.u32(CTX)?;
        let count_pos = r.position();
        let count = r.u32(CTX)? as usize;
        if count > limits.glyphs {
            return Err(at(ErrorKind::LimitExceeded, count_pos));
        }
        let payload = count
            .checked_mul(17)
            .ok_or_else(|| at(ErrorKind::LimitExceeded, count_pos))?;
        let size = (r.position() - start)
            .checked_add(payload)
            .ok_or_else(|| at(ErrorKind::LimitExceeded, start))?;
        if size > limits.bytes {
            return Err(at(ErrorKind::LimitExceeded, start));
        }
        if payload > r.remaining() {
            return Err(at(ErrorKind::UnexpectedEof, r.position()));
        }
        let mut owned = Vec::new();
        owned
            .try_reserve(name.len())
            .map_err(|_| at(ErrorKind::LimitExceeded, name_pos))?;
        owned.extend_from_slice(name);
        let mut glyphs = Vec::new();
        glyphs
            .try_reserve_exact(count)
            .map_err(|_| at(ErrorKind::LimitExceeded, count_pos))?;
        for _ in 0..count {
            glyphs.push(Glyph {
                width: r.u8(CTX)?,
                ..Default::default()
            });
        }
        for glyph in &mut glyphs {
            glyph.uv[0] = Vec2::read(&mut r, CTX)?;
        }
        for glyph in &mut glyphs {
            glyph.uv[1] = Vec2::read(&mut r, CTX)?;
        }
        *input = r;
        Ok(Self {
            name: owned,
            height,
            glyphs,
        })
    }
    /// Serialize canonical LF lines. Delimiter/NUL-containing names are rejected;
    /// raw high bytes and leading/trailing spaces remain unchanged.
    pub fn to_bytes(&self, limits: Limits) -> Result<Vec<u8>> {
        if self.name.len() > limits.name_bytes || self.glyphs.len() > limits.glyphs {
            return Err(bad(ErrorKind::LimitExceeded));
        }
        if self.name.iter().any(|v| matches!(*v, 0 | b'\r' | b'\n')) {
            return Err(bad(ErrorKind::Unsupported));
        }
        let count = u32::try_from(self.glyphs.len()).map_err(|_| bad(ErrorKind::LimitExceeded))?;
        let size = self
            .glyphs
            .len()
            .checked_mul(17)
            .and_then(|v| v.checked_add(self.name.len()))
            .and_then(|v| v.checked_add(11))
            .filter(|v| *v <= limits.bytes)
            .ok_or_else(|| bad(ErrorKind::LimitExceeded))?;
        let mut w = Writer::new(size);
        w.line(b"1", LineEnding::Lf, 1, CTX)?;
        w.line(&self.name, LineEnding::Lf, limits.name_bytes, CTX)?;
        w.u32(self.height, CTX)?;
        w.u32(count, CTX)?;
        for glyph in &self.glyphs {
            w.u8(glyph.width, CTX)?;
        }
        for glyph in &self.glyphs {
            glyph.uv[0].write(&mut w, CTX)?;
        }
        for glyph in &self.glyphs {
            glyph.uv[1].write(&mut w, CTX)?;
        }
        Ok(w.into_inner())
    }
}
