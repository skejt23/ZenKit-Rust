use std::{
    borrow::Cow,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use zenkit::objects::ObjectRef;
use zenkit::{
    Error, ErrorKind, GameVersion, Result,
    archive::Format,
    io::{HostNameCodec, HostNameEncoding},
    objects::{Graph, Link, Metadata, Object, ObjectArchive},
    savegame::{SaveMetadata, SaveSnapshot, SnapshotLimits, StateArchive},
    vfs::{Compression, HostOptions, Overwrite, SaveOptions, Timestamp, Vdf, Vfs},
    world::{Section, SectionData, SectionFrame, World},
};

fn err(kind: ErrorKind) -> Error {
    Error::operation(kind, "test name codec")
}

// Test-only application codec. Tables are generated independently by Python's
// strict standard codecs; production ZenKit does not choose either code page.
struct CodePage(&'static [u8; 1024]);
const CP1250: CodePage = CodePage(include_bytes!(
    "../tests/fixtures/synthetic-host-name-cp1250.bin"
));
const CP1252: CodePage = CodePage(include_bytes!(
    "../tests/fixtures/synthetic-host-name-cp1252.bin"
));
impl CodePage {
    fn point(&self, byte: u8) -> u32 {
        let offset = usize::from(byte) * 4;
        u32::from_le_bytes(self.0[offset..offset + 4].try_into().unwrap())
    }
}
impl HostNameCodec for CodePage {
    fn encode<'a>(&self, name: &'a OsStr, cap: usize) -> Result<Cow<'a, [u8]>> {
        let name = name.to_str().ok_or_else(|| err(ErrorKind::Unsupported))?;
        let mut bytes = Vec::new();
        for ch in name.chars() {
            if bytes.len() == cap {
                return Err(err(ErrorKind::LimitExceeded));
            }
            let byte = (0..=255u8)
                .find(|b| self.point(*b) == u32::from(ch))
                .ok_or_else(|| err(ErrorKind::Unsupported))?;
            bytes
                .try_reserve(1)
                .map_err(|_| err(ErrorKind::LimitExceeded))?;
            bytes.push(byte);
        }
        Ok(Cow::Owned(bytes))
    }
    fn decode<'a>(&self, name: &'a [u8], cap: usize) -> Result<Cow<'a, OsStr>> {
        let mut text = String::new();
        for &byte in name {
            let ch = char::from_u32(self.point(byte)).ok_or_else(|| err(ErrorKind::Unsupported))?;
            if ch.len_utf8() > cap.saturating_sub(text.len()) {
                return Err(err(ErrorKind::LimitExceeded));
            }
            text.try_reserve(ch.len_utf8())
                .map_err(|_| err(ErrorKind::LimitExceeded))?;
            text.push(ch);
        }
        Ok(Cow::Owned(text.into()))
    }
}
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static ID: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().canonicalize().unwrap();
        let path = root.join(format!(
            "zenkit-host-names-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let root = std::env::temp_dir().canonicalize().unwrap();
        assert!(self.0.is_absolute() && self.0.starts_with(root));
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn world(game: GameVersion, format: Format) -> ObjectArchive {
    let mut out = ObjectArchive {
        game,
        metadata: Metadata::new(format),
        graph: Graph::new().unwrap(),
    };
    out.metadata.save_game = true;
    let mut world = World::new(game, true);
    world.sections.push(Section {
        frame: SectionFrame::default(),
        data: SectionData::End,
    });
    let id = out.graph.insert(Object::World(world)).unwrap();
    out.graph
        .add_root(Link::new(b"world".to_vec(), Some(id)))
        .unwrap();
    out
}
fn snapshot(game: GameVersion, format: Format, raw: &[u8]) -> SaveSnapshot {
    let mut d = ObjectArchive {
        game,
        metadata: Metadata::new(format),
        graph: Graph::new().unwrap(),
    };
    let mut info = SaveMetadata::new(game);
    info.world = raw.to_vec();
    info.title = b"raw title\xff".to_vec();
    let id = d.graph.insert(Object::SaveMetadata(info)).unwrap();
    d.graph
        .add_root(Link::new(b"info".to_vec(), Some(id)))
        .unwrap();
    SaveSnapshot::from_files(
        game,
        [
            (
                PathBuf::from("SaVeInFo.SaV"),
                d.to_bytes(Default::default()).unwrap(),
            ),
            (
                PathBuf::from("SAVEDAT.SAV"),
                StateArchive::new(game, format)
                    .unwrap()
                    .to_bytes(Default::default())
                    .unwrap(),
            ),
            (PathBuf::from("opaque/data"), vec![0, 0xff, 0x81, 9]),
        ],
        Default::default(),
    )
    .unwrap()
}
fn metadata_world(snapshot: &SaveSnapshot) -> Vec<u8> {
    let d = snapshot.metadata(Default::default()).unwrap();
    let ObjectRef::SaveMetadata(info) = d.graph.get(d.graph.roots()[0].target.unwrap()).unwrap()
    else {
        panic!()
    };
    assert_eq!(info.title, b"raw title\xff");
    info.world.clone()
}

#[test]
fn builtins_borrow_and_require_explicit_encoding() {
    let utf8 = OsStr::new("caf\u{e9}");
    let raw = "caf\u{e9}".as_bytes();
    assert!(matches!(
        HostNameEncoding::Utf8.encode(utf8, raw.len()).unwrap(),
        Cow::Borrowed(_)
    ));
    assert!(matches!(
        HostNameEncoding::Utf8.decode(raw, raw.len()).unwrap(),
        Cow::Borrowed(_)
    ));
    assert_eq!(
        HostNameEncoding::Utf8
            .encode(utf8, raw.len() - 1)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(
        HostNameEncoding::Utf8
            .decode(raw, raw.len() - 1)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(
        HostNameEncoding::Ascii.encode(utf8, 100).unwrap_err().kind,
        ErrorKind::Unsupported
    );
    assert_eq!(
        HostNameEncoding::Utf8
            .decode(b"caf\xe9", 100)
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
    assert_eq!(
        HostNameEncoding::Ascii
            .decode(b"caf\xe9", 100)
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
    assert!(matches!(
        HostNameEncoding::Ascii
            .encode(OsStr::new("NAME"), 4)
            .unwrap(),
        Cow::Borrowed(_)
    ));
    assert_eq!(
        CP1252
            .encode(OsStr::new("Gr\u{fc}\u{df}e.W\u{c4}LD"), 100)
            .unwrap()
            .as_ref(),
        b"Gr\xfc\xdfe.W\xc4LD"
    );
    assert_eq!(
        CP1250
            .encode(OsStr::new("\u{15a}WIAT.\u{17b}\u{d3}\u{141}\u{106}"), 100)
            .unwrap()
            .as_ref(),
        b"\x8cWIAT.\xaf\xd3\xa3\xc6"
    );
    assert_eq!(
        CP1252.encode(OsStr::new("\u{141}"), 100).unwrap_err().kind,
        ErrorKind::Unsupported
    );
    assert_eq!(
        CP1252.decode(&[0x81], 100).unwrap_err().kind,
        ErrorKind::Unsupported
    );
}

#[test]
fn localized_save_selection_publication_and_reload_preserve_raw_stems() {
    for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
        for format in [Format::Ascii, Format::Binary, Format::BinSafe] {
            for (codec, stem, raw) in [
                (
                    &CP1250 as &dyn HostNameCodec,
                    "\u{15a}WIAT.\u{17b}\u{d3}\u{141}\u{106}",
                    b"\x8cWIAT.\xaf\xd3\xa3\xc6".as_slice(),
                ),
                (
                    &CP1252,
                    "Gr\u{fc}\u{df}e.W\u{c4}LD",
                    b"Gr\xfc\xdfe.W\xc4LD".as_slice(),
                ),
            ] {
                let mut save = snapshot(game, format, raw);
                let world = world(game, format);
                save.replace_world(Path::new("OTHER"), &world, Default::default())
                    .unwrap();
                let previous = save.file(Path::new("OTHER.SAV")).unwrap().to_vec();
                let prepared = save
                    .prepare_active_world_with_names(
                        Path::new(&format!("{stem}.ZEN")),
                        &world,
                        codec,
                        Default::default(),
                    )
                    .unwrap();
                let save = prepared.snapshot();
                assert_eq!(metadata_world(save), raw);
                assert_eq!(
                    save.file(Path::new("SAVEHDR.SAV")).unwrap(),
                    [raw, b".ZEN\n"].concat()
                );
                assert_eq!(save.file(Path::new("OTHER.SAV")).unwrap(), previous);
                assert_eq!(
                    save.file(Path::new("opaque/data")).unwrap(),
                    [0, 0xff, 0x81, 9]
                );
                assert!(
                    save.load_default_world_with_names(codec, Default::default())
                        .unwrap()
                        .is_some()
                );
                assert_eq!(
                    save.load_default_world(Default::default())
                        .unwrap_err()
                        .kind,
                    ErrorKind::Unsupported
                );
                assert!(save.files().any(|f| f.path == Path::new("SaVeInFo.SaV")));
                let temp = Temp::new();
                let destination = temp.0.join("published");
                prepared.publish_new(&destination).unwrap();
                let recaptured =
                    SaveSnapshot::capture(&destination, game, Default::default()).unwrap();
                assert!(
                    recaptured
                        .load_default_world_with_names(codec, Default::default())
                        .unwrap()
                        .is_some()
                );
                assert_eq!(metadata_world(&recaptured), raw);
                assert!(destination.join(format!("{stem}.SAV")).is_file());
            }
        }
    }
    for stem in ["WORLD.PART", "WORLD.ZEN", "WORLD."] {
        let prepared = snapshot(GameVersion::Gothic2, Format::BinSafe, b"ORIGINAL")
            .prepare_active_world(
                Path::new(&format!("{stem}.ZEN")),
                &world(GameVersion::Gothic2, Format::BinSafe),
                Default::default(),
            )
            .unwrap();
        assert_eq!(metadata_world(prepared.snapshot()), stem.as_bytes());
        assert!(
            prepared
                .snapshot()
                .load_default_world(Default::default())
                .unwrap()
                .is_some()
        );
    }
}

#[test]
fn legacy_host_mounts_keep_high_bytes_and_roundtrip_both_vdf_outputs() {
    let temp = Temp::new();
    let host = "\u{141}\u{d3}D\u{179}.BIN";
    fs::write(temp.0.join(host), [0, 255, 3, 7]).unwrap();
    fs::create_dir(temp.0.join("Gr\u{fc}\u{df}e")).unwrap();
    fs::write(temp.0.join("Gr\u{fc}\u{df}e/inner"), [2]).unwrap();
    let mut v = Vfs::default();
    assert!(v.mount_host(&temp.0, b"", HostOptions::default()).is_err());
    v.mount_host_with_names(&temp.0, b"", Overwrite::KeepExisting, &CP1250)
        .unwrap();
    let raw = b"\xa3\xd3D\x8f.BIN";
    assert_eq!(v.resolve(raw).unwrap().payload().unwrap(), [0, 255, 3, 7]);
    assert!(v.resolve(b"\xb3\xf3d\x9f.bin").is_none()); // high bytes never case-fold
    assert_eq!(
        v.resolve(b"\xa3\xd3d\x8f.bin").unwrap().payload().unwrap(),
        [0, 255, 3, 7]
    );
    for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
        for compression in [Compression::None, Compression::Union] {
            let bytes = v
                .to_bytes(SaveOptions {
                    compression,
                    ..SaveOptions::new(game, Timestamp::from_unix_seconds(1_704_164_646))
                })
                .unwrap();
            let archive = Vdf::from_bytes(&bytes, Default::default()).unwrap();
            assert_eq!(
                archive.resolve(raw).unwrap().payload().unwrap(),
                [0, 255, 3, 7]
            );
        }
    }
}

// Map valid host names to invalid archive names to exercise post-conversion checks.
struct Pair {
    host: OsString,
    raw: Vec<u8>,
}
impl HostNameCodec for Pair {
    fn encode<'a>(&self, name: &'a OsStr, _: usize) -> Result<Cow<'a, [u8]>> {
        if name == self.host {
            Ok(Cow::Owned(self.raw.clone()))
        } else {
            HostNameEncoding::Utf8.encode(name, usize::MAX)
        }
    }
    fn decode<'a>(&self, name: &'a [u8], _: usize) -> Result<Cow<'a, OsStr>> {
        if name == self.raw {
            Ok(Cow::Owned(self.host.clone()))
        } else {
            HostNameEncoding::Utf8.decode(name, usize::MAX)
        }
    }
}

#[test]
fn encoded_collisions_invalid_names_limits_and_inverse_failures_never_mount() {
    let temp = Temp::new();
    fs::write(temp.0.join("original"), [1]).unwrap();
    let mut v = Vfs::default();
    v.insert_file(b"keep", vec![9], None, Overwrite::KeepExisting)
        .unwrap();
    for raw in [
        vec![b'x'; 65],
        b"bad/name".to_vec(),
        vec![0],
        b".".to_vec(),
        b"..".to_vec(),
    ] {
        let codec = Pair {
            host: "original".into(),
            raw,
        };
        assert!(
            v.mount_host_with_names(&temp.0, b"", Overwrite::KeepExisting, &codec)
                .is_err()
        );
        assert_eq!(v.root().children().count(), 1);
        assert_eq!(v.resolve(b"keep").unwrap().payload().unwrap(), [9]);
    }
    fs::write(temp.0.join("KEEP"), [2]).unwrap();
    let codec = Pair {
        host: "original".into(),
        raw: b"keep ".to_vec(),
    };
    assert_eq!(
        v.mount_host_with_names(&temp.0, b"", Overwrite::KeepExisting, &codec)
            .unwrap_err()
            .kind,
        ErrorKind::InvalidData
    );
    assert_eq!(v.root().children().count(), 1);
    struct Lossy;
    impl HostNameCodec for Lossy {
        fn encode<'a>(&self, _: &'a OsStr, _: usize) -> Result<Cow<'a, [u8]>> {
            Ok(Cow::Borrowed(b"substitute"))
        }
        fn decode<'a>(&self, _: &'a [u8], _: usize) -> Result<Cow<'a, OsStr>> {
            Ok(Cow::Borrowed(OsStr::new("bad")))
        }
    }
    assert_eq!(
        v.mount_host_with_names(&temp.0, b"", Overwrite::ReplaceExisting, &Lossy)
            .unwrap_err()
            .kind,
        ErrorKind::InvalidData
    );
    assert_eq!(v.root().children().count(), 1);
}

#[test]
fn mapped_world_names_reject_paths_devices_limits_and_noncomposable_suffixes() {
    for name in [
        "../escape",
        "dir/name",
        "C:drive",
        "CON",
        "SAVEINFO",
        ".",
        "..",
        "trailing ",
        "a\\b",
    ] {
        let codec = Pair {
            host: name.into(),
            raw: b"RAW".to_vec(),
        };
        let save = snapshot(GameVersion::Gothic2, Format::BinSafe, b"RAW");
        assert!(
            save.load_default_world_with_names(&codec, Default::default())
                .is_err(),
            "{name}"
        );
    }
    let save = snapshot(GameVersion::Gothic2, Format::BinSafe, b"RAW");
    let codec = Pair {
        host: "expanded".into(),
        raw: b"RAW".to_vec(),
    };
    assert_eq!(
        save.load_default_world_with_names(
            &codec,
            SnapshotLimits {
                name_bytes: 11,
                ..Default::default()
            }
        )
        .unwrap_err()
        .kind,
        ErrorKind::LimitExceeded
    );
    // The stem round-trips, but this codec leaves full filenames in UTF-8.
    let codec = Pair {
        host: "NEW".into(),
        raw: b"RAW".to_vec(),
    };
    assert_eq!(
        save.load_default_world_with_names(&codec, Default::default())
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
    for raw in [b"RAW\0".as_slice(), b"RAW\n", b"RAW\r"] {
        let codec = Pair {
            host: "HOST".into(),
            raw: raw.to_vec(),
        };
        assert_eq!(
            snapshot(GameVersion::Gothic2, Format::BinSafe, b"ORIGINAL")
                .prepare_active_world_with_names(
                    Path::new("HOST.ZEN"),
                    &world(GameVersion::Gothic2, Format::BinSafe),
                    &codec,
                    Default::default()
                )
                .unwrap_err()
                .kind,
            ErrorKind::InvalidData
        );
    }
}
