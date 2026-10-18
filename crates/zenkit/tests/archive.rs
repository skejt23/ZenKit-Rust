use std::borrow::Cow;
use zenkit::{
    ErrorKind,
    archive::{Entry, Format, Header, Limits, Reader, Value, ValueType, Writer},
    values::{Aabb, Color, Mat3, Vec2, Vec3},
};

const FORMATS: [Format; 3] = [Format::Ascii, Format::Binary, Format::BinSafe];
fn values() -> Vec<Value<'static>> {
    vec![
        Value::String(Cow::Borrowed(b"Gr\xfcne : = []")),
        Value::Int(i32::MIN),
        Value::Float(1.234_567_9),
        Value::Byte(201),
        Value::Word(50_001),
        Value::Enum(201),
        Value::Bool(true),
        Value::Color(Color::from_rgba([1, 127, 254, 33])),
        Value::Vec2(Vec2::from([1.25, -2.5])),
        Value::Vec3(Vec3::from([-0.0, 3.5, -7.25])),
        Value::Aabb(Aabb {
            min: Vec3::from([-1., -2., -3.]),
            max: Vec3::from([4., 5., 6.]),
        }),
        Value::Mat3(Mat3::from_rows([
            [1., 2., 3.].into(),
            [4., 5., 6.].into(),
            [7., 8., 9.].into(),
        ])),
        Value::Raw(Cow::Borrowed(&[255, 1, 128, 0, 171])),
        Value::RawFloat(Cow::Borrowed(&[1.25, -2.5, 3.75])),
    ]
}
fn fixture(format: Format) -> Vec<u8> {
    let mut header = Header::new(format);
    header.save_game = true;
    header.date = Some(b"14.10.2026 12:34:56");
    header.user = Some(b"raw\xff");
    let mut w = Writer::new(header, Limits::default()).unwrap();
    assert_eq!(w.begin_object(b"root", b"Example", 7).unwrap(), 0);
    for value in values() {
        w.write(b"field", value).unwrap();
    }
    assert_eq!(w.begin_object(b"child", b"Other", 8).unwrap(), 1);
    w.write(b"tail", Value::String(Cow::Borrowed(b"done")))
        .unwrap();
    w.end_object().unwrap();
    w.write_reference(b"back", 0).unwrap();
    assert_eq!(w.begin_object(b"empty", b"%", 0).unwrap(), 0);
    w.end_object().unwrap();
    w.end_object().unwrap();
    w.finish().unwrap()
}
fn consume(r: &mut Reader<'_>) -> zenkit::Result<()> {
    let root = r
        .begin_object()?
        .ok_or_else(|| zenkit::Error::operation(ErrorKind::UnexpectedEof, "root"))?;
    assert_eq!(root.object_name, b"root");
    assert_eq!(root.class_name, b"Example");
    assert_eq!((root.version, root.index), (7, 0));
    for expected in values() {
        let Entry { name, value } = r.read(expected.value_type())?;
        if value != expected {
            return Err(zenkit::Error::operation(
                ErrorKind::InvalidData,
                "value mismatch",
            ));
        }
        assert_eq!(
            name,
            if r.header().format == Format::Binary {
                None
            } else {
                Some(b"field".as_slice())
            }
        );
    }
    let child = r
        .begin_object()?
        .ok_or_else(|| zenkit::Error::operation(ErrorKind::UnexpectedEof, "child"))?;
    assert_eq!((child.version, child.index), (8, 1));
    if r.read_string()?.as_ref() != b"done" {
        return Err(zenkit::Error::operation(
            ErrorKind::InvalidData,
            "tail mismatch",
        ));
    }
    if !r.end_object()? {
        return Err(zenkit::Error::operation(
            ErrorKind::UnexpectedEof,
            "missing end",
        ));
    }
    let reference = r
        .begin_object()?
        .ok_or_else(|| zenkit::Error::operation(ErrorKind::UnexpectedEof, "reference"))?;
    assert!(reference.is_reference());
    assert_eq!(reference.index, 0);
    if !r.end_object()? {
        return Err(zenkit::Error::operation(
            ErrorKind::UnexpectedEof,
            "missing end",
        ));
    }
    assert!(
        r.begin_object()?
            .ok_or_else(|| zenkit::Error::operation(ErrorKind::UnexpectedEof, "empty"))?
            .is_empty()
    );
    if !r.end_object()? {
        return Err(zenkit::Error::operation(
            ErrorKind::UnexpectedEof,
            "missing end",
        ));
    }
    if !r.end_object()? {
        return Err(zenkit::Error::operation(
            ErrorKind::UnexpectedEof,
            "missing end",
        ));
    }
    Ok(())
}
#[test]
fn all_encodings_metadata_primitives_nesting_and_raw_references() {
    for format in FORMATS {
        let bytes = fixture(format);
        let mut r = Reader::new(&bytes, Limits::default()).unwrap();
        assert!(r.header().save_game);
        assert_eq!(r.header().date, Some(b"14.10.2026 12:34:56".as_slice()));
        assert_eq!(r.header().user, Some(b"raw\xff".as_slice()));
        assert_eq!(r.object_count(), 2);
        consume(&mut r).unwrap();
        r.finish().unwrap();
    }
}
#[test]
fn every_truncation_fails_without_a_panic() {
    for format in FORMATS {
        let bytes = fixture(format);
        for end in 0..bytes.len() {
            if format == Format::Ascii && end == bytes.len() - 1 {
                continue;
            }
            if let Ok(mut r) = Reader::new(&bytes[..end], Limits::default()) {
                assert!(
                    consume(&mut r).and_then(|_| r.finish()).is_err(),
                    "{format:?} length {end}"
                );
            }
        }
    }
}
fn ascii(body: &[u8]) -> Vec<u8> {
    let mut v =
        b"ZenGin Archive\nver 1\nzCArchiverGeneric\nASCII\nsaveGame 0\nEND\nobjects 0\nEND\n\n"
            .to_vec();
    v.extend_from_slice(body);
    v
}
#[test]
fn independent_ascii_fields_and_aliases() {
    let bytes = ascii(
        b"a=byte:255\nb=word:65535\nc=raw:ff018000ab\nd=rawFloat:1 2 3 4 5 6 7 8 9\ne=bool:42\n",
    );
    let mut r = Reader::new(&bytes, Limits::default()).unwrap();
    assert_eq!(r.read_byte().unwrap(), 255);
    assert_eq!(r.read_word().unwrap(), 65535);
    assert_eq!(
        r.read(ValueType::Raw(1)).unwrap().value,
        Value::Raw(Cow::Borrowed(&[255, 1, 128, 0, 171]))
    );
    assert_eq!(
        r.read_mat3().unwrap().columns,
        [
            Vec3::from([1., 4., 7.]),
            Vec3::from([2., 5., 8.]),
            Vec3::from([3., 6., 9.])
        ]
    );
    assert!(r.read_bool().unwrap());
    r.finish().unwrap();
}
#[test]
fn strict_numbers_hex_and_types_are_transactional() {
    for (body, kind) in [
        (b"x=int:42junk\n".as_slice(), ValueType::Int),
        (b"x=int:256\n", ValueType::Byte),
        (b"x=int:-1\n", ValueType::Word),
        (b"x=raw:ff0z\n", ValueType::Raw(1)),
        (b"x=raw:abc\n", ValueType::Raw(1)),
        (b"x=vec3:1 2\n", ValueType::Vec3),
        (b"x=unknown:42\n", ValueType::Int),
    ] {
        let bytes = ascii(body);
        let mut r = Reader::new(&bytes, Limits::default()).unwrap();
        let p = r.position();
        assert!(r.read(kind).is_err());
        assert_eq!(r.position(), p);
        assert_eq!(r.depth(), 0);
    }
}
#[test]
fn typed_padding_can_be_preserved_through_raw_fallback() {
    for format in [Format::Ascii, Format::BinSafe] {
        let mut w = Writer::new(Header::new(format), Limits::default()).unwrap();
        w.write(b"v", Value::RawFloat(Cow::Borrowed(&[1., 2., 3.])))
            .unwrap();
        let bytes = w.finish().unwrap();
        let mut r = Reader::new(&bytes, Limits::default()).unwrap();
        let p = r.position();
        assert_eq!(r.read_vec2().unwrap_err().kind, ErrorKind::Unsupported);
        assert_eq!(r.position(), p);
        assert_eq!(
            r.read(ValueType::RawFloat(2)).unwrap().value,
            Value::RawFloat(Cow::Borrowed(&[1., 2., 3.]))
        );
        r.finish().unwrap();
    }
}
#[test]
fn binary_objects_bound_all_primitive_and_nested_reads() {
    let mut w = Writer::new(Header::new(Format::Binary), Limits::default()).unwrap();
    w.begin_object(b"root", b"Class", 1).unwrap();
    w.write(b"", Value::Byte(1)).unwrap();
    w.end_object().unwrap();
    w.write(b"", Value::Int(7)).unwrap();
    let bytes = w.finish().unwrap();
    let mut r = Reader::new(&bytes, Limits::default()).unwrap();
    r.begin_object().unwrap();
    let p = r.position();
    assert_eq!(r.read_int().unwrap_err().kind, ErrorKind::UnexpectedEof);
    assert_eq!(r.position(), p);
    assert!(r.begin_object().is_err());
    assert_eq!(r.position(), p);
    assert_eq!(r.read_byte().unwrap(), 1);
    assert!(r.end_object().unwrap());
    assert_eq!(r.read_int().unwrap(), 7);
    r.finish().unwrap();
}
#[test]
fn binary_object_length_validation_and_parent_overlap() {
    let mut w = Writer::new(Header::new(Format::Binary), Limits::default()).unwrap();
    w.begin_object(b"a", b"A", 0).unwrap();
    w.begin_object(b"b", b"B", 0).unwrap();
    w.end_object().unwrap();
    w.end_object().unwrap();
    let original = w.finish().unwrap();
    let start = Reader::new(&original, Limits::default())
        .unwrap()
        .position();
    for length in [0u32, 4, 11, u32::MAX] {
        let mut bytes = original.clone();
        bytes[start..start + 4].copy_from_slice(&length.to_le_bytes());
        let mut r = Reader::new(&bytes, Limits::default()).unwrap();
        assert!(r.begin_object().is_err());
        assert_eq!(r.position(), start);
    }
    let mut bytes = original;
    let child = start + 14;
    bytes[child..child + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut r = Reader::new(&bytes, Limits::default()).unwrap();
    r.begin_object().unwrap();
    assert!(r.begin_object().is_err());
    assert_eq!(r.position(), child);
}
#[test]
fn binary_leading_lf_and_byte_strings_are_preserved() {
    let mut w = Writer::new(Header::new(Format::Binary), Limits::default()).unwrap();
    w.write(b"", Value::Byte(10)).unwrap();
    w.write(b"", Value::String(Cow::Borrowed(b"line\r\n\xff")))
        .unwrap();
    let bytes = w.finish().unwrap();
    let mut r = Reader::new(&bytes, Limits::default()).unwrap();
    assert_eq!(r.read_byte().unwrap(), 10);
    assert_eq!(r.read_string().unwrap().as_ref(), b"line\r\n\xff");
    r.finish().unwrap();
}
#[test]
fn bounded_records_depth_and_output_transactions() {
    for format in FORMATS {
        let limits = Limits {
            objects: 2,
            depth: 1,
            ..Limits::default()
        };
        let mut w = Writer::new(Header::new(format), limits).unwrap();
        w.begin_object(b"a", b"A", 0).unwrap();
        let len = w.len();
        assert_eq!(
            w.begin_object(b"b", b"B", 0).unwrap_err().kind,
            ErrorKind::LimitExceeded
        );
        assert_eq!(w.len(), len);
        assert_eq!(w.depth(), 1);
        w.end_object().unwrap();
        w.write_reference(b"ref", 0).unwrap();
        assert!(w.write_reference(b"ref", 0).is_err());
        let bytes = w.finish().unwrap();
        let mut r = Reader::new(&bytes, limits).unwrap();
        r.begin_object().unwrap();
        r.end_object().unwrap();
        r.begin_object().unwrap();
        r.end_object().unwrap();
        r.finish().unwrap();
        let mut w = Writer::new(
            Header::new(format),
            Limits {
                bytes: 100,
                ..Limits::default()
            },
        )
        .unwrap();
        let len = w.len();
        assert_eq!(
            w.write(b"name", Value::String(Cow::Owned(vec![b'x'; 101])))
                .unwrap_err()
                .kind,
            ErrorKind::LimitExceeded
        );
        assert_eq!(w.len(), len);
        assert_eq!(w.depth(), 0);
    }
}
#[test]
fn strings_obey_encoding_representability_and_rejected_writes_are_atomic() {
    for format in FORMATS {
        let mut w = Writer::new(Header::new(format), Limits::default()).unwrap();
        let len = w.len();
        let invalid = if format == Format::BinSafe {
            None
        } else {
            Some(b"a\0b".as_slice())
        };
        if let Some(v) = invalid {
            assert_eq!(
                w.write(b"x", Value::String(Cow::Borrowed(v)))
                    .unwrap_err()
                    .kind,
                ErrorKind::InvalidData
            );
            assert_eq!(w.len(), len);
        }
        if format == Format::Binary {
            assert_eq!(
                w.write(b"x", Value::Enum(256)).unwrap_err().kind,
                ErrorKind::Unsupported
            );
            assert_eq!(w.len(), len);
        }
        w.write(b"x", Value::String(Cow::Borrowed(b"valid")))
            .unwrap();
        let bytes = w.finish().unwrap();
        let mut r = Reader::new(&bytes, Limits::default()).unwrap();
        assert_eq!(r.read_string().unwrap().as_ref(), b"valid");
        r.finish().unwrap();
    }
    let mut w = Writer::new(Header::new(Format::BinSafe), Limits::default()).unwrap();
    w.write(b"a\0\xff", Value::String(Cow::Borrowed(b"\0\r\n\xff")))
        .unwrap();
    let bytes = w.finish().unwrap();
    let mut r = Reader::new(&bytes, Limits::default()).unwrap();
    assert_eq!(
        r.read(ValueType::String).unwrap(),
        Entry {
            name: Some(b"a\0\xff"),
            value: Value::String(Cow::Borrowed(b"\0\r\n\xff"))
        }
    );
}
#[test]
fn header_validation_crlf_and_limits() {
    let bytes = ascii(b"");
    for (old, new) in [
        (b"ver 1".as_slice(), b"ver 2".as_slice()),
        (b"ASCII", b"OTHER"),
        (b"saveGame 0", b"saveGame x"),
    ] {
        let bytes = String::from_utf8(bytes.clone())
            .unwrap()
            .replace(
                std::str::from_utf8(old).unwrap(),
                std::str::from_utf8(new).unwrap(),
            )
            .into_bytes();
        assert!(Reader::new(&bytes, Limits::default()).is_err());
    }
    let crlf = String::from_utf8(bytes.clone())
        .unwrap()
        .replace('\n', "\r\n");
    Reader::new(crlf.as_bytes(), Limits::default())
        .unwrap()
        .finish()
        .unwrap();
    assert!(
        Reader::new(
            &bytes,
            Limits {
                bytes: bytes.len() - 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(
        Reader::new(
            &bytes,
            Limits {
                field_bytes: 3,
                ..Limits::default()
            }
        )
        .is_err()
    );
}
#[test]
fn binsafe_table_indices_offsets_lengths_and_key_references() {
    let mut w = Writer::new(Header::new(Format::BinSafe), Limits::default()).unwrap();
    w.write(b"a", Value::Int(1)).unwrap();
    w.write(b"b", Value::Int(2)).unwrap();
    let original = w.finish().unwrap();
    let body = Reader::new(&original, Limits::default())
        .unwrap()
        .position();
    let table = u32::from_le_bytes(original[body - 4..body].try_into().unwrap()) as usize;
    for (offset, value) in [(body - 4, 0u32), (body - 4, u32::MAX), (table, u32::MAX)] {
        let mut bytes = original.clone();
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(Reader::new(&bytes, Limits::default()).is_err());
    }
    let mut bytes = original.clone();
    bytes[table + 6..table + 8].copy_from_slice(&2u16.to_le_bytes());
    assert!(Reader::new(&bytes, Limits::default()).is_err());
    let mut bytes = original.clone();
    bytes[table + 15..table + 17].copy_from_slice(&0u16.to_le_bytes());
    assert!(Reader::new(&bytes, Limits::default()).is_err());
    let mut bytes = original.clone();
    bytes[body + 1..body + 5].copy_from_slice(&2u32.to_le_bytes());
    let mut r = Reader::new(&bytes, Limits::default()).unwrap();
    assert_eq!(r.read_int().unwrap_err().offset, Some(body + 1));
    assert_eq!(r.position(), body);
    let mut bytes = original.clone();
    bytes[body + 5] = 255;
    let mut r = Reader::new(&bytes, Limits::default()).unwrap();
    assert!(r.read_int().is_err());
    assert_eq!(r.position(), body);
    assert!(
        Reader::new(
            &original,
            Limits {
                keys: 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
    let mut bytes = original;
    bytes[table + 8..table + 12].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut r = Reader::new(&bytes, Limits::default()).unwrap();
    assert_eq!(r.read_int().unwrap(), 1);
    assert_eq!(r.read_int().unwrap(), 2);
    r.finish().unwrap();
}
#[test]
fn finalization_requires_explicit_complete_framing() {
    for format in FORMATS {
        let mut w = Writer::new(Header::new(format), Limits::default()).unwrap();
        assert!(w.end_object().is_err());
        w.begin_object(b"a", b"A", 0).unwrap();
        assert!(w.finish().is_err());
        let bytes = fixture(format);
        let mut r = Reader::new(&bytes, Limits::default()).unwrap();
        assert!(r.end_object().is_err());
        r.begin_object().unwrap();
        assert!(r.finish().is_err());
    }
}
#[test]
fn floating_bits_survive_binary_and_decimal_finite_roundtrips() {
    for format in FORMATS {
        let bits = [
            0u32,
            0x8000_0000,
            1,
            0x007f_ffff,
            0x0080_0000,
            0x3f9e_0652,
            0x7f7f_ffff,
            0xff7f_ffff,
            0x7f80_0000,
            0xff80_0000,
        ];
        let mut w = Writer::new(Header::new(format), Limits::default()).unwrap();
        for bit in bits {
            w.write(b"f", Value::Float(f32::from_bits(bit))).unwrap();
        }
        let bytes = w.finish().unwrap();
        let mut r = Reader::new(&bytes, Limits::default()).unwrap();
        for bit in bits {
            assert_eq!(r.read_float().unwrap().to_bits(), bit);
        }
        r.finish().unwrap();
        if format != Format::Ascii {
            let bit = 0x7fc1_2345;
            let mut w = Writer::new(Header::new(format), Limits::default()).unwrap();
            w.write(b"f", Value::Float(f32::from_bits(bit))).unwrap();
            let bytes = w.finish().unwrap();
            assert_eq!(
                Reader::new(&bytes, Limits::default())
                    .unwrap()
                    .read_float()
                    .unwrap()
                    .to_bits(),
                bit
            );
        }
    }
}
#[test]
fn binsafe_field_width_and_key_limits_are_checked_before_commit() {
    let limits = Limits {
        keys: 1,
        ..Limits::default()
    };
    let mut w = Writer::new(Header::new(Format::BinSafe), limits).unwrap();
    w.write(b"a", Value::Int(1)).unwrap();
    let len = w.len();
    assert_eq!(
        w.write(b"b", Value::Int(2)).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(w.len(), len);
    w.write(b"a", Value::Int(3)).unwrap();
    let len = w.len();
    assert!(
        w.write(b"a", Value::Raw(Cow::Owned(vec![0; 65_536])))
            .is_err()
    );
    assert_eq!(w.len(), len);
    let bytes = w.finish().unwrap();
    let mut r = Reader::new(&bytes, limits).unwrap();
    assert_eq!(r.read_int().unwrap(), 1);
    assert_eq!(r.read_int().unwrap(), 3);
    r.finish().unwrap();
}
#[test]
fn debug_does_not_dump_archive_payload() {
    let bytes = fixture(Format::BinSafe);
    let r = Reader::new(&bytes, Limits::default()).unwrap();
    assert!(format!("{r:?}").len() < 200);
    let w = Writer::new(Header::new(Format::BinSafe), Limits::default()).unwrap();
    assert!(format!("{w:?}").len() < 200);
}

#[test]
fn reviewed_header_object_and_diagnostic_boundaries() {
    for format in [Format::Ascii, Format::Binary] {
        let header = Header {
            archiver: b"x",
            ..Header::new(format)
        };
        for field_bytes in 14..18 {
            assert_eq!(
                Writer::new(
                    header,
                    Limits {
                        field_bytes,
                        ..Limits::default()
                    }
                )
                .unwrap_err()
                .kind,
                ErrorKind::LimitExceeded
            );
        }
        let limits = Limits {
            field_bytes: 18,
            ..Limits::default()
        };
        let bytes = Writer::new(header, limits).unwrap().finish().unwrap();
        Reader::new(&bytes, limits).unwrap().finish().unwrap();
    }
    let mut w = Writer::new(Header::new(Format::Binary), Limits::default()).unwrap();
    w.begin_object(b"", b"space [class]", 1).unwrap();
    w.end_object().unwrap();
    let bytes = w.finish().unwrap();
    let mut r = Reader::new(&bytes, Limits::default()).unwrap();
    assert_eq!(
        r.begin_object().unwrap().unwrap().class_name,
        b"space [class]"
    );
    assert!(r.end_object().unwrap());
    r.finish().unwrap();
    let mut bytes = Writer::new(Header::new(Format::Binary), Limits::default())
        .unwrap()
        .finish()
        .unwrap();
    bytes.extend_from_slice(&[12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let mut r = Reader::new(&bytes, Limits::default()).unwrap();
    assert!(r.begin_object().unwrap().unwrap().is_empty());
    assert!(r.end_object().unwrap());
    r.finish().unwrap();
    for body in [
        b"\tfield=int:  invalid\n".as_slice(),
        b"\tfield=vec3: 1 2 invalid\n",
        b"\tfield=rawFloat: 1 2 invalid\n",
    ] {
        let bytes = ascii(body);
        let expected = bytes.windows(7).position(|v| v == b"invalid").unwrap();
        let mut r = Reader::new(&bytes, Limits::default()).unwrap();
        let p = r.position();
        let kind = if body.windows(3).any(|v| v == b"int") {
            ValueType::Int
        } else if body.windows(4).any(|v| v == b"vec3") {
            ValueType::Vec3
        } else {
            ValueType::RawFloat(1)
        };
        assert_eq!(r.read(kind).unwrap_err().offset, Some(expected));
        assert_eq!(r.position(), p);
    }
    let data = vec![b'x'; 70_000];
    let mut w = Writer::new(Header::new(Format::Ascii), Limits::default()).unwrap();
    w.begin_object(&data, b"A", 0).unwrap();
    w.end_object().unwrap();
    let bytes = w.finish().unwrap();
    let mut r = Reader::new(&bytes, Limits::default()).unwrap();
    assert_eq!(r.begin_object().unwrap().unwrap().object_name, data);
    assert!(r.end_object().unwrap());
    r.finish().unwrap();
    let header = Header {
        archiver: &data,
        date: Some(&data),
        user: Some(&data),
        ..Header::new(Format::Ascii)
    };
    assert!(format!("{header:?}").len() < 200);
    assert!(format!("{:?}", Value::Raw(Cow::Borrowed(&data))).len() < 100);
    assert!(
        format!(
            "{:?}",
            Entry {
                name: Some(&data),
                value: Value::String(Cow::Borrowed(&data))
            }
        )
        .len()
            < 200
    );
}

#[test]
fn binsafe_pending_key_table_is_reserved_before_retaining_keys() {
    let base = Writer::new(Header::new(Format::BinSafe), Limits::default())
        .unwrap()
        .len();
    assert!(
        Writer::new(
            Header::new(Format::BinSafe),
            Limits {
                bytes: base + 3,
                ..Limits::default()
            }
        )
        .is_err()
    );
    let limits = Limits {
        bytes: base + 4 + 10 + 8 + 3,
        ..Limits::default()
    };
    let mut w = Writer::new(Header::new(Format::BinSafe), limits).unwrap();
    let before = w.len();
    assert_eq!(
        w.write(b"oversized-key", Value::Int(1)).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(w.len(), before);
    w.write(b"key", Value::Int(7)).unwrap();
    let before = w.len();
    assert!(w.write(b"key", Value::Int(8)).is_err());
    assert_eq!(w.len(), before);
    let bytes = w.finish().unwrap();
    assert_eq!(bytes.len(), limits.bytes);
    let mut r = Reader::new(&bytes, limits).unwrap();
    assert_eq!(r.read_int().unwrap(), 7);
    r.finish().unwrap();
    // End markers also respect the space already reserved for the table.
    let limits = Limits {
        bytes: base + 4 + 12,
        ..Limits::default()
    };
    let mut w = Writer::new(Header::new(Format::BinSafe), limits).unwrap();
    w.begin_object(b"a", b"A", 0).unwrap();
    assert!(w.end_object().is_err());
    assert_eq!(w.depth(), 1);
}

#[test]
fn ascii_blank_record_separators_preserve_payload_and_transaction_offsets() {
    let bytes =
        ascii(b" \t\n[root Class 7 0]\n\n\t \n x=string:  preserved  \n\n x=int:  bad\n\n[]\n \t");
    let mut r = Reader::new(&bytes, Limits::default()).unwrap();
    assert!(r.begin_object().unwrap().is_some());
    assert_eq!(r.read_string().unwrap().as_ref(), b"  preserved  ");
    let p = r.position();
    assert!(!r.end_object().unwrap());
    assert_eq!(r.position(), p);
    let bad = bytes.windows(3).position(|v| v == b"bad").unwrap();
    assert_eq!(r.read_int().unwrap_err().offset, Some(bad));
    assert_eq!(r.position(), p);
    let bytes = ascii(b"\n[root Class 7 0]\n\n x=int:7\n\n[]\n \t");
    let mut r = Reader::new(&bytes, Limits::default()).unwrap();
    r.begin_object().unwrap();
    assert_eq!(r.read_int().unwrap(), 7);
    assert!(r.end_object().unwrap());
    assert!(r.begin_object().unwrap().is_none());
    r.finish().unwrap();
}

#[test]
fn ascii_final_unterminated_records_are_valid_but_missing_ends_are_not() {
    let bytes = ascii(b"x=int:7");
    let mut r = Reader::new(&bytes, Limits::default()).unwrap();
    assert_eq!(r.read_int().unwrap(), 7);
    r.finish().unwrap();
    let bytes = fixture(Format::Ascii);
    let mut r = Reader::new(&bytes[..bytes.len() - 1], Limits::default()).unwrap();
    consume(&mut r).unwrap();
    r.finish().unwrap();
    let bytes = ascii(b"[a A 0 0]\nx=int:7");
    let mut r = Reader::new(&bytes, Limits::default()).unwrap();
    r.begin_object().unwrap();
    r.read_int().unwrap();
    assert_eq!(r.end_object().unwrap_err().kind, ErrorKind::UnexpectedEof);
    assert!(r.finish().is_err());
}

#[test]
fn ascii_line_limits_and_object_numbers_report_actual_offsets() {
    for ending in [b"\n".as_slice(), b"\r\n"] {
        let mut body = b"x=string:12345678901".to_vec();
        body.extend_from_slice(ending);
        let bytes = ascii(&body);
        let mut r = Reader::new(
            &bytes,
            Limits {
                field_bytes: 18,
                ..Limits::default()
            },
        )
        .unwrap();
        let p = r.position();
        assert_eq!(r.read_string().unwrap_err().offset, Some(p));
        assert_eq!(r.position(), p);
    }
    for body in [
        b"\n\t[root Class invalid 0]\n".as_slice(),
        b"\n\t[root Class 7 invalid]\n",
    ] {
        let bytes = ascii(body);
        let mut r = Reader::new(&bytes, Limits::default()).unwrap();
        let p = r.position();
        let expected = bytes.windows(7).position(|v| v == b"invalid").unwrap();
        assert_eq!(r.begin_object().unwrap_err().offset, Some(expected));
        assert_eq!(r.position(), p);
    }
}
