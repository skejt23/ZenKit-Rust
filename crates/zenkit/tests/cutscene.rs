use zenkit::{
    ErrorKind, GameVersion,
    archive::{Format, Limits as ArchiveLimits},
    cutscene::{self, Library},
    objects::*,
};
fn fixture(format: Format, variant: &str) -> Vec<u8> {
    let format = match format {
        Format::Ascii => "ascii",
        Format::Binary => "binary",
        Format::BinSafe => "bin_safe",
    };
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "tests/fixtures/synthetic-cutscene-{format}-{variant}.zen"
        )),
    )
    .unwrap()
}
fn parse(format: Format, variant: &str) -> ObjectArchive {
    ObjectArchive::from_bytes(
        &fixture(format, variant),
        GameVersion::Gothic1,
        Limits::default(),
    )
    .unwrap()
}
fn root(doc: &ObjectArchive) -> ObjectId {
    doc.graph.roots()[0].target.unwrap()
}
fn library(doc: &ObjectArchive) -> &Library {
    let ObjectRef::Library(v) = doc.graph.get(root(doc)).unwrap() else {
        panic!()
    };
    v
}
fn first(doc: &ObjectArchive) -> ObjectId {
    library(doc).blocks[0].target.unwrap()
}
fn formats() -> [Format; 3] {
    [Format::Ascii, Format::Binary, Format::BinSafe]
}
fn fresh(format: Format, save: bool) -> ObjectArchive {
    let mut g = Graph::new().unwrap();
    let msg = g
        .insert(Object::Message(ConversationMessage {
            subtype: 7,
            text: b"full\xff bytes".to_vec(),
            audio_name: b"SOUND\xfe.WAV".to_vec(),
            state: save.then_some(MessageState {
                high_priority: true,
                in_use: true,
                deleted: false,
            }),
        }))
        .unwrap();
    let atomic = g
        .insert(Object::Atomic(AtomicBlock {
            message: Link::new(b"message".to_vec(), Some(msg)),
            synchronized: None,
        }))
        .unwrap();
    let block = g
        .insert(Object::Block(CutsceneBlock {
            name: b"NAME\xff".to_vec(),
            sub_block0: 0.375,
            child: Link::new(b"child".to_vec(), Some(atomic)),
        }))
        .unwrap();
    let lib = g
        .insert(Object::Library(Library {
            blocks: vec![
                Link::new(b"first".to_vec(), Some(block)),
                Link::new(b"alias".to_vec(), Some(block)),
            ],
        }))
        .unwrap();
    g.add_root(Link::new(b"library".to_vec(), Some(lib)))
        .unwrap();
    let mut metadata = Metadata::new(format);
    metadata.save_game = save;
    ObjectArchive {
        metadata,
        game: GameVersion::Gothic2,
        graph: g,
    }
}
#[test]
fn full_library_fields_aliases_and_borrowed_text() {
    for f in formats() {
        let d = parse(f, "g1-normal");
        assert_eq!(d.graph.len(), 10);
        let l = library(&d);
        assert_eq!(l.blocks.len(), 5);
        assert_eq!(l.blocks[0].target, l.blocks[1].target);
        let a = l.blocks[0].target.unwrap();
        let b = l.blocks[2].target.unwrap();
        let c = l.blocks[3].target.unwrap();
        assert_eq!(
            cutscene::message_id(&d.graph, a).unwrap(),
            cutscene::message_id(&d.graph, b).unwrap()
        );
        assert_eq!(
            cutscene::message_id(&d.graph, a).unwrap(),
            cutscene::message_id(&d.graph, c).unwrap()
        );
        let m = cutscene::message(&d.graph, a).unwrap().unwrap();
        assert_eq!(m.text, b"Gr\xfcne Stimme: [ja] = \"komplett\"; \xff");
        assert_eq!(m.audio_name, b"VOICE\xfd.WAV");
        assert_eq!(
            cutscene::message(&d.graph, l.blocks[4].target.unwrap()).unwrap(),
            None
        );
    }
}
#[test]
fn constructed_and_loaded_writes_preserve_modes_games_and_names() {
    for f in formats() {
        for save in [false, true] {
            let doc = fresh(f, save);
            let bytes = doc.to_bytes(Limits::default()).unwrap();
            for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
                let read = ObjectArchive::from_bytes(&bytes, game, Limits::default()).unwrap();
                let l = library(&read);
                assert_eq!(l.blocks[0].name, b"first");
                assert_eq!(l.blocks[1].name, b"alias");
                assert_eq!(l.blocks[0].target, l.blocks[1].target);
                let ObjectRef::Block(b) = read.graph.get(first(&read)).unwrap() else {
                    panic!()
                };
                assert_eq!(b.sub_block0, 0.375);
                assert_eq!(b.child.name, b"child");
                assert_eq!(
                    cutscene::message(&read.graph, first(&read))
                        .unwrap()
                        .unwrap()
                        .state
                        .is_some(),
                    save
                );
                let rewrite = read.to_bytes(Limits::default()).unwrap();
                assert_eq!(rewrite, bytes);
            }
        }
    }
}
#[test]
fn exact_lookup_keeps_wire_order_and_duplicates() {
    for f in formats() {
        let doc = parse(f, "unsorted");
        let l = library(&doc);
        let ObjectRef::Block(b) = doc.graph.get(l.blocks[0].target.unwrap()).unwrap() else {
            panic!()
        };
        assert_eq!(b.name, b"NULLMSG");
        for absent in [
            b"".as_slice(),
            b"ALPH",
            b"ALPHB",
            b"BETAA",
            b"ZZZ",
            b"alpha",
        ] {
            assert_eq!(l.block_by_name(&doc.graph, absent).unwrap(), None);
        }
        assert!(l.block_by_name(&doc.graph, b"BETA").unwrap().is_some());
        let doc = parse(f, "duplicate");
        let l = library(&doc);
        assert_eq!(
            l.block_by_name(&doc.graph, b"ALPHA").unwrap(),
            l.blocks[0].target
        );
        assert_ne!(l.blocks[0].target, l.blocks[1].target);
    }
}
#[test]
fn nonzero_block_fields_survive_loaded_roundtrip() {
    for f in formats() {
        let doc = parse(f, "nonzero");
        let a = first(&doc);
        let ObjectRef::Block(b) = doc.graph.get(a).unwrap() else {
            panic!()
        };
        assert_eq!(b.sub_block0, 0.375);
        let ObjectRef::Block(b) = doc.graph.get(b.child.target.unwrap()).unwrap() else {
            panic!()
        };
        assert_eq!(b.sub_block0, -1.25);
        let bytes = doc.to_bytes(Limits::default()).unwrap();
        let read = ObjectArchive::from_bytes(&bytes, doc.game, Limits::default()).unwrap();
        let ObjectRef::Block(b) = read.graph.get(first(&read)).unwrap() else {
            panic!()
        };
        assert_eq!(b.sub_block0, 0.375);
    }
}
#[test]
fn empty_library_and_complete_raw_binsafe_text() {
    for f in formats() {
        let d = parse(f, "empty");
        assert!(library(&d).blocks.is_empty());
        assert_eq!(d.graph.len(), 1);
        let read = ObjectArchive::from_bytes(
            &d.to_bytes(Limits::default()).unwrap(),
            d.game,
            Limits::default(),
        )
        .unwrap();
        assert!(library(&read).blocks.is_empty());
    }
    let d = parse(Format::BinSafe, "raw");
    let m = cutscene::message(&d.graph, first(&d)).unwrap().unwrap();
    assert_eq!(m.text, b" \tFull\0text\nwith\rbytes\xff \t");
    assert_eq!(m.audio_name, b"\0\xff");
    let bytes = d.to_bytes(Limits::default()).unwrap();
    let r = ObjectArchive::from_bytes(&bytes, d.game, Limits::default()).unwrap();
    assert_eq!(cutscene::message(&r.graph, first(&r)).unwrap(), Some(m));
}
#[test]
fn leading_trailing_text_whitespace_is_not_trimmed() {
    for f in formats() {
        let d = parse(f, "spaces");
        let text = &cutscene::message(&d.graph, first(&d))
            .unwrap()
            .unwrap()
            .text;
        assert_eq!(text, b" \tLeading and trailing \xff \t");
    }
}
#[test]
fn cycle_storage_is_valid_but_helpers_are_fallible() {
    for f in formats() {
        let mut d = parse(f, "cycle");
        let start = first(&d);
        let before = d.to_bytes(Limits::default()).unwrap();
        assert_eq!(
            cutscene::message_id(&d.graph, start).unwrap_err().kind,
            ErrorKind::CycleDetected
        );
        assert_eq!(
            cutscene::set_message(&mut d.graph, start, None)
                .unwrap_err()
                .kind,
            ErrorKind::CycleDetected
        );
        assert_eq!(d.to_bytes(Limits::default()).unwrap(), before);
    }
}
#[test]
fn setter_changes_shared_atomic_and_retains_occurrence_name() {
    let mut d = fresh(Format::BinSafe, false);
    let block = first(&d);
    let ObjectRef::Block(b) = d.graph.get(block).unwrap() else {
        panic!()
    };
    let atomic = b.child.target.unwrap();
    let replacement = d
        .graph
        .insert(Object::Message(ConversationMessage {
            text: b"replacement\0text".to_vec(),
            ..Default::default()
        }))
        .unwrap();
    cutscene::set_message(&mut d.graph, block, Some(replacement)).unwrap();
    assert_eq!(
        cutscene::message_id(&d.graph, block).unwrap(),
        Some(replacement)
    );
    let ObjectRef::Atomic(a) = d.graph.get(atomic).unwrap() else {
        panic!()
    };
    assert_eq!(a.message.name, b"message");
    let alias = library(&d).blocks[1].target.unwrap();
    assert_eq!(
        cutscene::message_id(&d.graph, alias).unwrap(),
        Some(replacement)
    );
    let bytes = d.to_bytes(Limits::default()).unwrap();
    let r = ObjectArchive::from_bytes(&bytes, d.game, Limits::default()).unwrap();
    assert_eq!(
        cutscene::message(&r.graph, first(&r))
            .unwrap()
            .unwrap()
            .text,
        b"replacement\0text"
    );
    cutscene::set_message(&mut d.graph, block, None).unwrap();
    assert_eq!(cutscene::message_id(&d.graph, block).unwrap(), None);
}
#[test]
fn invalid_handles_kinds_and_setter_failure_do_not_mutate() {
    let mut d = fresh(Format::Binary, false);
    let id = first(&d);
    let mut other = Graph::new().unwrap();
    let foreign = other
        .insert(Object::Message(ConversationMessage::default()))
        .unwrap();
    let before = d.to_bytes(Limits::default()).unwrap();
    assert_eq!(
        cutscene::set_message(&mut d.graph, id, Some(foreign))
            .unwrap_err()
            .kind,
        ErrorKind::InvalidHandle
    );
    assert_eq!(
        cutscene::set_message(&mut d.graph, id, Some(id))
            .unwrap_err()
            .kind,
        ErrorKind::WrongObjectType
    );
    assert_eq!(d.to_bytes(Limits::default()).unwrap(), before);
    assert_eq!(
        cutscene::message_id(&d.graph, root(&d)).unwrap_err().kind,
        ErrorKind::WrongObjectType
    );
    assert_eq!(
        cutscene::message_id(&d.graph, foreign).unwrap_err().kind,
        ErrorKind::InvalidHandle
    );
    let ObjectMut::Block(b) = d.graph.get_mut(id).unwrap() else {
        panic!()
    };
    b.child.target = None;
    assert_eq!(
        cutscene::message_id(&d.graph, id).unwrap_err().kind,
        ErrorKind::InvalidData
    );
}
#[test]
fn lookup_validates_library_links_even_after_first_match() {
    let mut d = fresh(Format::Ascii, false);
    let id = root(&d);
    let ObjectMut::Library(l) = d.graph.get_mut(id).unwrap() else {
        panic!()
    };
    l.blocks.push(Link::default());
    assert_eq!(
        library(&d)
            .block_by_name(&d.graph, b"NAME\xff")
            .unwrap_err()
            .kind,
        ErrorKind::InvalidData
    );
    assert!(d.validate(Limits::default()).is_err());
}
#[test]
fn link_budget_counts_aliases_roots_and_orphans() {
    let mut d = fresh(Format::BinSafe, false);
    let exact = Limits {
        links: 5,
        ..Default::default()
    };
    let bytes = d.to_bytes(exact).unwrap();
    ObjectArchive::from_bytes(&bytes, d.game, exact).unwrap();
    assert_eq!(
        d.to_bytes(Limits { links: 4, ..exact }).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(
        ObjectArchive::from_bytes(&bytes, d.game, Limits { links: 4, ..exact })
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    d.graph
        .insert(Object::Atomic(AtomicBlock::default()))
        .unwrap();
    assert_eq!(
        d.validate(exact).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    assert!(d.validate(Limits { links: 6, ..exact }).is_ok());
}
#[test]
fn count_and_schema_errors_are_explicit() {
    let original = fixture(Format::Ascii, "g1-normal");
    let text = std::str::from_utf8(
        &original
            .iter()
            .map(|b| if *b >= 128 { b'x' } else { *b })
            .collect::<Vec<_>>(),
    )
    .unwrap()
    .to_owned();
    for (replacement, kind) in [
        ("NumOfItems=int:-1", ErrorKind::InvalidData),
        ("NumOfItems=int:2147483647", ErrorKind::LimitExceeded),
        ("NumOfItems=int:0", ErrorKind::Unsupported),
    ] {
        let bytes = text.replace("NumOfItems=int:5", replacement);
        assert_eq!(
            ObjectArchive::from_bytes(bytes.as_bytes(), GameVersion::Gothic1, Limits::default())
                .unwrap_err()
                .kind,
            kind
        );
    }
    let wrong = text.replacen("NumOfItems=int:5", "numOfItems=int:5", 1);
    assert_eq!(
        ObjectArchive::from_bytes(wrong.as_bytes(), GameVersion::Gothic1, Limits::default())
            .unwrap_err()
            .kind,
        ErrorKind::InvalidData
    );
    let wrong = text.replacen("zCCSLib 0", "zCCSLib 1", 1);
    assert_eq!(
        ObjectArchive::from_bytes(wrong.as_bytes(), GameVersion::Gothic1, Limits::default())
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
}
#[test]
fn every_truncated_record_fails_without_partial_document() {
    for f in formats() {
        let bytes = fixture(f, "g1-normal");
        for end in 0..bytes.len() {
            if let Ok(d) =
                ObjectArchive::from_bytes(&bytes[..end], GameVersion::Gothic1, Limits::default())
            {
                if d.graph.roots().is_empty() {
                    let body = zenkit::archive::Reader::new(&bytes, ArchiveLimits::default())
                        .unwrap()
                        .position();
                    assert!(end <= body);
                    assert!(d.graph.is_empty());
                    continue;
                }
                assert_eq!(f, Format::Ascii);
                assert_eq!(end, bytes.len() - 1);
                assert_eq!(library(&d).blocks.len(), 5);
            }
        }
    }
}
#[test]
fn strict_null_and_wrong_class_library_members() {
    for f in formats() {
        let mut d = fresh(f, false);
        let lib = root(&d);
        let msg = cutscene::message_id(&d.graph, first(&d)).unwrap().unwrap();
        let ObjectMut::Library(l) = d.graph.get_mut(lib).unwrap() else {
            panic!()
        };
        l.blocks[0].target = Some(msg);
        assert_eq!(
            d.to_bytes(Limits::default()).unwrap_err().kind,
            ErrorKind::WrongObjectType
        );
        let ObjectMut::Library(l) = d.graph.get_mut(lib).unwrap() else {
            panic!()
        };
        l.blocks[0].target = None;
        assert_eq!(
            d.to_bytes(Limits::default()).unwrap_err().kind,
            ErrorKind::InvalidData
        );
    }
}
#[test]
fn broad_libraries_use_bounded_depth_and_keep_aliases() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut d = fresh(Format::Binary, false);
            let block = first(&d);
            let lib = root(&d);
            let ObjectMut::Library(l) = d.graph.get_mut(lib).unwrap() else {
                panic!()
            };
            l.blocks = (0..10000)
                .map(|_| Link::new(b"%".to_vec(), Some(block)))
                .collect();
            let limits = Limits {
                archive: ArchiveLimits {
                    depth: 4,
                    ..Default::default()
                },
                ..Default::default()
            };
            let bytes = d.to_bytes(limits).unwrap();
            let r = ObjectArchive::from_bytes(&bytes, d.game, limits).unwrap();
            assert_eq!(library(&r).blocks.len(), 10000);
            assert!(
                library(&r)
                    .blocks
                    .iter()
                    .all(|l| l.target == Some(first(&r)))
            );
        })
        .unwrap()
        .join()
        .unwrap();
}
#[test]
fn compact_library_debug_does_not_dump_occurrences() {
    let l = Library {
        blocks: (0..10000)
            .map(|_| Link::new(vec![b'x'; 100], None))
            .collect(),
    };
    assert!(format!("{l:?}").len() < 100);
}

#[test]
fn malformed_wire_members_and_counts_reject_in_every_encoding() {
    for f in formats() {
        for count in [-1, 1] {
            let mut w = zenkit::archive::Writer::new(
                zenkit::archive::Header::new(f),
                ArchiveLimits::default(),
            )
            .unwrap();
            w.begin_object(b"library", b"zCCSLib", 0).unwrap();
            w.write(b"NumOfItems", zenkit::archive::Value::Int(count))
                .unwrap();
            if count == 1 {
                w.begin_object(b"%", b"%", 0).unwrap();
                w.end_object().unwrap();
            }
            w.end_object().unwrap();
            let bytes = w.finish().unwrap();
            assert_eq!(
                ObjectArchive::from_bytes(&bytes, GameVersion::Gothic1, Limits::default())
                    .unwrap_err()
                    .kind,
                ErrorKind::InvalidData
            );
        }
    }
}

#[test]
fn multiple_libraries_root_aliases_and_raw_metadata_share_identity() {
    for format in formats() {
        let mut d = fresh(format, false);
        let first_lib = root(&d);
        let block = first(&d);
        let second = d
            .graph
            .insert(Object::Library(Library {
                blocks: vec![Link::new(b"second_member".to_vec(), Some(block))],
            }))
            .unwrap();
        d.graph
            .add_root(Link::new(b"root_alias".to_vec(), Some(first_lib)))
            .unwrap();
        d.graph
            .add_root(Link::new(b"second_library".to_vec(), Some(second)))
            .unwrap();
        d.metadata.date = Some(b"raw date\xff".to_vec());
        d.metadata.user = Some(b"raw user\xfe".to_vec());
        let bytes = d.to_bytes(Limits::default()).unwrap();
        let r = ObjectArchive::from_bytes(&bytes, d.game, Limits::default()).unwrap();
        assert_eq!(r.metadata, d.metadata);
        assert_eq!(r.graph.roots()[0].target, r.graph.roots()[1].target);
        assert_eq!(r.graph.roots()[1].name, b"root_alias");
        assert_eq!(r.graph.roots()[2].name, b"second_library");
        let ObjectRef::Library(second) = r.graph.get(r.graph.roots()[2].target.unwrap()).unwrap()
        else {
            panic!()
        };
        assert_eq!(second.blocks[0].target, Some(first(&r)));
        assert_eq!(second.blocks[0].name, b"second_member");
    }
}
#[test]
fn duplicate_lookup_resolves_first_message_and_mutates_only_its_shared_atomic() {
    let mut d = parse(Format::BinSafe, "duplicate");
    let l = library(&d);
    let first = l.block_by_name(&d.graph, b"ALPHA").unwrap().unwrap();
    let second = l.blocks[1].target.unwrap();
    let before = cutscene::message_id(&d.graph, second).unwrap();
    assert_ne!(cutscene::message_id(&d.graph, first).unwrap(), before);
    cutscene::set_message(&mut d.graph, first, None).unwrap();
    assert_eq!(cutscene::message_id(&d.graph, first).unwrap(), None);
    assert_eq!(cutscene::message_id(&d.graph, second).unwrap(), before);
}
