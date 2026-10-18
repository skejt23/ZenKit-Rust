use zenkit::objects::ObjectRef;
use zenkit::{
    ErrorKind, GameVersion,
    archive::{self, Format, Value as V},
    objects::{self, Object, ObjectArchive},
    savegame::*,
};
include!("savegame_cases.inc");
fn fixture(label: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(format!("synthetic-{label}.sav")),
    )
    .unwrap()
}
fn canonical_header(bytes: &[u8]) -> Vec<u8> {
    let Some(at) = bytes.windows(8).position(|v| v == b"objects ") else {
        return bytes.to_vec();
    };
    let end = at + bytes[at..].iter().position(|b| *b == b'\n').unwrap();
    let count = std::str::from_utf8(&bytes[at + 8..end])
        .unwrap()
        .parse::<u32>()
        .unwrap();
    let mut result = bytes[..at].to_vec();
    result.extend_from_slice(format!("objects {count:010}").as_bytes());
    result.extend_from_slice(&bytes[end..]);
    result
}
fn game(n: u8) -> GameVersion {
    if n == 1 {
        GameVersion::Gothic1
    } else {
        GameVersion::Gothic2
    }
}
#[test]
fn independent_full_field_fixtures_and_malformed_records() {
    assert!(std::mem::size_of::<ObjectRef<'_>>() <= 64);
    let mut accepted = 0;
    let mut rejected = 0;
    for &(label, valid, metadata, g, variant) in CASES {
        let input = fixture(label);
        if metadata {
            let result = ObjectArchive::from_bytes(&input, game(g), Default::default());
            if !valid {
                assert!(result.is_err(), "{label}");
                rejected += 1;
                continue;
            }
            let d = result.unwrap_or_else(|e| panic!("{label}: {e}"));
            let ObjectRef::SaveMetadata(v) =
                d.graph.get(d.graph.roots()[0].target.unwrap()).unwrap()
            else {
                panic!()
            };
            assert_eq!(v.title, b"SAVE_TITLE\xff");
            assert_eq!(v.world, b"WORLD_SYNTH");
            assert_eq!((v.day, v.hour, v.minute), (-13, 23, 59));
            assert_eq!(v.save_date, b"free-form date\xfe");
            assert_eq!(
                (v.version_major, v.version_minor, v.play_time_seconds),
                (-2, 37, -91)
            );
            if let MetadataVersion::Gothic2(v) = &v.version {
                assert_eq!((v.version_point, v.version_int), (-7, 12345));
                assert_eq!(v.application, b"APP\xfd");
            } else {
                assert_eq!(g, 1);
            }
            assert_eq!(
                d.to_bytes(Default::default()).unwrap(),
                canonical_header(&input),
                "{label}"
            );
        } else {
            let result = StateArchive::from_bytes(&input, game(g), Default::default());
            if !valid {
                assert!(result.is_err(), "{label}");
                rejected += 1;
                continue;
            }
            let d = result.unwrap_or_else(|e| panic!("{label}: {e}"));
            let s = &d.fields;
            assert_eq!((s.day, s.hour, s.minute), (-12, 23, 59));
            if variant != "empty" {
                assert_eq!(s.infos.len(), 2);
                assert!(s.infos[0].told && !s.infos[1].told);
                assert_eq!(
                    (
                        s.missions[0].id,
                        s.missions[0].available,
                        s.missions[0].status_index
                    ),
                    (-7, true, -91)
                );
                assert_eq!(s.missions[0].history.len(), 2);
                assert_eq!(s.missions[0].history[0].vob_id, -3);
                assert_eq!(s.missions[0].history[1].status, -17);
                assert_eq!(s.missions[1].status_index, 999);
                assert_eq!(s.log[0].manager_entry_count, -29);
                assert_eq!(s.log[0].entries.len(), 2);
                assert!(s.log[1].entries.is_empty());
                if variant == "special-floats" {
                    assert_eq!(s.missions[0].history[0].start_time.to_bits(), 0x80000000);
                    assert_eq!(s.missions[0].history[1].start_time.to_bits(), 0x7fc12345);
                } else {
                    assert_eq!(s.missions[0].history[1].start_time, -33.5);
                }
                match &s.symbols {
                    Symbols::Gothic1(v) => {
                        assert_eq!(
                            v[0].value,
                            if variant == "integer-range" {
                                -123456
                            } else {
                                i32::MIN
                            }
                        );
                        assert_eq!(v[1].value, i32::MAX);
                    }
                    Symbols::Gothic2(v) => {
                        assert!(v[0].value.values.is_empty());
                        assert_eq!(
                            v[1].value.values,
                            [
                                -2,
                                7,
                                i32::MAX,
                                if variant == "integer-range" {
                                    -123456
                                } else {
                                    i32::MIN
                                }
                            ]
                        );
                        assert_eq!(
                            v[1].value.names,
                            if variant == "repeated-zero" && d.metadata.format != Format::Binary {
                                ValueNames::RepeatedZero
                            } else {
                                ValueNames::Indexed
                            }
                        );
                    }
                }
            }
            for row in 0..42 {
                for col in 0..42 {
                    assert_eq!(
                        s.guild_attitudes[row][col],
                        ((row * 7 + col * 13) & 255) as u8
                    );
                }
            }
            if let Some(m) = d.manager().unwrap() {
                if variant == "empty" {
                    assert!(m.pool_items.is_empty());
                } else {
                    assert_eq!(m.pool_items.len(), 4);
                    assert_eq!(m.pool_items[0].target, m.pool_items[2].target);
                    assert_eq!(m.pool_items[1].target, None);
                }
            }
            if variant == "manager-alias" {
                assert_eq!(
                    d.graph
                        .record_version(d.graph.roots()[0].target.unwrap(), d.game)
                        .unwrap(),
                    36865
                );
            }
            let bytes = d
                .to_bytes(Default::default())
                .unwrap_or_else(|e| panic!("write {label}: {e}"));
            if variant != "noncanonical-bool" {
                assert!(
                    bytes == canonical_header(&input),
                    "byte rewrite differs: {label}"
                );
            } else {
                assert_ne!(bytes, canonical_header(&input));
            }
            StateArchive::from_bytes(&bytes, game(g), Default::default()).unwrap();
        }
        accepted += 1;
    }
    assert_eq!((accepted, rejected), (95, 72));
}
#[test]
fn shared_budgets_cover_both_scalar_halves_graph_and_orphans() {
    let bytes = fixture("savedat-bin_safe-g2-s1-canonical");
    let mut d = StateArchive::from_bytes(&bytes, GameVersion::Gothic2, Default::default()).unwrap();
    let base = objects::Limits {
        nodes: 3,
        roots: 1,
        links: 5,
        sequence_items: 18,
        ..Default::default()
    };
    d.validate(base).unwrap();
    StateArchive::from_bytes(&bytes, d.game, base).unwrap();
    for limits in [
        objects::Limits { nodes: 2, ..base },
        objects::Limits { roots: 0, ..base },
        objects::Limits { links: 4, ..base },
        objects::Limits {
            sequence_items: 17,
            ..base
        },
    ] {
        assert_eq!(
            d.validate(limits).unwrap_err().kind,
            ErrorKind::LimitExceeded
        );
        assert_eq!(
            StateArchive::from_bytes(&bytes, d.game, limits)
                .unwrap_err()
                .kind,
            ErrorKind::LimitExceeded
        );
    }
    let (mut low, mut high) = (0, 16384);
    while low < high {
        let mid = (low + high) / 2;
        let limits = objects::Limits {
            copied_bytes: mid,
            ..base
        };
        if d.validate(limits).is_ok() {
            high = mid
        } else {
            low = mid + 1
        }
    }
    let exact = objects::Limits {
        copied_bytes: low,
        ..base
    };
    StateArchive::from_bytes(&bytes, d.game, exact).unwrap();
    assert!(
        StateArchive::from_bytes(
            &bytes,
            d.game,
            objects::Limits {
                copied_bytes: low - 1,
                ..base
            }
        )
        .is_err()
    );
    assert_eq!(d.to_bytes(exact).unwrap(), bytes);
    d.graph
        .insert(Object::SaveMetadata(SaveMetadata::new(
            GameVersion::Gothic1,
        )))
        .unwrap();
    assert!(d.validate(Default::default()).is_err());
}
#[test]
fn fresh_writes_preserve_all_data_without_synthetic_wrapper() {
    for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
        for format in [Format::Ascii, Format::Binary, Format::BinSafe] {
            let mut s = StateArchive::new(game, format).unwrap();
            s.metadata.save_game = false;
            s.fields.day = -99;
            s.fields.missions.push(Mission {
                name: b"test".to_vec(),
                id: -1,
                available: true,
                history: vec![MissionHistory {
                    vob_id: 7,
                    status: -8,
                    start_time: 2.5,
                }],
                status_index: -44,
            });
            s.fields.log.push(LogTopic {
                manager_entry_count: -1,
                entries: vec![b"one".to_vec()],
                ..Default::default()
            });
            let bytes = s.to_bytes(Default::default()).unwrap();
            let mut r = archive::Reader::new(&bytes, Default::default()).unwrap();
            assert!(!r.header().save_game);
            assert_eq!(r.read_int().unwrap(), -99); // first bytes are a field, not an invented graph wrapper
            let again = StateArchive::from_bytes(&bytes, game, Default::default()).unwrap();
            assert_eq!(again.fields, s.fields);
            assert!(again.manager().unwrap().is_none());
        }
    }
}
#[test]
fn late_errors_and_truncations_do_not_return_partial_state() {
    for format in [Format::Ascii, Format::Binary, Format::BinSafe] {
        let mut w = archive::Writer::new(archive::Header::new(format), Default::default()).unwrap();
        for name in [
            b"day".as_slice(),
            b"hour",
            b"min",
            b"NumOfEntries",
            b"NumOfEntries",
            b"LOGMANAGERTOPICCOUNT",
        ] {
            w.write(name, V::Int(0)).unwrap();
        }
        w.begin_object(b"manager", b"%", 0).unwrap();
        w.end_object().unwrap();
        w.write(b"numSymbols", V::Int(0)).unwrap();
        w.write(b"guildTable", V::Raw(vec![0; 1764].into()))
            .unwrap();
        let bytes = w.finish().unwrap();
        for i in 0..bytes.len() {
            if format == Format::Ascii && i + 1 == bytes.len() {
                continue;
            }
            assert!(
                StateArchive::from_bytes(&bytes[..i], GameVersion::Gothic1, Default::default())
                    .is_err(),
                "{format:?} {i}"
            );
        }
    }
}
#[test]
fn empty_strings_have_sequence_cost_and_large_valid_arrays_are_not_overpreflighted() {
    let mut d = StateArchive::new(GameVersion::Gothic1, Format::Binary).unwrap();
    d.fields.infos = vec![Info::default(); 4096];
    let bytes = d.to_bytes(Default::default()).unwrap();
    let restored = StateArchive::from_bytes(&bytes, d.game, Default::default()).unwrap();
    assert_eq!(restored.fields.infos.len(), 4096);
}
