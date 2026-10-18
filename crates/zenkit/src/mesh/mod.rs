//! Bounded MSH/MRM, soft-skin and MDH/MDM/MDL storage.
//!
//! Public records are editable drafts. Read/write validates structural references,
//! versions and finite resource limits; geometry and float bits are not normalized.
//! Unknown chunks and surplus known payloads return Unsupported, except for the
//! explicitly retained opaque MSH material-chunk suffix and MRM backing data.
//! MSH stores G2's raw alpha-test byte separately from its opaque material suffix.
//! New G2 meshes enable testing; older inputs without the byte remain byte-exact
//! on rewrite and use an effective enabled default. G1 has no alpha-test byte.
//! MRM versions 0x0305 (G1), 0x0705/0x0805/0x0905 (G2) are retained on write.
//! All supported G2 layouts include the alpha byte. Model-mesh headers retain
//! their legacy, early G2 or G2 version; new meshes default to the legacy header.
//! The model header and embedded MRM versions are edited independently.
//! Binary material 0x7401 retains five unclassified parameter bytes; no modern
//! wave/alpha interpretation or tagged encoding conversion is inferred.
//! Skin weights store skeleton node IDs. Standalone skins/model meshes cannot
//! bound these against a hierarchy; `Model` checks both weights and declared
//! node IDs against its hierarchy. No local-list membership rule is imposed.
//! Embedded materials can be read from ASCII, BINARY and BIN_SAFE archives.
//! MSH requires one blank ASCII header separator before its raw u32 count.
//! Writers emit BINARY material archives only: set `materials.metadata.format`
//! (or MRM `material_metadata.format`) to `archive::Format::Binary` before writing
//! a tagged input. Binary representability checks still apply.
//! For G2 MSH with no alpha byte and a retained BIN_SAFE table in
//! `material_tail`, also set `alpha_test` to `Some(1)` to preserve its enabled
//! default. BIN_SAFE key-table layout is not promised byte-identical relocation.
//! Physical table lookups share bounded work across the complete nested document.
//!
//! `read` is transactional and leaves an enclosing suffix available. `from_bytes`
//! additionally requires the complete supplied buffer to belong to one document.
//! `validate` checks semantic references, retained ownership and chunk budgets;
//! encoding also checks string representability, output size, texture limits and
//! MRM opaque-update edit guards. Structural validation errors on edited values
//! have no byte offset.
//!
//! ```
//! use zenkit::{GameVersion, mesh::{Limits, MultiResolutionMesh}};
//! let mesh = MultiResolutionMesh::new(GameVersion::Gothic1);
//! let bytes = mesh.to_bytes(Limits::default())?;
//! let loaded = MultiResolutionMesh::from_bytes(
//!     &bytes, GameVersion::Gothic1, Limits::default())?;
//! assert!(loaded.positions.is_empty());
//! # Ok::<(), zenkit::Error>(())
//! ```
// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
mod model;
pub(crate) mod mrm;
pub(crate) mod msh;
use crate::{
    Error, ErrorKind, GameVersion, Result, archive,
    io::{LineEnding, Reader, Writer},
    material::MaterialRecord,
    objects::Metadata,
    values::{Aabb, Date, Mat4, Obb, ObbLimits, Vec2, Vec3},
};
pub use model::{
    Attachment, HierarchyNode, Model, ModelHierarchy, ModelMesh, ModelMeshVersion, SoftSkinMesh,
    Source, WedgeNormal, Weight,
};
pub use mrm::{MrmVersion, MultiResolutionMesh, OpaqueSection, Plane, SubMesh, Wedge};
pub use msh::{
    Corner, Feature, Lightmap, LightmapEncoding, Lightmaps, Mesh, Polygon, PolygonFlags, Triangle,
};
use std::{fmt, io::SeekFrom, mem::size_of};
const CTX: &str = "mesh";
fn bad(kind: ErrorKind) -> Error {
    Error::operation(kind, CTX)
}
fn at(kind: ErrorKind, p: usize) -> Error {
    Error::new(kind, p, CTX)
}
fn add(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b)
        .ok_or_else(|| bad(ErrorKind::LimitExceeded))
}
fn mul(a: usize, b: usize) -> Result<usize> {
    a.checked_mul(b)
        .ok_or_else(|| bad(ErrorKind::LimitExceeded))
}
fn u32n(n: usize) -> Result<u32> {
    u32::try_from(n).map_err(|_| bad(ErrorKind::LimitExceeded))
}
fn u16n(n: usize) -> Result<u16> {
    u16::try_from(n).map_err(|_| bad(ErrorKind::LimitExceeded))
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Maximum complete input/output and each bounded serialization scratch buffer.
    pub bytes: usize,
    /// Aggregate logical owned allocation, including retained original MRM buffers.
    /// Spare capacity and temporary serialization buffers are not allocator quotas.
    pub owned_bytes: usize,
    /// Aggregate typed array elements across the complete nested document.
    pub elements: usize,
    /// Aggregate chunk headers across the document, including nested skin streams.
    pub chunks: usize,
    /// Maximum raw line or material archive field length, without text decoding.
    pub string_bytes: usize,
    /// Maximum depth of each OBB, also capped by `Obb::MAX_DEPTH`.
    pub obb_depth: usize,
    /// Per-texture layout limits, additionally capped by the shared owned budget.
    pub texture: crate::texture::Limits,
    /// Per embedded table key limit; two index vectors can coexist during parsing.
    pub material_table_keys: usize,
    /// Cumulative inspected table entries, including repeated table lookups.
    pub material_table_entries: usize,
    /// Cumulative inspected table bytes across all nested material archives.
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
            obb_depth: 128,
            texture: Default::default(),
            material_table_keys: 65_536,
            material_table_entries: 1_000_000,
            material_table_bytes: 256 * 1024 * 1024,
        }
    }
}
pub(crate) struct Budget<'a> {
    pub(crate) limits: Limits,
    pub(crate) owned: usize,
    pub(crate) elements: usize,
    pub(crate) chunks: usize,
    pub(crate) space: Option<archive::AddressSpace<'a>>,
    pub(crate) tables: archive::TableBudget,
}
impl<'a> Budget<'a> {
    pub(crate) fn new(limits: Limits) -> Self {
        Self {
            limits,
            owned: 0,
            elements: 0,
            chunks: 0,
            space: None,
            tables: archive::TableBudget::new(
                limits.material_table_entries,
                limits.material_table_bytes,
            ),
        }
    }
    pub(crate) fn with_space(limits: Limits, space: archive::AddressSpace<'a>) -> Self {
        let mut result = Self::new(limits);
        result.space = Some(space);
        result
    }
    pub(crate) fn charge(&mut self, bytes: usize, items: usize, p: Option<usize>) -> Result<()> {
        let owned = self.owned.checked_add(bytes);
        let elements = self.elements.checked_add(items);
        if owned.is_none_or(|n| n > self.limits.owned_bytes)
            || elements.is_none_or(|n| n > self.limits.elements)
        {
            return Err(p.map_or_else(
                || bad(ErrorKind::LimitExceeded),
                |p| at(ErrorKind::LimitExceeded, p),
            ));
        }
        self.owned = owned.unwrap();
        self.elements = elements.unwrap();
        Ok(())
    }
    pub(crate) fn array<T>(&mut self, n: usize, p: Option<usize>) -> Result<Vec<T>> {
        let bytes = n.checked_mul(size_of::<T>()).ok_or_else(|| {
            p.map_or_else(
                || bad(ErrorKind::LimitExceeded),
                |p| at(ErrorKind::LimitExceeded, p),
            )
        })?;
        self.charge(bytes, n, p)?;
        let mut v = Vec::new();
        v.try_reserve_exact(n).map_err(|_| {
            p.map_or_else(
                || bad(ErrorKind::LimitExceeded),
                |p| at(ErrorKind::LimitExceeded, p),
            )
        })?;
        Ok(v)
    }
    pub(crate) fn copy(&mut self, bytes: &[u8], p: Option<usize>) -> Result<Vec<u8>> {
        self.charge(bytes.len(), 0, p)?;
        let mut v = Vec::new();
        v.try_reserve_exact(bytes.len())
            .map_err(|_| bad(ErrorKind::LimitExceeded))?;
        v.extend_from_slice(bytes);
        Ok(v)
    }
    pub(crate) fn owned<T>(&mut self, v: &[T]) -> Result<()> {
        self.charge(mul(v.len(), size_of::<T>())?, v.len(), None)
    }
    pub(crate) fn raw(&mut self, v: &[u8]) -> Result<()> {
        self.charge(v.len(), 0, None)
    }
    fn chunks(&mut self, count: usize) -> Result<()> {
        let total = add(self.chunks, count)?;
        if total > self.limits.chunks {
            return Err(bad(ErrorKind::LimitExceeded));
        }
        self.chunks = total;
        Ok(())
    }
}
fn array<T>(
    r: &mut Reader<'_>,
    n: usize,
    stride: usize,
    b: &mut Budget,
    mut read: impl FnMut(&mut Reader<'_>) -> Result<T>,
) -> Result<Vec<T>> {
    let p = r.position();
    let bytes = n
        .checked_mul(stride)
        .ok_or_else(|| at(ErrorKind::LimitExceeded, p))?;
    if bytes > r.remaining() {
        return Err(at(ErrorKind::UnexpectedEof, p));
    }
    let mut v = b.array::<T>(n, Some(p))?;
    for _ in 0..n {
        v.push(read(r)?);
    }
    Ok(v)
}
fn line(r: &mut Reader<'_>, b: &mut Budget) -> Result<Vec<u8>> {
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
fn write_line(w: &mut Writer, v: &[u8], limits: Limits) -> Result<()> {
    if v.iter().any(|c| matches!(c, 0 | b'\r' | b'\n')) {
        return Err(bad(ErrorKind::InvalidData));
    }
    w.line(v, LineEnding::Lf, limits.string_bytes, CTX)
}
fn next<'a>(r: &mut Reader<'a>, b: &mut Budget) -> Result<(u16, Reader<'a>)> {
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
fn end(r: &Reader<'_>) -> Result<()> {
    if r.remaining() != 0 {
        Err(at(ErrorKind::Unsupported, r.position()))
    } else {
        Ok(())
    }
}
fn once(seen: &mut u32, bit: u32, p: usize) -> Result<()> {
    if *seen & bit != 0 {
        return Err(at(ErrorKind::InvalidData, p));
    }
    *seen |= bit;
    Ok(())
}
fn chunk(w: &mut Writer, id: u16, write: impl FnOnce(&mut Writer) -> Result<()>) -> Result<()> {
    w.u16(id, CTX)?;
    let offset = w.position();
    w.u32(0, CTX)?;
    write(w)?;
    let end = w.position();
    w.seek_to(SeekFrom::Start(offset as u64), CTX)?;
    w.u32(u32n(end - offset - 4)?, CTX)?;
    w.seek_to(SeekFrom::Start(end as u64), CTX)?;
    Ok(())
}
fn read_obb(r: &mut Reader<'_>, b: &mut Budget) -> Result<Obb> {
    let p = r.position();
    let cap = (b.limits.owned_bytes - b.owned) / size_of::<Obb>();
    let nodes = cap.min(b.limits.elements - b.elements);
    let v = Obb::read(
        r,
        ObbLimits {
            nodes,
            depth: b.limits.obb_depth,
        },
        CTX,
    )?;
    let count = (r.position() - p) / 62;
    b.charge(mul(count, size_of::<Obb>())?, count, Some(p))?;
    Ok(v)
}
fn own_obb(v: &Obb, b: &mut Budget) -> Result<()> {
    let mut pending = vec![(v, 1usize)];
    while let Some((n, depth)) = pending.pop() {
        if depth > b.limits.obb_depth.min(Obb::MAX_DEPTH) {
            return Err(bad(ErrorKind::LimitExceeded));
        }
        b.charge(size_of::<Obb>(), 1, None)?;
        pending
            .try_reserve(n.children.len())
            .map_err(|_| bad(ErrorKind::LimitExceeded))?;
        pending.extend(n.children.iter().map(|n| (n, depth + 1)));
    }
    Ok(())
}
fn write_obb(v: &Obb, w: &mut Writer, limits: Limits) -> Result<()> {
    v.write(
        w,
        ObbLimits {
            nodes: limits.elements,
            depth: limits.obb_depth,
        },
        CTX,
    )
}
fn check_game(version: u16, game: GameVersion, g1: u16, g2: u16, p: usize) -> Result<()> {
    if version != if game == GameVersion::Gothic1 { g1 } else { g2 } {
        Err(at(ErrorKind::Unsupported, p))
    } else {
        Ok(())
    }
}
fn archive_limits(b: &Budget) -> archive::Limits {
    archive::Limits {
        bytes: b.limits.bytes,
        field_bytes: b.limits.string_bytes,
        objects: b.limits.elements,
        keys: b.limits.material_table_keys,
        ..Default::default()
    }
}
fn own_record(v: &MaterialRecord, b: &mut Budget) -> Result<()> {
    b.raw(&v.prefix)?;
    b.raw(&v.object_name)?;
    if let Some(n) = &v.prefix_field_name {
        b.raw(n)?;
    }
    for n in v.material.buffers() {
        b.raw(n)?;
    }
    Ok(())
}
fn own_metadata(v: &Metadata, b: &mut Budget) -> Result<()> {
    b.raw(&v.archiver)?;
    if let Some(v) = &v.date {
        b.raw(v)?;
    }
    if let Some(v) = &v.user {
        b.raw(v)?;
    }
    Ok(())
}
/// Embedded material archive, retaining metadata and external name prefixes.
#[derive(Clone, PartialEq)]
pub struct Materials {
    pub metadata: Metadata,
    pub records: Vec<MaterialRecord>,
}
impl Default for Materials {
    fn default() -> Self {
        Self {
            metadata: Metadata::new(archive::Format::Binary),
            records: Vec::new(),
        }
    }
}
impl fmt::Debug for Materials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Materials")
            .field("metadata", &self.metadata)
            .field("records", &self.records.len())
            .finish()
    }
}
fn materials(
    r: &mut Reader<'_>,
    game: GameVersion,
    count: Option<usize>,
    b: &mut Budget,
) -> Result<(Materials, Option<std::ops::Range<usize>>)> {
    let start = r.position();
    let space = b
        .space
        .unwrap_or_else(|| archive::AddressSpace::from_reader(r));
    if !space.contains(r) {
        return Err(bad(ErrorKind::InvalidData));
    }
    let mut limits = archive_limits(b);
    // The outer owner bounds the physical resolver; geometry bytes still bound
    // sequential bodies. A world's address space can exceed its geometry cap.
    limits.bytes = space.range().len();
    let mut ar = archive::Reader::embedded(
        space,
        start..start + r.remaining(),
        if count.is_none() {
            archive::AsciiSeparator::Required
        } else {
            archive::AsciiSeparator::Optional
        },
        limits,
        &mut b.tables,
    )?;
    let h = ar.header();
    let metadata = Metadata {
        version: h.version,
        archiver: b.copy(h.archiver, r.offset_of(h.archiver))?,
        format: h.format,
        save_game: h.save_game,
        date: h.date.map(|v| b.copy(v, r.offset_of(v))).transpose()?,
        user: h.user.map(|v| b.copy(v, r.offset_of(v))).transpose()?,
    };
    let count = match count {
        Some(n) => n,
        None => {
            let p = ar.position();
            let n = ar.unframed(4)?.u32(CTX)?;
            usize::try_from(n).map_err(|_| at(ErrorKind::LimitExceeded, p))?
        }
    };
    // Even an empty prefix and object require a NUL plus the fixed frame header
    // and two NUL-terminated names. Reject impossible counts before allocating.
    if count > ar.remaining_bytes() / 13 {
        return Err(at(ErrorKind::UnexpectedEof, ar.position()));
    }
    let mut records = b.array(count, Some(ar.position()))?;
    for _ in 0..count {
        let record = MaterialRecord::read(&mut ar, game, b.limits.owned_bytes - b.owned)?;
        own_record(&record, b)?;
        records.push(record);
    }
    ar.finish_records()?;
    let table = ar.table_range();
    r.take_bytes(ar.position() - start, CTX)?;
    Ok((Materials { metadata, records }, table))
}
fn write_materials(
    v: &Materials,
    w: &mut Writer,
    game: GameVersion,
    count: bool,
    limits: Limits,
) -> Result<()> {
    if v.metadata.format != archive::Format::Binary {
        return Err(bad(ErrorKind::Unsupported));
    }
    let b = Budget::new(limits);
    let mut ar = archive::Writer::new(v.metadata.as_header(), archive_limits(&b))?;
    if count {
        ar.unframed(&u32n(v.records.len())?.to_le_bytes())?;
    }
    for record in &v.records {
        record.write(&mut ar, game, limits.owned_bytes)?;
    }
    w.bytes(&ar.finish()?, CTX)
}
fn validate_materials(v: &Materials, game: GameVersion, b: &mut Budget) -> Result<()> {
    own_metadata(&v.metadata, b)?;
    b.owned(&v.records)?;
    for r in &v.records {
        r.material.check_game(game)?;
        own_record(r, b)?;
    }
    Ok(())
}
macro_rules! codec {
    ($ty:ty,$read:path,$write:path,$validate:path) => {
        impl $ty {
            pub fn from_bytes(bytes: &[u8], game: GameVersion, limits: Limits) -> Result<Self> {
                if bytes.len() > limits.bytes {
                    return Err(at(ErrorKind::LimitExceeded, 0));
                }
                let mut r = Reader::new(bytes);
                let value = Self::read(&mut r, game, limits)?;
                end(&r)?;
                Ok(value)
            }
            /// Transactional embedded read; any enclosing suffix remains available.
            pub fn read(input: &mut Reader<'_>, game: GameVersion, limits: Limits) -> Result<Self> {
                Self::read_with_address_space(
                    input,
                    archive::AddressSpace::from_reader(input),
                    game,
                    limits,
                )
            }
            /// Read a bounded body with an explicitly supplied enclosing physical
            /// address space for remote material tables. The input window must be
            /// the identical borrowed subrange, not merely equal bytes.
            pub(crate) fn read_with_address_space(
                input: &mut Reader<'_>,
                space: archive::AddressSpace<'_>,
                game: GameVersion,
                limits: Limits,
            ) -> Result<Self> {
                if !space.contains(input) {
                    return Err(bad(ErrorKind::InvalidData));
                }
                let mut source = input.clone();
                let start = source.position();
                let mut r = source.substream(source.remaining().min(limits.bytes), CTX)?;
                let mut b = Budget::with_space(limits, space);
                let value = $read(&mut r, game, &mut b).map_err(|mut e| {
                    if e.kind == ErrorKind::UnexpectedEof && input.remaining() > limits.bytes {
                        e.kind = ErrorKind::LimitExceeded;
                    }
                    e
                })?;
                $validate(&value, &mut Budget::new(limits))?;
                input.take_bytes(r.position() - start, CTX)?;
                Ok(value)
            }
            pub fn validate(&self, limits: Limits) -> Result<()> {
                $validate(self, &mut Budget::new(limits))
            }
            /// Encode a complete document. Failure returns no partial output.
            /// MRM content with opaque updates additionally requires unchanged
            /// decoded section bytes until updates are explicitly discarded.
            pub fn to_bytes(&self, limits: Limits) -> Result<Vec<u8>> {
                self.validate(limits)?;
                let mut w = Writer::new(limits.bytes);
                $write(self, &mut w, limits)?;
                Ok(w.into_inner())
            }
        }
    };
}
use codec;
