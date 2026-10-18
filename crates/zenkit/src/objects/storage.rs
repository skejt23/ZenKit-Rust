//! Typed payload ownership; construction enums are never retained.
use super::arena::{Arena, Prepared, Replacement, Stored};
use super::*;
use crate::vob::{
    VobMut, VobRef,
    storage::{PreparedVob, VobPools},
};

macro_rules! object_pools {
    ($($name:ident($ty:ty)),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub enum ObjectRef<'a> { $($name(&'a $ty),)* Vob(VobRef<'a>) }
        #[derive(Debug)]
        pub enum ObjectMut<'a> { $($name(&'a mut $ty),)* Vob(VobMut<'a>) }
        #[derive(Clone, Copy)]
        pub(super) enum Location { $($name(u32),)* Vob(u32) }
        #[derive(Default)]
        #[allow(non_snake_case)]
        pub(super) struct Pools { $($name: Arena<Stored<$ty>>,)* vobs: VobPools }
        pub(super) enum PreparedObject { $($name(Prepared<Stored<$ty>>),)* Vob(PreparedVob) }
        impl Object {
            pub fn as_ref(&self) -> ObjectRef<'_> {
                match self { $(Self::$name(v) => ObjectRef::$name(v),)* Self::Vob(v) => ObjectRef::Vob(v.as_ref()) }
            }
            pub fn as_mut(&mut self) -> ObjectMut<'_> {
                match self { $(Self::$name(v) => ObjectMut::$name(v),)* Self::Vob(v) => ObjectMut::Vob(v.as_mut()) }
            }
        }
        impl ObjectMut<'_> {
            pub fn as_ref(&self) -> ObjectRef<'_> {
                match self { $(Self::$name(v) => ObjectRef::$name(v),)* Self::Vob(v) => ObjectRef::Vob(v.as_ref()) }
            }
            pub fn reborrow(&mut self) -> ObjectMut<'_> {
                match self { $(Self::$name(v) => ObjectMut::$name(v),)* Self::Vob(v) => ObjectMut::Vob(v.reborrow()) }
            }
        }
        impl ObjectRef<'_> {
            /// Explicitly clone a payload into a construction value. Handles in
            /// its links still belong to the original graph; no graph is cloned.
            pub fn to_owned(self) -> Object {
                match self { $(Self::$name(v) => Object::$name(v.clone()),)* Self::Vob(v) => Object::Vob(v.to_owned()) }
            }
        }
        #[allow(non_snake_case)]
        impl Pools {
            pub(super) fn replace_vob_kind(&mut self, location: Location, value: vob::VobKind) -> Result<vob::VobKind> {
                let Location::Vob(index) = location else { return Err(bad(ErrorKind::WrongObjectType, "replace VOB subtype")); };
                self.vobs.replace_kind(index as usize, value)
            }
            $(pub(super) fn $name(&mut self, value: $ty, owner: usize) -> Result<Location> {
                u32::try_from(self.$name.len()).map_err(|_| bad(ErrorKind::LimitExceeded, "typed payload index"))?;
                let prepared = self.$name.prepare()?;
                Ok(Location::$name(self.$name.commit(prepared, Stored { owner, value }) as u32))
            })*
            pub(super) fn prepare(&mut self, value: &Object) -> Result<PreparedObject> {
                Ok(match value {
                    $(Object::$name(_) => {
                        u32::try_from(self.$name.len()).map_err(|_| bad(ErrorKind::LimitExceeded, "typed payload index"))?;
                        PreparedObject::$name(self.$name.prepare()?)
                    },)*
                    Object::Vob(v) => PreparedObject::Vob(self.vobs.prepare(v)?),
                })
            }
            pub(super) fn commit(&mut self, prepared: PreparedObject, value: Object, owner: usize) -> Location {
                match (prepared, value) {
                    $((PreparedObject::$name(p), Object::$name(value)) => Location::$name(self.$name.commit(p, Stored { owner, value }) as u32),)*
                    (PreparedObject::Vob(p), Object::Vob(value)) => Location::Vob(self.vobs.commit(p, value, owner) as u32),
                    _ => unreachable!("prepared payload schema"),
                }
            }
            pub(super) fn get(&self, location: Location) -> ObjectRef<'_> {
                match location { $(Location::$name(i) => ObjectRef::$name(&self.$name.get(i as usize).value),)* Location::Vob(i) => ObjectRef::Vob(self.vobs.get(i as usize)) }
            }
            pub(super) fn get_mut(&mut self, location: Location) -> ObjectMut<'_> {
                match location { $(Location::$name(i) => ObjectMut::$name(&mut self.$name.get_mut(i as usize).value),)* Location::Vob(i) => ObjectMut::Vob(self.vobs.get_mut(i as usize)) }
            }
            pub(super) fn replace_same(&mut self, location: Location, value: Object) -> Result<Replacement<Object>> {
                Ok(match (location, value) {
                    $((Location::$name(i), Object::$name(value)) => Replacement::Replaced(Object::$name(std::mem::replace(&mut self.$name.get_mut(i as usize).value, value)), ()),)*
                    (Location::Vob(i), Object::Vob(v)) => Replacement::Replaced(Object::Vob(self.vobs.replace(i as usize, v)?), ()),
                    (_, value) => Replacement::Different(value),
                })
            }
            pub(super) fn remove(&mut self, location: Location) -> (Object, Option<(usize, Location)>) {
                match location {
                    $(Location::$name(i) => {
                        let value = self.$name.swap_remove(i as usize).value;
                        let moved = ((i as usize) < self.$name.len()).then(|| (self.$name.get(i as usize).owner, Location::$name(i)));
                        (Object::$name(value), moved)
                    },)*
                    Location::Vob(i) => { let (value, moved) = self.vobs.remove(i as usize); (Object::Vob(value), moved.map(|owner| (owner, Location::Vob(i)))) }
                }
            }
        }
    }
}
object_pools! {
    Message(ConversationMessage),
    Atomic(AtomicBlock),
    Block(CutsceneBlock),
    Material(Material),
    Library(Library),
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

impl Pools {
    pub(super) fn register(
        &mut self,
        class: &[u8],
        version: u16,
        game: GameVersion,
        save: bool,
        owner: usize,
        material: Option<Material>,
    ) -> Result<Location> {
        if let Some(schema) = vob::storage::Schema::from_class(class) {
            return Ok(Location::Vob(
                self.vobs.register(schema, game, version, owner)? as u32,
            ));
        }
        match class {
            MESSAGE => self.Message(ConversationMessage::default(), owner),
            ATOMIC => self.Atomic(
                AtomicBlock {
                    message: Link::new(Vec::new(), None),
                    synchronized: None,
                },
                owner,
            ),
            BLOCK => self.Block(
                CutsceneBlock {
                    name: Vec::new(),
                    sub_block0: 0.,
                    child: Link::new(Vec::new(), None),
                },
                owner,
            ),
            material::CLASS => self.Material(material.expect("parsed leaf material"), owner),
            LIBRARY => self.Library(Library::default(), owner),
            b"zCCSProps" => self.CutsceneProps(Props::default(), owner),
            b"zCCutscene:zCCSBlock" => self.Cutscene(
                CutsceneBlock {
                    name: Vec::new(),
                    sub_block0: 0.,
                    child: Link::new(Vec::new(), None),
                },
                owner,
            ),
            b"zCCSCutsceneContext:zCCutscene:zCCSBlock" => {
                self.CutsceneContext(Context::default(), owner)
            }
            b"oCCSPlayer:zCCSPlayer" => self.CutscenePlayer(Player::default(), owner),
            b"zCCSPoolItem" => self.CutscenePoolItem(PoolItem::default(), owner),
            b"oCCSManager:zCCSManager" => self.CutsceneManager(Manager::default(), owner),
            b"oCNpcTalent" => self.NpcTalent(vob::NpcTalent::default(), owner),
            b"oCWorld:zCWorld" => self.World(world::World::new(game, save), owner),
            b"zCWayNet" => self.WayNet(world::WayNet::default(), owner),
            b"zCWaypoint" => self.WayPoint(world::WayPoint::default(), owner),
            b"zCSkyControler_Outdoor" => self.SkyController(world::SkyController::new(game), owner),
            b"oCSavegameInfo" => self.SaveMetadata(savegame::SaveMetadata::new(game), owner),
            vob::EVENT_MANAGER => self.EventManager(
                EventManager {
                    cleared: false,
                    active: false,
                    cutscene: Link::new(Vec::new(), None),
                },
                owner,
            ),
            vob::AI_MOVE => self.AiMove(
                AiMove {
                    vob: Link::new(Vec::new(), None),
                    owner: Link::new(Vec::new(), None),
                },
                owner,
            ),
            vob::AI_HUMAN => self.AiHuman(
                AiHuman {
                    water_level: 0,
                    floor_y: 0.,
                    water_y: 0.,
                    ceil_y: 0.,
                    feet_y: 0.,
                    head_y: 0.,
                    fall_dist_y: 0.,
                    fall_start_y: 0.,
                    npc: Link::new(Vec::new(), None),
                    walk_mode: 0,
                    weapon_mode: 0,
                    weapon_mode_last: 0,
                    weapon_mode_select: 0,
                    change_weapon: false,
                    action_mode: 0,
                },
                owner,
            ),
            b"zCDecal" | b"zCMesh" | b"zCProgMeshProto" | b"zCParticleFX" | b"zCAICamera"
            | b"zCModel" | b"zCMorphMesh" => {
                let kind = match class {
                    b"zCDecal" => VisualKind::Decal(vob::Decal::new(game)),
                    b"zCMesh" => VisualKind::Mesh,
                    b"zCProgMeshProto" => VisualKind::ProgressiveMesh,
                    b"zCParticleFX" => VisualKind::ParticleFx,
                    b"zCAICamera" => VisualKind::Camera,
                    b"zCModel" => VisualKind::Model,
                    _ => VisualKind::MorphMesh,
                };
                self.Visual(Visual { version, kind }, owner)
            }
            _ => Err(bad(ErrorKind::Unsupported, "unknown object class")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vob::{NpcData, VobKind, VobKindRef, ZoneMusic};
    fn vob(kind: VobKind) -> Object {
        let mut value = Vob::new(GameVersion::Gothic2);
        value.kind = kind;
        Object::Vob(value)
    }
    fn talent(value: i32) -> Object {
        Object::NpcTalent(vob::NpcTalent {
            talent: value,
            value,
            skill: value,
        })
    }
    #[test]
    fn directory_is_compact_and_tiny_payloads_have_tiny_slots() {
        assert!(std::mem::size_of::<Location>() <= 16);
        assert!(std::mem::size_of::<Stored<vob::NpcTalent>>() <= 24);
        assert!(std::mem::size_of::<ObjectRef<'_>>() <= 64);
    }
    #[test]
    fn whole_object_replacement_repairs_moved_payload_owners() {
        let mut graph = Graph::new().unwrap();
        let first = graph.insert(talent(11)).unwrap();
        let second = graph.insert(talent(22)).unwrap();
        graph.replace(first, vob(VobKind::Base)).unwrap();
        assert!(matches!(graph.get(second).unwrap(), ObjectRef::NpcTalent(v) if v.value == 22));
        graph
            .replace(second, Object::WayPoint(world::WayPoint::default()))
            .unwrap();
        graph.replace(first, talent(33)).unwrap();
        assert!(matches!(graph.get(first).unwrap(), ObjectRef::NpcTalent(v) if v.value == 33));
        assert!(matches!(graph.get(second).unwrap(), ObjectRef::WayPoint(_)));
        assert_eq!(graph.len(), 2);
    }
    #[test]
    fn vob_record_and_subtype_compaction_preserve_every_identity() {
        let mut graph = Graph::new().unwrap();
        let ids: Vec<_> = (0..150)
            .map(|i| {
                let mut n = NpcData::new(GameVersion::Gothic2);
                n.guild = i;
                graph.insert(vob(VobKind::Npc(n))).unwrap()
            })
            .collect();
        for i in (0..ids.len()).step_by(3) {
            graph.replace(ids[i], talent(i as i32)).unwrap();
        }
        for i in (1..ids.len()).step_by(3) {
            graph.replace_vob_kind(ids[i], VobKind::Base).unwrap();
        }
        for (i, id) in ids.iter().enumerate() {
            match i % 3 {
                0 => assert!(
                    matches!(graph.get(*id).unwrap(), ObjectRef::NpcTalent(v) if v.value == i as i32)
                ),
                1 => assert!(
                    matches!(graph.get(*id).unwrap(), ObjectRef::Vob(v) if matches!(v.kind, VobKindRef::Base))
                ),
                _ => assert!(
                    matches!(graph.get(*id).unwrap(), ObjectRef::Vob(v) if matches!(v.kind, VobKindRef::Npc(n) if n.guild == i as i32))
                ),
            }
        }
        assert_eq!(graph.len(), ids.len());
    }
    #[test]
    fn shared_subtype_pool_keeps_the_wire_class_when_moving_or_replacing() {
        let mut graph = Graph::new().unwrap();
        let a = graph
            .insert(vob(VobKind::ZoneMusic(ZoneMusic::default())))
            .unwrap();
        let b = graph
            .insert(vob(VobKind::ZoneMusicDefault(ZoneMusic::default())))
            .unwrap();
        graph.replace_vob_kind(a, VobKind::Base).unwrap();
        assert!(
            matches!(graph.get(b).unwrap(), ObjectRef::Vob(v) if matches!(v.kind, VobKindRef::ZoneMusicDefault(_)))
        );
        let old = graph
            .replace_vob_kind(b, VobKind::ZoneMusic(ZoneMusic::default()))
            .unwrap();
        assert!(matches!(old, VobKind::ZoneMusicDefault(_)));
        assert!(
            matches!(graph.get(b).unwrap(), ObjectRef::Vob(v) if matches!(v.kind, VobKindRef::ZoneMusic(_)))
        );
    }
    #[test]
    fn allocation_failure_does_not_publish_nodes_or_change_existing_values() {
        for fail in 0..6 {
            let mut graph = Graph::new().unwrap();
            let result = arena::with_allocation_failure(fail, || {
                graph.insert(vob(VobKind::Npc(NpcData::new(GameVersion::Gothic2))))
            });
            assert_eq!(result.unwrap_err().kind, ErrorKind::LimitExceeded);
            assert_eq!(graph.len(), 0);
            let id = graph.insert(talent(7)).unwrap();
            assert!(matches!(graph.get(id).unwrap(), ObjectRef::NpcTalent(v) if v.value == 7));
        }
        let mut graph = Graph::new().unwrap();
        let id = graph.insert(vob(VobKind::Base)).unwrap();
        for fail in 0..2 {
            let result = arena::with_allocation_failure(fail, || {
                graph.replace_vob_kind(id, VobKind::Npc(NpcData::new(GameVersion::Gothic2)))
            });
            assert_eq!(result.unwrap_err().kind, ErrorKind::LimitExceeded);
            assert!(
                matches!(graph.get(id).unwrap(), ObjectRef::Vob(v) if matches!(v.kind, VobKindRef::Base))
            );
        }
        graph
            .replace_vob_kind(id, VobKind::Npc(NpcData::new(GameVersion::Gothic2)))
            .unwrap();
    }

    #[test]
    fn failed_schema_replacement_preserves_retained_version_alias() {
        for fail in 0..4 {
            let mut graph = Graph::new().unwrap();
            let id = graph
                .insert(Object::SkyController(world::SkyController::new(
                    GameVersion::Gothic2,
                )))
                .unwrap();
            graph.versions.insert(
                id.slot,
                RetainedVersion {
                    class: b"zCSkyControler_Outdoor",
                    game: GameVersion::Gothic2,
                    version: 52417,
                },
            );
            let result = arena::with_allocation_failure(fail, || {
                graph.replace(id, vob(VobKind::Npc(NpcData::new(GameVersion::Gothic2))))
            });
            assert_eq!(result.unwrap_err().kind, ErrorKind::LimitExceeded);
            assert!(matches!(
                graph.get(id).unwrap(),
                ObjectRef::SkyController(_)
            ));
            assert_eq!(
                graph.record_version(id, GameVersion::Gothic2).unwrap(),
                52417
            );
        }
    }
}
