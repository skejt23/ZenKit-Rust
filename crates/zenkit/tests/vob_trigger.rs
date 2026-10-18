use zenkit::{
    ErrorKind, GameVersion,
    animation::Sample,
    archive::{self, Format},
    objects::*,
    values::{Quat, Vec3},
    vob::*,
};
fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/synthetic-{name}.zen")),
    )
    .unwrap()
}
fn parse(name: &str, game: GameVersion) -> ObjectArchive {
    ObjectArchive::from_bytes(&fixture(name), game, Limits::default()).unwrap()
}
fn vob(d: &ObjectArchive, i: usize) -> VobRef<'_> {
    let ObjectRef::Vob(v) = d.graph.get(d.graph.roots()[i].target.unwrap()).unwrap() else {
        panic!()
    };
    v
}
fn vob_mut(d: &mut ObjectArchive, i: usize) -> VobMut<'_> {
    let id = d.graph.roots()[i].target.unwrap();
    let ObjectMut::Vob(v) = d.graph.get_mut(id).unwrap() else {
        panic!()
    };
    v
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
fn fresh(kind: VobKind, game: GameVersion, format: Format, save: bool) -> ObjectArchive {
    let mut graph = Graph::new().unwrap();
    let mut v = Vob::new(game);
    v.kind = kind;
    v.base.physics_enabled = false;
    v.base.state = save.then_some(SaveState::default());
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
fn trigger(game: GameVersion) -> Trigger {
    Trigger {
        state: Some(TriggerState::new(game)),
        ..Default::default()
    }
}
fn motion(n: usize) -> MoverMotion {
    MoverMotion {
        speed: 1.,
        lerp: MoverLerp::CURVE,
        speed_mode: MoverSpeed::CONSTANT,
        samples: vec![
            Sample {
                position: Vec3::ZERO,
                rotation: Quat::IDENTITY
            };
            n
        ],
        tail: Vec::new(),
    }
}

#[test]
fn complete_independent_graphs_preserve_phases_and_identity() {
    for enc in ["ascii", "binary", "bin_safe"] {
        for (game, g) in [(GameVersion::Gothic1, 1), (GameVersion::Gothic2, 2)] {
            for save in [false, true] {
                for packed in [false, true] {
                    let d = parse(
                        &format!(
                            "vob-trigger-{enc}-g{g}-{}-{}",
                            if save { "save" } else { "normal" },
                            if packed { "packed" } else { "unpacked" }
                        ),
                        game,
                    );
                    assert_eq!(d.graph.roots().len(), 10);
                    assert_eq!(d.graph.len(), 11);
                    let root = d.graph.roots()[0].target.unwrap();
                    for i in [0, 1, 2, 3, 4, 5, 9] {
                        let t = vob(&d, i).kind().trigger().unwrap();
                        assert_eq!(t.flags.raw(), 0xa5);
                        assert_eq!(t.filter.raw(), 0xd6);
                        assert_eq!(t.target, b"TARGET\xff");
                        assert_eq!(t.state.is_some(), save);
                        if let Some(s) = &t.state {
                            assert_eq!(s.other.target, Some(root));
                            assert_eq!(s.activation_count, -2);
                            assert_eq!(s.enabled, (g == 2).then_some(false));
                        }
                    }
                    let VobKindRef::Mover(m) = &vob(&d, 2).kind() else {
                        panic!()
                    };
                    assert_eq!(
                        m.motion.as_ref().unwrap().samples[0].rotation.to_wxyz(),
                        [0.5, -0.25, 0.125, 0.75]
                    );
                    assert_eq!(m.auto_rotate, (g == 2).then_some(true));
                    assert_eq!(m.state.is_some(), save);
                    let VobKindRef::Mover(empty) = &vob(&d, 9).kind() else {
                        panic!()
                    };
                    assert!(empty.motion.is_none());
                    let VobKindRef::CodeMaster(c) = &vob(&d, 8).kind() else {
                        panic!()
                    };
                    assert_eq!(c.slaves[1], b"");
                    assert_eq!(c.state.is_some(), save && g == 2);
                    if let Some(s) = &c.state {
                        assert_eq!(s.triggered_count, 2);
                        assert_eq!(
                            s.triggered.iter().map(|l| l.target).collect::<Vec<_>>(),
                            [Some(root), None, Some(root)]
                        );
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
fn semantic_flag_setters_preserve_every_unrelated_bit() {
    for bits in 0..=255 {
        let mut f = TriggerFlags::from_raw(bits);
        assert_eq!(f.start_enabled(), bits & 1 != 0);
        assert_eq!(f.send_untrigger(), bits & 4 != 0);
        f.set_start_enabled(true);
        assert_eq!(f.raw(), bits | 1);
        f.set_start_enabled(false);
        assert_eq!(f.raw(), bits & !1);
        f = TriggerFlags::from_raw(bits);
        f.set_send_untrigger(true);
        assert_eq!(f.raw(), bits | 4);
        f.set_send_untrigger(false);
        assert_eq!(f.raw(), bits & !4);
        let mut f = TriggerFilter::from_raw(bits);
        assert_eq!(
            [
                f.on_trigger(),
                f.on_touch(),
                f.on_damage(),
                f.object(),
                f.player(),
                f.npc()
            ],
            [1, 2, 4, 8, 16, 32].map(|m| bits & m != 0)
        );
        for (mask, set) in [
            (
                1,
                TriggerFilter::set_on_trigger as fn(&mut TriggerFilter, bool),
            ),
            (2, TriggerFilter::set_on_touch),
            (4, TriggerFilter::set_on_damage),
            (8, TriggerFilter::set_object),
            (16, TriggerFilter::set_player),
            (32, TriggerFilter::set_npc),
        ] {
            f = TriggerFilter::from_raw(bits);
            set(&mut f, true);
            assert_eq!(f.raw(), bits | mask);
            set(&mut f, false);
            assert_eq!(f.raw(), bits & !mask);
        }
    }
}

#[test]
fn tagged_flag_and_motion_tails_survive_without_binary_truncation() {
    let t = Trigger {
        flags: TriggerFlags::from_raw(0xa5),
        flags_tail: vec![0, 255],
        filter_tail: vec![77],
        ..Default::default()
    };
    let mut m = Mover::new(GameVersion::Gothic2);
    m.trigger = t;
    m.motion = Some(motion(2));
    m.motion.as_mut().unwrap().tail = vec![9, 8, 7];
    let mut d = fresh(
        VobKind::Mover(m),
        GameVersion::Gothic2,
        Format::BinSafe,
        false,
    );
    for format in [Format::Ascii, Format::BinSafe] {
        d.metadata.format = format;
        assert_eq!(vob(&reopen(&d), 0).kind(), vob(&d, 0).kind());
    }
    d.metadata.format = Format::Binary;
    assert_eq!(
        d.to_bytes(Limits::default()).unwrap_err().kind,
        ErrorKind::Unsupported
    );
}

#[test]
fn sequence_budget_is_aggregate_including_empty_names_and_orphans() {
    let mut d = parse("vob-trigger-bin_safe-g2-save-packed", GameVersion::Gothic2);
    let exact = Limits {
        sequence_items: 8,
        ..Limits::default()
    };
    let bytes = d.to_bytes(exact).unwrap();
    ObjectArchive::from_bytes(&bytes, d.game, exact).unwrap();
    let short = Limits {
        sequence_items: 7,
        ..exact
    };
    assert_eq!(
        d.validate(short).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(
        ObjectArchive::from_bytes(&bytes, d.game, short)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    let mut v = Vob::new(d.game);
    v.base.physics_enabled = false;
    v.base.state = Some(SaveState::default());
    v.kind = VobKind::CodeMaster(CodeMaster {
        slaves: vec![Vec::new()],
        state: Some(CodeMasterState {
            triggered_count: 255,
            triggered: vec![Link::default()],
        }),
        ..Default::default()
    });
    d.graph.insert(Object::Vob(v)).unwrap();
    assert_eq!(
        d.validate(exact).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    d.validate(Limits {
        sequence_items: 9,
        ..exact
    })
    .unwrap();
}

#[test]
fn late_edges_count_names_and_occurrences_and_check_target_kinds() {
    for format in formats() {
        let mut d = fresh(
            VobKind::Trigger(trigger(GameVersion::Gothic2)),
            GameVersion::Gothic2,
            format,
            true,
        );
        let id = d.graph.roots()[0].target;
        let mut view = vob_mut(&mut d, 0);
        let mut kind = view.kind_mut();
        let t = kind.trigger_mut().unwrap();
        t.state.as_mut().unwrap().other.target = id;
        let exact = Limits {
            links: 2,
            copied_bytes: d.metadata.archiver.len() + 4 + b"savedOtherVob".len(),
            ..Limits::default()
        };
        let bytes = d.to_bytes(exact).unwrap();
        ObjectArchive::from_bytes(&bytes, d.game, exact).unwrap();
        assert!(d.validate(Limits { links: 1, ..exact }).is_err());
        assert!(ObjectArchive::from_bytes(&bytes, d.game, Limits { links: 1, ..exact }).is_err());
        assert!(
            d.validate(Limits {
                copied_bytes: exact.copied_bytes - 1,
                ..exact
            })
            .is_err()
        );
        let wrong = d
            .graph
            .insert(Object::Visual(Visual::new(VisualKind::Mesh)))
            .unwrap();
        vob_mut(&mut d, 0)
            .kind_mut()
            .trigger_mut()
            .unwrap()
            .state
            .as_mut()
            .unwrap()
            .other
            .target = Some(wrong);
        assert_eq!(
            d.validate(Limits::default()).unwrap_err().kind,
            ErrorKind::WrongObjectType
        );
    }
}

#[test]
fn code_master_accepts_supported_nonvob_links_without_inventing_meaning() {
    for format in formats() {
        let mut d = fresh(
            VobKind::CodeMaster(CodeMaster {
                slaves: vec![b"slave".to_vec(), Vec::new()],
                state: Some(CodeMasterState {
                    triggered_count: 255,
                    triggered: vec![
                        Link::new(b"arbitrary".to_vec(), None),
                        Link::new(b"alias".to_vec(), None),
                    ],
                }),
                ..Default::default()
            }),
            GameVersion::Gothic2,
            format,
            true,
        );
        let id = d
            .graph
            .insert(Object::Message(ConversationMessage {
                state: Some(MessageState::default()),
                text: b"message".to_vec(),
                ..Default::default()
            }))
            .unwrap();
        let VobKindMut::CodeMaster(c) = vob_mut(&mut d, 0).into_parts().1 else {
            panic!()
        };
        for l in &mut c.state.as_mut().unwrap().triggered {
            l.target = Some(id);
        }
        let again = reopen(&d);
        let VobKindRef::CodeMaster(c) = &vob(&again, 0).kind() else {
            panic!()
        };
        let s = c.state.as_ref().unwrap();
        assert_eq!(s.triggered_count, 255);
        assert_eq!(s.triggered[0].target, s.triggered[1].target);
        assert!(matches!(
            again.graph.get(s.triggered[0].target.unwrap()).unwrap(),
            ObjectRef::Message(_)
        ));
    }
}

#[test]
fn zero_count_and_game_save_presence_are_distinct() {
    let game = GameVersion::Gothic2;
    let mut d = fresh(
        VobKind::CodeMaster(CodeMaster {
            state: Some(CodeMasterState {
                triggered_count: 255,
                triggered: Vec::new(),
            }),
            ..Default::default()
        }),
        game,
        Format::BinSafe,
        true,
    );
    assert_eq!(vob(&reopen(&d), 0).kind(), vob(&d, 0).kind());
    let VobKindMut::CodeMaster(c) = vob_mut(&mut d, 0).into_parts().1 else {
        panic!()
    };
    c.state = None;
    assert!(d.validate(Limits::default()).is_err());
    let mut m = Mover::new(game);
    m.motion = Some(motion(0));
    let mut d = fresh(VobKind::Mover(m), game, Format::BinSafe, false);
    assert!(d.validate(Limits::default()).is_err());
    let VobKindMut::Mover(m) = vob_mut(&mut d, 0).into_parts().1 else {
        panic!()
    };
    m.motion = None;
    d.validate(Limits::default()).unwrap();
    mismatch_game(&mut d);
    let mut d = fresh(
        VobKind::TriggerWorldStart(TriggerWorldStart {
            has_fired: Some(false),
            ..Default::default()
        }),
        GameVersion::Gothic1,
        Format::BinSafe,
        true,
    );
    assert!(d.validate(Limits::default()).is_err());
    let id = d.graph.roots()[0].target.unwrap();
    d.graph
        .replace_vob_kind(id, VobKind::TriggerWorldStart(TriggerWorldStart::default()))
        .unwrap();
    d.validate(Limits::default()).unwrap();
}
fn mismatch_game(d: &mut ObjectArchive) {
    let VobKindMut::Mover(m) = vob_mut(d, 0).into_parts().1 else {
        panic!()
    };
    m.auto_rotate = None;
    assert!(d.validate(Limits::default()).is_err());
}

#[test]
fn count_caps_and_binsafe_raw_byte_cap_reject_before_narrowing() {
    let game = GameVersion::Gothic2;
    let mut m = Mover::new(game);
    m.motion = Some(motion(2340));
    let mut d = fresh(VobKind::Mover(m), game, Format::BinSafe, false);
    reopen(&d);
    let VobKindMut::Mover(m) = vob_mut(&mut d, 0).into_parts().1 else {
        panic!()
    };
    m.motion.as_mut().unwrap().samples.push(Sample {
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
    });
    assert_eq!(
        d.to_bytes(Limits::default()).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    d.metadata.format = Format::Binary;
    reopen(&d);
    let VobKindMut::Mover(m) = vob_mut(&mut d, 0).into_parts().1 else {
        panic!()
    };
    m.motion = Some(motion(65536));
    assert!(d.validate(Limits::default()).is_err());
    for kind in [
        VobKind::TriggerList(TriggerList {
            targets: vec![TriggerTarget::default(); 256],
            ..Default::default()
        }),
        VobKind::CodeMaster(CodeMaster {
            slaves: vec![Vec::new(); 256],
            ..Default::default()
        }),
    ] {
        assert!(
            fresh(kind, game, Format::BinSafe, false)
                .validate(Limits::default())
                .is_err()
        );
    }
}

#[test]
fn raw_samples_preserve_nonunit_quaternions_and_special_bits() {
    for format in formats() {
        let mut m = Mover::new(GameVersion::Gothic2);
        let mut frames = motion(1);
        frames.samples[0] = Sample {
            position: [f32::from_bits(0x7fc01234), -0., f32::from_bits(1)].into(),
            rotation: Quat::from_wxyz([7., -8., f32::INFINITY, f32::from_bits(0x7fa01234)]),
        };
        m.motion = Some(frames);
        let d = fresh(VobKind::Mover(m), GameVersion::Gothic2, format, false);
        let again = reopen(&d);
        let VobKindRef::Mover(m) = &vob(&again, 0).kind() else {
            panic!()
        };
        let s = m.motion.as_ref().unwrap().samples[0];
        assert_eq!(s.position.x.to_bits(), 0x7fc01234);
        assert_eq!(s.position.y.to_bits(), 0x80000000);
        assert_eq!(s.position.z.to_bits(), 1);
        assert_eq!(
            s.rotation.to_wxyz().map(f32::to_bits),
            [
                7f32.to_bits(),
                (-8f32).to_bits(),
                f32::INFINITY.to_bits(),
                0x7fa01234
            ]
        );
    }
}

#[test]
fn exact_count_alias_is_accepted_but_unrelated_field_names_fail() {
    let bytes = fixture("vob-trigger-ascii-single-3");
    let p = bytes.windows(9).position(|w| w == b"numTarget").unwrap();
    let mut alias = bytes.clone();
    alias.insert(p + 9, b's');
    let d = ObjectArchive::from_bytes(&alias, GameVersion::Gothic2, Limits::default()).unwrap();
    let output = d.to_bytes(Limits::default()).unwrap();
    assert!(output.windows(10).any(|w| w == b"numTarget="));
    assert!(!output.windows(11).any(|w| w == b"numTargets="));
    let mut bad = bytes;
    bad[p..p + 9].copy_from_slice(b"numRandom");
    let e = ObjectArchive::from_bytes(&bad, GameVersion::Gothic2, Limits::default()).unwrap_err();
    assert_eq!(e.kind, ErrorKind::InvalidData);
    assert!(e.offset.is_some());
}

#[test]
fn truncated_interleaved_save_records_and_extra_payload_fail() {
    let d = fresh(
        VobKind::TriggerScript(TriggerScript {
            trigger: trigger(GameVersion::Gothic2),
            function: b"AFTER_CHILD".to_vec(),
        }),
        GameVersion::Gothic2,
        Format::Binary,
        true,
    );
    let bytes = d.to_bytes(Limits::default()).unwrap();
    let header = archive::Reader::new(&bytes, archive::Limits::default())
        .unwrap()
        .position();
    for n in header + 1..bytes.len() {
        assert!(
            ObjectArchive::from_bytes(&bytes[..n], d.game, Limits::default()).is_err(),
            "prefix {n}"
        );
    }
    let mut bad = bytes;
    bad.push(0);
    assert!(ObjectArchive::from_bytes(&bad, d.game, Limits::default()).is_err());
}

#[test]
fn iterative_trigger_links_and_code_master_sequences_fit_small_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            for format in formats() {
                let game = GameVersion::Gothic2;
                let mut d = fresh(VobKind::Trigger(trigger(game)), game, format, true);
                let root = d.graph.roots()[0].target.unwrap();
                let mut previous = root;
                for _ in 1..127 {
                    let mut v = Vob::new(game);
                    v.base.physics_enabled = false;
                    v.base.state = Some(SaveState::default());
                    v.kind = VobKind::Trigger(trigger(game));
                    let id = d.graph.insert(Object::Vob(v)).unwrap();
                    let ObjectMut::Vob(mut v) = d.graph.get_mut(previous).unwrap() else {
                        panic!()
                    };
                    v.kind_mut()
                        .trigger_mut()
                        .unwrap()
                        .state
                        .as_mut()
                        .unwrap()
                        .other
                        .target = Some(id);
                    previous = id;
                }
                let ObjectMut::Vob(mut v) = d.graph.get_mut(previous).unwrap() else {
                    panic!()
                };
                v.kind_mut()
                    .trigger_mut()
                    .unwrap()
                    .state
                    .as_mut()
                    .unwrap()
                    .other
                    .target = Some(root);
                let again = reopen(&d);
                assert_eq!(again.graph.len(), 127);
                assert_eq!(
                    d.to_bytes(Limits::default()).unwrap(),
                    again.to_bytes(Limits::default()).unwrap()
                );
                d.graph
                    .replace_vob_kind(
                        previous,
                        VobKind::CodeMaster(CodeMaster {
                            slaves: vec![Vec::new(); 255],
                            state: Some(CodeMasterState {
                                triggered_count: 255,
                                triggered: vec![Link::new(b"alias".to_vec(), Some(root)); 255],
                            }),
                            ..Default::default()
                        }),
                    )
                    .unwrap();
                reopen(&d);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn nested_code_master_declared_counts_obey_the_aggregate_link_limit() {
    let game = GameVersion::Gothic2;
    let mut d = fresh(
        VobKind::CodeMaster(CodeMaster::default()),
        game,
        Format::Binary,
        true,
    );
    let mut id = d.graph.roots()[0].target.unwrap();
    for depth in 0..50 {
        let next = if depth == 49 {
            None
        } else {
            let mut v = Vob::new(game);
            v.base.physics_enabled = false;
            v.base.state = Some(SaveState::default());
            v.kind = VobKind::CodeMaster(CodeMaster::default());
            Some(d.graph.insert(Object::Vob(v)).unwrap())
        };
        let n = 100 - depth;
        let mut links = vec![Link::default(); n];
        links[0].target = next;
        d.graph
            .replace_vob_kind(
                id,
                VobKind::CodeMaster(CodeMaster {
                    slaves: vec![Vec::new(); n],
                    state: Some(CodeMasterState {
                        triggered_count: 0,
                        triggered: links,
                    }),
                    ..Default::default()
                }),
            )
            .unwrap();
        if let Some(next) = next {
            id = next;
        }
    }
    let bytes = d.to_bytes(Limits::default()).unwrap();
    // At each nested prefix the claimed sequence still fits the remaining
    // occurrence budget. The complete encoded graph still exceeds the aggregate
    // cap and must fail. The internal phase test separately checks that pending
    // declarations do not allocate placeholder occurrence arrays.
    let e = ObjectArchive::from_bytes(
        &bytes,
        game,
        Limits {
            links: 101,
            ..Limits::default()
        },
    )
    .unwrap_err();
    assert_eq!(e.kind, ErrorKind::LimitExceeded);
    assert!(e.offset.is_some());
}
