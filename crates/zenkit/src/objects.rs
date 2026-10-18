//! Owned archive object graphs with stable checked handles.
//!
//! Flat ownership supports cycles without leaks or recursive destruction.
//! Unknown classes/versions and extra fields are rejected. Definitions are
//! registered before their children, allowing ancestor references and self-cycles.
//! A reference to an unseen ID returns [`ErrorKind::UnresolvedReference`].
//! Graph writers define objects on first encounter, regardless of storage order.
//! Primitive [`archive::Reader`] records expose numeric IDs without resolving them.
//!
//! ```
//! use zenkit::{GameVersion, archive::Format, objects::*};
//! let mut graph = Graph::new()?;
//! let id = graph.insert(Object::Block(CutsceneBlock::default()))?;
//! if let ObjectMut::Block(block) = graph.get_mut(id)? {
//!     block.child.target = Some(id); // real self-cycle, no Rc ownership cycle
//! }
//! graph.add_root(Link::new(b"root".to_vec(), Some(id)))?;
//! let document = ObjectArchive { metadata: Metadata::new(Format::BinSafe),
//!     game: GameVersion::Gothic2, graph };
//! let bytes = document.to_bytes(Limits::default())?;
//! let restored = ObjectArchive::from_bytes(&bytes, GameVersion::Gothic2, Limits::default())?;
//! assert_eq!(restored.graph.len(), 1);
//! # Ok::<(), zenkit::Error>(())
//! ```
// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use crate::{
    Error, ErrorKind, GameVersion, Result, archive,
    cutscene::{Context, Library, Manager, Player, PoolItem, Props},
    material::{self, Material},
    mesh, savegame,
    vob::{self, AiHuman, AiMove, EventManager, Visual, VisualKind, Vob},
    world,
};
use std::{
    collections::HashMap,
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};

mod access;
mod cutscene_state;
use cutscene_state::ContextPhase;
pub(crate) mod arena;
mod session;
mod storage;
pub use session::{ReadSession, WriteSession};
use storage::{Location, Pools};
pub use storage::{ObjectMut, ObjectRef};

#[derive(Debug)]
enum DefinitionPhase {
    Atomic,
    Vob(vob::Phase),
    Context(ContextPhase),
    World(world::codec::Phase),
}
impl DefinitionPhase {
    fn new(node: ObjectRef<'_>) -> Self {
        if matches!(node, ObjectRef::Atomic(_)) {
            Self::Atomic
        } else if matches!(node, ObjectRef::CutsceneContext(_)) {
            Self::Context(ContextPhase::Block)
        } else if matches!(
            node,
            ObjectRef::World(_)
                | ObjectRef::WayNet(_)
                | ObjectRef::WayPoint(_)
                | ObjectRef::SkyController(_)
        ) {
            Self::World(world::codec::Phase::new(node))
        } else {
            Self::Vob(vob::Phase::Base)
        }
    }
}

const MESSAGE: &[u8] = b"oCMsgConversation:oCNpcMessage:zCEventMessage";
const ATOMIC: &[u8] = b"zCCSAtomicBlock";
const BLOCK: &[u8] = b"zCCSBlock";
const LIBRARY: &[u8] = b"zCCSLib";
const MAX_DEPTH: usize = 128;
static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
fn bad(kind: ErrorKind, context: &'static str) -> Error {
    Error::operation(kind, context)
}
fn at(kind: ErrorKind, offset: usize, context: &'static str) -> Error {
    Error::new(kind, offset, context)
}
fn reserve<T>(v: &mut Vec<T>, count: usize) -> Result<()> {
    v.try_reserve(count)
        .map_err(|_| bad(ErrorKind::LimitExceeded, "object graph allocation"))
}

/// A stable graph-owned handle, distinct from the archive's numeric object index.
/// Slots are never deleted/reused. Moving the graph preserves IDs; an ID from a
/// destroyed/different graph is rejected by its unique owner token. Not a pointer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ObjectId {
    owner: u64,
    slot: usize,
}

/// One named wire occurrence. Null is explicit and distinct from a bad reference.
/// Names belong to occurrences, so multiple references may have different names.
#[derive(Clone, PartialEq, Eq)]
pub struct Link {
    pub name: Vec<u8>,
    pub target: Option<ObjectId>,
}
impl Link {
    pub fn new(name: Vec<u8>, target: Option<ObjectId>) -> Self {
        Self { name, target }
    }
}
impl Default for Link {
    fn default() -> Self {
        Self::new(b"%".to_vec(), None)
    }
}
impl fmt::Debug for Link {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Link")
            .field("name_bytes", &self.name.len())
            .field("target", &self.target)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MessageState {
    pub high_priority: bool,
    pub in_use: bool,
    pub deleted: bool,
}
/// `state` is absent in normal archives and present in save archives. Conversion
/// between modes requires the caller to explicitly add/remove that state.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ConversationMessage {
    pub subtype: u32,
    pub text: Vec<u8>,
    pub audio_name: Vec<u8>,
    pub state: Option<MessageState>,
}
impl fmt::Debug for ConversationMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConversationMessage")
            .field("subtype", &self.subtype)
            .field("text_bytes", &self.text.len())
            .field("audio_bytes", &self.audio_name.len())
            .field("state", &self.state)
            .finish()
    }
}
/// A cutscene message and its optional synchronization flag.
///
/// A null message is a supported wire value. The optional flag follows the
/// message and preserves absence separately from an explicit `false` or `true`.
/// It is observed in Gothic I ASCII dialogue libraries; all three encodings
/// support preserving it. As with other archive booleans, nonzero wire values
/// become `true` and writers emit a canonical boolean value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AtomicBlock {
    pub message: Link,
    pub synchronized: Option<bool>,
}
/// One sub-block is supported, preserving the raw f32 value of `sub_block0`.
/// A null child permits draft construction (including cycles), but
/// validation/serialization reject it.
#[derive(Clone, Default, PartialEq)]
pub struct CutsceneBlock {
    pub name: Vec<u8>,
    pub sub_block0: f32,
    pub child: Link,
}
impl fmt::Debug for CutsceneBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CutsceneBlock")
            .field("name_bytes", &self.name.len())
            .field("sub_block0", &self.sub_block0)
            .field("child", &self.child)
            .finish()
    }
}
#[derive(Clone, Debug, PartialEq)]
// Graph stores typed payloads separately; it does not retain this enum.
#[allow(clippy::large_enum_variant)]
pub enum Object {
    Message(ConversationMessage),
    Atomic(AtomicBlock),
    Block(CutsceneBlock),
    Material(Material),
    Library(Library),
    Vob(Vob),
    Visual(Visual),
    EventManager(EventManager),
    AiMove(AiMove),
    AiHuman(AiHuman),
    NpcTalent(vob::NpcTalent),
    CutsceneProps(Props),
    Cutscene(CutsceneBlock),
    CutsceneContext(Context),
    CutscenePlayer(Player),
    CutscenePoolItem(PoolItem),
    CutsceneManager(Manager),
    World(world::World),
    WayNet(world::WayNet),
    WayPoint(world::WayPoint),
    SkyController(world::SkyController),
    SaveMetadata(savegame::SaveMetadata),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Message,
    Atomic,
    Block,
    Material,
    Library,
    Vob,
    Visual,
    EventManager,
    AiMove,
    AiHuman,
    NpcTalent,
    CutsceneProps,
    Cutscene,
    CutsceneContext,
    CutscenePlayer,
    CutscenePoolItem,
    CutsceneManager,
    World,
    WayNet,
    WayPoint,
    SkyController,
    SaveMetadata,
}

/// An editable draft graph. Lookup checks handles; direct `get_mut`/`roots_mut`
/// edits may temporarily violate edge kinds, mode or budgets. `validate` and
/// serialization enforce whole-graph validity, including orphan nodes. There is
/// no deletion/slot reuse or implicit Clone that would conflate graph identities.
pub struct Graph {
    owner: u64,
    nodes: arena::Arena<Location>,
    payloads: Pools,
    roots: Vec<Link>,
    versions: HashMap<usize, RetainedVersion>,
}
#[derive(Default)]
struct Usage {
    copied: usize,
    links: usize,
}
#[derive(Clone, Copy, Debug)]
struct RetainedVersion {
    class: &'static [u8],
    game: GameVersion,
    version: u16,
}
// Narrow identifiers observed in complete saved-world records. Payload parsing
// still uses the exact class/game schema and requires its complete object end.
fn canonical_alias(class: &[u8], version: u16, game: GameVersion) -> Option<u16> {
    match (class, version, game) {
        (b"oCCSPlayer:zCCSPlayer" | b"oCCSManager:zCCSManager", 36865, _)
        | (b"zCSkyControler_Outdoor", 52417, _) => Some(0),
        (b"oCAIHuman:oCAniCtrl_Human:zCAIPlayer", 36865, GameVersion::Gothic2)
        | (b"oCNpcTalent", 64513, GameVersion::Gothic2) => Some(0),
        (b"oCNpc:zCVob", 29697, GameVersion::Gothic2) => Some(52224),
        (b"zCTriggerList:zCTrigger:zCVob", 45570, GameVersion::Gothic2) => Some(5505),
        (b"zCCSCamera:zCVob", 12289, GameVersion::Gothic2) => Some(33793),
        (
            b"zCPFXControler:zCVob" | b"zCTriggerWorldStart:zCVob" | b"zCEarthquake:zCVob",
            20483,
            GameVersion::Gothic2,
        ) => Some(52224),
        (b"zCTriggerUntouch:zCVob", 33026, GameVersion::Gothic2) => Some(193),
        (b"zCVobScreenFX:zCVob", 12289, GameVersion::Gothic2) => Some(52224),
        _ => None,
    }
}
fn default_version(node: ObjectRef<'_>) -> u16 {
    match node {
        ObjectRef::Material(v) => v.version_identifier(),
        ObjectRef::Vob(v) => v.version_identifier(),
        ObjectRef::Visual(v) => v.version,
        ObjectRef::World(_) => 64513,
        _ => 0,
    }
}
impl fmt::Debug for Graph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Graph")
            .field("owner", &self.owner)
            .field("nodes", &self.nodes.len())
            .field("roots", &self.roots.len())
            .finish()
    }
}
impl Graph {
    pub fn new() -> Result<Self> {
        let owner = NEXT_OWNER
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
            .map_err(|_| bad(ErrorKind::LimitExceeded, "graph owner identities exhausted"))?;
        Ok(Self {
            owner,
            nodes: arena::Arena::default(),
            payloads: Pools::default(),
            roots: Vec::new(),
            versions: HashMap::new(),
        })
    }
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
    fn register(
        &mut self,
        class: &[u8],
        version: u16,
        game: GameVersion,
        save: bool,
        material: Option<Material>,
    ) -> Result<ObjectId> {
        let prepared = self.nodes.prepare()?;
        let slot = self.nodes.len();
        let location = self
            .payloads
            .register(class, version, game, save, slot, material)?;
        self.nodes.commit(prepared, location);
        Ok(ObjectId {
            owner: self.owner,
            slot,
        })
    }
    pub fn insert(&mut self, object: Object) -> Result<ObjectId> {
        let node = self.nodes.prepare()?;
        let prepared = self.payloads.prepare(&object)?;
        let slot = self.nodes.len();
        let location = self.payloads.commit(prepared, object, slot);
        self.nodes.commit(node, location);
        Ok(ObjectId {
            owner: self.owner,
            slot,
        })
    }
    /// Replace an object's schema or payload while preserving its stable ID.
    /// Allocation failure leaves the logical graph and retained aliases intact.
    /// Related edge kinds are checked by whole-graph validation after draft edits.
    pub fn replace(&mut self, id: ObjectId, object: Object) -> Result<Object> {
        let slot = self.slot(id)?;
        let location = *self.nodes.get(slot);
        let old = match self.payloads.replace_same(location, object)? {
            arena::Replacement::Replaced(old, ()) => old,
            arena::Replacement::Different(object) => {
                let prepared = self.payloads.prepare(&object)?;
                let (old, moved) = self.payloads.remove(location);
                if let Some((owner, location)) = moved {
                    *self.nodes.get_mut(owner) = location;
                }
                let location = self.payloads.commit(prepared, object, slot);
                *self.nodes.get_mut(slot) = location;
                old
            }
        };
        self.clear_stale_version(id)?;
        Ok(old)
    }
    /// Replace a VOB subtype while retaining its common fields and stable ID.
    pub fn replace_vob_kind(&mut self, id: ObjectId, kind: vob::VobKind) -> Result<vob::VobKind> {
        let slot = self.slot(id)?;
        let old = self
            .payloads
            .replace_vob_kind(*self.nodes.get(slot), kind)?;
        self.clear_stale_version(id)?;
        Ok(old)
    }
    fn clear_stale_version(&mut self, id: ObjectId) -> Result<()> {
        let slot = self.slot(id)?;
        if let Some(version) = self.versions.get(&slot) {
            let node = self.get(id)?;
            let game = match node {
                ObjectRef::Vob(v) => Some(v.game()),
                ObjectRef::SkyController(v) => Some(v.game()),
                ObjectRef::SaveMetadata(v) => Some(v.game()),
                _ => None,
            };
            if node.class() != version.class
                || game.is_some_and(|g| g != version.game)
                || canonical_alias(version.class, version.version, version.game)
                    != Some(default_version(node))
            {
                self.versions.remove(&slot);
            }
        }
        Ok(())
    }
    fn slot(&self, id: ObjectId) -> Result<usize> {
        if id.owner != self.owner || id.slot >= self.nodes.len() {
            Err(bad(ErrorKind::InvalidHandle, "object graph handle"))
        } else {
            Ok(id.slot)
        }
    }
    pub fn get(&self, id: ObjectId) -> Result<ObjectRef<'_>> {
        Ok(self.payloads.get(*self.nodes.get(self.slot(id)?)))
    }
    pub fn get_mut(&mut self, id: ObjectId) -> Result<ObjectMut<'_>> {
        let slot = self.slot(id)?;
        Ok(self.payloads.get_mut(*self.nodes.get(slot)))
    }
    /// Effective retained wire version for this owner-checked node and game.
    /// Historical aliases are tied to the exact parsed class and game; changing
    /// the draft's class cannot silently transfer an alias to a different schema.
    /// Camera frames and visuals keep their version in their own public payload.
    pub fn record_version(&self, id: ObjectId, game: GameVersion) -> Result<u16> {
        let node = self.get(id)?;
        let payload_game = match node {
            ObjectRef::Vob(v) => Some(v.game()),
            ObjectRef::SkyController(v) => Some(v.game()),
            ObjectRef::SaveMetadata(v) => Some(v.game()),
            ObjectRef::Material(v) => {
                v.check_game(game)?;
                None
            }
            ObjectRef::Visual(Visual {
                kind: VisualKind::Decal(v),
                ..
            }) => Some(v.game()),
            _ => None,
        };
        if payload_game.is_some_and(|g| g != game) {
            return Err(bad(ErrorKind::InvalidData, "object version game mismatch"));
        }
        if let Some(v) = self.versions.get(&id.slot) {
            if v.class != node.class()
                || v.game != game
                || canonical_alias(v.class, v.version, game) != Some(default_version(node))
            {
                return Err(bad(ErrorKind::InvalidData, "stale retained object version"));
            }
            return Ok(v.version);
        }
        Ok(default_version(node))
    }
    /// Explicitly discard a historical record alias, selecting the current
    /// class's canonical version. Payload-owned visual/frame versions are unchanged.
    pub fn clear_record_version(&mut self, id: ObjectId) -> Result<()> {
        self.slot(id)?;
        self.versions.remove(&id.slot);
        Ok(())
    }
    pub fn objects(
        &self,
    ) -> impl ExactSizeIterator<Item = (ObjectId, ObjectRef<'_>)> + DoubleEndedIterator + '_ {
        self.nodes.iter().enumerate().map(|(slot, node)| {
            (
                ObjectId {
                    owner: self.owner,
                    slot,
                },
                self.payloads.get(*node),
            )
        })
    }
    pub fn roots(&self) -> &[Link] {
        &self.roots
    }
    pub fn roots_mut(&mut self) -> &mut [Link] {
        &mut self.roots
    }
    /// Checked insertion validates the root target immediately. User-provided
    /// byte buffers remain subject to whole-document limits at validation time.
    pub fn add_root(&mut self, root: Link) -> Result<()> {
        if let Some(id) = root.target {
            self.slot(id)?;
        }
        reserve(&mut self.roots, 1)?;
        self.roots.push(root);
        Ok(())
    }
    fn edge(&self, link: &Link, allowed: &[Kind], required: bool) -> Result<()> {
        match link.target {
            Some(id) => {
                if !allowed.contains(&self.get(id)?.kind()) {
                    return Err(bad(ErrorKind::WrongObjectType, "object graph edge kind"));
                }
            }
            None if required => {
                return Err(bad(
                    ErrorKind::InvalidData,
                    "required object edge must be non-null",
                ));
            }
            None => {}
        }
        Ok(())
    }
    fn child_edge(&self, node: ObjectRef<'_>, index: usize, link: &Link) -> Result<()> {
        let (allowed, required): (&[Kind], bool) = match node {
            ObjectRef::Atomic(_) => (&[Kind::Message], false),
            ObjectRef::Block(_) | ObjectRef::Cutscene(_) => (&[Kind::Atomic, Kind::Block], true),
            ObjectRef::CutsceneContext(_) => match index {
                0 => (&[Kind::Atomic, Kind::Block], true),
                1 => (&[Kind::CutsceneProps], false),
                2 | 3 => {
                    if let Some(id) = link.target
                        && !matches!(self.get(id)?,ObjectRef::Vob(v) if matches!(v.kind,vob::VobKindRef::Npc(_)))
                    {
                        return Err(bad(ErrorKind::WrongObjectType, "cutscene NPC subtype"));
                    }
                    return Ok(());
                }
                4 | 5 => {
                    if let Some(id) = link.target {
                        self.slot(id)?;
                    }
                    return Ok(());
                }
                _ => unreachable!(),
            },
            ObjectRef::CutscenePlayer(_) => (&[Kind::CutsceneContext], false),
            ObjectRef::CutsceneManager(_) => (&[Kind::CutscenePoolItem], false),
            ObjectRef::Library(_) => (&[Kind::Block], true),
            ObjectRef::Vob(v) => {
                let visual = usize::from(v.base.visual.is_some());
                if index < visual {
                    (&[Kind::Visual], false)
                } else if index < visual + usize::from(v.base.ai.is_some()) {
                    (&[Kind::AiMove, Kind::AiHuman], false)
                } else if index < v.base_child_count() {
                    (&[Kind::EventManager], false)
                } else if let Some(n) = v.kind.npc() {
                    let index = index - v.base_child_count();
                    let expected = if index < n.talents.len() {
                        vob::npc_codec::NpcEdge::Talent
                    } else if index < n.talents.len() + 2 {
                        vob::npc_codec::NpcEdge::Vob
                    } else {
                        vob::npc_codec::NpcEdge::Item
                    };
                    return self.npc_edge(link, expected);
                } else if matches!(v.kind, vob::VobKindRef::Container(_)) {
                    if let Some(id) = link.target
                        && !matches!(self.get(id)?, ObjectRef::Vob(v) if matches!(v.kind, vob::VobKindRef::Item(_)))
                    {
                        return Err(bad(ErrorKind::WrongObjectType, "container item subtype"));
                    }
                    return Ok(());
                } else if matches!(v.kind, vob::VobKindRef::Camera(_)) {
                    if let Some(id) = link.target
                        && !matches!(self.get(id)?,ObjectRef::Vob(v) if matches!(v.kind,vob::VobKindRef::CameraFrame(_)))
                    {
                        return Err(bad(ErrorKind::WrongObjectType, "camera frame subtype"));
                    }
                    return Ok(());
                } else if matches!(v.kind, vob::VobKindRef::CodeMaster(_)) {
                    if let Some(id) = link.target {
                        self.slot(id)?;
                    }
                    return Ok(());
                } else {
                    (&[Kind::Vob], false)
                }
            }
            ObjectRef::AiMove(_) if index == 0 => (&[Kind::Vob], false),
            ObjectRef::AiMove(_) | ObjectRef::AiHuman(_) => {
                if let Some(id) = link.target
                    && !matches!(self.get(id)?,ObjectRef::Vob(v) if matches!(v.kind,vob::VobKindRef::Npc(_)))
                {
                    return Err(bad(ErrorKind::WrongObjectType, "AI NPC subtype"));
                }
                return Ok(());
            }
            ObjectRef::EventManager(_) => (&[Kind::CutsceneContext], false),
            _ => unreachable!(),
        };
        self.edge(link, allowed, required)
    }
    fn npc_edge(&self, link: &Link, expected: vob::npc_codec::NpcEdge) -> Result<()> {
        if let Some(id) = link.target {
            let node = self.get(id)?;
            let valid = match expected {
                vob::npc_codec::NpcEdge::Talent => matches!(node, ObjectRef::NpcTalent(_)),
                vob::npc_codec::NpcEdge::Vob => matches!(node, ObjectRef::Vob(_)),
                vob::npc_codec::NpcEdge::Item => {
                    matches!(node,ObjectRef::Vob(v) if matches!(v.kind,vob::VobKindRef::Item(_)))
                }
            };
            if !valid {
                return Err(bad(ErrorKind::WrongObjectType, "NPC occurrence subtype"));
            }
        }
        Ok(())
    }
    fn world_edge(&self, link: &Link, expected: world::codec::Edge) -> Result<()> {
        use world::codec::Edge;
        if let Some(id) = link.target {
            let node = self.get(id)?;
            let valid = match expected {
                Edge::Vob => matches!(node, ObjectRef::Vob(_)),
                Edge::WayPoint => matches!(node, ObjectRef::WayPoint(_)),
                Edge::WayNet => matches!(node, ObjectRef::WayNet(_)),
                Edge::Player => matches!(node, ObjectRef::CutscenePlayer(_)),
                Edge::Sky => matches!(node, ObjectRef::SkyController(_)),
                Edge::Npc => {
                    matches!(node,ObjectRef::Vob(v) if matches!(v.kind,vob::VobKindRef::Npc(_)))
                }
            };
            if !valid {
                return Err(bad(ErrorKind::WrongObjectType, "world occurrence subtype"));
            }
        }
        Ok(())
    }
    fn item_key(&self, link: &Link) -> Result<bool> {
        self.npc_edge(link, vob::npc_codec::NpcEdge::Item)?;
        Ok(match link.target.map(|id| self.get(id)).transpose()? {
            Some(ObjectRef::Vob(v)) => match &v.kind {
                vob::VobKindRef::Item(i) => i.state.is_some_and(|s| s.flags & 0x200 != 0),
                _ => unreachable!(),
            },
            _ => false,
        })
    }
    fn npc_items(&self, n: &vob::NpcData) -> Result<()> {
        for item in &n.items {
            if item.short_key.is_some() != self.item_key(&item.item)? {
                return Err(bad(
                    ErrorKind::InvalidData,
                    "NPC shortKey presence must match Item flags",
                ));
            }
        }
        Ok(())
    }
    /// Validate semantic edges, save state and owned-byte/count budgets for all
    /// nodes, including unreachable nodes. This does not preflight encoding,
    /// nesting or output limits; those are checked by `ObjectArchive::to_bytes`.
    /// Serialization emits only root-reachable nodes, in root/link order,
    /// and renumbers wire IDs as needed.
    pub fn validate(&self, save_game: bool, limits: Limits) -> Result<()> {
        self.validate_usage(save_game, limits).map(|_| ())
    }
    fn validate_usage(&self, save_game: bool, limits: Limits) -> Result<Usage> {
        if self.nodes.len() > limits.nodes || self.roots.len() > limits.roots {
            return Err(bad(ErrorKind::LimitExceeded, "object graph count"));
        }
        let mut total = 0;
        let mut links = 0;
        let mut sequences = 0usize;
        let mut geometry = mesh::Budget::new(limits.world.geometry);
        let mut sections = 0usize;
        link_count(&mut links, self.roots.len(), limits.links)?;
        for link in &self.roots {
            count(&mut total, link.name.len(), limits.copied_bytes)?;
            if let Some(id) = link.target {
                self.slot(id)?;
            }
        }
        for location in self.nodes.iter() {
            let node = self.payloads.get(*location);
            match node {
                ObjectRef::World(w) => world::codec::validate(
                    w,
                    save_game,
                    limits,
                    &mut geometry,
                    &mut sections,
                    &mut total,
                )?,
                ObjectRef::WayNet(w) => {
                    i32::try_from(w.points.len())
                        .and_then(|_| i32::try_from(w.edges.len()))
                        .map_err(|_| bad(ErrorKind::LimitExceeded, "waynet count"))?;
                }
                ObjectRef::WayPoint(w) => count(&mut total, w.name.len(), limits.copied_bytes)?,
                ObjectRef::SkyController(_) => {}
                ObjectRef::SaveMetadata(v) => {
                    for bytes in v.buffers() {
                        count(&mut total, bytes.len(), limits.copied_bytes)?;
                    }
                }
                ObjectRef::Vob(v) => {
                    v.check(save_game)?;
                    if let Some(n) = v.kind.npc() {
                        self.npc_items(n)?;
                    }
                    sequences = sequences
                        .checked_add(v.kind.sequence_items())
                        .filter(|n| *n <= limits.sequence_items)
                        .ok_or_else(|| bad(ErrorKind::LimitExceeded, "object sequence items"))?;
                    for bytes in v.buffers() {
                        count(&mut total, bytes.len(), limits.copied_bytes)?;
                    }
                }
                ObjectRef::Visual(v) => {
                    if let VisualKind::Decal(d) = &v.kind {
                        if !vob::Decal::version_supported(v.version, d.game()) {
                            return Err(bad(ErrorKind::Unsupported, "decal version"));
                        }
                        count(&mut total, d.name.len(), limits.copied_bytes)?;
                    }
                }
                ObjectRef::AiMove(_)
                | ObjectRef::AiHuman(_)
                | ObjectRef::EventManager(_)
                | ObjectRef::NpcTalent(_) => {}
                ObjectRef::CutsceneProps(v) => {
                    for b in [&v.name, &v.stage_name, &v.script_function_on_stop] {
                        count(&mut total, b.len(), limits.copied_bytes)?;
                    }
                }
                ObjectRef::CutscenePoolItem(v) => {
                    count(&mut total, v.name.len(), limits.copied_bytes)?
                }
                ObjectRef::CutsceneContext(v) => {
                    count(&mut total, v.block.name.len(), limits.copied_bytes)?
                }
                ObjectRef::CutscenePlayer(v) => {
                    i32::try_from(v.playlists.len())
                        .map_err(|_| bad(ErrorKind::LimitExceeded, "cutscene playlist count"))?;
                }
                ObjectRef::CutsceneManager(v) => {
                    i32::try_from(v.pool_items.len())
                        .map_err(|_| bad(ErrorKind::LimitExceeded, "cutscene pool count"))?;
                }
                ObjectRef::Library(v) => {
                    link_count(&mut links, v.blocks.len(), limits.links)?;
                    i32::try_from(v.blocks.len()).map_err(|_| {
                        bad(ErrorKind::LimitExceeded, "cutscene library item count")
                    })?;
                    for link in &v.blocks {
                        self.edge(link, &[Kind::Block], true)?;
                        count(&mut total, link.name.len(), limits.copied_bytes)?;
                    }
                }
                ObjectRef::Material(v) => {
                    for bytes in v.buffers() {
                        count(&mut total, bytes.len(), limits.copied_bytes)?;
                    }
                }
                ObjectRef::Message(v) => {
                    if v.state.is_some() != save_game {
                        return Err(bad(
                            ErrorKind::InvalidData,
                            "message state must match archive save mode",
                        ));
                    }
                    count(&mut total, v.text.len(), limits.copied_bytes)?;
                    count(&mut total, v.audio_name.len(), limits.copied_bytes)?;
                }
                ObjectRef::Atomic(v) => {
                    link_count(&mut links, 1, limits.links)?;
                    self.edge(&v.message, &[Kind::Message], false)?;
                    count(&mut total, v.message.name.len(), limits.copied_bytes)?;
                }
                ObjectRef::Block(v) | ObjectRef::Cutscene(v) => {
                    link_count(&mut links, 1, limits.links)?;
                    self.edge(&v.child, &[Kind::Atomic, Kind::Block], true)?;
                    count(&mut total, v.name.len(), limits.copied_bytes)?;
                    count(&mut total, v.child.name.len(), limits.copied_bytes)?;
                }
            }
            if matches!(node, ObjectRef::World(_) | ObjectRef::WayNet(_)) {
                for (kind, link) in world::codec::edges(node) {
                    link_count(&mut links, 1, limits.links)?;
                    self.world_edge(link, kind)?;
                    count(&mut total, link.name.len(), limits.copied_bytes)?;
                }
            }
            if matches!(
                node,
                ObjectRef::Vob(_)
                    | ObjectRef::EventManager(_)
                    | ObjectRef::AiMove(_)
                    | ObjectRef::AiHuman(_)
                    | ObjectRef::CutsceneContext(_)
                    | ObjectRef::CutscenePlayer(_)
                    | ObjectRef::CutsceneManager(_)
            ) {
                link_count(&mut links, node.child_count(), limits.links)?;
                if let ObjectRef::Vob(v) = node {
                    for (i, link) in v.links().enumerate() {
                        self.child_edge(node, i, link)?;
                        count(&mut total, link.name.len(), limits.copied_bytes)?;
                    }
                } else {
                    for i in 0..node.child_count() {
                        let link = node.child(i).expect("existing child");
                        self.child_edge(node, i, link)?;
                        count(&mut total, link.name.len(), limits.copied_bytes)?;
                    }
                }
            }
        }
        for (&slot, v) in &self.versions {
            self.record_version(
                ObjectId {
                    owner: self.owner,
                    slot,
                },
                v.game,
            )?;
        }
        Ok(Usage {
            copied: total,
            links,
        })
    }
}

/// Owned metadata retained from the primitive archive header.
#[derive(Clone, PartialEq, Eq)]
pub struct Metadata {
    pub version: u32,
    pub archiver: Vec<u8>,
    pub format: archive::Format,
    pub save_game: bool,
    pub date: Option<Vec<u8>>,
    pub user: Option<Vec<u8>>,
}
impl Metadata {
    pub fn new(format: archive::Format) -> Self {
        let h = archive::Header::new(format);
        Self {
            version: h.version,
            archiver: h.archiver.to_vec(),
            format,
            save_game: false,
            date: None,
            user: None,
        }
    }
    pub fn as_header(&self) -> archive::Header<'_> {
        archive::Header {
            version: self.version,
            archiver: &self.archiver,
            format: self.format,
            save_game: self.save_game,
            date: self.date.as_deref(),
            user: self.user.as_deref(),
        }
    }
}
impl fmt::Debug for Metadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_header().fmt(f)
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Primitive input/output bytes, variable fields, keys, framing records and
    /// depth. Encoding/output checks run during serialization, not `validate`.
    pub archive: archive::Limits,
    /// All unique owned nodes, including orphans during validation.
    pub nodes: usize,
    /// Ordered root occurrences, including nulls and repeated aliases.
    pub roots: usize,
    /// Cumulative root and child occurrences, including nulls, aliases and orphan
    /// edges during validation. Bounds retained Link slots independently of nodes.
    pub links: usize,
    /// Cumulative metadata, occurrence names and all owned class byte buffers,
    /// including material text, VOB names/packed tails, subtype payloads, raw
    /// light animation text, opaque screen fields and decal names.
    /// Validation includes orphan buffers; parsing checks before each copy.
    /// SAVEDAT sessions also charge state strings and the complete guild table.
    pub copied_bytes: usize,
    /// Cumulative Mover samples, TriggerList targets, CodeMaster slave-name
    /// slots and NPC overlays/news/inventory-slot descriptors. Includes orphans
    /// during validation; talents/items/used-slot occurrences use `links`.
    /// SAVEDAT also charges infos, missions/history, topics/entries and symbols
    /// plus G2 symbol values across the same session.
    pub sequence_items: usize,
    /// Aggregate world sections and geometry; hierarchy and BSP depths are
    /// independent from object-reference framing depth.
    pub world: world::Limits,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            archive: archive::Limits::default(),
            nodes: 100_000,
            roots: 100_000,
            links: 1_000_000,
            copied_bytes: 256 * 1024 * 1024,
            sequence_items: 1_000_000,
            world: world::Limits::default(),
        }
    }
}
/// Owned metadata, caller-selected game schema and graph. The cutscene classes
/// share G1/G2 layouts; materials and VOB/decal records have checked game fields.
/// Empty visual schemas retain their uninterpreted object version identifiers.
/// Game choice is not inferred from generic archive headers.
#[derive(Debug)]
pub struct ObjectArchive {
    pub metadata: Metadata,
    pub game: GameVersion,
    pub graph: Graph,
}
impl ObjectArchive {
    pub fn from_bytes(bytes: &[u8], game: GameVersion, limits: Limits) -> Result<Self> {
        let (metadata, mut parser) = Parser::new(bytes, game, limits)?;
        while parser.read_root()? {}
        Ok(Self {
            metadata,
            game,
            graph: parser.finish()?,
        })
    }
    /// Validate metadata/graph owned-byte budgets and whole-graph semantics.
    /// Encoding representability, metadata version/format rules, traversal depth
    /// and output/key budgets are checked separately by `to_bytes`.
    pub fn validate(&self, limits: Limits) -> Result<()> {
        validate_parts(&self.metadata, self.game, &self.graph, limits)
    }
    /// Produce a complete independent archive or an error; the graph is never
    /// mutated. Unknown classes/extra fields are rejected on read, not discarded.
    /// Iterative traversal has a hard framing depth ceiling of 128 even if
    /// configured archive depth is larger. Reference occurrences count as depth.
    pub fn to_bytes(&self, limits: Limits) -> Result<Vec<u8>> {
        self.validate(limits)?;
        let output = archive::Writer::new(self.metadata.as_header(), limits.archive)?;
        let mut writer = Encoder {
            output,
            graph: &self.graph,
            game: self.game,
            ids: HashMap::new(),
            limits,
        };
        for root in self.graph.roots() {
            writer.link(root)?;
        }
        writer.output.finish()
    }
}
fn count(total: &mut usize, amount: usize, limit: usize) -> Result<()> {
    *total = total
        .checked_add(amount)
        .filter(|v| *v <= limit)
        .ok_or_else(|| bad(ErrorKind::LimitExceeded, "object copied bytes"))?;
    Ok(())
}
fn link_count(total: &mut usize, amount: usize, limit: usize) -> Result<()> {
    *total = total
        .checked_add(amount)
        .filter(|v| *v <= limit)
        .ok_or_else(|| bad(ErrorKind::LimitExceeded, "object link count"))?;
    Ok(())
}
pub(crate) fn copy(bytes: &[u8], total: &mut usize, limit: usize, p: usize) -> Result<Vec<u8>> {
    count(total, bytes.len(), limit)
        .map_err(|_| at(ErrorKind::LimitExceeded, p, "object copied bytes"))?;
    let mut out = Vec::new();
    reserve(&mut out, bytes.len())
        .map_err(|_| at(ErrorKind::LimitExceeded, p, "object string allocation"))?;
    out.extend_from_slice(bytes);
    Ok(out)
}
fn depth(depth: usize, limits: Limits, p: Option<usize>) -> Result<()> {
    if depth > limits.archive.depth.min(MAX_DEPTH) {
        return Err(match p {
            Some(p) => at(ErrorKind::LimitExceeded, p, "object graph nesting"),
            None => bad(ErrorKind::LimitExceeded, "object graph nesting"),
        });
    }
    Ok(())
}
pub(crate) fn validate_parts(
    metadata: &Metadata,
    game: GameVersion,
    graph: &Graph,
    limits: Limits,
) -> Result<()> {
    let mut metadata_bytes = 0;
    for bytes in [
        Some(metadata.archiver.as_slice()),
        metadata.date.as_deref(),
        metadata.user.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        count(&mut metadata_bytes, bytes.len(), limits.copied_bytes)?;
    }
    let limits = Limits {
        copied_bytes: limits.copied_bytes - metadata_bytes,
        ..limits
    };
    graph.validate(metadata.save_game, limits)?;
    validate_game(graph, game, metadata.user.as_deref() == Some(b"XZEN"))
}
fn validate_game(graph: &Graph, game: GameVersion, wide_g1: bool) -> Result<()> {
    for (id, node) in graph.objects() {
        graph.record_version(id, game)?;
        match node {
            ObjectRef::World(w) => {
                if let Some(s) = &w.state
                    && s.spawn_flags.is_some() != (game == GameVersion::Gothic2)
                {
                    return Err(bad(ErrorKind::InvalidData, "world save game mismatch"));
                }
                for section in &w.sections {
                    if let world::SectionData::MeshAndBsp(g) = &section.data {
                        if g.mesh.game != game {
                            return Err(bad(
                                ErrorKind::InvalidData,
                                "world geometry game mismatch",
                            ));
                        }
                        if game == GameVersion::Gothic1 && g.mesh.wide_indices != wide_g1 {
                            return Err(bad(
                                ErrorKind::InvalidData,
                                "world XZEN index-width metadata",
                            ));
                        }
                    }
                }
            }
            ObjectRef::SkyController(v) if v.game() != game => {
                return Err(bad(ErrorKind::InvalidData, "sky game mismatch"));
            }
            ObjectRef::Material(v) => v.check_game(game)?,
            ObjectRef::Vob(v) if v.game() != game => {
                return Err(bad(ErrorKind::InvalidData, "VOB game mismatch"));
            }
            ObjectRef::Visual(Visual {
                kind: VisualKind::Decal(v),
                ..
            }) if v.game() != game => {
                return Err(bad(ErrorKind::InvalidData, "decal game mismatch"));
            }
            _ => {}
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum DefinitionState {
    Incomplete,
    // Private wire hint, never exposed as a partially initialized Item value.
    SavedBinaryItem(i32),
    Complete,
}

pub(crate) struct Parser<'a> {
    pub(crate) reader: archive::Reader<'a>,
    graph: Graph,
    ids: HashMap<u32, ObjectId>,
    definitions: Vec<DefinitionState>,
    deferred_keys: Vec<(ObjectId, usize, usize)>,
    pub(crate) limits: Limits,
    pub(crate) copied: usize,
    links: usize,
    pub(crate) sequences: usize,
    geometry: mesh::Budget<'a>,
    world_sections: usize,
    game: GameVersion,
}
impl<'a> Parser<'a> {
    pub(crate) fn new(
        bytes: &'a [u8],
        game: GameVersion,
        limits: Limits,
    ) -> Result<(Metadata, Self)> {
        let reader = archive::Reader::new(bytes, limits.archive)?;
        Self::from_reader(reader, game, limits)
    }
    fn from_reader(
        reader: archive::Reader<'a>,
        game: GameVersion,
        limits: Limits,
    ) -> Result<(Metadata, Self)> {
        if reader.configured_limits() != limits.archive {
            return Err(at(
                ErrorKind::InvalidData,
                reader.position(),
                "archive session limits mismatch",
            ));
        }
        let header = reader.header();
        let mut copied = 0;
        let offset = |part: &[u8]| reader.offset_of(part).expect("borrowed archive header");
        let metadata = Metadata {
            version: header.version,
            archiver: copy(
                header.archiver,
                &mut copied,
                limits.copied_bytes,
                offset(header.archiver),
            )?,
            format: header.format,
            save_game: header.save_game,
            date: header
                .date
                .map(|v| copy(v, &mut copied, limits.copied_bytes, offset(v)))
                .transpose()?,
            user: header
                .user
                .map(|v| copy(v, &mut copied, limits.copied_bytes, offset(v)))
                .transpose()?,
        };
        let space = reader.address_space();
        let parser = Self {
            reader,
            graph: Graph::new()?,
            ids: HashMap::new(),
            definitions: Vec::new(),
            deferred_keys: Vec::new(),
            limits,
            copied,
            links: 0,
            sequences: 0,
            geometry: mesh::Budget::with_space(limits.world.geometry, space),
            world_sections: 0,
            game,
        };
        Ok((metadata, parser))
    }
    pub(crate) fn graph(&self) -> &Graph {
        &self.graph
    }
    pub(crate) fn read_root(&mut self) -> Result<bool> {
        let p = self.reader.position();
        let Some(header) = self.reader.begin_object()? else {
            return Ok(false);
        };
        if self.graph.roots.len() >= self.limits.roots {
            return Err(at(ErrorKind::LimitExceeded, p, "object roots"));
        }
        let root = self.open(header, p)?;
        self.graph.add_root(root)?;
        Ok(true)
    }
    pub(crate) fn finish(self) -> Result<Graph> {
        let (reader, graph) = self.into_parts()?;
        reader.finish()?;
        Ok(graph)
    }
    fn into_parts(self) -> Result<(archive::Reader<'a>, Graph)> {
        if self
            .definitions
            .iter()
            .any(|state| !matches!(state, DefinitionState::Complete))
        {
            return Err(at(
                ErrorKind::InvalidData,
                self.reader.position(),
                "unfinished object definition",
            ));
        }
        for (id, index, p) in &self.deferred_keys {
            let ObjectRef::Vob(v) = self.graph.get(*id)? else {
                unreachable!()
            };
            let item = &v.kind.npc().expect("NPC definition").items[*index];
            if item.short_key.is_some() != self.graph.item_key(&item.item)? {
                return Err(at(
                    ErrorKind::InvalidData,
                    *p,
                    "NPC shortKey presence must match Item flags",
                ));
            }
        }
        Ok((self.reader, self.graph))
    }

    fn field(&mut self, name: &[u8], kind: archive::ValueType) -> Result<archive::Value<'a>> {
        let p = self.reader.position();
        let entry = self.reader.read(kind)?;
        if entry.name.is_some_and(|actual| actual != name) {
            return Err(at(ErrorKind::InvalidData, p, "object field name"));
        }
        Ok(entry.value)
    }
    fn string(&mut self, name: &[u8]) -> Result<Vec<u8>> {
        let p = self.reader.position();
        let limit = self.limits.copied_bytes;
        let value = self.field(name, archive::ValueType::String)?;
        let archive::Value::String(v) = value else {
            unreachable!()
        };
        copy(&v, &mut self.copied, limit, p)
    }
    fn boolean(&mut self, name: &[u8]) -> Result<bool> {
        match self.field(name, archive::ValueType::Bool)? {
            archive::Value::Bool(v) => Ok(v),
            _ => unreachable!(),
        }
    }
    fn open(&mut self, mut h: archive::ObjectHeader<'a>, mut p: usize) -> Result<Link> {
        struct Parent {
            link: Link,
            remaining: usize,
            next: usize,
            edge_pos: usize,
            phase: DefinitionPhase,
        }
        let mut parents: Vec<Parent> = Vec::new();
        'record: loop {
            let (mut link, mut children, definition) = self.record(h, p, parents.len() + 1)?;
            let mut phase = if definition {
                DefinitionPhase::new(self.graph.get(link.target.expect("definition"))?)
            } else {
                DefinitionPhase::Vob(vob::Phase::Base)
            };
            if definition && children == 0 {
                children = self.resume(link.target.expect("definition"), &mut phase)?;
            }
            if children != 0 {
                p = self.reader.position();
                depth(parents.len() + 2, self.limits, Some(p))?;
                reserve(&mut parents, 1)
                    .map_err(|_| at(ErrorKind::LimitExceeded, p, "object traversal frames"))?;
                parents.push(Parent {
                    link,
                    remaining: children,
                    next: 0,
                    edge_pos: p,
                    phase,
                });
                h = self
                    .reader
                    .begin_object()?
                    .ok_or_else(|| at(ErrorKind::InvalidData, p, "expected object record"))?;
                continue;
            }
            self.end()?;
            if definition {
                self.complete_definition(link.target.expect("definition"))?;
            }
            loop {
                let Some(parent) = parents.last_mut() else {
                    return Ok(link);
                };
                let id = parent
                    .link
                    .target
                    .expect("definition registered before children");
                let npc_phase = if let DefinitionPhase::Vob(vob::Phase::Npc(p)) = &parent.phase {
                    Some(*p)
                } else {
                    None
                };
                let mut item_key = Some(false);
                if let Some(phase) = npc_phase {
                    self.graph
                        .npc_edge(&link, phase.edge())
                        .map_err(|e| at(e.kind, parent.edge_pos, e.context))?;
                    if let vob::npc_codec::NpcPhase::Item { index, .. } = phase {
                        if self.reader.header().save_game
                            && link.target.is_some_and(|id| {
                                !matches!(self.definitions[id.slot], DefinitionState::Complete)
                            })
                        {
                            item_key = match self.definitions[link.target.expect("Item").slot] {
                                DefinitionState::SavedBinaryItem(flags) => Some(flags & 0x200 != 0),
                                _ => None,
                            };
                            reserve(&mut self.deferred_keys, 1)?;
                            self.deferred_keys.push((id, index, self.reader.position()));
                        } else {
                            item_key = Some(self.graph.item_key(&link)?);
                        }
                    }
                } else if let DefinitionPhase::World(phase) = &parent.phase {
                    self.graph
                        .world_edge(&link, world::codec::edge(phase, self.graph.get(id)?))
                        .map_err(|e| at(e.kind, parent.edge_pos, e.context))?;
                } else {
                    self.graph
                        .child_edge(self.graph.get(id)?, parent.next, &link)
                        .map_err(|e| at(e.kind, parent.edge_pos, e.context))?;
                }
                match self.graph.get_mut(id)? {
                    node @ (ObjectMut::World(_) | ObjectMut::WayNet(_)) => {
                        let DefinitionPhase::World(phase) = &mut parent.phase else {
                            unreachable!()
                        };
                        world::codec::append(node, phase, link)?;
                    }
                    ObjectMut::Vob(mut v) if npc_phase.is_some() => {
                        let DefinitionPhase::Vob(vob::Phase::Npc(phase)) = &mut parent.phase else {
                            unreachable!()
                        };
                        vob::npc_codec::append(
                            v.kind.npc_mut().expect("NPC phase"),
                            phase,
                            link,
                            item_key,
                            parent.edge_pos,
                        )?;
                    }
                    ObjectMut::Vob(mut v)
                        if matches!(v.kind, vob::VobKindMut::Camera(_))
                            && parent.next >= v.base_child_count() =>
                    {
                        let index = parent.next - v.base_child_count();
                        let DefinitionPhase::Vob(vob::Phase::Camera { positions }) = &parent.phase
                        else {
                            unreachable!("camera occurrence phase")
                        };
                        let vob::VobKindMut::Camera(c) = &mut v.kind else {
                            unreachable!()
                        };
                        let links = if index < *positions {
                            &mut c.positions
                        } else {
                            &mut c.targets
                        };
                        reserve(links, 1).map_err(|_| {
                            at(
                                ErrorKind::LimitExceeded,
                                parent.edge_pos,
                                "camera link allocation",
                            )
                        })?;
                        links.push(link);
                    }
                    ObjectMut::CutscenePlayer(v) => {
                        reserve(&mut v.playlists, 1)
                            .map_err(|e| at(e.kind, parent.edge_pos, e.context))?;
                        v.playlists.push(link);
                    }
                    ObjectMut::CutsceneManager(v) => {
                        reserve(&mut v.pool_items, 1)
                            .map_err(|e| at(e.kind, parent.edge_pos, e.context))?;
                        v.pool_items.push(link);
                    }
                    ObjectMut::Library(v) => {
                        reserve(&mut v.blocks, 1).map_err(|_| {
                            at(
                                ErrorKind::LimitExceeded,
                                parent.edge_pos,
                                "library link allocation",
                            )
                        })?;
                        v.blocks.push(link);
                    }
                    ObjectMut::Vob(mut v)
                        if matches!(
                            v.kind,
                            vob::VobKindMut::CodeMaster(_) | vob::VobKindMut::Container(_)
                        ) && parent.next >= v.base_child_count() =>
                    {
                        let links = v.kind.appended_links_mut().expect("save sequence");
                        reserve(links, 1).map_err(|_| {
                            at(
                                ErrorKind::LimitExceeded,
                                parent.edge_pos,
                                "counted object link allocation",
                            )
                        })?;
                        links.push(link);
                    }
                    mut node => *node.child_mut(parent.next).expect("declared child slot") = link,
                }
                parent.next += 1;
                parent.remaining -= 1;
                if parent.remaining == 0 {
                    parent.remaining = self.resume(id, &mut parent.phase)?;
                }
                if parent.remaining != 0 {
                    p = self.reader.position();
                    parent.edge_pos = p;
                    depth(parents.len() + 1, self.limits, Some(p))?;
                    h = self
                        .reader
                        .begin_object()?
                        .ok_or_else(|| at(ErrorKind::InvalidData, p, "expected child object"))?;
                    continue 'record;
                }
                let parent = parents.pop().expect("current parent");
                self.end()?;
                self.complete_definition(id)?;
                link = parent.link;
            }
        }
    }
    fn complete_definition(&mut self, id: ObjectId) -> Result<()> {
        if let DefinitionState::SavedBinaryItem(expected) = self.definitions[id.slot] {
            let actual = match self.graph.get(id)? {
                ObjectRef::Vob(v) => match &v.kind {
                    vob::VobKindRef::Item(item) => item.state.map(|state| state.flags),
                    _ => None,
                },
                _ => None,
            };
            if actual != Some(expected) {
                return Err(at(
                    ErrorKind::InvalidData,
                    self.reader.position(),
                    "BINARY Item flags disagree with fixed suffix",
                ));
            }
        }
        self.definitions[id.slot] = DefinitionState::Complete;
        Ok(())
    }
    fn resume(&mut self, id: ObjectId, phase: &mut DefinitionPhase) -> Result<usize> {
        let p = self.reader.position();
        let remaining = match phase {
            DefinitionPhase::Atomic => {
                let synchronized = if self.reader.has_object_end()? {
                    None
                } else {
                    if self.reader.header().format == archive::Format::Binary
                        && self.reader.remaining_bytes() != 1
                    {
                        return Err(at(ErrorKind::Unsupported, p, "extra atomic block payload"));
                    }
                    Some(self.boolean(b"synchronized")?)
                };
                let ObjectMut::Atomic(block) = self.graph.get_mut(id)? else {
                    unreachable!("atomic phase")
                };
                block.synchronized = synchronized;
                0
            }
            DefinitionPhase::World(phase) => world::codec::read_phase(
                &mut self.reader,
                self.graph.get_mut(id)?,
                phase,
                self.game,
                world::codec::ReadBudget {
                    copied: &mut self.copied,
                    geometry: &mut self.geometry,
                    sections: &mut self.world_sections,
                    limits: self.limits,
                    links_left: self.limits.links.saturating_sub(self.links),
                },
            )?,
            DefinitionPhase::Vob(phase) => vob::read_phase(
                &mut self.reader,
                self.graph.get_mut(id)?,
                phase,
                vob::ReadBudget {
                    copied: &mut self.copied,
                    sequences: &mut self.sequences,
                    limits: self.limits,
                    links_left: self.limits.links.saturating_sub(self.links),
                },
            )?,
            DefinitionPhase::Context(phase) => cutscene_state::read_phase(
                &mut self.reader,
                self.graph.get_mut(id)?,
                phase,
                &mut self.copied,
                self.limits.copied_bytes,
            )?,
        };
        if remaining > self.limits.links.saturating_sub(self.links) {
            return Err(at(
                ErrorKind::LimitExceeded,
                p,
                "object pending child count",
            ));
        }
        Ok(remaining)
    }
    fn end(&mut self) -> Result<()> {
        if !self.reader.end_object()? {
            return Err(at(
                ErrorKind::Unsupported,
                self.reader.position(),
                "extra object payload",
            ));
        }
        Ok(())
    }
    /// Register definitions and read their fields preceding a possible child.
    /// Returning a positive count leaves children and the enclosing end pending.
    fn record(
        &mut self,
        h: archive::ObjectHeader<'_>,
        p: usize,
        level: usize,
    ) -> Result<(Link, usize, bool)> {
        depth(level, self.limits, Some(p))?;
        link_count(&mut self.links, 1, self.limits.links).map_err(|e| at(e.kind, p, e.context))?;
        if !h.is_empty() && !h.is_reference() {
            if self.ids.contains_key(&h.index) {
                return Err(at(ErrorKind::InvalidData, p, "duplicate object index"));
            }
            if self.graph.len() >= self.limits.nodes {
                return Err(at(ErrorKind::LimitExceeded, p, "object nodes"));
            }
        }
        let alias = canonical_alias(h.class_name, h.version, self.game);
        let parsed_version = alias.unwrap_or(h.version);
        let extra = vob::check_object(h.class_name, parsed_version, self.game, p)?;
        if h.class_name == material::CLASS {
            material::check_version(h.version, self.game, p)?;
        } else if h.class_name == b"oCWorld:zCWorld" {
            if h.version != 64513 {
                return Err(at(ErrorKind::Unsupported, p, "world version"));
            }
        } else if !extra && parsed_version != 0 {
            return Err(at(ErrorKind::Unsupported, p, "object record version"));
        }
        let name = copy(h.object_name, &mut self.copied, self.limits.copied_bytes, p)?;
        let mut children = 0;
        let target = if h.is_empty() {
            None
        } else if h.is_reference() {
            Some(*self.ids.get(&h.index).ok_or_else(|| {
                at(
                    ErrorKind::UnresolvedReference,
                    p,
                    "unresolved or forward object reference",
                )
            })?)
        } else {
            if self.ids.contains_key(&h.index) {
                return Err(at(ErrorKind::InvalidData, p, "duplicate object index"));
            }
            if self.graph.len() >= self.limits.nodes {
                return Err(at(ErrorKind::LimitExceeded, p, "object nodes"));
            }
            self.ids
                .try_reserve(1)
                .map_err(|_| at(ErrorKind::LimitExceeded, p, "object identity table"))?;
            reserve(&mut self.definitions, 1)?;
            let definition = if h.class_name == b"oCItem:zCVob"
                && h.version == 0
                && self.reader.header().save_game
                && self.reader.header().format == archive::Format::Binary
            {
                // Both supported games end the exact saved Item schema with
                // amount:i32, flags:i32. Use the enclosing length only for this
                // known suffix; ordinary parsing and end validation remain required.
                let suffix = self.reader.binary_object_tail(8)?;
                DefinitionState::SavedBinaryItem(i32::from_le_bytes(
                    suffix[4..].try_into().expect("fixed flags suffix"),
                ))
            } else {
                DefinitionState::Incomplete
            };
            if alias.is_some() {
                self.graph
                    .versions
                    .try_reserve(1)
                    .map_err(|_| at(ErrorKind::LimitExceeded, p, "object version metadata"))?;
            }
            // Materials have no child links, so parse before registering the ID.
            let material = if h.class_name == material::CLASS {
                Some(Material::read_fields(
                    &mut self.reader,
                    self.game,
                    h.version,
                    &mut self.copied,
                    self.limits.copied_bytes,
                )?)
            } else {
                None
            };
            let id = self
                .graph
                .register(
                    h.class_name,
                    parsed_version,
                    self.game,
                    self.reader.header().save_game,
                    material,
                )
                .map_err(|e| at(e.kind, p, e.context))?;
            let class = self.graph.get(id)?.class();
            self.definitions.push(definition);
            if alias.is_some() {
                self.graph.versions.insert(
                    id.slot,
                    RetainedVersion {
                        class,
                        game: self.game,
                        version: h.version,
                    },
                );
            }
            self.ids.insert(h.index, id); // reserve identity before fields, including cycles
            let kind = self.graph.get(id)?.kind();
            match kind {
                Kind::World | Kind::WayNet | Kind::WayPoint | Kind::SkyController => {
                    world::codec::read_prefix(
                        &mut self.reader,
                        self.graph.get_mut(id)?,
                        world::codec::ReadBudget {
                            copied: &mut self.copied,
                            geometry: &mut self.geometry,
                            sections: &mut self.world_sections,
                            limits: self.limits,
                            links_left: self.limits.links.saturating_sub(self.links),
                        },
                    )?;
                }
                Kind::Vob | Kind::Visual | Kind::EventManager | Kind::AiMove | Kind::AiHuman => {
                    vob::read_prefix(
                        &mut self.reader,
                        self.game,
                        &mut self.copied,
                        self.limits.copied_bytes,
                        self.graph.get_mut(id)?,
                    )?;
                    let value = self.graph.get(id)?;
                    children = match value {
                        ObjectRef::Vob(v) => v.base_child_count(),
                        _ => value.child_count(),
                    };
                    if children > self.limits.links.saturating_sub(self.links) {
                        return Err(at(ErrorKind::LimitExceeded, p, "VOB child count"));
                    }
                }
                Kind::CutsceneProps
                | Kind::Cutscene
                | Kind::CutsceneContext
                | Kind::CutscenePlayer
                | Kind::CutscenePoolItem
                | Kind::CutsceneManager => {
                    children = cutscene_state::read_prefix(
                        &mut self.reader,
                        self.graph.get_mut(id)?,
                        &mut self.copied,
                        self.limits.copied_bytes,
                        self.limits.links.saturating_sub(self.links),
                    )?;
                }
                Kind::NpcTalent => {
                    let read = |this: &mut Self, name: &[u8]| -> Result<i32> {
                        let archive::Value::Int(v) = this.field(name, archive::ValueType::Int)?
                        else {
                            unreachable!()
                        };
                        Ok(v)
                    };
                    let value = vob::NpcTalent {
                        talent: read(self, b"talent")?,
                        value: read(self, b"value")?,
                        skill: read(self, b"skill")?,
                    };
                    let ObjectMut::NpcTalent(target) = self.graph.get_mut(id)? else {
                        unreachable!()
                    };
                    *target = value;
                }
                Kind::Library => {
                    let p = self.reader.position();
                    let n = match self.field(b"NumOfItems", archive::ValueType::Int)? {
                        archive::Value::Int(v) => v,
                        _ => unreachable!(),
                    };
                    children = usize::try_from(n).map_err(|_| {
                        at(ErrorKind::InvalidData, p, "negative library item count")
                    })?;
                    if children > self.limits.links.saturating_sub(self.links) {
                        return Err(at(ErrorKind::LimitExceeded, p, "library item count"));
                    }
                }
                Kind::SaveMetadata => {
                    let value = savegame::codec::read_metadata(
                        &mut self.reader,
                        self.game,
                        &mut self.copied,
                        self.limits.copied_bytes,
                    )?;
                    let ObjectMut::SaveMetadata(target) = self.graph.get_mut(id)? else {
                        unreachable!()
                    };
                    *target = value;
                }
                Kind::Material => {}
                Kind::Message => {
                    let subtype = match self.field(b"subType", archive::ValueType::Enum)? {
                        archive::Value::Enum(v) => v,
                        _ => unreachable!(),
                    };
                    let state = if self.reader.header().save_game {
                        Some(MessageState {
                            high_priority: self.boolean(b"highPriority")?,
                            in_use: self.boolean(b"inUse")?,
                            deleted: self.boolean(b"deleted")?,
                        })
                    } else {
                        None
                    };
                    let value = ConversationMessage {
                        subtype,
                        text: self.string(b"text")?,
                        audio_name: self.string(b"name")?,
                        state,
                    };
                    let ObjectMut::Message(target) = self.graph.get_mut(id)? else {
                        unreachable!()
                    };
                    *target = value;
                }
                Kind::Atomic => {
                    children = 1;
                }
                Kind::Block => {
                    let name = self.string(b"blockName")?;
                    let count_pos = self.reader.position();
                    let count = match self.field(b"numOfBlocks", archive::ValueType::Int)? {
                        archive::Value::Int(v) => v,
                        _ => unreachable!(),
                    };
                    if count != 1 {
                        return Err(at(
                            ErrorKind::Unsupported,
                            count_pos,
                            "cutscene block count must be one",
                        ));
                    }
                    let sub_block0 = match self.field(b"subBlock0", archive::ValueType::Float)? {
                        archive::Value::Float(v) => v,
                        _ => unreachable!(),
                    };
                    children = 1;
                    let value = CutsceneBlock {
                        name,
                        sub_block0,
                        child: Link::new(Vec::new(), None),
                    };
                    let ObjectMut::Block(target) = self.graph.get_mut(id)? else {
                        unreachable!()
                    };
                    *target = value;
                }
            };
            Some(id)
        };
        Ok((
            Link { name, target },
            children,
            !h.is_empty() && !h.is_reference(),
        ))
    }
}
pub(crate) struct Encoder<'a> {
    pub(crate) output: archive::Writer,
    graph: &'a Graph,
    game: GameVersion,
    ids: HashMap<ObjectId, u32>,
    limits: Limits,
}
impl<'a> Encoder<'a> {
    pub(crate) fn new(
        metadata: &Metadata,
        game: GameVersion,
        graph: &'a Graph,
        limits: Limits,
    ) -> Result<Self> {
        Ok(Self {
            output: archive::Writer::new(metadata.as_header(), limits.archive)?,
            graph,
            game,
            ids: HashMap::new(),
            limits,
        })
    }
    pub(crate) fn finish(self) -> Result<Vec<u8>> {
        self.output.finish()
    }

    pub(crate) fn link(&mut self, root: &Link) -> Result<()> {
        enum Task<'a> {
            Enter(&'a Link, usize),
            Continue {
                id: ObjectId,
                next: usize,
                remaining: usize,
                phase: DefinitionPhase,
                level: usize,
            },
        }
        let mut tasks = Vec::new();
        reserve(&mut tasks, 1)?;
        tasks.push(Task::Enter(root, 1));
        while let Some(task) = tasks.pop() {
            match task {
                Task::Enter(link, level) => {
                    if let Some(id) = self.record(link, level)? {
                        let node = self.graph.get(id)?;
                        let remaining = match node {
                            ObjectRef::World(_)
                            | ObjectRef::WayNet(_)
                            | ObjectRef::WayPoint(_)
                            | ObjectRef::SkyController(_) => 0,
                            ObjectRef::Vob(v) => v.base_child_count(),
                            ObjectRef::CutsceneContext(_) => 1,
                            _ => node.child_count(),
                        };
                        reserve(&mut tasks, 1)?;
                        tasks.push(Task::Continue {
                            id,
                            next: 0,
                            remaining,
                            phase: DefinitionPhase::new(node),
                            level: level + 1,
                        });
                    }
                }
                Task::Continue {
                    id,
                    next,
                    mut remaining,
                    mut phase,
                    level,
                } => {
                    let node = self.graph.get(id)?;
                    if remaining == 0 {
                        remaining = match &mut phase {
                            DefinitionPhase::Atomic => {
                                let ObjectRef::Atomic(block) = node else {
                                    unreachable!("atomic phase")
                                };
                                if let Some(value) = block.synchronized {
                                    self.output
                                        .write(b"synchronized", archive::Value::Bool(value))?;
                                }
                                0
                            }
                            DefinitionPhase::World(phase) => world::codec::write_phase(
                                &mut self.output,
                                node,
                                phase,
                                self.limits,
                            )?,
                            DefinitionPhase::Vob(phase) => vob::write_phase(
                                &mut self.output,
                                node,
                                phase,
                                self.limits.archive,
                            )?,
                            DefinitionPhase::Context(phase) => {
                                cutscene_state::write_phase(&mut self.output, node, phase)?
                            }
                        };
                    }
                    if remaining != 0 {
                        let child = if let DefinitionPhase::World(phase) = &mut phase {
                            world::codec::write_link(node, phase)
                        } else if let (
                            ObjectRef::Vob(v),
                            DefinitionPhase::Vob(vob::Phase::Npc(p)),
                        ) = (node, &phase)
                        {
                            let n = v.kind.npc().expect("NPC phase");
                            vob::npc_codec::phase_link(n, *p, next - v.base_child_count())
                        } else {
                            node.child(next).expect("validated phase child")
                        };
                        reserve(&mut tasks, 2)?;
                        tasks.push(Task::Continue {
                            id,
                            next: next + 1,
                            remaining: remaining - 1,
                            phase,
                            level,
                        });
                        tasks.push(Task::Enter(child, level));
                    } else {
                        self.output.end_object()?;
                    }
                }
            }
        }
        Ok(())
    }
    /// Start a definition and return its identity for child/suffix continuation.
    /// Frames retain only the current depth, never all pending siblings.
    fn record(&mut self, link: &Link, level: usize) -> Result<Option<ObjectId>> {
        depth(level, self.limits, None)?;
        let Some(id) = link.target else {
            self.output.begin_object(&link.name, b"%", 0)?;
            self.output.end_object()?;
            return Ok(None);
        };
        if let Some(index) = self.ids.get(&id) {
            self.output.write_reference(&link.name, *index)?;
            return Ok(None);
        }
        let node = self.graph.get(id)?;
        self.ids
            .try_reserve(1)
            .map_err(|_| bad(ErrorKind::LimitExceeded, "object writer identity table"))?;
        let version = self.graph.record_version(id, self.game)?;
        let index = self
            .output
            .begin_object(&link.name, node.class(), version)?;
        self.ids.insert(id, index);
        use archive::Value as V;
        match node {
            ObjectRef::World(_)
            | ObjectRef::WayNet(_)
            | ObjectRef::WayPoint(_)
            | ObjectRef::SkyController(_) => world::codec::write_prefix(&mut self.output, node)?,
            ObjectRef::Vob(_)
            | ObjectRef::Visual(_)
            | ObjectRef::EventManager(_)
            | ObjectRef::AiMove(_)
            | ObjectRef::AiHuman(_) => {
                vob::write_prefix(&mut self.output, node, self.limits.archive)?
            }
            ObjectRef::CutsceneProps(_)
            | ObjectRef::Cutscene(_)
            | ObjectRef::CutsceneContext(_)
            | ObjectRef::CutscenePlayer(_)
            | ObjectRef::CutscenePoolItem(_)
            | ObjectRef::CutsceneManager(_) => {
                cutscene_state::write_prefix(&mut self.output, node)?
            }
            ObjectRef::NpcTalent(v) => {
                self.output.write(b"talent", V::Int(v.talent))?;
                self.output.write(b"value", V::Int(v.value))?;
                self.output.write(b"skill", V::Int(v.skill))?;
            }
            ObjectRef::Library(v) => {
                let count = i32::try_from(v.blocks.len())
                    .map_err(|_| bad(ErrorKind::LimitExceeded, "library item count"))?;
                self.output.write(b"NumOfItems", V::Int(count))?;
            }
            ObjectRef::SaveMetadata(v) => savegame::codec::write_metadata(v, &mut self.output)?,
            ObjectRef::Material(v) => v.write_fields(&mut self.output, self.game)?,
            ObjectRef::Message(v) => {
                self.output.write(b"subType", V::Enum(v.subtype))?;
                if let Some(s) = v.state {
                    self.output
                        .write(b"highPriority", V::Bool(s.high_priority))?;
                    self.output.write(b"inUse", V::Bool(s.in_use))?;
                    self.output.write(b"deleted", V::Bool(s.deleted))?;
                }
                self.output
                    .write(b"text", V::String(v.text.as_slice().into()))?;
                self.output
                    .write(b"name", V::String(v.audio_name.as_slice().into()))?;
            }
            ObjectRef::Atomic(_) => {}
            ObjectRef::Block(v) => {
                self.output
                    .write(b"blockName", V::String(v.name.as_slice().into()))?;
                self.output.write(b"numOfBlocks", V::Int(1))?;
                self.output.write(b"subBlock0", V::Float(v.sub_block0))?;
            }
        }
        Ok(Some(id))
    }
}

impl<'a> ObjectRef<'a> {
    fn kind(self) -> Kind {
        match self {
            Self::Message(_) => Kind::Message,
            Self::Atomic(_) => Kind::Atomic,
            Self::Block(_) => Kind::Block,
            Self::Material(_) => Kind::Material,
            Self::Library(_) => Kind::Library,
            Self::Vob(_) => Kind::Vob,
            Self::Visual(_) => Kind::Visual,
            Self::EventManager(_) => Kind::EventManager,
            Self::AiMove(_) => Kind::AiMove,
            Self::AiHuman(_) => Kind::AiHuman,
            Self::CutsceneProps(_) => Kind::CutsceneProps,
            Self::Cutscene(_) => Kind::Cutscene,
            Self::CutsceneContext(_) => Kind::CutsceneContext,
            Self::CutscenePlayer(_) => Kind::CutscenePlayer,
            Self::CutscenePoolItem(_) => Kind::CutscenePoolItem,
            Self::CutsceneManager(_) => Kind::CutsceneManager,
            Self::NpcTalent(_) => Kind::NpcTalent,
            Self::World(_) => Kind::World,
            Self::WayNet(_) => Kind::WayNet,
            Self::WayPoint(_) => Kind::WayPoint,
            Self::SkyController(_) => Kind::SkyController,
            Self::SaveMetadata(_) => Kind::SaveMetadata,
        }
    }
}

impl<'a> ObjectRef<'a> {
    fn class(self) -> &'static [u8] {
        match self {
            Self::Message(_) => MESSAGE,
            Self::Atomic(_) => ATOMIC,
            Self::Block(_) => BLOCK,
            Self::Material(_) => material::CLASS,
            Self::Library(_) => LIBRARY,
            Self::Vob(v) => v.kind.class_name(),
            Self::Visual(v) => v.kind.class_name(),
            Self::EventManager(_) => vob::EVENT_MANAGER,
            Self::AiMove(_) => vob::AI_MOVE,
            Self::AiHuman(_) => vob::AI_HUMAN,
            Self::CutsceneProps(_) => b"zCCSProps",
            Self::Cutscene(_) => b"zCCutscene:zCCSBlock",
            Self::CutsceneContext(_) => b"zCCSCutsceneContext:zCCutscene:zCCSBlock",
            Self::CutscenePlayer(_) => b"oCCSPlayer:zCCSPlayer",
            Self::CutscenePoolItem(_) => b"zCCSPoolItem",
            Self::CutsceneManager(_) => b"oCCSManager:zCCSManager",
            Self::NpcTalent(_) => b"oCNpcTalent",
            Self::World(_) => b"oCWorld:zCWorld",
            Self::WayNet(_) => b"zCWayNet",
            Self::WayPoint(_) => b"zCWaypoint",
            Self::SkyController(_) => b"zCSkyControler_Outdoor",
            Self::SaveMetadata(_) => b"oCSavegameInfo",
        }
    }
}

impl<'a> ObjectRef<'a> {
    fn child_count(self) -> usize {
        match self {
            Self::Atomic(_) | Self::Block(_) | Self::EventManager(_) | Self::AiHuman(_) => 1,
            Self::AiMove(_) => 2,
            Self::Cutscene(_) => 1,
            Self::CutsceneContext(_) => 6,
            Self::CutscenePlayer(v) => v.playlists.len(),
            Self::CutsceneManager(v) => v.pool_items.len(),
            Self::Library(v) => v.blocks.len(),
            Self::Vob(v) => v.links().count(),
            Self::World(v) => v.links().count(),
            Self::WayNet(v) => v.links().count(),
            _ => 0,
        }
    }
}

impl<'a> ObjectRef<'a> {
    fn child(self, index: usize) -> Option<&'a Link> {
        match self {
            Self::Atomic(v) => (index == 0).then_some(&v.message),
            Self::Block(v) | Self::Cutscene(v) => (index == 0).then_some(&v.child),
            Self::CutsceneContext(v) => v.links().get(index).copied(),
            Self::CutscenePlayer(v) => v.playlists.get(index),
            Self::CutsceneManager(v) => v.pool_items.get(index),
            Self::Library(v) => v.blocks.get(index),
            Self::Vob(v) => v.links().nth(index),
            Self::World(v) => v.links().nth(index),
            Self::WayNet(v) => v.links().nth(index),
            Self::EventManager(v) => (index == 0).then_some(&v.cutscene),
            Self::AiMove(v) => match index {
                0 => Some(&v.vob),
                1 => Some(&v.owner),
                _ => None,
            },
            Self::AiHuman(v) => (index == 0).then_some(&v.npc),
            _ => None,
        }
    }
}

impl ObjectMut<'_> {
    fn child_mut(&mut self, index: usize) -> Option<&mut Link> {
        match self {
            Self::Atomic(v) => (index == 0).then_some(&mut v.message),
            Self::Block(v) | Self::Cutscene(v) => (index == 0).then_some(&mut v.child),
            Self::CutsceneContext(v) => v.links_mut().into_iter().nth(index),
            Self::CutscenePlayer(v) => v.playlists.get_mut(index),
            Self::CutsceneManager(v) => v.pool_items.get_mut(index),
            Self::Library(v) => v.blocks.get_mut(index),
            Self::Vob(v) => v.links_mut().nth(index),
            Self::World(v) => v.links_mut().nth(index),
            Self::WayNet(v) => v.links_mut().nth(index),
            Self::EventManager(v) => (index == 0).then_some(&mut v.cutscene),
            Self::AiMove(v) => match index {
                0 => Some(&mut v.vob),
                1 => Some(&mut v.owner),
                _ => None,
            },
            Self::AiHuman(v) => (index == 0).then_some(&mut v.npc),
            _ => None,
        }
    }
}
