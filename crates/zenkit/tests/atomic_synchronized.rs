use zenkit::objects::ObjectRef;
use zenkit::{
    ErrorKind, GameVersion as Game,
    archive::{self, Format, Header, Value, Writer},
    objects::{Limits, ObjectArchive, ReadSession},
};

const FORMATS: [Format; 3] = [Format::Ascii, Format::Binary, Format::BinSafe];

fn flags(doc: &ObjectArchive) -> Vec<Option<bool>> {
    doc.graph
        .objects()
        .filter_map(|(_, node)| match node {
            ObjectRef::Atomic(v) => Some(v.synchronized),
            _ => None,
        })
        .collect()
}

#[test]
fn independent_fixtures_preserve_flags_and_shared_identity_across_encodings() {
    for format in ["ascii", "binary", "bin_safe"] {
        for game in [Game::Gothic1, Game::Gothic2] {
            for save in [false, true] {
                let bytes = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
                    format!(
                        "tests/fixtures/synthetic-cutscene-{format}-synchronized-{}.zen",
                        if save { "save" } else { "normal" }
                    ),
                ))
                .unwrap();
                let mut doc = ObjectArchive::from_bytes(&bytes, game, Limits::default()).unwrap();
                assert_eq!(flags(&doc), [Some(false), Some(true), Some(true)]);
                for output in FORMATS {
                    doc.metadata.format = output;
                    doc.metadata.archiver = Header::new(output).archiver.to_vec();
                    let bytes = doc.to_bytes(Limits::default()).unwrap();
                    let got = ObjectArchive::from_bytes(&bytes, game, Limits::default()).unwrap();
                    assert_eq!(flags(&got), flags(&doc));
                    assert_eq!(got.graph.len(), 10);
                    let atoms: Vec<_> = got
                        .graph
                        .objects()
                        .filter_map(|(_, v)| {
                            if let ObjectRef::Atomic(v) = v {
                                Some(v)
                            } else {
                                None
                            }
                        })
                        .collect();
                    assert_eq!(atoms[0].message.target, atoms[1].message.target);
                    assert_eq!(atoms[2].message.target, None);
                    let blocks: Vec<_> = got
                        .graph
                        .objects()
                        .filter_map(|(_, v)| {
                            if let ObjectRef::Block(v) = v {
                                Some(v)
                            } else {
                                None
                            }
                        })
                        .collect();
                    assert_eq!(blocks[1].child.target, blocks[3].child.target);
                }
            }
        }
    }
}

fn wire(format: Format, fields: &[(&[u8], Value<'_>)], sibling: bool) -> Vec<u8> {
    let mut w = Writer::new(Header::new(format), archive::Limits::default()).unwrap();
    let index = w.begin_object(b"atomic", b"zCCSAtomicBlock", 0).unwrap();
    w.begin_object(b"message", b"%", 0).unwrap();
    w.end_object().unwrap();
    for (name, value) in fields {
        w.write(name, value.clone()).unwrap();
    }
    w.end_object().unwrap();
    if sibling {
        w.write(b"synchronized", Value::Bool(false)).unwrap();
        w.write_reference(b"alias", index).unwrap();
        w.write(b"sentinel", Value::Int(7351)).unwrap();
    }
    w.finish().unwrap()
}

#[test]
fn absence_is_distinct_and_session_leaves_parent_fields_and_aliases_intact() {
    for format in FORMATS {
        for value in [None, Some(false), Some(true)] {
            let fields = value.map(|v| (b"synchronized".as_slice(), Value::Bool(v)));
            let bytes = wire(format, fields.as_slice(), true);
            let mut reader = ReadSession::new(&bytes, Game::Gothic1, Limits::default()).unwrap();
            let first = reader.read_object().unwrap().unwrap();
            assert!(!reader.with_fields(|r| r.read_bool()).unwrap());
            let alias = reader.read_object().unwrap().unwrap();
            assert_eq!(first.target, alias.target);
            let (mut fields, graph) = reader.into_parts().unwrap();
            assert_eq!(fields.read_int().unwrap(), 7351);
            fields.finish().unwrap();
            let ObjectRef::Atomic(atomic) = graph.get(first.target.unwrap()).unwrap() else {
                panic!()
            };
            assert_eq!(atomic.synchronized, value);
            assert_eq!(graph.len(), 1);
        }
    }
}

#[test]
fn malformed_suffixes_reject_and_poison_the_session() {
    for format in FORMATS {
        let duplicate = wire(
            format,
            &[
                (b"synchronized", Value::Bool(false)),
                (b"synchronized", Value::Bool(true)),
            ],
            false,
        );
        let extra = wire(format, &[(b"synchronized", Value::Int(1))], false);
        let mut cases = vec![duplicate, extra];
        if format != Format::Binary {
            cases.push(wire(format, &[(b"wrong", Value::Bool(true))], false));
        }
        for bytes in cases {
            let mut reader = ReadSession::new(&bytes, Game::Gothic2, Limits::default()).unwrap();
            assert!(reader.read_object().is_err());
            assert_eq!(reader.graph().unwrap_err().kind, ErrorKind::InvalidData);
            assert!(reader.into_parts().is_err());
        }
    }
}

#[test]
fn missing_close_and_payload_on_reference_are_not_treated_as_absence() {
    let bytes = wire(
        Format::Ascii,
        &[(b"synchronized", Value::Bool(true))],
        false,
    );
    for end in bytes.len() - 3..bytes.len() - 1 {
        assert!(
            ObjectArchive::from_bytes(&bytes[..end], Game::Gothic1, Limits::default()).is_err()
        );
    }
    for format in FORMATS {
        let mut w = Writer::new(Header::new(format), archive::Limits::default()).unwrap();
        let id = w.begin_object(b"a", b"zCCSAtomicBlock", 0).unwrap();
        w.begin_object(b"m", b"%", 0).unwrap();
        w.end_object().unwrap();
        w.end_object().unwrap();
        // Build a same-sized placeholder and mutate it into a malformed reference.
        w.begin_object(b"alias", b"x", 0).unwrap();
        w.write(b"synchronized", Value::Bool(true)).unwrap();
        w.end_object().unwrap();
        let mut bytes = w.finish().unwrap();
        // Patch the new occurrence's index to reference the existing definition.
        if format == Format::Binary {
            let at = bytes.windows(6).position(|v| v == b"alias\0").unwrap();
            bytes[at - 4..at].copy_from_slice(&id.to_le_bytes());
            bytes[at + 6] = 0xa7;
        } else {
            let at = bytes.windows(9).position(|v| v == b"[alias x ").unwrap();
            bytes[at + 7] = 0xa7;
            bytes[at + 11] = b'0';
        }
        assert!(ObjectArchive::from_bytes(&bytes, Game::Gothic1, Limits::default()).is_err());
    }
}

#[test]
fn canonical_bool_semantics_and_output_and_field_budgets_apply() {
    for format in FORMATS {
        let bytes = wire(format, &[(b"synchronized", Value::Bool(true))], false);
        let doc = ObjectArchive::from_bytes(&bytes, Game::Gothic1, Limits::default()).unwrap();
        let encoded = doc.to_bytes(Limits::default()).unwrap();
        let mut limits = Limits::default();
        limits.archive.bytes = encoded.len() - 1;
        assert_eq!(
            doc.to_bytes(limits).unwrap_err().kind,
            ErrorKind::LimitExceeded
        );
        if format == Format::BinSafe {
            limits = Limits::default();
            limits.archive.keys = 0; // synchronized is the only keyed primitive.
            assert!(ObjectArchive::from_bytes(&bytes, Game::Gothic1, limits).is_err());
        }
    }
    let bytes = wire(
        Format::Binary,
        &[(b"synchronized", Value::Byte(255))],
        false,
    );
    let doc = ObjectArchive::from_bytes(&bytes, Game::Gothic1, Limits::default()).unwrap();
    assert_eq!(flags(&doc), [Some(true)]);
    assert_eq!(*doc.to_bytes(Limits::default()).unwrap().last().unwrap(), 1);
    let bytes = wire(
        Format::Ascii,
        &[(b"synchronized", Value::Bool(true))],
        false,
    );
    let text = String::from_utf8(bytes)
        .unwrap()
        .replace("bool:1", &format!("bool:{}1", "0".repeat(70)));
    let mut limits = Limits::default();
    limits.archive.field_bytes = 64;
    assert_eq!(
        ObjectArchive::from_bytes(text.as_bytes(), Game::Gothic1, limits)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
}
