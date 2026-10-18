use zenkit::{
    ErrorKind, GameVersion, Limits,
    vfs::{
        Compression, MountReport, Node, Overwrite, ReadOptions, SaveOptions, Timestamp, Vdf, Vfs,
    },
};

const TIME: Timestamp = Timestamp::from_unix_seconds(1_728_907_200);
const INPUT: &[u8] = include_bytes!("../tests/fixtures/synthetic-vdf-minimal-g2.vdf");

fn file(vfs: &mut Vfs, path: &[u8], bytes: &[u8], time: Option<Timestamp>) {
    vfs.insert_file(path, bytes.to_vec(), time, Overwrite::ReplaceExisting)
        .unwrap();
}

#[derive(Debug, PartialEq, Eq)]
struct Observation {
    path: Vec<u8>,
    raw: [u8; 64],
    time: Option<Timestamp>,
    attributes: u32,
    flags: u32,
    payload: Option<Vec<u8>>,
}

fn snapshot(vfs: &Vfs) -> Vec<Observation> {
    let mut pending = vec![(vfs.root(), Vec::new())];
    let mut rows = Vec::new();
    while let Some((node, path)) = pending.pop() {
        rows.push(Observation {
            path: path.clone(),
            raw: *node.raw_name(),
            time: node.timestamp(),
            attributes: node.attributes(),
            flags: node.extra_flags(),
            payload: node.payload().map(<[u8]>::to_vec),
        });
        for child in node.children().rev() {
            let mut next = path.clone();
            next.push(b'/');
            next.extend_from_slice(child.name());
            pending.push((child, next));
        }
    }
    rows
}

fn branch() -> Node {
    let mut source = Vfs::default();
    source.mkdir(b"branch", None).unwrap();
    file(&mut source, b"branch/a", b"abc", Some(TIME));
    file(&mut source, b"branch/b", b"12345", None);
    source.resolve(b"branch").unwrap().clone()
}

#[test]
fn owned_snapshot_preserves_mixed_metadata_empty_directories_and_shared_payloads() {
    let mut source = Vfs::default();
    let negative = Some(Timestamp::from_unix_seconds(-1));
    source.mkdir(b"Mod \t/Empty", negative).unwrap();
    source.mkdir(b"Mod/Other", None).unwrap();
    file(&mut source, b"Mod/raw\xff", b"\0\xffbytes", Some(TIME));
    let pointer = source
        .resolve(b"Mod/raw\xff")
        .unwrap()
        .payload()
        .unwrap()
        .as_ptr();
    let incoming = source.resolve(b"Mod").unwrap().clone();
    let raw = *incoming.raw_name();
    file(&mut source, b"Mod/raw\xff", b"changed", None);
    source.remove(b"Mod/Empty").unwrap();
    drop(source);
    let mut target = Vfs::default();
    target.mkdir(b"parent", None).unwrap();
    let old = target.clone();
    let report = target
        .mount_node(incoming, b"/parent//", Overwrite::KeepExisting)
        .unwrap();
    assert_eq!(
        report,
        MountReport {
            inserted: 4,
            ..MountReport::default()
        }
    );
    let mounted = target.resolve(b"parent/mod").unwrap();
    assert_eq!(mounted.raw_name(), &raw);
    assert_eq!(mounted.timestamp(), negative);
    assert_eq!(
        target.resolve(b"parent/mod/empty").unwrap().timestamp(),
        negative
    );
    assert_eq!(
        target.resolve(b"parent/mod/other").unwrap().timestamp(),
        None
    );
    let payload = target.resolve(b"parent/mod/raw\xff").unwrap();
    assert_eq!(payload.payload(), Some(&b"\0\xffbytes"[..]));
    assert_eq!(payload.payload().unwrap().as_ptr(), pointer);
    assert_eq!(payload.timestamp(), Some(TIME));
    assert_eq!(old.resolve(b"parent").unwrap().children().count(), 0);
}

#[test]
fn all_collision_types_policies_and_absent_negative_equal_timestamps() {
    let times = [
        None,
        Some(Timestamp::from_unix_seconds(-1)),
        Some(Timestamp::from_unix_seconds(0)),
    ];
    for old in times {
        for new in times {
            for policy in [
                Overwrite::KeepExisting,
                Overwrite::ReplaceExisting,
                Overwrite::ReplaceIfExistingNewer,
                Overwrite::ReplaceIfExistingOlder,
            ] {
                for source_directory in [false, true] {
                    for target_directory in [false, true] {
                        let mut source = Vfs::default();
                        if source_directory {
                            source.mkdir(b"BRANCH", new).unwrap();
                            file(&mut source, b"BRANCH/new", b"new", new);
                        } else {
                            file(&mut source, b"BRANCH", b"new", new);
                        }
                        let mut target = Vfs::default();
                        if target_directory {
                            target.mkdir(b"branch", old).unwrap();
                            file(&mut target, b"branch/old", b"old", old);
                        } else {
                            file(&mut target, b"branch", b"old", old);
                        }
                        let incoming = source.resolve(b"BRANCH").unwrap().clone();
                        let report = target.mount_node(incoming, b"", policy).unwrap();
                        let merged = source_directory && target_directory;
                        let replace = match policy {
                            Overwrite::KeepExisting => false,
                            Overwrite::ReplaceExisting => true,
                            Overwrite::ReplaceIfExistingNewer => {
                                old.zip(new).is_some_and(|(a, b)| a > b)
                            }
                            Overwrite::ReplaceIfExistingOlder => {
                                old.zip(new).is_some_and(|(a, b)| a < b)
                            }
                        };
                        let expected = if merged {
                            MountReport {
                                inserted: 1,
                                merged_directories: 1,
                                ..MountReport::default()
                            }
                        } else if replace {
                            MountReport {
                                replaced: 1,
                                inserted: usize::from(source_directory),
                                ..MountReport::default()
                            }
                        } else {
                            MountReport {
                                skipped: 1 + usize::from(source_directory),
                                ..MountReport::default()
                            }
                        };
                        assert_eq!(report, expected);
                        let node = target.resolve(b"branch").unwrap();
                        assert_eq!(node.timestamp(), if merged || !replace { old } else { new });
                        assert_eq!(
                            node.name(),
                            if merged || !replace {
                                &b"branch"[..]
                            } else {
                                &b"BRANCH"[..]
                            }
                        );
                        assert_eq!(
                            node.is_directory(),
                            if merged || !replace {
                                target_directory
                            } else {
                                source_directory
                            }
                        );
                        if merged {
                            assert_eq!(node.children().count(), 2);
                            assert_eq!(node.child(b"old").unwrap().payload(), Some(&b"old"[..]));
                            assert_eq!(node.child(b"new").unwrap().payload(), Some(&b"new"[..]));
                        } else if node.is_directory() {
                            assert_eq!(node.children().count(), 1);
                            let payload = if replace { &b"new"[..] } else { &b"old"[..] };
                            assert_eq!(node.children().next().unwrap().payload(), Some(payload));
                        } else {
                            assert_eq!(
                                node.payload(),
                                Some(if replace { &b"new"[..] } else { &b"old"[..] })
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn skipped_subtrees_still_obey_every_source_budget() {
    let incoming = branch();
    for limits in [
        Limits {
            entries: 2,
            ..Limits::default()
        },
        Limits {
            nesting: 1,
            ..Limits::default()
        },
        Limits {
            file_payload_bytes: 4,
            ..Limits::default()
        },
        Limits {
            payload_bytes: 7,
            ..Limits::default()
        },
        Limits {
            input_bytes: 7,
            ..Limits::default()
        },
    ] {
        let mut target = Vfs::new(limits);
        file(&mut target, b"branch", b"x", None);
        let before = snapshot(&target);
        let err = target
            .mount_node(incoming.clone(), b"", Overwrite::KeepExisting)
            .unwrap_err();
        assert_eq!(err.kind, ErrorKind::LimitExceeded);
        assert_eq!(err.offset, None);
        assert_eq!(err.virtual_path(), Some(&b""[..]));
        assert_eq!(snapshot(&target), before);
    }
    let mut target = Vfs::new(Limits {
        entries: 3,
        nesting: 2,
        file_payload_bytes: 5,
        payload_bytes: 8,
        input_bytes: 8,
        ..Limits::default()
    });
    file(&mut target, b"branch", b"x", None);
    assert_eq!(
        target
            .mount_node(incoming, b"", Overwrite::KeepExisting)
            .unwrap(),
        MountReport {
            skipped: 3,
            ..MountReport::default()
        }
    );
}

#[test]
fn candidate_aggregate_limits_and_parent_relative_depth_roll_back_every_change() {
    for limits in [
        Limits {
            entries: 3,
            ..Limits::default()
        },
        Limits {
            payload_bytes: 8,
            ..Limits::default()
        },
        Limits {
            input_bytes: 8,
            ..Limits::default()
        },
    ] {
        let mut target = Vfs::new(limits);
        file(&mut target, b"sentinel", b"x", None);
        let before = snapshot(&target);
        assert_eq!(
            target
                .mount_node(branch(), b"", Overwrite::ReplaceExisting)
                .unwrap_err()
                .kind,
            ErrorKind::LimitExceeded
        );
        assert_eq!(snapshot(&target), before);
    }
    let mut target = Vfs::new(Limits {
        nesting: 2,
        ..Limits::default()
    });
    target.mkdir(b"parent", None).unwrap();
    let before = snapshot(&target);
    assert_eq!(
        target
            .mount_node(branch(), b"parent", Overwrite::ReplaceExisting)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(snapshot(&target), before);
}

#[test]
fn retained_owned_storage_is_deduplicated_but_logical_occurrences_are_charged() {
    let mut source = Vfs::default();
    file(&mut source, b"data", b"abc", None);
    let incoming = source.resolve(b"data").unwrap().clone();
    drop(source);
    let mut target = Vfs::new(Limits {
        input_bytes: 3,
        payload_bytes: 6,
        ..Limits::default()
    });
    target.mkdir(b"a", None).unwrap();
    target.mkdir(b"b", None).unwrap();
    target.mkdir(b"c", None).unwrap();
    for parent in [b"a", b"b"] {
        target
            .mount_node(incoming.clone(), parent, Overwrite::KeepExisting)
            .unwrap();
    }
    assert_eq!(
        target
            .resolve(b"a/data")
            .unwrap()
            .payload()
            .unwrap()
            .as_ptr(),
        target
            .resolve(b"b/data")
            .unwrap()
            .payload()
            .unwrap()
            .as_ptr()
    );
    let before = snapshot(&target);
    assert_eq!(
        target
            .mount_node(incoming, b"c", Overwrite::KeepExisting)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(snapshot(&target), before);
}

#[test]
fn selected_archive_file_retains_hidden_decoded_storage_and_deduplicates_shared_archives() {
    let mut builder = Vfs::default();
    file(&mut builder, b"large", &[42; 1000], None);
    file(&mut builder, b"small", b"x", None);
    let mut options = SaveOptions::new(GameVersion::Gothic2, TIME);
    options.compression = Compression::Union;
    let bytes = builder.to_bytes(options).unwrap();
    let make_node = || {
        let mut source = Vfs::default();
        source
            .mount_archive(
                Vdf::from_bytes(&bytes, ReadOptions::default()).unwrap(),
                b"",
                Overwrite::KeepExisting,
            )
            .unwrap();
        source.resolve(b"small").unwrap().clone()
    };
    let incoming = make_node();
    for limits in [
        Limits {
            input_bytes: bytes.len() - 1,
            ..Limits::default()
        },
        Limits {
            payload_bytes: 1000,
            ..Limits::default()
        },
    ] {
        let mut target = Vfs::new(limits);
        assert_eq!(
            target
                .mount_node(incoming.clone(), b"", Overwrite::KeepExisting)
                .unwrap_err()
                .kind,
            ErrorKind::LimitExceeded
        );
        assert_eq!(target.root().children().count(), 0);
    }
    let mut target = Vfs::new(Limits {
        input_bytes: bytes.len(),
        payload_bytes: 1001,
        file_payload_bytes: 1,
        compression_blocks: 0,
        compression_block_bytes: 0,
        compression_input_bytes: 0,
        ..Limits::default()
    });
    for parent in [b"a", b"b", b"c"] {
        target.mkdir(parent, None).unwrap();
    }
    for parent in [b"a", b"b"] {
        target
            .mount_node(incoming.clone(), parent, Overwrite::KeepExisting)
            .unwrap();
    }
    let before = snapshot(&target);
    assert_eq!(
        target
            .mount_node(make_node(), b"c", Overwrite::KeepExisting)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(snapshot(&target), before);
    assert_eq!(
        target.resolve(b"a/small").unwrap().payload(),
        Some(&b"x"[..])
    );
}

#[test]
fn archive_metadata_survives_detachment_and_both_games_and_compression_modes() {
    let mut bytes = INPUT.to_vec();
    bytes[376] = 0xe9;
    bytes[452..456].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    bytes[448..452].copy_from_slice(&0x12345u32.to_le_bytes());
    let mut source = Vfs::default();
    source
        .mount_archive(
            Vdf::from_bytes(&bytes, ReadOptions::default()).unwrap(),
            b"",
            Overwrite::KeepExisting,
        )
        .unwrap();
    let node = source.resolve(b"\xe9OOT.TXT").unwrap().clone();
    let raw = *node.raw_name();
    drop(source);
    let mut target = Vfs::default();
    target
        .mount_node(node, b"", Overwrite::KeepExisting)
        .unwrap();
    let expected = target.resolve(b"\xe9OOT.TXT").unwrap();
    assert_eq!(expected.raw_name(), &raw);
    assert_eq!(expected.attributes(), 0x1234_5678);
    assert_eq!(expected.extra_flags(), 0x12345);
    for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
        for compression in [Compression::None, Compression::Union] {
            let mut options = SaveOptions::new(game, TIME);
            options.compression = compression;
            let wire = target.to_bytes(options).unwrap();
            let catalog = u32::from_le_bytes(wire[288..292].try_into().unwrap()) as usize;
            assert_eq!(&wire[catalog..catalog + 64], &raw);
            let output = Vdf::from_owned(wire, ReadOptions::default()).unwrap();
            let entry = output.resolve(b"\xe9OOT.TXT").unwrap();
            assert_eq!(entry.payload(), expected.payload());
            assert_eq!(entry.metadata().attributes(), expected.attributes());
            assert_eq!(
                entry.metadata().flags() & 0x3fff_ffff,
                expected.extra_flags()
            );
        }
    }
}

#[test]
fn invalid_parents_and_synthetic_root_have_context_without_mutation() {
    let mut target = Vfs::default();
    file(&mut target, b"file", b"x", None);
    for (parent, kind) in [
        (&b"missing"[..], ErrorKind::NotFound),
        (&b"file"[..], ErrorKind::NotDirectory),
        (&b"../bad"[..], ErrorKind::InvalidData),
    ] {
        let before = snapshot(&target);
        let err = target
            .mount_node(branch(), parent, Overwrite::KeepExisting)
            .unwrap_err();
        assert_eq!(err.kind, kind);
        assert_eq!(err.virtual_path(), Some(parent));
        assert_eq!(err.offset, None);
        assert_eq!(snapshot(&target), before);
    }
    let before = snapshot(&target);
    let root = target.root().clone();
    assert_eq!(
        target
            .mount_node(root, b"", Overwrite::KeepExisting)
            .unwrap_err()
            .kind,
        ErrorKind::InvalidData
    );
    let err = target
        .mount_node(branch(), &vec![b'/'; 20_000], Overwrite::KeepExisting)
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::LimitExceeded);
    assert_eq!(err.virtual_path(), None);
    assert_eq!(snapshot(&target), before);
}

#[test]
fn mounting_under_own_descendant_uses_a_finite_snapshot() {
    let mut target = Vfs::default();
    target.mkdir(b"a/b", None).unwrap();
    file(&mut target, b"a/data", b"shared", None);
    let detached = target.resolve(b"a").unwrap().clone();
    let report = target
        .mount_node(detached, b"a/b", Overwrite::KeepExisting)
        .unwrap();
    assert_eq!(
        report,
        MountReport {
            inserted: 3,
            ..MountReport::default()
        }
    );
    assert_eq!(target.resolve(b"a/b/a/b").unwrap().children().count(), 0);
    assert_eq!(
        target.resolve(b"a/b/a/data").unwrap().payload(),
        Some(&b"shared"[..])
    );
    assert_eq!(snapshot(&target).len(), 7);
}

#[test]
fn maximum_depth_merges_and_wide_source_rejection_fit_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let limits = Limits {
                nesting: usize::MAX,
                ..Limits::default()
            };
            let path = vec![b"d".as_slice(); 256].join(&b'/');
            let mut source = Vfs::new(limits);
            source.mkdir(&path, None).unwrap();
            let incoming = source.resolve(b"d").unwrap().clone();
            let mut target = Vfs::new(limits);
            target.mkdir(&path, None).unwrap();
            assert_eq!(
                target
                    .mount_node(incoming.clone(), b"", Overwrite::KeepExisting)
                    .unwrap(),
                MountReport {
                    merged_directories: 256,
                    ..MountReport::default()
                }
            );
            let before = snapshot(&target);
            assert_eq!(
                target
                    .mount_node(incoming, b"d", Overwrite::KeepExisting)
                    .unwrap_err()
                    .kind,
                ErrorKind::LimitExceeded
            );
            assert_eq!(snapshot(&target), before);
            let mut wide = Vfs::default();
            wide.mkdir(b"wide", None).unwrap();
            for i in 0..1000 {
                file(&mut wide, format!("wide/{i}").as_bytes(), b"", None);
            }
            let mut tiny = Vfs::new(Limits {
                entries: 1,
                ..Limits::default()
            });
            assert_eq!(
                tiny.mount_node(
                    wide.resolve(b"wide").unwrap().clone(),
                    b"",
                    Overwrite::KeepExisting
                )
                .unwrap_err()
                .kind,
                ErrorKind::LimitExceeded
            );
            assert_eq!(tiny.root().children().count(), 0);
        })
        .unwrap()
        .join()
        .unwrap();
}
