//! Add a file to an archive and write a compressed copy.
use zenkit::{
    GameVersion,
    vfs::{Compression, Overwrite, SaveOptions, Timestamp, Vfs},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: compose_vfs INPUT.vdf NOTE.txt OUTPUT.vdf".into());
    }
    let mut files = Vfs::default();
    files.mount_archive_path(&args[0], b"", Overwrite::KeepExisting)?;
    let note = zenkit::io::read_bounded(std::fs::File::open(&args[1])?, 1024 * 1024)?;
    files.insert_file(b"NOTE.TXT", note, None, Overwrite::ReplaceExisting)?;

    let timestamp = Timestamp::from_system_time(std::time::SystemTime::now())?;
    let mut options = SaveOptions::new(GameVersion::Gothic2, timestamp);
    options.compression = Compression::Union;
    std::fs::write(&args[2], files.to_bytes(options)?)?;
    Ok(())
}
