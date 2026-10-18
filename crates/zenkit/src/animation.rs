//! Bounded MAN animation reading with exact packed samples and explicit decoding.
//!
//! This module does not quantize or encode samples. Raw packed words remain
//! available, while decoded views use f32 multiply/add, positive quaternion w
//! and overlength xyz normalization.
//! Input strings have LF or CRLF delimiters, preserve leading whitespace and raw
//! non-UTF8 bytes, and reject embedded NUL/CR. Dates retain even invalid calendar
//! values. Unknown chunks/version IDs fail explicitly; no payload is silently skipped.
//!
//! ```
//! use zenkit::animation::PackedSample;
//! let packed = PackedSample { rotation: [32767; 3], position: [0, 2, 4] };
//! let decoded = packed.decode(0.5, -1.0);
//! assert_eq!(decoded.position.to_array(), [-1.0, 0.0, 1.0]);
//! assert_eq!(decoded.rotation.to_wxyz(), [1.0, 0.0, 0.0, 0.0]);
//! ```
// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use crate::{
    Error, ErrorKind, Result,
    io::{LineEnding, Reader},
    mesh::{Budget, ModelHierarchy},
    values::{Aabb, Date, Quat, Vec3},
};
use std::{fmt, mem::size_of};
pub(crate) const CTX: &str = "animation";
pub(crate) fn bad(kind: ErrorKind) -> Error {
    Error::operation(kind, CTX)
}
pub(crate) fn at(kind: ErrorKind, p: usize) -> Error {
    Error::new(kind, p, CTX)
}
/// Shared animation/morph/script budgets. These bound logical retained data;
/// capacity and temporary validation stacks are not a process allocator quota.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Maximum explicitly supplied input span, checked before parsing.
    pub bytes: usize,
    /// Aggregate logical owned buffers and collection slots, including nested MRM data.
    pub owned_bytes: usize,
    /// Aggregate collection entries, including events, diagnostics, samples and nested MRM arrays.
    pub elements: usize,
    /// Binary chunk headers, including nested MRM records; unused by source scripts.
    pub chunks: usize,
    /// Individual byte strings and source token lengths.
    pub string_bytes: usize,
    /// Source-script tokens, including ignored parentheses; comments, whitespace
    /// and semicolon separators are excluded. Unused by binary readers.
    pub tokens: usize,
    /// Source-script braces or embedded morph OBB depth; MAN is flat.
    pub depth: usize,
    /// MMB only: per embedded material-table keys (two temporary index vectors).
    pub material_table_keys: usize,
    /// MMB only: cumulative inspected key entries, counting repeated lookups.
    pub material_table_entries: usize,
    /// MMB only: cumulative inspected table bytes, counting repeated lookups.
    pub material_table_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            bytes: 256 * 1024 * 1024,
            owned_bytes: 512 * 1024 * 1024,
            elements: 4_000_000,
            chunks: 100_000,
            string_bytes: 1024 * 1024,
            tokens: 4_000_000,
            depth: 128,
            material_table_keys: 65_536,
            material_table_entries: 1_000_000,
            material_table_bytes: 256 * 1024 * 1024,
        }
    }
}
impl Limits {
    pub(crate) fn budget<'a>(self) -> Budget<'a> {
        Budget::new(crate::mesh::Limits {
            bytes: self.bytes,
            owned_bytes: self.owned_bytes,
            elements: self.elements,
            chunks: self.chunks,
            string_bytes: self.string_bytes,
            obb_depth: self.depth,
            material_table_keys: self.material_table_keys,
            material_table_entries: self.material_table_entries,
            material_table_bytes: self.material_table_bytes,
            ..Default::default()
        })
    }
}
pub(crate) fn finish(r: &Reader<'_>) -> Result<()> {
    if r.remaining() != 0 {
        Err(at(ErrorKind::Unsupported, r.position()))
    } else {
        Ok(())
    }
}
pub(crate) fn chunk<'a>(r: &mut Reader<'a>, b: &mut Budget) -> Result<(u16, Reader<'a>)> {
    let p = r.position();
    if b.chunks >= b.limits.chunks {
        return Err(at(ErrorKind::LimitExceeded, p));
    }
    let id = r.u16(CTX)?;
    let n = r.u32(CTX)? as usize;
    let body = r.substream(n, CTX)?;
    b.chunks += 1;
    Ok((id, body))
}
pub(crate) fn once(seen: &mut u32, bit: u32, p: usize) -> Result<()> {
    if *seen & bit != 0 {
        return Err(at(ErrorKind::InvalidData, p));
    }
    *seen |= bit;
    Ok(())
}
pub(crate) fn line(r: &mut Reader<'_>, b: &mut Budget) -> Result<Vec<u8>> {
    let p = r.position();
    let v = r.line(LineEnding::Lf, b.limits.string_bytes.saturating_add(1), CTX)?;
    let v = v.strip_suffix(b"\r").unwrap_or(v);
    if v.len() > b.limits.string_bytes {
        return Err(at(ErrorKind::LimitExceeded, p));
    }
    if v.iter().any(|c| matches!(c, 0 | b'\r')) {
        return Err(at(ErrorKind::InvalidData, p));
    }
    b.copy(v, Some(p))
}
pub(crate) fn array<T>(
    r: &mut Reader<'_>,
    n: usize,
    stride: usize,
    b: &mut Budget,
    mut read: impl FnMut(&mut Reader<'_>) -> Result<T>,
) -> Result<Vec<T>> {
    let p = r.position();
    let len = n
        .checked_mul(stride)
        .ok_or_else(|| at(ErrorKind::LimitExceeded, p))?;
    if len > r.remaining() {
        return Err(at(ErrorKind::UnexpectedEof, p));
    }
    let mut v = b.array(n, Some(p))?;
    for _ in 0..n {
        v.push(read(r)?);
    }
    Ok(v)
}
pub(crate) fn push<T>(v: &mut Vec<T>, value: T, b: &mut Budget, p: usize) -> Result<()> {
    b.charge(size_of::<T>(), 1, Some(p))?;
    v.try_reserve(1)
        .map_err(|_| at(ErrorKind::LimitExceeded, p))?;
    v.push(value);
    Ok(())
}
pub(crate) fn window<T>(
    input: &mut Reader<'_>,
    length: usize,
    limits: Limits,
    parse: impl FnOnce(&mut Reader<'_>, &mut Budget) -> Result<T>,
) -> Result<T> {
    window_with_space(
        input,
        crate::archive::AddressSpace::from_reader(input),
        length,
        limits,
        parse,
    )
}
pub(crate) fn window_with_space<T>(
    input: &mut Reader<'_>,
    space: crate::archive::AddressSpace<'_>,
    length: usize,
    limits: Limits,
    parse: impl FnOnce(&mut Reader<'_>, &mut Budget) -> Result<T>,
) -> Result<T> {
    if !space.contains(input) {
        return Err(bad(ErrorKind::InvalidData));
    }
    let p = input.position();
    if length > limits.bytes {
        return Err(at(ErrorKind::LimitExceeded, p));
    }
    let mut candidate = input.clone();
    let mut r = candidate.substream(length, CTX)?;
    let mut budget = limits.budget();
    budget.space = Some(space);
    let v = parse(&mut r, &mut budget).map_err(|mut e| {
        e.context = CTX;
        e
    })?;
    finish(&r)?;
    *input = candidate;
    Ok(v)
}
#[derive(Clone, PartialEq)]
pub struct Source {
    pub date: Date,
    pub path: Vec<u8>,
    pub script: Vec<u8>,
}
impl fmt::Debug for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Source")
            .field("date", &self.date)
            .field("path_bytes", &self.path.len())
            .field("script_bytes", &self.script.len())
            .finish()
    }
}
/// Raw event discriminator; all u32 values survive loading.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct EventType(pub u32);
impl EventType {
    pub const TAG: Self = Self(0);
    pub const SOUND: Self = Self(1);
    pub const SOUND_GROUND: Self = Self(2);
    pub const BATCH: Self = Self(3);
    pub const SWAP_MESH: Self = Self(4);
    pub const HEADING: Self = Self(5);
    pub const PARTICLE: Self = Self(6);
    pub const PARTICLE_GROUND: Self = Self(7);
    pub const PARTICLE_STOP: Self = Self(8);
    pub const SET_MESH: Self = Self(9);
    pub const MORPH: Self = Self(10);
    pub const CAMERA_TREMOR: Self = Self(11);
}
#[derive(Clone, PartialEq)]
pub struct Event {
    pub kind: EventType,
    pub frame: u32,
    pub tag: Vec<u8>,
    pub content: [Vec<u8>; 4],
    pub values: [f32; 4],
    pub probability: f32,
}
impl fmt::Debug for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Event")
            .field("kind", &self.kind)
            .field("frame", &self.frame)
            .field("tag_bytes", &self.tag.len())
            .field("content_bytes", &self.content.each_ref().map(|v| v.len()))
            .field("values", &self.values)
            .field("probability", &self.probability)
            .finish()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackedSample {
    pub rotation: [u16; 3],
    pub position: [u16; 3],
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub position: Vec3,
    pub rotation: Quat,
}
impl PackedSample {
    /// Decode without quantization. Header scale/minimum may be nonfinite and are
    /// not repaired. Quaternion xyz outside the unit sphere is normalized to w=0.
    pub fn decode(self, scale: f32, minimum: f32) -> Sample {
        const SCALE: f32 = (1.0 / 65535.0) * 2.1;
        let [mut x, mut y, mut z] = self.rotation.map(|v| (f32::from(v) - 32767.) * SCALE);
        let len = x * x + y * y + z * z;
        let w = if len > 1. {
            let inverse = 1. / len.sqrt();
            x *= inverse;
            y *= inverse;
            z *= inverse;
            0.
        } else {
            (1. - len).sqrt()
        };
        Sample {
            position: self.position.map(|v| f32::from(v) * scale + minimum).into(),
            rotation: Quat::from_wxyz([w, x, y, z]),
        }
    }
}
#[derive(Clone, PartialEq)]
pub struct ModelAnimation {
    pub version: u16,
    pub marker: bool,
    pub name: Vec<u8>,
    pub next: Vec<u8>,
    pub layer: u32,
    pub frame_count: u32,
    pub fps: f32,
    pub fps_source: f32,
    pub sample_position_min: f32,
    pub sample_position_scale: f32,
    pub bbox: Aabb,
    pub checksum: u32,
    pub source: Option<Source>,
    pub node_indices: Vec<u32>,
    pub packed_samples: Vec<PackedSample>,
    /// None means no EVENTS chunk; Some(empty) preserves an explicitly empty one.
    pub events: Option<Vec<Event>>,
}
impl fmt::Debug for ModelAnimation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelAnimation")
            .field("version", &self.version)
            .field("name_bytes", &self.name.len())
            .field("frames", &self.frame_count)
            .field("nodes", &self.node_indices.len())
            .field("samples", &self.packed_samples.len())
            .field("events", &self.events.as_ref().map(Vec::len))
            .finish()
    }
}
impl ModelAnimation {
    pub fn from_bytes(bytes: &[u8], limits: Limits) -> Result<Self> {
        Self::read(&mut Reader::new(bytes), bytes.len(), limits)
    }
    /// MAN has no end chunk; the caller supplies the exact embedded document span.
    /// Failure leaves the original reader unchanged.
    pub fn read(r: &mut Reader<'_>, length: usize, limits: Limits) -> Result<Self> {
        window(r, length, limits, read_man)
    }
    pub fn samples(&self) -> impl ExactSizeIterator<Item = Sample> + DoubleEndedIterator + '_ {
        self.packed_samples
            .iter()
            .map(|v| v.decode(self.sample_position_scale, self.sample_position_min))
    }
    /// Frame-major lookup. `node` is the channel ordinal in `node_indices`, not a skeleton ID.
    pub fn sample(&self, frame: usize, node: usize) -> Option<Sample> {
        if frame >= self.frame_count as usize || node >= self.node_indices.len() {
            return None;
        }
        let i = frame
            .checked_mul(self.node_indices.len())?
            .checked_add(node)?;
        self.packed_samples
            .get(i)
            .map(|v| v.decode(self.sample_position_scale, self.sample_position_min))
    }
    /// Optional relationship check against the separate skeleton; node IDs are
    /// otherwise preserved without guessing a global hierarchy size.
    pub fn validate_hierarchy(&self, h: &ModelHierarchy) -> Result<()> {
        if self.checksum != h.checksum
            || self
                .node_indices
                .iter()
                .any(|&i| i as usize >= h.nodes.len())
        {
            Err(bad(ErrorKind::InvalidData))
        } else {
            Ok(())
        }
    }
}
fn read_man(r: &mut Reader<'_>, b: &mut Budget) -> Result<ModelAnimation> {
    let mut m = ModelAnimation {
        version: 12,
        marker: false,
        name: vec![],
        next: vec![],
        layer: 0,
        frame_count: 0,
        fps: 0.,
        fps_source: 0.,
        sample_position_min: 0.,
        sample_position_scale: 0.,
        bbox: Aabb::default(),
        checksum: 0,
        source: None,
        node_indices: vec![],
        packed_samples: vec![],
        events: None,
    };
    let mut seen = 0;
    let mut nodes = 0usize;
    while r.remaining() != 0 {
        let p = r.position();
        let (id, mut c) = chunk(r, b)?;
        match id {
            0xa000 => {
                once(&mut seen, 1, p)?;
                m.marker = true;
            }
            0xa010 => {
                once(&mut seen, 2, p)?;
                m.source = Some(Source {
                    date: Date::read(&mut c, CTX)?,
                    path: line(&mut c, b)?,
                    script: line(&mut c, b)?,
                });
            }
            0xa020 => {
                once(&mut seen, 4, p)?;
                m.version = c.u16(CTX)?;
                if m.version != 12 {
                    return Err(at(ErrorKind::Unsupported, p + 6));
                }
                m.name = line(&mut c, b)?;
                m.layer = c.u32(CTX)?;
                m.frame_count = c.u32(CTX)?;
                nodes = c.u32(CTX)? as usize;
                m.fps = c.f32(CTX)?;
                m.fps_source = c.f32(CTX)?;
                m.sample_position_min = c.f32(CTX)?;
                m.sample_position_scale = c.f32(CTX)?;
                m.bbox = Aabb::read(&mut c, CTX)?;
                m.next = line(&mut c, b)?;
            }
            0xa030 => {
                once(&mut seen, 8, p)?;
                let n = c.u32(CTX)? as usize;
                if n > c.remaining() / 33 {
                    return Err(at(ErrorKind::UnexpectedEof, c.position()));
                }
                let mut events = b.array(n, Some(c.position()))?;
                for _ in 0..n {
                    events.push(Event {
                        kind: EventType(c.u32(CTX)?),
                        frame: c.u32(CTX)?,
                        tag: line(&mut c, b)?,
                        content: [
                            line(&mut c, b)?,
                            line(&mut c, b)?,
                            line(&mut c, b)?,
                            line(&mut c, b)?,
                        ],
                        values: [c.f32(CTX)?, c.f32(CTX)?, c.f32(CTX)?, c.f32(CTX)?],
                        probability: c.f32(CTX)?,
                    });
                }
                m.events = Some(events);
            }
            0xa090 => {
                once(&mut seen, 16, p)?;
                if seen & 4 == 0 {
                    return Err(at(ErrorKind::InvalidData, p));
                }
                let count = nodes
                    .checked_mul(m.frame_count as usize)
                    .ok_or_else(|| at(ErrorKind::LimitExceeded, p))?;
                let total = nodes
                    .checked_mul(4)
                    .and_then(|v| count.checked_mul(12).and_then(|n| v.checked_add(n)))
                    .and_then(|n| n.checked_add(4))
                    .ok_or_else(|| at(ErrorKind::LimitExceeded, p))?;
                if total > c.remaining() {
                    return Err(at(ErrorKind::UnexpectedEof, c.position()));
                }
                m.checksum = c.u32(CTX)?;
                m.node_indices = array(&mut c, nodes, 4, b, |r| r.u32(CTX))?;
                m.packed_samples = array(&mut c, count, 12, b, |r| {
                    Ok(PackedSample {
                        rotation: [r.u16(CTX)?, r.u16(CTX)?, r.u16(CTX)?],
                        position: [r.u16(CTX)?, r.u16(CTX)?, r.u16(CTX)?],
                    })
                })?;
            }
            _ => return Err(at(ErrorKind::Unsupported, p)),
        }
        finish(&c)?;
    }
    if seen & 20 != 20 {
        return Err(at(ErrorKind::InvalidData, r.position()));
    }
    Ok(m)
}
