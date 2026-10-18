use zenkit::{
    GameVersion,
    vfs::{
        Compression, InsertOutcome, MountReport, Overwrite, ReadOptions, SaveOptions, Timestamp,
        Vdf, Vfs,
    },
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let time = Timestamp::from_unix_seconds(1_728_907_200);
    let mut source = Vfs::default();
    source.mkdir(b"DATA", Some(time))?;
    let outcome = source.insert_file(
        b"DATA/hello.txt",
        b"owned local data".to_vec(),
        Some(time),
        Overwrite::ReplaceExisting,
    )?;
    assert_eq!(outcome, InsertOutcome::Inserted);
    let subtree = source.resolve(b"DATA").unwrap().clone();
    drop(source);
    let mut files = Vfs::default();
    assert_eq!(
        files.mount_node(subtree, b"", Overwrite::KeepExisting)?,
        MountReport {
            inserted: 2,
            ..MountReport::default()
        }
    );
    let mut options = SaveOptions::new(GameVersion::Gothic2, time);
    options.compression = Compression::Union;
    let archive = Vdf::from_owned(files.to_bytes(options)?, ReadOptions::default())?;
    assert_eq!(
        archive.resolve(b"data/HELLO.TXT").unwrap().payload(),
        Some(&b"owned local data"[..])
    );
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
