//! Bounded version-0 TEX storage, explicit pixel conversion and legacy DDS export.
//!
//! Mips are largest-first in this API and smallest-first on the TEX wire. Raw
//! construction/rewrite preserves every byte. [`Texture::converted`] and
//! [`Builder::build_converted`] explicitly permit quantization and compression.
//! No mip generation or automatic palette generation occurs.
//!
//! ```
//! use zenkit::texture::{Format, Metadata, Texture};
//! let texture = Texture::from_parts(Format::R8G8B8A8, Metadata::new(1, 1),
//!     None, vec![vec![255, 0, 0, 255]], Default::default())?;
//! let bytes = texture.to_bytes(Default::default())?;
//! assert_eq!(Texture::from_bytes(&bytes, Default::default())?, texture);
//! assert_eq!(texture.mip(0).unwrap().rgba8(Default::default())?, [255,0,0,255]);
//! # Ok::<(), zenkit::Error>(())
//! ```
// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
mod bc;
mod codec;
mod dds;
use crate::{
    Error, ErrorKind, Result,
    io::{Reader, Writer},
    values::Color,
};
use std::fmt;
const CTX: &str = "texture";
fn bad(kind: ErrorKind) -> Error {
    Error::operation(kind, CTX)
}
fn at(kind: ErrorKind, p: usize) -> Error {
    Error::new(kind, p, CTX)
}
fn sum(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b)
        .ok_or_else(|| bad(ErrorKind::LimitExceeded))
}
fn product(a: usize, b: usize) -> Result<usize> {
    a.checked_mul(b)
        .ok_or_else(|| bad(ErrorKind::LimitExceeded))
}
fn copy(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut v = Vec::new();
    v.try_reserve_exact(bytes.len())
        .map_err(|_| bad(ErrorKind::LimitExceeded))?;
    v.extend_from_slice(bytes);
    Ok(v)
}
fn zeroed(n: usize) -> Result<Vec<u8>> {
    let mut v = Vec::new();
    v.try_reserve_exact(n)
        .map_err(|_| bad(ErrorKind::LimitExceeded))?;
    v.resize(n, 0);
    Ok(v)
}
/// Wire format. Channel names describe byte order for byte-packed formats;
/// the 16-bit formats use their conventional little-endian bit fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Format {
    B8G8R8A8,
    R8G8B8A8,
    A8B8G8R8,
    A8R8G8B8,
    B8G8R8,
    R8G8B8,
    A4R4G4B4,
    A1R5G5B5,
    R5G6B5,
    P8,
    Dxt1,
    Dxt2,
    Dxt3,
    Dxt4,
    Dxt5,
}
impl TryFrom<u32> for Format {
    type Error = Error;
    fn try_from(v: u32) -> Result<Self> {
        Ok(match v {
            0 => Self::B8G8R8A8,
            1 => Self::R8G8B8A8,
            2 => Self::A8B8G8R8,
            3 => Self::A8R8G8B8,
            4 => Self::B8G8R8,
            5 => Self::R8G8B8,
            6 => Self::A4R4G4B4,
            7 => Self::A1R5G5B5,
            8 => Self::R5G6B5,
            9 => Self::P8,
            10 => Self::Dxt1,
            11 => Self::Dxt2,
            12 => Self::Dxt3,
            13 => Self::Dxt4,
            14 => Self::Dxt5,
            _ => return Err(bad(ErrorKind::Unsupported)),
        })
    }
}
impl From<Format> for u32 {
    fn from(v: Format) -> u32 {
        v as u32
    }
}
impl Format {
    pub const fn is_compressed(self) -> bool {
        self as u32 >= 10
    }
    pub const fn is_premultiplied(self) -> bool {
        matches!(self, Self::Dxt2 | Self::Dxt4)
    }
    fn unit(self) -> usize {
        match self {
            Self::B8G8R8A8 | Self::R8G8B8A8 | Self::A8B8G8R8 | Self::A8R8G8B8 => 4,
            Self::B8G8R8 | Self::R8G8B8 => 3,
            Self::A4R4G4B4 | Self::A1R5G5B5 | Self::R5G6B5 => 2,
            Self::P8 => 1,
            Self::Dxt1 => 8,
            _ => 16,
        }
    }
    /// Exact storage size, using ceil-divided 4x4 blocks for compressed edges.
    pub fn byte_len(self, width: u32, height: u32) -> Result<usize> {
        if width == 0 || height == 0 {
            return Err(bad(ErrorKind::InvalidData));
        }
        let (w, h) = if self.is_compressed() {
            (width.div_ceil(4), height.div_ceil(4))
        } else {
            (width, height)
        };
        product(product(w as usize, h as usize)?, self.unit())
    }
}
/// A P8 palette in logical RGBA order. TEX stores BGRA; legacy DDS stores RGBA.
pub type Palette = [Color; 256];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub width: u32,
    pub height: u32,
    pub reference_width: u32,
    pub reference_height: u32,
    pub average_color: u32,
}
impl Metadata {
    pub const fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            reference_width: width,
            reference_height: height,
            average_color: 0,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Complete TEX/DDS output or retained raw inputs, including headers/palettes.
    pub bytes: usize,
    /// Maximum base dimension; zero dimensions are always invalid.
    pub dimension: u32,
    /// Effective count, including repeated clamped 1x1 tails.
    pub mipmaps: usize,
    /// Decoded RGBA bytes: one mip for decode, sum of converted mips for conversion.
    pub rgba_bytes: usize,
    /// Conversion's retained destination plus largest concurrent RGBA scratch.
    /// Counts logical bytes, not Vec/container overhead. Source ownership and
    /// allocator spare capacity are excluded.
    pub conversion_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            bytes: 256 * 1024 * 1024,
            dimension: 16384,
            mipmaps: 32,
            rgba_bytes: 256 * 1024 * 1024,
            conversion_bytes: 512 * 1024 * 1024,
        }
    }
}
fn dimensions(meta: Metadata, limits: Limits) -> Result<()> {
    if meta.width == 0 || meta.height == 0 {
        return Err(bad(ErrorKind::InvalidData));
    }
    if meta.width > limits.dimension || meta.height > limits.dimension {
        return Err(bad(ErrorKind::LimitExceeded));
    }
    Ok(())
}
fn level_size(meta: Metadata, level: usize) -> (u32, u32) {
    (
        meta.width
            .checked_shr(u32::try_from(level).unwrap_or(u32::MAX))
            .unwrap_or(0)
            .max(1),
        meta.height
            .checked_shr(u32::try_from(level).unwrap_or(u32::MAX))
            .unwrap_or(0)
            .max(1),
    )
}
fn palette_check(format: Format, palette: Option<&Palette>) -> Result<()> {
    if (format == Format::P8) != palette.is_some() {
        Err(bad(ErrorKind::InvalidData))
    } else {
        Ok(())
    }
}
#[derive(Clone, PartialEq, Eq)]
pub struct Texture {
    format: Format,
    metadata: Metadata,
    palette: Option<Box<Palette>>,
    mips: Vec<Vec<u8>>,
    zero_count: bool,
}
impl fmt::Debug for Texture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Texture")
            .field("format", &self.format)
            .field("metadata", &self.metadata)
            .field("mipmaps", &self.mips.len())
            .field("palette", &self.palette.is_some())
            .field("zero_count", &self.zero_count)
            .finish()
    }
}
impl Texture {
    /// Lossless raw construction. Mip lengths must match every clamped level.
    pub fn from_parts(
        format: Format,
        metadata: Metadata,
        palette: Option<Box<Palette>>,
        mips: Vec<Vec<u8>>,
        limits: Limits,
    ) -> Result<Self> {
        let t = Self {
            format,
            metadata,
            palette,
            mips,
            zero_count: false,
        };
        t.validate(limits)?;
        Ok(t)
    }
    fn validate(&self, limits: Limits) -> Result<usize> {
        dimensions(self.metadata, limits)?;
        palette_check(self.format, self.palette.as_deref())?;
        if self.mips.is_empty() {
            return Err(bad(ErrorKind::InvalidData));
        }
        if self.mips.len() > limits.mipmaps || u32::try_from(self.mips.len()).is_err() {
            return Err(bad(ErrorKind::LimitExceeded));
        }
        let mut n = 36 + if self.palette.is_some() { 1024 } else { 0 };
        for (i, mip) in self.mips.iter().enumerate() {
            let (w, h) = level_size(self.metadata, i);
            if mip.len() != self.format.byte_len(w, h)? {
                return Err(bad(ErrorKind::InvalidData));
            }
            n = sum(n, mip.len())?;
        }
        if n > limits.bytes {
            return Err(bad(ErrorKind::LimitExceeded));
        }
        Ok(n)
    }
    pub const fn format(&self) -> Format {
        self.format
    }
    pub const fn metadata(&self) -> Metadata {
        self.metadata
    }
    pub fn palette(&self) -> Option<&Palette> {
        self.palette.as_deref()
    }
    pub fn mipmap_count(&self) -> usize {
        self.mips.len()
    }
    /// The wire's zero-count alias denotes one effective base mip and is retained.
    pub fn declared_mipmap_count(&self) -> u32 {
        if self.zero_count {
            0
        } else {
            self.mips.len() as u32
        }
    }
    pub fn mip(&self, index: usize) -> Option<Mip<'_>> {
        self.mips.get(index).map(|bytes| {
            let (width, height) = level_size(self.metadata, index);
            Mip {
                format: self.format,
                palette: self.palette.as_deref(),
                index,
                width,
                height,
                bytes,
            }
        })
    }
    pub fn mips(
        &self,
    ) -> impl ExactSizeIterator<Item = Mip<'_>> + DoubleEndedIterator + Clone + '_ {
        (0..self.mips.len()).map(|i| self.mip(i).expect("bounded mip index"))
    }
    /// Non-layout metadata is editable without invalidating mip storage.
    pub fn set_reference_size(&mut self, width: u32, height: u32) {
        self.metadata.reference_width = width;
        self.metadata.reference_height = height;
    }
    pub fn set_average_color(&mut self, color: u32) {
        self.metadata.average_color = color;
    }
    /// Replace exactly one raw mip. Failure leaves this texture unchanged.
    pub fn replace_mip(&mut self, index: usize, bytes: Vec<u8>) -> Result<Vec<u8>> {
        let target = self
            .mips
            .get_mut(index)
            .ok_or_else(|| bad(ErrorKind::NotFound))?;
        if bytes.len() != target.len() {
            return Err(bad(ErrorKind::InvalidData));
        }
        Ok(std::mem::replace(target, bytes))
    }
    pub fn from_bytes(bytes: &[u8], limits: Limits) -> Result<Self> {
        if bytes.len() > limits.bytes {
            return Err(at(ErrorKind::LimitExceeded, 0));
        }
        let mut r = Reader::new(bytes);
        let t = Self::read(&mut r, limits)?;
        if r.remaining() != 0 {
            return Err(at(ErrorKind::InvalidData, r.position()));
        }
        Ok(t)
    }
    /// Read one TEX atomically, preserving any enclosing stream's suffix.
    pub fn read(input: &mut Reader<'_>, limits: Limits) -> Result<Self> {
        let mut r = input.clone();
        let start = r.position();
        if r.take_bytes(4, CTX)? != b"ZTEX" {
            return Err(at(ErrorKind::InvalidData, start));
        }
        if r.u32(CTX)? != 0 {
            return Err(at(ErrorKind::Unsupported, start + 4));
        }
        let format =
            Format::try_from(r.u32(CTX)?).map_err(|_| at(ErrorKind::Unsupported, start + 8))?;
        let width = r.u32(CTX)?;
        let height = r.u32(CTX)?;
        if width == 0 || height == 0 {
            return Err(at(
                ErrorKind::InvalidData,
                start + if width == 0 { 12 } else { 16 },
            ));
        }
        if width > limits.dimension || height > limits.dimension {
            return Err(at(
                ErrorKind::LimitExceeded,
                start + if width > limits.dimension { 12 } else { 16 },
            ));
        }
        let declared = r.u32(CTX)?;
        let count = declared.max(1) as usize;
        if count > limits.mipmaps {
            return Err(at(ErrorKind::LimitExceeded, start + 20));
        }
        let metadata = Metadata {
            width,
            height,
            reference_width: r.u32(CTX)?,
            reference_height: r.u32(CTX)?,
            average_color: r.u32(CTX)?,
        };
        let mut size = if format == Format::P8 { 1024usize } else { 0 };
        for i in 0..count {
            let (w, h) = level_size(metadata, i);
            size = sum(
                size,
                format
                    .byte_len(w, h)
                    .map_err(|_| at(ErrorKind::LimitExceeded, start + 12))?,
            )
            .map_err(|_| at(ErrorKind::LimitExceeded, start + 20))?;
            if size.checked_add(36).is_none_or(|n| n > limits.bytes) {
                return Err(at(ErrorKind::LimitExceeded, start));
            }
            if size > r.remaining() {
                return Err(at(ErrorKind::UnexpectedEof, r.position()));
            }
        }
        if sum(size, 36)? > limits.bytes {
            return Err(at(ErrorKind::LimitExceeded, start));
        }
        if size > r.remaining() {
            return Err(at(ErrorKind::UnexpectedEof, r.position()));
        }
        let palette = if format == Format::P8 {
            let mut p = Box::new([Color::default(); 256]);
            for color in p.iter_mut() {
                let b = r.take_bytes(4, CTX)?;
                *color = Color::from_bgra([b[0], b[1], b[2], b[3]]);
            }
            Some(p)
        } else {
            None
        };
        let mut mips = Vec::new();
        mips.try_reserve_exact(count)
            .map_err(|_| at(ErrorKind::LimitExceeded, start + 20))?;
        for i in (0..count).rev() {
            let (w, h) = level_size(metadata, i);
            mips.push(copy(r.take_bytes(format.byte_len(w, h)?, CTX)?)?);
        }
        mips.reverse();
        *input = r;
        Ok(Self {
            format,
            metadata,
            palette,
            mips,
            zero_count: declared == 0,
        })
    }
    pub fn to_bytes(&self, limits: Limits) -> Result<Vec<u8>> {
        self.validate(limits)?;
        let mut w = Writer::new(limits.bytes);
        w.bytes(b"ZTEX", CTX)?;
        for v in [
            0,
            self.format as u32,
            self.metadata.width,
            self.metadata.height,
            self.declared_mipmap_count(),
            self.metadata.reference_width,
            self.metadata.reference_height,
            self.metadata.average_color,
        ] {
            w.u32(v, CTX)?;
        }
        if let Some(p) = &self.palette {
            for c in p.iter() {
                w.bytes(&c.to_bgra(), CTX)?;
            }
        }
        for m in self.mips.iter().rev() {
            w.bytes(m, CTX)?;
        }
        Ok(w.into_inner())
    }
    /// Export legacy DDS with correct little-endian channel masks, tightly packed
    /// rows and largest-first mips. DXT2/4 retain premultiplied stored colors.
    /// TEX's repeated 1x1 tails beyond the natural mip chain return Unsupported;
    /// DDS consumers reject those extra levels, and none are silently dropped.
    pub fn to_dds(&self, limits: Limits) -> Result<Vec<u8>> {
        self.validate(limits)?;
        let natural = 32
            - self
                .metadata
                .width
                .max(self.metadata.height)
                .leading_zeros();
        if self.mips.len() > natural as usize {
            return Err(bad(ErrorKind::Unsupported));
        }
        dds::write(self, limits)
    }
    /// Explicitly lossy conversion. Same format/palette copies raw bytes exactly;
    /// changing a P8 palette remaps indices. P8 requires a supplied palette, and
    /// a palette for any other target is an error. Metadata and count alias survive.
    ///
    /// Packed channels round to nearest. BC1 alpha below128 becomes transparent.
    /// BC compression uses deterministic endpoint candidates, not an optimal
    /// cluster fit. P8 selects the nearest RGBA palette color (first tie wins).
    /// BC2/3 fitting ignores colors whose reconstructed alpha is zero. Straight
    /// DXT3/5 score RGB error weighted by reconstructed alpha squared, including
    /// an endpoint candidate from the highest-alpha pixels. DXT2/4 quantize
    /// alpha first and fit premultiplied RGB without additional alpha weighting.
    pub fn converted(
        &self,
        target: Format,
        palette: Option<&Palette>,
        limits: Limits,
    ) -> Result<Self> {
        self.validate(limits)?;
        palette_check(target, palette)?;
        let sources = self
            .mips()
            .map(|m| (m.format, m.palette, m.width, m.height, m.bytes));
        let mips = convert_mips(sources, target, palette, limits)?;
        Ok(Self {
            format: target,
            metadata: self.metadata,
            palette: palette.map(|p| Box::new(*p)),
            mips,
            zero_count: self.zero_count,
        })
    }
}
/// Borrowed mip access remains tied to the owning texture; no raw index handle.
#[derive(Clone, Copy)]
pub struct Mip<'a> {
    format: Format,
    palette: Option<&'a Palette>,
    index: usize,
    width: u32,
    height: u32,
    bytes: &'a [u8],
}
impl fmt::Debug for Mip<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mip")
            .field("index", &self.index)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}
impl<'a> Mip<'a> {
    pub const fn index(self) -> usize {
        self.index
    }
    pub const fn width(self) -> u32 {
        self.width
    }
    pub const fn height(self) -> u32 {
        self.height
    }
    pub const fn bytes(self) -> &'a [u8] {
        self.bytes
    }
    /// Straight RGBA8. DXT2/4 unpremultiply using rounded/clamped division;
    /// alpha0 yields black. BC interpolation rounds down;
    /// raw565 expands by floor(n*255/max), independently of BC bit replication.
    /// Only `limits.rgba_bytes` applies to this already validated borrowed mip;
    /// storage, dimension and conversion limits apply at their owning operations.
    pub fn rgba8(self, limits: Limits) -> Result<Vec<u8>> {
        let n = product(product(self.width as usize, self.height as usize)?, 4)?;
        if n > limits.rgba_bytes {
            return Err(bad(ErrorKind::LimitExceeded));
        }
        codec::decode(
            self.format,
            self.width,
            self.height,
            self.bytes,
            self.palette,
        )
    }
}
struct Source {
    format: Format,
    palette: Option<Box<Palette>>,
    bytes: Vec<u8>,
}
/// Explicit per-level builder. Pushes are largest-first and atomic on error.
/// Input palettes belong to each P8 source; no mip or palette generation occurs.
pub struct Builder {
    metadata: Metadata,
    sources: Vec<Source>,
    retained: usize,
    limits: Limits,
}
impl fmt::Debug for Builder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Builder")
            .field("metadata", &self.metadata)
            .field("mipmaps", &self.sources.len())
            .field("retained_bytes", &self.retained)
            .finish()
    }
}
impl Builder {
    pub fn new(width: u32, height: u32, limits: Limits) -> Result<Self> {
        let metadata = Metadata::new(width, height);
        dimensions(metadata, limits)?;
        if limits.bytes < 36 {
            return Err(bad(ErrorKind::LimitExceeded));
        }
        Ok(Self {
            metadata,
            sources: Vec::new(),
            retained: 36,
            limits,
        })
    }
    pub fn mipmap_count(&self) -> usize {
        self.sources.len()
    }
    pub fn push(&mut self, format: Format, bytes: &[u8], palette: Option<&Palette>) -> Result<()> {
        palette_check(format, palette)?;
        if self.sources.len() >= self.limits.mipmaps || self.sources.len() >= u32::MAX as usize {
            return Err(bad(ErrorKind::LimitExceeded));
        }
        let (w, h) = level_size(self.metadata, self.sources.len());
        if bytes.len() != format.byte_len(w, h)? {
            return Err(bad(ErrorKind::InvalidData));
        }
        let retained = sum(
            sum(self.retained, bytes.len())?,
            if palette.is_some() { 1024 } else { 0 },
        )?;
        if retained > self.limits.bytes {
            return Err(bad(ErrorKind::LimitExceeded));
        }
        let bytes = copy(bytes)?;
        self.sources
            .try_reserve(1)
            .map_err(|_| bad(ErrorKind::LimitExceeded))?;
        self.sources.push(Source {
            format,
            palette: palette.map(|p| Box::new(*p)),
            bytes,
        });
        self.retained = retained;
        Ok(())
    }
    /// Quantize/compress supplied levels into one format. Reference dimensions
    /// equal the base size and average_color is0; use Texture setters to change them.
    /// Uses the same quantization, alpha-aware endpoint fitting and fixed-palette
    /// policy documented on [`Texture::converted`].
    pub fn build_converted(&self, target: Format, palette: Option<&Palette>) -> Result<Texture> {
        palette_check(target, palette)?;
        if self.sources.is_empty() {
            return Err(bad(ErrorKind::InvalidData));
        }
        let sources = self.sources.iter().enumerate().map(|(i, s)| {
            let (w, h) = level_size(self.metadata, i);
            (s.format, s.palette.as_deref(), w, h, s.bytes.as_slice())
        });
        let mips = convert_mips(sources, target, palette, self.limits)?;
        Texture::from_parts(
            target,
            self.metadata,
            palette.map(|p| Box::new(*p)),
            mips,
            self.limits,
        )
    }
}
fn convert_mips<'a>(
    sources: impl Iterator<Item = (Format, Option<&'a Palette>, u32, u32, &'a [u8])> + Clone,
    target: Format,
    palette: Option<&Palette>,
    limits: Limits,
) -> Result<Vec<Vec<u8>>> {
    let mut output = 36 + if palette.is_some() { 1024 } else { 0 };
    let mut decoded = 0;
    let mut scratch = 0;
    let mut count = 0;
    for (format, p, w, h, _) in sources.clone() {
        count += 1;
        output = sum(output, target.byte_len(w, h)?)?;
        if format != target || p != palette {
            let n = product(product(w as usize, h as usize)?, 4)?;
            decoded = sum(decoded, n)?;
            scratch = scratch.max(n);
        }
    }
    if output > limits.bytes
        || decoded > limits.rgba_bytes
        || sum(output, scratch)? > limits.conversion_bytes
    {
        return Err(bad(ErrorKind::LimitExceeded));
    }
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| bad(ErrorKind::LimitExceeded))?;
    for (format, p, w, h, bytes) in sources {
        let out = if format == target && p == palette {
            copy(bytes)?
        } else {
            let rgba = codec::decode(format, w, h, bytes, p)?;
            codec::encode(target, w, h, &rgba, palette)?
        };
        result.push(out);
    }
    Ok(result)
}
