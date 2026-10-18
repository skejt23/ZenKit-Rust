//! Build, publish and edit a save in a private temporary directory.
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use zenkit::{
    GameVersion,
    archive::Format,
    objects::{Graph, Link, Metadata, Object, ObjectArchive},
    savegame::{Info, SaveMetadata, SaveSnapshot, SnapshotLimits, StateArchive},
    world::{Section, SectionData, SectionFrame, World},
};

struct TemporarySave(PathBuf);
impl TemporarySave {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = std::env::temp_dir().canonicalize()?;
        loop {
            let path = parent.join(format!(
                "zenkit-save-example-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
    }
}
impl Drop for TemporarySave {
    fn drop(&mut self) {
        // Only this invocation's exclusively created directory is removed.
        if let Ok(parent) = std::env::temp_dir().canonicalize()
            && self.0.is_absolute()
            && self.0.parent() == Some(parent.as_path())
        {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let game = GameVersion::Gothic2;
    let limits = SnapshotLimits::default();

    let mut graph = Graph::new()?;
    let mut info = SaveMetadata::new(game);
    info.title = b"Example save".to_vec();
    info.world = b"EXAMPLE".to_vec();
    let info = graph.insert(Object::SaveMetadata(info))?;
    graph.add_root(Link::new(b"metadata".to_vec(), Some(info)))?;
    let metadata = ObjectArchive {
        metadata: Metadata::new(Format::Ascii),
        game,
        graph,
    };

    // SAVEDAT owns one shared graph. A null manager is a valid empty initial state.
    let mut state = StateArchive::new(game, Format::BinSafe)?;
    state.fields.day = 3;
    state.fields.infos.push(Info {
        name: b"DIA_EXAMPLE".to_vec(),
        told: true,
    });

    let mut world_graph = Graph::new()?;
    let mut world = World::new(game, true);
    world.sections.push(Section {
        frame: SectionFrame::default(),
        data: SectionData::End,
    });
    let world = world_graph.insert(Object::World(world))?;
    world_graph.add_root(Link::new(b"world".to_vec(), Some(world)))?;
    let mut world_metadata = Metadata::new(Format::BinSafe);
    world_metadata.save_game = true;
    let world = ObjectArchive {
        metadata: world_metadata,
        game,
        graph: world_graph,
    };

    let snapshot = SaveSnapshot::from_files(
        game,
        [
            (
                PathBuf::from("SAVEINFO.SAV"),
                metadata.to_bytes(limits.objects)?,
            ),
            (
                PathBuf::from("SAVEDAT.SAV"),
                state.to_bytes(limits.objects)?,
            ),
            (PathBuf::from("custom/note.bin"), vec![0, 255, 7]),
        ],
        limits,
    )?;
    let prepared = snapshot.prepare_active_world(Path::new("EXAMPLE.ZEN"), &world, limits)?;
    let temporary = TemporarySave::new()?;
    let destination = temporary.0.join("save");
    prepared.publish_new(&destination)?;

    let mut reopened = SaveSnapshot::capture(&destination, game, limits)?;
    assert!(reopened.load_default_world(limits.objects)?.is_some());
    assert!(reopened.thumbnail(limits.texture)?.is_none());
    let mut edited = reopened.state(limits.objects)?;
    edited.fields.day += 1;
    reopened.replace_state(&edited, limits)?;
    let prepared = reopened.prepare(limits)?;
    let published = prepared.replace_exclusive(
        &destination,
        &temporary.0.join("staging"),
        &temporary.0.join("backup"),
    )?;
    assert!(published.backup.is_some());
    let final_save = SaveSnapshot::capture(&destination, game, limits)?;
    assert_eq!(final_save.state(limits.objects)?.fields.day, 4);
    assert_eq!(
        final_save.file(Path::new("custom/note.bin")),
        Some([0, 255, 7].as_slice())
    );
    println!("Created, reopened and edited a save; unrelated bytes and the backup were preserved.");
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
