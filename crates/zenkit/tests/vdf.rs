use zenkit::{
    ErrorKind, Limits,
    io::Reader,
    vfs::{ReadOptions, Vdf},
};

const INPUT: &[u8] = include_bytes!("../tests/fixtures/synthetic-vdf-minimal-g2.vdf");

#[test]
fn file_snapshots_match_streams_and_retain_exact_byte_limits_and_paths() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/synthetic-vdf-minimal-g2.vdf");
    let mut options = ReadOptions::default();
    options.limits.input_bytes = INPUT.len();
    let file = Vdf::from_path(&path, options).unwrap();
    let stream = Vdf::from_reader(INPUT, options).unwrap();
    assert_eq!(file.entries().len(), stream.entries().len());
    for (index, (left, right)) in file.entries().iter().zip(stream.entries()).enumerate() {
        assert_eq!(left.name(), right.name());
        assert_eq!(left.parent(), right.parent());
        assert_eq!(left.attributes(), right.attributes());
        assert_eq!(left.flags(), right.flags());
        assert_eq!(file.payload(index), stream.payload(index));
    }
    options.limits.input_bytes -= 1;
    let error = Vdf::from_path(&path, options).unwrap_err();
    assert_eq!(error.kind, ErrorKind::LimitExceeded);
    assert_eq!(error.offset, None);
    assert_eq!(error.host_path(), Some(path.as_path()));
}

#[test]
fn entry_views_support_reverse_iteration_and_retain_exact_remaining_length() {
    let archive = Vdf::from_bytes(INPUT, ReadOptions::default()).unwrap();
    let count = archive.entries().len();
    assert!(
        archive
            .entry_views()
            .rev()
            .map(|e| e.index())
            .eq((0..count).rev())
    );
    let mut views = archive.entry_views();
    assert_eq!(views.next().unwrap().index(), 0);
    assert_eq!(views.next_back().unwrap().index(), count - 1);
    assert_eq!(views.len(), count - 2);
    assert!(views.map(|e| e.index()).eq(1..count - 1));
}

#[test]
fn archive_bound_views_keep_metadata_payload_and_parent_together() {
    let archive = Vdf::from_bytes(INPUT, ReadOptions::default()).unwrap();
    let payload = archive
        .resolve(b"DIR/INNER/DEEP.DAT")
        .unwrap()
        .payload()
        .unwrap();
    assert_eq!(payload, b"deep\x80\xfe");
    let entry = archive.resolve(b"//dir/inner/deep.dat \t/").unwrap();
    assert_eq!(entry.name(), b"DEEP.DAT");
    assert!(!entry.is_directory());
    let parent = entry.parent().unwrap();
    assert_eq!(parent.name(), b"INNER");
    assert!(parent.is_directory());
    assert!(parent.payload().is_none());
    assert_eq!(parent.parent().unwrap().name(), b"DIR");
    assert!(parent.parent().unwrap().parent().is_none());
    assert!(archive.entry(usize::MAX).is_none());
    assert!(archive.resolve(b"missing").is_none());
    assert_eq!(archive.entry_views().len(), archive.entries().len());
    for (index, view) in archive.entry_views().enumerate() {
        assert_eq!(view.index(), index);
        assert!(std::ptr::eq(view.metadata(), &archive.entries()[index]));
        assert_eq!(view.payload(), archive.payload(index));
    }
}

#[test]
fn output_and_stream_limit_failures_do_not_claim_input_byte_locations() {
    use zenkit::vfs::{Compression, SaveOptions, Timestamp, Vfs, WriteOptions};
    let archive = Vdf::from_bytes(INPUT, ReadOptions::default()).unwrap();
    let mut files = Vfs::default();
    files
        .mount_archive(
            Vdf::from_bytes(INPUT, ReadOptions::default()).unwrap(),
            b"",
            zenkit::vfs::Overwrite::KeepExisting,
        )
        .unwrap();
    // First cap fails before the catalog, second fails while appending a payload.
    for cap in [0, 296 + 80 * 6] {
        for compression in [Compression::None, Compression::Union] {
            let error = archive
                .to_bytes_with_options(WriteOptions {
                    compression,
                    output_bytes: cap,
                })
                .unwrap_err();
            assert_eq!(error.kind, ErrorKind::LimitExceeded);
            assert_eq!(error.offset, None);
            let mut options = SaveOptions::new(
                zenkit::GameVersion::Gothic2,
                Timestamp::from_unix_seconds(1_728_907_200),
            );
            options.compression = compression;
            options.output_bytes = cap;
            let error = files.to_bytes(options).unwrap_err();
            assert_eq!(error.kind, ErrorKind::LimitExceeded);
            assert_eq!(error.offset, None);
        }
    }
    for compression in [Compression::None, Compression::Union] {
        let bytes = archive
            .to_bytes_with_options(WriteOptions {
                compression,
                ..WriteOptions::default()
            })
            .unwrap();
        let mut options = WriteOptions {
            compression,
            output_bytes: bytes.len(),
        };
        assert_eq!(archive.to_bytes_with_options(options).unwrap(), bytes);
        options.output_bytes -= 1;
        let error = archive.to_bytes_with_options(options).unwrap_err();
        assert_eq!(error.kind, ErrorKind::LimitExceeded);
        assert_eq!(error.offset, None);
    }
    let error = zenkit::io::read_bounded(&b"ab"[..], 1).unwrap_err();
    assert_eq!(error.kind, ErrorKind::LimitExceeded);
    assert_eq!(error.offset, None);
    let error = zenkit::io::read_bounded(&b""[..], usize::MAX).unwrap_err();
    assert_eq!(error.kind, ErrorKind::LimitExceeded);
    assert_eq!(error.offset, None);
}

fn parse(bytes: &[u8]) -> zenkit::Result<Vdf> {
    Vdf::from_bytes(bytes, ReadOptions::default())
}
fn set(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn follows_catalog_offsets_and_opens_exact_nonempty_payloads() {
    let archive = parse(INPUT).unwrap();
    assert_eq!(archive.entries().len(), 6);
    assert_eq!(
        archive
            .payload(archive.find(b"//dir/inner/deep.dat  ").unwrap())
            .unwrap(),
        b"deep\x80\xfe"
    );
    assert_eq!(
        archive.payload(archive.find(b"ROOT.TXT").unwrap()).unwrap(),
        b"hello\0world\xff"
    );
    assert_eq!(
        archive.payload(archive.find(b"zero.bin").unwrap()).unwrap(),
        b""
    );
    assert!(archive.payload(archive.find(b"DIR").unwrap()).is_none());
    assert!(archive.find(b"DIR/ROOT.TXT").is_none());
    assert!(archive.payload(usize::MAX).is_none());
}

#[test]
fn rejects_every_truncation_of_nonempty_fixture() {
    for size in 0..INPUT.len() {
        assert!(parse(&INPUT[..size]).is_err(), "accepted length {size}");
    }
}

#[test]
fn rejects_cycles_missing_terminators_and_unreachable_entries() {
    for (offset, value) in [
        (296 + 64, 0),
        (296 + 64, u32::MAX),
        (296 + 2 * 80 + 72, 0),
        (296 + 4 * 80 + 72, 0),
    ] {
        let mut bytes = INPUT.to_vec();
        set(&mut bytes, offset, value);
        assert!(parse(&bytes).is_err(), "accepted mutation at {offset}");
    }
    let mut bytes = INPUT.to_vec();
    set(&mut bytes, 296 + 72, 0xc000_0000);
    assert_eq!(parse(&bytes).unwrap_err().kind, ErrorKind::InvalidData);
}

#[test]
fn rejects_invalid_extent_count_and_metadata_overlap() {
    for (offset, value) in [
        (272, u32::MAX),
        (276, 5),
        (288, u32::MAX),
        (288, 1),
        (296 + 80 + 64, u32::MAX),
        (296 + 80 + 68, u32::MAX),
        (296 + 80 + 64, 296),
    ] {
        let mut bytes = INPUT.to_vec();
        set(&mut bytes, offset, value);
        assert!(parse(&bytes).is_err(), "accepted mutation at {offset}");
    }
}

#[test]
fn unknown_volume_is_explicitly_unsupported() {
    for flags in [0, u32::MAX] {
        let mut bytes = INPUT.to_vec();
        set(&mut bytes, 292, flags);
        let error = parse(&bytes).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unsupported);
        assert_eq!(error.offset, Some(292));
    }
}

#[test]
fn checks_limits_before_file_controlled_allocation_or_traversal() {
    let defaults = Limits::default();
    for limits in [
        Limits {
            input_bytes: 10,
            ..defaults
        },
        Limits {
            entries: 5,
            ..defaults
        },
        Limits {
            nesting: 0,
            ..defaults
        },
        Limits {
            payload_bytes: 1,
            ..defaults
        },
    ] {
        assert_eq!(
            Vdf::from_bytes(INPUT, ReadOptions { limits })
                .unwrap_err()
                .kind,
            ErrorKind::LimitExceeded
        );
    }
}

#[test]
fn preserves_raw_names_metadata_and_payloads_through_writer() {
    let mut bytes = INPUT.to_vec();
    bytes[296 + 80] = 0xe9;
    set(&mut bytes, 296 + 80 + 76, 0x12345678);
    let archive = parse(&bytes).unwrap();
    let encoded = archive.to_bytes().unwrap();
    let again = parse(&encoded).unwrap();
    assert_eq!(again.entries()[1].name(), b"\xe9OOT.TXT");
    assert_eq!(again.entries()[1].attributes(), 0x12345678);
    assert_eq!(again.timestamp_dos(), archive.timestamp_dos());
    for i in 0..archive.entries().len() {
        assert_eq!(archive.entries()[i].name(), again.entries()[i].name());
        assert_eq!(archive.entries()[i].parent(), again.entries()[i].parent());
        assert_eq!(archive.payload(i), again.payload(i));
    }
}

#[test]
fn validates_signature_and_legacy_zero_catalog_offset() {
    let mut bytes = INPUT.to_vec();
    set(&mut bytes, 288, 0);
    assert!(parse(&bytes).is_ok());
    bytes[256] = 0;
    assert_eq!(parse(&bytes).unwrap_err().offset, Some(256));
}

#[test]
fn writer_refuses_to_silently_discard_unknown_trailing_data() {
    let mut bytes = INPUT.to_vec();
    bytes.extend_from_slice(b"unknown extension");
    assert_eq!(
        parse(&bytes).unwrap().to_bytes().unwrap_err().kind,
        ErrorKind::Unsupported
    );
}

#[test]
fn preserves_catalog_collisions_for_explicit_vfs_mount_policy() {
    let mut bytes = INPUT.to_vec();
    bytes[296 + 160..296 + 224].copy_from_slice(&INPUT[296 + 80..296 + 144]);
    let archive = parse(&bytes).unwrap();
    assert_eq!(archive.entries()[1].name(), archive.entries()[2].name());
    assert_eq!(archive.find(b"root.txt"), Some(1));
    assert_eq!(
        parse(&archive.to_bytes().unwrap()).unwrap().entries().len(),
        6
    );
}

#[test]
fn substreams_preserve_absolute_offsets_and_do_not_overread() {
    let mut reader = Reader::new(&[0, 0, 1, 2, 3, 4, 5]);
    reader.take_bytes(2, "prefix").unwrap();
    let mut child = reader.substream(4, "child").unwrap();
    assert_eq!(child.u32("value").unwrap(), 0x04030201);
    let error = child.take_bytes(1, "child end").unwrap_err();
    assert_eq!(error.kind, ErrorKind::UnexpectedEof);
    assert_eq!(error.offset, Some(6));
    assert_eq!(reader.take_bytes(1, "tail").unwrap(), &[5]);
    assert!(reader.take_bytes(usize::MAX, "overflow").is_err());
}

#[test]
fn reader_handles_short_reads_and_preserves_io_source() {
    use std::io::{self, Read};
    struct OneByte<'a>(&'a [u8]);
    impl Read for OneByte<'_> {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if out.is_empty() || self.0.is_empty() {
                return Ok(0);
            }
            out[0] = self.0[0];
            self.0 = &self.0[1..];
            Ok(1)
        }
    }
    assert!(Vdf::from_reader(OneByte(INPUT), ReadOptions::default()).is_ok());
    assert!(Vdf::from_reader(OneByte(&INPUT[..10]), ReadOptions::default()).is_err());
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("broken source"))
        }
    }
    let error = Vdf::from_reader(Broken, ReadOptions::default()).unwrap_err();
    assert!(std::error::Error::source(&error).is_some());
}
