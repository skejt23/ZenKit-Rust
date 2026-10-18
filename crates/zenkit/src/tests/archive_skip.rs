use crate as zenkit;
use zenkit::{
    ErrorKind,
    archive::{self, AddressSpace, AsciiSeparator, SkipLimits, TableBudget},
};
fn ascii(body: &[u8]) -> Vec<u8> {
    let mut v = b"ZenGin Archive\nver 1\nTest\nASCII\nsaveGame 0\nEND\nobjects 0\nEND\n\n".to_vec();
    v.extend(body);
    v
}
fn skip() -> SkipLimits {
    SkipLimits::default()
}

#[test]
fn unknown_ascii_fields_and_nested_classes_are_explicitly_skipped() {
    let bytes=ascii(b"[root Root 1 0]\na=opaque:malformed [ ] []\n[child Unknown 9 3]\nb=int:NOT A NUMBER\n[ref \xa7 0 2]\n[]\n[]\nafter=int:42\n[]\nend=int:77\n");
    let mut r = archive::Reader::new(&bytes, Default::default()).unwrap();
    r.begin_object().unwrap();
    r.skip_entry(skip()).unwrap();
    assert_eq!(
        r.skip_next_object(skip()).unwrap().unwrap().class_name,
        b"Unknown"
    );
    assert_eq!(r.read_int().unwrap(), 42);
    r.end_object().unwrap();
    assert_eq!(r.read_int().unwrap(), 77);
    r.finish().unwrap();
    let mut r = archive::Reader::new(&bytes, Default::default()).unwrap();
    r.begin_object().unwrap();
    r.skip_current_object(skip()).unwrap();
    assert_eq!(r.depth(), 0);
    assert_eq!(r.read_int().unwrap(), 77);
}

#[test]
fn failed_skip_restores_cursor_stack_and_record_budget() {
    let bytes = ascii(b"[root R 0 0]\n[child X 0 1]\nx=int:5\n[]\n[]\n");
    let mut r = archive::Reader::new(
        &bytes,
        archive::Limits {
            objects: 2,
            ..Default::default()
        },
    )
    .unwrap();
    r.begin_object().unwrap();
    let p = r.position();
    for limits in [
        SkipLimits { steps: 1, ..skip() },
        SkipLimits { bytes: 8, ..skip() },
    ] {
        assert_eq!(
            r.skip_current_object(limits).unwrap_err().kind,
            ErrorKind::LimitExceeded
        );
        assert_eq!(r.position(), p);
        assert_eq!(r.depth(), 1);
    }
    r.begin_object().unwrap().unwrap();
    assert_eq!(r.read_int().unwrap(), 5);
    r.end_object().unwrap();
    r.end_object().unwrap();
    r.finish().unwrap();
    let bytes = ascii(b"[root R 0 0]\nx=int:17\n");
    let mut r = archive::Reader::new(&bytes, Default::default()).unwrap();
    let p = r.position();
    assert!(r.skip_next_object(skip()).is_err());
    assert_eq!(r.position(), p);
    assert_eq!(r.depth(), 0);
}

#[test]
fn byte_cap_cannot_make_a_partial_ascii_field_look_like_eof() {
    let bytes = ascii(b"field=unknown:LONG_PAYLOAD\nnext=int:4\n");
    let mut r = archive::Reader::new(&bytes, Default::default()).unwrap();
    let p = r.position();
    assert_eq!(
        r.skip_entry(SkipLimits {
            bytes: 18,
            ..skip()
        })
        .unwrap_err()
        .kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(r.position(), p);
    r.skip_entry(skip()).unwrap();
    assert_eq!(r.read_int().unwrap(), 4);
    let bytes = ascii(b"\n\n\nfield=opaque:value\n");
    let mut r = archive::Reader::new(&bytes, Default::default()).unwrap();
    assert_eq!(
        r.skip_entry(SkipLimits { steps: 3, ..skip() })
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    r.skip_entry(SkipLimits { steps: 4, ..skip() }).unwrap();
}

fn binary_object() -> Vec<u8> {
    let mut v =
        b"ZenGin Archive\nver 1\nTest\nBINARY\nsaveGame 0\nEND\nobjects 0\nEND\n\n".to_vec();
    // Outer length19 =12-byte empty-name/class header + seven opaque bytes.
    v.extend(19u32.to_le_bytes());
    v.extend(0u16.to_le_bytes());
    v.extend(0u32.to_le_bytes());
    v.extend([0, 0]);
    v.extend([0xff, 1, 2, 3, 4, 5, 6]);
    v.extend(42i32.to_le_bytes());
    v
}
#[test]
fn binary_skips_only_framed_objects_and_cannot_infer_fields() {
    let bytes = binary_object();
    let mut r = archive::Reader::new(&bytes, Default::default()).unwrap();
    let p = r.position();
    assert_eq!(
        r.skip_entry(skip()).unwrap_err().kind,
        ErrorKind::Unsupported
    );
    assert_eq!(r.position(), p);
    assert_eq!(
        r.skip_next_object(SkipLimits {
            bytes: 18,
            ..skip()
        })
        .unwrap_err()
        .kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(r.position(), p);
    r.skip_next_object(SkipLimits {
        bytes: 19,
        ..skip()
    })
    .unwrap()
    .unwrap();
    assert_eq!(r.read_int().unwrap(), 42);
    r.finish().unwrap();
    let mut r = archive::Reader::new(&bytes, Default::default()).unwrap();
    r.begin_object().unwrap();
    r.skip_current_object(SkipLimits { bytes: 7, ..skip() })
        .unwrap();
    assert_eq!(r.read_int().unwrap(), 42);
}

fn binsafe(tag: u8, payload: &[u8]) -> Vec<u8> {
    let mut h = b"ZenGin Archive\nver 1\nTest\nBIN_SAFE\nsaveGame 0\nEND\n".to_vec();
    h.extend(2u32.to_le_bytes());
    h.extend(0u32.to_le_bytes());
    let at = h.len();
    h.extend([0; 4]);
    h.push(18);
    h.extend(0u32.to_le_bytes());
    h.push(tag);
    if matches!(tag, 1 | 9 | 16) {
        h.extend((payload.len() as u16).to_le_bytes());
    }
    h.extend(payload);
    h.push(18);
    h.extend(0u32.to_le_bytes());
    h.push(2);
    h.extend(42i32.to_le_bytes());
    let end = h.len() as u32;
    h[at..at + 4].copy_from_slice(&end.to_le_bytes());
    h.extend(1u32.to_le_bytes());
    h.extend(1u16.to_le_bytes());
    h.extend(0u16.to_le_bytes());
    h.extend(0u32.to_le_bytes());
    h.push(b'x');
    h
}
#[test]
fn binsafe_skip_covers_complete_fields_and_checks_unknown_tags_and_keys() {
    for (tag, n) in [
        (1, 7),
        (2, 4),
        (3, 4),
        (4, 1),
        (5, 2),
        (6, 4),
        (7, 12),
        (8, 4),
        (9, 11),
        (16, 3),
        (17, 4),
    ] {
        let bytes = binsafe(tag, &vec![0xff; n]);
        let mut r = archive::Reader::new(&bytes, Default::default()).unwrap();
        r.skip_entry(skip()).unwrap();
        assert_eq!(r.read_int().unwrap(), 42);
        r.finish().unwrap();
    }
    let bytes = binsafe(255, &[]);
    let mut r = archive::Reader::new(&bytes, Default::default()).unwrap();
    let p = r.position();
    assert_eq!(
        r.skip_entry(skip()).unwrap_err().kind,
        ErrorKind::Unsupported
    );
    assert_eq!(r.position(), p);
    let mut bytes = binsafe(2, &[0; 4]);
    let p = archive::Reader::new(&bytes, Default::default())
        .unwrap()
        .position();
    bytes[p + 1..p + 5].copy_from_slice(&1u32.to_le_bytes());
    let mut r = archive::Reader::new(&bytes, Default::default()).unwrap();
    assert_eq!(r.skip_entry(skip()).unwrap_err().offset, Some(p + 1));
    assert_eq!(r.position(), p);
}

#[test]
fn skip_offsets_remain_absolute_and_depth_is_iterative() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let depth = 1024;
            let mut body = vec![];
            for _ in 0..depth {
                body.extend(b"[x Unknown 0 0]\n");
            }
            for _ in 0..depth {
                body.extend(b"[]\n");
            }
            let bytes = ascii(&body);
            let mut r = archive::Reader::new(
                &bytes,
                archive::Limits {
                    depth,
                    objects: depth,
                    ..Default::default()
                },
            )
            .unwrap();
            r.skip_next_object(skip()).unwrap();
            r.finish().unwrap();
            let mut r = archive::Reader::new(
                &bytes,
                archive::Limits {
                    depth: depth - 1,
                    ..Default::default()
                },
            )
            .unwrap();
            let p = r.position();
            assert_eq!(
                r.skip_next_object(skip()).unwrap_err().kind,
                ErrorKind::LimitExceeded
            );
            assert_eq!(r.position(), p);
        })
        .unwrap()
        .join()
        .unwrap();
    let bytes = ascii(b"[x C 0 0]\nfield=int:42\n");
    let mut r = archive::Reader::embedded(
        AddressSpace::new(&bytes, 400).unwrap(),
        400..400 + bytes.len(),
        AsciiSeparator::Required,
        Default::default(),
        &mut TableBudget::new(0, 0),
    )
    .unwrap();
    let p = r.position();
    let e = r.skip_next_object(skip()).unwrap_err();
    assert!(e.offset.unwrap() >= 400);
    assert_eq!(r.position(), p);
}

#[test]
fn next_object_lookahead_and_every_partial_object_are_transactional() {
    let bytes = ascii(b"bad=unknown:opaque\n[]\n");
    let mut r = archive::Reader::new(&bytes, Default::default()).unwrap();
    let p = r.position();
    assert!(r.skip_next_object(skip()).unwrap().is_none());
    assert_eq!(r.position(), p);
    r.skip_entry(skip()).unwrap();
    let p = r.position();
    assert!(r.skip_next_object(skip()).unwrap().is_none());
    assert_eq!(r.position(), p);
    let bytes = binsafe(255, &[]);
    let mut r = archive::Reader::new(&bytes, Default::default()).unwrap();
    let p = r.position();
    assert!(r.skip_next_object(skip()).unwrap().is_none());
    assert_eq!(r.position(), p);
    assert_eq!(
        r.skip_entry(skip()).unwrap_err().kind,
        ErrorKind::Unsupported
    );
    let bytes = ascii(b"[x C 0 0]\n[y D 0 1]\na=int:4\n[]\n[]\n");
    let header = archive::Reader::new(&bytes, Default::default())
        .unwrap()
        .position();
    for length in header..bytes.len() - 1 {
        let mut r = archive::Reader::new(&bytes[..length], Default::default()).unwrap();
        let p = r.position();
        if length == header {
            assert!(r.skip_next_object(skip()).unwrap().is_none());
        } else {
            assert!(r.skip_next_object(skip()).is_err());
        }
        assert_eq!(r.position(), p);
        assert_eq!(r.depth(), 0);
    }
    let bytes = binary_object();
    let header = archive::Reader::new(&bytes, Default::default())
        .unwrap()
        .position();
    for length in header + 1..header + 19 {
        let mut r = archive::Reader::new(&bytes[..length], Default::default()).unwrap();
        let p = r.position();
        assert!(r.skip_next_object(skip()).is_err());
        assert_eq!(r.position(), p);
        assert_eq!(r.depth(), 0);
    }
}
