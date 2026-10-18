use zenkit::{
    ErrorKind, GameVersion,
    archive::{self, Format},
    objects::*,
    values::{Mat4, Vec4},
    vob::*,
};
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
fn root(d: &ObjectArchive) -> VobRef<'_> {
    let ObjectRef::Vob(v) = d.graph.get(d.graph.roots()[0].target.unwrap()).unwrap() else {
        panic!()
    };
    v
}
fn root_mut(d: &mut ObjectArchive) -> VobMut<'_> {
    let id = d.graph.roots()[0].target.unwrap();
    let ObjectMut::Vob(v) = d.graph.get_mut(id).unwrap() else {
        panic!()
    };
    v
}
fn camera(d: &ObjectArchive) -> &Camera {
    let VobKindRef::Camera(c) = &root(d).kind() else {
        panic!()
    };
    c
}
fn camera_mut(d: &mut ObjectArchive) -> &mut Camera {
    let VobKindMut::Camera(c) = root_mut(d).into_parts().1 else {
        panic!()
    };
    c
}
fn frame(d: &ObjectArchive, id: ObjectId) -> &CameraFrame {
    let ObjectRef::Vob(v) = d.graph.get(id).unwrap() else {
        panic!()
    };
    let VobKindRef::CameraFrame(f) = &v.kind() else {
        panic!()
    };
    f
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
fn fresh(format: Format, game: GameVersion, save: bool) -> ObjectArchive {
    let mut graph = Graph::new().unwrap();
    let mut v = Vob::new(game);
    v.base.physics_enabled = false;
    v.base.state = save.then_some(SaveState::default());
    v.kind = VobKind::Camera(Camera {
        state: (save && game == GameVersion::Gothic2).then_some(CameraState::default()),
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
fn add_frame(d: &mut ObjectArchive) -> ObjectId {
    let mut v = Vob::new(d.game);
    v.base.physics_enabled = false;
    v.base.state = d.metadata.save_game.then_some(SaveState::default());
    v.kind = VobKind::CameraFrame(CameraFrame::new(d.game));
    d.graph.insert(Object::Vob(v)).unwrap()
}
#[test]
fn all_modes_preserve_complete_fields_both_sequences_and_suffix() {
    for enc in ["ascii", "binary", "bin_safe"] {
        for (game, g) in [(GameVersion::Gothic1, 1), (GameVersion::Gothic2, 2)] {
            for save in [false, true] {
                for packed in [false, true] {
                    let d = parse(
                        &format!(
                            "vob-camera-{enc}-g{g}-{}-{}-canonical",
                            if save { "save" } else { "normal" },
                            if packed { "packed" } else { "unpacked" }
                        ),
                        game,
                    );
                    let c = camera(&d);
                    assert_eq!(d.graph.len(), 6);
                    assert_eq!(c.trajectory_for, CameraCoordinate::OBJECT);
                    assert_eq!(c.target_trajectory_for, CameraCoordinate::WORLD);
                    assert_eq!(c.loop_mode, CameraLoop::PINGPONG);
                    assert_eq!(c.lerp_mode, CameraLerp::PATH_ROTATION_SAMPLES);
                    assert!(
                        c.ignore_for_vob_rotation
                            && !c.ignore_for_vob_rotation_target
                            && c.adapt
                            && !c.ease_first
                            && c.ease_last
                    );
                    assert_eq!(c.total_duration, 12.5);
                    assert_eq!(c.auto_focus_vob, b"FOCUS\xff");
                    assert!(!c.auto_player_movable && c.auto_untrigger_last);
                    assert_eq!(c.auto_untrigger_last_delay, 0.625);
                    assert_eq!(c.positions.len(), 4);
                    assert_eq!(c.targets.len(), 3);
                    assert_eq!(c.positions[0].target, c.positions[3].target);
                    assert_eq!(c.targets[1].target, c.positions[0].target);
                    assert_eq!(c.targets[0].target, c.positions[2].target);
                    assert!(c.positions[1].target.is_none() && c.targets[2].target.is_none());
                    assert_eq!(c.positions[3].name, b"positionAlias");
                    assert_eq!(c.targets[1].name, b"targetAlias");
                    assert_eq!(
                        c.state,
                        if g == 2 && save {
                            Some(CameraState {
                                paused: true,
                                started: false,
                                goto_time_mode: true,
                                cs_time: 7.75,
                            })
                        } else {
                            None
                        }
                    );
                    let f = frame(&d, c.positions[0].target.unwrap());
                    assert_eq!((f.time, f.roll_angle, f.fov_scale), (1.25, -12.5, 0.875));
                    assert_eq!(
                        (
                            f.motion_type,
                            f.motion_type_fov,
                            f.motion_type_roll,
                            f.motion_type_time_scale
                        ),
                        (
                            CameraMotion::CUSTOM,
                            CameraMotion::LINEAR,
                            CameraMotion::STEP,
                            CameraMotion::SLOW
                        )
                    );
                    assert_eq!(
                        (f.tension, f.cam_bias, f.continuity, f.time_scale),
                        (-0.25, 0.375, -0.625, 1.5)
                    );
                    assert!(f.time_fixed);
                    for r in 0..4 {
                        for col in 0..4 {
                            assert_eq!(f.original_pose[col][r], (r * 4 + col + 1) as f32);
                        }
                    }
                    let ObjectRef::Vob(v) = d.graph.get(c.positions[0].target.unwrap()).unwrap()
                    else {
                        panic!()
                    };
                    let ObjectRef::AiMove(ai) = d
                        .graph
                        .get(v.base().ai.as_ref().unwrap().target.unwrap())
                        .unwrap()
                    else {
                        panic!()
                    };
                    assert_eq!(ai.vob.target, d.graph.roots()[0].target);
                    assert_eq!(
                        d.to_bytes(Limits::default()).unwrap(),
                        reopen(&d).to_bytes(Limits::default()).unwrap()
                    );
                }
            }
        }
    }
}
#[test]
fn zero_and_one_empty_sequence_still_reaches_save_suffix() {
    for enc in ["ascii", "binary", "bin_safe"] {
        for (game, g) in [(GameVersion::Gothic1, 1), (GameVersion::Gothic2, 2)] {
            for (variant, np, nt) in [
                ("empty", 0, 0),
                ("positions-empty", 0, 3),
                ("targets-empty", 4, 0),
            ] {
                let d = parse(&format!("vob-camera-{enc}-g{g}-{variant}"), game);
                let c = camera(&d);
                assert_eq!((c.positions.len(), c.targets.len()), (np, nt));
                assert_eq!(c.state.is_some(), g == 2);
                let a = reopen(&d);
                assert_eq!(
                    (camera(&a).positions.len(), camera(&a).targets.len()),
                    (np, nt)
                );
                assert_eq!(c.state, camera(&a).state);
            }
        }
    }
}
#[test]
fn retained_frame_versions_are_game_checked() {
    for enc in ["ascii", "binary", "bin_safe"] {
        let d = parse(
            &format!("vob-camera-{enc}-retained-version"),
            GameVersion::Gothic2,
        );
        let id = d.graph.roots()[0].target.unwrap();
        assert_eq!(frame(&d, id).version, 31489);
        let a = reopen(&d);
        assert_eq!(frame(&a, a.graph.roots()[0].target.unwrap()).version, 31489);
    }
    for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
        let mut d = fresh(Format::BinSafe, game, false);
        let f = add_frame(&mut d);
        camera_mut(&mut d)
            .positions
            .push(Link::new(b"frame".to_vec(), Some(f)));
        assert_eq!(
            frame(&d, f).version,
            if game == GameVersion::Gothic1 {
                12289
            } else {
                52224
            }
        );
        let ObjectMut::Vob(mut v) = d.graph.get_mut(f).unwrap() else {
            panic!()
        };
        let VobKindMut::CameraFrame(f) = v.kind_mut() else {
            panic!()
        };
        f.version = if game == GameVersion::Gothic1 {
            31489
        } else {
            12289
        };
        assert_eq!(
            d.validate(Limits::default()).unwrap_err().kind,
            ErrorKind::Unsupported
        );
    }
}
#[test]
fn raw_pose_bits_survive_without_transpose_or_tail_loss() {
    let words = [
        0x80000000, 1, 0x7f800000, 0xff800000, 0x7fc12345, 0xffc23456, 0x007fffff, 0x00800000,
        0x3f800000, 0xbf800000, 0x3e800000, 0x40000000, 0, 0x7f7fffff, 0xff7fffff, 0x3f000000,
    ];
    for enc in ["ascii", "binary", "bin_safe"] {
        let d = parse(
            &format!("vob-camera-{enc}-special-bits"),
            GameVersion::Gothic2,
        );
        let a = reopen(&d);
        let f = frame(&a, a.graph.roots()[0].target.unwrap());
        for (i, word) in words.iter().enumerate() {
            assert_eq!(f.original_pose[i / 4][i % 4].to_bits(), *word);
        }
    }
    for enc in ["ascii", "bin_safe"] {
        let mut d = parse(&format!("vob-camera-{enc}-raw"), GameVersion::Gothic2);
        let a = reopen(&d);
        assert_eq!(
            frame(&a, a.graph.roots()[0].target.unwrap()).pose_tail,
            [0x91, 0x82, 0x73, 0, 255]
        );
        root_mut(&mut d).base_mut().packing = Packing::Packed {
            marker: 1,
            reserved: 0,
            tail: Vec::new(),
        };
        d.metadata.format = Format::Binary;
        assert_eq!(
            d.to_bytes(Limits::default()).unwrap_err().kind,
            ErrorKind::Unsupported
        );
    }
}
#[test]
fn counts_versions_short_pose_and_wrong_subtypes_fail_explicitly() {
    for enc in ["ascii", "binary", "bin_safe"] {
        for variant in [
            "negative-position",
            "negative-target",
            "huge-position",
            "huge-target",
            "missing-count",
            "wrong-position-kind",
            "wrong-target-kind",
            "short-pose",
            "camera-version",
            "frame-version",
        ] {
            let e = ObjectArchive::from_bytes(
                &bytes(&format!("vob-camera-invalid-{enc}-{variant}")),
                GameVersion::Gothic2,
                Limits::default(),
            )
            .unwrap_err();
            assert!(e.offset.is_some());
            if variant.starts_with("negative") {
                assert_eq!(e.kind, ErrorKind::InvalidData)
            } else if variant.starts_with("huge") {
                assert_eq!(e.kind, ErrorKind::LimitExceeded)
            } else if variant.starts_with("wrong") {
                assert_eq!(e.kind, ErrorKind::WrongObjectType)
            } else if variant.ends_with("version") {
                assert_eq!(e.kind, ErrorKind::Unsupported)
            }
        }
    }
}
#[test]
fn exact_cumulative_links_and_copied_bytes_include_orphans() {
    for format in formats() {
        let mut d = fresh(format, GameVersion::Gothic2, true);
        let f = add_frame(&mut d);
        camera_mut(&mut d).auto_focus_vob = b"focus".to_vec();
        camera_mut(&mut d).positions = vec![
            Link::new(b"one".to_vec(), Some(f)),
            Link::new(b"nil".to_vec(), None),
        ];
        camera_mut(&mut d).targets = vec![Link::new(b"two".to_vec(), Some(f))];
        let cap = d.metadata.archiver.len() + 4 + 5 + 9;
        let exact = Limits {
            links: 4,
            copied_bytes: cap,
            sequence_items: 0,
            ..Limits::default()
        };
        let wire = d.to_bytes(exact).unwrap();
        ObjectArchive::from_bytes(&wire, d.game, exact).unwrap();
        for limit in [
            Limits { links: 3, ..exact },
            Limits {
                copied_bytes: cap - 1,
                ..exact
            },
        ] {
            assert_eq!(
                d.validate(limit).unwrap_err().kind,
                ErrorKind::LimitExceeded
            );
            assert_eq!(
                ObjectArchive::from_bytes(&wire, d.game, limit)
                    .unwrap_err()
                    .kind,
                ErrorKind::LimitExceeded
            );
        }
        let mut orphan = Vob::new(d.game);
        orphan.base.physics_enabled = false;
        orphan.base.state = Some(SaveState::default());
        orphan.kind = VobKind::Camera(Camera {
            targets: vec![Link::new(Vec::new(), None)],
            state: Some(CameraState::default()),
            ..Default::default()
        });
        d.graph.insert(Object::Vob(orphan)).unwrap();
        assert!(d.validate(exact).is_err());
        d.validate(Limits { links: 5, ..exact }).unwrap();
    }
}
#[test]
fn exact_frame_only_rule_rejects_items_and_foreign_or_orphan_edges() {
    let mut d = fresh(Format::BinSafe, GameVersion::Gothic2, false);
    let root = d.graph.roots()[0].target;
    camera_mut(&mut d).targets = vec![Link::new(b"self".to_vec(), root)];
    assert_eq!(
        d.validate(Limits::default()).unwrap_err().kind,
        ErrorKind::WrongObjectType
    );
    let mut item = Vob::new(d.game);
    item.kind = VobKind::Item(Item::default());
    let item = d.graph.insert(Object::Vob(item)).unwrap();
    camera_mut(&mut d).positions = vec![Link::new(b"item".to_vec(), Some(item))];
    assert_eq!(
        d.validate(Limits::default()).unwrap_err().kind,
        ErrorKind::WrongObjectType
    );
    camera_mut(&mut d).positions.clear();
    camera_mut(&mut d).targets[0].target =
        fresh(Format::BinSafe, d.game, false).graph.roots()[0].target;
    assert_eq!(
        d.validate(Limits::default()).unwrap_err().kind,
        ErrorKind::InvalidHandle
    );
    camera_mut(&mut d).targets.clear();
    let mut orphan = Vob::new(d.game);
    orphan.kind = VobKind::Camera(Camera {
        positions: vec![Link::new(b"bad".to_vec(), Some(item))],
        ..Default::default()
    });
    d.graph.insert(Object::Vob(orphan)).unwrap();
    assert_eq!(
        d.validate(Limits::default()).unwrap_err().kind,
        ErrorKind::WrongObjectType
    );
}
#[test]
fn save_presence_and_unknown_enums_are_never_silently_converted() {
    for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
        for save in [false, true] {
            let mut d = fresh(Format::BinSafe, game, save);
            let expected = game == GameVersion::Gothic2 && save;
            assert_eq!(camera(&reopen(&d)).state.is_some(), expected);
            camera_mut(&mut d).state = if expected {
                None
            } else {
                Some(CameraState::default())
            };
            assert_eq!(
                d.validate(Limits::default()).unwrap_err().kind,
                ErrorKind::InvalidData
            );
        }
    }
    let mut d = fresh(Format::BinSafe, GameVersion::Gothic2, false);
    camera_mut(&mut d).loop_mode = CameraLoop::from_raw(u32::MAX);
    camera_mut(&mut d).trajectory_for = CameraCoordinate::from_raw(300);
    camera_mut(&mut d).lerp_mode = CameraLerp::from_raw(301);
    assert_eq!(camera(&reopen(&d)).loop_mode.raw(), u32::MAX);
    d.metadata.format = Format::Binary;
    assert_eq!(
        d.to_bytes(Limits::default()).unwrap_err().kind,
        ErrorKind::Unsupported
    );
}
#[test]
fn raw_pose_output_preflight_and_exact_tail_copy_cap() {
    for format in [Format::Ascii, Format::BinSafe] {
        let mut d = fresh(format, GameVersion::Gothic2, false);
        let id = d.graph.roots()[0].target.unwrap();
        let mut f = CameraFrame::new(d.game);
        f.pose_tail = vec![7; 65_471];
        let root = d.graph.roots()[0].target.unwrap();
        d.graph
            .replace_vob_kind(root, VobKind::CameraFrame(f))
            .unwrap();
        let exact = Limits {
            copied_bytes: d.metadata.archiver.len() + 4 + 65_471,
            ..Limits::default()
        };
        let wire = d.to_bytes(exact).unwrap();
        ObjectArchive::from_bytes(&wire, d.game, exact).unwrap();
        assert!(
            d.validate(Limits {
                copied_bytes: exact.copied_bytes - 1,
                ..exact
            })
            .is_err()
        );
        let ObjectMut::Vob(mut v) = d.graph.get_mut(id).unwrap() else {
            panic!()
        };
        let VobKindMut::CameraFrame(f) = v.kind_mut() else {
            panic!()
        };
        f.pose_tail.push(8);
        if format == Format::BinSafe {
            assert_eq!(
                d.to_bytes(Limits::default()).unwrap_err().kind,
                ErrorKind::LimitExceeded
            );
        } else {
            let limits = Limits {
                archive: archive::Limits {
                    field_bytes: 1000,
                    ..archive::Limits::default()
                },
                ..Limits::default()
            };
            assert_eq!(
                d.to_bytes(limits).unwrap_err().kind,
                ErrorKind::LimitExceeded
            );
        }
    }
}
#[test]
fn matrix_mutation_shared_identity_and_compact_debug() {
    let mut d = parse(
        "vob-camera-bin_safe-g2-save-packed-canonical",
        GameVersion::Gothic2,
    );
    let id = camera(&d).positions[0].target.unwrap();
    let ObjectMut::Vob(mut v) = d.graph.get_mut(id).unwrap() else {
        panic!()
    };
    let VobKindMut::CameraFrame(f) = v.kind_mut() else {
        panic!()
    };
    f.original_pose = Mat4::from_rows([
        Vec4::from([1., 2., 3., 4.]),
        Vec4::from([5., 6., 7., 8.]),
        Vec4::from([9., 10., 11., 12.]),
        Vec4::from([13., 14., 15., 16.]),
    ]);
    let a = reopen(&d);
    let c = camera(&a);
    assert_eq!(c.positions[0].target, c.targets[1].target);
    assert_eq!(
        frame(&a, c.targets[1].target.unwrap()).original_pose[1][0],
        2.
    );
    camera_mut(&mut d).auto_focus_vob = vec![1; 100_000];
    assert!(format!("{:?}", camera(&d)).len() < 1500);
    assert!(std::mem::size_of::<ObjectRef<'_>>() <= 64);
}
#[test]
fn list_insert_remove_reorder_and_clone_keep_counts_derived() {
    for format in formats() {
        let mut d = fresh(format, GameVersion::Gothic2, true);
        let a = add_frame(&mut d);
        let b = add_frame(&mut d);
        camera_mut(&mut d)
            .positions
            .push(Link::new(b"a".to_vec(), Some(a)));
        camera_mut(&mut d)
            .targets
            .push(Link::new(b"b".to_vec(), Some(b)));
        camera_mut(&mut d)
            .positions
            .insert(0, Link::new(b"front".to_vec(), Some(b)));
        camera_mut(&mut d)
            .targets
            .push(Link::new(b"end".to_vec(), Some(a)));
        let a1 = reopen(&d);
        assert_eq!(
            (camera(&a1).positions.len(), camera(&a1).targets.len()),
            (2, 2)
        );
        assert_eq!(
            camera(&a1).positions[0].target,
            camera(&a1).targets[0].target
        );
        camera_mut(&mut d).positions.remove(1);
        camera_mut(&mut d).targets.swap(0, 1);
        let mut copied = camera(&d).clone();
        copied.targets.remove(0);
        copied.positions[0].target = Some(a);
        let root = d.graph.roots()[0].target.unwrap();
        d.graph
            .replace_vob_kind(root, VobKind::Camera(copied))
            .unwrap();
        let a2 = reopen(&d);
        assert_eq!(
            (camera(&a2).positions.len(), camera(&a2).targets.len()),
            (1, 1)
        );
        assert_ne!(
            camera(&a2).positions[0].target,
            camera(&a2).targets[0].target
        );
        camera_mut(&mut d).positions.clear();
        camera_mut(&mut d).targets.clear();
        assert!(camera(&reopen(&d)).state.is_some());
    }
}
#[test]
fn deep_camera_frame_ai_cycles_and_many_targets_use_small_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            for format in formats() {
                let mut d = fresh(format, GameVersion::Gothic2, true);
                let first = d.graph.roots()[0].target.unwrap();
                let mut current = first;
                for level in 0..42 {
                    let f = add_frame(&mut d);
                    let next = if level == 41 {
                        first
                    } else {
                        let mut v = Vob::new(d.game);
                        v.base.physics_enabled = false;
                        v.base.state = Some(SaveState::default());
                        v.kind = VobKind::Camera(Camera {
                            state: Some(CameraState {
                                paused: true,
                                cs_time: level as f32,
                                ..Default::default()
                            }),
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
                    let ObjectMut::Vob(mut v) = d.graph.get_mut(f).unwrap() else {
                        panic!()
                    };
                    v.base_mut().ai = Some(Link::new(b"ai".to_vec(), Some(ai)));
                    let ObjectMut::Vob(mut v) = d.graph.get_mut(current).unwrap() else {
                        panic!()
                    };
                    let VobKindMut::Camera(c) = v.kind_mut() else {
                        panic!()
                    };
                    c.targets.push(Link::new(b"target".to_vec(), Some(f)));
                    current = next;
                }
                assert_eq!(reopen(&d).graph.len(), 126);
                let link = camera(&d).targets[0].clone();
                camera_mut(&mut d).targets = vec![link; 20_000];
                assert_eq!(camera(&reopen(&d)).targets.len(), 20_000);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
