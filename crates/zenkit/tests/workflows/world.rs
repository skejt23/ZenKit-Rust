//! A hierarchy owns occurrences; the graph owns the referenced objects.
use zenkit::{
    GameVersion,
    archive::Format,
    objects::{Graph, Limits, Link, Metadata, Object, ObjectArchive},
    vob::Vob,
    world::*,
};
fn main() -> zenkit::Result<()> {
    let game = GameVersion::Gothic2;
    let mut graph = Graph::new()?;
    let vob = graph.insert(Object::Vob(Vob::new(game)))?;
    let point = graph.insert(Object::WayPoint(WayPoint {
        name: b"CROSSROADS".to_vec(),
        ..Default::default()
    }))?;
    let net = graph.insert(Object::WayNet(WayNet {
        version: 1,
        points: vec![Link::new(b"point0".to_vec(), Some(point))],
        edges: vec![Way {
            left: Link::new(b"left".to_vec(), Some(point)),
            right: Link::new(b"right".to_vec(), Some(point)),
        }],
    }))?;
    let sections = vec![
        Section {
            frame: Default::default(),
            data: SectionData::VobTree(Hierarchy {
                root_count_name: None,
                entries: vec![
                    HierarchyEntry {
                        object: Link::new(b"first".to_vec(), Some(vob)),
                        parent: None,
                        children_name: None,
                    },
                    HierarchyEntry {
                        object: Link::new(b"childAlias".to_vec(), Some(vob)),
                        parent: Some(0),
                        children_name: None,
                    },
                    HierarchyEntry {
                        object: Link::new(b"secondRoot".to_vec(), Some(vob)),
                        parent: None,
                        children_name: None,
                    },
                ],
            }),
        },
        Section {
            frame: Default::default(),
            data: SectionData::WayNet(Link::new(b"navigation".to_vec(), Some(net))),
        },
        Section {
            frame: Default::default(),
            data: SectionData::End,
        },
    ];
    let world = graph.insert(Object::World(World {
        sections,
        state: None,
    }))?;
    graph.add_root(Link::new(b"world".to_vec(), Some(world)))?;
    let document = ObjectArchive {
        metadata: Metadata::new(Format::BinSafe),
        game,
        graph,
    };
    let bytes = document.to_bytes(Limits::default())?;
    let restored = ObjectArchive::from_bytes(&bytes, game, Limits::default())?;
    let world = restored
        .graph
        .world(restored.graph.roots()[0].target.unwrap())?;
    let SectionData::VobTree(tree) = &world.sections[0].data else {
        unreachable!()
    };
    assert_eq!(tree.entries[0].object.target, tree.entries[1].object.target);
    assert_eq!(tree.entries[0].object.target, tree.entries[2].object.target);
    assert_eq!(tree.entries[1].parent, Some(0));
    assert_eq!(tree.entries[2].parent, None);
    println!(
        "{} bytes; {} shared nodes; {} hierarchy occurrences",
        bytes.len(),
        restored.graph.len(),
        tree.entries.len()
    );
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
