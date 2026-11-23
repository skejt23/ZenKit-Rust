use zenkit::{
    ErrorKind, GameVersion,
    archive::{self, Format, Header},
    objects::*,
};
const FORMATS: [Format; 3] = [Format::Ascii, Format::Binary, Format::BinSafe];
fn fixture(format: Format, save: bool) -> ObjectArchive {
    let mut graph = Graph::new().unwrap();
    let a = graph
        .insert(Object::Block(CutsceneBlock {
            name: b"A".to_vec(),
            ..Default::default()
        }))
        .unwrap();
    let b = graph
        .insert(Object::Block(CutsceneBlock {
            name: b"B".to_vec(),
            child: Link::new(b"to-a".to_vec(), Some(a)),
            ..Default::default()
        }))
        .unwrap();
    if let ObjectMut::Block(v) = graph.get_mut(a).unwrap() {
        v.child = Link::new(b"to-b".to_vec(), Some(b));
    }
    let message = graph
        .insert(Object::Message(ConversationMessage {
            subtype: 2,
            text: b"raw\xff".to_vec(),
            audio_name: b"VOICE.WAV".to_vec(),
            state: save.then_some(MessageState {
                high_priority: true,
                in_use: false,
                deleted: true,
            }),
        }))
        .unwrap();
    let first = graph
        .insert(Object::Atomic(AtomicBlock {
            message: Link::new(b"first-message".to_vec(), Some(message)),
            synchronized: None,
        }))
        .unwrap();
    let second = graph
        .insert(Object::Atomic(AtomicBlock {
            message: Link::new(b"shared-message".to_vec(), Some(message)),
            synchronized: None,
        }))
        .unwrap();
    for (name, target) in [
        (b"cycle".as_slice(), Some(a)),
        (b"first", Some(first)),
        (b"second", Some(second)),
        (b"alias", Some(a)),
        (b"null", None),
    ] {
        graph.add_root(Link::new(name.to_vec(), target)).unwrap();
    }
    let mut metadata = Metadata::new(format);
    metadata.save_game = save;
    metadata.date = Some(b"raw-date".to_vec());
    metadata.user = Some(b"user\xff".to_vec());
    ObjectArchive {
        metadata,
        game: GameVersion::Gothic2,
        graph,
    }
}
fn verify(document: &ObjectArchive) {
    let g = &document.graph;
    assert_eq!(g.len(), 5);
    let roots = g.roots();
    assert_eq!(roots.len(), 5);
    assert_eq!(roots[0].target, roots[3].target);
    assert_eq!(roots[4].target, None);
    let a = roots[0].target.unwrap();
    let ObjectRef::Block(av) = g.get(a).unwrap() else {
        panic!()
    };
    let ObjectRef::Block(bv) = g.get(av.child.target.unwrap()).unwrap() else {
        panic!()
    };
    assert_eq!(bv.child.target, Some(a));
    assert_eq!(av.child.name, b"to-b");
    assert_eq!(bv.child.name, b"to-a");
    let ObjectRef::Atomic(first) = g.get(roots[1].target.unwrap()).unwrap() else {
        panic!()
    };
    let ObjectRef::Atomic(second) = g.get(roots[2].target.unwrap()).unwrap() else {
        panic!()
    };
    assert_eq!(first.message.target, second.message.target);
    assert_eq!(second.message.name, b"shared-message");
    let ObjectRef::Message(message) = g.get(first.message.target.unwrap()).unwrap() else {
        panic!()
    };
    assert_eq!(message.text, b"raw\xff");
    assert_eq!(message.state.is_some(), document.metadata.save_game);
}
#[test]
fn actual_cycles_shared_identity_modes_and_games() {
    for format in FORMATS {
        for save in [false, true] {
            for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
                let mut original = fixture(format, save);
                original.game = game;
                let bytes = original.to_bytes(Limits::default()).unwrap();
                let restored = ObjectArchive::from_bytes(&bytes, game, Limits::default()).unwrap();
                verify(&restored);
                assert_eq!(restored.metadata, original.metadata);
                assert_eq!(restored.game, game);
                let twice = ObjectArchive::from_bytes(
                    &restored.to_bytes(Limits::default()).unwrap(),
                    game,
                    Limits::default(),
                )
                .unwrap();
                verify(&twice);
            }
        }
    }
}
#[test]
fn mutations_change_shared_target_once_and_preserve_identity() {
    for format in FORMATS {
        let mut document = fixture(format, false);
        let id = document
            .graph
            .objects()
            .find(|(_, v)| matches!(v, ObjectRef::Message(_)))
            .unwrap()
            .0;
        if let ObjectMut::Message(v) = document.graph.get_mut(id).unwrap() {
            v.text = b"changed".to_vec();
        }
        let bytes = document.to_bytes(Limits::default()).unwrap();
        let restored = ObjectArchive::from_bytes(&bytes, document.game, Limits::default()).unwrap();
        let g = &restored.graph;
        let ObjectRef::Atomic(a) = g.get(g.roots()[1].target.unwrap()).unwrap() else {
            panic!()
        };
        let ObjectRef::Atomic(b) = g.get(g.roots()[2].target.unwrap()).unwrap() else {
            panic!()
        };
        assert_eq!(a.message.target, b.message.target);
        let ObjectRef::Message(v) = g.get(a.message.target.unwrap()).unwrap() else {
            panic!()
        };
        assert_eq!(v.text, b"changed");
    }
}
#[test]
fn handles_check_owner_and_survive_moves() {
    let mut a = Graph::new().unwrap();
    let id = a
        .insert(Object::Message(ConversationMessage::default()))
        .unwrap();
    let mut b = Graph::new().unwrap();
    b.insert(Object::Message(ConversationMessage::default()))
        .unwrap();
    assert_eq!(b.get(id).unwrap_err().kind, ErrorKind::InvalidHandle);
    assert_eq!(
        b.add_root(Link::new(vec![], Some(id))).unwrap_err().kind,
        ErrorKind::InvalidHandle
    );
    assert!(b.roots().is_empty());
    let moved = a;
    assert!(moved.get(id).is_ok());
    drop(moved);
    assert_eq!(
        Graph::new().unwrap().get(id).unwrap_err().kind,
        ErrorKind::InvalidHandle
    );
}
#[test]
fn whole_graph_validation_includes_orphans_and_draft_edits() {
    let mut doc = fixture(Format::Ascii, false);
    let foreign = Graph::new()
        .unwrap()
        .insert(Object::Message(ConversationMessage::default()))
        .unwrap();
    doc.graph
        .insert(Object::Atomic(AtomicBlock {
            message: Link::new(vec![], Some(foreign)),
            synchronized: None,
        }))
        .unwrap();
    assert_eq!(
        doc.validate(Limits::default()).unwrap_err().kind,
        ErrorKind::InvalidHandle
    );
    let mut doc = fixture(Format::Ascii, false);
    let block = doc.graph.roots()[0].target.unwrap();
    let atom = doc.graph.roots()[1].target.unwrap();
    if let ObjectMut::Atomic(v) = doc.graph.get_mut(atom).unwrap() {
        v.message.target = Some(block);
    }
    assert_eq!(
        doc.to_bytes(Limits::default()).unwrap_err().kind,
        ErrorKind::WrongObjectType
    );
    assert!(doc.graph.get(atom).is_ok());
}
#[test]
fn normal_save_conversion_requires_explicit_message_state() {
    let mut doc = fixture(Format::Ascii, false);
    doc.metadata.save_game = true;
    assert_eq!(
        doc.to_bytes(Limits::default()).unwrap_err().kind,
        ErrorKind::InvalidData
    );
    let id = doc
        .graph
        .objects()
        .find(|(_, o)| matches!(o, ObjectRef::Message(_)))
        .unwrap()
        .0;
    if let ObjectMut::Message(v) = doc.graph.get_mut(id).unwrap() {
        v.state = Some(MessageState::default());
    }
    assert!(doc.to_bytes(Limits::default()).is_ok());
}
fn ascii(body: &[u8]) -> Vec<u8> {
    let mut bytes =
        b"ZenGin Archive\nver 1\nzCArchiverGeneric\nASCII\nsaveGame 0\nEND\nobjects 9\nEND\n\n"
            .to_vec();
    bytes.extend_from_slice(body);
    bytes
}
#[test]
fn null_unknown_forward_duplicate_and_wrong_kind_are_distinct() {
    let input = ascii(b"[n % 0 0]\n[]\n");
    let d = ObjectArchive::from_bytes(&input, GameVersion::Gothic1, Limits::default()).unwrap();
    assert_eq!(d.graph.roots()[0].target, None);
    for (body,kind) in [
        (b"[r \xa7 0 99]\n[]\n".as_slice(),ErrorKind::UnresolvedReference),
        (b"[r FutureClass 0 0]\n[]\n",ErrorKind::Unsupported),
        (b"[r % 1 0]\n[]\n",ErrorKind::Unsupported),
        (b"[r zCCSAtomicBlock 0 0]\n[n % 0 0]\n[]\n[]\n[r zCCSAtomicBlock 0 0]\n[n % 0 0]\n[]\n[]\n",ErrorKind::InvalidData),
        (b"[r zCCSAtomicBlock 0 0]\n[r \xa7 0 0]\n[]\n[]\n",ErrorKind::WrongObjectType),
        (b"[r zCCSBlock 0 0]\nblockName=string:B\nnumOfBlocks=int:1\nsubBlock0=float:0\n[n % 0 0]\n[]\n[]\n",ErrorKind::InvalidData),
        (b"[n % 0 0]\nextra=int:1\n[]\n",ErrorKind::Unsupported),
    ]{let bytes=ascii(body);let error=ObjectArchive::from_bytes(&bytes,GameVersion::Gothic1,Limits::default()).unwrap_err();assert_eq!(error.kind,kind,"{body:?}");assert!(error.offset.is_some());}
}
#[test]
fn reference_registration_precedes_fields_and_ids_are_renumbered() {
    let bytes=ascii(b"[root zCCSBlock 0 999]\nblockName=string:self\nnumOfBlocks=int:1\nsubBlock0=float:-0\n[back \xa7 0 999]\n[]\n[]\n");
    let d = ObjectArchive::from_bytes(&bytes, GameVersion::Gothic2, Limits::default()).unwrap();
    let root = d.graph.roots()[0].target.unwrap();
    let ObjectRef::Block(v) = d.graph.get(root).unwrap() else {
        panic!()
    };
    assert_eq!(v.child.target, Some(root));
    assert_eq!(v.sub_block0.to_bits(), (-0f32).to_bits());
    let bytes = d.to_bytes(Limits::default()).unwrap();
    assert!(
        bytes
            .windows(b"[root zCCSBlock 0 0]".len())
            .any(|v| v == b"[root zCCSBlock 0 0]")
    );
}
#[test]
fn wrong_field_names_counts_versions_and_truncation_are_rejected() {
    for body in [
        b"[r zCCSBlock 1 0]\n[]\n".as_slice(),
        b"[r zCCSBlock 0 0]\nwrong=string:B\n[]\n",
        b"[r zCCSBlock 0 0]\nblockName=string:B\nnumOfBlocks=int:2\n[]\n",
    ] {
        assert!(
            ObjectArchive::from_bytes(&ascii(body), GameVersion::Gothic2, Limits::default())
                .is_err()
        );
    }
    for format in FORMATS {
        let bytes = fixture(format, false).to_bytes(Limits::default()).unwrap();
        for end in 0..bytes.len() {
            // There is no root count: a prefix ending after a complete root can
            // be a valid smaller graph. Incomplete nested records must not leak
            // placeholders or unresolved handles into an accepted graph.
            if let Ok(prefix) =
                ObjectArchive::from_bytes(&bytes[..end], GameVersion::Gothic2, Limits::default())
            {
                prefix.validate(Limits::default()).unwrap();
                let encoded = prefix.to_bytes(Limits::default()).unwrap();
                ObjectArchive::from_bytes(&encoded, GameVersion::Gothic2, Limits::default())
                    .unwrap();
            }
        }
    }
}
#[test]
fn cumulative_budgets_apply_before_reads_and_to_all_owned_metadata() {
    let d = fixture(Format::BinSafe, false);
    let bytes = d.to_bytes(Limits::default()).unwrap();
    for limits in [
        Limits {
            nodes: 4,
            ..Limits::default()
        },
        Limits {
            roots: 4,
            ..Limits::default()
        },
        Limits {
            copied_bytes: 1,
            ..Limits::default()
        },
    ] {
        assert_eq!(
            ObjectArchive::from_bytes(&bytes, d.game, limits)
                .unwrap_err()
                .kind,
            ErrorKind::LimitExceeded
        );
        assert_eq!(
            d.to_bytes(limits).unwrap_err().kind,
            ErrorKind::LimitExceeded
        );
    }
    assert_eq!(d.graph.len(), 5);
}
#[test]
fn hard_nesting_limit_covers_cycles_and_deep_drafts() {
    let mut d = ObjectArchive {
        metadata: Metadata::new(Format::Binary),
        game: GameVersion::Gothic1,
        graph: Graph::new().unwrap(),
    };
    let mut tail = d
        .graph
        .insert(Object::Atomic(AtomicBlock::default()))
        .unwrap();
    for _ in 0..128 {
        tail = d
            .graph
            .insert(Object::Block(CutsceneBlock {
                child: Link::new(b"%".to_vec(), Some(tail)),
                ..Default::default()
            }))
            .unwrap();
    }
    d.graph
        .add_root(Link::new(b"r".to_vec(), Some(tail)))
        .unwrap();
    let limits = Limits {
        archive: archive::Limits {
            depth: usize::MAX,
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(
        d.to_bytes(limits).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    drop(d);
}
#[test]
fn maximum_depth_parse_and_write_use_bounded_explicit_stacks() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            // Independent ASCII bytes: N definitions and a final ancestor
            // reference produce N+1 framing levels, without fixture recursion.
            let encoded = |definitions| {
                let mut bytes = b"ZenGin Archive\nver 1\nx\nASCII\nsaveGame 0\nEND\nobjects 0\nEND\n\n".to_vec();
                for index in 0..definitions {
                    bytes.extend_from_slice(format!("[% zCCSBlock 0 {index}]\nblockName=string:b\nnumOfBlocks=int:1\nsubBlock0=float:0\n").as_bytes());
                }
                bytes.extend_from_slice(b"[% \xa7 0 0]\n[]\n");
                for _ in 0..definitions {
                    bytes.extend_from_slice(b"[]\n");
                }
                bytes
            };
            let mut document = ObjectArchive::from_bytes(
                &encoded(127), GameVersion::Gothic2, Limits::default(),
            ).unwrap();
            assert_eq!(document.graph.len(), 127);
            for format in FORMATS {
                document.metadata.format = format;
                let bytes = document.to_bytes(Limits::default()).unwrap();
                let restored = ObjectArchive::from_bytes(&bytes, document.game, Limits::default()).unwrap();
                assert_eq!(restored.graph.len(), 127);
                let first = restored.graph.roots()[0].target.unwrap();
                let mut current = first;
                for _ in 0..127 {
                    let ObjectRef::Block(block) = restored.graph.get(current).unwrap() else { panic!() };
                    current = block.child.target.unwrap();
                }
                assert_eq!(current, first);
            }
            let limits = Limits {
                archive: archive::Limits { depth: usize::MAX, ..Default::default() },
                ..Default::default()
            };
            assert_eq!(ObjectArchive::from_bytes(&encoded(128), document.game, limits).unwrap_err().kind, ErrorKind::LimitExceeded);
            // A flat manually constructed draft can be much deeper than either
            // serialized or parsed limits; rejection and drop must stay bounded.
            let mut tail = document.graph.roots()[0].target.unwrap();
            for _ in 0..1000 {
                tail = document.graph.insert(Object::Block(CutsceneBlock {
                    child: Link::new(b"%".to_vec(), Some(tail)), ..Default::default()
                })).unwrap();
            }
            document.graph.roots_mut()[0].target = Some(tail);
            assert_eq!(document.to_bytes(limits).unwrap_err().kind, ErrorKind::LimitExceeded);
        })
        .unwrap()
        .join()
        .unwrap();
}
#[test]
fn binary_null_messages_and_minimum_subtype_representation() {
    let mut w = archive::Writer::new(Header::new(Format::Binary), Default::default()).unwrap();
    w.begin_object(b"r", b"zCCSAtomicBlock", 0).unwrap();
    w.begin_object(b"%", b"%", 0).unwrap();
    w.end_object().unwrap();
    w.end_object().unwrap();
    let d = ObjectArchive::from_bytes(
        &w.finish().unwrap(),
        GameVersion::Gothic2,
        Limits::default(),
    )
    .unwrap();
    let ObjectRef::Atomic(v) = d.graph.get(d.graph.roots()[0].target.unwrap()).unwrap() else {
        panic!()
    };
    assert_eq!(v.message.target, None);
    let mut d = fixture(Format::Binary, false);
    let id = d
        .graph
        .objects()
        .find(|(_, o)| matches!(o, ObjectRef::Message(_)))
        .unwrap()
        .0;
    if let ObjectMut::Message(v) = d.graph.get_mut(id).unwrap() {
        v.subtype = 256;
    }
    assert_eq!(
        d.to_bytes(Limits::default()).unwrap_err().kind,
        ErrorKind::Unsupported
    );
}

#[test]
fn repeated_occurrence_names_count_towards_exact_copied_budget() {
    let bytes=b"ZenGin Archive\nver 1\nx\nASCII\nsaveGame 0\nEND\nobjects 1\nEND\n\n[abc oCMsgConversation:oCNpcMessage:zCEventMessage 0 7]\nsubType=enum:2\ntext=string:hi\nname=string:wave\n[]\n[zz \xa7 0 7]\n[]\n";
    // 1 archiver + 3 first occurrence + 2 text + 4 audio + 2 alias = 12.
    let exact = Limits {
        copied_bytes: 12,
        ..Default::default()
    };
    let d = ObjectArchive::from_bytes(bytes, GameVersion::Gothic1, exact).unwrap();
    d.validate(exact).unwrap();
    d.to_bytes(exact).unwrap();
    let short = Limits {
        copied_bytes: 11,
        ..Default::default()
    };
    let error = ObjectArchive::from_bytes(bytes, GameVersion::Gothic1, short).unwrap_err();
    assert_eq!(error.kind, ErrorKind::LimitExceeded);
    assert_eq!(error.offset, bytes.windows(3).position(|v| v == b"[zz"));
    assert!(d.validate(short).is_err());
}

#[test]
fn block_subfield_preserves_binary_float_bits_and_self_cycles() {
    for format in [Format::Binary, Format::BinSafe] {
        for bits in [0, 0x8000_0000, 1, 0x7f80_0000, 0x7fc1_2345] {
            let mut graph = Graph::new().unwrap();
            let id = graph
                .insert(Object::Block(CutsceneBlock {
                    sub_block0: f32::from_bits(bits),
                    ..Default::default()
                }))
                .unwrap();
            if let ObjectMut::Block(v) = graph.get_mut(id).unwrap() {
                v.child.target = Some(id);
            }
            graph.add_root(Link::new(b"r".to_vec(), Some(id))).unwrap();
            let d = ObjectArchive {
                metadata: Metadata::new(format),
                game: GameVersion::Gothic2,
                graph,
            };
            let bytes = d.to_bytes(Limits::default()).unwrap();
            let restored = ObjectArchive::from_bytes(&bytes, d.game, Limits::default()).unwrap();
            let ObjectRef::Block(v) = restored
                .graph
                .get(restored.graph.roots()[0].target.unwrap())
                .unwrap()
            else {
                panic!()
            };
            assert_eq!(v.sub_block0.to_bits(), bits);
        }
    }
}

#[test]
fn copied_metadata_limits_point_to_actual_header_payload() {
    let bytes =
        b"ZenGin Archive\nver 1\nx\nASCII\nsaveGame 0\ndate dd\nuser uuu\nEND\nobjects 0\nEND\n\n";
    for (budget, needle) in [(0usize, b"x\n".as_slice()), (2, b"dd\n"), (5, b"uuu\n")] {
        let error = ObjectArchive::from_bytes(
            bytes,
            GameVersion::Gothic1,
            Limits {
                copied_bytes: budget,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(error.kind, ErrorKind::LimitExceeded);
        assert_eq!(
            error.offset,
            bytes.windows(needle.len()).position(|v| v == needle)
        );
    }
}
#[test]
fn dense_iterators_reverse_and_debug_stay_small() {
    let mut d = fixture(Format::Ascii, false);
    assert_eq!(d.graph.objects().len(), 5);
    let first = d.graph.objects().next().unwrap().0;
    assert_eq!(d.graph.objects().rev().next_back().unwrap().0, first);
    let id = d
        .graph
        .objects()
        .find(|(_, o)| matches!(o, ObjectRef::Message(_)))
        .unwrap()
        .0;
    if let ObjectMut::Message(v) = d.graph.get_mut(id).unwrap() {
        v.text = vec![b'x'; 100_000];
    }
    assert!(format!("{d:?}").len() < 500);
    assert!(format!("{:?}", d.graph.get(id).unwrap()).len() < 300);
}

// Lenient parsing. Fixtures are written with the graph API and then patched in
// place: same-length text patches, or insertions with BINARY record lengths and
// the BIN_SAFE table pointer adjusted.
fn lenient(bytes: &[u8], game: GameVersion) -> zenkit::Result<Parsed<ObjectArchive>> {
    let options = ParseOptions::new(Limits::default()).tolerance(Tolerance::Lenient);
    ObjectArchive::parse(bytes, game, options)
}
fn find(bytes: &[u8], pattern: &[u8]) -> usize {
    bytes
        .windows(pattern.len())
        .position(|w| w == pattern)
        .unwrap_or_else(|| panic!("{:?} not found", String::from_utf8_lossy(pattern)))
}
fn replace(bytes: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    assert_eq!(from.len(), to.len());
    let at = find(bytes, from);
    let mut out = bytes.to_vec();
    out[at..at + to.len()].copy_from_slice(to);
    out
}
/// Start of the record whose object name is `name`.
fn record_start(bytes: &[u8], format: Format, name: &[u8]) -> usize {
    match format {
        Format::Binary => find(bytes, &[name, b"\0"].concat()) - 10,
        Format::BinSafe => find(bytes, &[b"[", name, b" "].concat()) - 3,
        Format::Ascii => find(bytes, &[b"[", name, b" "].concat()),
    }
}
fn set_version(
    bytes: &[u8],
    format: Format,
    name: &[u8],
    class: &[u8],
    from: u16,
    to: u16,
) -> Vec<u8> {
    if format == Format::Binary {
        let at = find(bytes, &[name, b"\0", class, b"\0"].concat()) - 6;
        let mut out = bytes.to_vec();
        assert_eq!(u16::from_le_bytes([out[at], out[at + 1]]), from);
        out[at..at + 2].copy_from_slice(&to.to_le_bytes());
        return out;
    }
    let text = |v: u16| {
        [
            b"[",
            name,
            b" ",
            class,
            b" ",
            v.to_string().as_bytes(),
            b" ",
        ]
        .concat()
    };
    replace(bytes, &text(from), &text(to))
}
/// Insert `filler` at `at`, inside the BINARY records named `enclosing`.
fn insert(bytes: &[u8], format: Format, at: usize, filler: &[u8], enclosing: &[&[u8]]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let n = filler.len() as u32;
    match format {
        Format::Binary => {
            for name in enclosing {
                let start = record_start(bytes, format, name);
                let length = u32::from_le_bytes(out[start..start + 4].try_into().unwrap());
                out[start..start + 4].copy_from_slice(&(length + n).to_le_bytes());
            }
        }
        Format::BinSafe => {
            let pointer = find(bytes, b"END\n") + 4 + 8;
            let table = u32::from_le_bytes(out[pointer..pointer + 4].try_into().unwrap());
            out[pointer..pointer + 4].copy_from_slice(&(table + n).to_le_bytes());
        }
        Format::Ascii => {}
    }
    out.splice(at..at, filler.iter().copied());
    out
}
/// A field where a child record is expected: invalid for the reader, skippable.
fn stray_field(format: Format) -> &'static [u8] {
    match format {
        Format::Ascii => b"stray=int:1\n",
        Format::BinSafe => &[18, 0, 0, 0, 0, 2, 1, 0, 0, 0],
        Format::Binary => &[0, 0, 0, 0],
    }
}
fn vob_links(d: &ObjectArchive) -> (Option<ObjectId>, Option<ObjectId>) {
    let ObjectRef::Vob(v) = d.graph.get(d.graph.roots()[0].target.unwrap()).unwrap() else {
        panic!("root VOB")
    };
    (
        v.base().visual.as_ref().and_then(|l| l.target),
        v.base().ai.as_ref().and_then(|l| l.target),
    )
}
fn document(format: Format, game: GameVersion, save: bool, graph: Graph) -> ObjectArchive {
    let mut metadata = Metadata::new(format);
    metadata.save_game = save;
    ObjectArchive {
        metadata,
        game,
        graph,
    }
}
/// A G1 VOB with a mesh visual and a camera AI, plus a second root referencing it.
fn camera_ai_vob(format: Format) -> Vec<u8> {
    use zenkit::vob::{Visual, VisualKind, Vob};
    let mut graph = Graph::new().unwrap();
    let mesh = graph
        .insert(Object::Visual(Visual::new(VisualKind::Mesh)))
        .unwrap();
    let camera = graph
        .insert(Object::Visual(Visual::new(VisualKind::Camera)))
        .unwrap();
    let mut vob = Vob::new(GameVersion::Gothic1);
    vob.base.visual_name = Some(b"ROCK.3DS".to_vec());
    vob.base.visual = Some(Link::new(b"visual".to_vec(), Some(mesh)));
    vob.base.ai = Some(Link::new(b"ai".to_vec(), Some(camera)));
    let id = graph.insert(Object::Vob(vob)).unwrap();
    graph
        .add_root(Link::new(b"root-vob".to_vec(), Some(id)))
        .unwrap();
    graph
        .add_root(Link::new(b"again".to_vec(), Some(id)))
        .unwrap();
    document(format, GameVersion::Gothic1, false, graph)
        .to_bytes(Limits::default())
        .unwrap()
}
#[test]
fn lenient_parse_skips_unknown_records_and_reads_unlisted_versions() {
    let g1 = GameVersion::Gothic1;
    for format in FORMATS {
        let bytes = camera_ai_vob(format);
        // The strict reader accepts a camera AI in the AI slot.
        let read = ObjectArchive::from_bytes(&bytes, g1, Limits::default()).unwrap();
        assert!(matches!(vob_links(&read), (Some(_), Some(_))));

        // An unknown nested record is skipped whole; the record after it loads.
        let unknown = replace(&bytes, b"zCMesh", b"zCMosh");
        let error = ObjectArchive::from_bytes(&unknown, g1, Limits::default()).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unsupported);
        let parsed = lenient(&unknown, g1).unwrap();
        assert!(matches!(vob_links(&parsed.value), (None, Some(_))));
        assert_eq!(parsed.diagnostics.len(), 1);
        assert_eq!(parsed.diagnostics[0].class, b"zCMosh");
        assert_eq!(
            parsed.diagnostics[0].kind,
            DiagnosticKind::Skipped(SkipReason::UnknownClass)
        );
        assert!(parsed.is_lossy() && parsed.value.graph.is_lossy());
        // Written back, the skipped record is gone and the output is strict-clean.
        let written = parsed.value.to_bytes(Limits::default()).unwrap();
        let reread = ObjectArchive::from_bytes(&written, g1, Limits::default()).unwrap();
        assert!(matches!(vob_links(&reread), (None, Some(_))));

        // A broken reference after an unrelated skip still fails.
        let broken = match format {
            Format::Binary => {
                let at = find(&unknown, b"again\0") - 4;
                let mut out = unknown.clone();
                out[at..at + 4].copy_from_slice(&9u32.to_le_bytes());
                out
            }
            _ => replace(&unknown, b"[again \xa7 0 0]", b"[again \xa7 0 9]"),
        };
        assert_eq!(
            lenient(&broken, g1).unwrap_err().kind,
            ErrorKind::UnresolvedReference
        );

        // A known class with an unlisted version is read with the game's layout
        // and written back with its own version.
        let unlisted = set_version(&bytes, format, b"root-vob", b"zCVob", 12289, 12290);
        let error = ObjectArchive::from_bytes(&unlisted, g1, Limits::default()).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unsupported);
        let parsed = lenient(&unlisted, g1).unwrap();
        assert!(matches!(vob_links(&parsed.value), (Some(_), Some(_))));
        assert_eq!(
            parsed.diagnostics[0].kind,
            DiagnosticKind::ReadWithLayout {
                layout: 12289,
                complete: true
            }
        );
        assert!(!parsed.is_lossy() && !parsed.value.graph.is_lossy());
        let written = parsed.value.to_bytes(Limits::default()).unwrap();
        assert_eq!(written, unlisted);

        // An unknown root fails.
        let root = replace(&bytes, b"zCVob", b"zCVox");
        assert_eq!(lenient(&root, g1).unwrap_err().kind, ErrorKind::Unsupported);
    }
    // ASCII drops trailing fields of a record read with a fallback layout and
    // writes it back with the layout's version; a root whose fields do not
    // parse fails.
    let unlisted = set_version(
        &camera_ai_vob(Format::Ascii),
        Format::Ascii,
        b"root-vob",
        b"zCVob",
        12289,
        12290,
    );
    let end = unlisted[..find(&unlisted, b"[again")]
        .windows(2)
        .rposition(|w| w == b"[]")
        .unwrap();
    let extra = insert(&unlisted, Format::Ascii, end, b"extra=int:7\n", &[]);
    let parsed = lenient(&extra, g1).unwrap();
    assert_eq!(
        parsed.diagnostics[0].kind,
        DiagnosticKind::ReadWithLayout {
            layout: 12289,
            complete: false
        }
    );
    assert!(parsed.is_lossy());
    let written = parsed.value.to_bytes(Limits::default()).unwrap();
    assert!(ObjectArchive::from_bytes(&written, g1, Limits::default()).is_ok());
    let header_end = find(&unlisted, b"[root-vob");
    let header_end = header_end + find(&unlisted[header_end..], b"\n") + 1;
    let bad = insert(&unlisted, Format::Ascii, header_end, b"bogus=int:1\n", &[]);
    assert_eq!(lenient(&bad, g1).unwrap_err().kind, ErrorKind::InvalidData);
}
/// A VOB whose AI moves a second VOB, which a second root references.
fn ai_move_vob(format: Format, game: GameVersion, save: bool) -> Vec<u8> {
    use zenkit::vob::{AiMove, Item, ItemState, SaveState, Vob, VobKind};
    let vob = |kind| {
        let mut v = Vob::new(game);
        v.base.physics_enabled = false;
        v.base.state = save.then_some(SaveState::default());
        v.kind = kind;
        v
    };
    let mut graph = Graph::new().unwrap();
    let moved = graph
        .insert(Object::Vob(vob(if save {
            VobKind::Item(Item {
                instance: b"ITEM".to_vec(),
                state: Some(ItemState {
                    amount: 1,
                    flags: 0,
                }),
            })
        } else {
            VobKind::Base
        })))
        .unwrap();
    let ai = graph
        .insert(Object::AiMove(AiMove {
            vob: Link::new(b"moved".to_vec(), Some(moved)),
            owner: Link::new(b"owner-link".to_vec(), None),
        }))
        .unwrap();
    let mut root = vob(VobKind::Base);
    root.base.ai = Some(Link::new(b"ai-link".to_vec(), Some(ai)));
    let root = graph.insert(Object::Vob(root)).unwrap();
    graph
        .add_root(Link::new(b"root-vob".to_vec(), Some(root)))
        .unwrap();
    graph
        .add_root(Link::new(b"again".to_vec(), Some(moved)))
        .unwrap();
    document(format, game, save, graph)
        .to_bytes(Limits::default())
        .unwrap()
}
fn second_root(d: &ObjectArchive) -> Option<ObjectId> {
    d.graph.roots()[1].target
}
#[test]
fn lenient_rollback_nulls_only_references_into_rolled_back_records() {
    let g1 = GameVersion::Gothic1;
    for format in FORMATS {
        let bytes = ai_move_vob(format, g1, false);
        let unlisted = set_version(&bytes, format, b"ai-link", b"oCAIVobMove", 0, 1);
        let parsed = lenient(&unlisted, g1).unwrap();
        assert!(second_root(&parsed.value).is_some());
        assert_eq!(
            parsed.diagnostics[0].kind,
            DiagnosticKind::ReadWithLayout {
                layout: 0,
                complete: true
            }
        );

        // A limit inside a record read with a fallback layout stays a hard error.
        let limits = Limits {
            nodes: 1,
            ..Limits::default()
        };
        let options = ParseOptions::new(limits).tolerance(Tolerance::Lenient);
        assert_eq!(
            ObjectArchive::parse(&unlisted, g1, options)
                .unwrap_err()
                .kind,
            ErrorKind::LimitExceeded
        );

        // The AI record's own children do not parse after its moved VOB was
        // registered: the record is rolled back and skipped, and the later
        // reference to the moved VOB reads as null.
        let at = record_start(&unlisted, format, b"owner-link");
        let broken = insert(
            &unlisted,
            format,
            at,
            stray_field(format),
            &[b"ai-link", b"root-vob"],
        );
        assert!(ObjectArchive::from_bytes(&broken, g1, Limits::default()).is_err());
        let parsed = lenient(&broken, g1).unwrap();
        assert!(matches!(vob_links(&parsed.value), (None, None)));
        assert_eq!(second_root(&parsed.value), None);
        let kinds: Vec<_> = parsed.diagnostics.iter().map(|d| d.kind).collect();
        assert_eq!(
            kinds,
            [
                DiagnosticKind::Skipped(SkipReason::LayoutMismatch),
                DiagnosticKind::NullReference
            ]
        );
        assert!(parsed.diagnostics.iter().all(|d| d.class == b"oCAIVobMove"));
        assert_eq!(parsed.value.graph.len(), 1);
    }
    // A BINARY save Item with an unlisted version and bytes left over is
    // rolled back and skipped; its record length decides, not its flags suffix.
    let g2 = GameVersion::Gothic2;
    let bytes = ai_move_vob(Format::Binary, g2, true);
    let unlisted = set_version(&bytes, Format::Binary, b"moved", b"oCItem:zCVob", 0, 1);
    let start = record_start(&unlisted, Format::Binary, b"moved");
    let end = start + u32::from_le_bytes(unlisted[start..start + 4].try_into().unwrap()) as usize;
    let extra = insert(
        &unlisted,
        Format::Binary,
        end,
        &[0; 8],
        &[b"moved", b"ai-link", b"root-vob"],
    );
    assert_eq!(
        ObjectArchive::from_bytes(&extra, g2, Limits::default())
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
    let parsed = lenient(&extra, g2).unwrap();
    assert_eq!(second_root(&parsed.value), None);
    assert_eq!(parsed.diagnostics[0].class, b"oCItem:zCVob");
    assert_eq!(
        parsed.diagnostics[0].kind,
        DiagnosticKind::Skipped(SkipReason::LayoutMismatch)
    );
}
#[test]
fn lenient_layouts_follow_the_game_and_required_edges_still_fail() {
    use zenkit::{material::Material, vob::*};
    // A G2 material of an unlisted version reads with the complete G2 layout.
    let g2 = GameVersion::Gothic2;
    let mut graph = Graph::new().unwrap();
    let id = graph.insert(Object::Material(Material::new(g2))).unwrap();
    graph
        .add_root(Link::new(b"material".to_vec(), Some(id)))
        .unwrap();
    let bytes = document(Format::Ascii, g2, false, graph)
        .to_bytes(Limits::default())
        .unwrap();
    let unlisted = set_version(
        &bytes,
        Format::Ascii,
        b"material",
        b"zCMaterial",
        0x9c03,
        0x9c04,
    );
    let parsed = lenient(&unlisted, g2).unwrap();
    assert_eq!(
        parsed.diagnostics[0].kind,
        DiagnosticKind::ReadWithLayout {
            layout: 0x9c03,
            complete: true
        }
    );

    // G1 camera frames of version 34560 read strictly.
    let g1 = GameVersion::Gothic1;
    let mut graph = Graph::new().unwrap();
    let mut frame = Vob::new(g1);
    frame.kind = VobKind::CameraFrame(CameraFrame {
        version: 34560,
        ..CameraFrame::new(g1)
    });
    let id = graph.insert(Object::Vob(frame)).unwrap();
    graph
        .add_root(Link::new(b"frame".to_vec(), Some(id)))
        .unwrap();
    let bytes = document(Format::Ascii, g1, false, graph)
        .to_bytes(Limits::default())
        .unwrap();
    let read = ObjectArchive::from_bytes(&bytes, g1, Limits::default()).unwrap();
    assert_eq!(read.to_bytes(Limits::default()).unwrap(), bytes);

    // Only a camera visual may stand in the AI slot.
    let mut graph = Graph::new().unwrap();
    let mesh = graph
        .insert(Object::Visual(Visual::new(VisualKind::Mesh)))
        .unwrap();
    let mut vob = Vob::new(g1);
    vob.base.ai = Some(Link::new(b"ai".to_vec(), Some(mesh)));
    let id = graph.insert(Object::Vob(vob)).unwrap();
    graph
        .add_root(Link::new(b"root".to_vec(), Some(id)))
        .unwrap();
    let error = document(Format::Ascii, g1, false, graph)
        .to_bytes(Limits::default())
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::WrongObjectType);

    // A skipped child of a block, which requires its child, fails the block.
    for format in FORMATS {
        let mut graph = Graph::new().unwrap();
        let atomic = graph
            .insert(Object::Atomic(AtomicBlock::default()))
            .unwrap();
        let block = graph
            .insert(Object::Block(CutsceneBlock {
                child: Link::new(b"atomic".to_vec(), Some(atomic)),
                ..Default::default()
            }))
            .unwrap();
        graph
            .add_root(Link::new(b"block".to_vec(), Some(block)))
            .unwrap();
        let bytes = document(format, g2, false, graph)
            .to_bytes(Limits::default())
            .unwrap();
        let unknown = replace(&bytes, b"zCCSAtomicBlock", b"zCCSAtomicBlocX");
        assert!(ObjectArchive::from_bytes(&unknown, g2, Limits::default()).is_err());
        assert_eq!(
            lenient(&unknown, g2).unwrap_err().kind,
            ErrorKind::InvalidData
        );
    }
}
#[test]
fn lenient_world_keeps_vob_tree_aligned_around_a_skipped_vob() {
    use zenkit::vob::{Vob, VobKind};
    use zenkit::world::{Hierarchy, HierarchyEntry, Section, SectionData, SectionFrame, World};
    let game = GameVersion::Gothic2;
    for format in FORMATS {
        let mut graph = Graph::new().unwrap();
        let world = graph
            .insert(Object::World(World::new(game, false)))
            .unwrap();
        let mut spot = Vob::new(game);
        spot.kind = VobKind::Spot;
        let spot = graph.insert(Object::Vob(spot)).unwrap();
        let child = graph.insert(Object::Vob(Vob::new(game))).unwrap();
        let ObjectMut::World(w) = graph.get_mut(world).unwrap() else {
            unreachable!()
        };
        w.sections = vec![
            Section {
                frame: SectionFrame::default(),
                data: SectionData::VobTree(Hierarchy {
                    root_count_name: Some(b"childs0".to_vec()),
                    entries: vec![
                        HierarchyEntry {
                            object: Link::new(b"spot".to_vec(), Some(spot)),
                            parent: None,
                            children_name: Some(b"childs0".to_vec()),
                        },
                        HierarchyEntry {
                            object: Link::new(b"child".to_vec(), Some(child)),
                            parent: Some(0),
                            children_name: Some(b"childs1".to_vec()),
                        },
                    ],
                }),
            },
            Section {
                frame: SectionFrame::default(),
                data: SectionData::End,
            },
        ];
        graph
            .add_root(Link::new(b"world".to_vec(), Some(world)))
            .unwrap();
        let bytes = document(format, game, false, graph)
            .to_bytes(Limits::default())
            .unwrap();
        let unknown = replace(&bytes, b"zCVobSpot:", b"zCVobSpox:");
        let parsed = lenient(&unknown, game).unwrap();
        let ObjectRef::World(w) = parsed
            .value
            .graph
            .get(parsed.value.graph.roots()[0].target.unwrap())
            .unwrap()
        else {
            panic!("world root")
        };
        let SectionData::VobTree(tree) = &w.sections[0].data else {
            panic!("VOB tree")
        };
        assert_eq!(tree.entries.len(), 2);
        assert_eq!(tree.entries[0].object.target, None);
        assert_eq!(tree.entries[1].parent, Some(0));
        assert!(tree.entries[1].object.target.is_some());
    }
}
