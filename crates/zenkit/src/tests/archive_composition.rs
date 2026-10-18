use crate as zenkit;
use std::io::SeekFrom;
use zenkit::{
    ErrorKind, GameVersion,
    archive::{
        AddressSpace, AsciiSeparator, Format, Header, Limits, Reader, TableBudget, Value, Writer,
    },
    io,
    objects::{
        self, ConversationMessage, Graph, Link, Metadata, Object, ReadSession, WriteSession,
    },
};

const FORMATS: [Format; 3] = [Format::Ascii, Format::Binary, Format::BinSafe];
const RAW: &[u8] = b"\x00\xff\x01\x02\x00[]\n\rEND\n\x12\xff\xff\xff\xff";

fn header(format: Format, save: bool, metadata: u8, crlf: bool) -> Vec<u8> {
    let format = match format {
        Format::Ascii => "ASCII",
        Format::Binary => "BINARY",
        Format::BinSafe => "BIN_SAFE",
    };
    let mut bytes = format!(
        "ZenGin Archive\nver 1\nProbe\n{format}\nsaveGame {}\n",
        u8::from(save)
    )
    .into_bytes();
    if metadata & 1 != 0 {
        bytes.extend(b"date \n");
    }
    if metadata & 2 != 0 {
        bytes.extend(b"user NAME\xff\n");
    }
    bytes.extend(b"END\n");
    if crlf {
        bytes = bytes
            .into_iter()
            .flat_map(|b| if b == b'\n' { vec![b'\r', b] } else { vec![b] })
            .collect();
    }
    bytes
}

#[test]
fn header_only_borrows_metadata_and_leaves_every_suffix_byte() {
    for format in FORMATS {
        for save in [false, true] {
            for metadata in 0..4 {
                for crlf in [false, true] {
                    let head = header(format, save, metadata, crlf);
                    let mut bytes = b"PREFIX".to_vec();
                    bytes.extend(&head);
                    bytes.extend(b"\n\r\0\xffinvalid backend header");
                    let mut input = io::Reader::new(&bytes);
                    input.take_bytes(6, "prefix").unwrap();
                    let limits = Limits {
                        bytes: head.len(),
                        objects: 0,
                        depth: 0,
                        keys: 0,
                        ..Limits::default()
                    };
                    let value = Header::read(&mut input, limits).unwrap();
                    assert_eq!(
                        (value.version, value.format, value.save_game),
                        (1, format, save)
                    );
                    assert_eq!(value.archiver, b"Probe");
                    assert_eq!(value.date, (metadata & 1 != 0).then_some(b"".as_slice()));
                    assert_eq!(
                        value.user,
                        (metadata & 2 != 0).then_some(b"NAME\xff".as_slice())
                    );
                    assert_eq!(input.position(), 6 + head.len());
                    assert_eq!(
                        input.take_bytes(input.remaining(), "suffix").unwrap(),
                        b"\n\r\0\xffinvalid backend header"
                    );
                    assert!(value.archiver.as_ptr().addr() >= bytes.as_ptr().addr());
                    assert!(value.archiver.as_ptr().addr() < bytes.as_ptr().addr() + bytes.len());
                    assert!(Reader::new(&head, Limits::default()).is_err());
                    let mut only = io::Reader::new(&head);
                    assert_eq!(Header::read(&mut only, limits).unwrap(), value);
                    assert_eq!(only.remaining(), 0);
                }
            }
        }
    }
}

#[test]
fn every_header_truncation_and_work_window_rolls_back_with_absolute_offsets() {
    for crlf in [false, true] {
        let head = header(Format::BinSafe, true, 3, crlf);
        let mut full = b"prefix".to_vec();
        full.extend(&head);
        full.extend(b"suffix");
        for n in 0..head.len() {
            let mut physical = io::Reader::new(&full[..6 + n]);
            physical.take_bytes(6, "prefix").unwrap();
            let error = Header::read(&mut physical, Limits::default()).unwrap_err();
            assert_eq!(error.kind, ErrorKind::UnexpectedEof, "physical cut {n}");
            assert_eq!(physical.position(), 6);
            assert!((6..=6 + n).contains(&error.offset.unwrap()));
            let mut bounded = io::Reader::new(&full);
            bounded.take_bytes(6, "prefix").unwrap();
            let error = Header::read(
                &mut bounded,
                Limits {
                    bytes: n,
                    ..Limits::default()
                },
            )
            .unwrap_err();
            assert_eq!(error.kind, ErrorKind::LimitExceeded, "work window {n}");
            assert_eq!(bounded.position(), 6);
            assert!((6..=6 + n).contains(&error.offset.unwrap()));
        }
        let mut input = io::Reader::new(&full);
        input.take_bytes(6, "prefix").unwrap();
        Header::read(
            &mut input,
            Limits {
                bytes: head.len(),
                field_bytes: 14,
                ..Limits::default()
            },
        )
        .unwrap();
        assert_eq!(input.position(), 6 + head.len());
    }
}

#[test]
fn invalid_header_and_line_limits_are_atomic_and_match_full_reader() {
    let good = header(Format::Ascii, false, 3, false);
    for (old, new, kind) in [
        ("ZenGin Archive", "ZenGin BROKEN!", ErrorKind::InvalidData),
        ("ver 1", "ver 2", ErrorKind::Unsupported),
        ("ver 1", "ver x", ErrorKind::InvalidData),
        ("Probe", "", ErrorKind::InvalidData),
        ("ASCII", "OTHER", ErrorKind::Unsupported),
        ("saveGame 0", "saveGame x", ErrorKind::InvalidData),
        ("END", "BAD", ErrorKind::InvalidData),
    ] {
        let at = good
            .windows(old.len())
            .position(|b| b == old.as_bytes())
            .unwrap();
        let mut bad = good.clone();
        bad.splice(at..at + old.len(), new.bytes());
        let mut input = io::Reader::new(&bad);
        let error = Header::read(&mut input, Limits::default()).unwrap_err();
        assert_eq!(error.kind, kind);
        assert_eq!(input.position(), 0);
        let full = Reader::new(&bad, Limits::default()).unwrap_err();
        assert_eq!((full.kind, full.offset), (error.kind, error.offset));
    }
    for limit in 0..14 {
        let mut input = io::Reader::new(&good);
        assert_eq!(
            Header::read(
                &mut input,
                Limits {
                    field_bytes: limit,
                    ..Limits::default()
                }
            )
            .unwrap_err()
            .kind,
            ErrorKind::LimitExceeded
        );
        assert_eq!(input.position(), 0);
    }
}

fn composed(format: Format, save: bool, limit: usize) -> Vec<u8> {
    let mut h = Header::new(format);
    h.save_game = save;
    let mut w = Writer::new(
        h,
        Limits {
            bytes: limit,
            ..Limits::default()
        },
    )
    .unwrap();
    assert_eq!(w.is_save_game(), save);
    w.unframed(RAW).unwrap();
    w.begin_object(b"parent", b"Probe", 3).unwrap();
    w.write(b"before", Value::Int(-7)).unwrap();
    w.begin_object(b"child", b"Probe", 4).unwrap();
    w.unframed(RAW).unwrap();
    w.write(b"inside", Value::Int(123)).unwrap();
    w.end_object().unwrap();
    w.unframed(RAW).unwrap();
    w.write(b"after", Value::Int(29)).unwrap();
    w.end_object().unwrap();
    w.unframed(RAW).unwrap();
    assert_eq!(w.is_save_game(), save);
    w.finish().unwrap()
}

#[test]
fn raw_spans_compose_with_nested_fields_and_do_not_change_counts() {
    for format in FORMATS {
        for save in [false, true] {
            let bytes = composed(format, save, 4096);
            assert_eq!(composed(format, save, bytes.len()), bytes);
            let mut r = Reader::new(&bytes, Limits::default()).unwrap();
            assert_eq!(r.object_count(), 2);
            assert_eq!(r.header().save_game, save);
            let start = r.position();
            let mut raw = r.unframed(RAW.len()).unwrap();
            assert_eq!(raw.position(), start);
            assert_eq!(r.position(), start + RAW.len());
            assert_eq!(raw.take_bytes(RAW.len(), "raw").unwrap(), RAW);
            assert!(raw.seek_to(SeekFrom::End(1), "outside").is_err());
            assert_eq!(r.begin_object().unwrap().unwrap().version, 3);
            assert_eq!(r.read_int().unwrap(), -7);
            assert_eq!(r.begin_object().unwrap().unwrap().version, 4);
            assert_eq!(
                r.unframed(RAW.len())
                    .unwrap()
                    .take_bytes(RAW.len(), "raw")
                    .unwrap(),
                RAW
            );
            assert_eq!(r.read_int().unwrap(), 123);
            assert!(r.end_object().unwrap());
            r.unframed(RAW.len()).unwrap();
            assert_eq!(r.read_int().unwrap(), 29);
            assert!(r.end_object().unwrap());
            r.unframed(RAW.len()).unwrap();
            r.finish().unwrap();
        }
    }
}

#[test]
fn raw_reader_boundaries_and_independent_subreader_failure() {
    for format in FORMATS {
        let mut w = Writer::new(Header::new(format), Limits::default()).unwrap();
        w.begin_object(b"r", b"Probe", 0).unwrap();
        w.unframed(&[4, 5]).unwrap();
        w.end_object().unwrap();
        let bytes = w.finish().unwrap();
        let mut r = Reader::new(&bytes, Limits::default()).unwrap();
        r.begin_object().unwrap().unwrap();
        let p = r.position();
        assert!(r.unframed(usize::MAX).is_err());
        assert_eq!((r.position(), r.depth()), (p, 1));
        if format == Format::Binary {
            assert_eq!(r.unframed(3).unwrap_err().kind, ErrorKind::UnexpectedEof);
            assert_eq!(r.position(), p);
        }
        let mut sub = r.unframed(2).unwrap();
        assert_eq!(
            sub.u32("truncated raw value").unwrap_err().kind,
            ErrorKind::UnexpectedEof
        );
        assert_eq!(sub.position(), p);
        assert_eq!(r.position(), p + 2);
        assert!(r.end_object().unwrap());
        let p = r.position();
        assert_eq!(r.unframed(1).unwrap_err().kind, ErrorKind::UnexpectedEof);
        assert_eq!(r.position(), p);
        assert_eq!(r.unframed(0).unwrap().remaining(), 0);
        r.finish().unwrap();
    }
}

#[test]
fn raw_writer_reserves_table_and_failed_append_does_not_publish() {
    for format in FORMATS {
        let mut probe = Writer::new(Header::new(format), Limits::default()).unwrap();
        probe.write(b"key", Value::Int(42)).unwrap();
        let exact = probe.finish().unwrap();
        let limits = Limits {
            bytes: exact.len() + 3,
            ..Limits::default()
        };
        let mut writer = Writer::new(Header::new(format), limits).unwrap();
        writer.write(b"key", Value::Int(42)).unwrap();
        let p = writer.len();
        assert_eq!(
            writer.unframed(&[1, 2, 3, 4]).unwrap_err().kind,
            ErrorKind::LimitExceeded
        );
        assert_eq!(writer.len(), p);
        writer.unframed(&[1, 2, 3]).unwrap();
        assert_eq!(
            writer.unframed(&[4]).unwrap_err().kind,
            ErrorKind::LimitExceeded
        );
        writer.unframed(&[]).unwrap();
        let bytes = writer.finish().unwrap();
        assert_eq!(bytes.len(), limits.bytes);
        let mut reader = Reader::new(&bytes, limits).unwrap();
        assert_eq!(reader.read_int().unwrap(), 42);
        assert_eq!(
            reader.unframed(3).unwrap().take_bytes(3, "raw").unwrap(),
            [1, 2, 3]
        );
        reader.finish().unwrap();
    }
}

#[test]
fn raw_embedded_offsets_and_large_span_ignore_typed_field_limit() {
    for format in FORMATS {
        let payload = [0xa5; 100];
        let limits = Limits {
            field_bytes: 20,
            objects: 0,
            keys: 0,
            depth: 0,
            ..Limits::default()
        };
        let mut writer = Writer::new(Header::new(format), limits).unwrap();
        writer.unframed(&payload).unwrap();
        let archive = writer.finish().unwrap();
        let mut bytes = vec![0xee; 7];
        bytes.extend(&archive);
        bytes.extend(b"outside");
        if format == Format::BinSafe {
            let mut input = io::Reader::new(&bytes);
            input.take_bytes(7, "prefix").unwrap();
            Header::read(&mut input, limits).unwrap();
            let pointer = input.position() + 8;
            let old = u32::from_le_bytes(bytes[pointer..pointer + 4].try_into().unwrap());
            bytes[pointer..pointer + 4].copy_from_slice(&(old + 7).to_le_bytes());
        }
        let mut tables = TableBudget::new(0, 4);
        let mut reader = Reader::embedded(
            AddressSpace::new(&bytes, 0).unwrap(),
            7..7 + archive.len(),
            AsciiSeparator::Required,
            limits,
            &mut tables,
        )
        .unwrap();
        let p = reader.position();
        let mut raw = reader.unframed(payload.len()).unwrap();
        assert_eq!(raw.position(), p);
        assert_eq!(raw.take_bytes(100, "raw").unwrap(), payload);
        assert!(reader.unframed(1).is_err());
        reader.finish().unwrap();
    }
}

#[test]
fn object_sessions_keep_shared_identity_across_custom_raw_spans() {
    for format in FORMATS {
        let game = GameVersion::Gothic2;
        let limits = objects::Limits::default();
        let mut graph = Graph::new().unwrap();
        let id = graph
            .insert(Object::Message(ConversationMessage {
                subtype: 2,
                text: b"dialogue".to_vec(),
                audio_name: b"audio".to_vec(),
                state: None,
            }))
            .unwrap();
        let link = Link::new(b"message".to_vec(), Some(id));
        let mut writer = WriteSession::new(&Metadata::new(format), game, &graph, limits).unwrap();
        writer.write_object(&link).unwrap();
        writer
            .with_fields(|w| {
                assert!(!w.is_save_game());
                w.unframed(RAW)
            })
            .unwrap();
        writer.write_object(&link).unwrap();
        let bytes = writer.finish().unwrap();
        let mut reader = ReadSession::new(&bytes, game, limits).unwrap();
        let first = reader.read_object().unwrap().unwrap();
        let mut raw = reader.with_fields(|r| r.unframed(RAW.len())).unwrap();
        let second = reader.read_object().unwrap().unwrap();
        assert_eq!(raw.take_bytes(RAW.len(), "raw").unwrap(), RAW);
        assert_eq!(first.target, second.target);
        assert_eq!(reader.finish().unwrap().len(), 1);
    }
}
