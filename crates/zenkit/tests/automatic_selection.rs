use zenkit::{
    ErrorKind, GameVersion,
    animation::Limits,
    archive::{self, Format, Header, Value, Writer},
    io::Reader,
    material::{Material, MaterialRecord},
    model_script::{ModelScript, SyntaxMode},
};

const FORMATS: [(&str, Format); 3] = [
    ("ascii", Format::Ascii),
    ("binary", Format::Binary),
    ("bin_safe", Format::BinSafe),
];
const GAMES: [GameVersion; 2] = [GameVersion::Gothic1, GameVersion::Gothic2];
const MDS: &[u8] = include_bytes!("../tests/fixtures/synthetic-animation-mds-g1.mds");
const MSB: &[u8] = include_bytes!("../tests/fixtures/synthetic-animation-msb-g1.msb");

fn material_fixture(encoding: &str, game: GameVersion, save: bool) -> Vec<u8> {
    let number = if game == GameVersion::Gothic1 { 1 } else { 2 };
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "tests/fixtures/synthetic-material-record-{encoding}-g{number}-{}.mat",
            if save { "save" } else { "normal" }
        )),
    )
    .unwrap()
}

#[test]
fn auto_material_matches_explicit_complete_records_and_exact_copy_budget() {
    for (encoding, _) in FORMATS {
        for game in GAMES {
            for save in [false, true] {
                let bytes = material_fixture(encoding, game, save);
                let mut explicit =
                    archive::Reader::new(&bytes, archive::Limits::default()).unwrap();
                explicit.read_int().unwrap();
                let expected = MaterialRecord::read(&mut explicit, game, 4096).unwrap();
                explicit.finish().unwrap();
                let copied = expected.prefix.len()
                    + expected.prefix_field_name.as_ref().map_or(0, Vec::len)
                    + expected.object_name.len()
                    + expected.material.name.len()
                    + expected.material.texture.len()
                    + expected.material.detail_object.len()
                    + expected.material.texture_scale.as_bytes().len()
                    + expected.material.texture_anim_map_dir.as_bytes().len();
                for cap in [copied, copied - 1] {
                    let mut r = archive::Reader::new(&bytes, archive::Limits::default()).unwrap();
                    r.read_int().unwrap();
                    let result = MaterialRecord::read_auto(&mut r, cap);
                    if cap == copied {
                        let actual = result.unwrap();
                        assert_eq!(actual, expected);
                        assert_eq!(actual.material.version.game(), game);
                        assert_eq!(actual.prefix, b"external\xfc");
                        assert_eq!(actual.object_name, b"%");
                        r.finish().unwrap();
                    } else {
                        assert_eq!(result.unwrap_err().kind, ErrorKind::LimitExceeded);
                    }
                }
            }
        }
    }
}

#[test]
fn auto_material_handles_mixed_games_and_leaves_the_caller_suffix() {
    for (_, format) in FORMATS {
        let mut w = Writer::new(Header::new(format), archive::Limits::default()).unwrap();
        for game in [GAMES[1], GAMES[0], GAMES[1]] {
            MaterialRecord {
                prefix: b"external".to_vec(),
                prefix_field_name: Some(b"label".to_vec()),
                object_name: b"material".to_vec(),
                material: Material::new(game),
            }
            .write(&mut w, game, 4096)
            .unwrap();
        }
        w.write(b"sentinel", Value::Int(-913)).unwrap();
        let bytes = w.finish().unwrap();
        let mut r = archive::Reader::new(&bytes, archive::Limits::default()).unwrap();
        for game in [GAMES[1], GAMES[0], GAMES[1]] {
            let record = MaterialRecord::read_auto(&mut r, 4096).unwrap();
            assert_eq!(record.material, Material::new(game));
            assert_eq!(record.material.version.game(), game);
        }
        assert_eq!(r.read_int().unwrap(), -913);
        r.finish().unwrap();
    }
}

#[test]
fn auto_material_rejects_unknown_versions_and_wrong_classes_at_the_header() {
    for (_, format) in FORMATS {
        for (class, version, kind) in [
            (b"zCMaterial".as_slice(), 0, ErrorKind::Unsupported),
            (b"zCMaterial", 0x4401, ErrorKind::Unsupported),
            (b"zCMaterial", 0x9c04, ErrorKind::Unsupported),
            (b"zCMaterial", u16::MAX, ErrorKind::Unsupported),
            (b"Other", 0x4400, ErrorKind::WrongObjectType),
            (b"%", 0, ErrorKind::WrongObjectType),
        ] {
            let mut w = Writer::new(Header::new(format), archive::Limits::default()).unwrap();
            w.write(b"prefix", Value::String(b"raw".as_slice().into()))
                .unwrap();
            w.begin_object(b"m", class, version).unwrap();
            w.end_object().unwrap();
            let bytes = w.finish().unwrap();
            let mut probe = archive::Reader::new(&bytes, archive::Limits::default()).unwrap();
            probe.read_string().unwrap();
            let header = probe.position();
            let mut r = archive::Reader::new(&bytes, archive::Limits::default()).unwrap();
            let error = MaterialRecord::read_auto(&mut r, 4096).unwrap_err();
            assert_eq!(error.kind, kind);
            assert_eq!(error.offset, Some(header));
            assert!(r.position() > header); // This streaming API consumes on error.
        }
    }
}

#[test]
fn auto_script_matches_complete_explicit_codecs_in_both_source_modes() {
    for mode in [SyntaxMode::Strict, SyntaxMode::Legacy] {
        assert_eq!(
            ModelScript::from_bytes(MDS, mode, Limits::default()).unwrap(),
            ModelScript::from_mds(MDS, mode, Limits::default()).unwrap()
        );
        assert_eq!(
            ModelScript::from_bytes(MSB, mode, Limits::default()).unwrap(),
            ModelScript::from_msb(MSB, Limits::default()).unwrap()
        );
    }
    let legacy = b"Model(\"M\"){ / comment\naniEnum{";
    let recovered = ModelScript::from_bytes(legacy, SyntaxMode::Legacy, Limits::default()).unwrap();
    assert!(!recovered.diagnostics.is_empty());
    assert_eq!(
        recovered,
        ModelScript::from_mds(legacy, SyntaxMode::Legacy, Limits::default()).unwrap()
    );
    assert!(ModelScript::from_bytes(legacy, SyntaxMode::Strict, Limits::default()).is_err());
}

#[test]
fn auto_script_embedded_windows_preserve_offsets_and_suffixes() {
    for bytes in [MDS, MSB] {
        let storage = [b"prefix".as_slice(), bytes, b"suffix"].concat();
        let mut r = Reader::new(&storage);
        r.take_bytes(6, "prefix").unwrap();
        let actual =
            ModelScript::read(&mut r, bytes.len(), SyntaxMode::Strict, Limits::default()).unwrap();
        let mut explicit = Reader::new(&storage);
        explicit.take_bytes(6, "prefix").unwrap();
        let expected = if bytes == MSB {
            ModelScript::read_msb(&mut explicit, bytes.len(), Limits::default())
        } else {
            ModelScript::read_mds(
                &mut explicit,
                bytes.len(),
                SyntaxMode::Strict,
                Limits::default(),
            )
        }
        .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(r.position(), explicit.position());
        assert_eq!(r.take_bytes(r.remaining(), "suffix").unwrap(), b"suffix");
    }
}

#[test]
fn auto_script_first_word_rule_does_not_fallback_or_inspect_a_short_span_suffix() {
    for first in [0xcfffu16, 0xd000, 0xefff, 0xf000, 0xf123, 0xffff] {
        let bytes = [b"pre".as_slice(), &first.to_le_bytes(), &[0; 4], b"suffix"].concat();
        let mut auto = Reader::new(&bytes);
        auto.take_bytes(3, "prefix").unwrap();
        let result = ModelScript::read(&mut auto, 6, SyntaxMode::Strict, Limits::default());
        let mut explicit = Reader::new(&bytes);
        explicit.take_bytes(3, "prefix").unwrap();
        let expected = if first >= 0xf000 || first == 0xd000 {
            ModelScript::read_msb(&mut explicit, 6, Limits::default())
        } else {
            ModelScript::read_mds(&mut explicit, 6, SyntaxMode::Strict, Limits::default())
        };
        let error = |e: zenkit::Error| (e.kind, e.offset, e.context);
        assert_eq!(result.map_err(error), expected.map_err(error));
        assert_eq!(auto.position(), explicit.position());
        for length in [0, 1] {
            let mut short = Reader::new(&bytes);
            short.take_bytes(3, "prefix").unwrap();
            let e = ModelScript::read(&mut short, length, SyntaxMode::Strict, Limits::default())
                .unwrap_err();
            assert_eq!(e.kind, ErrorKind::UnexpectedEof);
            assert_eq!(e.offset, Some(3));
            assert_eq!(short.position(), 3);
        }
    }
}

#[test]
fn auto_script_limits_and_selected_codec_errors_roll_back_parent_state() {
    for bytes in [MDS, MSB] {
        for limits in [
            Limits {
                bytes: bytes.len() - 1,
                ..Default::default()
            },
            Limits {
                owned_bytes: 0,
                ..Default::default()
            },
            Limits {
                elements: 0,
                ..Default::default()
            },
            Limits {
                string_bytes: 0,
                ..Default::default()
            },
            if bytes == MSB {
                Limits {
                    chunks: 0,
                    ..Default::default()
                }
            } else {
                Limits {
                    tokens: 0,
                    ..Default::default()
                }
            },
        ] {
            let storage = [b"prefix".as_slice(), bytes, b"suffix"].concat();
            let mut r = Reader::new(&storage);
            r.take_bytes(6, "prefix").unwrap();
            assert_eq!(
                ModelScript::read(&mut r, bytes.len(), SyntaxMode::Strict, limits)
                    .unwrap_err()
                    .kind,
                ErrorKind::LimitExceeded
            );
            assert_eq!(r.position(), 6);
        }
    }
}
