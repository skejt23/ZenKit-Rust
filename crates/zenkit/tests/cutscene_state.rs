use zenkit::vob::VobKindRef;
use zenkit::{
    ErrorKind, GameVersion,
    archive::Format,
    cutscene::{self, *},
    objects::*,
};
fn wire(format: Format, game: GameVersion, variant: &str) -> Vec<u8> {
    let enc = match format {
        Format::Ascii => "ascii",
        Format::Binary => "binary",
        Format::BinSafe => "bin_safe",
    };
    let game = if game == GameVersion::Gothic1 { 1 } else { 2 };
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "tests/fixtures/synthetic-cutscene-state-{enc}-g{game}-{variant}.zen"
        )),
    )
    .unwrap()
}
fn parsed(format: Format, game: GameVersion, variant: &str) -> ObjectArchive {
    ObjectArchive::from_bytes(&wire(format, game, variant), game, Limits::default()).unwrap()
}
fn formats() -> [Format; 3] {
    [Format::Ascii, Format::Binary, Format::BinSafe]
}
fn context(d: &ObjectArchive) -> ObjectId {
    d.graph
        .objects()
        .find(|(_, n)| matches!(n, ObjectRef::CutsceneContext(_)))
        .unwrap()
        .0
}
#[test]
fn complete_fields_and_interleaved_cycles_in_all_modes() {
    for format in formats() {
        for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
            for save in [false, true] {
                for packed in [false, true] {
                    let variant = format!(
                        "{}-{}-canonical",
                        if save { "save" } else { "normal" },
                        if packed { "packed" } else { "unpacked" }
                    );
                    let d = parsed(format, game, &variant);
                    assert_eq!(d.graph.len(), 19);
                    assert_eq!(d.graph.roots().len(), 4);
                    let id = context(&d);
                    let ObjectRef::CutsceneContext(c) = d.graph.get(id).unwrap() else {
                        panic!()
                    };
                    assert_eq!(
                        (&c.block.name, c.block.sub_block0.to_bits()),
                        (&b"CONTEXT\xff".to_vec(), 0.375f32.to_bits())
                    );
                    assert_eq!(
                        (c.role_count, c.role_vob_count, c.reference, c.actual_block),
                        (-3, 17, -7, 23)
                    );
                    assert!(c.is_cutscene);
                    assert!(!c.was_triggered);
                    assert_eq!(c.npc.target, c.main_role.target);
                    assert_eq!(c.stage.target, Some(id));
                    assert_eq!(c.alt_stage.target, d.graph.roots()[0].target);
                    let ObjectRef::CutsceneProps(p) = d.graph.get(c.props.target.unwrap()).unwrap()
                    else {
                        panic!()
                    };
                    assert_eq!(
                        (&p.name, &p.stage_name, &p.script_function_on_stop),
                        (
                            &b"PROPS\xfc".to_vec(),
                            &b"STAGE\xfb".to_vec(),
                            &b"STOP\xfa".to_vec()
                        )
                    );
                    assert!(p.global && p.has_to_be_triggered && !p.looped);
                    assert_eq!(
                        (
                            p.distance,
                            p.range,
                            p.locked_block_count,
                            p.run_behavior,
                            p.run_behavior_value
                        ),
                        (123.25, 456.5, -5, 2, -11)
                    );
                    let ObjectRef::CutscenePlayer(p) =
                        d.graph.get(d.graph.roots()[0].target.unwrap()).unwrap()
                    else {
                        panic!()
                    };
                    assert_eq!((p.last_process_day, p.last_process_hour), (-3, 17));
                    assert_eq!(p.playlists[0].target, p.playlists[2].target);
                    assert!(p.playlists[1].target.is_none());
                    let ObjectRef::CutsceneManager(m) =
                        d.graph.get(d.graph.roots()[2].target.unwrap()).unwrap()
                    else {
                        panic!()
                    };
                    assert_eq!(m.pool_items[0].target, m.pool_items[2].target);
                    assert!(m.pool_items[1].target.is_none());
                    let ObjectRef::CutscenePoolItem(p) =
                        d.graph.get(m.pool_items[3].target.unwrap()).unwrap()
                    else {
                        panic!()
                    };
                    assert_eq!(
                        (
                            &p.name,
                            p.run_behavior,
                            p.run_behavior_value,
                            p.times_played,
                            p.deactivated,
                            p.flags
                        ),
                        (&b"POOL_B\xf7".to_vec(), 2, -3, -5, 4, -17)
                    );
                    let ObjectRef::Vob(npc) = d.graph.get(c.npc.target.unwrap()).unwrap() else {
                        panic!()
                    };
                    assert!(matches!(npc.kind(), VobKindRef::Npc(_)));
                    if packed && save {
                        let ObjectRef::EventManager(e) = d
                            .graph
                            .get(npc.base().event_manager.as_ref().unwrap().target.unwrap())
                            .unwrap()
                        else {
                            panic!()
                        };
                        assert_eq!(e.cutscene.target, Some(id));
                    }
                    assert_eq!(
                        cutscene::message(&d.graph, id).unwrap().unwrap().text,
                        b"Synthetic runtime line \xff"
                    );
                    let bytes = d.to_bytes(Limits::default()).unwrap();
                    let restored =
                        ObjectArchive::from_bytes(&bytes, game, Limits::default()).unwrap();
                    assert_eq!(restored.to_bytes(Limits::default()).unwrap(), bytes);
                }
            }
        }
    }
}
#[test]
fn derived_roots_resolve_and_set_only_exact_block_paths() {
    let mut d = parsed(
        Format::BinSafe,
        GameVersion::Gothic2,
        "save-packed-canonical",
    );
    let ctx = context(&d);
    let scene = d.graph.roots()[1].target.unwrap();
    let message = cutscene::message_id(&d.graph, ctx).unwrap();
    assert_eq!(message, cutscene::message_id(&d.graph, scene).unwrap());
    cutscene::set_message(&mut d.graph, scene, None).unwrap();
    assert_eq!(cutscene::message_id(&d.graph, ctx).unwrap(), None);
    cutscene::set_message(&mut d.graph, ctx, message).unwrap();
    let before = d.to_bytes(Limits::default()).unwrap();
    assert_eq!(
        cutscene::set_message(&mut d.graph, ctx, Some(scene))
            .unwrap_err()
            .kind,
        ErrorKind::WrongObjectType
    );
    assert_eq!(d.to_bytes(Limits::default()).unwrap(), before);
    let ObjectMut::CutsceneContext(c) = d.graph.get_mut(ctx).unwrap() else {
        panic!()
    };
    let old = c.block.child.target;
    c.block.child.target = Some(scene);
    assert_eq!(
        cutscene::message_id(&d.graph, ctx).unwrap_err().kind,
        ErrorKind::WrongObjectType
    );
    assert_eq!(
        d.validate(Limits::default()).unwrap_err().kind,
        ErrorKind::WrongObjectType
    );
    let ObjectMut::CutsceneContext(c) = d.graph.get_mut(ctx).unwrap() else {
        panic!()
    };
    c.block.child.target = old;
    let block = old.unwrap();
    let ObjectMut::Block(b) = d.graph.get_mut(block).unwrap() else {
        panic!()
    };
    b.child.target = Some(block);
    assert_eq!(
        cutscene::message_id(&d.graph, ctx).unwrap_err().kind,
        ErrorKind::CycleDetected
    );
    assert_eq!(
        cutscene::set_message(&mut d.graph, ctx, None)
            .unwrap_err()
            .kind,
        ErrorKind::CycleDetected
    );
}
#[test]
fn exact_relationships_and_foreign_handles_include_orphans() {
    let mut d = parsed(Format::BinSafe, GameVersion::Gothic2, "normal-context");
    let ctx = context(&d);
    let mut foreign = Graph::new().unwrap();
    let foreign_id = foreign
        .insert(Object::CutsceneProps(Props::default()))
        .unwrap();
    for field in 0..6 {
        let ObjectMut::CutsceneContext(c) = d.graph.get_mut(ctx).unwrap() else {
            panic!()
        };
        let links = [
            &mut c.block.child,
            &mut c.props,
            &mut c.npc,
            &mut c.main_role,
            &mut c.alt_stage,
            &mut c.stage,
        ];
        let old = links[field].target;
        links[field].target = Some(foreign_id);
        assert_eq!(
            d.validate(Limits::default()).unwrap_err().kind,
            ErrorKind::InvalidHandle
        );
        let ObjectMut::CutsceneContext(c) = d.graph.get_mut(ctx).unwrap() else {
            panic!()
        };
        [
            &mut c.block.child,
            &mut c.props,
            &mut c.npc,
            &mut c.main_role,
            &mut c.alt_stage,
            &mut c.stage,
        ][field]
            .target = old;
    }
    let orphan = d
        .graph
        .insert(Object::CutscenePlayer(Player {
            playlists: vec![Link::new(vec![], Some(d.graph.roots()[0].target.unwrap()))],
            ..Default::default()
        }))
        .unwrap();
    d.validate(Limits::default()).unwrap();
    d.graph
        .replace(
            orphan,
            Object::CutsceneManager(Manager {
                pool_items: vec![Link::new(vec![], Some(ctx))],
            }),
        )
        .unwrap();
    assert_eq!(
        d.validate(Limits::default()).unwrap_err().kind,
        ErrorKind::WrongObjectType
    );
}
#[test]
fn empty_lists_and_nullable_roles_preserve_presence() {
    for format in formats() {
        for variant in ["empty", "nullable"] {
            let d = parsed(format, GameVersion::Gothic1, variant);
            d.validate(Limits::default()).unwrap();
            let b = d.to_bytes(Limits::default()).unwrap();
            assert_eq!(
                ObjectArchive::from_bytes(&b, d.game, Limits::default())
                    .unwrap()
                    .to_bytes(Limits::default())
                    .unwrap(),
                b
            );
        }
    }
}
#[test]
fn exact_list_and_copied_budgets_include_null_alias_and_orphan() {
    let mut graph = Graph::new().unwrap();
    let p = graph
        .insert(Object::CutscenePoolItem(PoolItem {
            name: b"POOL".to_vec(),
            ..Default::default()
        }))
        .unwrap();
    let m = graph
        .insert(Object::CutsceneManager(Manager {
            pool_items: vec![
                Link::new(b"a".to_vec(), Some(p)),
                Link::new(b"nil".to_vec(), None),
                Link::new(b"alias".to_vec(), Some(p)),
            ],
        }))
        .unwrap();
    graph
        .add_root(Link::new(b"root".to_vec(), Some(m)))
        .unwrap();
    let mut d = ObjectArchive {
        metadata: Metadata::new(Format::BinSafe),
        game: GameVersion::Gothic1,
        graph,
    };
    let copied = d.metadata.archiver.len() + 4 + 1 + 3 + 5 + 4;
    let exact = Limits {
        nodes: 2,
        roots: 1,
        links: 4,
        copied_bytes: copied,
        sequence_items: 0,
        ..Limits::default()
    };
    let b = d.to_bytes(exact).unwrap();
    ObjectArchive::from_bytes(&b, d.game, exact).unwrap();
    for lower in [
        Limits { nodes: 1, ..exact },
        Limits { roots: 0, ..exact },
        Limits { links: 3, ..exact },
        Limits {
            copied_bytes: copied - 1,
            ..exact
        },
    ] {
        assert_eq!(
            d.validate(lower).unwrap_err().kind,
            ErrorKind::LimitExceeded
        );
        assert_eq!(
            ObjectArchive::from_bytes(&b, d.game, lower)
                .unwrap_err()
                .kind,
            ErrorKind::LimitExceeded
        );
    }
    d.graph
        .insert(Object::CutscenePlayer(Player {
            playlists: vec![Link::new(vec![], None)],
            ..Default::default()
        }))
        .unwrap();
    assert_eq!(
        d.validate(Limits { nodes: 3, ..exact }).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    d.validate(Limits {
        nodes: 3,
        links: 5,
        ..exact
    })
    .unwrap();
}
#[test]
fn malformed_counts_names_versions_and_exact_kinds_reject() {
    for format in formats() {
        let enc = match format {
            Format::Ascii => "ascii",
            Format::Binary => "binary",
            Format::BinSafe => "bin_safe",
        };
        for variant in [
            "negative-player",
            "negative-manager",
            "huge-player",
            "huge-manager",
            "wrong-playlist",
            "wrong-pool",
            "wrong-props",
            "wrong-npc",
            "wrong-main",
            "wrong-events",
            "null-block",
            "derived-child",
            "zero-block-count",
            "two-block-count",
            "props-version",
            "context-version",
        ] {
            let bytes = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
                format!("tests/fixtures/synthetic-cutscene-state-invalid-{enc}-{variant}.zen"),
            ))
            .unwrap();
            let e = ObjectArchive::from_bytes(&bytes, GameVersion::Gothic2, Limits::default())
                .unwrap_err();
            assert!(e.offset.is_some(), "{enc}/{variant}: {e:?}");
        }
    }
    let b = wire(Format::Ascii, GameVersion::Gothic1, "normal-props");
    let mut text = String::from_utf8_lossy(&b).into_owned();
    text = text.replace("hasToBeTriggerd", "hasToBeTriggered");
    assert_eq!(
        ObjectArchive::from_bytes(text.as_bytes(), GameVersion::Gothic1, Limits::default())
            .unwrap_err()
            .kind,
        ErrorKind::InvalidData
    );
}
#[test]
fn binary_and_tagged_float_bits_and_enum_width_are_explicit() {
    for format in [Format::Binary, Format::BinSafe] {
        let mut d = parsed(format, GameVersion::Gothic2, "normal-props");
        let id = d.graph.roots()[0].target.unwrap();
        let ObjectMut::CutsceneProps(p) = d.graph.get_mut(id).unwrap() else {
            panic!()
        };
        p.distance = f32::from_bits(0xffc12345);
        p.range = f32::from_bits(1);
        p.run_behavior = 255;
        let b = d.to_bytes(Limits::default()).unwrap();
        let d2 = ObjectArchive::from_bytes(&b, d.game, Limits::default()).unwrap();
        let ObjectRef::CutsceneProps(p) =
            d2.graph.get(d2.graph.roots()[0].target.unwrap()).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            (p.distance.to_bits(), p.range.to_bits(), p.run_behavior),
            (0xffc12345, 1, 255)
        );
        let ObjectMut::CutsceneProps(p) = d.graph.get_mut(id).unwrap() else {
            panic!()
        };
        p.run_behavior = 256;
        if format == Format::Binary {
            assert!(d.to_bytes(Limits::default()).is_err());
        } else {
            assert!(d.to_bytes(Limits::default()).is_ok());
        }
    }
}
#[test]
fn arbitrary_stage_objects_and_material_lexemes_survive() {
    for format in formats() {
        for variant in [
            "stage-material-library",
            "stage-block",
            "stage-atomic-message",
            "stage-pool-manager",
            "stage-props-scene",
            "stage-npc-talent",
            "stage-human-events",
            "stage-move-visual",
        ] {
            let d = parsed(format, GameVersion::Gothic2, variant);
            let b = d.to_bytes(Limits::default()).unwrap();
            let again = ObjectArchive::from_bytes(&b, d.game, Limits::default()).unwrap();
            assert_eq!(again.to_bytes(Limits::default()).unwrap(), b);
        }
    }
}
#[test]
fn representation_and_debug_stay_bounded() {
    assert!(std::mem::size_of::<ObjectRef<'_>>() <= 64);
    let p = Props {
        name: vec![b'x'; 100000],
        ..Default::default()
    };
    assert!(format!("{p:?}").len() < 500);
    let p = Player {
        playlists: vec![Link::new(vec![], None); 10000],
        ..Default::default()
    };
    assert!(format!("{p:?}").len() < 200);
}
