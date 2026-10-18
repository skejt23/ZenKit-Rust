//! Store an editable object archive in an ordinary or Union VDF, then reopen it.
use zenkit::{
    GameVersion,
    archive::Format,
    material::Material,
    objects::{Graph, Limits, Link, Metadata, Object, ObjectArchive},
    vfs::{Compression, Overwrite, ReadOptions, SaveOptions, Timestamp, Vdf, Vfs},
};

fn read_document(archive: &Vdf, game: GameVersion) -> zenkit::Result<ObjectArchive> {
    let entry = archive
        .resolve(b"data/MATERIAL.ZEN")
        .expect("inserted file");
    ObjectArchive::from_bytes(
        entry.payload().expect("file payload"),
        game,
        Limits::default(),
    )
}

fn main() -> zenkit::Result<()> {
    let time = Timestamp::from_unix_seconds(1_728_907_200);
    let mut completed = 0;
    for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
        for format in [Format::Ascii, Format::Binary, Format::BinSafe] {
            for compression in [Compression::None, Compression::Union] {
                let mut graph = Graph::new()?;
                let mut material = Material::new(game);
                material.name = b"EXAMPLE".to_vec();
                material.texture = b"ORIGINAL.TGA".to_vec();
                let id = graph.insert(Object::Material(material))?;
                graph.add_root(Link::new(b"material".to_vec(), Some(id)))?;
                graph.add_root(Link::new(b"same-material".to_vec(), Some(id)))?;
                let document = ObjectArchive {
                    metadata: Metadata::new(format),
                    game,
                    graph,
                };

                let mut files = Vfs::default();
                files.mkdir(b"DATA", Some(time))?;
                files.insert_file(
                    b"DATA/material.zen",
                    document.to_bytes(Limits::default())?,
                    Some(time),
                    Overwrite::ReplaceExisting,
                )?;
                let mut options = SaveOptions::new(game, time);
                options.compression = compression;
                let archive =
                    Vdf::from_owned(files.to_bytes(options.clone())?, ReadOptions::default())?;
                let mut reopened = read_document(&archive, game)?;
                drop(archive); // The parsed graph owns its values and identities.
                assert_eq!(reopened.graph.len(), 1);
                assert_eq!(
                    reopened.graph.roots()[0].target,
                    reopened.graph.roots()[1].target
                );
                let parsed_id = reopened.graph.roots()[0].target.expect("material target");
                let material = reopened.graph.material_mut(parsed_id)?;
                assert_eq!(material.texture, b"ORIGINAL.TGA");
                material.texture = b"EDITED.TGA".to_vec();
                files.insert_file(
                    b"DATA/material.zen",
                    reopened.to_bytes(Limits::default())?,
                    Some(time),
                    Overwrite::ReplaceExisting,
                )?;
                let edited = Vdf::from_owned(files.to_bytes(options)?, ReadOptions::default())?;
                let restored = read_document(&edited, game)?;
                let id = restored.graph.roots()[0].target.expect("material target");
                let material = restored.graph.material(id)?;
                assert_eq!(material.texture, b"EDITED.TGA");
                assert_eq!(
                    restored.graph.roots()[0].target,
                    restored.graph.roots()[1].target
                );
                completed += 1;
            }
        }
    }
    println!("{completed} archive/object/VDF edit-and-reopen workflows passed");
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
