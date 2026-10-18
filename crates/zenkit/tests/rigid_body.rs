use zenkit::objects::ObjectRef;
use zenkit::{
    ErrorKind, GameVersion,
    archive::{self, Format, Header, Reader, Value, Writer},
    objects::{Graph, Limits, Link, Metadata, Object, ObjectArchive},
    values::Vec3,
    vob::{RigidBody, SaveState, Vob},
};

const FORMATS: [Format; 3] = [Format::Ascii, Format::Binary, Format::BinSafe];
fn body() -> RigidBody {
    RigidBody {
        velocity: [-1.25, 2.5, 3.75].into(),
        mode: 255,
        gravity: true,
        gravity_scale: -0.125,
        slide: [4.5, -5.25, 6.125].into(),
    }
}
fn fixture(format: &str, save: bool) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "tests/fixtures/synthetic-rigid-body-{format}-{}.zen",
            if save { "save" } else { "normal" }
        )),
    )
    .unwrap()
}
fn writer(format: Format, limits: archive::Limits) -> Writer {
    Writer::new(Header::new(format), limits).unwrap()
}

#[test]
fn independent_records_compose_with_surrounding_fields_in_both_header_modes() {
    for format in ["ascii", "binary", "bin_safe"] {
        for save in [false, true] {
            let bytes = fixture(format, save);
            let mut reader = Reader::new(&bytes, archive::Limits::default()).unwrap();
            assert_eq!(reader.header().save_game, save);
            assert_eq!(reader.read_int().unwrap(), -701);
            assert_eq!(RigidBody::read(&mut reader).unwrap(), body());
            assert_eq!(reader.read_int().unwrap(), 812);
            let second = RigidBody::read(&mut reader).unwrap();
            assert_eq!(second.velocity, Vec3::from([7.25, -8.5, 9.75]));
            assert_eq!((second.mode, second.gravity), (0, false));
            assert_eq!(second.gravity_scale, 2.25);
            assert_eq!(second.slide, Vec3::from([-10.5, 11.25, -12.125]));
            assert_eq!(reader.read_int().unwrap(), -913);
            reader.finish().unwrap();
        }
    }
}

#[test]
fn nested_fields_add_no_object_frames_and_preserve_following_objects() {
    for format in FORMATS {
        let mut w = writer(format, archive::Limits::default());
        w.begin_object(b"container", b"Example", 9).unwrap();
        w.write(b"before", Value::Int(17)).unwrap();
        body().write(&mut w).unwrap();
        w.write(b"after", Value::Int(29)).unwrap();
        w.end_object().unwrap();
        w.begin_object(b"sibling", b"Example", 10).unwrap();
        w.end_object().unwrap();
        let bytes = w.finish().unwrap();
        let mut r = Reader::new(&bytes, archive::Limits::default()).unwrap();
        assert_eq!(r.begin_object().unwrap().unwrap().version, 9);
        assert_eq!(r.read_int().unwrap(), 17);
        assert_eq!(RigidBody::read(&mut r).unwrap(), body());
        assert_eq!(r.read_int().unwrap(), 29);
        assert!(r.end_object().unwrap());
        assert_eq!(r.begin_object().unwrap().unwrap().version, 10);
        assert!(r.end_object().unwrap());
        r.finish().unwrap();
    }
}

#[test]
fn binary_bits_survive_and_every_mode_byte_is_accepted() {
    // Includes signed zeros, subnormals, infinities and distinct NaN payloads.
    let patterns = [
        0x8000_0000,
        1,
        0xff80_0000,
        0x7fc0_1234,
        0x7fa0_0001,
        0xffc0_789a,
        0x007f_ffff,
    ];
    for format in [Format::Binary, Format::BinSafe] {
        let mut w = writer(format, archive::Limits::default());
        for mode in 0..=255 {
            RigidBody {
                velocity: patterns[..3]
                    .try_into()
                    .map(|v: [u32; 3]| v.map(f32::from_bits).into())
                    .unwrap(),
                mode,
                gravity: mode % 2 != 0,
                gravity_scale: f32::from_bits(patterns[3]),
                slide: patterns[4..]
                    .try_into()
                    .map(|v: [u32; 3]| v.map(f32::from_bits).into())
                    .unwrap(),
            }
            .write(&mut w)
            .unwrap();
        }
        let bytes = w.finish().unwrap();
        let mut r = Reader::new(&bytes, archive::Limits::default()).unwrap();
        for mode in 0..=255 {
            let value = RigidBody::read(&mut r).unwrap();
            assert_eq!(value.velocity.to_array().map(f32::to_bits), patterns[..3]);
            assert_eq!(value.gravity_scale.to_bits(), patterns[3]);
            assert_eq!(value.slide.to_array().map(f32::to_bits), patterns[4..]);
            assert_eq!((value.mode, value.gravity), (mode, mode % 2 != 0));
        }
        r.finish().unwrap();
    }
}

#[test]
fn malformed_composites_retain_the_primitive_failure_boundary() {
    for format in [Format::Ascii, Format::BinSafe] {
        for wrong_name in [false, true] {
            let mut w = writer(format, archive::Limits::default());
            w.write(b"vel", Value::Vec3(body().velocity)).unwrap();
            w.write(b"mode", Value::Byte(255)).unwrap();
            w.write(b"gravOn", Value::Bool(true)).unwrap();
            w.write(b"gravScale", Value::Float(-0.125)).unwrap();
            w.write(
                if wrong_name { b"wrong" } else { b"slideDir" },
                if wrong_name {
                    Value::Vec3(body().slide)
                } else {
                    Value::Int(45)
                },
            )
            .unwrap();
            let bytes = w.finish().unwrap();
            let mut r = Reader::new(&bytes, archive::Limits::default()).unwrap();
            let start = r.position();
            let error = RigidBody::read(&mut r).unwrap_err();
            assert_eq!(error.kind, ErrorKind::InvalidData);
            assert!(error.offset.unwrap() > start);
            assert!(r.position() > start); // Earlier composite fields were consumed.
            if !wrong_name {
                // The final typed primitive failed without consuming its field.
                assert_eq!(r.read_int().unwrap(), 45);
                r.finish().unwrap();
            }
        }
    }
    let mut w = writer(Format::Binary, archive::Limits::default());
    body().write(&mut w).unwrap();
    let bytes = w.finish().unwrap();
    // Every truncation inside the 30-byte binary body must fail, never default.
    for length in bytes.len() - 30..bytes.len() {
        let mut r = Reader::new(&bytes[..length], archive::Limits::default()).unwrap();
        assert_eq!(
            RigidBody::read(&mut r).unwrap_err().kind,
            ErrorKind::UnexpectedEof
        );
    }
}

#[test]
fn late_limits_leave_partial_composites_and_atomic_primitives() {
    let value = RigidBody {
        slide: [123_456_780.0; 3].into(),
        ..body()
    };
    let limits = archive::Limits {
        field_bytes: 40,
        ..archive::Limits::default()
    };
    let mut w = writer(Format::Ascii, limits);
    assert_eq!(
        value.write(&mut w).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    // Inspection of the failed writer is a test only; applications discard it.
    let bytes = w.finish().unwrap();
    let mut r = Reader::new(&bytes, limits).unwrap();
    assert_eq!(r.read_vec3().unwrap(), value.velocity);
    assert_eq!(r.read_byte().unwrap(), value.mode);
    assert_eq!(r.read_bool().unwrap(), value.gravity);
    assert_eq!(r.read_float().unwrap(), value.gravity_scale);
    r.finish().unwrap(); // No fragment of the rejected slide field leaked.

    let mut w = writer(Format::Ascii, archive::Limits::default());
    value.write(&mut w).unwrap();
    let bytes = w.finish().unwrap();
    let mut r = Reader::new(&bytes, limits).unwrap();
    let start = r.position();
    assert_eq!(
        RigidBody::read(&mut r).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    assert!(r.position() > start);
    let failed = r.position();
    assert_eq!(r.read_vec3().unwrap_err().kind, ErrorKind::LimitExceeded);
    assert_eq!(r.position(), failed);

    for format in FORMATS {
        let mut w = writer(format, archive::Limits::default());
        body().write(&mut w).unwrap();
        let bytes = w.finish().unwrap();
        let limits = archive::Limits {
            bytes: bytes.len(),
            ..archive::Limits::default()
        };
        let mut w = writer(format, limits);
        body().write(&mut w).unwrap();
        assert_eq!(w.finish().unwrap().len(), bytes.len());
        let limits = archive::Limits {
            bytes: bytes.len() - 1,
            ..limits
        };
        let mut w = writer(format, limits);
        assert_eq!(
            body().write(&mut w).unwrap_err().kind,
            ErrorKind::LimitExceeded
        );
    }
}

#[test]
fn containing_save_vobs_use_the_same_complete_body_in_both_games() {
    for format in FORMATS {
        for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
            let mut graph = Graph::new().unwrap();
            let mut vob = Vob::new(game);
            vob.base.state = Some(SaveState {
                rigid_body: Some(body()),
                ..SaveState::default()
            });
            let id = graph.insert(Object::Vob(vob)).unwrap();
            graph
                .add_root(Link::new(b"body-owner".to_vec(), Some(id)))
                .unwrap();
            let mut metadata = Metadata::new(format);
            metadata.save_game = true;
            let doc = ObjectArchive {
                metadata,
                game,
                graph,
            };
            let bytes = doc.to_bytes(Limits::default()).unwrap();
            let loaded = ObjectArchive::from_bytes(&bytes, game, Limits::default()).unwrap();
            let ObjectRef::Vob(value) = loaded
                .graph
                .get(loaded.graph.roots()[0].target.unwrap())
                .unwrap()
            else {
                panic!()
            };
            assert_eq!(value.base().state.unwrap().rigid_body, Some(body()));
        }
    }
}
