use zenkit::{
    ErrorKind, GameVersion,
    material::Material,
    objects::{Graph, Object},
    vob::{Container, Item, NpcData, Vob, VobKind, VobKindMut, VobKindRef, ZoneMusic},
    world::World,
};

fn vob(kind: VobKind) -> Object {
    let mut value = Vob::new(GameVersion::Gothic2);
    value.kind = kind;
    Object::Vob(value)
}

#[test]
fn typed_accessors_distinguish_every_supported_schema_and_foreign_owner() {
    let game = GameVersion::Gothic2;
    let mut graph = Graph::new().unwrap();
    let ids = [
        graph.insert(vob(VobKind::Npc(NpcData::new(game)))).unwrap(),
        graph.insert(vob(VobKind::Item(Item::default()))).unwrap(),
        graph
            .insert(vob(VobKind::Container(Container::default())))
            .unwrap(),
        graph.insert(Object::Material(Material::new(game))).unwrap(),
        graph
            .insert(Object::World(World::new(game, false)))
            .unwrap(),
        graph.insert(vob(VobKind::Base)).unwrap(),
    ];
    for (index, id) in ids.into_iter().enumerate() {
        let expect = |error: Option<zenkit::Error>, matches: bool| {
            if matches {
                assert!(error.is_none());
            } else {
                assert_eq!(error.unwrap().kind, ErrorKind::WrongObjectType);
            }
        };
        expect(graph.npc(id).err(), index == 0);
        expect(graph.npc_mut(id).err(), index == 0);
        expect(graph.item(id).err(), index == 1);
        expect(graph.item_mut(id).err(), index == 1);
        expect(graph.container(id).err(), index == 2);
        expect(graph.container_mut(id).err(), index == 2);
        expect(graph.material(id).err(), index == 3);
        expect(graph.material_mut(id).err(), index == 3);
        expect(graph.world(id).err(), index == 4);
        expect(graph.world_mut(id).err(), index == 4);
        expect(graph.vob(id).err(), index != 3 && index != 4);
        expect(graph.vob_mut(id).err(), index != 3 && index != 4);
    }
    let mut other = Graph::new().unwrap();
    let foreign = other.insert(vob(VobKind::Npc(NpcData::new(game)))).unwrap();
    let errors = [
        graph.npc(foreign).unwrap_err(),
        graph.npc_mut(foreign).unwrap_err(),
        graph.item(foreign).unwrap_err(),
        graph.item_mut(foreign).unwrap_err(),
        graph.container(foreign).unwrap_err(),
        graph.container_mut(foreign).unwrap_err(),
        graph.material(foreign).unwrap_err(),
        graph.material_mut(foreign).unwrap_err(),
        graph.world(foreign).unwrap_err(),
        graph.world_mut(foreign).unwrap_err(),
        graph.vob(foreign).unwrap_err(),
        graph.vob_mut(foreign).unwrap_err(),
    ];
    assert!(errors.iter().all(|e| e.kind == ErrorKind::InvalidHandle));
    assert_eq!(graph.len(), ids.len());
}

#[test]
fn projections_edit_stored_fields_and_keep_exact_shared_payload_tags() {
    let mut graph = Graph::new().unwrap();
    let ordinary = graph
        .insert(vob(VobKind::ZoneMusic(ZoneMusic::default())))
        .unwrap();
    let default = graph
        .insert(vob(VobKind::ZoneMusicDefault(ZoneMusic::default())))
        .unwrap();
    for (id, is_default) in [(ordinary, false), (default, true)] {
        let mut view = graph.vob_mut(id).unwrap();
        view.base_mut().show_visual = true;
        assert_eq!(
            matches!(view.kind_mut(), VobKindMut::ZoneMusicDefault(_)),
            is_default
        );
        let (base, kind) = view.into_parts();
        base.physics_enabled = false;
        assert_eq!(matches!(kind, VobKindMut::ZoneMusicDefault(_)), is_default);
        let view = graph.vob(id).unwrap();
        assert!(view.base().show_visual);
        assert!(!view.base().physics_enabled);
        assert_eq!(
            matches!(view.kind(), VobKindRef::ZoneMusicDefault(_)),
            is_default
        );
    }
    let npc = graph
        .insert(vob(VobKind::Npc(NpcData::new(GameVersion::Gothic2))))
        .unwrap();
    let value = graph.npc_mut(npc).unwrap();
    value.guild = 41;
    value.level = 12;
    let (base, kind) = graph.vob_mut(npc).unwrap().into_parts();
    let VobKindMut::Npc(value) = kind else {
        panic!()
    };
    base.show_visual = true;
    value.guild += 1;
    assert_eq!(
        (graph.npc(npc).unwrap().guild, graph.npc(npc).unwrap().level),
        (42, 12)
    );
    graph
        .replace_vob_kind(npc, VobKind::Item(Item::default()))
        .unwrap();
    assert_eq!(graph.npc(npc).unwrap_err().kind, ErrorKind::WrongObjectType);
    assert!(graph.item(npc).is_ok());
    assert!(graph.vob(npc).unwrap().base().show_visual);
}
