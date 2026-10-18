//! Edit and serialize object graphs for both games and all archive encodings.
//!
use zenkit::{
    ErrorKind, GameVersion, Result,
    archive::Format,
    material::Material,
    objects::{
        Graph, Limits, Link, Metadata, Object, ObjectArchive, ObjectId, ObjectMut, ObjectRef,
    },
    vob::*,
    world::{Hierarchy, HierarchyEntry, Section, SectionData, World},
};

fn saved_vob(game: GameVersion, kind: VobKind) -> Vob {
    let mut value = Vob::new(game);
    value.base.physics_enabled = false;
    value.base.state = Some(SaveState::default());
    value.kind = kind;
    value
}

// Read the Item flags before mutably borrowing the NPC.
fn set_shortcut(graph: &mut Graph, npc: ObjectId, index: usize, key: i32) -> Result<()> {
    let item = graph.npc(npc)?.items[index].item.target;
    let needs_key = match item {
        Some(id) => graph
            .item(id)?
            .state
            .as_ref()
            .is_some_and(|state| state.flags & 0x200 != 0),
        None => false,
    };
    graph.npc_mut(npc)?.items[index].short_key = needs_key.then_some(key);
    Ok(())
}

fn construct(game: GameVersion, format: Format) -> Result<ObjectArchive> {
    let mut graph = Graph::new()?;
    let mut data = NpcData::new(game);
    data.instance = b"EXAMPLE_NPC".to_vec();
    data.carry = Link::new(b"carryVob".to_vec(), None);
    if let NpcVersion::Gothic2 { packed, .. } = &mut data.version {
        *packed = NpcPacked::from_fields([b""; 9], 8)?;
    }
    let npc = graph.insert(Object::Vob(saved_vob(game, VobKind::Npc(data))))?;
    let item = graph.insert(Object::Vob(saved_vob(
        game,
        VobKind::Item(Item {
            instance: b"ITMI_EXAMPLE".to_vec(),
            state: Some(ItemState {
                amount: 3,
                flags: 0x200,
            }),
        }),
    )))?;
    let chest = graph.insert(Object::Vob(saved_vob(
        game,
        VobKind::Container(Container {
            items: Some(Vec::new()),
            ..Default::default()
        }),
    )))?;
    let material = graph.insert(Object::Material(Material::new(game)))?;
    let world = graph.insert(Object::World(World::new(game, true)))?;

    let data = graph.npc_mut(npc)?;
    data.guild = 42;
    data.enemy = Link::new(b"self".to_vec(), Some(npc));
    data.items = vec![
        NpcItem {
            item: Link::new(b"first".to_vec(), Some(item)),
            short_key: Some(0),
        },
        NpcItem {
            item: Link::new(b"alias".to_vec(), Some(item)),
            short_key: Some(0),
        },
        NpcItem {
            item: Link::new(b"empty".to_vec(), None),
            short_key: None,
        },
    ];
    graph.item_mut(item)?.state.as_mut().unwrap().amount = 5;
    graph.container_mut(chest)?.items = Some(vec![
        Link::new(b"first".to_vec(), Some(item)),
        Link::new(b"sameItem".to_vec(), Some(item)),
    ]);
    graph.material_mut(material)?.texture = b"EDITED.TGA".to_vec();
    graph.vob_mut(chest)?.base_mut().name = Some(b"CHEST".to_vec());
    assert_eq!(
        graph.vob(chest)?.base().name.as_deref(),
        Some(b"CHEST".as_slice())
    );

    // into_parts() retains both borrows after the temporary view is consumed.
    let (base, kind) = graph.vob_mut(npc)?.into_parts();
    let VobKindMut::Npc(data) = kind else {
        unreachable!()
    };
    base.show_visual = true;
    data.guild_true = data.guild;

    // Bind the view before borrowing its subtype.
    let mut view = graph.vob_mut(npc)?;
    {
        let VobKindMut::Npc(data) = view.kind_mut() else {
            unreachable!()
        };
        data.guild_true = 42;
    }
    view.reborrow().base_mut().show_visual = true;
    assert!(matches!(view.kind(), VobKindRef::Npc(_)));

    if let ObjectMut::Material(value) = graph.get_mut(material)? {
        value.name = b"EXAMPLE_MATERIAL".to_vec();
    }

    // Two occurrences of one Item retain independent shortcuts.
    set_shortcut(&mut graph, npc, 0, 7)?;
    set_shortcut(&mut graph, npc, 1, -3)?;
    set_shortcut(&mut graph, npc, 2, 99)?;

    // The NPC and its AI refer to each other.
    let ai = graph.insert(Object::AiMove(AiMove {
        vob: Link::new(b"vob".to_vec(), Some(npc)),
        owner: Link::new(b"owner".to_vec(), Some(npc)),
    }))?;
    graph.vob_mut(npc)?.base_mut().ai = Some(Link::new(b"ai".to_vec(), Some(ai)));
    let owner = match graph.get(ai)? {
        ObjectRef::AiMove(ai) => ai.owner.target.unwrap(),
        _ => unreachable!(),
    };
    graph.npc_mut(owner)?.level = 12;

    let value = graph.world_mut(world)?;
    value.sections = vec![
        Section {
            frame: Default::default(),
            data: SectionData::VobTree(Hierarchy {
                root_count_name: None,
                entries: vec![npc, chest]
                    .into_iter()
                    .map(|id| HierarchyEntry {
                        object: Link::new(b"vob".to_vec(), Some(id)),
                        parent: None,
                        children_name: None,
                    })
                    .collect(),
            }),
        },
        Section {
            frame: Default::default(),
            data: SectionData::End,
        },
    ];
    value
        .state
        .as_mut()
        .unwrap()
        .npcs
        .push(Link::new(b"npc".to_vec(), Some(npc)));

    // Replace an unreferenced object; inventory links require an Item target.
    let draft = graph.insert(Object::Vob(saved_vob(game, VobKind::Base)))?;
    let old = graph.replace_vob_kind(draft, VobKind::Item(Item::default()))?;
    assert!(matches!(old, VobKind::Base));
    assert!(graph.item(draft).is_ok());
    let old = graph.replace(draft, Object::Material(Material::new(game)))?;
    assert!(matches!(old, Object::Vob(_)));
    assert!(graph.material(draft).is_ok());

    // Explicit cloning keeps links to this graph, including the self-reference.
    let owned_copy = graph.get(npc)?.to_owned();
    let Object::Vob(copy) = owned_copy else {
        unreachable!()
    };
    let VobKind::Npc(copy) = copy.kind else {
        unreachable!()
    };
    assert_eq!(copy.enemy.target, Some(npc));

    assert_eq!(
        graph.npc(item).unwrap_err().kind,
        ErrorKind::WrongObjectType
    );
    assert_eq!(
        Graph::new()?.npc(npc).unwrap_err().kind,
        ErrorKind::InvalidHandle
    );

    let mut npc_count = 0;
    for (_, object) in graph.objects() {
        if let ObjectRef::Vob(value) = object {
            match value.kind() {
                VobKindRef::Npc(data) => {
                    assert_eq!(data.guild, 42);
                    assert!(value.base().show_visual);
                    npc_count += 1;
                }
                VobKindRef::Container(_) | VobKindRef::Item(_) => {}
                _ => unreachable!(),
            }
        }
    }
    assert_eq!(npc_count, 1);

    // Reopening may renumber slots; recover IDs from the roots and links.
    for (name, id) in [
        (b"world".as_slice(), world),
        (b"material", material),
        (b"npcAlias", npc),
        (b"chestAlias", chest),
        (b"replacement", draft),
    ] {
        graph.add_root(Link::new(name.to_vec(), Some(id)))?;
    }
    let mut metadata = Metadata::new(format);
    metadata.save_game = true;
    Ok(ObjectArchive {
        metadata,
        game,
        graph,
    })
}

fn round_trip(game: GameVersion, format: Format) -> Result<()> {
    let document = construct(game, format)?;
    document.validate(Limits::default())?;
    let bytes = document.to_bytes(Limits::default())?;
    let restored = ObjectArchive::from_bytes(&bytes, game, Limits::default())?;
    let ids: Vec<_> = restored
        .graph
        .roots()
        .iter()
        .map(|link| link.target.unwrap())
        .collect();
    let world = restored.graph.world(ids[0])?;
    let npc = restored.graph.npc(ids[2])?;
    assert_eq!(world.state.as_ref().unwrap().npcs[0].target, Some(ids[2]));
    assert_eq!((npc.guild, npc.guild_true, npc.level), (42, 42, 12));
    assert_eq!(npc.enemy.target, Some(ids[2]));
    assert_eq!(npc.items[0].item.target, npc.items[1].item.target);
    assert_eq!(
        (
            npc.items[0].short_key,
            npc.items[1].short_key,
            npc.items[2].short_key
        ),
        (Some(7), Some(-3), None)
    );
    let item = npc.items[0].item.target.unwrap();
    assert_eq!(restored.graph.item(item)?.state.unwrap().amount, 5);
    assert_eq!(
        restored.graph.container(ids[3])?.items.as_ref().unwrap()[0].target,
        Some(item)
    );
    assert_eq!(restored.graph.material(ids[1])?.texture, b"EDITED.TGA");
    assert_eq!(restored.to_bytes(Limits::default())?, bytes);
    Ok(())
}

fn main() -> Result<()> {
    for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
        for format in [Format::Ascii, Format::Binary, Format::BinSafe] {
            round_trip(game, format)?;
        }
    }
    println!("Six complete graph-editing workflows passed, including fields, cycles and aliases");
    Ok(())
}

#[test]
fn complete_api_workflows() {
    main().unwrap();
}
