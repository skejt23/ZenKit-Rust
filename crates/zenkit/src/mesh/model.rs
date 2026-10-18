// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;
#[derive(Clone, Default, PartialEq)]
pub struct Source {
    pub date: Date,
    pub path: Vec<u8>,
}
impl fmt::Debug for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Source")
            .field("date", &self.date)
            .field("path_bytes", &self.path.len())
            .finish()
    }
}
#[derive(Clone, PartialEq)]
pub struct HierarchyNode {
    pub name: Vec<u8>,
    pub parent: i16,
    pub transform: Mat4,
}
impl fmt::Debug for HierarchyNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HierarchyNode")
            .field("name_bytes", &self.name.len())
            .field("parent", &self.parent)
            .field("transform", &self.transform)
            .finish()
    }
}
#[derive(Clone, Default, PartialEq)]
pub struct ModelHierarchy {
    pub nodes: Vec<HierarchyNode>,
    pub bbox: Aabb,
    pub collision_bbox: Aabb,
    pub root_translation: Vec3,
    pub checksum: u32,
    pub source: Option<Source>,
}
impl fmt::Debug for ModelHierarchy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelHierarchy")
            .field("nodes", &self.nodes.len())
            .field("checksum", &self.checksum)
            .field("source", &self.source)
            .finish()
    }
}
impl ModelHierarchy {
    pub fn from_bytes(bytes: &[u8], limits: Limits) -> Result<Self> {
        if bytes.len() > limits.bytes {
            return Err(at(ErrorKind::LimitExceeded, 0));
        }
        let mut r = Reader::new(bytes);
        let h = Self::read(&mut r, limits)?;
        end(&r)?;
        Ok(h)
    }
    pub fn read(input: &mut Reader<'_>, limits: Limits) -> Result<Self> {
        let mut source = input.clone();
        let start = source.position();
        let mut r = source.substream(source.remaining().min(limits.bytes), CTX)?;
        let h = read_hierarchy(&mut r, &mut Budget::new(limits)).map_err(|mut e| {
            if e.kind == ErrorKind::UnexpectedEof && input.remaining() > limits.bytes {
                e.kind = ErrorKind::LimitExceeded;
            }
            e
        })?;
        h.validate(limits)?;
        input.take_bytes(r.position() - start, CTX)?;
        Ok(h)
    }
    /// Forward parent references and multiple roots are valid; cycles are not.
    pub fn validate(&self, limits: Limits) -> Result<()> {
        validate_hierarchy(self, &mut Budget::new(limits))
    }
    pub fn to_bytes(&self, limits: Limits) -> Result<Vec<u8>> {
        self.validate(limits)?;
        let mut w = Writer::new(limits.bytes);
        write_hierarchy(self, &mut w, limits)?;
        Ok(w.into_inner())
    }
}
fn source(r: &mut Reader<'_>, b: &mut Budget) -> Result<Source> {
    Ok(Source {
        date: Date::read(r, CTX)?,
        path: line(r, b)?,
    })
}
fn source_write(s: &Source, w: &mut Writer, limits: Limits) -> Result<()> {
    s.date.write(w, CTX)?;
    write_line(w, &s.path, limits)
}
fn read_hierarchy(r: &mut Reader<'_>, b: &mut Budget) -> Result<ModelHierarchy> {
    let mut h = ModelHierarchy::default();
    let mut seen = 0;
    loop {
        let p = r.position();
        let (id, mut c) = next(r, b)?;
        match id {
            0xd100 => {
                once(&mut seen, 1, p)?;
                if c.u32(CTX)? != 3 {
                    return Err(at(ErrorKind::Unsupported, p + 6));
                }
                let n = c.u16(CTX)? as usize;
                if n > c.remaining() / 67 {
                    return Err(at(ErrorKind::UnexpectedEof, c.position()));
                }
                h.nodes = b.array(n, Some(c.position()))?;
                for _ in 0..n {
                    h.nodes.push(HierarchyNode {
                        name: line(&mut c, b)?,
                        parent: c.i16(CTX)?,
                        transform: Mat4::read(&mut c, CTX)?,
                    });
                }
                h.bbox = Aabb::read(&mut c, CTX)?;
                h.collision_bbox = Aabb::read(&mut c, CTX)?;
                h.root_translation = Vec3::read(&mut c, CTX)?;
                h.checksum = c.u32(CTX)?;
            }
            0xd110 => {
                once(&mut seen, 2, p)?;
                h.source = Some(source(&mut c, b)?);
            }
            0xd120 => {
                end(&c)?;
                if seen & 1 == 0 {
                    return Err(at(ErrorKind::InvalidData, p));
                }
                return Ok(h);
            }
            _ => return Err(at(ErrorKind::Unsupported, p)),
        }
        end(&c)?;
    }
}
fn validate_hierarchy(h: &ModelHierarchy, b: &mut Budget) -> Result<()> {
    b.chunks(2 + usize::from(h.source.is_some()))?;
    u16n(h.nodes.len())?;
    b.owned(&h.nodes)?;
    for n in &h.nodes {
        b.raw(&n.name)?;
        if n.parent < -1 || n.parent >= 0 && n.parent as usize >= h.nodes.len() {
            return Err(bad(ErrorKind::InvalidData));
        }
    }
    if let Some(s) = &h.source {
        b.raw(&s.path)?;
    }
    let mut state = Vec::new();
    state
        .try_reserve_exact(h.nodes.len())
        .map_err(|_| bad(ErrorKind::LimitExceeded))?;
    state.resize(h.nodes.len(), 0u8);
    for start in 0..h.nodes.len() {
        if state[start] != 0 {
            continue;
        }
        let mut current = Some(start);
        while let Some(i) = current {
            if state[i] == 1 {
                return Err(bad(ErrorKind::InvalidData));
            }
            if state[i] == 2 {
                break;
            }
            state[i] = 1;
            current = usize::try_from(h.nodes[i].parent).ok();
        }
        let mut current = Some(start);
        while let Some(i) = current {
            if state[i] != 1 {
                break;
            }
            state[i] = 2;
            current = usize::try_from(h.nodes[i].parent).ok();
        }
    }
    Ok(())
}
fn write_hierarchy(h: &ModelHierarchy, w: &mut Writer, limits: Limits) -> Result<()> {
    chunk(w, 0xd100, |w| {
        w.u32(3, CTX)?;
        w.u16(u16n(h.nodes.len())?, CTX)?;
        for n in &h.nodes {
            write_line(w, &n.name, limits)?;
            w.i16(n.parent, CTX)?;
            n.transform.write(w, CTX)?;
        }
        h.bbox.write(w, CTX)?;
        h.collision_bbox.write(w, CTX)?;
        h.root_translation.write(w, CTX)?;
        w.u32(h.checksum, CTX)
    })?;
    if let Some(s) = &h.source {
        chunk(w, 0xd110, |w| source_write(s, w, limits))?;
    }
    chunk(w, 0xd120, |_| Ok(()))
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Weight {
    pub weight: f32,
    pub position: Vec3,
    /// Skeleton node ID, not an index into `SoftSkinMesh::nodes`.
    /// Bounds are checked when a complete `Model` supplies the hierarchy.
    pub node_index: u8,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WedgeNormal {
    pub normal: Vec3,
    /// Raw index with an uninterpreted referent.
    pub index: u32,
}
#[derive(Clone, PartialEq)]
pub struct SoftSkinMesh {
    pub mesh: MultiResolutionMesh,
    pub bboxes: Vec<Obb>,
    pub wedge_normals: Vec<WedgeNormal>,
    pub weights: Vec<Vec<Weight>>,
    /// Skeleton node IDs paired with `bboxes`, not a local weight-index table.
    pub nodes: Vec<i32>,
}
impl SoftSkinMesh {
    pub fn new(game: GameVersion) -> Self {
        Self {
            mesh: MultiResolutionMesh::new(game),
            bboxes: Vec::new(),
            wedge_normals: Vec::new(),
            weights: Vec::new(),
            nodes: Vec::new(),
        }
    }
}
impl fmt::Debug for SoftSkinMesh {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SoftSkinMesh")
            .field("mesh", &self.mesh)
            .field("nodes", &self.nodes.len())
            .field("weight_rows", &self.weights.len())
            .field("wedge_normals", &self.wedge_normals.len())
            .finish()
    }
}
codec!(SoftSkinMesh, read_skin, write_skin, validate_skin);
fn read_skin(r: &mut Reader<'_>, game: GameVersion, b: &mut Budget) -> Result<SoftSkinMesh> {
    let mut s = SoftSkinMesh::new(game);
    let mut seen = 0;
    loop {
        let p = r.position();
        let (id, mut c) = next(r, b)?;
        match id {
            0xe100 => {
                once(&mut seen, 1, p)?;
                if c.u32(CTX)? != 4 {
                    return Err(at(ErrorKind::Unsupported, p + 6));
                }
            }
            0xb100 => {
                once(&mut seen, 2, p)?;
                s.mesh = mrm::read_section(&mut c, game, b)?;
            }
            0xb1ff => {
                once(&mut seen, 4, p)?;
                if seen & 2 == 0 {
                    return Err(at(ErrorKind::InvalidData, p));
                }
                let size = c.u32(CTX)? as usize;
                let mut weights = c.substream(size, CTX)?;
                let count = s.mesh.positions.len();
                if count > weights.remaining() / 4 {
                    return Err(at(ErrorKind::UnexpectedEof, weights.position()));
                }
                s.weights = b.array(count, Some(weights.position()))?;
                for _ in 0..count {
                    let n = weights.u32(CTX)? as usize;
                    s.weights.push(array(&mut weights, n, 17, b, |r| {
                        Ok(Weight {
                            weight: r.f32(CTX)?,
                            position: Vec3::read(r, CTX)?,
                            node_index: r.u8(CTX)?,
                        })
                    })?);
                }
                end(&weights)?;
                let n = c.u32(CTX)? as usize;
                s.wedge_normals = array(&mut c, n, 16, b, |r| {
                    Ok(WedgeNormal {
                        normal: Vec3::read(r, CTX)?,
                        index: r.u32(CTX)?,
                    })
                })?;
                let n = c.u16(CTX)? as usize;
                s.nodes = array(&mut c, n, 4, b, |r| r.i32(CTX))?;
                if n > c.remaining() / 62 {
                    return Err(at(ErrorKind::UnexpectedEof, c.position()));
                }
                s.bboxes = b.array(n, Some(c.position()))?;
                for _ in 0..n {
                    s.bboxes.push(read_obb(&mut c, b)?);
                }
            }
            0xe110 => {
                end(&c)?;
                if seen != 7 {
                    return Err(at(ErrorKind::InvalidData, p));
                }
                return Ok(s);
            }
            _ => return Err(at(ErrorKind::Unsupported, p)),
        }
        end(&c)?;
    }
}
fn validate_skin(s: &SoftSkinMesh, b: &mut Budget) -> Result<()> {
    b.chunks(4)?;
    mrm::validate_section(&s.mesh, b)?;
    u16n(s.nodes.len())?;
    b.owned(&s.nodes)?;
    b.owned(&s.wedge_normals)?;
    b.owned(&s.weights)?;
    b.owned(&s.bboxes)?;
    for row in &s.weights {
        b.owned(row)?;
    }
    if s.weights.len() != s.mesh.positions.len() || s.bboxes.len() != s.nodes.len() {
        return Err(bad(ErrorKind::InvalidData));
    }
    for bbox in &s.bboxes {
        own_obb(bbox, b)?;
    }
    Ok(())
}
fn write_skin(s: &SoftSkinMesh, w: &mut Writer, limits: Limits) -> Result<()> {
    chunk(w, 0xe100, |w| w.u32(4, CTX))?;
    chunk(w, 0xb100, |w| mrm::write_section(&s.mesh, w, limits))?;
    chunk(w, 0xb1ff, |w| {
        let offset = w.position();
        w.u32(0, CTX)?;
        for row in &s.weights {
            w.u32(u32n(row.len())?, CTX)?;
            for v in row {
                w.f32(v.weight, CTX)?;
                v.position.write(w, CTX)?;
                w.u8(v.node_index, CTX)?;
            }
        }
        let end = w.position();
        w.seek_to(SeekFrom::Start(offset as u64), CTX)?;
        w.u32(u32n(end - offset - 4)?, CTX)?;
        w.seek_to(SeekFrom::Start(end as u64), CTX)?;
        w.u32(u32n(s.wedge_normals.len())?, CTX)?;
        for n in &s.wedge_normals {
            n.normal.write(w, CTX)?;
            w.u32(n.index, CTX)?;
        }
        w.u16(u16n(s.nodes.len())?, CTX)?;
        for n in &s.nodes {
            w.i32(*n, CTX)?;
        }
        for bbox in &s.bboxes {
            write_obb(bbox, w, limits)?;
        }
        Ok(())
    })?;
    chunk(w, 0xe110, |_| Ok(()))
}
#[derive(Clone, PartialEq)]
pub struct Attachment {
    pub name: Vec<u8>,
    pub mesh: MultiResolutionMesh,
    /// Preserve an optional empty B1FF terminator following this MRM section.
    pub end_marker: bool,
}
impl fmt::Debug for Attachment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Attachment")
            .field("name_bytes", &self.name.len())
            .field("mesh", &self.mesh)
            .field("end_marker", &self.end_marker)
            .finish()
    }
}
/// Recognized model-mesh header words. The legacy word supports both games;
/// the other two are Gothic II variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ModelMeshVersion {
    Legacy = 0x04030506,
    Gothic2Legacy = 0x04070506,
    Gothic2Early = 0x04080506,
    Gothic2 = 0x04090506,
}
impl ModelMeshVersion {
    pub const fn from_raw(value: u32) -> Option<Self> {
        match value {
            0x04030506 => Some(Self::Legacy),
            0x04070506 => Some(Self::Gothic2Legacy),
            0x04080506 => Some(Self::Gothic2Early),
            0x04090506 => Some(Self::Gothic2),
            _ => None,
        }
    }
    pub const fn raw(self) -> u32 {
        self as u32
    }
    const fn supports(self, game: GameVersion) -> bool {
        matches!(self, Self::Legacy) || matches!(game, GameVersion::Gothic2)
    }
}
#[derive(Clone, PartialEq)]
pub struct ModelMesh {
    pub game: GameVersion,
    /// Loaded header word, retained by the writer. New meshes use `Legacy`.
    pub version: ModelMeshVersion,
    pub attachments: Vec<Attachment>,
    /// Retain the optional D020 name section even when it is empty.
    pub attachment_names_present: bool,
    pub meshes: Vec<SoftSkinMesh>,
    /// D030 presence and its checksum. Absence requires an empty skin collection.
    pub checksum: Option<u32>,
    pub source: Option<Source>,
}
impl ModelMesh {
    pub fn new(game: GameVersion) -> Self {
        Self {
            game,
            version: ModelMeshVersion::Legacy,
            attachments: Vec::new(),
            attachment_names_present: true,
            meshes: Vec::new(),
            checksum: Some(0),
            source: None,
        }
    }
}
impl fmt::Debug for ModelMesh {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelMesh")
            .field("game", &self.game)
            .field("version", &self.version)
            .field("attachments", &self.attachments.len())
            .field("skins", &self.meshes.len())
            .field("checksum", &self.checksum)
            .finish()
    }
}
codec!(
    ModelMesh,
    read_model_mesh,
    write_model_mesh,
    validate_model_mesh
);
fn read_model_mesh(r: &mut Reader<'_>, game: GameVersion, b: &mut Budget) -> Result<ModelMesh> {
    let mut m = ModelMesh::new(game);
    m.attachment_names_present = false;
    m.checksum = None;
    let mut seen = 0;
    let mut names = Vec::new();
    let mut names_index = 0;
    let mut follows_attachment = false;
    loop {
        let p = r.position();
        let (id, mut c) = next(r, b)?;
        match id {
            0xd000 => {
                once(&mut seen, 1, p)?;
                m.version = ModelMeshVersion::from_raw(c.u32(CTX)?)
                    .filter(|v| v.supports(game))
                    .ok_or_else(|| at(ErrorKind::Unsupported, p + 6))?;
            }
            0xd010 => {
                once(&mut seen, 2, p)?;
                m.source = Some(source(&mut c, b)?);
            }
            0xd020 => {
                once(&mut seen, 4, p)?;
                m.attachment_names_present = true;
                let n = c.u16(CTX)? as usize;
                if n > c.remaining() {
                    return Err(at(ErrorKind::UnexpectedEof, c.position()));
                }
                names
                    .try_reserve_exact(n)
                    .map_err(|_| at(ErrorKind::LimitExceeded, p))?;
                m.attachments = b.array(n, Some(c.position()))?;
                for _ in 0..n {
                    names.push(line(&mut c, b)?);
                }
            }
            0xb100 => {
                let name = names
                    .get_mut(names_index)
                    .ok_or_else(|| at(ErrorKind::InvalidData, p))?;
                let mesh = mrm::read_section(&mut c, game, b)?;
                m.attachments.push(Attachment {
                    name: std::mem::take(name),
                    mesh,
                    end_marker: false,
                });
                names_index += 1;
            }
            0xb1ff => {
                if !follows_attachment {
                    return Err(at(ErrorKind::InvalidData, p));
                }
                end(&c)?;
                m.attachments
                    .last_mut()
                    .expect("preceding attachment")
                    .end_marker = true;
            }
            0xd030 => {
                once(&mut seen, 8, p)?;
                m.checksum = Some(c.u32(CTX)?);
                let n = c.u16(CTX)? as usize;
                end(&c)?;
                // The declared chunk payload is only checksum/count. Following skin
                // streams are siblings on the enclosing stream, not inside this chunk.
                if n > r.remaining() / 24 {
                    return Err(at(ErrorKind::UnexpectedEof, r.position()));
                }
                m.meshes = b.array(n, Some(r.position()))?;
                for _ in 0..n {
                    m.meshes.push(read_skin(r, game, b)?);
                }
            }
            0xd040 => {
                end(&c)?;
                if seen & 1 == 0 || names_index != names.len() {
                    return Err(at(ErrorKind::InvalidData, p));
                }
                return Ok(m);
            }
            _ => return Err(at(ErrorKind::Unsupported, p)),
        }
        end(&c)?;
        follows_attachment = id == 0xb100;
    }
}
fn validate_model_mesh(m: &ModelMesh, b: &mut Budget) -> Result<()> {
    if !m.version.supports(m.game) {
        return Err(bad(ErrorKind::Unsupported));
    }
    let sections = 2
        + usize::from(m.source.is_some())
        + usize::from(m.attachment_names_present)
        + usize::from(m.checksum.is_some());
    let attachment_chunks = m
        .attachments
        .iter()
        .try_fold(0, |n, a| add(n, 1 + usize::from(a.end_marker)))?;
    b.chunks(add(sections, attachment_chunks)?)?;
    if (!m.attachment_names_present && !m.attachments.is_empty())
        || (m.checksum.is_none() && !m.meshes.is_empty())
    {
        return Err(bad(ErrorKind::InvalidData));
    }
    u16n(m.attachments.len())?;
    u16n(m.meshes.len())?;
    b.owned(&m.attachments)?;
    b.owned(&m.meshes)?;
    if let Some(s) = &m.source {
        b.raw(&s.path)?;
    }
    let mut names = Vec::new();
    names
        .try_reserve_exact(m.attachments.len())
        .map_err(|_| bad(ErrorKind::LimitExceeded))?;
    names.extend(m.attachments.iter().map(|a| a.name.as_slice()));
    names.sort_unstable();
    if names.windows(2).any(|w| w[0] == w[1]) {
        return Err(bad(ErrorKind::InvalidData));
    }
    for a in &m.attachments {
        b.raw(&a.name)?;
        if a.mesh.game != m.game {
            return Err(bad(ErrorKind::Unsupported));
        }
        mrm::validate_section(&a.mesh, b)?;
    }
    for s in &m.meshes {
        if s.mesh.game != m.game {
            return Err(bad(ErrorKind::Unsupported));
        }
        validate_skin(s, b)?;
    }
    Ok(())
}
fn write_model_mesh(m: &ModelMesh, w: &mut Writer, limits: Limits) -> Result<()> {
    chunk(w, 0xd000, |w| w.u32(m.version.raw(), CTX))?;
    if let Some(s) = &m.source {
        chunk(w, 0xd010, |w| source_write(s, w, limits))?;
    }
    if m.attachment_names_present {
        chunk(w, 0xd020, |w| {
            w.u16(u16n(m.attachments.len())?, CTX)?;
            for a in &m.attachments {
                write_line(w, &a.name, limits)?;
            }
            Ok(())
        })?;
    }
    for a in &m.attachments {
        chunk(w, 0xb100, |w| mrm::write_section(&a.mesh, w, limits))?;
        if a.end_marker {
            chunk(w, 0xb1ff, |_| Ok(()))?;
        }
    }
    if let Some(checksum) = m.checksum {
        chunk(w, 0xd030, |w| {
            w.u32(checksum, CTX)?;
            w.u16(u16n(m.meshes.len())?, CTX)
        })?;
    }
    for s in &m.meshes {
        write_skin(s, w, limits)?;
    }
    chunk(w, 0xd040, |_| Ok(()))
}
#[derive(Clone, PartialEq)]
pub struct Model {
    pub hierarchy: ModelHierarchy,
    pub mesh: ModelMesh,
}
impl fmt::Debug for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Model")
            .field("hierarchy", &self.hierarchy)
            .field("mesh", &self.mesh)
            .finish()
    }
}
codec!(Model, read_model, write_model, validate_model);
fn read_model(r: &mut Reader<'_>, game: GameVersion, b: &mut Budget) -> Result<Model> {
    Ok(Model {
        hierarchy: read_hierarchy(r, b)?,
        mesh: read_model_mesh(r, game, b)?,
    })
}
fn validate_model(m: &Model, b: &mut Budget) -> Result<()> {
    validate_hierarchy(&m.hierarchy, b)?;
    validate_model_mesh(&m.mesh, b)?;
    if m.mesh
        .checksum
        .is_some_and(|checksum| m.hierarchy.checksum != checksum)
    {
        return Err(bad(ErrorKind::InvalidData));
    }
    let mut names = Vec::new();
    names
        .try_reserve_exact(m.hierarchy.nodes.len())
        .map_err(|_| bad(ErrorKind::LimitExceeded))?;
    names.extend(m.hierarchy.nodes.iter().map(|n| n.name.as_slice()));
    names.sort_unstable();
    for a in &m.mesh.attachments {
        if names.binary_search(&a.name.as_slice()).is_err() {
            return Err(bad(ErrorKind::InvalidData));
        }
    }
    for s in &m.mesh.meshes {
        if s.nodes
            .iter()
            .any(|&i| i < 0 || i as usize >= m.hierarchy.nodes.len())
            || s.weights
                .iter()
                .flatten()
                .any(|w| w.node_index as usize >= m.hierarchy.nodes.len())
        {
            return Err(bad(ErrorKind::InvalidData));
        }
    }
    Ok(())
}
fn write_model(m: &Model, w: &mut Writer, limits: Limits) -> Result<()> {
    write_hierarchy(&m.hierarchy, w, limits)?;
    write_model_mesh(&m.mesh, w, limits)
}
