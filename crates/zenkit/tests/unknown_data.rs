use zenkit::{
    ErrorKind, GameVersion, Result,
    animation::ModelAnimation,
    archive::{self, Format, Header, SkipLimits, Value, ValueType},
    mesh::{Mesh, Model, ModelHierarchy, ModelMesh, MultiResolutionMesh, SoftSkinMesh},
    model_script::ModelScript,
    morph::MorphMesh,
    vfs::{Compression, Overwrite, ReadOptions, SaveOptions, Timestamp, Vdf, Vfs, WriteOptions},
};

#[test]
fn padded_typed_tags_require_explicit_wire_capture_or_skipping() {
    for (line, kind) in [
        (b"padded=vec3:+1.000  2 3 4\n".as_slice(), ValueType::Vec3),
        (b"padded=color:1 2 3 4 5\n".as_slice(), ValueType::Color),
    ] {
        let mut out = archive::Writer::new(Header::new(Format::Ascii), Default::default()).unwrap();
        out.unframed(line).unwrap();
        out.write(b"after", Value::Int(77)).unwrap();
        let bytes = out.finish().unwrap();
        let mut input = archive::Reader::new(&bytes, Default::default()).unwrap();
        let position = input.position();
        assert_eq!(input.read(kind).unwrap_err().kind, ErrorKind::Unsupported);
        for raw in [ValueType::Raw(0), ValueType::RawFloat(0)] {
            assert!(input.read(raw).is_err());
            assert_eq!(input.position(), position);
        }
        // The caller knows this ASCII field's exact physical line extent. Its
        // typed tag and lexical bytes survive only through explicit wire capture.
        let mut wire = input.unframed(line.len()).unwrap();
        let retained = wire.take_bytes(line.len(), "known line extent").unwrap();
        assert_eq!(retained, line);
        let mut output =
            archive::Writer::new(Header::new(Format::Ascii), Default::default()).unwrap();
        output.unframed(retained).unwrap();
        output
            .write(b"after", Value::Int(input.read_int().unwrap()))
            .unwrap();
        input.finish().unwrap();
        let rewritten = output.finish().unwrap();
        assert!(rewritten.windows(line.len()).any(|span| span == line));
        let mut input = archive::Reader::new(&rewritten, Default::default()).unwrap();
        input.skip_entry(SkipLimits::default()).unwrap();
        assert_eq!(input.read_int().unwrap(), 77);
        input.finish().unwrap();
    }
}

#[test]
fn binsafe_raw_float_requires_whole_elements_without_consuming_invalid_data() {
    let mut output =
        archive::Writer::new(Header::new(Format::BinSafe), Default::default()).unwrap();
    output
        .write(b"raw", Value::RawFloat([1., 2., 3.].as_slice().into()))
        .unwrap();
    let mut bytes = output.finish().unwrap();
    let position = archive::Reader::new(&bytes, Default::default())
        .unwrap()
        .position();
    // Marker:u8, key:u32, rawFloat tag:u8, payload length:u16. Keep the
    // enclosing body/table boundaries intact while declaring an invalid shape.
    bytes[position + 6..position + 8].copy_from_slice(&11u16.to_le_bytes());
    let mut input = archive::Reader::new(&bytes, Default::default()).unwrap();
    let error = input.read_raw_float(0).unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidData);
    assert_eq!(error.offset, Some(position + 6));
    assert_eq!(input.position(), position);
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn catalog_gap(source: &[u8], gap: &[u8]) -> Vec<u8> {
    let mut bytes = source.to_vec();
    let count = word(source, 272) as usize;
    let original = word(source, 288) as usize;
    let catalog = original + gap.len();
    bytes.splice(original..original, gap.iter().copied());
    put(&mut bytes, 288, catalog as u32);
    for index in 0..count {
        let at = catalog + index * 80;
        if word(&bytes, at + 72) & 0x8000_0000 == 0 {
            let offset = word(&bytes, at + 64);
            put(&mut bytes, at + 64, offset + gap.len() as u32);
        }
    }
    bytes
}

#[test]
fn vdf_gaps_and_trailers_distinguish_zero_padding_from_unknown_nonzero_data() {
    for source in [
        include_bytes!("../tests/fixtures/synthetic-vdf-minimal-g2.vdf").as_slice(),
        include_bytes!("../tests/fixtures/synthetic-union-g2.vdf").as_slice(),
    ] {
        let original = Vdf::from_bytes(source, ReadOptions::default()).unwrap();
        for interior in [false, true] {
            for nonzero in [false, true] {
                let mut padding = [0; 7];
                if nonzero {
                    padding[3] = 0xa5;
                }
                let bytes = if interior {
                    catalog_gap(source, &padding)
                } else {
                    [source, padding.as_slice()].concat()
                };
                let archive = Vdf::from_bytes(&bytes, ReadOptions::default()).unwrap();
                for index in 0..original.entries().len() {
                    assert_eq!(
                        archive.entries()[index].name(),
                        original.entries()[index].name()
                    );
                    assert_eq!(archive.payload(index), original.payload(index));
                }
                for compression in [Compression::None, Compression::Union] {
                    let options = WriteOptions {
                        compression,
                        ..Default::default()
                    };
                    let result = archive.to_bytes_with_options(options);
                    if nonzero {
                        assert_eq!(result.unwrap_err().kind, ErrorKind::Unsupported);
                    } else {
                        assert_eq!(
                            result.unwrap(),
                            original.to_bytes_with_options(options).unwrap()
                        );
                    }
                }
                let mut vfs = Vfs::default();
                vfs.insert_file(b"kept", vec![9], None, Overwrite::KeepExisting)
                    .unwrap();
                let options = SaveOptions::new(
                    GameVersion::Gothic2,
                    Timestamp::from_unix_seconds(1_704_164_646),
                );
                let before = vfs.to_bytes(options.clone()).unwrap();
                let result = vfs.mount_archive(archive, b"", Overwrite::KeepExisting);
                if nonzero {
                    assert_eq!(result.unwrap_err().kind, ErrorKind::Unsupported);
                    assert_eq!(vfs.to_bytes(options).unwrap(), before);
                } else {
                    assert!(result.is_ok());
                }
            }
        }
    }
}

#[test]
fn chunked_assets_reject_unknown_records_and_surplus_known_payloads() {
    struct Case {
        name: &'static str,
        bytes: &'static [u8],
        parse: fn(&[u8]) -> Result<()>,
    }
    let cases = [
        Case {
            name: "MSH",
            bytes: include_bytes!("../tests/fixtures/synthetic-mesh-msh-g1.msh"),
            parse: |b| {
                Mesh::from_bytes(b, GameVersion::Gothic1, false, Default::default()).map(|_| ())
            },
        },
        Case {
            name: "MRM",
            bytes: include_bytes!("../tests/fixtures/synthetic-mesh-mrm-g1.mrm"),
            parse: |b| {
                MultiResolutionMesh::from_bytes(b, GameVersion::Gothic1, Default::default())
                    .map(|_| ())
            },
        },
        Case {
            name: "skin",
            bytes: include_bytes!("../tests/fixtures/synthetic-mesh-skin-g1.skin"),
            parse: |b| {
                SoftSkinMesh::from_bytes(b, GameVersion::Gothic1, Default::default()).map(|_| ())
            },
        },
        Case {
            name: "MDH",
            bytes: include_bytes!("../tests/fixtures/synthetic-mesh-mdh-g1.mdh"),
            parse: |b| ModelHierarchy::from_bytes(b, Default::default()).map(|_| ()),
        },
        Case {
            name: "MDM",
            bytes: include_bytes!("../tests/fixtures/synthetic-mesh-mdm-g1.mdm"),
            parse: |b| {
                ModelMesh::from_bytes(b, GameVersion::Gothic1, Default::default()).map(|_| ())
            },
        },
        Case {
            name: "MDL",
            bytes: include_bytes!("../tests/fixtures/synthetic-mesh-mdl-g1.mdl"),
            parse: |b| Model::from_bytes(b, GameVersion::Gothic1, Default::default()).map(|_| ()),
        },
        Case {
            name: "MAN",
            bytes: include_bytes!("../tests/fixtures/synthetic-animation-man-g1.man"),
            parse: |b| ModelAnimation::from_bytes(b, Default::default()).map(|_| ()),
        },
        Case {
            name: "MMB",
            bytes: include_bytes!("../tests/fixtures/synthetic-animation-mmb-g1.mmb"),
            parse: |b| {
                MorphMesh::from_bytes(b, GameVersion::Gothic1, Default::default()).map(|_| ())
            },
        },
        Case {
            name: "MSB",
            bytes: include_bytes!("../tests/fixtures/synthetic-animation-msb-g1.msb"),
            parse: |b| ModelScript::from_msb(b, Default::default()).map(|_| ()),
        },
    ];
    for case in cases {
        (case.parse)(case.bytes).unwrap();
        let mut unknown = case.bytes.to_vec();
        unknown[..2].copy_from_slice(&0x1234u16.to_le_bytes());
        assert!(
            (case.parse)(&unknown).is_err(),
            "{} unknown first chunk",
            case.name
        );
        let mut appended = case.bytes.to_vec();
        appended.extend_from_slice(&[0x34, 0x12, 0, 0, 0, 0]);
        assert!(
            (case.parse)(&appended).is_err(),
            "{} appended unknown chunk",
            case.name
        );
        let length = word(case.bytes, 2) as usize;
        let mut padded = case.bytes.to_vec();
        padded.insert(6 + length, 0xab);
        put(&mut padded, 2, length as u32 + 1);
        assert!(
            (case.parse)(&padded).is_err(),
            "{} surplus first chunk",
            case.name
        );
    }
}
