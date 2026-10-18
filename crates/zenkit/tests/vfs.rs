use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use zenkit::{
    ErrorKind, GameVersion, Limits,
    vfs::{
        Compression, HostNameEncoding, HostOptions, InsertOutcome, MountReport, Overwrite,
        ReadOptions, SaveOptions, Timestamp, Vdf, Vfs,
    },
};

const INPUT: &[u8] = include_bytes!("../tests/fixtures/synthetic-vdf-minimal-g2.vdf");
const COLLISIONS: &[u8] = include_bytes!("../tests/fixtures/synthetic-vfs-collisions.vdf");
const TIME: Timestamp = Timestamp::from_unix_seconds(1_728_907_200);
fn archive(bytes: &[u8]) -> Vdf {
    Vdf::from_bytes(bytes, ReadOptions::default()).unwrap()
}
fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn options() -> SaveOptions {
    SaveOptions::new(GameVersion::Gothic2, TIME)
}
fn file(vfs: &mut Vfs, path: &[u8], payload: &[u8], time: i64, policy: Overwrite) {
    vfs.insert_file(
        path,
        payload.to_vec(),
        Some(Timestamp::from_unix_seconds(time)),
        policy,
    )
    .unwrap();
}

#[test]
fn overwrite_direction_ties_and_directory_file_collisions() {
    for (policy, old, new, replacement) in [
        (Overwrite::KeepExisting, 10, 20, false),
        (Overwrite::ReplaceExisting, 10, 10, true),
        (Overwrite::ReplaceIfExistingNewer, 20, 10, true),
        (Overwrite::ReplaceIfExistingNewer, 10, 20, false),
        (Overwrite::ReplaceIfExistingOlder, 10, 20, true),
        (Overwrite::ReplaceIfExistingOlder, 20, 10, false),
        (Overwrite::ReplaceIfExistingNewer, 10, 10, false),
        (Overwrite::ReplaceIfExistingOlder, 10, 10, false),
    ] {
        let mut v = Vfs::default();
        file(&mut v, b"File", b"old", old, Overwrite::ReplaceExisting);
        let outcome = v
            .insert_file(
                b"fILE \t",
                b"new".to_vec(),
                Some(Timestamp::from_unix_seconds(new)),
                policy,
            )
            .unwrap();
        assert_eq!(
            outcome,
            if replacement {
                InsertOutcome::Replaced
            } else {
                InsertOutcome::KeptExisting
            }
        );
        let n = v.resolve(b"//file\n/").unwrap();
        assert_eq!(
            n.payload().unwrap(),
            if replacement { b"new" } else { b"old" }
        );
        assert_eq!(n.name(), if replacement { b"fILE" } else { b"File" });
        v.mkdir(b"DIR/CHILD", Some(TIME)).unwrap();
        file(&mut v, b"dir", b"replacement", 50, Overwrite::KeepExisting);
        assert!(v.resolve(b"dir/child").is_some());
        file(
            &mut v,
            b"dir",
            b"replacement",
            50,
            Overwrite::ReplaceExisting,
        );
        assert!(v.resolve(b"dir/child").is_none());
    }
}

#[test]
fn insertion_outcomes_describe_committed_decisions_including_directory_replacement() {
    let mut files = Vfs::default();
    assert_eq!(
        files
            .insert_file(b"same", vec![1], None, Overwrite::KeepExisting)
            .unwrap(),
        InsertOutcome::Inserted
    );
    assert_eq!(
        files
            .insert_file(b"SAME", vec![1], None, Overwrite::ReplaceExisting)
            .unwrap(),
        InsertOutcome::Replaced
    );
    assert_eq!(files.resolve(b"same").unwrap().name(), b"SAME");
    assert_eq!(
        files
            .insert_file(
                b"same",
                vec![2],
                Some(TIME),
                Overwrite::ReplaceIfExistingOlder
            )
            .unwrap(),
        InsertOutcome::KeptExisting
    );
    assert_eq!(files.resolve(b"same").unwrap().payload(), Some(&[1][..]));
    files.mkdir(b"dir/child", None).unwrap();
    assert_eq!(
        files
            .insert_file(b"DIR", vec![3], None, Overwrite::KeepExisting)
            .unwrap(),
        InsertOutcome::KeptExisting
    );
    assert!(files.resolve(b"dir/child").is_some());
    assert_eq!(
        files
            .insert_file(b"DIR", vec![3], None, Overwrite::ReplaceExisting)
            .unwrap(),
        InsertOutcome::Replaced
    );
    assert!(files.resolve(b"dir/child").is_none());
    assert_eq!(files.resolve(b"dir").unwrap().payload(), Some(&[3][..]));

    let mut bounded = Vfs::new(Limits {
        entries: 1,
        ..Limits::default()
    });
    assert!(
        bounded
            .insert_file(b"missing/child", vec![], None, Overwrite::KeepExisting)
            .is_err()
    );
    assert_eq!(
        bounded
            .insert_file(b"first", vec![], None, Overwrite::KeepExisting)
            .unwrap(),
        InsertOutcome::Inserted
    );
    assert!(
        bounded
            .insert_file(b"second", vec![], None, Overwrite::KeepExisting)
            .is_err()
    );
    assert_eq!(bounded.root().children().count(), 1);
    assert!(bounded.resolve(b"second").is_none());
}

#[test]
fn archive_mount_reports_count_directories_descendants_and_replacements() {
    let mut files = Vfs::default();
    assert_eq!(
        files
            .mount_archive(archive(INPUT), b"", Overwrite::KeepExisting)
            .unwrap(),
        MountReport {
            inserted: 6,
            ..MountReport::default()
        }
    );
    assert_eq!(
        files
            .mount_archive(archive(INPUT), b"", Overwrite::KeepExisting)
            .unwrap(),
        MountReport {
            skipped: 4,
            merged_directories: 2,
            ..MountReport::default()
        }
    );
    assert_eq!(
        files
            .mount_archive(archive(INPUT), b"", Overwrite::ReplaceExisting)
            .unwrap(),
        MountReport {
            replaced: 4,
            merged_directories: 2,
            ..MountReport::default()
        }
    );
    for policy in [
        Overwrite::ReplaceIfExistingNewer,
        Overwrite::ReplaceIfExistingOlder,
    ] {
        assert_eq!(
            files.mount_archive(archive(INPUT), b"", policy).unwrap(),
            MountReport {
                skipped: 4,
                merged_directories: 2,
                ..MountReport::default()
            }
        );
    }
    for (policy, expected) in [
        (
            Overwrite::KeepExisting,
            MountReport {
                inserted: 2,
                skipped: 4,
                ..MountReport::default()
            },
        ),
        (
            Overwrite::ReplaceExisting,
            MountReport {
                inserted: 5,
                replaced: 1,
                ..MountReport::default()
            },
        ),
    ] {
        let mut destination = Vfs::default();
        destination
            .insert_file(b"DIR", vec![42], None, Overwrite::KeepExisting)
            .unwrap();
        assert_eq!(
            destination
                .mount_archive(archive(INPUT), b"", policy)
                .unwrap(),
            expected
        );
        assert_eq!(
            destination.resolve(b"dir/inner/deep.dat").is_some(),
            policy == Overwrite::ReplaceExisting
        );
    }
    let empty = archive(&Vfs::default().to_bytes(options()).unwrap());
    assert_eq!(
        files
            .mount_archive(empty, b"", Overwrite::KeepExisting)
            .unwrap(),
        MountReport::default()
    );

    let mut bounded = Vfs::new(Limits {
        entries: 6,
        ..Limits::default()
    });
    bounded
        .insert_file(b"sentinel", vec![1], None, Overwrite::KeepExisting)
        .unwrap();
    assert!(
        bounded
            .mount_archive(archive(INPUT), b"", Overwrite::KeepExisting)
            .is_err()
    );
    assert_eq!(bounded.root().children().count(), 1);
    assert_eq!(
        bounded.resolve(b"sentinel").unwrap().payload(),
        Some(&[1][..])
    );
}

#[test]
fn archive_report_counts_catalog_decisions_even_when_later_duplicates_replace_them() {
    for (policy, empty_report, merged_report) in [
        (
            Overwrite::KeepExisting,
            MountReport {
                inserted: 2,
                skipped: 2,
                ..MountReport::default()
            },
            MountReport {
                inserted: 1,
                skipped: 1,
                merged_directories: 2,
                ..MountReport::default()
            },
        ),
        (
            Overwrite::ReplaceExisting,
            MountReport {
                inserted: 3,
                replaced: 1,
                ..MountReport::default()
            },
            MountReport {
                inserted: 1,
                replaced: 2,
                merged_directories: 1,
                ..MountReport::default()
            },
        ),
    ] {
        let mut empty = Vfs::default();
        assert_eq!(
            empty
                .mount_archive(archive(COLLISIONS), b"", policy)
                .unwrap(),
            empty_report
        );
        let mut destination = Vfs::default();
        destination.mkdir(b"D/E", None).unwrap();
        destination
            .insert_file(b"D/E/old", vec![42], None, Overwrite::KeepExisting)
            .unwrap();
        assert_eq!(
            destination
                .mount_archive(archive(COLLISIONS), b"", policy)
                .unwrap(),
            merged_report
        );
        assert_eq!(
            destination.resolve(b"D/E/y").unwrap().payload(),
            Some(b"new".as_slice())
        );
        assert_eq!(
            destination.resolve(b"D/E/old").is_some(),
            policy == Overwrite::KeepExisting
        );
    }
}

#[test]
fn children_support_reverse_and_interleaved_iteration_in_normalized_name_order() {
    let mut files = Vfs::default();
    for name in [b"Z", b"a", b"B"] {
        files
            .insert_file(name, vec![], None, Overwrite::KeepExisting)
            .unwrap();
    }
    let reversed: Vec<_> = files.root().children().rev().map(|n| n.name()).collect();
    assert_eq!(reversed, [b"Z", b"B", b"a"]);
    let mut children = files.root().children();
    assert_eq!(children.next().unwrap().name(), b"a");
    assert_eq!(children.next_back().unwrap().name(), b"Z");
    assert_eq!(children.next_back().unwrap().name(), b"B");
    assert!(children.next().is_none());
    assert!(children.next_back().is_none());
    assert!(
        files
            .resolve(b"a")
            .unwrap()
            .children()
            .next_back()
            .is_none()
    );
    assert!(Vfs::default().root().children().next_back().is_none());
}

#[test]
fn lookup_find_and_atomic_path_operations() {
    let mut v = Vfs::default();
    v.mkdir(b"a/nested", Some(TIME)).unwrap();
    v.mkdir(b"z", Some(TIME)).unwrap();
    file(
        &mut v,
        b"a/nested/match",
        b"a",
        1,
        Overwrite::ReplaceExisting,
    );
    file(&mut v, b"z/MATCH", b"z", 2, Overwrite::ReplaceExisting);
    assert_eq!(v.find(b"match  ").unwrap().payload(), Some(&b"z"[..]));
    file(&mut v, b"match", b"root", 3, Overwrite::ReplaceExisting);
    assert_eq!(v.find(b"MATCH").unwrap().payload(), Some(&b"root"[..]));
    assert!(v.resolve(b"match/nope").is_none());
    assert_eq!(
        v.mkdir(b"match/nope", Some(TIME)).unwrap_err().kind,
        ErrorKind::NotDirectory
    );
    assert!(v.mkdir(b"new/../bad", Some(TIME)).is_err());
    assert!(v.resolve(b"new").is_none());
    assert!(!v.remove(b"z/").unwrap());
    assert!(v.remove(b"z").unwrap());
    assert!(v.find(b"MATCH").is_some());
    assert!(!v.remove(b"/").unwrap());
    assert!(!v.remove(b"match/nope").unwrap());
    assert!(v.resolve(b"").unwrap().is_directory());
}

#[test]
fn archive_mount_preserves_raw_metadata_and_save_all_variants() {
    let mut source = INPUT.to_vec();
    source[296 + 80] = 0xe9;
    put(&mut source, 296 + 80 + 76, 0x12345678);
    put(&mut source, 296 + 80 + 72, 0x12345);
    let mut v = Vfs::default();
    v.mkdir(b"mount", Some(TIME)).unwrap();
    v.mount_archive(archive(&source), b"mount", Overwrite::ReplaceExisting)
        .unwrap();
    let expected = v.resolve(b"mount/\xe9OOT.TXT").unwrap();
    assert_eq!(expected.attributes(), 0x12345678);
    assert_eq!(expected.extra_flags(), 0x12345);
    for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
        for compression in [Compression::None, Compression::Union] {
            let mut opts = SaveOptions::new(game, TIME);
            opts.compression = compression;
            let out = archive(&v.to_bytes(opts).unwrap());
            let id = out.find(b"mount/\xe9OOT.TXT").unwrap();
            assert_eq!(out.payload(id), expected.payload());
            assert_eq!(out.entries()[id].attributes(), expected.attributes());
            assert_eq!(out.entries()[id].flags() & 0x3fff_ffff, 0x12345);
            assert_eq!(out.timestamp_dos(), TIME.to_dos().unwrap());
            assert_eq!(
                &v.to_bytes(SaveOptions::new(game, TIME)).unwrap()[..256],
                &source[..256]
            );
        }
    }
}

#[test]
fn catalog_case_collisions_use_policy_in_source_order() {
    let mut bytes = INPUT.to_vec();
    bytes[456..520].copy_from_slice(&INPUT[376..440]);
    bytes[456] = b'r';
    for policy in [
        Overwrite::KeepExisting,
        Overwrite::ReplaceExisting,
        Overwrite::ReplaceIfExistingNewer,
        Overwrite::ReplaceIfExistingOlder,
    ] {
        let mut v = Vfs::default();
        v.mount_archive(archive(&bytes), b"", policy).unwrap();
        assert_eq!(
            v.resolve(b"root.txt").unwrap().payload().unwrap(),
            if policy == Overwrite::ReplaceExisting {
                &b""[..]
            } else {
                &b"hello\0world\xff"[..]
            }
        );
    }
}

#[test]
fn nested_duplicate_entries_apply_against_actual_destination_state() {
    let mut v = Vfs::default();
    v.mkdir(b"D/E", None).unwrap();
    file(&mut v, b"D/E/old", b"keep", 1, Overwrite::ReplaceExisting);
    v.mount_archive(archive(COLLISIONS), b"", Overwrite::KeepExisting)
        .unwrap();
    assert_eq!(v.resolve(b"D/E/old").unwrap().payload(), Some(&b"keep"[..]));
    assert_eq!(v.resolve(b"D/E/y").unwrap().payload(), Some(&b"new"[..]));
}

#[test]
fn directory_merge_retains_destination_metadata() {
    let mut v = Vfs::default();
    v.mkdir(b"dIr", None).unwrap();
    v.mount_archive(archive(INPUT), b"", Overwrite::KeepExisting)
        .unwrap();
    let dir = v.resolve(b"DIR").unwrap();
    assert_eq!(dir.name(), b"dIr");
    assert_eq!(dir.timestamp(), None);
    assert_eq!(
        v.resolve(b"dir/inner/deep.dat").unwrap().payload().unwrap(),
        b"deep\x80\xfe"
    );
}

#[test]
fn absent_timestamps_never_order_as_real_negative_instants() {
    let minus_one = Some(Timestamp::from_unix_seconds(-1));
    let zero = Some(Timestamp::from_unix_seconds(0));
    for (existing, incoming) in [(None, None), (None, minus_one), (minus_one, None)] {
        for policy in [
            Overwrite::KeepExisting,
            Overwrite::ReplaceIfExistingNewer,
            Overwrite::ReplaceIfExistingOlder,
            Overwrite::ReplaceExisting,
        ] {
            let mut files = Vfs::default();
            files
                .insert_file(b"old", vec![1], existing, Overwrite::KeepExisting)
                .unwrap();
            files
                .insert_file(b"OLD", vec![2], incoming, policy)
                .unwrap();
            let node = files.resolve(b"old").unwrap();
            if policy == Overwrite::ReplaceExisting {
                assert_eq!(node.name(), b"OLD");
                assert_eq!(node.payload(), Some(&[2][..]));
                assert_eq!(node.timestamp(), incoming);
            } else {
                assert_eq!(node.name(), b"old");
                assert_eq!(node.payload(), Some(&[1][..]));
                assert_eq!(node.timestamp(), existing);
            }
        }
    }
    for (old, new, policy) in [
        (minus_one, zero, Overwrite::ReplaceIfExistingOlder),
        (zero, minus_one, Overwrite::ReplaceIfExistingNewer),
    ] {
        let mut files = Vfs::default();
        files
            .insert_file(b"value", vec![1], old, Overwrite::KeepExisting)
            .unwrap();
        files.insert_file(b"value", vec![2], new, policy).unwrap();
        assert_eq!(files.resolve(b"value").unwrap().payload(), Some(&[2][..]));
        assert_eq!(files.resolve(b"value").unwrap().timestamp(), new);
    }
    assert_eq!(Vfs::default().root().timestamp(), None);
}

#[test]
fn virtual_operation_errors_have_bounded_paths_and_no_fake_byte_offsets() {
    let mut files = Vfs::new(Limits {
        file_payload_bytes: 1,
        ..Limits::default()
    });
    let error = files
        .insert_file(b"raw\xff", vec![1, 2], None, Overwrite::KeepExisting)
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::LimitExceeded);
    assert_eq!(error.offset, None);
    assert_eq!(error.virtual_path(), Some(b"raw\xff".as_slice()));
    assert!(error.to_string().contains("raw\\xff"));
    assert!(!error.to_string().contains("at byte"));
    assert_eq!(files.root().children().count(), 0);

    let error = files.mkdir(&vec![b'x'; 20_000], None).unwrap_err();
    assert_eq!(error.kind, ErrorKind::LimitExceeded);
    assert_eq!(error.offset, None);
    assert_eq!(error.virtual_path(), None);
    assert!(error.to_string().len() < 200);

    let error = files
        .insert_file(b"missing/child", vec![], None, Overwrite::KeepExisting)
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::NotFound);
    assert_eq!(error.offset, None);
    assert_eq!(error.virtual_path(), Some(b"missing/child".as_slice()));
}

#[test]
fn debug_summaries_do_not_recurse_into_archives_or_dump_payloads() {
    let mut files = Vfs::default();
    for index in 0..8 {
        files
            .insert_file(
                format!("file{index}").as_bytes(),
                vec![123; 4096],
                None,
                Overwrite::KeepExisting,
            )
            .unwrap();
    }
    for compression in [Compression::None, Compression::Union] {
        let mut options = options();
        options.compression = compression;
        let archive = archive(&files.to_bytes(options).unwrap());
        assert!(format!("{archive:#?}").len() < 512);
        for entry in archive.entry_views() {
            assert!(format!("{entry:#?}").len() < 512);
            assert!(format!("{:?}", entry.metadata()).len() < 512);
        }
        let mut mounted = Vfs::default();
        mounted
            .mount_archive(archive, b"", Overwrite::KeepExisting)
            .unwrap();
        assert!(format!("{mounted:#?}").len() < 1024);
        for node in mounted.root().children() {
            let debug = format!("{node:#?}");
            assert!(debug.len() < 512);
            assert!(debug.contains("payload_bytes: 4096"));
            assert!(!debug.contains("123, 123"));
        }
    }
    assert!(format!("{files:#?}").len() < 1024);
    assert!(format!("{:?}", files.resolve(b"file0").unwrap()).len() < 512);
}

#[test]
fn limits_fail_atomically_for_insert_mkdir_and_mount() {
    for limits in [
        Limits {
            entries: 2,
            ..Limits::default()
        },
        Limits {
            payload_bytes: 10,
            ..Limits::default()
        },
        Limits {
            input_bytes: 20,
            ..Limits::default()
        },
        Limits {
            nesting: 1,
            ..Limits::default()
        },
    ] {
        let mut v = Vfs::new(limits);
        file(&mut v, b"keep", b"x", 0, Overwrite::ReplaceExisting);
        assert_eq!(
            v.mount_archive(archive(INPUT), b"", Overwrite::ReplaceExisting)
                .unwrap_err()
                .kind,
            ErrorKind::LimitExceeded
        );
        assert_eq!(v.root().children().count(), 1);
        assert_eq!(v.resolve(b"keep").unwrap().payload(), Some(&b"x"[..]));
    }
    let mut v = Vfs::new(Limits {
        entries: 2,
        ..Limits::default()
    });
    assert!(v.mkdir(b"a/b/c", Some(TIME)).is_err());
    assert_eq!(v.root().children().count(), 0);
    v.mkdir(b"a", Some(TIME)).unwrap();
    file(&mut v, b"b", b"", 0, Overwrite::ReplaceExisting);
    assert!(
        v.insert_file(b"c", vec![], Some(TIME), Overwrite::ReplaceExisting)
            .is_err()
    );
    assert!(v.resolve(b"c").is_none());
}

#[test]
fn retained_decoded_archive_storage_is_charged_after_removals() {
    let mut source = Vfs::default();
    file(
        &mut source,
        b"large",
        &vec![1; 1000],
        0,
        Overwrite::ReplaceExisting,
    );
    file(&mut source, b"small", b"x", 0, Overwrite::ReplaceExisting);
    let mut opts = options();
    opts.compression = Compression::Union;
    let bytes = source.to_bytes(opts).unwrap();
    let mut v = Vfs::new(Limits {
        payload_bytes: 1500,
        ..Limits::default()
    });
    v.mkdir(b"a", Some(TIME)).unwrap();
    v.mkdir(b"b", Some(TIME)).unwrap();
    v.mount_archive(archive(&bytes), b"a", Overwrite::ReplaceExisting)
        .unwrap();
    v.remove(b"a/large").unwrap();
    assert_eq!(
        v.mount_archive(archive(&bytes), b"b", Overwrite::ReplaceExisting)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(v.resolve(b"b").unwrap().children().count(), 0);
    assert_eq!(v.resolve(b"a/small").unwrap().payload(), Some(&b"x"[..]));
}

#[test]
fn unknown_source_bytes_and_conflicting_comments_require_explicit_decisions() {
    let mut bytes = INPUT.to_vec();
    bytes.extend_from_slice(b"unknown");
    let mut v = Vfs::default();
    assert_eq!(
        v.mount_archive(archive(&bytes), b"", Overwrite::ReplaceExisting)
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
    assert_eq!(v.root().children().count(), 0);
    v.mkdir(b"a", Some(TIME)).unwrap();
    v.mkdir(b"b", Some(TIME)).unwrap();
    v.mount_archive(archive(INPUT), b"a", Overwrite::ReplaceExisting)
        .unwrap();
    let mut other = INPUT.to_vec();
    other[0] ^= 1;
    v.mount_archive(archive(&other), b"b", Overwrite::ReplaceExisting)
        .unwrap();
    assert_eq!(
        v.to_bytes(options()).unwrap_err().kind,
        ErrorKind::Unsupported
    );
    let mut opts = options();
    opts.comment = Some([42; 256]);
    assert_eq!(&v.to_bytes(opts).unwrap()[..256], &[42; 256]);
}

#[test]
fn empty_tree_supported_empty_directories_and_small_output_rejected() {
    let mut v = Vfs::default();
    assert_eq!(archive(&v.to_bytes(options()).unwrap()).entries().len(), 0);
    v.mkdir(b"empty", Some(TIME)).unwrap();
    assert_eq!(
        v.to_bytes(options()).unwrap_err().kind,
        ErrorKind::Unsupported
    );
    v.remove(b"empty").unwrap();
    file(&mut v, b"empty.bin", b"", 0, Overwrite::ReplaceExisting);
    let mut opts = options();
    opts.compression = Compression::Union;
    let out = archive(&v.to_bytes(opts.clone()).unwrap());
    assert_eq!(out.payload(0), Some(&b""[..]));
    opts.output_bytes = 375;
    assert_eq!(v.to_bytes(opts).unwrap_err().kind, ErrorKind::LimitExceeded);
}

#[test]
fn utc_timestamp_bounds_calendar_validation_and_two_second_flooring() {
    for time in [315_532_800, 951_782_400, 1_728_907_201, 4_354_819_199] {
        let t = Timestamp::from_unix_seconds(time);
        assert_eq!(
            Timestamp::from_dos(t.to_dos().unwrap())
                .unwrap()
                .unix_seconds(),
            time - time % 2
        );
    }
    for time in [i64::MIN, 0, 315_532_799, 4_354_819_200, i64::MAX] {
        assert!(Timestamp::from_unix_seconds(time).to_dos().is_err());
    }
    for dos in [
        0,
        (120 << 25) | (2 << 21) | (29 << 16),
        (44 << 25) | (1 << 21) | (1 << 16) | 31,
        (44 << 25) | (1 << 21) | (1 << 16) | (24 << 11),
    ] {
        assert!(Timestamp::from_dos(dos).is_err());
    }
}

struct Temp(PathBuf, PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let path = root.join(format!(
            "vfs-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        let path = path.canonicalize().unwrap();
        assert_eq!(path.parent(), Some(root.as_path()));
        Self(path, root)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let path = self.0.canonicalize().unwrap();
        assert_eq!(path.parent(), Some(self.1.as_path()));
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[test]
fn host_and_archive_path_mounts_report_source_entries_and_exclude_container_roots() {
    let tmp = Temp::new();
    std::fs::create_dir_all(tmp.0.join("nested/deeper")).unwrap();
    std::fs::write(tmp.0.join("nested/deeper/empty.bin"), []).unwrap();
    std::fs::write(tmp.0.join("data.bin"), [1]).unwrap();
    let mut files = Vfs::default();
    assert_eq!(
        files
            .mount_host(&tmp.0, b"", HostOptions::default())
            .unwrap(),
        MountReport {
            inserted: 4,
            ..MountReport::default()
        }
    );
    let keep = HostOptions {
        overwrite: Overwrite::KeepExisting,
        ..HostOptions::default()
    };
    assert_eq!(
        files.mount_host(&tmp.0, b"", keep).unwrap(),
        MountReport {
            skipped: 2,
            merged_directories: 2,
            ..MountReport::default()
        }
    );
    assert_eq!(
        files
            .mount_host(&tmp.0, b"", HostOptions::default())
            .unwrap(),
        MountReport {
            replaced: 2,
            merged_directories: 2,
            ..MountReport::default()
        }
    );
    for (policy, expected) in [
        (
            Overwrite::KeepExisting,
            MountReport {
                inserted: 1,
                skipped: 3,
                ..MountReport::default()
            },
        ),
        (
            Overwrite::ReplaceIfExistingOlder,
            MountReport {
                inserted: 1,
                skipped: 3,
                ..MountReport::default()
            },
        ),
        (
            Overwrite::ReplaceExisting,
            MountReport {
                inserted: 3,
                replaced: 1,
                ..MountReport::default()
            },
        ),
    ] {
        let mut destination = Vfs::default();
        destination
            .insert_file(b"nested", vec![42], None, Overwrite::KeepExisting)
            .unwrap();
        assert_eq!(
            destination
                .mount_host(
                    &tmp.0,
                    b"",
                    HostOptions {
                        overwrite: policy,
                        ..keep
                    }
                )
                .unwrap(),
            expected
        );
        assert_eq!(
            destination.resolve(b"nested/deeper/empty.bin").is_some(),
            policy == Overwrite::ReplaceExisting
        );
    }
    let mut single = Vfs::default();
    let file = tmp.0.join("data.bin");
    assert_eq!(
        single.mount_host(&file, b"", keep).unwrap(),
        MountReport {
            inserted: 1,
            ..MountReport::default()
        }
    );
    assert_eq!(
        single.mount_host(&file, b"", keep).unwrap(),
        MountReport {
            skipped: 1,
            ..MountReport::default()
        }
    );
    let empty = Temp::new();
    assert_eq!(
        single.mount_host(&empty.0, b"", keep).unwrap(),
        MountReport::default()
    );

    let mut bounded = Vfs::new(Limits {
        entries: 3,
        ..Limits::default()
    });
    assert!(bounded.mount_host(&tmp.0, b"", keep).is_err());
    assert_eq!(bounded.root().children().count(), 0);

    let path = tmp.0.join("source.vdf");
    std::fs::write(&path, INPUT).unwrap();
    let mut destination = Vfs::default();
    assert_eq!(
        destination
            .mount_archive_path(&path, b"", Overwrite::KeepExisting)
            .unwrap(),
        MountReport {
            inserted: 6,
            ..MountReport::default()
        }
    );
}

#[test]
fn host_snapshot_includes_empty_files_file_roots_and_explicit_encoding() {
    let tmp = Temp::new();
    std::fs::create_dir(tmp.0.join("Nested")).unwrap();
    std::fs::write(tmp.0.join("Nested/empty.bin"), []).unwrap();
    std::fs::write(tmp.0.join("data.bin"), [0, 1, 255]).unwrap();
    let mut v = Vfs::default();
    v.mount_host(&tmp.0, b"", HostOptions::default()).unwrap();
    assert_eq!(
        v.resolve(b"nested/empty.bin").unwrap().payload(),
        Some(&b""[..])
    );
    let time = std::fs::metadata(tmp.0.join("data.bin"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(
        v.resolve(b"data.bin").unwrap().timestamp(),
        Some(Timestamp::from_system_time(time).unwrap())
    );
    v.mkdir(b"single", Some(TIME)).unwrap();
    v.mount_host(tmp.0.join("data.bin"), b"single", HostOptions::default())
        .unwrap();
    std::fs::write(tmp.0.join("caf\u{e9}.bin"), b"utf8").unwrap();
    assert_eq!(
        v.mount_host(&tmp.0, b"", HostOptions::default())
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
    assert!(v.resolve("caf\u{e9}.bin".as_bytes()).is_none());
    v.mount_host(
        &tmp.0,
        b"",
        HostOptions {
            names: HostNameEncoding::Utf8,
            ..HostOptions::default()
        },
    )
    .unwrap();
    assert_eq!(
        v.resolve("caf\u{e9}.bin".as_bytes()).unwrap().payload(),
        Some(&b"utf8"[..])
    );
}

#[test]
fn host_errors_and_limits_do_not_publish_partial_snapshots() {
    let tmp = Temp::new();
    std::fs::write(tmp.0.join("file"), [1, 2, 3]).unwrap();
    std::fs::write(tmp.0.join("other"), [4, 5, 6]).unwrap();
    let mut v = Vfs::new(Limits {
        input_bytes: 5,
        ..Limits::default()
    });
    file(&mut v, b"keep", b"x", 0, Overwrite::ReplaceExisting);
    assert_eq!(
        v.mount_host(&tmp.0, b"", HostOptions::default())
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(v.root().children().count(), 1);
    assert!(
        v.mount_host(tmp.0.join("missing"), b"", HostOptions::default())
            .is_err()
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(tmp.0.join("file"), tmp.0.join("link")).unwrap();
        let mut v = Vfs::default();
        assert_eq!(
            v.mount_host(&tmp.0, b"", HostOptions::default())
                .unwrap_err()
                .kind,
            ErrorKind::Unsupported
        );
    }
}

#[test]
fn host_errors_preserve_failing_child_paths_sources_and_parser_offsets() {
    use std::error::Error as _;
    let tmp = Temp::new();
    let mut files = Vfs::default();
    files.mkdir(b"target", None).unwrap();
    let missing = tmp.0.join("missing");
    let error = files
        .mount_host(&missing, b"target", HostOptions::default())
        .unwrap_err();
    assert_eq!(error.host_path(), Some(missing.as_path()));
    assert_eq!(error.virtual_path(), Some(b"target".as_slice()));
    assert_eq!(error.offset, None);
    assert!(
        error
            .source()
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .is_some()
    );

    let nested = tmp.0.join("nested");
    std::fs::create_dir(&nested).unwrap();
    let child = nested.join("caf\u{e9}.bin");
    std::fs::write(&child, b"data").unwrap();
    let error = files
        .mount_host(&tmp.0, b"target", HostOptions::default())
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unsupported);
    assert_eq!(error.host_path(), Some(child.as_path()));
    assert_eq!(error.virtual_path(), Some(b"target".as_slice()));
    assert_eq!(error.offset, None);
    assert_eq!(files.resolve(b"target").unwrap().children().count(), 0);

    let invalid = tmp.0.join("invalid.vdf");
    let mut bytes = INPUT.to_vec();
    bytes[256] = 0;
    std::fs::write(&invalid, bytes).unwrap();
    let error = files
        .mount_archive_path(&invalid, b"target", Overwrite::KeepExisting)
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidData);
    assert_eq!(error.offset, Some(256));
    assert_eq!(error.host_path(), Some(invalid.as_path()));
    assert_eq!(error.virtual_path(), Some(b"target".as_slice()));
}
