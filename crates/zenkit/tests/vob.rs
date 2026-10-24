use zenkit::{ErrorKind, GameVersion, archive::Format, objects::*, values::Vec3, vob::*};
fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/synthetic-vob-{name}.zen")),
    )
    .unwrap()
}
fn parse(name: &str, game: GameVersion) -> ObjectArchive {
    ObjectArchive::from_bytes(&fixture(name), game, Limits::default()).unwrap()
}
fn root(d: &ObjectArchive) -> ObjectId {
    d.graph.roots()[0].target.unwrap()
}
fn vob(d: &ObjectArchive) -> VobRef<'_> {
    let ObjectRef::Vob(v) = d.graph.get(root(d)).unwrap() else {
        panic!()
    };
    v
}
fn vob_mut(d: &mut ObjectArchive) -> VobMut<'_> {
    let id = root(d);
    let ObjectMut::Vob(v) = d.graph.get_mut(id).unwrap() else {
        panic!()
    };
    v
}
fn formats() -> [Format; 3] {
    [Format::Ascii, Format::Binary, Format::BinSafe]
}
fn reopen(d: &ObjectArchive) -> ObjectArchive {
    ObjectArchive::from_bytes(
        &d.to_bytes(Limits::default()).unwrap(),
        d.game,
        Limits::default(),
    )
    .unwrap()
}

fn decal_wire(format: Format, version: u16, save: bool, extra: bool) -> Vec<u8> {
    use zenkit::archive::{self, Value};
    let mut header = archive::Header::new(format);
    header.save_game = save;
    let mut w = archive::Writer::new(header, archive::Limits::default()).unwrap();
    w.begin_object(b"visual", b"zCDecal", version).unwrap();
    for (name, value) in [
        (
            b"name".as_slice(),
            Value::String(b"DECAL\xfd.TGA".as_slice().into()),
        ),
        (b"decalDim", Value::Vec2([32.5, 64.25].into())),
        (b"decalOffset", Value::Vec2([-1.25, 2.5].into())),
        (b"decal2Sided", Value::Bool(true)),
        (b"decalAlphaFunc", Value::Enum(3)),
        (b"decalTexAniFPS", Value::Float(-0.0)),
        (b"decalAlphaWeight", Value::Byte(173)),
        (b"ignoreDayLight", Value::Bool(true)),
    ] {
        w.write(name, value).unwrap();
    }
    if extra {
        w.write(b"unexpected", Value::Int(42)).unwrap();
    }
    w.end_object().unwrap();
    w.finish().unwrap()
}

#[test]
fn complete_g2_decal_corpus_versions_preserve_their_payload_owned_identifier() {
    for format in formats() {
        for version in [0, 53505, 64513] {
            for save in [false, true] {
                let input = decal_wire(format, version, save, false);
                let mut d =
                    ObjectArchive::from_bytes(&input, GameVersion::Gothic2, Limits::default())
                        .unwrap();
                let id = root(&d);
                let ObjectRef::Visual(visual) = d.graph.get(id).unwrap() else {
                    panic!()
                };
                assert_eq!(visual.version, version);
                let VisualKind::Decal(decal) = &visual.kind else {
                    panic!()
                };
                assert_eq!(decal.name, b"DECAL\xfd.TGA");
                assert_eq!(decal.dimension, [32.5, 64.25].into());
                assert_eq!(decal.offset, [-1.25, 2.5].into());
                assert!(decal.two_sided);
                assert_eq!(decal.alpha.raw(), 3);
                assert_eq!(decal.fps.to_bits(), (-0.0f32).to_bits());
                assert_eq!(
                    decal.version,
                    DecalVersion::Gothic2(Gothic2Decal {
                        alpha_weight: 173,
                        ignore_daylight: true
                    })
                );
                d.graph.clear_record_version(id).unwrap();
                assert_eq!(d.graph.record_version(id, d.game).unwrap(), version);
                assert_eq!(d.to_bytes(Limits::default()).unwrap(), input);
            }
        }
        for version in [53504, 64512, 64514] {
            assert_eq!(
                ObjectArchive::from_bytes(
                    &decal_wire(format, version, false, false),
                    GameVersion::Gothic2,
                    Limits::default()
                )
                .unwrap_err()
                .kind,
                ErrorKind::Unsupported
            );
        }
        for version in [53505, 64513] {
            assert_eq!(
                ObjectArchive::from_bytes(
                    &decal_wire(format, version, false, false),
                    GameVersion::Gothic1,
                    Limits::default()
                )
                .unwrap_err()
                .kind,
                ErrorKind::Unsupported
            );
            assert_eq!(
                ObjectArchive::from_bytes(
                    &decal_wire(format, version, false, true),
                    GameVersion::Gothic2,
                    Limits::default()
                )
                .unwrap_err()
                .kind,
                ErrorKind::Unsupported
            );
        }
    }
    let mut d = fresh(GameVersion::Gothic2, Format::BinSafe);
    let orphan = d
        .graph
        .insert(Object::Visual(Visual {
            version: 64513,
            kind: VisualKind::Decal(Decal::new(GameVersion::Gothic2)),
        }))
        .unwrap();
    d.validate(Limits::default()).unwrap();
    let ObjectMut::Visual(v) = d.graph.get_mut(orphan).unwrap() else {
        panic!()
    };
    v.kind = VisualKind::Decal(Decal::new(GameVersion::Gothic1));
    assert_eq!(
        d.validate(Limits::default()).unwrap_err().kind,
        ErrorKind::Unsupported
    );
}
fn fresh(game: GameVersion, format: Format) -> ObjectArchive {
    let mut graph = Graph::new().unwrap();
    let id = graph.insert(Object::Vob(Vob::new(game))).unwrap();
    graph
        .add_root(Link::new(b"root".to_vec(), Some(id)))
        .unwrap();
    ObjectArchive {
        metadata: Metadata::new(format),
        game,
        graph,
    }
}

#[test]
fn complete_fields_across_games_modes_encodings_and_layouts() {
    for format in ["ascii", "binary", "bin_safe"] {
        for (game, g) in [(GameVersion::Gothic1, 1), (GameVersion::Gothic2, 2)] {
            for packed in [false, true] {
                for save in [false, true] {
                    let d = parse(
                        &format!(
                            "{format}-g{g}-{}-{}-base",
                            if packed { "packed" } else { "unpacked" },
                            if save { "save" } else { "normal" }
                        ),
                        game,
                    );
                    let v = vob(&d);
                    assert_eq!(v.base().position, Vec3::from([7.25, -8.5, 9.75]));
                    assert_eq!(v.base().rotation[0], Vec3::from([1., 4., 7.]));
                    assert_eq!(v.base().bbox.min, Vec3::from([-1., -2., -3.]));
                    assert_eq!(v.base().preset.as_deref(), Some(b"PRESET\xff".as_slice()));
                    assert_eq!(v.base().name.as_deref(), Some(b"VOB\xfe".as_slice()));
                    assert!(
                        v.base().show_visual && v.base().collision_dynamic && v.base().static_vob
                    );
                    assert!(!v.base().collision_static);
                    assert_eq!(v.base().camera_alignment, CameraAlignment::FULL);
                    assert_eq!(v.base().shadow, Shadow::BLOB);
                    assert_eq!(v.base().physics_enabled, save || !packed);
                    if let VobVersion::Gothic2(fields) = v.base().version {
                        assert_eq!(fields.animation.raw(), 3);
                        assert_eq!(fields.bias, 17);
                        assert_eq!(fields.strength, 1.25);
                        assert_eq!(fields.far_clip, 0.75);
                        assert!(fields.ambient);
                    }
                    assert_eq!(v.base().state.is_some(), save);
                    if let Some(s) = v.base().state {
                        assert_eq!(s.sleep, 2);
                        assert_eq!(s.next_timer, 12.5);
                        let b = s.rigid_body.unwrap();
                        assert_eq!(b.velocity, Vec3::from([1.5, -2.25, 3.75]));
                        assert_eq!(b.mode, 7);
                        assert!(b.gravity);
                        assert_eq!(b.gravity_scale, 0.625);
                        assert_eq!(b.slide, Vec3::from([-4.5, 5.25, -6.75]));
                    }
                    let ObjectRef::Visual(Visual {
                        kind: VisualKind::Decal(decal),
                        ..
                    }) = d
                        .graph
                        .get(v.base().visual.as_ref().unwrap().target.unwrap())
                        .unwrap()
                    else {
                        panic!()
                    };
                    assert_eq!(decal.name, b"DECAL\xfd.TGA");
                    assert_eq!(decal.dimension.x, 32.5);
                    assert_eq!(decal.offset.y, 2.5);
                    assert_eq!(decal.alpha.raw(), 3);
                    assert!(decal.two_sided);
                    assert_eq!(decal.fps, 12.5);
                    let again = reopen(&d);
                    assert_eq!(d.metadata, again.metadata);
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
fn packed_presence_reserved_tail_and_noncanonical_discriminator_survive() {
    for format in ["ascii", "binary", "bin_safe"] {
        let d = parse(&format!("{format}-presence"), GameVersion::Gothic2);
        let v = vob(&d);
        assert_eq!(v.base().preset, Some(Vec::new()));
        assert_eq!(v.base().name, None);
        assert_eq!(v.base().visual_name, Some(Vec::new()));
        assert_eq!(v.base().visual.as_ref().unwrap().target, None);
        assert_eq!(v.base().ai.as_ref().unwrap().target, None);
        assert_eq!(v.base().event_manager, None);
        assert!(matches!(
            v.base().packing,
            Packing::Packed { reserved: 32, .. }
        ));
        assert_eq!(v.packed_flags().unwrap().unwrap().1 & 32, 32);
        assert_eq!(vob(&reopen(&d)).base().packing, v.base().packing);
        let d = parse(&format!("{format}-reserved"), GameVersion::Gothic2);
        let Packing::Packed {
            marker,
            reserved,
            tail,
        } = &vob(&d).base().packing
        else {
            panic!()
        };
        assert_eq!((*marker, *reserved), (7, 128));
        assert_eq!(
            tail.as_slice(),
            if format == "binary" {
                b"".as_slice()
            } else {
                b"\xde\xad\xbe\xef"
            }
        );
        assert_eq!(vob(&reopen(&d)).base().packing, vob(&d).base().packing);
    }
}
#[test]
fn raw_names_and_separate_decal_names_are_never_propagated() {
    for name in ["bin_safe-raw", "bin_safe-decal-name"] {
        let d = parse(name, GameVersion::Gothic2);
        let v = vob(&d);
        let target = v.base().visual.as_ref().unwrap().target.unwrap();
        let ObjectRef::Visual(Visual {
            kind: VisualKind::Decal(decal),
            ..
        }) = d.graph.get(target).unwrap()
        else {
            panic!()
        };
        assert_ne!(v.base().visual_name.as_deref(), Some(decal.name.as_slice()));
        let again = reopen(&d);
        assert_eq!(
            d.to_bytes(Limits::default()).unwrap(),
            again.to_bytes(Limits::default()).unwrap()
        );
    }
}
#[test]
fn all_marker_classes_and_empty_visual_versions_survive() {
    for name in ["bin_safe-markers", "bin_safe-visuals"] {
        let d = parse(name, GameVersion::Gothic2);
        assert!(d.graph.roots().len() >= 5);
        let again = reopen(&d);
        assert_eq!(
            d.to_bytes(Limits::default()).unwrap(),
            again.to_bytes(Limits::default()).unwrap()
        );
    }
    let d = parse("bin_safe-visuals", GameVersion::Gothic2);
    assert!(d.graph.objects().any(|(_, o)| matches!(
        o,
        ObjectRef::Visual(Visual {
            version: 64513,
            kind: VisualKind::ProgressiveMesh
        })
    )));
}
#[test]
fn aux_fields_cycles_and_save_suffix_are_not_consumed_by_references() {
    for format in ["ascii", "binary", "bin_safe"] {
        let d = parse(&format!("{format}-aux"), GameVersion::Gothic2);
        let v = vob(&d);
        let ObjectRef::AiMove(ai) = d
            .graph
            .get(v.base().ai.as_ref().unwrap().target.unwrap())
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(ai.vob.target, Some(root(&d)));
        assert_eq!(ai.owner.target, None);
        let ObjectRef::EventManager(em) = d
            .graph
            .get(v.base().event_manager.as_ref().unwrap().target.unwrap())
            .unwrap()
        else {
            panic!()
        };
        assert!(em.cleared && !em.active);
        assert_eq!(em.cutscene.target, None);
        let ObjectRef::AiHuman(h) = d.graph.get(d.graph.roots()[1].target.unwrap()).unwrap() else {
            panic!()
        };
        assert_eq!(h.water_level, 2);
        assert_eq!(h.floor_y, -10.5);
        assert_eq!(h.fall_start_y, 8.75);
        assert_eq!(h.walk_mode, 3);
        assert_eq!(h.weapon_mode, 4);
        assert_eq!(h.weapon_mode_last, 5);
        assert_eq!(h.weapon_mode_select, 6);
        assert!(h.change_weapon);
        assert_eq!(h.action_mode, 7);
        assert_eq!(v.base().state.unwrap().sleep, 2);
        let again = reopen(&d);
        assert_eq!(
            d.to_bytes(Limits::default()).unwrap(),
            again.to_bytes(Limits::default()).unwrap()
        );
    }
}
#[test]
fn multiple_roots_aliases_and_shared_visual_identity_survive_mutation() {
    let mut d = parse("bin_safe-shared", GameVersion::Gothic2);
    assert_eq!(d.graph.roots()[0].target, d.graph.roots()[2].target);
    assert_eq!(d.graph.roots()[3].target, None);
    let visual = vob(&d).base().visual.as_ref().unwrap().target.unwrap();
    let ObjectRef::Vob(other) = d.graph.get(d.graph.roots()[1].target.unwrap()).unwrap() else {
        panic!()
    };
    assert_eq!(other.base().visual.as_ref().unwrap().target, Some(visual));
    let ObjectMut::Visual(Visual {
        kind: VisualKind::Decal(decal),
        ..
    }) = d.graph.get_mut(visual).unwrap()
    else {
        panic!()
    };
    decal.name = b"edited child".to_vec();
    let again = reopen(&d);
    assert_eq!(again.graph.len(), 3);
    assert_eq!(
        vob(&again).base().visual_name.as_deref(),
        Some(b"DECAL\xfd.TGA".as_slice())
    );
}
#[test]
fn packed_draft_ranges_reserved_collisions_and_tail_conversion_refuse_loss() {
    for change in 0..7 {
        let mut d = fresh(GameVersion::Gothic2, Format::Binary);
        let mut v = vob_mut(&mut d);
        match change {
            0 => {
                v.base_mut().packing = Packing::Packed {
                    marker: 0,
                    reserved: 0,
                    tail: vec![],
                }
            }
            1 => {
                v.base_mut().packing = Packing::Packed {
                    marker: 1,
                    reserved: 1,
                    tail: vec![],
                }
            }
            2 => v.base_mut().camera_alignment = CameraAlignment::from_raw(4),
            3 => v.base_mut().shadow = Shadow::from_raw(4),
            4 => {
                let VobVersion::Gothic2(g) = &mut v.base_mut().version else {
                    panic!()
                };
                g.bias = 32
            }
            5 => {
                let VobVersion::Gothic2(g) = &mut v.base_mut().version else {
                    panic!()
                };
                g.animation = AnimationMode::from_raw(4)
            }
            _ => {
                v.base_mut().packing = Packing::Packed {
                    marker: 1,
                    reserved: 0,
                    tail: vec![1],
                }
            }
        }
        assert!(d.to_bytes(Limits::default()).is_err());
    }
    let mut d = fresh(GameVersion::Gothic1, Format::BinSafe);
    vob_mut(&mut d).base_mut().packing = Packing::Packed {
        marker: -7,
        reserved: 128,
        tail: vec![1, 2],
    };
    assert_eq!(vob(&reopen(&d)).base().packing, vob(&d).base().packing);
}
#[test]
fn unpacked_and_save_invariants_reject_missing_or_invented_fields() {
    for change in 0..7 {
        let mut d = parse("binary-g2-unpacked-save-base", GameVersion::Gothic2);
        let mut v = vob_mut(&mut d);
        match change {
            0 => v.base_mut().preset = None,
            1 => v.base_mut().name = None,
            2 => v.base_mut().visual = None,
            3 => v.base_mut().ai = None,
            4 => v.base_mut().event_manager = Some(Link::default()),
            5 => v.base_mut().physics_enabled = false,
            _ => v.base_mut().state.as_mut().unwrap().rigid_body = None,
        }
        assert_eq!(
            d.validate(Limits::default()).unwrap_err().kind,
            ErrorKind::InvalidData
        );
    }
    let mut d = fresh(GameVersion::Gothic2, Format::Binary);
    d.metadata.save_game = true;
    assert!(d.validate(Limits::default()).is_err());
    vob_mut(&mut d).base_mut().state = Some(SaveState::default());
    assert!(d.validate(Limits::default()).is_err());
    vob_mut(&mut d).base_mut().physics_enabled = false;
    d.validate(Limits::default()).unwrap();
    vob_mut(&mut d).base_mut().packing = Packing::Packed {
        marker: 1,
        reserved: 32,
        tail: vec![],
    };
    assert!(d.validate(Limits::default()).is_err());
}
#[test]
fn normal_physics_true_needs_no_body_and_has_no_native_writer_loss() {
    for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
        for format in formats() {
            let d = fresh(game, format);
            let again = reopen(&d);
            assert!(vob(&again).base().physics_enabled);
            assert!(vob(&again).base().state.is_none());
        }
    }
}
#[test]
fn physics_disabled_saves_have_no_rigid_body_in_either_game() {
    for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
        for format in formats() {
            let mut d = fresh(game, format);
            d.metadata.save_game = true;
            let mut v = vob_mut(&mut d);
            v.base_mut().physics_enabled = false;
            v.base_mut().state = Some(SaveState {
                sleep: 9,
                next_timer: -2.5,
                rigid_body: None,
            });
            let again = reopen(&d);
            assert!(!vob(&again).base().physics_enabled);
            assert_eq!(
                vob(&again).base().state,
                Some(SaveState {
                    sleep: 9,
                    next_timer: -2.5,
                    rigid_body: None
                })
            );
        }
    }
}
#[test]
fn packed_payload_preflight_accounts_for_hex_expansion_and_tagged_lengths() {
    for (format, limit) in [
        (Format::Ascii, 178),
        (Format::Binary, 83),
        (Format::BinSafe, 83),
    ] {
        let d = fresh(GameVersion::Gothic2, format);
        let mut limits = Limits::default();
        limits.archive.field_bytes = limit;
        let bytes = d.to_bytes(limits).unwrap();
        ObjectArchive::from_bytes(&bytes, d.game, limits).unwrap();
        limits.archive.field_bytes -= 1;
        assert_eq!(
            d.to_bytes(limits).unwrap_err().kind,
            ErrorKind::LimitExceeded
        );
        assert!(vob(&d).base().physics_enabled);
    }
    let mut d = fresh(GameVersion::Gothic2, Format::BinSafe);
    vob_mut(&mut d).base_mut().packing = Packing::Packed {
        marker: 1,
        reserved: 0,
        tail: vec![42; 65535 - 83],
    };
    reopen(&d);
    let Packing::Packed { tail, .. } = &mut vob_mut(&mut d).into_parts().0.packing else {
        panic!()
    };
    tail.push(42);
    assert_eq!(
        d.to_bytes(Limits::default()).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
}
#[test]
fn unknown_unpacked_enums_survive_but_binary_range_is_checked() {
    let mut d = parse("bin_safe-g2-unpacked-normal-base", GameVersion::Gothic2);
    vob_mut(&mut d).base_mut().camera_alignment = CameraAlignment::from_raw(300);
    vob_mut(&mut d).base_mut().shadow = Shadow::from_raw(301);
    if let VobVersion::Gothic2(g) = &mut vob_mut(&mut d).base_mut().version {
        g.animation = AnimationMode::from_raw(302);
        g.bias = -44;
    }
    let again = reopen(&d);
    assert_eq!(vob(&again).base().camera_alignment.raw(), 300);
    assert_eq!(vob(&again).base().version, vob(&d).base().version);
    d.metadata.format = Format::Binary;
    assert_eq!(
        d.to_bytes(Limits::default()).unwrap_err().kind,
        ErrorKind::Unsupported
    );
}
#[test]
fn game_and_version_checks_include_orphans() {
    let mut d = fresh(GameVersion::Gothic1, Format::Binary);
    d.graph
        .insert(Object::Vob(Vob::new(GameVersion::Gothic2)))
        .unwrap();
    assert!(d.validate(Limits::default()).is_err());
    let mut d = fresh(GameVersion::Gothic1, Format::Binary);
    d.graph
        .insert(Object::Visual(Visual::new(VisualKind::Decal(Decal::new(
            GameVersion::Gothic2,
        )))))
        .unwrap();
    assert!(d.validate(Limits::default()).is_err());
    let mut d = fresh(GameVersion::Gothic1, Format::Binary);
    d.graph
        .insert(Object::Visual(Visual {
            version: 1,
            kind: VisualKind::Decal(Decal::new(GameVersion::Gothic1)),
        }))
        .unwrap();
    assert_eq!(
        d.validate(Limits::default()).unwrap_err().kind,
        ErrorKind::Unsupported
    );
}
#[test]
fn child_kinds_foreign_handles_and_unavailable_dependencies_reject() {
    let mut d = fresh(GameVersion::Gothic1, Format::BinSafe);
    let own = root(&d);
    vob_mut(&mut d).base_mut().visual = Some(Link::new(b"visual".to_vec(), Some(own)));
    assert_eq!(
        d.validate(Limits::default()).unwrap_err().kind,
        ErrorKind::WrongObjectType
    );
    let other = fresh(GameVersion::Gothic1, Format::BinSafe);
    vob_mut(&mut d).base_mut().visual.as_mut().unwrap().target = Some(root(&other));
    assert_eq!(
        d.validate(Limits::default()).unwrap_err().kind,
        ErrorKind::InvalidHandle
    );
    vob_mut(&mut d).base_mut().visual = None;
    for node in [
        Object::AiMove(AiMove {
            owner: Link::new(vec![], Some(own)),
            ..Default::default()
        }),
        Object::AiHuman(AiHuman {
            npc: Link::new(vec![], Some(own)),
            ..Default::default()
        }),
        Object::EventManager(EventManager {
            cutscene: Link::new(vec![], Some(own)),
            ..Default::default()
        }),
    ] {
        let expected = ErrorKind::WrongObjectType;
        let id = d.graph.insert(node).unwrap();
        assert_eq!(d.validate(Limits::default()).unwrap_err().kind, expected);
        d.graph
            .replace(id, Object::Visual(Visual::new(VisualKind::Mesh)))
            .unwrap();
    }
}
#[test]
fn malformed_names_versions_extra_fields_and_unknown_classes_have_offsets() {
    let bytes = fixture("ascii-g1-packed-normal-base");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("12289"));
    for (old, new) in [
        (b"12289".as_slice(), b"12288".as_slice()),
        (b"zCVob ", b"zCVox "),
        (b"pack=int:", b"pock=int:"),
        (b"decalDim=", b"decalDum="),
    ] {
        let p = bytes.windows(old.len()).position(|v| v == old).unwrap();
        let mut broken = bytes.clone();
        broken.splice(p..p + old.len(), new.iter().copied());
        let err = ObjectArchive::from_bytes(&broken, GameVersion::Gothic1, Limits::default())
            .unwrap_err();
        assert!(err.offset.is_some());
    }
    let mut broken = bytes.clone();
    let last = broken.windows(3).rposition(|v| v == b"[]\n").unwrap();
    broken.splice(last..last, b"extra=int:7\n".iter().copied());
    assert_eq!(
        ObjectArchive::from_bytes(&broken, GameVersion::Gothic1, Limits::default())
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
    assert_eq!(
        ObjectArchive::from_bytes(&bytes, GameVersion::Gothic2, Limits::default())
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
}
#[test]
fn all_truncations_inside_a_single_object_fail() {
    for format in ["ascii", "binary", "bin_safe"] {
        let bytes = fixture(&format!("{format}-g2-packed-save-base"));
        let start = if format == "binary" {
            bytes.windows(4).position(|v| v == b"vob\0").unwrap() - 10
        } else {
            bytes.windows(5).position(|v| v == b"[vob ").unwrap()
        };
        for n in start + 1..bytes.len() - 1 {
            assert!(
                ObjectArchive::from_bytes(&bytes[..n], GameVersion::Gothic2, Limits::default())
                    .is_err(),
                "{format} prefix {n}"
            );
        }
    }
}
#[test]
fn cumulative_link_and_byte_budgets_match_read_and_write() {
    let d = parse("bin_safe-aux", GameVersion::Gothic2);
    let bytes = d.to_bytes(Limits::default()).unwrap();
    let mut lo = 0;
    let mut hi = 4096;
    while lo < hi {
        let m = (lo + hi) / 2;
        let l = Limits {
            copied_bytes: m,
            ..Limits::default()
        };
        if d.validate(l).is_ok() {
            hi = m
        } else {
            lo = m + 1
        }
    }
    let exact = Limits {
        copied_bytes: lo,
        ..Limits::default()
    };
    d.to_bytes(exact).unwrap();
    ObjectArchive::from_bytes(&bytes, d.game, exact).unwrap();
    let short = Limits {
        copied_bytes: lo - 1,
        ..Limits::default()
    };
    assert_eq!(
        d.to_bytes(short).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(
        ObjectArchive::from_bytes(&bytes, d.game, short)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    // root VOB + visual + AI + AI.vob reference + AI.owner null + manager +
    // manager.cutscene null + human root + human.npc null.
    let exact = Limits {
        links: 9,
        ..Limits::default()
    };
    d.to_bytes(exact).unwrap();
    ObjectArchive::from_bytes(&bytes, d.game, exact).unwrap();
    let short = Limits { links: 8, ..exact };
    assert!(d.to_bytes(short).is_err());
    assert!(ObjectArchive::from_bytes(&bytes, d.game, short).is_err());
}
#[test]
fn packed_and_binary_float_bits_survive_without_normalization() {
    for format in formats() {
        let mut d = fresh(GameVersion::Gothic2, format);
        let mut v = vob_mut(&mut d);
        v.base_mut().position = Vec3::from([f32::from_bits(0x7fc01234), -0.0, f32::from_bits(1)]);
        v.base_mut().rotation[1].z = f32::NEG_INFINITY;
        let VobVersion::Gothic2(g) = &mut v.base_mut().version else {
            panic!()
        };
        g.strength = f32::from_bits(0x7fa01234);
        let again = reopen(&d);
        assert_eq!(vob(&again).base().position.x.to_bits(), 0x7fc01234);
        assert_eq!(vob(&again).base().position.y.to_bits(), 0x80000000);
        assert_eq!(vob(&again).base().position.z.to_bits(), 1);
        assert_eq!(
            vob(&again).base().rotation[1].z.to_bits(),
            f32::NEG_INFINITY.to_bits()
        );
        let VobVersion::Gothic2(g) = vob(&again).base().version else {
            panic!()
        };
        assert_eq!(g.strength.to_bits(), 0x7fa01234);
    }
}
#[test]
fn compact_debug_and_default_state() {
    let mut v = Vob::new(GameVersion::Gothic2);
    v.base.name = Some(vec![b'X'; 100_000]);
    v.base.packing = Packing::Packed {
        marker: 1,
        reserved: 0,
        tail: vec![1; 100_000],
    };
    assert!(format!("{v:?}").len() < 1500);
    let mut d = Decal::new(GameVersion::Gothic1);
    d.name = vec![b'X'; 100_000];
    assert!(format!("{d:?}").len() < 500);
    assert!(v.base.physics_enabled);
    assert!(v.base.state.is_none());
    assert_eq!(v.base.rotation, zenkit::values::Mat3::IDENTITY);
}
#[test]
fn iterative_vob_ai_cycles_and_suffixes_fit_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            for format in formats() {
                let mut d = fresh(GameVersion::Gothic2, format);
                d.metadata.save_game = true;
                let mut current = root(&d);
                for i in 0..63 {
                    let next = d.graph.insert(Object::Vob(Vob::new(d.game))).unwrap();
                    let ai = d
                        .graph
                        .insert(Object::AiMove(AiMove {
                            vob: Link::new(b"vob".to_vec(), Some(next)),
                            owner: Link::default(),
                        }))
                        .unwrap();
                    let ObjectMut::Vob(mut v) = d.graph.get_mut(current).unwrap() else {
                        panic!()
                    };
                    v.base_mut().ai = Some(Link::new(b"ai".to_vec(), Some(ai)));
                    v.base_mut().state = Some(SaveState {
                        sleep: i,
                        next_timer: i as f32,
                        rigid_body: Some(RigidBody::default()),
                    });
                    current = next;
                }
                let ObjectMut::Vob(mut v) = d.graph.get_mut(current).unwrap() else {
                    panic!()
                };
                v.base_mut().state = Some(SaveState {
                    rigid_body: Some(RigidBody::default()),
                    ..Default::default()
                });
                let again = reopen(&d);
                assert_eq!(again.graph.len(), 127);
                assert_eq!(vob(&again).base().state.unwrap().sleep, 0);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
