use zenkit::{
    ErrorKind, GameVersion,
    archive::{self, Format},
    objects::*,
    values::{Color, Vec2, Vec3},
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
fn vob(doc: &ObjectArchive, index: usize) -> VobRef<'_> {
    let ObjectRef::Vob(v) = doc
        .graph
        .get(doc.graph.roots()[index].target.unwrap())
        .unwrap()
    else {
        panic!()
    };
    v
}
fn vob_mut(doc: &mut ObjectArchive, index: usize) -> VobMut<'_> {
    let id = doc.graph.roots()[index].target.unwrap();
    let ObjectMut::Vob(v) = doc.graph.get_mut(id).unwrap() else {
        panic!()
    };
    v
}
fn reopen(doc: &ObjectArchive) -> ObjectArchive {
    ObjectArchive::from_bytes(
        &doc.to_bytes(Limits::default()).unwrap(),
        doc.game,
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
fn preset_bytes(p: &LightPreset, game: GameVersion, format: Format) -> Vec<u8> {
    let mut w =
        archive::Writer::new(archive::Header::new(format), archive::Limits::default()).unwrap();
    p.write(&mut w, game).unwrap();
    w.finish().unwrap()
}

#[test]
fn independent_full_fixtures_cover_all_layouts_and_shared_visual_identity() {
    for encoding in ["ascii", "binary", "bin_safe"] {
        for (game, g) in [(GameVersion::Gothic1, 1), (GameVersion::Gothic2, 2)] {
            for save in [false, true] {
                for packed in [false, true] {
                    let doc = parse(
                        &format!(
                            "vob-special-{encoding}-g{g}-{}-{}",
                            if save { "save" } else { "normal" },
                            if packed { "packed" } else { "unpacked" }
                        ),
                        game,
                    );
                    assert_eq!(doc.graph.len(), 20);
                    assert_eq!(doc.graph.roots().len(), 19);
                    let visual = vob(&doc, 0).base().visual.as_ref().unwrap().target;
                    for i in 0..19 {
                        assert_eq!(vob(&doc, i).base().visual.as_ref().unwrap().target, visual);
                        assert_eq!(vob(&doc, i).base().state.is_some(), save);
                    }
                    let VobKindRef::Animate(a) = &vob(&doc, 0).kind() else {
                        panic!()
                    };
                    assert!(a.start_on);
                    assert_eq!(a.running, save.then_some(false));
                    assert_eq!(a.is_running(), !save);
                    let VobKindRef::Item(item) = &vob(&doc, 1).kind() else {
                        panic!()
                    };
                    assert_eq!(item.instance, b"ITMI\xff_TEST");
                    assert_eq!(
                        item.state,
                        save.then_some(ItemState {
                            amount: 17,
                            flags: 0x402
                        })
                    );
                    let VobKindRef::TouchDamage(damage) = &vob(&doc, 6).kind() else {
                        panic!()
                    };
                    assert_eq!(damage.damage, 125.5);
                    assert!(damage.barrier && damage.edge && damage.fly && damage.point);
                    assert!(!damage.blunt && !damage.fire && !damage.magic && !damage.fall);
                    let VobKindRef::SoundDaytime(sound) = &vob(&doc, 16).kind() else {
                        panic!()
                    };
                    assert_eq!((sound.start_time, sound.end_time), (6.25, 20.5));
                    assert_eq!(sound.second_name, b"NIGHT\xfe.WAV");
                    assert_eq!(
                        sound.sound.state,
                        save.then_some(SoundState {
                            running: false,
                            allowed_to_run: true
                        })
                    );
                    let VobKindRef::Light(light) = &vob(&doc, 18).kind() else {
                        panic!()
                    };
                    let a = light.animation.as_ref().unwrap();
                    assert_eq!(a.range_scale.as_bytes(), b" \t0.5  1.25 2.5 \t");
                    assert_eq!(
                        a.range_scale.decoded().unwrap().collect::<Vec<_>>(),
                        [0.5, 1.25, 2.5]
                    );
                    assert_eq!(a.can_move, (g == 2).then_some(true));
                    let restored = reopen(&doc);
                    for i in 0..19 {
                        assert_eq!(vob(&doc, i).kind(), vob(&restored, i).kind());
                    }
                    assert_eq!(doc.metadata, restored.metadata);
                    assert_eq!(
                        doc.to_bytes(Limits::default()).unwrap(),
                        restored.to_bytes(Limits::default()).unwrap()
                    );
                }
            }
        }
    }
}

#[test]
fn subtype_versions_are_derived_and_wrong_versions_fail() {
    let expected = [
        (12289, 52224),
        (0, 0),
        (64704, 193),
        (12289, 52224),
        (0, 0),
        (0, 0),
        (36865, 36865),
        (52224, 52224),
        (12289, 52224),
        (0, 0),
        (0, 0),
        (12289, 52224),
        (12289, 52224),
        (64704, 5505),
        (64704, 5505),
        (12289, 12289),
        (12289, 12289),
        (46080, 39168),
        (46080, 39168),
    ];
    for (game, g) in [(GameVersion::Gothic1, 1), (GameVersion::Gothic2, 2)] {
        let d = parse(&format!("vob-special-ascii-g{g}-normal-packed"), game);
        for (i, (g1, g2)) in expected.into_iter().enumerate() {
            assert_eq!(
                vob(&d, i).version_identifier(),
                if g == 1 { g1 } else { g2 }
            );
        }
    }
    let wire = fixture("vob-special-ascii-single-2");
    let wire = String::from_utf8_lossy(&wire)
        .replace("zCVobLensFlare:zCVob 193 ", "zCVobLensFlare:zCVob 194 ")
        .into_bytes();
    assert_eq!(
        ObjectArchive::from_bytes(&wire, GameVersion::Gothic2, Limits::default())
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
}

#[test]
fn standalone_presets_have_complete_fields_and_exact_copy_budgets() {
    for encoding in ["ascii", "binary", "bin_safe"] {
        for (game, g) in [(GameVersion::Gothic1, 1), (GameVersion::Gothic2, 2)] {
            for dynamic in [false, true] {
                let bytes = fixture(&format!(
                    "light-preset-{encoding}-g{g}-{}",
                    if dynamic { "dynamic" } else { "static" }
                ));
                let mut r = archive::Reader::new(&bytes, archive::Limits::default()).unwrap();
                let p = LightPreset::read(&mut r, game, usize::MAX).unwrap();
                r.finish().unwrap();
                assert_eq!(p.preset, b"LIGHT\xff");
                assert_eq!(p.lensflare, b"HALO\xfe");
                assert_eq!(p.range, 1500.5);
                assert_eq!(p.color.to_rgba(), [17, 34, 51, 203]);
                assert_eq!(p.animation.is_some(), dynamic);
                let n = p.preset.len()
                    + p.lensflare.len()
                    + p.animation.as_ref().map_or(0, |a| {
                        a.range_scale.as_bytes().len() + a.colors.as_bytes().len()
                    });
                let mut r = archive::Reader::new(&bytes, archive::Limits::default()).unwrap();
                assert_eq!(LightPreset::read(&mut r, game, n).unwrap(), p);
                r.finish().unwrap();
                let mut r = archive::Reader::new(&bytes, archive::Limits::default()).unwrap();
                let e = LightPreset::read(&mut r, game, n - 1).unwrap_err();
                assert_eq!(e.kind, ErrorKind::LimitExceeded);
                assert!(e.offset.is_some());
                for format in formats() {
                    let out = preset_bytes(&p, game, format);
                    let mut r = archive::Reader::new(&out, archive::Limits::default()).unwrap();
                    assert_eq!(LightPreset::read(&mut r, game, n).unwrap(), p);
                    r.finish().unwrap();
                }
            }
        }
    }
}

#[test]
fn range_views_validate_every_token_and_preserve_finite_bits() {
    let raw = RangeAnimation::from_bytes(b" \t-0\x0b1.25\x0c-2.5\r\n".to_vec());
    let mut view = raw.decoded().unwrap();
    assert_eq!(view.len(), 3);
    assert_eq!(view.next_back(), Some(-2.5));
    assert_eq!(view.next().unwrap().to_bits(), 0x80000000);
    assert_eq!(view.next_back(), Some(1.25));
    assert_eq!(view.next(), None);
    assert_eq!(view.len(), 0);
    for (text, offset, kind) in [
        (b"1 2 junk".as_slice(), 4, ErrorKind::InvalidData),
        (b"1 NaN", 2, ErrorKind::Unsupported),
        (b"1 \xff", 2, ErrorKind::InvalidData),
    ] {
        let e = RangeAnimation::from_bytes(text.to_vec())
            .decoded()
            .unwrap_err();
        assert_eq!(e.kind, kind);
        assert_eq!(e.offset, Some(offset));
    }
    let mut bits = 0x12345678u32;
    for _ in 0..2048 {
        bits = bits.wrapping_mul(1664525).wrapping_add(1013904223);
        let f = f32::from_bits(bits);
        if !f.is_finite() {
            continue;
        }
        let a = RangeAnimation::from_values(&[f], 128).unwrap();
        assert_eq!(a.decoded().unwrap().next().unwrap().to_bits(), bits);
    }
    for f in [0.0, -0.0, f32::from_bits(1), f32::MAX, f32::MIN] {
        let a = RangeAnimation::from_values(&[f], 128).unwrap();
        assert_eq!(a.decoded().unwrap().next().unwrap().to_bits(), f.to_bits());
    }
}

#[test]
fn color_views_are_strict_and_setters_are_transactional() {
    let a = ColorAnimation::from_bytes(b" 64\x0b(1\t2\x0c3) 255 (4 5 6)".to_vec());
    assert_eq!(
        a.decoded().unwrap().map(Color::to_rgba).collect::<Vec<_>>(),
        [
            [64, 64, 64, 255],
            [1, 2, 3, 255],
            [255, 255, 255, 255],
            [4, 5, 6, 255]
        ]
    );
    for (text, offset) in [
        (b"1 (2 3)".as_slice(), 6),
        (b"1 256", 4),
        (b"1 -2", 2),
        (b"(1 2 3) junk", 8),
    ] {
        assert_eq!(
            ColorAnimation::from_bytes(text.to_vec())
                .decoded()
                .unwrap_err()
                .offset,
            Some(offset)
        );
    }
    let mut colors = a.clone();
    assert_eq!(
        colors
            .set_values(&[Color::from_rgba([1, 2, 3, 0])], 100)
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
    assert_eq!(colors, a);
    assert_eq!(
        colors
            .set_values(&[Color::from_rgba([1, 2, 3, 255])], 6)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(colors, a);
    colors
        .set_values(&[Color::from_rgba([1, 2, 3, 255])], 7)
        .unwrap();
    assert_eq!(colors.as_bytes(), b"(1 2 3)");
    let mut ranges = RangeAnimation::from_bytes(b"original".to_vec());
    for (v, n) in [(&[f32::INFINITY][..], 100), (&[1.25][..], 3)] {
        assert!(ranges.set_values(v, n).is_err());
        assert_eq!(ranges.as_bytes(), b"original");
    }
    ranges.set_values(&[-0.0, 1.25], 7).unwrap();
    assert_eq!(ranges.as_bytes(), b"-0 1.25");
}

#[test]
fn malformed_light_text_is_losslessly_stored_and_rewritten() {
    for format in formats() {
        let mut p = LightPreset::new(GameVersion::Gothic2);
        let a = p.animation.as_mut().unwrap();
        a.range_scale = RangeAnimation::from_bytes(b"1.0 not-a-number \xff".to_vec());
        a.colors = ColorAnimation::from_bytes(b"(300 -1 missing)".to_vec());
        let doc = fresh(
            VobKind::Light(p.clone()),
            GameVersion::Gothic2,
            format,
            false,
        );
        let out = reopen(&doc);
        assert_eq!(vob(&out, 0).kind(), VobKind::Light(p.clone()).as_ref());
        let bytes = preset_bytes(&p, doc.game, format);
        let mut r = archive::Reader::new(&bytes, archive::Limits::default()).unwrap();
        let p = LightPreset::read(&mut r, doc.game, 1000).unwrap();
        r.finish().unwrap();
        let a = p.animation.unwrap();
        assert!(a.range_scale.decoded().is_err());
        assert!(a.colors.decoded().is_err());
    }
}

#[test]
fn screen_save_bytes_and_tagged_tails_survive_and_binary_conversion_refuses_loss() {
    for (game, g) in [(GameVersion::Gothic1, 1), (GameVersion::Gothic2, 2)] {
        let mut doc = parse(&format!("vob-special-bin_safe-g{g}-screen-save"), game);
        let VobKindMut::ScreenEffect(screen) = vob_mut(&mut doc, 0).into_parts().1 else {
            panic!()
        };
        let s = screen.state.as_mut().unwrap();
        assert_eq!(s.fov_saved, Vec2::from([87.25, 65.5]));
        assert_eq!(s.blend[0..3], [1, 18, 35]);
        s.blend.extend_from_slice(b"opaque-tail");
        s.cinema.push(0xFF);
        for format in [Format::Ascii, Format::BinSafe] {
            doc.metadata.format = format;
            assert_eq!(vob(&reopen(&doc), 0).kind(), vob(&doc, 0).kind());
        }
        doc.metadata.format = Format::Binary;
        assert_eq!(
            doc.to_bytes(Limits::default()).unwrap_err().kind,
            ErrorKind::Unsupported
        );
        doc.metadata.format = Format::BinSafe;
        let VobKindMut::ScreenEffect(screen) = vob_mut(&mut doc, 0).into_parts().1 else {
            panic!()
        };
        screen.state.as_mut().unwrap().blend.truncate(19);
        assert!(doc.validate(Limits::default()).is_err());
    }
}

#[test]
fn game_and_save_options_are_validated_including_orphans() {
    let mut doc = fresh(
        VobKind::Sound(Sound::default()),
        GameVersion::Gothic2,
        Format::BinSafe,
        true,
    );
    assert!(doc.validate(Limits::default()).is_err());
    let VobKindMut::Sound(s) = vob_mut(&mut doc, 0).into_parts().1 else {
        panic!()
    };
    s.state = Some(SoundState::default());
    doc.validate(Limits::default()).unwrap();
    let mut orphan = Vob::new(doc.game);
    orphan.base.physics_enabled = false;
    orphan.base.state = Some(SaveState::default());
    orphan.kind = VobKind::Light(LightPreset::new(GameVersion::Gothic1));
    let id = doc.graph.insert(Object::Vob(orphan)).unwrap();
    assert!(doc.validate(Limits::default()).is_err());
    doc.graph
        .replace_vob_kind(id, VobKind::Light(LightPreset::new(doc.game)))
        .unwrap();
    doc.validate(Limits::default()).unwrap();
    doc.graph
        .replace_vob_kind(id, VobKind::ZoneFog(ZoneFog::new(GameVersion::Gothic1)))
        .unwrap();
    assert!(doc.validate(Limits::default()).is_err());
}

#[test]
fn unknown_subtype_enums_remain_raw_and_binary_rejects_narrowing() {
    let mut doc = fresh(
        VobKind::MessageFilter(MessageFilter {
            on_trigger: MessageAction::from_raw(0x80000001),
            ..Default::default()
        }),
        GameVersion::Gothic2,
        Format::BinSafe,
        false,
    );
    for format in [Format::Ascii, Format::BinSafe] {
        doc.metadata.format = format;
        assert_eq!(vob(&reopen(&doc), 0).kind(), vob(&doc, 0).kind());
    }
    doc.metadata.format = Format::Binary;
    assert_eq!(
        doc.to_bytes(Limits::default()).unwrap_err().kind,
        ErrorKind::Unsupported
    );
}

#[test]
fn subtype_buffers_are_charged_cumulatively_on_read_and_orphan_validation() {
    let mut doc = fresh(
        VobKind::Item(Item {
            instance: b"payload".to_vec(),
            state: None,
        }),
        GameVersion::Gothic2,
        Format::BinSafe,
        false,
    );
    let exact = Limits {
        copied_bytes: doc.metadata.archiver.len() + 4 + 7,
        ..Limits::default()
    };
    let bytes = doc.to_bytes(exact).unwrap();
    ObjectArchive::from_bytes(&bytes, doc.game, exact).unwrap();
    let short = Limits {
        copied_bytes: exact.copied_bytes - 1,
        ..exact
    };
    assert_eq!(
        doc.validate(short).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(
        ObjectArchive::from_bytes(&bytes, doc.game, short)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    let mut orphan = Vob::new(doc.game);
    orphan.kind = VobKind::LensFlare(LensFlare {
        effect: vec![0; 64],
    });
    doc.graph.insert(Object::Vob(orphan)).unwrap();
    assert_eq!(
        doc.validate(exact).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
}

#[test]
fn preset_truncation_and_extra_fields_never_succeed_as_complete_records() {
    for (encoding, format) in [("binary", Format::Binary), ("bin_safe", Format::BinSafe)] {
        let bytes = fixture(&format!("light-preset-{encoding}-g2-dynamic"));
        for n in 0..bytes.len() {
            if let Ok(mut r) = archive::Reader::new(&bytes[..n], archive::Limits::default())
                && LightPreset::read(&mut r, GameVersion::Gothic2, 1024).is_ok()
            {
                assert!(r.finish().is_err(), "prefix {n}");
            }
        }
        let mut w =
            archive::Writer::new(archive::Header::new(format), archive::Limits::default()).unwrap();
        LightPreset::new(GameVersion::Gothic2)
            .write(&mut w, GameVersion::Gothic2)
            .unwrap();
        w.write(b"extra", archive::Value::Int(1)).unwrap();
        let bytes = w.finish().unwrap();
        let mut r = archive::Reader::new(&bytes, archive::Limits::default()).unwrap();
        LightPreset::read(&mut r, GameVersion::Gothic2, 1024).unwrap();
        assert!(r.finish().is_err());
    }
}

#[test]
fn tagged_field_names_are_checked_at_the_actual_subtype_record() {
    let bytes = fixture("vob-special-ascii-single-15");
    let needle = b"sndMode";
    let p = bytes
        .windows(needle.len())
        .position(|x| x == needle)
        .unwrap();
    let mut bad = bytes.clone();
    bad[p..p + needle.len()].copy_from_slice(b"sndFake");
    let e = ObjectArchive::from_bytes(&bad, GameVersion::Gothic2, Limits::default()).unwrap_err();
    assert!(e.offset.is_some_and(|n| n <= p && n + 8 >= p));
    assert_eq!(e.kind, ErrorKind::InvalidData);
}

#[test]
fn binary_special_float_bits_are_preserved_in_subtype_scalars() {
    for format in [Format::Binary, Format::BinSafe] {
        let doc = fresh(
            VobKind::Earthquake(Earthquake {
                radius: f32::from_bits(0x7fc01234),
                duration: -0.0,
                amplitude: Vec3::from([f32::from_bits(1), f32::INFINITY, f32::NEG_INFINITY]),
            }),
            GameVersion::Gothic2,
            format,
            false,
        );
        let out = reopen(&doc);
        let VobKindRef::Earthquake(v) = vob(&out, 0).kind() else {
            panic!()
        };
        assert_eq!(v.radius.to_bits(), 0x7fc01234);
        assert_eq!(v.duration.to_bits(), 0x80000000);
        assert_eq!(v.amplitude.x.to_bits(), 1);
        assert!(v.amplitude.y.is_infinite());
    }
}

#[test]
fn raw_debug_is_bounded_and_defaults_match_native_semantics() {
    let p = LightPreset::new(GameVersion::Gothic2);
    assert_eq!(p.light_type, LightType::SPOT);
    assert_eq!(p.quality, LightQuality::MEDIUM);
    assert_eq!(p.animation.unwrap().can_move, Some(true));
    assert_eq!(Sound::default().mode, SoundMode::ONCE);
    assert!(Sound::default().obstruction);
    assert!(MusicState::default().local_enabled);
    let mut p = LightPreset::new(GameVersion::Gothic2);
    p.preset = vec![b'X'; 100000];
    p.animation.as_mut().unwrap().colors = ColorAnimation::from_bytes(vec![b'Y'; 100000]);
    assert!(format!("{p:?}").len() < 1000);
    let a = VobKind::Item(Item {
        instance: vec![b'X'; 100000],
        state: None,
    });
    assert!(format!("{a:?}").len() < 200);
}

#[test]
fn iterative_nested_leaf_suffixes_fit_small_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            for format in formats() {
                let mut doc = fresh(
                    VobKind::Animate(Animate {
                        start_on: true,
                        running: Some(false),
                    }),
                    GameVersion::Gothic2,
                    format,
                    true,
                );
                let mut current = doc.graph.roots()[0].target.unwrap();
                for i in 0..63 {
                    let mut next = Vob::new(doc.game);
                    next.base.physics_enabled = false;
                    next.base.state = Some(SaveState::default());
                    next.kind = VobKind::Item(Item {
                        instance: format!("ITEM_{i}").into_bytes(),
                        state: Some(ItemState {
                            amount: i,
                            flags: -1,
                        }),
                    });
                    let next = doc.graph.insert(Object::Vob(next)).unwrap();
                    let ai = doc
                        .graph
                        .insert(Object::AiMove(AiMove {
                            vob: Link::new(b"vob".to_vec(), Some(next)),
                            owner: Link::default(),
                        }))
                        .unwrap();
                    let ObjectMut::Vob(mut v) = doc.graph.get_mut(current).unwrap() else {
                        panic!()
                    };
                    v.base_mut().ai = Some(Link::new(b"ai".to_vec(), Some(ai)));
                    current = next;
                }
                let out = reopen(&doc);
                assert_eq!(out.graph.len(), 127);
                assert_eq!(vob(&out, 0).kind(), vob(&doc, 0).kind());
                assert_eq!(
                    doc.to_bytes(Limits::default()).unwrap(),
                    out.to_bytes(Limits::default()).unwrap()
                );
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
