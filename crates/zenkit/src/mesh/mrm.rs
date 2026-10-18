// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;
/// Recognized MRM wire layouts. All supported Gothic II variants carry an alpha byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum MrmVersion {
    Gothic1 = 0x0305,
    Gothic2Legacy = 0x0705,
    /// Early Gothic II layout found in embedded MDL meshes.
    Gothic2Early = 0x0805,
    Gothic2 = 0x0905,
}
impl MrmVersion {
    pub const fn from_raw(value: u16) -> Option<Self> {
        match value {
            0x0305 => Some(Self::Gothic1),
            0x0705 => Some(Self::Gothic2Legacy),
            0x0805 => Some(Self::Gothic2Early),
            0x0905 => Some(Self::Gothic2),
            _ => None,
        }
    }
    pub const fn raw(self) -> u16 {
        self as u16
    }
    pub const fn game(self) -> GameVersion {
        match self {
            Self::Gothic1 => GameVersion::Gothic1,
            Self::Gothic2Legacy | Self::Gothic2Early | Self::Gothic2 => GameVersion::Gothic2,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Plane {
    pub distance: f32,
    pub normal: Vec3,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wedge {
    pub normal: Vec3,
    pub texture: Vec2,
    pub index: u16,
    pub reserved: u16,
}
#[derive(Clone, PartialEq)]
pub struct SubMesh {
    pub material: MaterialRecord,
    pub triangles: Vec<[u16; 3]>,
    pub wedges: Vec<Wedge>,
    pub colors: Vec<f32>,
    pub triangle_plane_indices: Vec<u16>,
    pub triangle_planes: Vec<Plane>,
    pub wedge_map: Vec<u16>,
    pub triangle_edges: Vec<[u16; 3]>,
    /// Two raw u16 values with uninterpreted referents.
    pub edges: Vec<[u16; 2]>,
    pub edge_scores: Vec<f32>,
}
impl SubMesh {
    pub fn new(material: MaterialRecord) -> Self {
        Self {
            material,
            triangles: Vec::new(),
            wedges: Vec::new(),
            colors: Vec::new(),
            triangle_plane_indices: Vec::new(),
            triangle_planes: Vec::new(),
            wedge_map: Vec::new(),
            triangle_edges: Vec::new(),
            edges: Vec::new(),
            edge_scores: Vec::new(),
        }
    }
}
impl fmt::Debug for SubMesh {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubMesh")
            .field("material", &self.material)
            .field("triangles", &self.triangles.len())
            .field("wedges", &self.wedges.len())
            .field("planes", &self.triangle_planes.len())
            .field("edges", &self.edges.len())
            .finish()
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpaqueSection {
    pub offset: u32,
    pub count: u32,
}
type Region = OpaqueSection;
#[derive(Clone, PartialEq)]
struct Layout {
    content: Vec<u8>,
    positions: Region,
    normals: Region,
    submeshes: Vec<[Region; 10]>,
}
/// MRM sections retain uninterpreted content bytes and vertex-update descriptors.
/// With nonempty opaque updates, changing any decoded content array is Unsupported.
/// Call `discard_vertex_updates` explicitly before editing that geometry. Materials,
/// bounds and metadata remain editable without discarding opaque update data.
#[derive(Clone, PartialEq)]
pub struct MultiResolutionMesh {
    pub game: GameVersion,
    /// Loaded wire version, retained by the writer. Must agree with `game`.
    pub version: MrmVersion,
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub submeshes: Vec<SubMesh>,
    pub material_metadata: Metadata,
    /// G1 has no alpha byte; G2 retains all eight bits rather than normalizing bool.
    pub alpha_test: Option<u8>,
    pub bbox: Aabb,
    pub obb: Obb,
    pub trailer: [u8; 16],
    layout: Option<Layout>,
}
impl fmt::Debug for MultiResolutionMesh {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MultiResolutionMesh")
            .field("game", &self.game)
            .field("version", &self.version)
            .field("positions", &self.positions.len())
            .field("normals", &self.normals.len())
            .field("submeshes", &self.submeshes.len())
            .field(
                "retained_content_bytes",
                &self.layout.as_ref().map(|l| l.content.len()),
            )
            .finish()
    }
}
impl MultiResolutionMesh {
    pub fn new(game: GameVersion) -> Self {
        Self {
            game,
            version: match game {
                GameVersion::Gothic1 => MrmVersion::Gothic1,
                GameVersion::Gothic2 => MrmVersion::Gothic2,
            },
            positions: Vec::new(),
            normals: Vec::new(),
            submeshes: Vec::new(),
            material_metadata: Metadata::new(archive::Format::Binary),
            alpha_test: if game == GameVersion::Gothic2 {
                Some(1)
            } else {
                None
            },
            bbox: Aabb::default(),
            obb: Obb::default(),
            trailer: [0; 16],
            layout: None,
        }
    }
    pub fn original_content(&self) -> Option<&[u8]> {
        self.layout.as_ref().map(|l| l.content.as_slice())
    }
    pub fn vertex_updates(
        &self,
    ) -> impl ExactSizeIterator<Item = (usize, OpaqueSection)> + DoubleEndedIterator + '_ {
        self.layout
            .as_ref()
            .map_or(&[][..], |l| l.submeshes.as_slice())
            .iter()
            .enumerate()
            .map(|(i, s)| (i, s[6]))
    }
    /// Explicitly forget opaque update descriptors, authorizing content edits.
    /// Original uninterpreted bytes stay retained; no record stride is guessed.
    pub fn discard_vertex_updates(&mut self) {
        if let Some(l) = &mut self.layout {
            for s in &mut l.submeshes {
                s[6].count = 0;
            }
        }
    }
}
codec!(MultiResolutionMesh, read, write, validate);
pub(super) fn read(
    r: &mut Reader<'_>,
    game: GameVersion,
    b: &mut Budget,
) -> Result<MultiResolutionMesh> {
    let mut mesh = None;
    loop {
        let p = r.position();
        let (id, mut c) = next(r, b)?;
        match id {
            0xb100 => {
                if mesh.is_some() {
                    return Err(at(ErrorKind::InvalidData, p));
                }
                mesh = Some(read_section(&mut c, game, b)?);
            }
            0xb1ff => {
                end(&c)?;
                return mesh.ok_or_else(|| at(ErrorKind::InvalidData, p));
            }
            _ => return Err(at(ErrorKind::Unsupported, p)),
        }
        end(&c)?;
    }
}
fn region(r: &mut Reader<'_>) -> Result<Region> {
    Ok(Region {
        offset: r.u32(CTX)?,
        count: r.u32(CTX)?,
    })
}
fn section_array<T>(
    content: &Reader<'_>,
    map: Region,
    stride: usize,
    b: &mut Budget,
    read: impl FnMut(&mut Reader<'_>) -> Result<T>,
) -> Result<Vec<T>> {
    if map.count == 0 {
        return Ok(Vec::new());
    }
    let mut r = content.clone();
    let offset = map.offset as usize;
    if offset > r.remaining() {
        return Err(at(ErrorKind::InvalidData, r.position()));
    }
    r.take_bytes(offset, CTX)?;
    array(&mut r, map.count as usize, stride, b, read)
}
pub(crate) fn read_section(
    r: &mut Reader<'_>,
    game: GameVersion,
    b: &mut Budget,
) -> Result<MultiResolutionMesh> {
    let p = r.position();
    let version = MrmVersion::from_raw(r.u16(CTX)?)
        .filter(|v| v.game() == game)
        .ok_or_else(|| at(ErrorKind::Unsupported, p))?;
    let size = r.u32(CTX)? as usize;
    let content = r.substream(size, CTX)?;
    let count = r.u8(CTX)? as usize;
    let positions_map = region(r)?;
    let normals_map = region(r)?;
    let mut maps = b.array::<[Region; 10]>(count, Some(r.position()))?;
    for _ in 0..count {
        let mut m = [Region::default(); 10];
        for entry in &mut m {
            *entry = region(r)?;
        }
        if m[6].count != 0 && m[6].offset as usize >= size {
            return Err(at(ErrorKind::InvalidData, r.position() - 32));
        }
        maps.push(m);
    }
    let (mats, _) = materials(r, game, Some(count), b)?;
    let alpha_test = if game == GameVersion::Gothic2 {
        Some(r.u8(CTX)?)
    } else {
        None
    };
    let bbox = Aabb::read(r, CTX)?;
    let obb = read_obb(r, b)?;
    let trailer = r.take_bytes(16, CTX)?.try_into().expect("exact trailer");
    let positions = section_array(&content, positions_map, 12, b, |r| Vec3::read(r, CTX))?;
    let normals = section_array(&content, normals_map, 12, b, |r| Vec3::read(r, CTX))?;
    // The record slots are temporary transfer storage; their owned buffers move
    // into SubMesh. Account the final slots once, not both representations.
    b.owned -= count * size_of::<MaterialRecord>();
    b.elements -= count;
    let mut submeshes = b.array::<SubMesh>(count, Some(p))?;
    for (m, material) in maps.iter().zip(mats.records) {
        let triangles = section_array(&content, m[0], 6, b, |r| {
            Ok([r.u16(CTX)?, r.u16(CTX)?, r.u16(CTX)?])
        })?;
        let wedges = section_array(&content, m[1], 24, b, |r| {
            Ok(Wedge {
                normal: Vec3::read(r, CTX)?,
                texture: Vec2::read(r, CTX)?,
                index: r.u16(CTX)?,
                reserved: r.u16(CTX)?,
            })
        })?;
        let colors = section_array(&content, m[2], 4, b, |r| r.f32(CTX))?;
        let triangle_plane_indices = section_array(&content, m[3], 2, b, |r| r.u16(CTX))?;
        let triangle_planes = section_array(&content, m[4], 16, b, |r| {
            Ok(Plane {
                distance: r.f32(CTX)?,
                normal: Vec3::read(r, CTX)?,
            })
        })?;
        let wedge_map = section_array(&content, m[5], 2, b, |r| r.u16(CTX))?;
        let triangle_edges = section_array(&content, m[7], 6, b, |r| {
            Ok([r.u16(CTX)?, r.u16(CTX)?, r.u16(CTX)?])
        })?;
        let edges = section_array(&content, m[8], 4, b, |r| Ok([r.u16(CTX)?, r.u16(CTX)?]))?;
        let edge_scores = section_array(&content, m[9], 4, b, |r| r.f32(CTX))?;
        submeshes.push(SubMesh {
            material,
            triangles,
            wedges,
            colors,
            triangle_plane_indices,
            triangle_planes,
            wedge_map,
            triangle_edges,
            edges,
            edge_scores,
        });
    }
    let mut original = content.clone();
    let raw = b.copy(original.take_bytes(size, CTX)?, Some(content.position()))?;
    Ok(MultiResolutionMesh {
        game,
        version,
        positions,
        normals,
        submeshes,
        material_metadata: mats.metadata,
        alpha_test,
        bbox,
        obb,
        trailer,
        layout: Some(Layout {
            content: raw,
            positions: positions_map,
            normals: normals_map,
            submeshes: maps,
        }),
    })
}
pub(super) fn validate(m: &MultiResolutionMesh, b: &mut Budget) -> Result<()> {
    b.chunks(2)?;
    validate_section(m, b)
}
pub(crate) fn validate_section(m: &MultiResolutionMesh, b: &mut Budget) -> Result<()> {
    if m.submeshes.len() > 255 {
        return Err(bad(ErrorKind::LimitExceeded));
    }
    if m.version.game() != m.game || m.alpha_test.is_some() != (m.game == GameVersion::Gothic2) {
        return Err(bad(ErrorKind::Unsupported));
    }
    b.owned(&m.positions)?;
    b.owned(&m.normals)?;
    b.owned(&m.submeshes)?;
    own_metadata(&m.material_metadata, b)?;
    own_obb(&m.obb, b)?;
    if let Some(l) = &m.layout {
        b.raw(&l.content)?;
        b.owned(&l.submeshes)?;
        if l.submeshes.iter().any(|s| s[6].count != 0) && m.submeshes.len() != l.submeshes.len() {
            return Err(bad(ErrorKind::Unsupported));
        }
    }
    for s in &m.submeshes {
        own_record(&s.material, b)?;
        s.material.material.check_game(m.game)?;
        b.owned(&s.triangles)?;
        b.owned(&s.wedges)?;
        b.owned(&s.colors)?;
        b.owned(&s.triangle_plane_indices)?;
        b.owned(&s.triangle_planes)?;
        b.owned(&s.wedge_map)?;
        b.owned(&s.triangle_edges)?;
        b.owned(&s.edges)?;
        b.owned(&s.edge_scores)?;
        if s.triangles
            .iter()
            .flatten()
            .any(|&i| i as usize >= s.wedges.len())
            || s.wedges
                .iter()
                .any(|w| w.index as usize >= m.positions.len())
            || s.triangle_plane_indices
                .iter()
                .any(|&i| i as usize >= s.triangle_planes.len())
            || s.wedge_map
                .iter()
                .any(|&i| i != u16::MAX && i as usize >= s.wedges.len())
            || s.triangle_edges
                .iter()
                .flatten()
                .any(|&i| i as usize >= s.edges.len())
        {
            return Err(bad(ErrorKind::InvalidData));
        }
    }
    Ok(())
}
pub(super) fn write(m: &MultiResolutionMesh, w: &mut Writer, limits: Limits) -> Result<()> {
    chunk(w, 0xb100, |w| write_section(m, w, limits))?;
    chunk(w, 0xb1ff, |_| Ok(()))
}
fn encoded(limit: usize, write: impl FnOnce(&mut Writer) -> Result<()>) -> Result<Vec<u8>> {
    let mut w = Writer::new(limit);
    write(&mut w)?;
    Ok(w.into_inner())
}
fn insert(
    content: &mut Writer,
    old: Option<Region>,
    original: Option<&[u8]>,
    count: usize,
    bytes: &[u8],
    opaque: bool,
) -> Result<Region> {
    if let (Some(old), Some(raw)) = (old, original) {
        let offset = old.offset as usize;
        if old.count as usize == count
            && raw.get(offset..offset.saturating_add(bytes.len())) == Some(bytes)
        {
            return Ok(old);
        }
        if count == 0 && old.count == 0 {
            return Ok(old);
        }
    }
    if opaque {
        return Err(bad(ErrorKind::Unsupported));
    }
    let map = Region {
        offset: u32n(content.position())?,
        count: u32n(count)?,
    };
    content.bytes(bytes, CTX)?;
    Ok(map)
}
pub(super) fn write_section(m: &MultiResolutionMesh, w: &mut Writer, limits: Limits) -> Result<()> {
    if m.material_metadata.format != archive::Format::Binary {
        return Err(bad(ErrorKind::Unsupported));
    }
    let original = m.layout.as_ref().map(|l| l.content.as_slice());
    let opaque = m
        .layout
        .as_ref()
        .is_some_and(|l| l.submeshes.iter().any(|s| s[6].count != 0));
    let mut content = Writer::new(limits.bytes);
    if let Some(raw) = original {
        content.bytes(raw, CTX)?;
    }
    let positions = encoded(limits.bytes, |w| {
        for p in &m.positions {
            p.write(w, CTX)?;
        }
        Ok(())
    })?;
    let positions = insert(
        &mut content,
        m.layout.as_ref().map(|l| l.positions),
        original,
        m.positions.len(),
        &positions,
        opaque,
    )?;
    let normals = encoded(limits.bytes, |w| {
        for p in &m.normals {
            p.write(w, CTX)?;
        }
        Ok(())
    })?;
    let normals = insert(
        &mut content,
        m.layout.as_ref().map(|l| l.normals),
        original,
        m.normals.len(),
        &normals,
        opaque,
    )?;
    let mut maps = Vec::new();
    maps.try_reserve_exact(m.submeshes.len())
        .map_err(|_| bad(ErrorKind::LimitExceeded))?;
    for (i, s) in m.submeshes.iter().enumerate() {
        let old = m.layout.as_ref().and_then(|l| l.submeshes.get(i));
        let mut map = [Region::default(); 10];
        let mut store = |slot: usize, count: usize, bytes: Vec<u8>| -> Result<()> {
            map[slot] = insert(
                &mut content,
                old.map(|s| s[slot]),
                original,
                count,
                &bytes,
                opaque,
            )?;
            Ok(())
        };
        store(
            0,
            s.triangles.len(),
            encoded(limits.bytes, |w| {
                for t in &s.triangles {
                    for i in t {
                        w.u16(*i, CTX)?;
                    }
                }
                Ok(())
            })?,
        )?;
        store(
            1,
            s.wedges.len(),
            encoded(limits.bytes, |w| {
                for v in &s.wedges {
                    v.normal.write(w, CTX)?;
                    v.texture.write(w, CTX)?;
                    w.u16(v.index, CTX)?;
                    w.u16(v.reserved, CTX)?;
                }
                Ok(())
            })?,
        )?;
        store(
            2,
            s.colors.len(),
            encoded(limits.bytes, |w| {
                for v in &s.colors {
                    w.f32(*v, CTX)?;
                }
                Ok(())
            })?,
        )?;
        store(
            3,
            s.triangle_plane_indices.len(),
            encoded(limits.bytes, |w| {
                for v in &s.triangle_plane_indices {
                    w.u16(*v, CTX)?;
                }
                Ok(())
            })?,
        )?;
        store(
            4,
            s.triangle_planes.len(),
            encoded(limits.bytes, |w| {
                for v in &s.triangle_planes {
                    w.f32(v.distance, CTX)?;
                    v.normal.write(w, CTX)?;
                }
                Ok(())
            })?,
        )?;
        store(
            5,
            s.wedge_map.len(),
            encoded(limits.bytes, |w| {
                for v in &s.wedge_map {
                    w.u16(*v, CTX)?;
                }
                Ok(())
            })?,
        )?;
        store(
            7,
            s.triangle_edges.len(),
            encoded(limits.bytes, |w| {
                for t in &s.triangle_edges {
                    for v in t {
                        w.u16(*v, CTX)?;
                    }
                }
                Ok(())
            })?,
        )?;
        store(
            8,
            s.edges.len(),
            encoded(limits.bytes, |w| {
                for t in &s.edges {
                    for v in t {
                        w.u16(*v, CTX)?;
                    }
                }
                Ok(())
            })?,
        )?;
        store(
            9,
            s.edge_scores.len(),
            encoded(limits.bytes, |w| {
                for v in &s.edge_scores {
                    w.f32(*v, CTX)?;
                }
                Ok(())
            })?,
        )?;
        map[6] = old.map_or(Region::default(), |s| s[6]);
        maps.push(map);
    }
    w.u16(m.version.raw(), CTX)?;
    w.u32(u32n(content.as_slice().len())?, CTX)?;
    w.bytes(content.as_slice(), CTX)?;
    w.u8(m.submeshes.len() as u8, CTX)?;
    for r in [positions, normals]
        .into_iter()
        .chain(maps.into_iter().flatten())
    {
        w.u32(r.offset, CTX)?;
        w.u32(r.count, CTX)?;
    }
    let mut ar = archive::Writer::new(
        m.material_metadata.as_header(),
        archive_limits(&Budget::new(limits)),
    )?;
    for s in &m.submeshes {
        s.material.write(&mut ar, m.game, limits.owned_bytes)?;
    }
    w.bytes(&ar.finish()?, CTX)?;
    if let Some(alpha) = m.alpha_test {
        w.u8(alpha, CTX)?;
    }
    m.bbox.write(w, CTX)?;
    write_obb(&m.obb, w, limits)?;
    w.bytes(&m.trailer, CTX)
}
