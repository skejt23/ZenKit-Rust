//! Complete world sections, occurrence hierarchies and navigation in the shared
//! object arena. Geometry stays authoritative; triangulation is a derived view.
// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
pub mod bsp;
mod detect;
pub use bsp::{BspChunkKind, BspLimits, BspNode, BspNodeKind, BspOutdoors, BspSector, BspTree};
pub use detect::{detect_game, from_bytes};
pub(crate) mod codec;
use crate::{GameVersion, mesh, objects::Link, values::Vec3};
use std::fmt;

/// Whole-graph world limits, including nested and orphan Worlds in validation.
/// Object nodes/occurrences/raw strings use the surrounding objects::Limits.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Cumulative geometry ownership/elements/chunks across every world section.
    pub geometry: mesh::Limits,
    /// Cumulative world grammar section wrappers, including empty ones.
    pub sections: usize,
    /// Maximum occurrence-hierarchy depth, independent of object-reference depth.
    pub hierarchy_depth: usize,
    /// Maximum BSP tree depth; traversal is iterative.
    pub bsp_depth: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            geometry: Default::default(),
            sections: 1024,
            hierarchy_depth: 1024,
            bsp_depth: 1024,
        }
    }
}
/// Anonymous section framing is independent of object identity. Empty or `%`
/// class labels, version and index are preserved exactly on serialization.
/// Empty class labels require BINARY output; explicitly replace them with `%`
/// before conversion to a tagged encoding.
#[derive(Clone, PartialEq, Eq)]
pub struct SectionFrame {
    pub class_name: Vec<u8>,
    pub version: u16,
    pub index: u32,
}
impl Default for SectionFrame {
    fn default() -> Self {
        Self {
            class_name: b"%".to_vec(),
            version: 0,
            index: 0,
        }
    }
}
impl fmt::Debug for SectionFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SectionFrame")
            .field("class_bytes", &self.class_name.len())
            .field("version", &self.version)
            .field("index", &self.index)
            .finish()
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub frame: SectionFrame,
    pub data: SectionData,
}
#[derive(Clone, Debug, PartialEq)]
// Limits::sections bounds inline section slots separately from geometry buffers.
#[allow(clippy::large_enum_variant)]
pub enum SectionData {
    MeshAndBsp(MeshAndBsp),
    VobTree(Hierarchy),
    WayNet(Link),
    CutscenePlayer(Link),
    SkyController(Link),
    End,
}
impl SectionData {
    pub fn name(&self) -> &'static [u8] {
        match self {
            Self::MeshAndBsp(_) => b"MeshAndBsp",
            Self::VobTree(_) => b"VobTree",
            Self::WayNet(_) => b"WayNet",
            Self::CutscenePlayer(_) => b"CutscenePlayer",
            Self::SkyController(_) => b"SkyCtrl",
            Self::End => b"EndMarker",
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct MeshAndBsp {
    pub mesh: mesh::Mesh,
    pub bsp: BspTree,
}
impl MeshAndBsp {
    /// Derive mesh-order triangle fans from BSP leaf membership. Duplicate leaf
    /// polygon occurrences select a polygon once; the raw mesh/BSP stay unchanged.
    /// Bounds both temporary leaf occurrences and the resulting triangle list.
    pub fn leaf_triangles(
        &self,
        depth: usize,
        max_leaf_occurrences: usize,
        max_triangles: usize,
    ) -> crate::Result<Vec<mesh::Triangle>> {
        self.bsp.validate_mesh_polygons(self.mesh.polygons.len())?;
        let mut polygons = self.bsp.leaf_polygons(depth, max_leaf_occurrences)?;
        polygons.dedup();
        self.mesh.triangulate_leaves(&polygons, max_triangles)
    }
}
/// Flat preorder occurrence hierarchy. Repeated VOB identities can have different
/// child occurrences. Null occurrences and their subtrees remain explicit.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Hierarchy {
    pub root_count_name: Option<Vec<u8>>,
    pub entries: Vec<HierarchyEntry>,
}
impl fmt::Debug for Hierarchy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Hierarchy")
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}
#[derive(Clone, PartialEq, Eq)]
pub struct HierarchyEntry {
    pub object: Link,
    /// Index of an earlier ancestor entry; None begins a new root occurrence.
    pub parent: Option<usize>,
    /// Original tagged `childsN` spelling. None selects canonical output spelling.
    pub children_name: Option<Vec<u8>>,
}
impl fmt::Debug for HierarchyEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HierarchyEntry")
            .field("object", &self.object)
            .field("parent", &self.parent)
            .field(
                "children_name_bytes",
                &self.children_name.as_ref().map(Vec::len),
            )
            .finish()
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Spawn {
    pub npc: Link,
    pub position: Vec3,
    pub timer: f32,
}
#[derive(Clone, Default, PartialEq)]
pub struct WorldState {
    pub npcs: Vec<Link>,
    pub spawns: Vec<Spawn>,
    pub spawning_enabled: bool,
    /// Present only in Gothic2 saves; full signed value retained.
    pub spawn_flags: Option<i32>,
}
impl fmt::Debug for WorldState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorldState")
            .field("npcs", &self.npcs.len())
            .field("spawns", &self.spawns.len())
            .field("spawning_enabled", &self.spawning_enabled)
            .field("spawn_flags", &self.spawn_flags)
            .finish()
    }
}
/// Ordered complete section records plus the save-only tail. Sections may be
/// absent/repeated; End, when present, must be last. Saves require End before tail.
#[derive(Clone, Default, PartialEq)]
pub struct World {
    pub sections: Vec<Section>,
    pub state: Option<WorldState>,
}
impl fmt::Debug for World {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("World")
            .field("sections", &self.sections.len())
            .field("state", &self.state)
            .finish()
    }
}
impl World {
    /// Create an empty editable draft with the requested save-tail presence.
    /// A save draft needs a final [`SectionData::End`] section before validation
    /// or serialization; the constructor does not invent that section.
    pub fn new(game: GameVersion, save: bool) -> Self {
        Self {
            sections: Vec::new(),
            state: save.then_some(WorldState {
                spawn_flags: (game == GameVersion::Gothic2).then_some(0),
                ..Default::default()
            }),
        }
    }
}
#[derive(Clone, Default, PartialEq)]
pub struct WayPoint {
    pub name: Vec<u8>,
    pub water_depth: i32,
    pub under_water: bool,
    pub position: Vec3,
    pub direction: Vec3,
}
impl fmt::Debug for WayPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WayPoint")
            .field("name_bytes", &self.name.len())
            .field("water_depth", &self.water_depth)
            .field("under_water", &self.under_water)
            .field("position", &self.position)
            .field("direction", &self.direction)
            .finish()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Way {
    pub left: Link,
    pub right: Link,
}
/// Initial declared point occurrences and edge endpoints are separate. Endpoints
/// need not occur in `points`; there is no duplicated mutable free_point flag.
/// Null links are retained.
#[derive(Clone, PartialEq, Eq)]
pub struct WayNet {
    pub version: i32,
    pub points: Vec<Link>,
    pub edges: Vec<Way>,
}
impl Default for WayNet {
    fn default() -> Self {
        Self {
            version: 1,
            points: Vec::new(),
            edges: Vec::new(),
        }
    }
}
impl fmt::Debug for WayNet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WayNet")
            .field("version", &self.version)
            .field("points", &self.points.len())
            .field("edges", &self.edges.len())
            .finish()
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gothic2Sky {
    pub fade_scale: f32,
    pub render_lightning: bool,
    pub is_raining: bool,
    pub rain_ctr: i32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SkyVersion {
    Gothic1,
    Gothic2(Gothic2Sky),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyController {
    pub master_time: f32,
    pub rain_weight: f32,
    pub rain_start: f32,
    pub rain_stop: f32,
    pub rain_sct_timer: f32,
    pub rain_snd_vol: f32,
    pub day_ctr: f32,
    pub version: SkyVersion,
}
impl SkyController {
    pub fn new(game: GameVersion) -> Self {
        Self {
            master_time: 0.,
            rain_weight: 0.,
            rain_start: 0.,
            rain_stop: 0.,
            rain_sct_timer: 0.,
            rain_snd_vol: 0.,
            day_ctr: 0.,
            version: match game {
                GameVersion::Gothic1 => SkyVersion::Gothic1,
                GameVersion::Gothic2 => SkyVersion::Gothic2(Default::default()),
            },
        }
    }
    pub fn game(&self) -> GameVersion {
        match self.version {
            SkyVersion::Gothic1 => GameVersion::Gothic1,
            SkyVersion::Gothic2(_) => GameVersion::Gothic2,
        }
    }
}
impl World {
    pub fn links(&self) -> impl DoubleEndedIterator<Item = &Link> + std::iter::FusedIterator {
        self.sections
            .iter()
            .flat_map(|s| {
                let fixed = match &s.data {
                    SectionData::WayNet(l)
                    | SectionData::CutscenePlayer(l)
                    | SectionData::SkyController(l) => Some(l),
                    _ => None,
                };
                let entries = match &s.data {
                    SectionData::VobTree(t) => t.entries.as_slice(),
                    _ => &[],
                };
                fixed.into_iter().chain(entries.iter().map(|e| &e.object))
            })
            .chain(
                self.state
                    .iter()
                    .flat_map(|s| s.npcs.iter().chain(s.spawns.iter().map(|v| &v.npc))),
            )
    }
    pub fn links_mut(
        &mut self,
    ) -> impl DoubleEndedIterator<Item = &mut Link> + std::iter::FusedIterator {
        self.sections
            .iter_mut()
            .flat_map(|s| {
                let (fixed, entries) = match &mut s.data {
                    SectionData::WayNet(l)
                    | SectionData::CutscenePlayer(l)
                    | SectionData::SkyController(l) => (Some(l), &mut [][..]),
                    SectionData::VobTree(t) => (None, t.entries.as_mut_slice()),
                    _ => (None, &mut [][..]),
                };
                fixed
                    .into_iter()
                    .chain(entries.iter_mut().map(|e| &mut e.object))
            })
            .chain(self.state.iter_mut().flat_map(|s| {
                s.npcs
                    .iter_mut()
                    .chain(s.spawns.iter_mut().map(|v| &mut v.npc))
            }))
    }
}
impl WayNet {
    pub fn links(&self) -> impl DoubleEndedIterator<Item = &Link> + std::iter::FusedIterator {
        self.points
            .iter()
            .chain(self.edges.iter().flat_map(|e| [&e.left, &e.right]))
    }
    pub fn links_mut(
        &mut self,
    ) -> impl DoubleEndedIterator<Item = &mut Link> + std::iter::FusedIterator {
        self.points.iter_mut().chain(
            self.edges
                .iter_mut()
                .flat_map(|e| [&mut e.left, &mut e.right]),
        )
    }
}
