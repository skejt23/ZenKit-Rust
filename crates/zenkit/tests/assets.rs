use zenkit::objects::{ObjectMut, ObjectRef};
use zenkit::{
    ErrorKind, GameVersion,
    archive::{self, Format, Value},
    font,
    io::Reader,
    material::*,
    objects::{self, Link, Object, ObjectArchive},
    values::{Color, Vec2},
};
const FORMATS: [(&str, Format); 3] = [
    ("ascii", Format::Ascii),
    ("binary", Format::Binary),
    ("bin_safe", Format::BinSafe),
];
#[test]
fn material_constructor_defaults_match_each_game_schema() {
    let g1 = Material::new(GameVersion::Gothic1);
    assert!(matches!(g1.version, MaterialVersion::Gothic1));
    assert_eq!(g1.group, MaterialGroup::UNDEFINED);
    assert_eq!(g1.texture_scale.value(), Vec2::ZERO);
    let g2 = Material::new(GameVersion::Gothic2);
    let MaterialVersion::Gothic2(fields) = g2.version else {
        panic!()
    };
    assert_eq!(fields.alpha_func, AlphaFunction::NONE);
    assert_eq!(fields.wave_mode, WaveMode::NONE);
    assert_eq!(fields.wave_speed, WaveSpeed::NONE);
    assert_eq!(fields.detail_object_scale, 0.0);
    assert_eq!(AlphaFunction::default(), AlphaFunction::DEFAULT);
}
fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(format!("synthetic-{name}")),
    )
    .unwrap()
}
fn material_doc(encoding: &str, game: u8, save: bool) -> ObjectArchive {
    ObjectArchive::from_bytes(
        &fixture(&format!(
            "material-{encoding}-g{game}-{}.zen",
            if save { "save" } else { "normal" }
        )),
        if game == 1 {
            GameVersion::Gothic1
        } else {
            GameVersion::Gothic2
        },
        objects::Limits::default(),
    )
    .unwrap()
}
fn mat(doc: &ObjectArchive) -> &Material {
    let ObjectRef::Material(v) = doc.graph.get(doc.graph.roots()[0].target.unwrap()).unwrap()
    else {
        panic!()
    };
    v
}
fn assert_material(v: &Material, game: u8) {
    assert_eq!(v.name, b"STONE\xff");
    assert_eq!(v.group, MaterialGroup::STONE);
    assert_eq!(v.color, Color::from_rgba([17, 34, 51, 204]));
    assert_eq!(v.smooth_angle, 56.25);
    assert_eq!(v.texture, b"BRICK\xfe.TGA");
    assert_eq!(v.texture_scale.value(), Vec2::from([512.125, -64.5]));
    assert_eq!(v.texture_scale.as_bytes(), b" 512.125  -64.5 ");
    assert_eq!(v.texture_anim_fps, 12.5);
    assert_eq!(v.texture_anim_map_mode, AnimationMapping::LINEAR);
    assert_eq!(v.texture_anim_map_dir.value(), Vec2::from([0.125, -0.375]));
    assert!(v.disable_collision);
    assert!(!v.disable_lightmap);
    assert!(v.dont_collapse);
    assert_eq!(v.detail_object, b"DETAIL\xfd.3DS");
    assert_eq!(v.default_mapping, Vec2::from([2.34375, -1.125]));
    if game == 1 {
        assert!(matches!(v.version, MaterialVersion::Gothic1));
    } else {
        let MaterialVersion::Gothic2(g) = &v.version else {
            panic!()
        };
        assert_eq!(g.detail_object_scale, 1.75);
        assert!(g.force_occluder);
        assert!(g.environment_mapping);
        assert_eq!(g.environment_mapping_strength, 0.625);
        assert_eq!(g.wave_mode, WaveMode::WIND);
        assert_eq!(g.wave_speed, WaveSpeed::FAST);
        assert_eq!(g.wave_max_amplitude, 30.25);
        assert_eq!(g.wave_grid_size, 100.5);
        assert!(g.ignore_sun);
        assert_eq!(g.alpha_func, AlphaFunction::ADD);
    }
}
#[test]
fn material_all_fields_games_modes_encodings_and_shared_identity() {
    for (name, _) in FORMATS {
        for game in [1, 2] {
            for save in [false, true] {
                let d = material_doc(name, game, save);
                assert_eq!(d.metadata.save_game, save);
                assert_eq!(d.graph.len(), 1);
                assert_eq!(d.graph.roots()[0].target, d.graph.roots()[1].target);
                assert_material(mat(&d), game);
                let bytes = d.to_bytes(objects::Limits::default()).unwrap();
                let reread =
                    ObjectArchive::from_bytes(&bytes, d.game, objects::Limits::default()).unwrap();
                assert_material(mat(&reread), game);
                assert_eq!(mat(&d), mat(&reread));
            }
        }
    }
}
#[test]
fn material_mutation_retains_shared_identity_and_kind_checks() {
    let mut d = material_doc("binary", 2, true);
    let id = d.graph.roots()[0].target.unwrap();
    let ObjectMut::Material(v) = d.graph.get_mut(id).unwrap() else {
        panic!()
    };
    v.name = b"edited\xfe".to_vec();
    let bytes = d.to_bytes(objects::Limits::default()).unwrap();
    let restored = ObjectArchive::from_bytes(&bytes, d.game, objects::Limits::default()).unwrap();
    assert_eq!(mat(&restored).name, b"edited\xfe");
    assert_eq!(
        restored.graph.roots()[0].target,
        restored.graph.roots()[1].target
    );
    d.graph
        .insert(Object::Atomic(objects::AtomicBlock {
            message: Link::new(b"bad".to_vec(), Some(id)),
            synchronized: None,
        }))
        .unwrap();
    assert_eq!(
        d.validate(objects::Limits::default()).unwrap_err().kind,
        ErrorKind::WrongObjectType
    );
}
#[test]
fn material_game_mismatch_including_orphans_requires_explicit_conversion() {
    let mut d = material_doc("ascii", 1, false);
    d.game = GameVersion::Gothic2;
    assert_eq!(
        d.validate(objects::Limits::default()).unwrap_err().kind,
        ErrorKind::Unsupported
    );
    assert!(d.to_bytes(objects::Limits::default()).is_err());
    let mut d = material_doc("binary", 1, false);
    d.graph
        .insert(Object::Material(Material::new(GameVersion::Gothic2)))
        .unwrap();
    assert!(d.validate(objects::Limits::default()).is_err());
    for (name, _) in FORMATS {
        let bytes = fixture(&format!("material-{name}-g1-normal.zen"));
        assert_eq!(
            ObjectArchive::from_bytes(&bytes, GameVersion::Gothic2, objects::Limits::default())
                .unwrap_err()
                .kind,
            ErrorKind::Unsupported
        );
    }
}
#[test]
fn unknown_material_enum_values_are_preserved_without_narrowing() {
    for (_, format) in FORMATS {
        let mut d = material_doc("ascii", 2, false);
        d.metadata.format = format;
        let id = d.graph.roots()[0].target.unwrap();
        let ObjectMut::Material(v) = d.graph.get_mut(id).unwrap() else {
            panic!()
        };
        v.group = u32::MAX.into();
        v.texture_anim_map_mode = 12345.into();
        let MaterialVersion::Gothic2(g) = &mut v.version else {
            panic!()
        };
        g.wave_mode = 999.into();
        g.wave_speed = 1000.into();
        g.alpha_func = 99999.into();
        if format == Format::Binary {
            assert_eq!(
                d.to_bytes(objects::Limits::default()).unwrap_err().kind,
                ErrorKind::Unsupported
            );
        } else {
            let bytes = d.to_bytes(objects::Limits::default()).unwrap();
            let restored =
                ObjectArchive::from_bytes(&bytes, d.game, objects::Limits::default()).unwrap();
            assert_eq!(mat(&restored), mat(&d));
        }
    }
    assert_eq!(u32::from(MaterialGroup::NONE), 255);
    assert_eq!(WaveMode::from_raw(123).raw(), 123);
}
#[test]
fn text_vectors_preserve_spelling_and_exact_finite_values() {
    let vt = TextVec2::from_bytes(b"1\x0b2".to_vec()).unwrap();
    assert_eq!(vt.value(), Vec2::from([1.0, 2.0]));
    assert_eq!(vt.as_bytes(), b"1\x0b2");
    let v = TextVec2::from_bytes(b" \t-0  1.40129846e-45 \t".to_vec()).unwrap();
    assert_eq!(v.value().x.to_bits(), 0x80000000);
    assert_eq!(v.value().y.to_bits(), 1);
    assert_eq!(v.as_bytes(), b" \t-0  1.40129846e-45 \t");
    for bits in [0, 0x80000000, 1, 0x7f7fffff, 0x3f9e0652] {
        let x = f32::from_bits(bits);
        let v = TextVec2::from_value([x, -x].into()).unwrap();
        let r = TextVec2::from_bytes(v.as_bytes().to_vec()).unwrap();
        assert_eq!(r.value().x.to_bits(), bits);
        assert_eq!(r.value().y.to_bits(), (-x).to_bits());
    }
    for bytes in [b"".as_slice(), b"1", b"1 bad", b"1 2 3", b"1\xff 2"] {
        assert_eq!(
            TextVec2::from_bytes(bytes.to_vec()).unwrap_err().kind,
            ErrorKind::InvalidData
        );
    }
    for bytes in [b"NaN 1".as_slice(), b"1 inf", b"1e100 2"] {
        assert_eq!(
            TextVec2::from_bytes(bytes.to_vec()).unwrap_err().kind,
            ErrorKind::Unsupported
        );
    }
    assert!(TextVec2::from_value([f32::NAN, 1.0].into()).is_err());
}
#[test]
fn malformed_vector_errors_report_real_payload_token_offsets() {
    for (name, _) in FORMATS {
        let mut bytes = fixture(&format!("material-{name}-g1-normal.zen"));
        let p = bytes.windows(5).position(|b| b == b"-64.5").unwrap();
        bytes[p..p + 5].copy_from_slice(b"error");
        let e = ObjectArchive::from_bytes(&bytes, GameVersion::Gothic1, objects::Limits::default())
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::InvalidData);
        assert_eq!(e.offset, Some(p));
    }
}
#[test]
fn material_versions_names_types_and_extra_fields_are_strict() {
    let bytes = fixture("material-ascii-g2-normal.zen");
    for (from, to, kind) in [
        (
            b"39939".as_slice(),
            b"39938".as_slice(),
            ErrorKind::Unsupported,
        ),
        (
            b"smoothAngle=float:".as_slice(),
            b"wrongField=float:".as_slice(),
            ErrorKind::InvalidData,
        ),
        (
            b"alphaFunc=enum:3".as_slice(),
            b"alphaFunc=bool:1".as_slice(),
            ErrorKind::InvalidData,
        ),
        (
            b"defaultMapping=rawFloat:2.34375 -1.125".as_slice(),
            b"defaultMapping=rawFloat:2.34375 -1.125\nextra=int:3".as_slice(),
            ErrorKind::Unsupported,
        ),
    ] {
        let p = bytes.windows(from.len()).position(|b| b == from).unwrap();
        let mut changed = bytes.clone();
        changed.splice(p..p + from.len(), to.iter().copied());
        assert_eq!(
            ObjectArchive::from_bytes(&changed, GameVersion::Gothic2, objects::Limits::default())
                .unwrap_err()
                .kind,
            kind
        );
    }
    let p = bytes
        .windows(15)
        .position(|b| b == b"lodDontCollapse")
        .unwrap();
    let mut changed = bytes.clone();
    changed[p + 2] = b's';
    let d = ObjectArchive::from_bytes(&changed, GameVersion::Gothic2, objects::Limits::default())
        .unwrap();
    assert!(mat(&d).dont_collapse);
}
#[test]
fn material_finite_ascii_and_binary_special_float_bits() {
    for (_, format) in FORMATS {
        for bits in [0x3f9e0652, 1, 0x80000000, 0x7fc12345, 0x7f800000] {
            if format == Format::Ascii && !f32::from_bits(bits).is_finite() {
                continue;
            }
            let mut d = material_doc("binary", 2, false);
            d.metadata.format = format;
            let id = d.graph.roots()[0].target.unwrap();
            let ObjectMut::Material(v) = d.graph.get_mut(id).unwrap() else {
                panic!()
            };
            v.smooth_angle = f32::from_bits(bits);
            v.default_mapping.x = f32::from_bits(bits);
            let bytes = d.to_bytes(objects::Limits::default()).unwrap();
            let r = ObjectArchive::from_bytes(&bytes, d.game, objects::Limits::default()).unwrap();
            assert_eq!(mat(&r).smooth_angle.to_bits(), bits);
            assert_eq!(mat(&r).default_mapping.x.to_bits(), bits);
        }
    }
}
#[test]
fn material_owned_bytes_include_text_spelling_and_orphans() {
    let d = material_doc("ascii", 1, false);
    let v = mat(&d);
    let exact = d.metadata.archiver.len()
        + d.metadata.date.as_ref().unwrap().len()
        + d.metadata.user.as_ref().unwrap().len()
        + b"materialalias".len()
        + v.name.len()
        + v.texture.len()
        + v.detail_object.len()
        + v.texture_scale.as_bytes().len()
        + v.texture_anim_map_dir.as_bytes().len();
    let l = objects::Limits {
        copied_bytes: exact,
        ..Default::default()
    };
    d.validate(l).unwrap();
    d.to_bytes(l).unwrap();
    let short = objects::Limits {
        copied_bytes: exact - 1,
        ..l
    };
    assert!(d.validate(short).is_err());
    assert!(
        ObjectArchive::from_bytes(&fixture("material-ascii-g1-normal.zen"), d.game, short).is_err()
    );
}
#[test]
fn material_prefix_wrapper_preserves_distinct_names_and_shared_codec() {
    for (name, format) in FORMATS {
        for game in [1, 2] {
            for save in [false, true] {
                let bytes = fixture(&format!(
                    "material-record-{name}-g{game}-{}.mat",
                    if save { "save" } else { "normal" }
                ));
                let mut r = archive::Reader::new(&bytes, Default::default()).unwrap();
                let header = r.header();
                assert_eq!(r.read_int().unwrap(), 1);
                let g = if game == 1 {
                    GameVersion::Gothic1
                } else {
                    GameVersion::Gothic2
                };
                let record = MaterialRecord::read(&mut r, g, 10000).unwrap();
                assert_eq!(record.prefix, b"external\xfc");
                assert_eq!(record.object_name, b"%");
                assert_eq!(
                    record.prefix_field_name.as_deref(),
                    if format == Format::Binary {
                        None
                    } else {
                        Some(b"material".as_slice())
                    }
                );
                assert_material(&record.material, game);
                r.finish().unwrap();
                let mut w = archive::Writer::new(header, Default::default()).unwrap();
                w.write(b"count", Value::Int(1)).unwrap();
                record.write(&mut w, g, 10000).unwrap();
                let out = w.finish().unwrap();
                let mut r = archive::Reader::new(&out, Default::default()).unwrap();
                r.read_int().unwrap();
                assert_eq!(MaterialRecord::read(&mut r, g, 10000).unwrap(), record);
                r.finish().unwrap();
            }
        }
    }
}
#[test]
fn material_prefix_wrapper_has_explicit_consuming_error_and_budget() {
    let bytes = fixture("material-record-ascii-g1-normal.mat");
    let mut r = archive::Reader::new(&bytes, Default::default()).unwrap();
    r.read_int().unwrap();
    let start = r.position();
    assert!(MaterialRecord::read(&mut r, GameVersion::Gothic1, 0).is_err());
    assert!(r.position() > start); // discard reader; no retry contract
    let mut r = archive::Reader::new(&bytes, Default::default()).unwrap();
    r.read_int().unwrap();
    let mut record = MaterialRecord::read(&mut r, GameVersion::Gothic1, 10000).unwrap();
    record.prefix_field_name = None;
    let mut w =
        archive::Writer::new(archive::Header::new(Format::Ascii), Default::default()).unwrap();
    record.write(&mut w, GameVersion::Gothic1, 10000).unwrap();
    let bytes = w.finish().unwrap();
    assert!(bytes.windows(9).any(|b| b == b"material="));
}
#[test]
fn fonts_preserve_all_widths_and_uv_bits_from_independent_planes() {
    for kind in ["g1", "g2", "special", "empty", "leading"] {
        let bytes = fixture(&format!("font-{kind}.fnt"));
        let f = font::Font::from_bytes(&bytes, Default::default()).unwrap();
        assert_eq!(f.to_bytes(Default::default()).unwrap(), bytes);
        if kind == "g1" || kind == "g2" || kind == "leading" {
            let game = if kind == "g2" { 2 } else { 1 };
            assert_eq!(f.height, 16 + game);
            assert_eq!(f.glyphs.len(), 256);
            for (i, g) in f.glyphs.iter().enumerate() {
                assert_eq!(g.width, ((i * 17 + game as usize) & 255) as u8);
                let x = ((i % 17) as f32 - 2.) / 16.;
                let y = (i / 17) as f32 / 16.;
                assert_eq!(
                    g.uv,
                    [Vec2::from([x, y]), Vec2::from([x + 0.0625, y + 0.125])]
                );
            }
        }
        if kind == "special" {
            assert_eq!(f.height, u32::MAX);
            assert_eq!(f.glyphs[0].uv[0].x.to_bits(), 0x80000000);
            assert_eq!(f.glyphs[0].uv[0].y.to_bits(), 0x7fc12345);
            assert_eq!(f.glyphs[1].uv[0].x.to_bits(), 1);
        }
    }
}
#[test]
fn font_every_truncation_and_trailing_bytes_fail() {
    let bytes = fixture("font-special.fnt");
    for end in 0..bytes.len() {
        assert!(font::Font::from_bytes(&bytes[..end], Default::default()).is_err());
    }
    let mut extra = bytes;
    extra.push(0);
    assert_eq!(
        font::Font::from_bytes(&extra, Default::default())
            .unwrap_err()
            .kind,
        ErrorKind::InvalidData
    );
}
#[test]
fn font_counts_and_payload_sizes_are_checked_before_allocation() {
    let bytes = fixture("font-special.fnt");
    let mut huge = bytes.clone();
    let p = b"1\nSPECIAL\xfe\n".len() + 4;
    huge[p..p + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    let e = font::Font::from_bytes(&huge, Default::default()).unwrap_err();
    assert_eq!(e.kind, ErrorKind::LimitExceeded);
    assert_eq!(e.offset, Some(p));
    let e = font::Font::from_bytes(
        &huge,
        font::Limits {
            glyphs: usize::MAX,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(e.kind, ErrorKind::LimitExceeded);
    let f = font::Font::from_bytes(&bytes, Default::default()).unwrap();
    let exact = font::Limits {
        bytes: bytes.len(),
        glyphs: 2,
        name_bytes: f.name.len(),
    };
    assert_eq!(f.to_bytes(exact).unwrap(), bytes);
    assert!(font::Font::from_bytes(&bytes, font::Limits { glyphs: 1, ..exact }).is_err());
    assert!(
        f.to_bytes(font::Limits {
            bytes: bytes.len() - 1,
            ..exact
        })
        .is_err()
    );
}
#[test]
fn font_line_endings_whitespace_and_binary_boundary_are_explicit() {
    for name in [b" \tFONT \xff".as_slice(), b"", b"plain"] {
        let mut bytes = b"1\r\n".to_vec();
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(b"\r\n");
        bytes.extend_from_slice(&10u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        let f = font::Font::from_bytes(&bytes, Default::default()).unwrap();
        assert_eq!(f.name, name);
        assert_eq!(f.height, 10);
        let canonical = f.to_bytes(Default::default()).unwrap();
        assert_eq!(&canonical[..2], b"1\n");
        assert_eq!(
            font::Font::from_bytes(&canonical, Default::default()).unwrap(),
            f
        );
    }
}
#[test]
fn font_embedded_read_is_transactional_and_preserves_parent_remainder() {
    let bytes = fixture("font-special.fnt");
    let mut enclosing = vec![9; 7];
    enclosing.extend_from_slice(&bytes);
    enclosing.push(42);
    let mut r = Reader::new(&enclosing);
    r.take_bytes(7, "prefix").unwrap();
    let start = r.position();
    let f = font::Font::read(&mut r, Default::default()).unwrap();
    assert_eq!(f.glyphs.len(), 2);
    assert_eq!(r.u8("tail").unwrap(), 42);
    let mut r = Reader::new(&enclosing[..enclosing.len() - 2]);
    r.take_bytes(7, "prefix").unwrap();
    assert!(font::Font::read(&mut r, Default::default()).is_err());
    assert_eq!(r.position(), start);
}
#[test]
fn font_rejects_unsupported_version_and_unrepresentable_names() {
    let mut bytes = fixture("font-empty.fnt");
    bytes[0] = b'2';
    assert_eq!(
        font::Font::from_bytes(&bytes, Default::default())
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
    for name in [b"a\0b".as_slice(), b"a\rb", b"a\nb"] {
        let f = font::Font {
            name: name.to_vec(),
            ..Default::default()
        };
        assert_eq!(
            f.to_bytes(Default::default()).unwrap_err().kind,
            ErrorKind::Unsupported
        );
    }
}
#[test]
fn asset_debug_summaries_do_not_expand_payloads() {
    let f = font::Font {
        name: vec![b'x'; 100000],
        glyphs: vec![Default::default(); 10000],
        ..Default::default()
    };
    assert!(format!("{f:?}").len() < 120);
    let m = Material {
        name: vec![b'x'; 100000],
        ..Default::default()
    };
    assert!(format!("{m:?}").len() < 180);
    let mut bytes = vec![b' '; 100000];
    bytes.extend_from_slice(b"1 2");
    let v = TextVec2::from_bytes(bytes).unwrap();
    assert!(format!("{v:?}").len() < 120);
}
