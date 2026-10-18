use zenkit::{ErrorKind, GameVersion, archive::Format, objects::*, vob::*};
fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/synthetic-{name}.zen")),
    )
    .unwrap()
}
fn parse(name: &str, game: GameVersion) -> ObjectArchive {
    ObjectArchive::from_bytes(&bytes(name), game, Limits::default()).unwrap()
}
fn root(d: &ObjectArchive, index: usize) -> VobRef<'_> {
    let ObjectRef::Vob(v) = d.graph.get(d.graph.roots()[index].target.unwrap()).unwrap() else {
        panic!()
    };
    v
}
fn root_mut(d: &mut ObjectArchive, index: usize) -> VobMut<'_> {
    let id = d.graph.roots()[index].target.unwrap();
    let ObjectMut::Vob(v) = d.graph.get_mut(id).unwrap() else {
        panic!()
    };
    v
}
fn container(d: &ObjectArchive) -> &Container {
    let VobKindRef::Container(c) = &root(d, 0).kind() else {
        panic!()
    };
    c
}
fn container_mut(d: &mut ObjectArchive) -> &mut Container {
    let VobKindMut::Container(c) = root_mut(d, 0).into_parts().1 else {
        panic!()
    };
    c
}
fn reopen(d: &ObjectArchive) -> ObjectArchive {
    ObjectArchive::from_bytes(
        &d.to_bytes(Limits::default()).unwrap(),
        d.game,
        Limits::default(),
    )
    .unwrap()
}
fn formats() -> [Format; 3] {
    [Format::Ascii, Format::Binary, Format::BinSafe]
}
fn fresh(format: Format, save: bool) -> ObjectArchive {
    let game = GameVersion::Gothic2;
    let mut graph = Graph::new().unwrap();
    let mut v = Vob::new(game);
    v.base.physics_enabled = false;
    v.base.state = save.then_some(SaveState::default());
    v.kind = VobKind::Container(Container {
        items: save.then(Vec::new),
        ..Default::default()
    });
    let id = graph.insert(Object::Vob(v)).unwrap();
    graph
        .add_root(Link::new(b"root".to_vec(), Some(id)))
        .unwrap();
    let mut metadata = Metadata::new(format);
    metadata.save_game = save;
    ObjectArchive {
        metadata,
        game,
        graph,
    }
}
fn item(d: &mut ObjectArchive) -> ObjectId {
    let mut v = Vob::new(d.game);
    v.base.physics_enabled = false;
    v.base.state = Some(SaveState::default());
    v.kind = VobKind::Item(Item {
        instance: b"ITEM\xff".to_vec(),
        state: Some(ItemState {
            amount: -7,
            flags: i32::MIN,
        }),
    });
    d.graph.insert(Object::Vob(v)).unwrap()
}

#[test]
fn all_independent_schemas_preserve_fields_names_and_saved_identity() {
    for enc in ["ascii", "binary", "bin_safe"] {
        for (g, game) in [(1, GameVersion::Gothic1), (2, GameVersion::Gothic2)] {
            for save in [false, true] {
                for packed in [false, true] {
                    let d = parse(
                        &format!(
                            "vob-mob-{enc}-g{g}-{}-{}",
                            if save { "save" } else { "normal" },
                            if packed { "packed" } else { "unpacked" }
                        ),
                        game,
                    );
                    assert_eq!(d.graph.roots().len(), 9);
                    assert_eq!(d.graph.len(), if save { 13 } else { 10 });
                    for index in 0..9 {
                        let k = &root(&d, index).kind();
                        let m = k.movable().unwrap();
                        assert_eq!(m.focus_name, b"MOB_FOCUS\xff");
                        assert_eq!(m.hp, 123);
                        assert_eq!(m.damage, -7);
                        assert!(m.movable && m.takeable && !m.focus_override && m.destroyed);
                        assert_eq!(m.material, SoundMaterial::CLAY);
                        assert_eq!(m.visual_destroyed, b"BROKEN\xfe.3DS");
                        assert_eq!(m.owner, b"OWNER\xfd");
                        assert_eq!(m.owner_guild, b"GUILD\xfc");
                        assert_eq!(k.interactive().is_some(), index != 0);
                        if let Some(i) = k.interactive() {
                            assert_eq!(i.state_count, -3);
                            assert_eq!(i.target, b"TARGET\xfb");
                            assert_eq!(i.item, b"USE_ITEM\xfa");
                            assert_eq!(i.condition_function, b"COND\xf9");
                            assert_eq!(i.on_state_change_function, b"STATE\xf8");
                            assert!(i.rewind);
                        }
                    }
                    let VobKindRef::Fire(f) = &root(&d, 2).kind() else {
                        panic!()
                    };
                    assert_eq!(f.slot, b"BIP_FIRE\xf7");
                    assert_eq!(f.vob_tree, b"FIRE_TREE\xf6.ZEN");
                    let VobKindRef::Door(door) = &root(&d, 4).kind() else {
                        panic!()
                    };
                    assert!(door.locked);
                    assert_eq!(door.key, b"KEY\xf5");
                    assert_eq!(door.pick_string, b"LRRLL\xf4");
                    let VobKindRef::Container(c) = &root(&d, 3).kind() else {
                        panic!()
                    };
                    assert!(c.locked);
                    assert_eq!(c.key, door.key);
                    assert_eq!(c.pick_string, door.pick_string);
                    assert_eq!(c.contents, b"ITEM_A:2,ITEM_B:3\xf3");
                    assert_eq!(c.items.is_some(), save);
                    if let Some(items) = &c.items {
                        assert_eq!(
                            items.iter().map(|l| l.name.as_slice()).collect::<Vec<_>>(),
                            [b"item0".as_slice(), b"empty", b"itemAlias", b"item1"]
                        );
                        assert_eq!(items[0].target, items[2].target);
                        assert!(items[1].target.is_none());
                        assert_ne!(items[0].target, items[3].target);
                        let ObjectRef::Vob(v) = d.graph.get(items[0].target.unwrap()).unwrap()
                        else {
                            panic!()
                        };
                        let VobKindRef::Item(i) = &v.kind() else {
                            panic!()
                        };
                        assert_eq!(i.instance, b"ITMI\xff_TEST");
                        assert_eq!(
                            i.state,
                            Some(ItemState {
                                amount: 17,
                                flags: 0x402
                            })
                        );
                        let ObjectRef::AiMove(ai) = d
                            .graph
                            .get(v.base().ai.as_ref().unwrap().target.unwrap())
                            .unwrap()
                        else {
                            panic!()
                        };
                        assert_eq!(ai.vob.target, d.graph.roots()[3].target);
                    }
                    let again = reopen(&d);
                    assert_eq!(
                        d.to_bytes(Limits::default()).unwrap(),
                        again.to_bytes(Limits::default()).unwrap()
                    );
                }
            }
        }
    }
}
#[test]
fn count_and_kind_failures_are_explicit() {
    for enc in ["ascii", "binary", "bin_safe"] {
        for (suffix, kind) in [
            ("negative-count", ErrorKind::InvalidData),
            ("huge-count", ErrorKind::LimitExceeded),
            ("wrong-item", ErrorKind::WrongObjectType),
            ("short-items", ErrorKind::InvalidData),
        ] {
            let err = ObjectArchive::from_bytes(
                &bytes(&format!("vob-mob-invalid-{enc}-{suffix}")),
                GameVersion::Gothic2,
                Limits::default(),
            )
            .unwrap_err();
            assert_eq!(err.kind, kind);
            assert!(err.offset.is_some());
        }
        if enc != "binary" {
            assert_eq!(
                ObjectArchive::from_bytes(
                    &bytes(&format!("vob-mob-invalid-{enc}-wrong-field")),
                    GameVersion::Gothic2,
                    Limits::default()
                )
                .unwrap_err()
                .kind,
                ErrorKind::InvalidData
            );
        }
    }
}
#[test]
fn empty_save_and_absent_normal_lists_cannot_be_confused() {
    for format in formats() {
        for save in [false, true] {
            let mut d = fresh(format, save);
            assert_eq!(container(&reopen(&d)).items.is_some(), save);
            container_mut(&mut d).items = if save { None } else { Some(Vec::new()) };
            assert_eq!(
                d.validate(Limits::default()).unwrap_err().kind,
                ErrorKind::InvalidData
            );
            assert!(d.to_bytes(Limits::default()).is_err());
        }
    }
}
#[test]
fn exact_item_subtype_including_orphans_and_foreign_handles() {
    let mut d = fresh(Format::BinSafe, true);
    let owner = d.graph.roots()[0].target.unwrap();
    container_mut(&mut d).items = Some(vec![Link::new(b"self".to_vec(), Some(owner))]);
    assert_eq!(
        d.validate(Limits::default()).unwrap_err().kind,
        ErrorKind::WrongObjectType
    );
    let mut wrong = Vob::new(d.game);
    wrong.base.physics_enabled = false;
    wrong.base.state = Some(SaveState::default());
    let wrong = d.graph.insert(Object::Vob(wrong)).unwrap();
    container_mut(&mut d).items.as_mut().unwrap()[0].target = Some(wrong);
    assert_eq!(
        d.validate(Limits::default()).unwrap_err().kind,
        ErrorKind::WrongObjectType
    );
    let foreign = fresh(Format::BinSafe, true).graph.roots()[0].target;
    container_mut(&mut d).items.as_mut().unwrap()[0].target = foreign;
    assert_eq!(
        d.validate(Limits::default()).unwrap_err().kind,
        ErrorKind::InvalidHandle
    );
    container_mut(&mut d).items = Some(Vec::new());
    let mut orphan = Vob::new(d.game);
    orphan.base.physics_enabled = false;
    orphan.base.state = Some(SaveState::default());
    orphan.kind = VobKind::Container(Container {
        items: Some(vec![Link::new(b"wrong".to_vec(), Some(wrong))]),
        ..Default::default()
    });
    d.graph.insert(Object::Vob(orphan)).unwrap();
    assert_eq!(
        d.validate(Limits::default()).unwrap_err().kind,
        ErrorKind::WrongObjectType
    );
}
#[test]
fn exact_aggregate_links_and_bytes_do_not_double_charge_sequences() {
    for format in formats() {
        let mut d = fresh(format, true);
        let id = item(&mut d);
        container_mut(&mut d).interactive.movable.focus_name = b"focus".to_vec();
        container_mut(&mut d).contents = b"contents".to_vec();
        container_mut(&mut d).items = Some(vec![
            Link::new(b"one".to_vec(), Some(id)),
            Link::new(b"two".to_vec(), Some(id)),
            Link::new(b"nil".to_vec(), None),
        ]);
        let cap = d.metadata.archiver.len() + 4 + 5 + 8 + 5 + 9;
        let exact = Limits {
            links: 4,
            copied_bytes: cap,
            sequence_items: 0,
            ..Limits::default()
        };
        let wire = d.to_bytes(exact).unwrap();
        ObjectArchive::from_bytes(&wire, d.game, exact).unwrap();
        for limits in [
            Limits { links: 3, ..exact },
            Limits {
                copied_bytes: cap - 1,
                ..exact
            },
        ] {
            assert_eq!(
                d.validate(limits).unwrap_err().kind,
                ErrorKind::LimitExceeded
            );
            assert_eq!(
                ObjectArchive::from_bytes(&wire, d.game, limits)
                    .unwrap_err()
                    .kind,
                ErrorKind::LimitExceeded
            );
        }
        let mut orphan = Vob::new(d.game);
        orphan.base.physics_enabled = false;
        orphan.base.state = Some(SaveState::default());
        orphan.kind = VobKind::Container(Container {
            items: Some(vec![Link::new(Vec::new(), None)]),
            ..Default::default()
        });
        d.graph.insert(Object::Vob(orphan)).unwrap();
        assert_eq!(
            d.validate(exact).unwrap_err().kind,
            ErrorKind::LimitExceeded
        );
        d.validate(Limits { links: 5, ..exact }).unwrap();
    }
}
#[test]
fn unknown_materials_and_signed_sentinels_are_preserved() {
    for format in formats() {
        let mut d = fresh(format, false);
        let i = &mut container_mut(&mut d).interactive;
        i.state_count = i32::MIN;
        i.movable.hp = -1;
        i.movable.damage = i32::MAX;
        i.movable.material = SoundMaterial::from_raw(254);
        let again = reopen(&d);
        assert_eq!(container(&again), container(&d));
        container_mut(&mut d).interactive.movable.material = SoundMaterial::from_raw(u32::MAX);
        if format == Format::Binary {
            assert_eq!(
                d.to_bytes(Limits::default()).unwrap_err().kind,
                ErrorKind::Unsupported
            );
        } else {
            assert_eq!(container(&reopen(&d)), container(&d));
        }
    }
    assert_eq!(u32::from(SoundMaterial::GLASS), 5);
    assert_eq!(SoundMaterial::from(3), SoundMaterial::LEATHER);
}
#[test]
fn boolean_ff_is_semantic_true_and_rewrites_canonically() {
    for enc in ["ascii", "binary", "bin_safe"] {
        for (g, game) in [(1, GameVersion::Gothic1), (2, GameVersion::Gothic2)] {
            let wire = bytes(&format!("vob-mob-{enc}-g{g}-boolean-ff"));
            let d = ObjectArchive::from_bytes(&wire, game, Limits::default()).unwrap();
            let c = container(&d);
            assert!(
                c.locked
                    && c.interactive.rewind
                    && c.interactive.movable.takeable
                    && c.interactive.movable.destroyed
            );
            let rewritten = d.to_bytes(Limits::default()).unwrap();
            assert_ne!(wire, rewritten);
            let again = reopen(&d);
            assert_eq!(container(&again).interactive, c.interactive);
        }
    }
}
#[test]
fn raw_text_and_base_tail_retain_bytes_or_refuse_conversion() {
    let mut d = parse("vob-mob-bin_safe-raw-text", GameVersion::Gothic2);
    assert_eq!(
        container(&d).interactive.movable.focus_name,
        b" \tFOCUS\0\n\r\xff "
    );
    assert_eq!(container(&reopen(&d)).contents, b" \tITEM\0\n\r\xfe ");
    d.metadata.format = Format::Binary;
    assert_eq!(
        d.to_bytes(Limits::default()).unwrap_err().kind,
        ErrorKind::InvalidData
    );
    for enc in ["ascii", "bin_safe"] {
        let mut d = parse(&format!("vob-mob-{enc}-raw"), GameVersion::Gothic2);
        assert_eq!(
            root(&reopen(&d), 0).base().packing,
            root(&d, 0).base().packing
        );
        d.metadata.format = Format::Binary;
        assert!(d.to_bytes(Limits::default()).is_err());
    }
}
#[test]
fn aliases_survive_shared_item_mutation_and_repair() {
    let mut d = parse("vob-mob-bin_safe-g2-save-packed", GameVersion::Gothic2);
    let VobKindRef::Container(c) = &root(&d, 3).kind() else {
        panic!()
    };
    let id = c.items.as_ref().unwrap()[0].target.unwrap();
    let ObjectMut::Vob(mut v) = d.graph.get_mut(id).unwrap() else {
        panic!()
    };
    let VobKindMut::Item(i) = v.kind_mut() else {
        panic!()
    };
    i.instance = b"MUTATED".to_vec();
    i.state.as_mut().unwrap().flags = -999;
    let again = reopen(&d);
    let VobKindRef::Container(c) = &root(&again, 3).kind() else {
        panic!()
    };
    let items = c.items.as_ref().unwrap();
    assert_eq!(items[0].target, items[2].target);
    let ObjectRef::Vob(v) = again.graph.get(items[2].target.unwrap()).unwrap() else {
        panic!()
    };
    let VobKindRef::Item(i) = &v.kind() else {
        panic!()
    };
    assert_eq!(i.instance, b"MUTATED");
    assert_eq!(i.state.unwrap().flags, -999);
}
#[test]
fn complete_single_root_requires_all_child_records_and_exact_version() {
    let wire = bytes("vob-mob-binary-g2-boolean-ff");
    for end in 0..wire.len() {
        if let Ok(prefix) =
            ObjectArchive::from_bytes(&wire[..end], GameVersion::Gothic2, Limits::default())
        {
            // Object-count metadata is advisory; an empty complete header is a
            // valid zero-root archive, but no partial root may be published.
            assert!(prefix.graph.roots().is_empty(), "prefix{end}");
        }
    }
    let mut tagged = bytes("vob-mob-ascii-single-3");
    let pos = tagged.windows(5).position(|w| w == b"64513").unwrap();
    tagged[pos..pos + 5].copy_from_slice(b"64512");
    assert_eq!(
        ObjectArchive::from_bytes(&tagged, GameVersion::Gothic2, Limits::default())
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
}
#[test]
fn compact_debug_and_borrowed_common_views() {
    let mut d = fresh(Format::BinSafe, false);
    let k = &mut root_mut(&mut d, 0).into_parts().1;
    k.movable_mut().unwrap().focus_name = vec![b'x'; 100_000];
    k.interactive_mut().unwrap().target = vec![b'y'; 100_000];
    assert!(format!("{k:?}").len() < 1500);
    assert_eq!(k.movable().unwrap().focus_name.len(), 100_000);
    assert!(std::mem::size_of::<ObjectRef<'_>>() <= 64);
}
#[test]
fn deep_container_item_ai_cycles_and_wide_aliases_use_small_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            for format in formats() {
                let mut d = fresh(format, true);
                let first = d.graph.roots()[0].target.unwrap();
                let mut current = first;
                for level in 0..42 {
                    let it = item(&mut d);
                    let next = if level == 41 {
                        first
                    } else {
                        let mut v = Vob::new(d.game);
                        v.base.physics_enabled = false;
                        v.base.state = Some(SaveState::default());
                        v.kind = VobKind::Container(Container {
                            items: Some(Vec::new()),
                            ..Default::default()
                        });
                        d.graph.insert(Object::Vob(v)).unwrap()
                    };
                    let ai = d
                        .graph
                        .insert(Object::AiMove(AiMove {
                            vob: Link::new(b"vob".to_vec(), Some(next)),
                            owner: Link::new(b"owner".to_vec(), None),
                        }))
                        .unwrap();
                    let ObjectMut::Vob(mut v) = d.graph.get_mut(it).unwrap() else {
                        panic!()
                    };
                    v.base_mut().ai = Some(Link::new(b"ai".to_vec(), Some(ai)));
                    let ObjectMut::Vob(mut v) = d.graph.get_mut(current).unwrap() else {
                        panic!()
                    };
                    let VobKindMut::Container(c) = v.kind_mut() else {
                        panic!()
                    };
                    c.items = Some(vec![Link::new(b"item".to_vec(), Some(it))]);
                    current = next;
                }
                let again = reopen(&d);
                assert_eq!(again.graph.len(), 126);
                let ObjectMut::Vob(mut v) = d.graph.get_mut(first).unwrap() else {
                    panic!()
                };
                let VobKindMut::Container(c) = v.kind_mut() else {
                    panic!()
                };
                let link = c.items.as_ref().unwrap()[0].clone();
                c.items = Some(vec![link; 20_000]);
                let again = reopen(&d);
                assert_eq!(container(&again).items.as_ref().unwrap().len(), 20_000);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
