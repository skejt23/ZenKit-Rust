use zenkit::objects::ObjectRef;
use zenkit::{
    ErrorKind, GameVersion,
    archive::{self, Format, Header, Reader, SkipLimits, Value, Writer},
    objects::{Limits, ObjectArchive, ReadSession},
};

const FORMATS: [Format; 3] = [Format::Ascii, Format::Binary, Format::BinSafe];
const GAMES: [GameVersion; 2] = [GameVersion::Gothic1, GameVersion::Gothic2];

fn writer(format: Format, save: bool) -> Writer {
    let mut header = Header::new(format);
    header.save_game = save;
    Writer::new(header, archive::Limits::default()).unwrap()
}

fn atomic(writer: &mut Writer, name: &[u8]) -> u32 {
    let index = writer.begin_object(name, b"zCCSAtomicBlock", 0).unwrap();
    writer.begin_object(b"%", b"%", 0).unwrap();
    writer.end_object().unwrap();
    writer.end_object().unwrap();
    index
}

#[test]
fn future_and_never_defined_ids_report_the_reference_frame() {
    for format in FORMATS {
        for game in GAMES {
            for save in [false, true] {
                for missing in [0, 77] {
                    let mut w = writer(format, save);
                    w.write_reference(b"future", missing).unwrap();
                    assert_eq!(atomic(&mut w, b"later"), 0);
                    let bytes = w.finish().unwrap();
                    let mut r = Reader::new(&bytes, archive::Limits::default()).unwrap();
                    let before = r.position();
                    let h = r.begin_object().unwrap().unwrap();
                    assert!(h.is_reference());
                    assert_eq!(h.index, missing);
                    assert!(r.end_object().unwrap());
                    assert_eq!(r.begin_object().unwrap().unwrap().index, 0);
                    let error =
                        ObjectArchive::from_bytes(&bytes, game, Limits::default()).unwrap_err();
                    assert_eq!(error.kind, ErrorKind::UnresolvedReference);
                    assert_eq!(error.offset, Some(before));
                    assert_eq!(error.context, "unresolved or forward object reference");
                }
            }
        }
    }
}

#[test]
fn nested_future_message_cannot_escape_as_a_null_or_partial_graph() {
    for format in FORMATS {
        for game in GAMES {
            for save in [false, true] {
                let mut w = writer(format, save);
                w.begin_object(b"atomic", b"zCCSAtomicBlock", 0).unwrap();
                w.write_reference(b"%", 1).unwrap();
                w.end_object().unwrap();
                assert_eq!(
                    w.begin_object(
                        b"later",
                        b"oCMsgConversation:oCNpcMessage:zCEventMessage",
                        0
                    )
                    .unwrap(),
                    1
                );
                w.write(b"subType", Value::Enum(2)).unwrap();
                if save {
                    for name in [b"highPriority".as_slice(), b"inUse", b"deleted"] {
                        w.write(name, Value::Bool(false)).unwrap();
                    }
                }
                for name in [b"text".as_slice(), b"name"] {
                    w.write(name, Value::String(b"sample".as_slice().into()))
                        .unwrap();
                }
                w.end_object().unwrap();
                let bytes = w.finish().unwrap();
                let mut r = Reader::new(&bytes, archive::Limits::default()).unwrap();
                r.begin_object().unwrap().unwrap();
                let nested = r.position();
                assert_eq!(
                    ObjectArchive::from_bytes(&bytes, game, Limits::default())
                        .unwrap_err()
                        .offset,
                    Some(nested)
                );
                let mut session = ReadSession::new(&bytes, game, Limits::default()).unwrap();
                let error = session.read_object().unwrap_err();
                assert_eq!(error.kind, ErrorKind::UnresolvedReference);
                assert_eq!(error.offset, Some(nested));
                assert!(session.graph().is_err());
                assert!(session.into_parts().is_err());
            }
        }
    }
}

#[test]
fn missing_reference_poisons_a_previously_healthy_session() {
    for format in FORMATS {
        for game in GAMES {
            for save in [false, true] {
                let mut w = writer(format, save);
                atomic(&mut w, b"first");
                w.write_reference(b"missing", 77).unwrap();
                let bytes = w.finish().unwrap();
                let mut session = ReadSession::new(&bytes, game, Limits::default()).unwrap();
                let first = session.read_object().unwrap().unwrap();
                assert!(matches!(
                    session.graph().unwrap().get(first.target.unwrap()).unwrap(),
                    ObjectRef::Atomic(_)
                ));
                assert_eq!(
                    session.read_object().unwrap_err().kind,
                    ErrorKind::UnresolvedReference
                );
                assert!(session.graph().is_err());
                assert!(session.read_object().is_err());
                let mut called = false;
                assert!(
                    session
                        .with_fields(|_| {
                            called = true;
                            Ok(())
                        })
                        .is_err()
                );
                assert!(!called);
                assert!(session.finish().is_err());
            }
        }
    }
}

#[test]
fn only_one_shared_session_resolves_an_earlier_definition() {
    for format in FORMATS {
        for game in GAMES {
            for save in [false, true] {
                let mut w = writer(format, save);
                assert_eq!(atomic(&mut w, b"first"), 0);
                w.write_reference(b"alias", 0).unwrap();
                let bytes = w.finish().unwrap();
                let mut shared = ReadSession::new(&bytes, game, Limits::default()).unwrap();
                let first = shared.read_object().unwrap().unwrap();
                let alias = shared.read_object().unwrap().unwrap();
                assert_eq!(first.target, alias.target);
                assert_eq!(shared.finish().unwrap().len(), 1);
                let mut first_session = ReadSession::new(&bytes, game, Limits::default()).unwrap();
                let first = first_session.read_object().unwrap().unwrap();
                let (reader, graph) = first_session.into_parts().unwrap();
                assert!(graph.get(first.target.unwrap()).is_ok());
                let mut second_session =
                    ReadSession::from_reader(reader, game, Limits::default()).unwrap();
                assert_eq!(
                    second_session.read_object().unwrap_err().kind,
                    ErrorKind::UnresolvedReference
                );
                let mut reader = Reader::new(&bytes, archive::Limits::default()).unwrap();
                reader
                    .skip_next_object(SkipLimits::default())
                    .unwrap()
                    .unwrap();
                let mut after_skip =
                    ReadSession::from_reader(reader, game, Limits::default()).unwrap();
                assert_eq!(
                    after_skip.read_object().unwrap_err().kind,
                    ErrorKind::UnresolvedReference
                );
            }
        }
    }
}
