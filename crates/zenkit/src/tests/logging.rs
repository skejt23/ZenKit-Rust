use crate as zenkit;
use std::{
    fmt,
    io::{Read, Write},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use zenkit::logging::{self, Level, emit};

struct MustNotFormat;
impl fmt::Display for MustNotFormat {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        panic!("disabled Display invoked")
    }
}
struct DropReentry(Arc<AtomicBool>);
impl Drop for DropReentry {
    fn drop(&mut self) {
        emit(Level::Info, "drop", format_args!("old callback"));
        self.0.store(true, Ordering::SeqCst);
    }
}

// One test owns this process's global hook. The stderr child runs in a separate
// process; its ordinary test invocation below never changes the configuration.
#[test]
fn subscriber_and_emission_contracts() {
    assert!(!logging::enabled(Level::Error));
    emit(Level::Error, "silent", format_args!("{}", MustNotFormat));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let copy = seen.clone();
    logging::set(Level::Info, move |record| {
        copy.lock().unwrap().push((
            record.level,
            record.target.to_owned(),
            record.args.to_string(),
        ))
    });
    for level in [
        Level::Error,
        Level::Warn,
        Level::Info,
        Level::Debug,
        Level::Trace,
    ] {
        assert_eq!(logging::enabled(level), level <= Level::Info);
        emit(level, "levels", format_args!("level {level}"));
    }
    emit(Level::Trace, "filtered", format_args!("{}", MustNotFormat));
    assert_eq!(seen.lock().unwrap().len(), 3);
    let long = "x".repeat(32_768);
    emit(Level::Info, "long", format_args!("{long}"));
    assert_eq!(seen.lock().unwrap()[3].2, long);

    let dropped = Arc::new(AtomicBool::new(false));
    let marker = DropReentry(dropped.clone());
    logging::set(Level::Trace, move |_| {
        let _ = &marker;
    });
    let copy = seen.clone();
    logging::set(Level::Info, move |record| {
        copy.lock().unwrap().push((
            record.level,
            record.target.to_owned(),
            record.args.to_string(),
        ))
    });
    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(seen.lock().unwrap().last().unwrap().1, "drop");

    let nested = Arc::new(AtomicUsize::new(0));
    let copy = nested.clone();
    logging::set(Level::Trace, move |record| {
        copy.fetch_add(1, Ordering::SeqCst);
        if record.target == "outer" {
            emit(Level::Trace, "inner", format_args!("nested"));
        }
    });
    emit(Level::Info, "outer", format_args!("outer"));
    assert_eq!(nested.load(Ordering::SeqCst), 2);

    logging::set(Level::Info, |_| logging::disable());
    emit(Level::Info, "disable", format_args!("reentry"));
    assert!(!logging::enabled(Level::Error));
    logging::set(Level::Info, |_| panic!("deliberate callback panic"));
    assert!(std::panic::catch_unwind(|| emit(Level::Info, "panic", format_args!("test"))).is_err());

    let counts = Arc::new(Mutex::new(vec![0usize; 8]));
    let copy = counts.clone();
    logging::set(Level::Trace, move |record| {
        let index: usize = record.args.to_string().parse().unwrap();
        copy.lock().unwrap()[index] += 1;
    });
    std::thread::scope(|scope| {
        for index in 0..8 {
            scope.spawn(move || {
                for _ in 0..200 {
                    emit(Level::Info, "thread", format_args!("{index}"));
                }
            });
        }
    });
    assert_eq!(*counts.lock().unwrap(), vec![200; 8]);
    // A configuration replacement must not mix a new callback with an old cutoff.
    logging::set(Level::Error, |record| {
        assert_eq!(record.level, Level::Error)
    });
    let barrier = Arc::new(std::sync::Barrier::new(2));
    std::thread::scope(|scope| {
        let copy = barrier.clone();
        scope.spawn(move || {
            copy.wait();
            for _ in 0..2000 {
                logging::set(Level::Error, |record| {
                    assert_eq!(record.level, Level::Error)
                });
                logging::set(Level::Trace, |_| {});
            }
        });
        barrier.wait();
        for _ in 0..4000 {
            emit(Level::Trace, "replace-race", format_args!("trace"));
        }
    });
    logging::disable();
    emit(Level::Error, "disabled", format_args!("{}", MustNotFormat));

    // A running callback owns its snapshot even after another thread disables it.
    let entered = Arc::new(std::sync::Barrier::new(2));
    let released = Arc::new(std::sync::Barrier::new(2));
    let finished = Arc::new(AtomicBool::new(false));
    let (a, b, done) = (entered.clone(), released.clone(), finished.clone());
    logging::set(Level::Info, move |_| {
        a.wait();
        b.wait();
        done.store(true, Ordering::SeqCst);
    });
    std::thread::scope(|scope| {
        scope.spawn(|| emit(Level::Info, "in-flight", format_args!("test")));
        entered.wait();
        logging::disable();
        assert!(!logging::enabled(Level::Error));
        released.wait();
    });
    assert!(finished.load(Ordering::SeqCst));

    // Formatting itself may change configuration without holding a logger lock.
    struct FormatReentry;
    impl fmt::Display for FormatReentry {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            logging::disable();
            f.write_str("formatted")
        }
    }
    logging::set(Level::Info, |record| {
        assert_eq!(record.args.to_string(), "formatted")
    });
    emit(
        Level::Info,
        "format-reentry",
        format_args!("{}", FormatReentry),
    );
    assert!(!logging::enabled(Level::Error));
    logging::set(Level::Info, |record| {
        let _ = record.args.to_string();
    });
    assert!(
        std::panic::catch_unwind(|| emit(
            Level::Info,
            "formatter-panic",
            format_args!("{}", MustNotFormat)
        ))
        .is_err()
    );
    logging::disable();

    workflow_messages();
    default_sink_subprocesses();
}

fn workflow_messages() {
    use zenkit::{
        GameVersion,
        archive::Format,
        model_script::{ModelScript, Recovery, SyntaxMode},
        objects::{Graph, Link, Metadata, Object, ObjectArchive},
        savegame::{SaveMetadata, SaveSnapshot, StateArchive},
        vfs::{ReadOptions, Vdf},
    };
    let messages = Arc::new(Mutex::new(Vec::new()));
    let copy = messages.clone();
    logging::set(Level::Trace, move |r| {
        copy.lock()
            .unwrap()
            .push((r.level, r.target.to_owned(), r.args.to_string()))
    });
    let script = ModelScript::from_mds(
        b"/ comment\nModel(\"M\"){aniEnum{}}",
        SyntaxMode::Legacy,
        Default::default(),
    )
    .unwrap();
    assert_eq!(script.diagnostics.len(), 1);
    assert_eq!(script.diagnostics[0].recovery, Recovery::SingleSlashComment);
    assert_eq!(
        messages.lock().unwrap().as_slice(),
        &[(
            Level::Warn,
            "zenkit::model_script".into(),
            "source recovery at byte 0: SingleSlashComment".into()
        )]
    );
    messages.lock().unwrap().clear();
    let archive = Vdf::from_bytes(
        include_bytes!("../../tests/fixtures/synthetic-vdf-minimal-g2.vdf"),
        ReadOptions::default(),
    )
    .unwrap();
    {
        let seen = messages.lock().unwrap();
        assert_eq!(seen.len(), archive.entries().len() + 1);
        assert_eq!(
            (seen[0].0, seen[0].1.as_str()),
            (Level::Debug, "zenkit::vfs")
        );
        assert!(
            seen[1..]
                .iter()
                .all(|r| r.0 == Level::Trace && r.1 == "zenkit::vfs")
        );
    }
    messages.lock().unwrap().clear();
    let game = GameVersion::Gothic2;
    let mut graph = Graph::new().unwrap();
    let id = graph
        .insert(Object::SaveMetadata(SaveMetadata::new(game)))
        .unwrap();
    graph
        .add_root(Link::new(b"metadata".to_vec(), Some(id)))
        .unwrap();
    let metadata = ObjectArchive {
        metadata: Metadata::new(Format::Ascii),
        game,
        graph,
    };
    let state = StateArchive::new(game, Format::BinSafe).unwrap();
    let snapshot = SaveSnapshot::from_files(
        game,
        [
            (
                "SAVEINFO.SAV".into(),
                metadata.to_bytes(Default::default()).unwrap(),
            ),
            (
                "SAVEDAT.SAV".into(),
                state.to_bytes(Default::default()).unwrap(),
            ),
            (
                "THUMB.SAV".into(),
                include_bytes!("../../tests/fixtures/synthetic-texture-format-0.tex").to_vec(),
            ),
        ],
        Default::default(),
    )
    .unwrap();
    snapshot.metadata(Default::default()).unwrap();
    snapshot.state(Default::default()).unwrap();
    assert!(snapshot.thumbnail(Default::default()).unwrap().is_some());
    assert_eq!(
        *messages.lock().unwrap(),
        ["SAVEINFO.SAV", "SAVEDAT.SAV", "THUMB.SAV"].map(|name| (
            Level::Info,
            "zenkit::savegame".into(),
            format!("loaded {name}")
        ))
    );
    messages.lock().unwrap().clear();
    addon_messages();
    {
        let seen = messages.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(
            seen.iter()
                .all(|r| r.0 == Level::Debug && r.1 == "zenkit::daedalus::addon")
        );
    }
    messages.lock().unwrap().clear();
    assert!(Vdf::from_bytes(b"invalid", ReadOptions::default()).is_err());
    assert!(ModelScript::from_mds(b"/ comment", SyntaxMode::Strict, Default::default()).is_err());
    assert!(messages.lock().unwrap().is_empty());
    logging::disable();
}

fn addon_messages() {
    use zenkit::daedalus::{
        Script,
        addon::{self, FightAi},
        vm,
    };
    // One public source-defined addon class and its six integer slots, no code.
    let mut bytes = vec![50];
    bytes.extend(2u32.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes.extend(1u32.to_le_bytes());
    for (name, properties, parent) in [
        (b"C_FIGHTAI".as_slice(), 4u32 << 12, -1i32),
        (
            b"C_FIGHTAI.MOVE".as_slice(),
            (4u32 << 16) | (2 << 12) | 6,
            0,
        ),
    ] {
        bytes.extend(1u32.to_le_bytes());
        bytes.extend(name);
        bytes.push(b'\n');
        bytes.extend(0u32.to_le_bytes());
        bytes.extend(properties.to_le_bytes());
        bytes.extend([0; 20]);
        if parent == -1 {
            bytes.extend(0u32.to_le_bytes());
        }
        bytes.extend(parent.to_le_bytes());
    }
    bytes.extend(0u32.to_le_bytes());
    let script = Script::from_bytes(&bytes, Default::default()).unwrap();
    let mut vm = vm::Vm::from_script(
        &script,
        Default::default(),
        Default::default(),
        Default::default(),
    )
    .unwrap();
    assert!(
        addon::RegisteredAddon::register::<FightAi>(&mut vm, 1)
            .unwrap()
            .is_some()
    );
    assert_eq!(addon::register_all_present(&mut vm, 1).unwrap().len(), 1);
}

fn default_sink_subprocesses() {
    for closed in [false, true] {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::logging::default_sink_child",
                "--nocapture",
            ])
            .env("ZENKIT_LOGGING_TEST_CHILD", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if closed {
            drop(child.stderr.take());
        }
        // The child cannot log until the parent has closed its pipe reader.
        child.stdin.take().unwrap().write_all(b"!").unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        if !closed {
            assert_eq!(
                String::from_utf8(output.stderr)
                    .unwrap()
                    .replace("\r\n", "\n"),
                "[ZenKit] ERROR consumer: error 17\n[ZenKit] WARN consumer: warning 23\n"
            );
        }
    }
}

#[test]
fn default_sink_child() {
    if std::env::var_os("ZENKIT_LOGGING_TEST_CHILD").is_none() {
        return;
    }
    std::io::stdin().read_exact(&mut [0]).unwrap();
    logging::set_default(Level::Warn);
    emit(Level::Error, "consumer", format_args!("error {}", 17));
    emit(Level::Warn, "consumer", format_args!("warning {}", 23));
    emit(Level::Info, "consumer", format_args!("{}", MustNotFormat));
}
