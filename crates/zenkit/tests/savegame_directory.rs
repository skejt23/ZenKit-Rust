use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use zenkit::objects::ObjectRef;
use zenkit::{
    ErrorKind, GameVersion,
    archive::Format,
    objects::{Graph, Link, Metadata, Object, ObjectArchive},
    savegame::*,
    world::{Section, SectionData, SectionFrame, World},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static ID: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir().canonicalize().unwrap();
        let path = base.join(format!(
            "zenkit-savegame-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let base = std::env::temp_dir().canonicalize().unwrap();
        assert!(self.0.is_absolute() && self.0.starts_with(base));
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn world() -> ObjectArchive {
    let game = GameVersion::Gothic2;
    let mut d = ObjectArchive {
        metadata: Metadata::new(Format::BinSafe),
        game,
        graph: Graph::new().unwrap(),
    };
    d.metadata.save_game = true;
    let mut value = World::new(game, true);
    value.sections.push(Section {
        frame: SectionFrame::default(),
        data: SectionData::End,
    });
    let id = d.graph.insert(Object::World(value)).unwrap();
    d.graph
        .add_root(Link::new(b"world".to_vec(), Some(id)))
        .unwrap();
    d
}
fn snapshot() -> SaveSnapshot {
    let game = GameVersion::Gothic2;
    let mut graph = Graph::new().unwrap();
    let mut info = SaveMetadata::new(game);
    info.world = b"FIRST".to_vec();
    let id = graph.insert(Object::SaveMetadata(info)).unwrap();
    graph
        .add_root(Link::new(b"metadata".to_vec(), Some(id)))
        .unwrap();
    let metadata = ObjectArchive {
        metadata: Metadata::new(Format::Ascii),
        game,
        graph,
    };
    let state = StateArchive::new(game, Format::BinSafe).unwrap();
    let files = vec![
        (
            "SAVEINFO.SAV",
            metadata.to_bytes(Default::default()).unwrap(),
        ),
        ("SAVEDAT.SAV", state.to_bytes(Default::default()).unwrap()),
        ("FIRST.SAV", world().to_bytes(Default::default()).unwrap()),
        ("SECOND.SAV", world().to_bytes(Default::default()).unwrap()),
        ("SAVEHDR.SAV", b"FIRST.ZEN\r\n".to_vec()),
        ("custom/opaque.bin", vec![0, 255, 3, 8]),
    ];
    SaveSnapshot::from_files(
        game,
        files.into_iter().map(|(p, b)| (p.into(), b)),
        Default::default(),
    )
    .unwrap()
}
#[test]
fn whole_snapshot_capture_prepare_and_two_world_publication() {
    let temp = Temp::new();
    let limits = SnapshotLimits::default();
    let prepared = snapshot().prepare(limits).unwrap();
    let first = prepared
        .snapshot()
        .file(Path::new("FIRST.SAV"))
        .unwrap()
        .to_vec();
    assert!(
        prepared
            .snapshot()
            .thumbnail(Default::default())
            .unwrap()
            .is_none()
    );
    assert!(
        prepared
            .snapshot()
            .load_default_world(Default::default())
            .unwrap()
            .is_some()
    );
    assert!(
        prepared
            .snapshot()
            .load_world(Path::new("SECOND.ZEN"), Default::default())
            .unwrap()
            .is_some()
    );
    assert!(
        prepared
            .snapshot()
            .load_world(Path::new("MISSING"), Default::default())
            .unwrap()
            .is_none()
    );
    let published = temp.0.join("new");
    let result = prepared.publish_new(&published).unwrap();
    assert!(result.backup.is_none());
    assert!(!published.join("THUMB.SAV").exists());
    fs::create_dir(published.join("empty")).unwrap();
    let captured = SaveSnapshot::capture(&published, GameVersion::Gothic2, limits).unwrap();
    assert!(captured.directories().any(|d| d == Path::new("empty")));
    let prepared = captured
        .prepare_active_world(Path::new("SECOND.ZEN"), &world(), limits)
        .unwrap();
    let s = prepared.snapshot();
    assert_eq!(s.file(Path::new("FIRST.SAV")).unwrap(), first);
    assert_eq!(
        s.file(Path::new("custom/opaque.bin")).unwrap(),
        [0, 255, 3, 8]
    );
    assert_eq!(s.file(Path::new("SAVEHDR.SAV")).unwrap(), b"SECOND.ZEN\n");
    let metadata = s.metadata(Default::default()).unwrap();
    let ObjectRef::SaveMetadata(info) = metadata
        .graph
        .get(metadata.graph.roots()[0].target.unwrap())
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(info.world, b"SECOND");
    let stage = temp.0.join("staging");
    let backup = temp.0.join("backup");
    let report = prepared
        .replace_exclusive(&published, &stage, &backup)
        .unwrap();
    assert_eq!(report.backup, Some(backup.clone()));
    assert!(!stage.exists());
    assert_eq!(
        fs::read(backup.join("SAVEHDR.SAV")).unwrap(),
        b"FIRST.ZEN\r\n"
    );
    assert_eq!(
        fs::read(published.join("SAVEHDR.SAV")).unwrap(),
        b"SECOND.ZEN\n"
    );
    assert!(published.join("empty").is_dir());
}
#[test]
fn catalogue_updates_are_atomic_under_every_tightened_limit() {
    let files = vec![
        (PathBuf::from("a"), vec![1]),
        (PathBuf::from("dir/b"), vec![2, 3]),
    ];
    let limits = SnapshotLimits::default();
    for cap in [
        SnapshotLimits {
            entries: 1,
            ..limits
        },
        SnapshotLimits { depth: 1, ..limits },
        SnapshotLimits {
            name_bytes: 5,
            ..limits
        },
        SnapshotLimits {
            file_bytes: 1,
            ..limits
        },
        SnapshotLimits {
            total_bytes: 2,
            ..limits
        },
    ] {
        let mut s = SaveSnapshot::from_files(GameVersion::Gothic1, files.clone(), limits).unwrap();
        let e = s.replace_file(Path::new("a"), vec![9], cap).unwrap_err();
        assert_eq!(e.kind, ErrorKind::LimitExceeded);
        assert_eq!(e.offset, None);
        assert_eq!(s.file(Path::new("a")).unwrap(), [1]);
        assert_eq!(s.file(Path::new("dir/b")).unwrap(), [2, 3]);
    }
    let exact = SnapshotLimits {
        entries: 2,
        depth: 2,
        name_bytes: 6,
        file_bytes: 2,
        total_bytes: 3,
        ..limits
    };
    let mut s = SaveSnapshot::from_files(GameVersion::Gothic1, files, exact).unwrap();
    s.replace_file(Path::new("A"), vec![9], exact).unwrap();
    assert_eq!(s.files().next().unwrap().path, Path::new("a"));
    assert_eq!(s.file(Path::new("A")).unwrap(), [9]);
    assert!(s.replace_file(Path::new("c"), vec![], exact).is_err());
}
#[test]
fn unsafe_and_ambiguous_names_are_rejected_without_opening_devices() {
    for name in [
        "",
        ".",
        "../escape",
        "a/../b",
        "a/./b",
        "C:/x",
        "C:x",
        "a:stream",
        "CON",
        "nul.sav",
        "Lpt9.txt",
        "COM\u{00b9}.sav",
        "LPT\u{00b2}",
        "com\u{00b3}",
        "last.",
        "last ",
        "bad?name",
        "a/",
    ] {
        assert!(
            SaveSnapshot::from_files(
                GameVersion::Gothic1,
                [(PathBuf::from(name), vec![])],
                Default::default()
            )
            .is_err(),
            "{name}"
        );
    }
    for paths in [
        ["a", "A"],
        ["a", "a/b"],
        ["Dir/a", "dir/b"],
        ["\u{03c3}", "\u{03c2}"],
    ] {
        assert!(
            SaveSnapshot::from_files(
                GameVersion::Gothic1,
                paths.map(|p| (PathBuf::from(p), vec![])),
                Default::default()
            )
            .is_err()
        );
    }
    let s = snapshot();
    for name in ["../FIRST", "C:OTHER", "sub/world", "CON"] {
        assert!(s.load_world(Path::new(name), Default::default()).is_err());
    }
    let raw = vec![7; 1_000_000];
    let s = SaveSnapshot::from_files(
        GameVersion::Gothic1,
        [(PathBuf::from("large"), raw)],
        Default::default(),
    )
    .unwrap();
    assert!(format!("{:?}", s.files().next().unwrap()).len() < 100);
}
#[test]
fn preparation_rejects_bad_components_and_never_creates_empty_thumbnail() {
    let mut s = snapshot();
    s.replace_file(Path::new("THUMB.SAV"), vec![], Default::default())
        .unwrap();
    assert!(s.thumbnail(Default::default()).is_err());
    s.replace_thumbnail(None, Default::default()).unwrap();
    assert!(s.file(Path::new("THUMB.SAV")).is_none());
    s.prepare(Default::default()).unwrap();
    let mut s = snapshot();
    s.replace_file(Path::new("SAVEDAT.SAV"), vec![0], Default::default())
        .unwrap();
    assert!(s.prepare(Default::default()).is_err());
    let mut s = snapshot();
    s.remove_file(Path::new("SAVEINFO.SAV"));
    assert_eq!(
        s.prepare(Default::default()).unwrap_err().kind,
        ErrorKind::NotFound
    );
}

#[test]
fn typed_world_names_cannot_overwrite_save_components() {
    let world = world();
    for stem in ["SAVEINFO", "SAVEDAT", "SAVEHDR", "THUMB"] {
        for name in [
            stem.to_owned(),
            format!("{stem}.SAV"),
            format!("{}.zEn", stem.to_ascii_lowercase()),
            format!("{stem}.other"),
        ] {
            let mut snapshot = snapshot();
            let before = snapshot
                .files()
                .map(|file| (file.path.to_owned(), file.bytes.to_vec()))
                .collect::<Vec<_>>();
            let error = snapshot
                .load_world(Path::new(&name), Default::default())
                .unwrap_err();
            assert_eq!(error.kind, ErrorKind::InvalidData, "{name}");
            assert_eq!(error.offset, None);
            assert_eq!(
                snapshot
                    .replace_world(Path::new(&name), &world, Default::default())
                    .unwrap_err()
                    .kind,
                ErrorKind::InvalidData,
                "{name}"
            );
            assert_eq!(
                snapshot
                    .files()
                    .map(|file| (file.path.to_owned(), file.bytes.to_vec()))
                    .collect::<Vec<_>>(),
                before
            );
            assert_eq!(
                snapshot
                    .prepare_active_world(Path::new(&name), &world, Default::default())
                    .unwrap_err()
                    .kind,
                ErrorKind::InvalidData,
                "{name}"
            );
        }
    }
}

#[test]
fn localized_default_world_keeps_every_dot_in_the_metadata_stem() {
    let mut selected = world();
    selected.metadata.user = Some(b"selected dotted world".to_vec());
    let prepared = snapshot()
        .prepare_active_world(Path::new("FIRST.PART.ZEN"), &selected, Default::default())
        .unwrap();
    let snapshot = prepared.snapshot();
    assert!(snapshot.file(Path::new("FIRST.SAV")).is_some());
    assert!(snapshot.file(Path::new("FIRST.PART.SAV")).is_some());
    let loaded = snapshot
        .load_default_world(Default::default())
        .unwrap()
        .unwrap();
    assert_eq!(loaded.metadata.user, selected.metadata.user);
    // Explicit filenames retain their separate extension-replacement contract.
    assert_eq!(
        snapshot
            .load_world(Path::new("FIRST.PART"), Default::default())
            .unwrap()
            .unwrap()
            .metadata
            .user,
        None
    );
    assert_eq!(
        snapshot.file(Path::new("SAVEHDR.SAV")).unwrap(),
        b"FIRST.PART.ZEN\n"
    );
}

#[cfg(any(windows, unix))]
#[test]
fn localized_catalogue_collision_checks_are_independent_of_input_order() {
    #[cfg(windows)]
    let non_unicode = {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(&[b'S' as u16, b'T' as u16, 0xd800])
    };
    #[cfg(unix)]
    let non_unicode = {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(b"ST\xff".to_vec())
    };
    let names = [
        PathBuf::from("\u{df}"),
        PathBuf::from("SS"),
        PathBuf::from(non_unicode),
    ];
    for indices in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        for children in [false, true] {
            let files = indices.map(|i| {
                let name = if children {
                    names[i].join(format!("child{i}"))
                } else {
                    names[i].clone()
                };
                (name, vec![])
            });
            assert_eq!(
                SaveSnapshot::from_files(GameVersion::Gothic2, files, Default::default())
                    .unwrap_err()
                    .kind,
                ErrorKind::InvalidData,
                "{indices:?}, children={children}"
            );
        }
    }
}
#[test]
fn publication_never_overwrites_existing_destinations_or_backup_paths() {
    let temp = Temp::new();
    let d = temp.0.join("destination");
    let stage = temp.0.join("stage");
    let backup = temp.0.join("backup");
    let p = snapshot().prepare(Default::default()).unwrap();
    fs::create_dir(&d).unwrap();
    fs::write(d.join("sentinel"), b"old").unwrap();
    let error = p.publish_new(&d).unwrap_err();
    assert_eq!(error.phase, PublishPhase::Reserve);
    assert_eq!(error.destination_state, DestinationState::Unchanged);
    assert_eq!(fs::read(d.join("sentinel")).unwrap(), b"old");
    fs::create_dir(&backup).unwrap();
    let error = p.replace_exclusive(&d, &stage, &backup).unwrap_err();
    assert_eq!(error.phase, PublishPhase::Preflight);
    assert!(!stage.exists());
    assert_eq!(fs::read(d.join("sentinel")).unwrap(), b"old");
}
#[test]
fn capture_limits_apply_before_large_reads_and_include_empty_directories() {
    let temp = Temp::new();
    fs::write(temp.0.join("a"), [1, 2, 3]).unwrap();
    fs::create_dir(temp.0.join("empty")).unwrap();
    let base = SnapshotLimits::default();
    let exact = SnapshotLimits {
        entries: 2,
        depth: 1,
        name_bytes: 6,
        file_bytes: 3,
        total_bytes: 3,
        ..base
    };
    SaveSnapshot::capture(&temp.0, GameVersion::Gothic1, exact).unwrap();
    for cap in [
        SnapshotLimits {
            entries: 1,
            ..exact
        },
        SnapshotLimits {
            name_bytes: 5,
            ..exact
        },
        SnapshotLimits {
            file_bytes: 2,
            ..exact
        },
        SnapshotLimits {
            total_bytes: 2,
            ..exact
        },
    ] {
        assert_eq!(
            SaveSnapshot::capture(&temp.0, GameVersion::Gothic1, cap)
                .unwrap_err()
                .kind,
            ErrorKind::LimitExceeded
        );
    }
}
