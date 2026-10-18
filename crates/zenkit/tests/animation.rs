use zenkit::{
    ErrorKind, GameVersion,
    animation::{Limits, ModelAnimation, PackedSample},
    io::Reader,
    model_script::*,
    morph::MorphMesh,
};
const MAN: &[u8] = include_bytes!("../tests/fixtures/synthetic-animation-man-g1.man");
const MMB: &[u8] = include_bytes!("../tests/fixtures/synthetic-animation-mmb-g1.mmb");
const MDS: &[u8] = include_bytes!("../tests/fixtures/synthetic-animation-mds-g1.mds");
const MSB: &[u8] = include_bytes!("../tests/fixtures/synthetic-animation-msb-g1.msb");
fn chunk(id: u16, data: &[u8]) -> Vec<u8> {
    [
        id.to_le_bytes().as_slice(),
        &(data.len() as u32).to_le_bytes(),
        data,
    ]
    .concat()
}
fn parts(bytes: &[u8]) -> Vec<(u16, usize, Vec<u8>)> {
    let mut out = vec![];
    let mut p = 0;
    while p < bytes.len() {
        let id = u16::from_le_bytes(bytes[p..p + 2].try_into().unwrap());
        let n = u32::from_le_bytes(bytes[p + 2..p + 6].try_into().unwrap()) as usize;
        out.push((id, p, bytes[p + 6..p + 6 + n].to_vec()));
        p += 6 + n;
    }
    out
}
fn edited(bytes: &[u8], id: u16, edit: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut chunks = parts(bytes);
    edit(&mut chunks.iter_mut().find(|c| c.0 == id).unwrap().2);
    chunks
        .into_iter()
        .flat_map(|(id, _, v)| chunk(id, &v))
        .collect()
}
fn source(body: &str, mode: SyntaxMode) -> zenkit::Result<ModelScript> {
    ModelScript::from_mds(
        format!("Model(\"M\"){{aniEnum{{{body}}}}}").as_bytes(),
        mode,
        Limits::default(),
    )
}
#[test]
fn man_fields_and_frame_major_views() {
    let a = ModelAnimation::from_bytes(MAN, Limits::default()).unwrap();
    assert_eq!(a.version, 12);
    assert!(a.marker);
    assert_eq!(a.name, b"RUN\xff");
    assert_eq!(a.next, b"NEXT\xfe");
    assert_eq!(a.node_indices, [0, 4]);
    assert_eq!(a.packed_samples.len(), 6);
    assert_eq!(a.sample(1, 0), a.samples().nth(2));
    assert_eq!(a.sample(3, 0), None);
    assert_eq!(a.sample(0, 4), None);
    assert_eq!(a.source.as_ref().unwrap().date.reserved, 0xabcd);
    assert_eq!(a.events.as_ref().unwrap().len(), 13);
    assert_eq!(a.events.as_ref().unwrap()[12].kind.0, u32::MAX);
    assert_eq!(a.samples().len(), 6);
    assert_eq!(a.samples().next_back(), a.sample(2, 1));
}
#[test]
fn packed_decode_identity_and_normalization() {
    let p = PackedSample {
        rotation: [32767; 3],
        position: [0, 1, 65535],
    };
    let d = p.decode(0.5, -1.);
    assert_eq!(d.rotation.to_wxyz(), [1., 0., 0., 0.]);
    assert_eq!(d.position.to_array(), [-1., -0.5, 32766.5]);
    let d = PackedSample {
        rotation: [65535; 3],
        ..p
    }
    .decode(1., 0.);
    assert_eq!(d.rotation.w, 0.);
    let q = d.rotation;
    assert!((q.x * q.x + q.y * q.y + q.z * q.z - 1.).abs() < 0.000001);
}
#[test]
fn man_optional_presence_and_raw_nonfinite() {
    let mut bytes = MAN.to_vec();
    bytes = edited(&bytes, 0xa030, |v| {
        v.clear();
        v.extend(0u32.to_le_bytes());
    });
    let a = ModelAnimation::from_bytes(&bytes, Limits::default()).unwrap();
    assert_eq!(a.events, Some(vec![]));
    let noevents: Vec<_> = parts(MAN)
        .into_iter()
        .filter(|c| c.0 != 0xa030 && c.0 != 0xa010 && c.0 != 0xa000)
        .flat_map(|(id, _, v)| chunk(id, &v))
        .collect();
    let a = ModelAnimation::from_bytes(&noevents, Limits::default()).unwrap();
    assert!(a.events.is_none() && a.source.is_none() && !a.marker);
    let bytes = edited(MAN, 0xa020, |v| {
        let p = 2 + 5 + 12;
        v[p..p + 4].copy_from_slice(&0x7fc01234u32.to_le_bytes());
    });
    assert_eq!(
        ModelAnimation::from_bytes(&bytes, Limits::default())
            .unwrap()
            .fps
            .to_bits(),
        0x7fc01234
    );
}
#[test]
fn binary_every_truncation_rolls_back() {
    for (kind, bytes) in [(0, MAN), (1, MMB), (2, MSB)] {
        for n in 0..bytes.len() {
            let mut prefixed = vec![7; 13];
            prefixed.extend_from_slice(&bytes[..n]);
            let mut r = Reader::new(&prefixed);
            r.take_bytes(13, "test").unwrap();
            let result = match kind {
                0 => ModelAnimation::read(&mut r, n, Limits::default()).map(|_| ()),
                1 => {
                    MorphMesh::read(&mut r, n, GameVersion::Gothic1, Limits::default()).map(|_| ())
                }
                _ => ModelScript::read_msb(&mut r, n, Limits::default()).map(|_| ()),
            };
            if let Err(e) = result {
                assert_eq!(r.position(), 13);
                assert!(e.offset.is_none_or(|o| o >= 13));
            } else {
                assert_eq!(r.position(), 13 + n);
            }
        }
    }
}
#[test]
fn explicit_span_preserves_parent_suffix() {
    let mut all = vec![0; 5];
    all.extend(MAN);
    all.extend([8; 7]);
    let mut r = Reader::new(&all);
    r.take_bytes(5, "test").unwrap();
    ModelAnimation::read(&mut r, MAN.len(), Limits::default()).unwrap();
    assert_eq!(r.remaining(), 7);
    assert_eq!(
        ModelAnimation::from_bytes(&all[5..], Limits::default())
            .unwrap_err()
            .kind,
        ErrorKind::UnexpectedEof
    );
}
#[test]
fn count_products_and_unknown_chunks_fail_before_large_allocations() {
    let bytes = edited(MAN, 0xa020, |v| {
        v[11..19].fill(255);
    });
    assert!(ModelAnimation::from_bytes(&bytes, Limits::default()).is_err());
    let bytes = edited(MAN, 0xa030, |v| {
        v[..4].fill(255);
    });
    assert_eq!(
        ModelAnimation::from_bytes(&bytes, Limits::default())
            .unwrap_err()
            .kind,
        ErrorKind::UnexpectedEof
    );
    let mut bytes = MAN.to_vec();
    bytes.extend(chunk(0x1234, &[]));
    assert_eq!(
        ModelAnimation::from_bytes(&bytes, Limits::default())
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
    let bytes = edited(MMB, 0xe020, |v| v[..4].copy_from_slice(&4u32.to_le_bytes()));
    assert_eq!(
        MorphMesh::from_bytes(&bytes, GameVersion::Gothic1, Limits::default())
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
}
#[test]
fn zero_channels_and_frames_are_safe() {
    let bytes = edited(MAN, 0xa020, |v| v[15..19].fill(0));
    let bytes = edited(&bytes, 0xa090, |v| v.truncate(4));
    let a = ModelAnimation::from_bytes(&bytes, Limits::default()).unwrap();
    assert_eq!(a.samples().len(), 0);
    assert_eq!(a.sample(0, 0), None);
}
#[test]
fn morph_fields_nested_opaque_and_indexed_samples() {
    let m = MorphMesh::from_bytes(MMB, GameVersion::Gothic1, Limits::default()).unwrap();
    assert_eq!(m.version, 3);
    assert!(m.marker);
    assert_eq!(m.name, b"MORPH\xff");
    assert_eq!(m.animations.len(), 2);
    assert_eq!(m.sources.as_ref().unwrap().len(), 2);
    assert!(m.mesh.vertex_updates().any(|v| v.1.count > 0));
    let a = &m.animations[0];
    assert_eq!(a.layer, -3);
    assert_eq!(a.blend_out, -0.25);
    assert_eq!(a.vertices, [0, 2]);
    assert_eq!(a.sample(1, 1), a.samples.get(3).copied());
    assert_eq!(a.sample(0, 2), None);
}
#[test]
fn binary_and_source_all_collections_events() {
    for s in [
        ModelScript::from_msb(MSB, Limits::default()).unwrap(),
        ModelScript::from_mds(MDS, SyntaxMode::Strict, Limits::default()).unwrap(),
        ModelScript::from_mds(MDS, SyntaxMode::Legacy, Limits::default()).unwrap(),
    ] {
        assert_eq!(s.animations.len(), 1);
        assert_eq!(s.aliases.len(), 1);
        assert_eq!(s.blends.len(), 1);
        assert_eq!(s.combinations.len(), 1);
        assert_eq!(s.model_tags.len(), 1);
        assert_eq!(s.meshes.len(), 2);
        let a = &s.animations[0];
        assert_eq!(a.events.len(), 29);
        assert_eq!(a.first_frame, -2);
        assert_eq!(a.common.flags.known_bits(), 63);
        for (n, e) in a.events[..23].iter().enumerate() {
            let Event::Tag(t) = e else { panic!() };
            assert_eq!(t.kind() as usize, n);
        }
        assert!(s.diagnostics.is_empty());
    }
}
#[test]
fn compiled_ignored_fields_are_retained() {
    let s = ModelScript::from_msb(MSB, Limits::default()).unwrap();
    assert_eq!(s.root.unwrap().value, 7);
    assert_eq!(s.source.unwrap().date.reserved, 0);
    assert_eq!(s.model_tags[0].frame, Some(-7));
    assert_eq!(s.model_tags[0].name, b"DEF_HIT_LIMB");
    let Event::Morph(m) = &s.animations[0].events[27] else {
        panic!()
    };
    assert_eq!(m.compiled_values, Some([0.25, 0.75]));
    assert_eq!(
        s.markers.iter().find(|m| m.id == 0xf5a0).unwrap().value,
        Some(29)
    );
}
#[test]
fn frame_lists_are_checked_borrowed_and_bidirectional() {
    let f = FrameList::new(b"\t1 -2 3\x0b ").unwrap();
    assert_eq!(f.len(), 3);
    assert_eq!(f.iter().rev().collect::<Vec<_>>(), [3, -2, 1]);
    assert!(FrameList::new(b"1 2x").is_err());
    assert!(FrameList::new(b"2147483648").is_err());
    assert!(FrameList::new(b" ").unwrap().is_empty());
}
#[test]
fn raw_tag_payloads_and_unknown_modes_survive() {
    let t = Tag {
        frame: 0,
        name: b"DEF_FIGHTMODE".to_vec(),
        arguments: vec![b"future".to_vec()],
        attached: false,
    };
    assert!(matches!(
        t.payload().unwrap(),
        TagPayload::FightMode(FightMode::Unknown(b"future"))
    ));
    let t = Tag {
        name: b"FUTURE".to_vec(),
        ..t
    };
    assert!(matches!(t.payload().unwrap(), TagPayload::Unknown));
    assert_eq!(t.arguments[0], b"future");
}
#[test]
fn strict_preserves_quoted_parenthesis_and_legacy_recovers_lines() {
    for mode in [SyntaxMode::Strict, SyntaxMode::Legacy] {
        let s = ModelScript::from_mds(b"Model(\"M)\"){}", mode, Limits::default()).unwrap();
        assert_eq!(s.model_name.unwrap(), b"M)");
    }
    for line in [b"\n".as_slice(), b"\r\n"] {
        let bytes = [b"Model(\"M)".as_slice(), line, b"{}"].concat();
        assert!(ModelScript::from_mds(&bytes, SyntaxMode::Strict, Limits::default()).is_err());
        let s = ModelScript::from_mds(&bytes, SyntaxMode::Legacy, Limits::default()).unwrap();
        assert_eq!(s.model_name.unwrap(), b"M");
        assert_eq!(s.diagnostics.len(), 2);
    }
}
#[test]
fn legacy_recoveries_are_visible_and_eof_terminates() {
    let text=b"Model \"M\" { /comment\naniEnum { ani(\"A\" 1 \"\" 0 0 M \"x\" F 0 1) *eventSFX(0 \"s\" EMTPY_SLOT)";
    let s = ModelScript::from_mds(text, SyntaxMode::Legacy, Limits::default()).unwrap();
    assert!(
        s.diagnostics
            .iter()
            .any(|d| d.recovery == Recovery::MissingBrace)
    );
    assert!(
        s.diagnostics
            .iter()
            .any(|d| d.recovery == Recovery::SingleSlashComment)
    );
    assert!(
        s.diagnostics
            .iter()
            .any(|d| d.recovery == Recovery::UnbracedEvents)
    );
    assert!(
        s.diagnostics
            .iter()
            .any(|d| d.recovery == Recovery::MisspelledEmptySlot)
    );
    assert!(ModelScript::from_mds(text, SyntaxMode::Strict, Limits::default()).is_err());
    assert!(
        ModelScript::from_mds(
            b"Model(\"M\"){} // eof",
            SyntaxMode::Strict,
            Limits::default()
        )
        .is_ok()
    );
}
#[test]
fn source_blend_combination_events_and_optional_fields() {
    let s=source("aniBlend(\"A\" 4 \"B\" 0.5 0.75 MR){*eventTag(2 \"HIT_DIRECTION\" \"FRONT\")} aniComb(\"C\" 1 \"\" 0 0 M \"x\" 9){*eventSFXGrnd(3 \"s\" EMPTY_SLOT)}",SyntaxMode::Strict).unwrap();
    assert_eq!(s.blends[0].layer, Some(4));
    assert_eq!(s.blends[0].flags.as_ref().unwrap().0, b"MR");
    assert_eq!(s.blends[0].events.len(), 1);
    assert_eq!(s.combinations[0].events.len(), 1);
    let Event::GroundSound(v) = &s.combinations[0].events[0] else {
        panic!()
    };
    assert!(v.empty_slot.get());
}
#[test]
fn unknown_instructions_never_become_optional_blend_flags() {
    for mode in [SyntaxMode::Strict, SyntaxMode::Legacy] {
        let e = source("aniBlend(\"A\" \"B\") unknownInstruction()", mode).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Unsupported);
    }
}
#[test]
fn source_numeric_errors_point_at_real_token() {
    let text = b"Model(\"M\"){aniEnum{aniBlend(\"A\" \"B\" 1e999)}}";
    let e = ModelScript::from_mds(text, SyntaxMode::Strict, Limits::default()).unwrap_err();
    assert_eq!(
        e.offset,
        Some(text.windows(5).position(|v| v == b"1e999").unwrap())
    );
    let text =
        b"Model(\"M\"){aniEnum{aniBlend(\"A\" \"B\"){*eventTag(0 \"DEF_HIT_END\" \"1 2x\")}}}";
    let e = ModelScript::from_mds(text, SyntaxMode::Strict, Limits::default()).unwrap_err();
    assert_eq!(
        e.offset,
        Some(text.windows(2).position(|v| v == b"2x").unwrap())
    );
}
#[test]
fn compiled_group_counts_orphans_and_unknowns() {
    let bytes = edited(MSB, 0xf5a0, |v| v.copy_from_slice(&28u32.to_le_bytes()));
    assert_eq!(
        ModelScript::from_msb(&bytes, Limits::default())
            .unwrap_err()
            .kind,
        ErrorKind::InvalidData
    );
    let bytes = [chunk(0xf5a5, &[0; 8]), chunk(0xffff, &[])].concat();
    assert_eq!(
        ModelScript::from_msb(&bytes, Limits::default())
            .unwrap_err()
            .kind,
        ErrorKind::InvalidData
    );
    for id in [0xf510, 0xf5a6, 0x4321] {
        let bytes = [chunk(id, &[]), chunk(0xffff, &[])].concat();
        assert_eq!(
            ModelScript::from_msb(&bytes, Limits::default())
                .unwrap_err()
                .kind,
            ErrorKind::Unsupported
        );
    }
}
#[test]
fn compiled_groups_bind_latest_blend_and_reject_closed_owner() {
    let mut bytes = chunk(0xf540, b"A\nB\n\0\0\0\0\0\0\0\0");
    bytes.extend(chunk(0xf5a0, &1u32.to_le_bytes()));
    bytes.extend(chunk(0xf5a5, &[0; 8]));
    bytes.extend(chunk(0xf5af, &[]));
    bytes.extend(chunk(0xffff, &[]));
    let s = ModelScript::from_msb(&bytes, Limits::default()).unwrap();
    assert_eq!(s.blends[0].events.len(), 1);
    let mut chunks = parts(MSB);
    let p = chunks.iter().position(|c| c.0 == 0xf5ff).unwrap();
    chunks.insert(p + 1, (0xf5a5, 0, vec![0; 8]));
    let bytes: Vec<_> = chunks
        .into_iter()
        .flat_map(|(id, _, v)| chunk(id, &v))
        .collect();
    assert!(ModelScript::from_msb(&bytes, Limits::default()).is_err());
}
#[test]
fn limits_bound_every_format() {
    for n in 0..5 {
        let mut l = Limits::default();
        match n {
            0 => l.bytes = 0,
            1 => l.owned_bytes = 0,
            2 => l.elements = 0,
            3 => l.chunks = 0,
            _ => l.string_bytes = 0,
        };
        for result in [
            ModelAnimation::from_bytes(MAN, l).map(|_| ()),
            MorphMesh::from_bytes(MMB, GameVersion::Gothic1, l).map(|_| ()),
            ModelScript::from_msb(MSB, l).map(|_| ()),
        ] {
            assert_eq!(result.unwrap_err().kind, ErrorKind::LimitExceeded);
        }
    }
    for field in 0..5 {
        let mut l = Limits::default();
        match field {
            0 => l.bytes = 0,
            1 => l.owned_bytes = 0,
            2 => l.elements = 0,
            3 => l.tokens = 0,
            _ => l.depth = 0,
        };
        assert_eq!(
            ModelScript::from_mds(MDS, SyntaxMode::Strict, l)
                .unwrap_err()
                .kind,
            ErrorKind::LimitExceeded
        );
    }
}
#[test]
fn compact_debug_cannot_dump_raw_payloads() {
    let raw = vec![b'x'; 100000];
    assert!(format!("{:?}", FightMode::Unknown(&raw)).len() < 100);
    assert!(format!("{:?}", TagPayload::Slot(&raw)).len() < 100);
    assert!(
        format!(
            "{:?}",
            ModelScript::from_msb(MSB, Limits::default()).unwrap()
        )
        .len()
            < 1000
    );
}
#[test]
fn bounded_readers_work_on_small_worker_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            ModelAnimation::from_bytes(MAN, Limits::default()).unwrap();
            MorphMesh::from_bytes(MMB, GameVersion::Gothic1, Limits::default()).unwrap();
            ModelScript::from_msb(MSB, Limits::default()).unwrap();
            ModelScript::from_mds(MDS, SyntaxMode::Strict, Limits::default()).unwrap();
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn explicit_blend_boundary_does_not_consume_flag_named_instruction() {
    assert_eq!(
        source("aniBlend(\"A\" \"B\") M()", SyntaxMode::Legacy)
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
}
#[test]
fn compiled_raw_boolean_flags_direction_and_date_bits() {
    let mut data = edited(MSB, 0xf300, |v| {
        v[..4].copy_from_slice(&0x12345678u32.to_le_bytes())
    });
    data = edited(&data, 0xf100, |v| {
        v[14..16].copy_from_slice(&0xbeefu16.to_le_bytes())
    });
    for id in [0xf520, 0xf530, 0xf570] {
        data = edited(&data, id, |v| {
            let p = v
                .windows(2)
                .position(|v| v == b"MR" || v == b"I\n")
                .unwrap();
            v[p] = b'?';
        });
    }
    data = edited(&data, 0xf520, |v| {
        let p = v.windows(3).position(|v| v == b"\nR\n").unwrap();
        v[p + 1] = b'?';
    });
    let s = ModelScript::from_msb(&data, Limits::default()).unwrap();
    assert_eq!(s.skeleton.unwrap().disable_mesh.0, 0x12345678);
    assert_eq!(s.source.unwrap().date.reserved, 0xbeef);
    assert_eq!(s.animations[0].direction.playback(), None);
    assert_eq!(s.animations[0].direction.0, b"?");
    assert_eq!(s.animations[0].common.flags.0[0], b'?');
    assert_eq!(s.aliases[0].common.flags.0[0], b'?');
    assert_eq!(s.combinations[0].common.flags.0[0], b'?');
}
