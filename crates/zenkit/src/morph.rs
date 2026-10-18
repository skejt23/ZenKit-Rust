//! Version-3 MMB morph mesh reader.
//!
//! Embedded MRM and all morph arrays/strings share the input, ownership and element
//! budgets. Unknown chunks and versions fail explicitly. Binary string delimiters
//! are LF or CRLF; leading whitespace and raw non-UTF8 bytes are retained.
//! Animation vertex indices are checked against the embedded mesh, while signed
//! layers, negative blend times and all binary float bits remain unchanged.
// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use crate::{
    ErrorKind, GameVersion, Result,
    animation::{self, CTX, Limits, array, at, bad, chunk, finish, line, once},
    io::Reader,
    mesh::{Budget, MultiResolutionMesh},
    values::{Date, Vec3},
};
use std::fmt;
#[derive(Clone, PartialEq)]
pub struct Source {
    pub date: Date,
    pub name: Vec<u8>,
}
impl fmt::Debug for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MorphSource")
            .field("date", &self.date)
            .field("name_bytes", &self.name.len())
            .finish()
    }
}
#[derive(Clone, PartialEq)]
pub struct MorphAnimation {
    pub name: Vec<u8>,
    pub layer: i32,
    pub blend_in: f32,
    pub blend_out: f32,
    pub duration: f32,
    pub speed: f32,
    pub flags: u8,
    pub frame_count: u32,
    pub vertices: Vec<u32>,
    pub samples: Vec<Vec3>,
}
impl fmt::Debug for MorphAnimation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MorphAnimation")
            .field("name_bytes", &self.name.len())
            .field("frames", &self.frame_count)
            .field("vertices", &self.vertices.len())
            .field("samples", &self.samples.len())
            .finish()
    }
}
impl MorphAnimation {
    /// Frame-major lookup by the ordinal in `vertices`, not by global mesh vertex ID.
    pub fn sample(&self, frame: usize, vertex: usize) -> Option<Vec3> {
        if frame >= self.frame_count as usize || vertex >= self.vertices.len() {
            return None;
        }
        self.samples
            .get(
                frame
                    .checked_mul(self.vertices.len())?
                    .checked_add(vertex)?,
            )
            .copied()
    }
}
#[derive(Clone, PartialEq)]
pub struct MorphMesh {
    pub version: u32,
    pub marker: bool,
    pub name: Vec<u8>,
    pub mesh: MultiResolutionMesh,
    pub morph_positions: Vec<Vec3>,
    pub animations: Vec<MorphAnimation>,
    pub sources: Option<Vec<Source>>,
}
impl fmt::Debug for MorphMesh {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MorphMesh")
            .field("version", &self.version)
            .field("name_bytes", &self.name.len())
            .field("mesh", &self.mesh)
            .field("positions", &self.morph_positions.len())
            .field("animations", &self.animations.len())
            .field("sources", &self.sources.as_ref().map(Vec::len))
            .finish()
    }
}
impl MorphMesh {
    pub fn from_bytes(bytes: &[u8], game: GameVersion, limits: Limits) -> Result<Self> {
        Self::read(&mut Reader::new(bytes), bytes.len(), game, limits)
    }
    /// MMB is EOF-delimited; length is the exact embedded document span. Any error
    /// rolls the outer reader back, including errors in the consuming material codec.
    pub fn read(
        r: &mut Reader<'_>,
        length: usize,
        game: GameVersion,
        limits: Limits,
    ) -> Result<Self> {
        Self::read_with_address_space(
            r,
            crate::archive::AddressSpace::from_reader(r),
            length,
            game,
            limits,
        )
    }
    /// Read a bounded morph document with an explicit enclosing address space for
    /// embedded material tables. Input must be the identical borrowed subrange.
    pub(crate) fn read_with_address_space(
        r: &mut Reader<'_>,
        space: crate::archive::AddressSpace<'_>,
        length: usize,
        game: GameVersion,
        limits: Limits,
    ) -> Result<Self> {
        animation::window_with_space(r, space, length, limits, |r, b| read(r, game, b))
    }
}
fn read(r: &mut Reader<'_>, game: GameVersion, b: &mut Budget) -> Result<MorphMesh> {
    let mut m = MorphMesh {
        version: 3,
        marker: false,
        name: vec![],
        mesh: MultiResolutionMesh::new(game),
        morph_positions: vec![],
        animations: vec![],
        sources: None,
    };
    let mut seen = 0;
    while r.remaining() != 0 {
        let p = r.position();
        let (id, mut c) = chunk(r, b)?;
        match id {
            0xe000 => {
                once(&mut seen, 1, p)?;
                m.marker = true;
            }
            0xe020 => {
                once(&mut seen, 2, p)?;
                m.version = c.u32(CTX)?;
                if m.version != 3 {
                    return Err(at(ErrorKind::Unsupported, p + 6));
                }
                m.name = line(&mut c, b)?;
            }
            0xb100 => {
                once(&mut seen, 4, p)?;
                m.mesh = crate::mesh::mrm::read_section(&mut c, game, b)?;
            }
            0xb1ff => {
                once(&mut seen, 8, p)?;
                if seen & 4 == 0 {
                    return Err(at(ErrorKind::InvalidData, p));
                }
                m.morph_positions = array(&mut c, m.mesh.positions.len(), 12, b, |r| {
                    Vec3::read(r, CTX)
                })?;
            }
            0xe010 => {
                once(&mut seen, 16, p)?;
                let n = c.u16(CTX)? as usize;
                if n > c.remaining() / 17 {
                    return Err(at(ErrorKind::UnexpectedEof, c.position()));
                }
                let mut s = b.array(n, Some(c.position()))?;
                for _ in 0..n {
                    s.push(Source {
                        date: Date::read(&mut c, CTX)?,
                        name: line(&mut c, b)?,
                    });
                }
                m.sources = Some(s);
            }
            0xe030 => {
                once(&mut seen, 32, p)?;
                let n = c.u16(CTX)? as usize;
                if n > c.remaining() / 30 {
                    return Err(at(ErrorKind::UnexpectedEof, c.position()));
                }
                m.animations = b.array(n, Some(c.position()))?;
                for _ in 0..n {
                    let name = line(&mut c, b)?;
                    let blend_in = c.f32(CTX)?;
                    let blend_out = c.f32(CTX)?;
                    let duration = c.f32(CTX)?;
                    let layer = c.i32(CTX)?;
                    let speed = c.f32(CTX)?;
                    let flags = c.u8(CTX)?;
                    let count = c.u32(CTX)? as usize;
                    let frame_count = c.u32(CTX)?;
                    let samples = count
                        .checked_mul(frame_count as usize)
                        .ok_or_else(|| at(ErrorKind::LimitExceeded, c.position() - 8))?;
                    let bytes = count
                        .checked_mul(4)
                        .and_then(|n| samples.checked_mul(12).and_then(|s| n.checked_add(s)))
                        .ok_or_else(|| at(ErrorKind::LimitExceeded, c.position() - 8))?;
                    if bytes > c.remaining() {
                        return Err(at(ErrorKind::UnexpectedEof, c.position()));
                    }
                    let vertices = array(&mut c, count, 4, b, |r| {
                        let p = r.position();
                        let value = r.u32(CTX)?;
                        if seen & 4 != 0 && value as usize >= m.mesh.positions.len() {
                            return Err(at(ErrorKind::InvalidData, p));
                        }
                        Ok(value)
                    })?;
                    let samples = array(&mut c, samples, 12, b, |r| Vec3::read(r, CTX))?;
                    m.animations.push(MorphAnimation {
                        name,
                        blend_in,
                        blend_out,
                        duration,
                        layer,
                        speed,
                        flags,
                        frame_count,
                        vertices,
                        samples,
                    });
                }
            }
            _ => return Err(at(ErrorKind::Unsupported, p)),
        }
        finish(&c)?;
    }
    if seen & 46 != 46 {
        return Err(at(ErrorKind::InvalidData, r.position()));
    }
    // Validate decoded MRM references without charging its already-retained fields
    // twice. All owned allocations above used the same shared budget.
    crate::mesh::mrm::validate_section(&m.mesh, &mut Budget::new(b.limits))?;
    if m.animations.iter().any(|a| {
        a.vertices
            .iter()
            .any(|&v| v as usize >= m.mesh.positions.len())
    }) {
        return Err(bad(ErrorKind::InvalidData));
    }
    Ok(m)
}
