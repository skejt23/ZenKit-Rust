use zenkit::{
    ErrorKind,
    archive::{self, Value as V},
    objects::Limits as ObjectLimits,
};
use zenkit::{GameVersion, archive::Format, objects::*, world::*};

fn document(game: GameVersion, format: Format, save: bool) -> ObjectArchive {
    let mut graph = Graph::new().unwrap();
    let world = graph.insert(Object::World(World::new(game, save))).unwrap();
    let point = graph
        .insert(Object::WayPoint(WayPoint {
            name: b"POINT".to_vec(),
            water_depth: -9,
            ..Default::default()
        }))
        .unwrap();
    let way = graph
        .insert(Object::WayNet(WayNet {
            version: -3,
            points: vec![
                Link::new(b"point".to_vec(), Some(point)),
                Link::new(b"alias".to_vec(), Some(point)),
            ],
            edges: vec![Way {
                left: Link::new(b"left".to_vec(), Some(point)),
                right: Link::new(b"right".to_vec(), None),
            }],
        }))
        .unwrap();
    let sky = graph
        .insert(Object::SkyController(SkyController::new(game)))
        .unwrap();
    let ObjectMut::World(w) = graph.get_mut(world).unwrap() else {
        unreachable!()
    };
    w.sections = vec![
        Section {
            frame: SectionFrame::default(),
            data: SectionData::VobTree(Hierarchy {
                root_count_name: Some(b"childs0".to_vec()),
                entries: vec![
                    HierarchyEntry {
                        object: Link::new(b"empty".to_vec(), None),
                        parent: None,
                        children_name: Some(b"childs0".to_vec()),
                    },
                    HierarchyEntry {
                        object: Link::new(b"child".to_vec(), None),
                        parent: Some(0),
                        children_name: Some(b"childs1".to_vec()),
                    },
                ],
            }),
        },
        Section {
            frame: SectionFrame {
                index: 73,
                version: 2,
                ..Default::default()
            },
            data: SectionData::WayNet(Link::new(b"wayNet".to_vec(), Some(way))),
        },
        Section {
            frame: SectionFrame::default(),
            data: SectionData::SkyController(Link::new(b"sky".to_vec(), Some(sky))),
        },
        Section {
            frame: SectionFrame::default(),
            data: SectionData::End,
        },
    ];
    graph
        .add_root(Link::new(b"world".to_vec(), Some(world)))
        .unwrap();
    let mut metadata = Metadata::new(format);
    metadata.save_game = save;
    ObjectArchive {
        metadata,
        game,
        graph,
    }
}

#[test]
fn world_sections_navigation_and_null_hierarchy_roundtrip() {
    for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
        for format in [Format::Ascii, Format::Binary, Format::BinSafe] {
            for save in [false, true] {
                let doc = document(game, format, save);
                let bytes = doc.to_bytes(zenkit::objects::Limits::default()).unwrap();
                let loaded =
                    ObjectArchive::from_bytes(&bytes, game, zenkit::objects::Limits::default())
                        .unwrap();
                assert_eq!(
                    loaded.to_bytes(zenkit::objects::Limits::default()).unwrap(),
                    bytes
                );
                let ObjectRef::World(w) = loaded
                    .graph
                    .get(loaded.graph.roots()[0].target.unwrap())
                    .unwrap()
                else {
                    panic!()
                };
                assert_eq!(w.sections.len(), 4);
                assert_eq!(w.sections[1].frame.index, 73);
                let SectionData::VobTree(t) = &w.sections[0].data else {
                    panic!()
                };
                assert_eq!(t.entries[1].parent, Some(0));
                assert!(t.entries.iter().all(|e| e.object.target.is_none()));
            }
        }
    }
}

#[test]
fn world_unknown_sections_are_not_treated_as_opaque_known_frames() {
    for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
        for format in [Format::Ascii, Format::Binary, Format::BinSafe] {
            for save in [false, true] {
                let doc = document(game, format, save);
                let mut bytes = doc.to_bytes(ObjectLimits::default()).unwrap();
                let offset = bytes
                    .windows(9)
                    .position(|span| span == b"EndMarker")
                    .unwrap();
                bytes[offset..offset + 9].copy_from_slice(b"NewMarker");
                assert_eq!(
                    ObjectArchive::from_bytes(&bytes, game, ObjectLimits::default())
                        .unwrap_err()
                        .kind,
                    zenkit::ErrorKind::Unsupported
                );
            }
        }
    }
}

#[test]
fn anonymous_frame_classes_and_versions_are_not_object_identities() {
    let mut d = document(GameVersion::Gothic2, Format::Binary, false);
    let ObjectMut::World(w) = d.graph.get_mut(d.graph.roots()[0].target.unwrap()).unwrap() else {
        panic!()
    };
    for s in &mut w.sections {
        s.frame.class_name.clear();
        s.frame.index = 1000;
        s.frame.version = 65535;
    }
    let bytes = d.to_bytes(ObjectLimits::default()).unwrap();
    let mut loaded = ObjectArchive::from_bytes(&bytes, d.game, ObjectLimits::default()).unwrap();
    assert_eq!(loaded.graph.len(), 4);
    loaded.metadata.format = Format::BinSafe;
    assert_eq!(
        loaded.to_bytes(ObjectLimits::default()).unwrap_err().kind,
        ErrorKind::Unsupported
    );
    let id = loaded.graph.roots()[0].target.unwrap();
    let ObjectMut::World(w) = loaded.graph.get_mut(id).unwrap() else {
        panic!()
    };
    for s in &mut w.sections {
        s.frame.class_name = b"%".to_vec();
    }
    let tagged = loaded.to_bytes(ObjectLimits::default()).unwrap();
    let mut r = archive::Reader::new(&tagged, archive::Limits::default()).unwrap();
    assert_eq!(r.object_count(), 4);
    r.begin_object().unwrap();
    let section = r.begin_object().unwrap().unwrap();
    assert_eq!((section.version, section.index), (65535, 1000));
}

#[test]
fn hierarchy_order_types_and_aggregate_orphan_limits_are_checked() {
    let mut d = document(GameVersion::Gothic1, Format::BinSafe, false);
    let id = d.graph.roots()[0].target.unwrap();
    let ObjectMut::World(w) = d.graph.get_mut(id).unwrap() else {
        panic!()
    };
    let SectionData::VobTree(t) = &mut w.sections[0].data else {
        panic!()
    };
    t.entries[1].parent = Some(1);
    assert_eq!(
        d.validate(ObjectLimits::default()).unwrap_err().kind,
        ErrorKind::InvalidData
    );
    let ObjectMut::World(w) = d.graph.get_mut(id).unwrap() else {
        panic!()
    };
    let SectionData::VobTree(t) = &mut w.sections[0].data else {
        panic!()
    };
    t.entries[1].parent = Some(0);
    t.entries[0].object.target = Some(id);
    assert_eq!(
        d.validate(ObjectLimits::default()).unwrap_err().kind,
        ErrorKind::WrongObjectType
    );
    let ObjectMut::World(w) = d.graph.get_mut(id).unwrap() else {
        panic!()
    };
    let SectionData::VobTree(t) = &mut w.sections[0].data else {
        panic!()
    };
    t.entries[0].object.target = None;
    let exact = ObjectLimits {
        world: zenkit::world::Limits {
            sections: 4,
            ..Default::default()
        },
        ..Default::default()
    };
    d.validate(exact).unwrap();
    d.graph
        .insert(Object::World(World {
            sections: vec![Section {
                frame: Default::default(),
                data: SectionData::End,
            }],
            state: None,
        }))
        .unwrap();
    assert_eq!(
        d.validate(exact).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(
        d.to_bytes(exact).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
}

fn sky_wire(format: Format, version: u16, aliases: bool) -> Vec<u8> {
    if format == Format::Ascii && aliases {
        let text = String::from_utf8(sky_wire(format, version, false)).unwrap();
        return text
            .replace("isRaining=", "isRaining\t\t =")
            .replace("rainCtr=", "rainCtr\t\t =")
            .into_bytes();
    }
    let mut w =
        archive::Writer::new(archive::Header::new(format), archive::Limits::default()).unwrap();
    w.begin_object(b"sky", b"zCSkyControler_Outdoor", version)
        .unwrap();
    for n in [
        b"masterTime".as_slice(),
        b"rainWeight",
        b"rainStart",
        b"rainStop",
        b"rainSctTimer",
        b"rainSndVol",
        b"dayCtr",
        b"fadeScale",
    ] {
        w.write(n, V::Float(1.25)).unwrap();
    }
    w.write(b"renderLightning", V::Bool(true)).unwrap();
    w.write(
        if aliases {
            b"isRaining\t\t "
        } else {
            b"isRaining"
        },
        V::Bool(false),
    )
    .unwrap();
    w.write(
        if aliases { b"rainCtr\t\t " } else { b"rainCtr" },
        V::Int(-7),
    )
    .unwrap();
    w.end_object().unwrap();
    w.finish().unwrap()
}

#[test]
fn retained_versions_are_owner_class_and_game_checked() {
    let mut graph = Graph::new().unwrap();
    let canonical = graph
        .insert(Object::Vob(zenkit::vob::Vob::new(GameVersion::Gothic2)))
        .unwrap();
    assert_eq!(
        graph
            .record_version(canonical, GameVersion::Gothic1)
            .unwrap_err()
            .kind,
        ErrorKind::InvalidData
    );
    assert_eq!(
        graph
            .record_version(canonical, GameVersion::Gothic2)
            .unwrap(),
        52224
    );
    for format in [Format::Ascii, Format::Binary, Format::BinSafe] {
        let bytes = sky_wire(format, 52417, false);
        let mut d =
            ObjectArchive::from_bytes(&bytes, GameVersion::Gothic2, ObjectLimits::default())
                .unwrap();
        let id = d.graph.roots()[0].target.unwrap();
        assert_eq!(d.graph.record_version(id, d.game).unwrap(), 52417);
        assert_eq!(d.to_bytes(ObjectLimits::default()).unwrap(), bytes);
        let same_class = d.graph.get(id).unwrap().to_owned();
        d.graph.replace(id, same_class).unwrap();
        assert_eq!(d.graph.record_version(id, d.game).unwrap(), 52417);
        assert_eq!(
            d.graph
                .record_version(id, GameVersion::Gothic1)
                .unwrap_err()
                .kind,
            ErrorKind::InvalidData
        );
        let foreign = Graph::new()
            .unwrap()
            .insert(Object::WayPoint(Default::default()))
            .unwrap();
        assert_eq!(
            d.graph.clear_record_version(foreign).unwrap_err().kind,
            ErrorKind::InvalidHandle
        );
        d.graph
            .replace(id, Object::WayPoint(Default::default()))
            .unwrap();
        // Explicit schema replacement discards the old class's retained alias.
        d.validate(ObjectLimits::default()).unwrap();
        assert_eq!(d.graph.record_version(id, d.game).unwrap(), 0);
        d.graph.clear_record_version(id).unwrap();
        d.validate(ObjectLimits::default()).unwrap();
        assert_eq!(d.graph.record_version(id, d.game).unwrap(), 0);
        assert_eq!(
            ObjectArchive::from_bytes(
                &sky_wire(format, 52418, false),
                GameVersion::Gothic2,
                ObjectLimits::default()
            )
            .unwrap_err()
            .kind,
            ErrorKind::Unsupported
        );
    }
}

#[test]
fn original_sky_spellings_are_exact_aliases_with_canonical_output() {
    for format in [Format::Ascii, Format::BinSafe] {
        let d = ObjectArchive::from_bytes(
            &sky_wire(format, 52417, true),
            GameVersion::Gothic2,
            ObjectLimits::default(),
        )
        .unwrap();
        assert_eq!(
            d.to_bytes(ObjectLimits::default()).unwrap(),
            sky_wire(format, 52417, false)
        );
    }
}

fn rename_field(mut bytes: Vec<u8>, format: Format, old: &[u8], new: &[u8]) -> Vec<u8> {
    assert_eq!(old.len(), new.len());
    if format == Format::Ascii {
        let mut key = old.to_vec();
        key.extend_from_slice(b"=int:");
        let p = bytes.windows(key.len()).position(|v| v == key).unwrap();
        bytes[p..p + old.len()].copy_from_slice(new);
    } else {
        let head = bytes.windows(4).position(|v| v == b"END\n").unwrap() + 4;
        let table = u32::from_le_bytes(bytes[head + 8..head + 12].try_into().unwrap()) as usize;
        let count = u32::from_le_bytes(bytes[table..table + 4].try_into().unwrap());
        let mut p = table + 4;
        for _ in 0..count {
            let len = u16::from_le_bytes(bytes[p..p + 2].try_into().unwrap()) as usize;
            if &bytes[p + 8..p + 8 + len] == old {
                bytes[p + 8..p + 8 + len].copy_from_slice(new);
                return bytes;
            }
            p += 8 + len;
        }
        panic!("field absent");
    }
    bytes
}

#[test]
fn npc_hit_chance_alias_is_a_coherent_group() {
    let game = GameVersion::Gothic2;
    let source =
        include_bytes!("../tests/fixtures/synthetic-vob-npc-ascii-g2-save-packed-canonical.zen");
    for format in [Format::Ascii, Format::BinSafe] {
        let mut d = ObjectArchive::from_bytes(source, game, ObjectLimits::default()).unwrap();
        d.metadata.format = format;
        let canonical = d.to_bytes(ObjectLimits::default()).unwrap();
        let mut legacy = canonical.clone();
        for i in (0..4).rev() {
            legacy = rename_field(
                legacy,
                format,
                format!("hc{i}").as_bytes(),
                format!("hc{}", i + 1).as_bytes(),
            );
        }
        let loaded = ObjectArchive::from_bytes(&legacy, game, ObjectLimits::default()).unwrap();
        assert_eq!(loaded.to_bytes(ObjectLimits::default()).unwrap(), canonical);
        let mixed = rename_field(legacy, format, b"hc4", b"hc0");
        assert_eq!(
            ObjectArchive::from_bytes(&mixed, game, ObjectLimits::default())
                .unwrap_err()
                .kind,
            ErrorKind::InvalidData
        );
    }
}

#[test]
fn geometry_selection_deduplicates_and_ownership_budgets_accumulate() {
    let game = GameVersion::Gothic2;
    let mesh = zenkit::mesh::Mesh::from_bytes(
        include_bytes!("../tests/fixtures/synthetic-mesh-msh-g2.msh"),
        game,
        false,
        zenkit::mesh::Limits::default(),
    )
    .unwrap();
    let mut bsp = BspTree::new(game).unwrap();
    bsp.polygon_indices = vec![1, 0, 1];
    bsp.nodes[0].polygon_count = 3;
    let g = MeshAndBsp { mesh, bsp };
    assert_eq!(g.leaf_triangles(1, 3, 3).unwrap().len(), 3);
    assert_eq!(
        g.leaf_triangles(1, 2, 3).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    let mut d = document(game, Format::BinSafe, false);
    let id = d.graph.roots()[0].target.unwrap();
    let ObjectMut::World(w) = d.graph.get_mut(id).unwrap() else {
        panic!()
    };
    w.sections.insert(
        0,
        Section {
            frame: Default::default(),
            data: SectionData::MeshAndBsp(g.clone()),
        },
    );
    let mut low = 0;
    let mut high = zenkit::mesh::Limits::default().owned_bytes;
    while low < high {
        let mid = (low + high) / 2;
        let limits = ObjectLimits {
            world: zenkit::world::Limits {
                geometry: zenkit::mesh::Limits {
                    owned_bytes: mid,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        };
        if d.validate(limits).is_ok() {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    let limits = ObjectLimits {
        world: zenkit::world::Limits {
            geometry: zenkit::mesh::Limits {
                owned_bytes: low,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    d.validate(limits).unwrap();
    d.graph
        .insert(Object::World(World {
            sections: vec![Section {
                frame: Default::default(),
                data: SectionData::MeshAndBsp(g),
            }],
            state: None,
        }))
        .unwrap();
    assert_eq!(
        d.validate(limits).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
}
