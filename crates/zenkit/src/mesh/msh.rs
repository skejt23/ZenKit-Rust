// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;
use crate::texture::Texture;
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Feature {
    pub texture: Vec2,
    pub light: u32,
    pub normal: Vec3,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Corner {
    pub vertex: u32,
    pub feature: u32,
}
/// Raw flags preserve contradictory G1 normal-axis bits without guessing semantics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolygonFlags {
    Gothic1 { raw: [u8; 2], sector_index: i16 },
    Gothic2 { raw: u8, sector_index: i16 },
}
impl PolygonFlags {
    pub fn raw_first(self) -> u8 {
        match self {
            Self::Gothic1 { raw, .. } => raw[0],
            Self::Gothic2 { raw, .. } => raw,
        }
    }
    pub fn sector_index(self) -> i16 {
        match self {
            Self::Gothic1 { sector_index, .. } | Self::Gothic2 { sector_index, .. } => sector_index,
        }
    }
}
#[derive(Clone, PartialEq)]
pub struct Polygon {
    pub material: u16,
    pub lightmap: i16,
    pub plane: Plane,
    pub flags: PolygonFlags,
    pub corners: Vec<Corner>,
}
impl fmt::Debug for Polygon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Polygon")
            .field("material", &self.material)
            .field("lightmap", &self.lightmap)
            .field("flags", &self.flags)
            .field("corners", &self.corners.len())
            .finish()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightmapEncoding {
    Embedded,
    Shared,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lightmap {
    pub texture: u32,
    pub origin: Vec3,
    pub normals: [Vec3; 2],
}
#[derive(Clone, PartialEq)]
pub struct Lightmaps {
    pub encoding: LightmapEncoding,
    pub textures: Vec<Texture>,
    pub maps: Vec<Lightmap>,
}
impl fmt::Debug for Lightmaps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lightmaps")
            .field("encoding", &self.encoding)
            .field("textures", &self.textures.len())
            .field("maps", &self.maps.len())
            .finish()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Triangle {
    pub material: u16,
    pub lightmap: i16,
    pub flags: PolygonFlags,
    pub corners: [Corner; 3],
}
#[derive(Clone, PartialEq)]
pub struct Mesh {
    pub game: GameVersion,
    pub wide_indices: bool,
    pub date: Date,
    pub name: Vec<u8>,
    pub bbox: Aabb,
    pub obb: Obb,
    pub materials: Materials,
    /// Raw G2 alpha-test byte: zero disables testing, any nonzero value enables it.
    /// G1 must use `None`. Older G2 inputs without this byte also retain `None`;
    /// their effective value is enabled. New G2 meshes use `Some(1)`.
    pub alpha_test: Option<u8>,
    /// Opaque bytes after the counted materials and, in G2, the alpha-test byte.
    /// Retained within the chunk and ownership budgets. A BIN_SAFE input can
    /// omit alpha before an adjacent key table. Before writing such a value as
    /// BINARY, set `alpha_test` to `Some(1)` to preserve its enabled default.
    /// BINARY output requires a present alpha byte for a nonempty G2 tail.
    pub material_tail: Vec<u8>,
    pub lightmaps: Option<Lightmaps>,
    pub vertices: Vec<Vec3>,
    pub features: Vec<Feature>,
    pub polygons: Vec<Polygon>,
}
impl fmt::Debug for Mesh {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mesh")
            .field("game", &self.game)
            .field("name_bytes", &self.name.len())
            .field("alpha_test", &self.alpha_test)
            .field("material_tail_bytes", &self.material_tail.len())
            .field("vertices", &self.vertices.len())
            .field("features", &self.features.len())
            .field("polygons", &self.polygons.len())
            .finish()
    }
}
impl Mesh {
    pub fn new(game: GameVersion) -> Self {
        Self::with_materials(game, Materials::default())
    }
    fn with_materials(game: GameVersion, materials: Materials) -> Self {
        Self {
            game,
            wide_indices: game == GameVersion::Gothic2,
            date: Date::default(),
            name: Vec::new(),
            bbox: Aabb::default(),
            obb: Obb::default(),
            materials,
            alpha_test: (game == GameVersion::Gothic2).then_some(1),
            material_tail: Vec::new(),
            lightmaps: None,
            vertices: Vec::new(),
            features: Vec::new(),
            polygons: Vec::new(),
        }
    }
    /// Effective alpha testing, defaulting to enabled when no byte was stored.
    /// The raw `alpha_test` field retains noncanonical nonzero values on write.
    pub fn alpha_test_enabled(&self) -> bool {
        self.alpha_test.is_none_or(|value| value != 0)
    }
    pub fn from_bytes(
        bytes: &[u8],
        game: GameVersion,
        force_wide_indices: bool,
        limits: Limits,
    ) -> Result<Self> {
        if bytes.len() > limits.bytes {
            return Err(at(ErrorKind::LimitExceeded, 0));
        }
        let mut r = Reader::new(bytes);
        let m = Self::read(&mut r, game, force_wide_indices, limits)?;
        end(&r)?;
        Ok(m)
    }
    /// Transactional read. G1 wide indices are an explicit enclosing-format choice.
    pub fn read(
        input: &mut Reader<'_>,
        game: GameVersion,
        force_wide_indices: bool,
        limits: Limits,
    ) -> Result<Self> {
        Self::read_with_address_space(
            input,
            archive::AddressSpace::from_reader(input),
            game,
            force_wide_indices,
            limits,
        )
    }
    /// Read with an explicit enclosing table resolver. The input must be its
    /// actual borrowed subrange; equal bytes from another allocation are rejected.
    pub(crate) fn read_with_address_space(
        input: &mut Reader<'_>,
        space: archive::AddressSpace<'_>,
        game: GameVersion,
        force_wide_indices: bool,
        limits: Limits,
    ) -> Result<Self> {
        if !space.contains(input) {
            return Err(bad(ErrorKind::InvalidData));
        }
        let mut source = input.clone();
        let start = source.position();
        let mut r = source.substream(source.remaining().min(limits.bytes), CTX)?;
        let value = read(
            &mut r,
            game,
            force_wide_indices,
            &mut Budget::with_space(limits, space),
        )
        .map_err(|mut e| {
            if e.kind == ErrorKind::UnexpectedEof && input.remaining() > limits.bytes {
                e.kind = ErrorKind::LimitExceeded;
            }
            e
        })?;
        value.validate(limits)?;
        input.take_bytes(r.position() - start, CTX)?;
        Ok(value)
    }
    pub fn validate(&self, limits: Limits) -> Result<()> {
        validate(self, &mut Budget::new(limits))
    }
    pub fn to_bytes(&self, limits: Limits) -> Result<Vec<u8>> {
        self.validate(limits)?;
        let mut w = Writer::new(limits.bytes);
        write(self, &mut w, limits)?;
        Ok(w.into_inner())
    }
    /// Derive selected leaf triangle fans, in mesh order. Portal,
    /// ghost and outdoor polygons and polygons with fewer than 3 corners are skipped.
    /// Duplicate/out-of-range selections are errors; max_triangles bounds output.
    /// Raw polygons remain authoritative and unmodified.
    /// Selection and output are checked here; call `validate` separately to check
    /// the draft's material, lightmap and corner references.
    pub fn triangulate_leaves(
        &self,
        selection: &[u32],
        max_triangles: usize,
    ) -> Result<Vec<Triangle>> {
        if selection.len() > self.polygons.len() {
            return Err(bad(ErrorKind::InvalidData));
        }
        let mut ids = Vec::new();
        ids.try_reserve_exact(selection.len())
            .map_err(|_| bad(ErrorKind::LimitExceeded))?;
        ids.extend_from_slice(selection);
        ids.sort_unstable();
        if ids.windows(2).any(|w| w[0] == w[1])
            || ids
                .last()
                .is_some_and(|&i| i as usize >= self.polygons.len())
        {
            return Err(bad(ErrorKind::InvalidData));
        }
        let included = |p: &Polygon| p.corners.len() >= 3 && p.flags.raw_first() & 0x63 == 0;
        let count = ids
            .iter()
            .filter_map(|&i| {
                let p = &self.polygons[i as usize];
                included(p).then(|| p.corners.len() - 2)
            })
            .try_fold(0usize, add)?;
        if count > max_triangles {
            return Err(bad(ErrorKind::LimitExceeded));
        }
        let mut triangles = Vec::new();
        triangles
            .try_reserve_exact(count)
            .map_err(|_| bad(ErrorKind::LimitExceeded))?;
        for i in ids {
            let p = &self.polygons[i as usize];
            if included(p) {
                for j in 2..p.corners.len() {
                    triangles.push(Triangle {
                        material: p.material,
                        lightmap: p.lightmap,
                        flags: p.flags,
                        corners: [p.corners[0], p.corners[j - 1], p.corners[j]],
                    });
                }
            }
        }
        Ok(triangles)
    }
}
fn texture(r: &mut Reader<'_>, b: &mut Budget) -> Result<Texture> {
    let p = r.position();
    let mut header = r.clone();
    header.take_bytes(20, CTX)?;
    let count = header.u32(CTX)?.max(1) as usize;
    if count > b.limits.texture.mipmaps {
        return Err(at(ErrorKind::LimitExceeded, p + 20));
    }
    b.charge(mul(count, size_of::<Vec<u8>>())?, count, Some(p + 20))?;
    let limits = crate::texture::Limits {
        bytes: b
            .limits
            .texture
            .bytes
            .min(b.limits.bytes)
            .min((b.limits.owned_bytes - b.owned).saturating_add(36)),
        ..b.limits.texture
    };
    let t = Texture::read(r, limits)?;
    b.charge(r.position() - p - 36, 0, Some(p))?;
    Ok(t)
}
fn read_lights(r: &mut Reader<'_>, shared: bool, b: &mut Budget) -> Result<Lightmaps> {
    let count = r.u32(CTX)? as usize;
    if count > r.remaining() / if shared { 36 } else { 72 } {
        return Err(at(ErrorKind::UnexpectedEof, r.position()));
    }
    let mut textures = b.array(count, Some(r.position()))?;
    let mut maps;
    if shared {
        for _ in 0..count {
            textures.push(texture(r, b)?);
        }
        let count = r.u32(CTX)? as usize;
        maps = array(r, count, 40, b, |r| {
            Ok(Lightmap {
                origin: Vec3::read(r, CTX)?,
                normals: [Vec3::read(r, CTX)?, Vec3::read(r, CTX)?],
                texture: r.u32(CTX)?,
            })
        })?;
    } else {
        maps = b.array(count, Some(r.position()))?;
        for i in 0..count {
            let origin = Vec3::read(r, CTX)?;
            let normals = [Vec3::read(r, CTX)?, Vec3::read(r, CTX)?];
            textures.push(texture(r, b)?);
            maps.push(Lightmap {
                texture: i as u32,
                origin,
                normals,
            });
        }
    }
    Ok(Lightmaps {
        encoding: if shared {
            LightmapEncoding::Shared
        } else {
            LightmapEncoding::Embedded
        },
        textures,
        maps,
    })
}
pub(crate) fn read(
    r: &mut Reader<'_>,
    game: GameVersion,
    wide: bool,
    b: &mut Budget,
) -> Result<Mesh> {
    // The material chunk replaces this seed; avoid allocating a default archiver.
    let mut m = Mesh::with_materials(
        game,
        Materials {
            metadata: Metadata {
                version: 1,
                archiver: Vec::new(),
                format: archive::Format::Binary,
                save_game: false,
                date: None,
                user: None,
            },
            records: Vec::new(),
        },
    );
    m.wide_indices |= wide;
    let mut seen = 0;
    loop {
        let p = r.position();
        let (id, mut c) = next(r, b)?;
        match id {
            0xb000 => {
                once(&mut seen, 1, p)?;
                let v = c.u16(CTX)?;
                check_game(v, game, 9, 265, p + 6)?;
                m.date = Date::read(&mut c, CTX)?;
                m.name = line(&mut c, b)?;
            }
            0xb010 => {
                once(&mut seen, 2, p)?;
                m.bbox = Aabb::read(&mut c, CTX)?;
                m.obb = read_obb(&mut c, b)?;
            }
            0xb020 => {
                once(&mut seen, 4, p)?;
                let (materials, table) = materials(&mut c, game, None, b)?;
                m.materials = materials;
                let at_table = table.is_some_and(|range| range.contains(&c.position()));
                m.alpha_test = if game == GameVersion::Gothic2 && c.remaining() != 0 && !at_table {
                    Some(c.u8(CTX)?)
                } else {
                    None
                };
                let p = c.position();
                m.material_tail = b.copy(c.take_bytes(c.remaining(), CTX)?, Some(p))?;
            }
            0xb025 | 0xb026 => {
                once(&mut seen, 8, p)?;
                m.lightmaps = Some(read_lights(&mut c, id == 0xb026, b)?);
            }
            0xb030 => {
                once(&mut seen, 16, p)?;
                let n = c.u32(CTX)? as usize;
                m.vertices = array(&mut c, n, 12, b, |r| Vec3::read(r, CTX))?;
            }
            0xb040 => {
                once(&mut seen, 32, p)?;
                let n = c.u32(CTX)? as usize;
                m.features = array(&mut c, n, 24, b, |r| {
                    Ok(Feature {
                        texture: Vec2::read(r, CTX)?,
                        light: r.u32(CTX)?,
                        normal: Vec3::read(r, CTX)?,
                    })
                })?;
            }
            0xb050 => {
                once(&mut seen, 64, p)?;
                let n = c.u32(CTX)? as usize;
                if n > c.remaining() / 24 {
                    return Err(at(ErrorKind::UnexpectedEof, c.position()));
                }
                m.polygons = b.array(n, Some(c.position()))?;
                for _ in 0..n {
                    let material = c.u16(CTX)?;
                    let lightmap = c.i16(CTX)?;
                    let plane = Plane {
                        distance: c.f32(CTX)?,
                        normal: Vec3::read(&mut c, CTX)?,
                    };
                    let flags = if game == GameVersion::Gothic1 {
                        PolygonFlags::Gothic1 {
                            raw: [c.u8(CTX)?, c.u8(CTX)?],
                            sector_index: c.i16(CTX)?,
                        }
                    } else {
                        PolygonFlags::Gothic2 {
                            raw: c.u8(CTX)?,
                            sector_index: c.i16(CTX)?,
                        }
                    };
                    let count = c.u8(CTX)? as usize;
                    let corners =
                        array(&mut c, count, if m.wide_indices { 8 } else { 6 }, b, |r| {
                            Ok(Corner {
                                vertex: if m.wide_indices {
                                    r.u32(CTX)?
                                } else {
                                    u32::from(r.u16(CTX)?)
                                },
                                feature: r.u32(CTX)?,
                            })
                        })?;
                    m.polygons.push(Polygon {
                        material,
                        lightmap,
                        plane,
                        flags,
                        corners,
                    });
                }
            }
            0xb060 => {
                end(&c)?;
                if seen & 119 != 119 {
                    return Err(at(ErrorKind::InvalidData, p));
                }
                return Ok(m);
            }
            _ => return Err(at(ErrorKind::Unsupported, p)),
        }
        end(&c)?;
    }
}
pub(crate) fn validate(m: &Mesh, b: &mut Budget) -> Result<()> {
    b.chunks(7 + usize::from(m.lightmaps.is_some()))?;
    if m.game == GameVersion::Gothic2 && !m.wide_indices {
        return Err(bad(ErrorKind::Unsupported));
    }
    if m.game == GameVersion::Gothic1 && m.alpha_test.is_some()
        || m.game == GameVersion::Gothic2
            && m.alpha_test.is_none()
            && !m.material_tail.is_empty()
            && m.materials.metadata.format != archive::Format::BinSafe
    {
        return Err(bad(ErrorKind::InvalidData));
    }
    b.raw(&m.name)?;
    own_obb(&m.obb, b)?;
    validate_materials(&m.materials, m.game, b)?;
    b.raw(&m.material_tail)?;
    b.owned(&m.vertices)?;
    b.owned(&m.features)?;
    b.owned(&m.polygons)?;
    if let Some(l) = &m.lightmaps {
        b.owned(&l.textures)?;
        b.owned(&l.maps)?;
        for t in &l.textures {
            let bytes = t
                .mips()
                .try_fold(if t.palette().is_some() { 1024 } else { 0 }, |n, m| {
                    add(n, m.bytes().len())
                })?;
            b.charge(
                add(bytes, mul(t.mipmap_count(), size_of::<Vec<u8>>())?)?,
                t.mipmap_count(),
                None,
            )?;
        }
        if l.maps
            .iter()
            .any(|m| m.texture as usize >= l.textures.len())
        {
            return Err(bad(ErrorKind::InvalidData));
        }
        if l.encoding == LightmapEncoding::Embedded
            && (l.maps.len() != l.textures.len()
                || l.maps
                    .iter()
                    .enumerate()
                    .any(|(i, m)| m.texture as usize != i))
        {
            return Err(bad(ErrorKind::Unsupported));
        }
    }
    for p in &m.polygons {
        b.owned(&p.corners)?;
        if p.corners.len() > 255 {
            return Err(bad(ErrorKind::LimitExceeded));
        }
        if matches!(p.flags, PolygonFlags::Gothic1 { .. }) != (m.game == GameVersion::Gothic1) {
            return Err(bad(ErrorKind::Unsupported));
        }
        if p.material as usize >= m.materials.records.len()
            || p.lightmap < -1
            || p.lightmap >= 0
                && p.lightmap as usize >= m.lightmaps.as_ref().map_or(0, |l| l.maps.len())
            || p.corners.iter().any(|c| {
                c.vertex as usize >= m.vertices.len() || c.feature as usize >= m.features.len()
            })
        {
            return Err(bad(ErrorKind::InvalidData));
        }
        if !m.wide_indices && p.corners.iter().any(|c| c.vertex > u16::MAX.into()) {
            return Err(bad(ErrorKind::Unsupported));
        }
    }
    Ok(())
}
pub(crate) fn write(m: &Mesh, w: &mut Writer, limits: Limits) -> Result<()> {
    chunk(w, 0xb000, |w| {
        w.u16(
            if m.game == GameVersion::Gothic1 {
                9
            } else {
                265
            },
            CTX,
        )?;
        m.date.write(w, CTX)?;
        write_line(w, &m.name, limits)
    })?;
    chunk(w, 0xb010, |w| {
        m.bbox.write(w, CTX)?;
        write_obb(&m.obb, w, limits)
    })?;
    chunk(w, 0xb020, |w| {
        write_materials(&m.materials, w, m.game, true, limits)?;
        if let Some(alpha) = m.alpha_test {
            w.u8(alpha, CTX)?;
        }
        w.bytes(&m.material_tail, CTX)
    })?;
    if let Some(l) = &m.lightmaps {
        chunk(
            w,
            if l.encoding == LightmapEncoding::Shared {
                0xb026
            } else {
                0xb025
            },
            |w| {
                if l.encoding == LightmapEncoding::Shared {
                    w.u32(u32n(l.textures.len())?, CTX)?;
                    for t in &l.textures {
                        w.bytes(&t.to_bytes(limits.texture)?, CTX)?;
                    }
                    w.u32(u32n(l.maps.len())?, CTX)?;
                } else {
                    w.u32(u32n(l.maps.len())?, CTX)?;
                }
                for map in &l.maps {
                    map.origin.write(w, CTX)?;
                    for n in &map.normals {
                        n.write(w, CTX)?;
                    }
                    if l.encoding == LightmapEncoding::Shared {
                        w.u32(map.texture, CTX)?;
                    } else {
                        w.bytes(
                            &l.textures[map.texture as usize].to_bytes(limits.texture)?,
                            CTX,
                        )?;
                    }
                }
                Ok(())
            },
        )?;
    }
    chunk(w, 0xb030, |w| {
        w.u32(u32n(m.vertices.len())?, CTX)?;
        for v in &m.vertices {
            v.write(w, CTX)?;
        }
        Ok(())
    })?;
    chunk(w, 0xb040, |w| {
        w.u32(u32n(m.features.len())?, CTX)?;
        for v in &m.features {
            v.texture.write(w, CTX)?;
            w.u32(v.light, CTX)?;
            v.normal.write(w, CTX)?;
        }
        Ok(())
    })?;
    chunk(w, 0xb050, |w| {
        w.u32(u32n(m.polygons.len())?, CTX)?;
        for p in &m.polygons {
            w.u16(p.material, CTX)?;
            w.i16(p.lightmap, CTX)?;
            w.f32(p.plane.distance, CTX)?;
            p.plane.normal.write(w, CTX)?;
            match p.flags {
                PolygonFlags::Gothic1 { raw, sector_index } => {
                    w.bytes(&raw, CTX)?;
                    w.i16(sector_index, CTX)?;
                }
                PolygonFlags::Gothic2 { raw, sector_index } => {
                    w.u8(raw, CTX)?;
                    w.i16(sector_index, CTX)?;
                }
            }
            w.u8(p.corners.len() as u8, CTX)?;
            for c in &p.corners {
                if m.wide_indices {
                    w.u32(c.vertex, CTX)?;
                } else {
                    w.u16(c.vertex as u16, CTX)?;
                }
                w.u32(c.feature, CTX)?;
            }
        }
        Ok(())
    })?;
    chunk(w, 0xb060, |_| Ok(()))
}
