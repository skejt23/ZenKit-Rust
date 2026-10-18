use super::*;
use zenkit::{
    archive::Format,
    objects::{Graph, Link, Metadata, Object},
    vob::{Vob, VobKind},
    world::{Hierarchy, HierarchyEntry, Section, World},
};

// All names, coordinates and records are constructed here; no game files are used.
fn scene(game: GameVersion, format: Format) -> zenkit::Result<ObjectArchive> {
    let mut graph = Graph::new()?;
    let mut parent = Vob::new(game);
    parent.base.name = Some(b"PARENT".to_vec());
    parent.base.position = [1., 2., 3.].into();
    let parent = graph.insert(Object::Vob(parent))?;
    let mut child = Vob::new(game);
    child.base.name = Some(b"CHILD".to_vec());
    child.base.position = [4., 5., 6.].into();
    child.kind = VobKind::Spot;
    let child = graph.insert(Object::Vob(child))?;
    let entry = |target, parent| HierarchyEntry {
        object: Link::new(b"vob".to_vec(), target),
        parent,
        children_name: None,
    };
    let tree = |entries| Section {
        frame: Default::default(),
        data: SectionData::VobTree(Hierarchy {
            root_count_name: None,
            entries,
        }),
    };
    let mut world = World::new(game, false);
    world.sections = vec![
        tree(vec![
            entry(Some(parent), None),
            entry(Some(child), Some(0)),
            entry(None, Some(0)),
            entry(Some(child), Some(2)),
            entry(Some(parent), None),
        ]),
        tree(vec![entry(Some(child), None)]),
        Section {
            frame: Default::default(),
            data: SectionData::End,
        },
    ];
    let world = graph.insert(Object::World(world))?;
    graph.add_root(Link::new(b"world".to_vec(), Some(world)))?;
    Ok(ObjectArchive {
        metadata: Metadata::new(format),
        game,
        graph,
    })
}

#[test]
fn loaded_worlds_list_subtypes_positions_and_every_hierarchy_occurrence() {
    let expected = concat!(
        "VobTree section 0\n",
        "0: parent=None name=\"PARENT\" class=zCVob position=(1, 2, 3)\n",
        "1: parent=Some(0) name=\"CHILD\" class=zCVobSpot:zCVob position=(4, 5, 6)\n",
        "2: parent=Some(0) <null>\n",
        "3: parent=Some(2) name=\"CHILD\" class=zCVobSpot:zCVob position=(4, 5, 6)\n",
        "4: parent=None name=\"PARENT\" class=zCVob position=(1, 2, 3)\n",
        "VobTree section 1\n",
        "0: parent=None name=\"CHILD\" class=zCVobSpot:zCVob position=(4, 5, 6)\n",
        "5 VOB occurrences\n",
    );
    for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
        for format in [Format::Ascii, Format::Binary, Format::BinSafe] {
            let bytes = scene(game, format)
                .unwrap()
                .to_bytes(Limits::default())
                .unwrap();
            let document = ObjectArchive::from_bytes(&bytes, game, Limits::default()).unwrap();
            let mut output = Vec::new();
            list_vobs(&document, &mut output).unwrap();
            assert_eq!(
                String::from_utf8(output).unwrap(),
                expected,
                "{game:?} {format:?}"
            );
        }
    }
}

#[test]
fn vobs_outside_the_world_hierarchy_are_not_listed() {
    let mut document = scene(GameVersion::Gothic2, Format::BinSafe).unwrap();
    let root = document.graph.roots()[0].target.unwrap();
    document.graph.world_mut(root).unwrap().sections.clear();
    let mut output = Vec::new();
    list_vobs(&document, &mut output).unwrap();
    assert_eq!(output, b"0 VOB occurrences\n");
}

#[test]
fn a_non_world_root_is_rejected() {
    let mut document = scene(GameVersion::Gothic2, Format::BinSafe).unwrap();
    let vob = document
        .graph
        .insert(Object::Vob(Vob::new(document.game)))
        .unwrap();
    document.graph.roots_mut()[0].target = Some(vob);
    assert!(list_vobs(&document, Vec::new()).is_err());
}
