// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;
use crate::{archive, values::Color};

fn whitespace(b: u8) -> bool {
    b.is_ascii_whitespace() || b == 11
}
fn skip(bytes: &[u8], p: &mut usize) {
    while bytes.get(*p).is_some_and(|b| whitespace(*b)) {
        *p += 1;
    }
}
fn fail(p: usize) -> Error {
    Error::new(ErrorKind::InvalidData, p, "light animation text")
}
fn range(bytes: &[u8], p: usize) -> Result<f32> {
    let text = std::str::from_utf8(bytes).map_err(|e| fail(p + e.valid_up_to()))?;
    let v = text.parse::<f32>().map_err(|_| fail(p))?;
    if !v.is_finite() {
        return Err(Error::new(
            ErrorKind::Unsupported,
            p,
            "nonfinite light range text",
        ));
    }
    Ok(v)
}
fn number(bytes: &[u8], p: &mut usize) -> Result<u8> {
    let start = *p;
    let mut value = 0u16;
    while let Some(b) = bytes.get(*p).filter(|b| b.is_ascii_digit()) {
        value = value * 10 + u16::from(*b - b'0');
        if value > 255 {
            return Err(fail(*p));
        }
        *p += 1;
    }
    if start == *p {
        return Err(fail(start));
    }
    Ok(value as u8)
}
fn color(bytes: &[u8], p: &mut usize) -> Result<Option<Color>> {
    skip(bytes, p);
    let Some(b) = bytes.get(*p) else {
        return Ok(None);
    };
    if *b != b'(' {
        let c = number(bytes, p)?;
        return Ok(Some(Color::from_rgba([c, c, c, 255])));
    }
    *p += 1;
    skip(bytes, p);
    let r = number(bytes, p)?;
    let before = *p;
    skip(bytes, p);
    if *p == before {
        return Err(fail(*p));
    }
    let g = number(bytes, p)?;
    let before = *p;
    skip(bytes, p);
    if *p == before {
        return Err(fail(*p));
    }
    let b = number(bytes, p)?;
    skip(bytes, p);
    if bytes.get(*p) != Some(&b')') {
        return Err(fail(*p));
    }
    *p += 1;
    Ok(Some(Color::from_rgba([r, g, b, 255])))
}
macro_rules! raw_text {
    ($name:ident) => {
        /// Lossless raw light-animation spelling, including malformed text.
        /// Parsing is explicit through `decoded`; errors are byte offsets relative
        /// to this string. Whole-archive read/write never silently replaces it.
        #[derive(Clone, Default, PartialEq, Eq)]
        pub struct $name(Vec<u8>);
        impl $name {
            pub fn from_bytes(bytes: Vec<u8>) -> Self {
                Self(bytes)
            }
            pub fn as_bytes(&self) -> &[u8] {
                &self.0
            }
            pub fn into_bytes(self) -> Vec<u8> {
                self.0
            }
        }
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($name))
                    .field("bytes", &self.0.len())
                    .finish()
            }
        }
    };
}
raw_text!(RangeAnimation);
raw_text!(ColorAnimation);
/// A validated immutable range view. Construction validates all tokens before
/// yielding any value; classic ASCII whitespace includes vertical tab.
#[derive(Clone)]
pub struct RangeFrames<'a> {
    bytes: &'a [u8],
    front: usize,
    back: usize,
    remaining: usize,
}
impl fmt::Debug for RangeFrames<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RangeFrames")
            .field("remaining", &self.remaining)
            .finish()
    }
}
impl Iterator for RangeFrames<'_> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.remaining == 0 {
            return None;
        }
        while self.front < self.back && whitespace(self.bytes[self.front]) {
            self.front += 1;
        }
        let start = self.front;
        while self.front < self.back && !whitespace(self.bytes[self.front]) {
            self.front += 1;
        }
        self.remaining -= 1;
        Some(range(&self.bytes[start..self.front], start).expect("immutable validated tokens"))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}
impl DoubleEndedIterator for RangeFrames<'_> {
    fn next_back(&mut self) -> Option<f32> {
        if self.remaining == 0 {
            return None;
        }
        while self.back > self.front && whitespace(self.bytes[self.back - 1]) {
            self.back -= 1;
        }
        let end = self.back;
        while self.back > self.front && !whitespace(self.bytes[self.back - 1]) {
            self.back -= 1;
        }
        self.remaining -= 1;
        Some(range(&self.bytes[self.back..end], self.back).expect("immutable validated tokens"))
    }
}
impl ExactSizeIterator for RangeFrames<'_> {}
impl std::iter::FusedIterator for RangeFrames<'_> {}
/// Validated greyscale/(R G B) frames, with opaque alpha. No decoded allocation.
#[derive(Clone)]
pub struct ColorFrames<'a> {
    bytes: &'a [u8],
    position: usize,
    remaining: usize,
}
impl fmt::Debug for ColorFrames<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ColorFrames")
            .field("remaining", &self.remaining)
            .finish()
    }
}
impl Iterator for ColorFrames<'_> {
    type Item = Color;
    fn next(&mut self) -> Option<Color> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        color(self.bytes, &mut self.position).expect("immutable validated tokens")
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}
impl ExactSizeIterator for ColorFrames<'_> {}
impl std::iter::FusedIterator for ColorFrames<'_> {}
fn size(total: &mut usize, amount: usize, limit: usize) -> Result<()> {
    *total = total
        .checked_add(amount)
        .filter(|v| *v <= limit)
        .ok_or_else(|| Error::operation(ErrorKind::LimitExceeded, "light animation bytes"))?;
    Ok(())
}
fn storage(n: usize) -> Result<Vec<u8>> {
    let mut v = Vec::new();
    v.try_reserve_exact(n)
        .map_err(|_| Error::operation(ErrorKind::LimitExceeded, "light animation allocation"))?;
    Ok(v)
}
impl RangeAnimation {
    /// Validate finite, whitespace-delimited `f32` tokens, then borrow a view.
    /// Classic ASCII whitespace includes vertical tab. Malformed/nonfinite text
    /// remains losslessly stored, but this view reports its relative byte offset.
    pub fn decoded(&self) -> Result<RangeFrames<'_>> {
        let mut p = 0;
        let mut count = 0;
        loop {
            skip(&self.0, &mut p);
            if p == self.0.len() {
                break;
            }
            let start = p;
            while p < self.0.len() && !whitespace(self.0[p]) {
                p += 1;
            }
            range(&self.0[start..p], start)?;
            count += 1;
        }
        Ok(RangeFrames {
            bytes: &self.0,
            front: 0,
            back: self.0.len(),
            remaining: count,
        })
    }
    /// Canonical finite f32 spelling with exact finite round trips. Preflights
    /// the complete byte count before allocating the output buffer; no implicit
    /// truncation. Formatting individual values uses small temporary strings.
    pub fn from_values(values: &[f32], max_bytes: usize) -> Result<Self> {
        let mut total = 0;
        for (i, v) in values.iter().enumerate() {
            if !v.is_finite() {
                return Err(Error::operation(
                    ErrorKind::Unsupported,
                    "nonfinite light range",
                ));
            }
            size(
                &mut total,
                v.to_string().len() + usize::from(i != 0),
                max_bytes,
            )?;
        }
        let mut bytes = storage(total)?;
        for (i, v) in values.iter().enumerate() {
            if i != 0 {
                bytes.push(b' ');
            }
            bytes.extend_from_slice(v.to_string().as_bytes());
        }
        Ok(Self(bytes))
    }
    /// Replace only after complete validation/allocation; failure leaves bytes intact.
    pub fn set_values(&mut self, values: &[f32], max_bytes: usize) -> Result<()> {
        *self = Self::from_values(values, max_bytes)?;
        Ok(())
    }
}
impl ColorAnimation {
    /// Validate decimal greyscale values or `(R G B)` tuples, with each channel
    /// in `0..=255` and implicit opaque alpha. Classic ASCII whitespace includes
    /// vertical tab. The entire string is checked before any frame is exposed.
    pub fn decoded(&self) -> Result<ColorFrames<'_>> {
        let mut p = 0;
        let mut count = 0;
        while color(&self.0, &mut p)?.is_some() {
            count += 1;
        }
        Ok(ColorFrames {
            bytes: &self.0,
            position: 0,
            remaining: count,
        })
    }
    /// Canonical RGB tuples. Nonopaque colors fail because the wire grammar has
    /// no alpha component; callers must explicitly choose any alpha conversion.
    pub fn from_values(values: &[Color], max_bytes: usize) -> Result<Self> {
        let mut total = 0;
        for (i, v) in values.iter().enumerate() {
            let [r, g, b, a] = v.to_rgba();
            if a != 255 {
                return Err(Error::operation(
                    ErrorKind::Unsupported,
                    "light color text has no alpha",
                ));
            }
            size(
                &mut total,
                format!("({r} {g} {b})").len() + usize::from(i != 0),
                max_bytes,
            )?;
        }
        let mut bytes = storage(total)?;
        for (i, v) in values.iter().enumerate() {
            let [r, g, b, _] = v.to_rgba();
            if i != 0 {
                bytes.push(b' ');
            }
            bytes.extend_from_slice(format!("({r} {g} {b})").as_bytes());
        }
        Ok(Self(bytes))
    }
    pub fn set_values(&mut self, values: &[Color], max_bytes: usize) -> Result<()> {
        *self = Self::from_values(values, max_bytes)?;
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct LightAnimation {
    pub on: bool,
    pub range_scale: RangeAnimation,
    pub range_fps: f32,
    pub range_smooth: bool,
    pub colors: ColorAnimation,
    pub color_fps: f32,
    pub color_smooth: bool,
    pub can_move: Option<bool>,
}
impl LightAnimation {
    pub fn new(game: GameVersion) -> Self {
        Self {
            on: false,
            range_scale: RangeAnimation::default(),
            range_fps: 0.,
            range_smooth: true,
            colors: ColorAnimation::default(),
            color_fps: 0.,
            color_smooth: true,
            can_move: (game == GameVersion::Gothic2).then_some(true),
        }
    }
}
/// Shared complete light payload. `animation=None` is a static light: no hidden
/// animation fields are silently omitted. A dynamic G2 light has `can_move`;
/// dynamic G1 does not. This is a field record, not an invented archive class.
#[derive(Clone, PartialEq)]
pub struct LightPreset {
    pub preset: Vec<u8>,
    pub light_type: LightType,
    pub range: f32,
    pub color: Color,
    pub cone_angle: f32,
    pub quality: LightQuality,
    pub lensflare: Vec<u8>,
    pub animation: Option<LightAnimation>,
}
impl fmt::Debug for LightPreset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LightPreset")
            .field("preset_bytes", &self.preset.len())
            .field("lensflare_bytes", &self.lensflare.len())
            .field("light_type", &self.light_type)
            .field("range", &self.range)
            .field("color", &self.color)
            .field("cone_angle", &self.cone_angle)
            .field("quality", &self.quality)
            .field("animation", &self.animation)
            .finish()
    }
}
impl LightPreset {
    pub fn new(game: GameVersion) -> Self {
        Self {
            preset: Vec::new(),
            light_type: LightType::SPOT,
            range: 0.,
            color: Color::default(),
            cone_angle: 0.,
            quality: LightQuality::MEDIUM,
            lensflare: Vec::new(),
            animation: Some(LightAnimation::new(game)),
        }
    }
    pub(crate) fn check_game(&self, game: GameVersion) -> Result<()> {
        if self
            .animation
            .as_ref()
            .is_some_and(|v| v.can_move.is_some() != (game == GameVersion::Gothic2))
        {
            return Err(bad("dynamic light game mismatch"));
        }
        Ok(())
    }
    /// Read one field record. On error the archive reader may be partially
    /// consumed: discard it. The byte budget covers this retained record;
    /// callers budget collections cumulatively. Archive limits govern the stream.
    pub fn read(
        reader: &mut archive::Reader<'_>,
        game: GameVersion,
        max_copied_bytes: usize,
    ) -> Result<Self> {
        let mut copied = 0;
        super::special_codec::read_light(
            &mut super::codec::Fields {
                r: reader,
                copied: &mut copied,
                limit: max_copied_bytes,
            },
            game,
        )
    }
    /// Write one field record. On error discard the partially written archive
    /// writer. Raw animation spellings are preserved; output limits are enforced
    /// by that writer. No decoded replacement or implicit numeric repair occurs.
    pub fn write(&self, writer: &mut archive::Writer, game: GameVersion) -> Result<()> {
        self.check_game(game)?;
        super::special_codec::write_light(writer, self)
    }
}
