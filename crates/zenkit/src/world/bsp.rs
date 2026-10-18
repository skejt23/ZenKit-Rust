//! Bounded BSP storage with an owned flat tree and explicit wire node kinds.
//!
//! Node indices are local array positions. Serialization walks root zero in
//! front-first preorder and remaps sector node indices to that wire order.
//! Light points always follow front-first leaf traversal, independent of the
//! order of `nodes`. Unknown chunks and extra known payloads are rejected.
// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use crate::{
    Error, ErrorKind, GameVersion, Result,
    io::{LineEnding, Reader, Writer},
    mesh::{self, Budget, Plane},
    values::{Aabb, Vec3},
};
use std::{fmt, io::SeekFrom};

const CTX: &str = "BSP";
fn bad(kind: ErrorKind) -> Error {
    Error::operation(kind, CTX)
}
fn at(kind: ErrorKind, p: usize) -> Error {
    Error::new(kind, p, CTX)
}
fn reserve<T>(v: &mut Vec<T>, n: usize) -> Result<()> {
    v.try_reserve(n).map_err(|_| bad(ErrorKind::LimitExceeded))
}
fn u32n(n: usize) -> Result<u32> {
    u32::try_from(n).map_err(|_| bad(ErrorKind::LimitExceeded))
}

/// Shared geometry allocation/work limits plus a separate iterative tree depth.
#[derive(Clone, Copy, Debug)]
pub struct BspLimits {
    pub mesh: mesh::Limits,
    /// Root depth is one. Traversal and drop never recurse on the Rust stack.
    pub depth: usize,
}
impl Default for BspLimits {
    fn default() -> Self {
        Self {
            mesh: Default::default(),
            depth: 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BspChunkKind {
    Header,
    Polygons,
    Tree,
    Light,
    Outdoors,
    End,
}
impl BspChunkKind {
    fn id(self) -> u16 {
        match self {
            Self::Header => 0xc000,
            Self::Polygons => 0xc010,
            Self::Tree => 0xc040,
            Self::Light => 0xc045,
            Self::Outdoors => 0xc050,
            Self::End => 0xc0ff,
        }
    }
    fn from_id(id: u16) -> Option<Self> {
        Some(match id {
            0xc000 => Self::Header,
            0xc010 => Self::Polygons,
            0xc040 => Self::Tree,
            0xc045 => Self::Light,
            0xc050 => Self::Outdoors,
            0xc0ff => Self::End,
            _ => return None,
        })
    }
    fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// Wire classification is explicit: a branch with no children still has a plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BspNodeKind {
    Leaf,
    Branch {
        plane: Plane,
        front: Option<u32>,
        back: Option<u32>,
        /// Present only in Gothic 1 branch records.
        lod: Option<u8>,
        /// High four bits, plus a leaf bit (4/8) only when that child is absent.
        /// Child presence and present-child leaf flags are derived on output.
        extra_flags: u8,
    },
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BspNode {
    pub bbox: Aabb,
    /// Range into `BspTree::polygon_indices`, not into the mesh polygon table.
    pub polygon_index: u32,
    pub polygon_count: u32,
    pub kind: BspNodeKind,
}
impl Default for BspNode {
    fn default() -> Self {
        Self {
            bbox: Aabb::default(),
            polygon_index: 0,
            polygon_count: 0,
            kind: BspNodeKind::Leaf,
        }
    }
}
impl BspNode {
    pub fn is_leaf(&self) -> bool {
        matches!(self.kind, BspNodeKind::Leaf)
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct BspSector {
    pub name: Vec<u8>,
    pub node_indices: Vec<u32>,
    /// Indices into the enclosing mesh polygon table.
    pub portal_polygon_indices: Vec<u32>,
}
impl fmt::Debug for BspSector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BspSector")
            .field("name_bytes", &self.name.len())
            .field("nodes", &self.node_indices.len())
            .field("portals", &self.portal_polygon_indices.len())
            .finish()
    }
}
#[derive(Clone, Default, PartialEq, Eq)]
pub struct BspOutdoors {
    pub sectors: Vec<BspSector>,
    pub portal_polygon_indices: Vec<u32>,
}
impl fmt::Debug for BspOutdoors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BspOutdoors")
            .field("sectors", &self.sectors.len())
            .field("portals", &self.portal_polygon_indices.len())
            .finish()
    }
}
#[derive(Clone, PartialEq)]
pub struct BspTree {
    pub game: GameVersion,
    /// Uninterpreted HEADER word (defaults to 2).
    pub header_word: u16,
    /// Raw mode: 0 is indoor, 1 outdoor; unknown values are retained.
    pub mode: u32,
    pub polygon_indices: Vec<u32>,
    /// Exactly one rooted tree; root is index zero. Node order may be edited.
    pub nodes: Vec<BspNode>,
    /// One value per Leaf in front-first traversal order. None preserves absence.
    pub light_points: Option<Vec<Vec3>>,
    pub outdoors: Option<BspOutdoors>,
    /// Uninterpreted END payload byte (defaults to ASCII B).
    pub end_byte: u8,
    /// Unique recognized chunks in original order. END must be last.
    pub chunk_order: Vec<BspChunkKind>,
}
impl fmt::Debug for BspTree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BspTree")
            .field("game", &self.game)
            .field("mode", &self.mode)
            .field("polygons", &self.polygon_indices.len())
            .field("nodes", &self.nodes.len())
            .field("light_points", &self.light_points.as_ref().map(Vec::len))
            .field("outdoors", &self.outdoors)
            .field("chunks", &self.chunk_order.len())
            .finish()
    }
}

impl BspTree {
    /// Construct a valid one-leaf, empty-geometry tree with all six standard chunks.
    pub fn new(game: GameVersion) -> Result<Self> {
        let mut result = Self::empty(game);
        reserve(&mut result.nodes, 1)?;
        result.nodes.push(BspNode::default());
        let mut lights = Vec::new();
        reserve(&mut lights, 1)?;
        lights.push(Vec3::default());
        result.light_points = Some(lights);
        result.outdoors = Some(BspOutdoors::default());
        reserve(&mut result.chunk_order, 6)?;
        result.chunk_order.extend([
            BspChunkKind::Header,
            BspChunkKind::Polygons,
            BspChunkKind::Tree,
            BspChunkKind::Light,
            BspChunkKind::Outdoors,
            BspChunkKind::End,
        ]);
        Ok(result)
    }
    fn empty(game: GameVersion) -> Self {
        Self {
            game,
            header_word: 2,
            mode: 0,
            polygon_indices: Vec::new(),
            nodes: Vec::new(),
            light_points: None,
            outdoors: None,
            end_byte: b'B',
            chunk_order: Vec::new(),
        }
    }
    pub fn from_bytes(bytes: &[u8], game: GameVersion, limits: BspLimits) -> Result<Self> {
        if bytes.len() > limits.mesh.bytes {
            return Err(at(ErrorKind::LimitExceeded, 0));
        }
        let mut r = Reader::new(bytes);
        let tree = Self::read(&mut r, game, limits)?;
        if r.remaining() != 0 {
            return Err(at(ErrorKind::Unsupported, r.position()));
        }
        Ok(tree)
    }
    /// Transactional prefix read: errors leave the enclosing cursor unchanged.
    pub fn read(input: &mut Reader<'_>, game: GameVersion, limits: BspLimits) -> Result<Self> {
        let mut source = input.clone();
        let start = source.position();
        let mut r = source.substream(source.remaining().min(limits.mesh.bytes), CTX)?;
        let result = read_budget(&mut r, game, limits.depth, &mut Budget::new(limits.mesh))
            .map_err(|mut e| {
                if e.kind == ErrorKind::UnexpectedEof && input.remaining() > limits.mesh.bytes {
                    e.kind = ErrorKind::LimitExceeded;
                }
                e
            })?;
        input.take_bytes(r.position() - start, CTX)?;
        Ok(result)
    }
    pub fn validate(&self, limits: BspLimits) -> Result<()> {
        self.validate_budget(limits.depth, &mut Budget::new(limits.mesh))
    }
    pub fn to_bytes(&self, limits: BspLimits) -> Result<Vec<u8>> {
        self.validate(limits)?;
        let mut w = Writer::new(limits.mesh.bytes);
        self.write(&mut w, limits.depth, limits.mesh)?;
        Ok(w.into_inner())
    }
    /// Derive preorder node indices, rejecting cycles, shared children and orphans.
    pub fn preorder(&self, depth: usize) -> Result<Vec<u32>> {
        Ok(self.order(depth)?.nodes)
    }
    /// Leaf indices in front-first traversal; matches the light-point sequence.
    pub fn leaf_node_indices(&self, depth: usize) -> Result<Vec<u32>> {
        Ok(self.order(depth)?.leaves)
    }
    /// Parent indices for the current node array; the root has no parent.
    pub fn parent_indices(&self, depth: usize) -> Result<Vec<Option<u32>>> {
        Ok(self.order(depth)?.parents)
    }
    /// Sorted leaf polygon occurrences, including repetitions across leaves.
    /// `limit` bounds the number of returned indices before allocation.
    pub fn leaf_polygons(&self, depth: usize, limit: usize) -> Result<Vec<u32>> {
        let order = self.order(depth)?;
        let mut count = 0usize;
        for &index in &order.leaves {
            let n = &self.nodes[index as usize];
            self.polygon_range(n)?;
            count = count
                .checked_add(n.polygon_count as usize)
                .ok_or_else(|| bad(ErrorKind::LimitExceeded))?;
            if count > limit {
                return Err(bad(ErrorKind::LimitExceeded));
            }
        }
        let mut result = Vec::new();
        result
            .try_reserve_exact(count)
            .map_err(|_| bad(ErrorKind::LimitExceeded))?;
        for index in order.leaves {
            result.extend_from_slice(
                &self.polygon_indices[self.polygon_range(&self.nodes[index as usize])?],
            );
        }
        result.sort_unstable();
        Ok(result)
    }
    /// Check mesh-dependent polygon references separately from standalone BSP I/O.
    pub fn validate_mesh_polygons(&self, count: usize) -> Result<()> {
        let valid = |v: &[u32]| v.iter().all(|&i| (i as usize) < count);
        if !valid(&self.polygon_indices) {
            return Err(bad(ErrorKind::InvalidData));
        }
        if let Some(o) = &self.outdoors
            && (!valid(&o.portal_polygon_indices)
                || o.sectors.iter().any(|s| !valid(&s.portal_polygon_indices)))
        {
            return Err(bad(ErrorKind::InvalidData));
        }
        Ok(())
    }
    fn polygon_range(&self, n: &BspNode) -> Result<std::ops::Range<usize>> {
        let start = n.polygon_index as usize;
        let end = start
            .checked_add(n.polygon_count as usize)
            .ok_or_else(|| bad(ErrorKind::InvalidData))?;
        if end > self.polygon_indices.len() {
            return Err(bad(ErrorKind::InvalidData));
        }
        Ok(start..end)
    }
    fn order(&self, depth_limit: usize) -> Result<Order> {
        if self.nodes.is_empty() || (self.nodes.len() == 1 && !self.nodes[0].is_leaf()) {
            return Err(bad(ErrorKind::InvalidData));
        }
        u32n(self.nodes.len())?;
        let mut parents = Vec::new();
        parents
            .try_reserve_exact(self.nodes.len())
            .map_err(|_| bad(ErrorKind::LimitExceeded))?;
        parents.resize(self.nodes.len(), None);
        let mut seen = Vec::new();
        seen.try_reserve_exact(self.nodes.len())
            .map_err(|_| bad(ErrorKind::LimitExceeded))?;
        seen.resize(self.nodes.len(), false);
        let mut nodes = Vec::new();
        nodes
            .try_reserve_exact(self.nodes.len())
            .map_err(|_| bad(ErrorKind::LimitExceeded))?;
        let mut leaves = Vec::new();
        let mut pending = Vec::new();
        reserve(&mut pending, 1)?;
        pending.push((0u32, None, 1usize));
        while let Some((i, parent, depth)) = pending.pop() {
            if depth > depth_limit {
                return Err(bad(ErrorKind::LimitExceeded));
            }
            let n = self
                .nodes
                .get(i as usize)
                .ok_or_else(|| bad(ErrorKind::InvalidData))?;
            if seen[i as usize] {
                return Err(bad(ErrorKind::InvalidData));
            }
            seen[i as usize] = true;
            parents[i as usize] = parent;
            nodes.push(i);
            match n.kind {
                BspNodeKind::Leaf => {
                    reserve(&mut leaves, 1)?;
                    leaves.push(i);
                }
                BspNodeKind::Branch { front, back, .. } => {
                    let next_depth = depth
                        .checked_add(1)
                        .ok_or_else(|| bad(ErrorKind::LimitExceeded))?;
                    reserve(
                        &mut pending,
                        usize::from(front.is_some()) + usize::from(back.is_some()),
                    )?;
                    if let Some(child) = back {
                        pending.push((child, Some(i), next_depth));
                    }
                    if let Some(child) = front {
                        pending.push((child, Some(i), next_depth));
                    }
                }
            }
        }
        if nodes.len() != self.nodes.len() {
            return Err(bad(ErrorKind::InvalidData));
        }
        Ok(Order {
            nodes,
            leaves,
            parents,
        })
    }
    pub(crate) fn validate_budget(&self, depth: usize, b: &mut Budget) -> Result<()> {
        b.owned(&self.chunk_order)?;
        b.owned(&self.nodes)?;
        b.owned(&self.polygon_indices)?;
        let mut seen = 0u8;
        for (i, kind) in self.chunk_order.iter().copied().enumerate() {
            if seen & kind.bit() != 0
                || (kind == BspChunkKind::End && i + 1 != self.chunk_order.len())
            {
                return Err(bad(ErrorKind::InvalidData));
            }
            seen |= kind.bit();
        }
        let required = BspChunkKind::Header.bit()
            | BspChunkKind::Polygons.bit()
            | BspChunkKind::Tree.bit()
            | BspChunkKind::End.bit();
        if seen & required != required
            || (seen & BspChunkKind::Light.bit() != 0) != self.light_points.is_some()
            || (seen & BspChunkKind::Outdoors.bit() != 0) != self.outdoors.is_some()
        {
            return Err(bad(ErrorKind::InvalidData));
        }
        b.chunks = b
            .chunks
            .checked_add(self.chunk_order.len())
            .filter(|&n| n <= b.limits.chunks)
            .ok_or_else(|| bad(ErrorKind::LimitExceeded))?;
        for n in &self.nodes {
            self.polygon_range(n)?;
            if let BspNodeKind::Branch {
                front,
                back,
                lod,
                extra_flags,
                ..
            } = n.kind
                && (lod.is_some() != (self.game == GameVersion::Gothic1)
                    || extra_flags & 3 != 0
                    || (front.is_some() && extra_flags & 4 != 0)
                    || (back.is_some() && extra_flags & 8 != 0))
            {
                return Err(bad(ErrorKind::InvalidData));
            }
        }
        let order = self.order(depth)?;
        if let Some(points) = &self.light_points {
            b.owned(points)?;
            if points.len() != order.leaves.len() {
                return Err(bad(ErrorKind::InvalidData));
            }
        }
        if let Some(o) = &self.outdoors {
            b.owned(&o.sectors)?;
            b.owned(&o.portal_polygon_indices)?;
            u32n(o.sectors.len())?;
            u32n(o.portal_polygon_indices.len())?;
            for s in &o.sectors {
                b.raw(&s.name)?;
                b.owned(&s.node_indices)?;
                b.owned(&s.portal_polygon_indices)?;
                if s.name.len() > b.limits.string_bytes {
                    return Err(bad(ErrorKind::LimitExceeded));
                }
                if s.name.iter().any(|&byte| matches!(byte, 0 | b'\r' | b'\n')) {
                    return Err(bad(ErrorKind::InvalidData));
                }
                if s.node_indices
                    .iter()
                    .any(|&i| i as usize >= self.nodes.len())
                {
                    return Err(bad(ErrorKind::InvalidData));
                }
                u32n(s.node_indices.len())?;
                u32n(s.portal_polygon_indices.len())?;
            }
        }
        u32n(self.polygon_indices.len())?;
        Ok(())
    }
    pub(crate) fn write(&self, w: &mut Writer, depth: usize, limits: mesh::Limits) -> Result<()> {
        let order = self.order(depth)?;
        let mut remap = Vec::new();
        remap
            .try_reserve_exact(self.nodes.len())
            .map_err(|_| bad(ErrorKind::LimitExceeded))?;
        remap.resize(self.nodes.len(), 0u32);
        for (wire, &old) in order.nodes.iter().enumerate() {
            remap[old as usize] = u32n(wire)?;
        }
        for &kind in &self.chunk_order {
            write_chunk(w, kind.id(), |w| match kind {
                BspChunkKind::Header => {
                    w.u16(self.header_word, CTX)?;
                    w.u32(self.mode, CTX)
                }
                BspChunkKind::Polygons => write_indices(w, &self.polygon_indices),
                BspChunkKind::Tree => {
                    w.u32(u32n(self.nodes.len())?, CTX)?;
                    w.u32(u32n(order.leaves.len())?, CTX)?;
                    for &i in &order.nodes {
                        let node = &self.nodes[i as usize];
                        node.bbox.write(w, CTX)?;
                        w.u32(node.polygon_index, CTX)?;
                        w.u32(node.polygon_count, CTX)?;
                        if let BspNodeKind::Branch {
                            front,
                            back,
                            plane,
                            lod,
                            extra_flags,
                        } = node.kind
                        {
                            let mut flags = extra_flags;
                            if let Some(id) = front {
                                flags |= 1;
                                if self.nodes[id as usize].is_leaf() {
                                    flags |= 4;
                                }
                            }
                            if let Some(id) = back {
                                flags |= 2;
                                if self.nodes[id as usize].is_leaf() {
                                    flags |= 8;
                                }
                            }
                            w.u8(flags, CTX)?;
                            w.f32(plane.distance, CTX)?;
                            plane.normal.write(w, CTX)?;
                            if let Some(lod) = lod {
                                w.u8(lod, CTX)?;
                            }
                        }
                    }
                    Ok(())
                }
                BspChunkKind::Light => {
                    for point in self
                        .light_points
                        .as_ref()
                        .ok_or_else(|| bad(ErrorKind::InvalidData))?
                    {
                        point.write(w, CTX)?;
                    }
                    Ok(())
                }
                BspChunkKind::Outdoors => {
                    let o = self
                        .outdoors
                        .as_ref()
                        .ok_or_else(|| bad(ErrorKind::InvalidData))?;
                    w.u32(u32n(o.sectors.len())?, CTX)?;
                    for s in &o.sectors {
                        if s.name.iter().any(|&b| matches!(b, 0 | b'\r' | b'\n')) {
                            return Err(bad(ErrorKind::InvalidData));
                        }
                        w.line(&s.name, LineEnding::Lf, limits.string_bytes, CTX)?;
                        w.u32(u32n(s.node_indices.len())?, CTX)?;
                        w.u32(u32n(s.portal_polygon_indices.len())?, CTX)?;
                        for &i in &s.node_indices {
                            w.u32(
                                *remap
                                    .get(i as usize)
                                    .ok_or_else(|| bad(ErrorKind::InvalidData))?,
                                CTX,
                            )?;
                        }
                        for &i in &s.portal_polygon_indices {
                            w.u32(i, CTX)?;
                        }
                    }
                    write_indices(w, &o.portal_polygon_indices)
                }
                BspChunkKind::End => w.u8(self.end_byte, CTX),
            })?;
        }
        Ok(())
    }
}
struct Order {
    nodes: Vec<u32>,
    leaves: Vec<u32>,
    parents: Vec<Option<u32>>,
}

fn read_indices(r: &mut Reader<'_>, n: usize, b: &mut Budget) -> Result<Vec<u32>> {
    if n > r.remaining() / 4 {
        return Err(at(ErrorKind::UnexpectedEof, r.position()));
    }
    let mut values = b.array(n, Some(r.position()))?;
    for _ in 0..n {
        values.push(r.u32(CTX)?);
    }
    Ok(values)
}
fn read_nodes(
    r: &mut Reader<'_>,
    game: GameVersion,
    depth_limit: usize,
    b: &mut Budget,
) -> Result<Vec<BspNode>> {
    let count_position = r.position();
    let count = r.u32(CTX)? as usize;
    let leaf_count = r.u32(CTX)? as usize;
    if count == 0 || leaf_count > count {
        return Err(at(ErrorKind::InvalidData, count_position));
    }
    if count > r.remaining() / 32 {
        return Err(at(ErrorKind::UnexpectedEof, r.position()));
    }
    // No declared sibling array: reserve/grow only as complete records arrive.
    let required = count
        .checked_mul(size_of::<BspNode>())
        .ok_or_else(|| at(ErrorKind::LimitExceeded, count_position))?;
    if required > b.limits.owned_bytes.saturating_sub(b.owned)
        || count > b.limits.elements.saturating_sub(b.elements)
    {
        return Err(at(ErrorKind::LimitExceeded, count_position));
    }
    let mut nodes: Vec<BspNode> = Vec::new();
    let mut pending = Vec::new();
    reserve(&mut pending, 1)?;
    pending.push((None::<(usize, bool)>, count == 1, 1usize));
    let mut leaves = 0;
    while let Some((parent, is_leaf, depth)) = pending.pop() {
        let p = r.position();
        if depth > depth_limit {
            return Err(at(ErrorKind::LimitExceeded, p));
        }
        if nodes.len() >= count {
            return Err(at(ErrorKind::InvalidData, p));
        }
        let mut node = BspNode {
            bbox: Aabb::read(r, CTX)?,
            polygon_index: r.u32(CTX)?,
            polygon_count: r.u32(CTX)?,
            kind: BspNodeKind::Leaf,
        };
        let index = nodes.len();
        if is_leaf {
            leaves += 1;
        } else {
            let flags = r.u8(CTX)?;
            let plane = Plane {
                distance: r.f32(CTX)?,
                normal: Vec3::read(r, CTX)?,
            };
            let lod = if game == GameVersion::Gothic1 {
                Some(r.u8(CTX)?)
            } else {
                None
            };
            let extra_flags = flags
                & !(3 | if flags & 1 != 0 { 4 } else { 0 } | if flags & 2 != 0 { 8 } else { 0 });
            node.kind = BspNodeKind::Branch {
                plane,
                front: None,
                back: None,
                lod,
                extra_flags,
            };
            let next = depth
                .checked_add(1)
                .ok_or_else(|| at(ErrorKind::LimitExceeded, p))?;
            reserve(
                &mut pending,
                usize::from(flags & 1 != 0) + usize::from(flags & 2 != 0),
            )?;
            if flags & 2 != 0 {
                pending.push((Some((index, false)), flags & 8 != 0, next));
            }
            if flags & 1 != 0 {
                pending.push((Some((index, true)), flags & 4 != 0, next));
            }
        }
        b.charge(size_of::<BspNode>(), 1, Some(p))?;
        reserve(&mut nodes, 1)?;
        nodes.push(node);
        if let Some((parent, front)) = parent {
            let BspNodeKind::Branch { front: f, back, .. } = &mut nodes[parent].kind else {
                unreachable!()
            };
            *if front { f } else { back } = Some(u32n(index)?);
        }
    }
    if nodes.len() != count || leaves != leaf_count {
        return Err(at(ErrorKind::InvalidData, count_position));
    }
    Ok(nodes)
}
pub(crate) fn read_budget(
    r: &mut Reader<'_>,
    game: GameVersion,
    depth: usize,
    b: &mut Budget,
) -> Result<BspTree> {
    let mut tree = BspTree::empty(game);
    let mut seen = 0;
    loop {
        let p = r.position();
        if b.chunks >= b.limits.chunks {
            return Err(at(ErrorKind::LimitExceeded, p));
        }
        let id = r.u16(CTX)?;
        let n = r.u32(CTX)? as usize;
        let mut c = r.substream(n, CTX)?;
        b.chunks += 1;
        let kind = BspChunkKind::from_id(id).ok_or_else(|| at(ErrorKind::Unsupported, p))?;
        if seen & kind.bit() != 0 {
            return Err(at(ErrorKind::InvalidData, p));
        }
        seen |= kind.bit();
        b.charge(size_of::<BspChunkKind>(), 1, Some(p))?;
        reserve(&mut tree.chunk_order, 1)?;
        tree.chunk_order.push(kind);
        match kind {
            BspChunkKind::Header => {
                tree.header_word = c.u16(CTX)?;
                tree.mode = c.u32(CTX)?;
            }
            BspChunkKind::Polygons => {
                let n = c.u32(CTX)? as usize;
                tree.polygon_indices = read_indices(&mut c, n, b)?;
            }
            BspChunkKind::Tree => {
                tree.nodes = read_nodes(&mut c, game, depth, b)?;
            }
            BspChunkKind::Light => {
                if c.remaining() % 12 != 0 {
                    return Err(at(ErrorKind::InvalidData, c.position()));
                }
                let n = c.remaining() / 12;
                let mut points = b.array(n, Some(c.position()))?;
                for _ in 0..n {
                    points.push(Vec3::read(&mut c, CTX)?);
                }
                tree.light_points = Some(points);
            }
            BspChunkKind::Outdoors => {
                let n = c.u32(CTX)? as usize;
                if n > c.remaining() / 9 {
                    return Err(at(ErrorKind::UnexpectedEof, c.position()));
                }
                let mut sectors = b.array(n, Some(c.position()))?;
                for _ in 0..n {
                    let p = c.position();
                    let name =
                        c.line(LineEnding::Lf, b.limits.string_bytes.saturating_add(1), CTX)?;
                    let name = name.strip_suffix(b"\r").unwrap_or(name);
                    if name.len() > b.limits.string_bytes {
                        return Err(at(ErrorKind::LimitExceeded, p));
                    }
                    if name.iter().any(|&v| matches!(v, 0 | b'\r')) {
                        return Err(at(ErrorKind::Unsupported, p));
                    }
                    let name = b.copy(name, Some(p))?;
                    let nodes = c.u32(CTX)? as usize;
                    let portals = c.u32(CTX)? as usize;
                    let node_indices = read_indices(&mut c, nodes, b)?;
                    let portal_polygon_indices = read_indices(&mut c, portals, b)?;
                    sectors.push(BspSector {
                        name,
                        node_indices,
                        portal_polygon_indices,
                    });
                }
                let n = c.u32(CTX)? as usize;
                let portal_polygon_indices = read_indices(&mut c, n, b)?;
                tree.outdoors = Some(BspOutdoors {
                    sectors,
                    portal_polygon_indices,
                });
            }
            BspChunkKind::End => {
                tree.end_byte = c.u8(CTX)?;
            }
        }
        if c.remaining() != 0 {
            return Err(at(ErrorKind::Unsupported, c.position()));
        }
        if kind == BspChunkKind::End {
            break;
        }
    }
    // Validate without charging retained data twice to the enclosing graph budget.
    // Cross-chunk invariants do not identify one offending source position.
    tree.validate_budget(depth, &mut Budget::new(b.limits))?;
    Ok(tree)
}
fn write_indices(w: &mut Writer, values: &[u32]) -> Result<()> {
    w.u32(u32n(values.len())?, CTX)?;
    for &v in values {
        w.u32(v, CTX)?;
    }
    Ok(())
}
fn write_chunk(
    w: &mut Writer,
    id: u16,
    body: impl FnOnce(&mut Writer) -> Result<()>,
) -> Result<()> {
    w.u16(id, CTX)?;
    let offset = w.position();
    w.u32(0, CTX)?;
    body(w)?;
    let end = w.position();
    w.seek_to(SeekFrom::Start(offset as u64), CTX)?;
    w.u32(u32n(end - offset - 4)?, CTX)?;
    w.seek_to(SeekFrom::Start(end as u64), CTX)?;
    Ok(())
}
