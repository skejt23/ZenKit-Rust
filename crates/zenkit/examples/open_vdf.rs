//! List the files in an ordinary or compressed VDF archive.
use zenkit::vfs::{ReadOptions, Vdf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("usage: open_vdf ARCHIVE")?;
    let archive = Vdf::from_path(path, ReadOptions::default())?;
    for entry in archive.entry_views() {
        if let Some(bytes) = entry.payload() {
            println!(
                "{}: {} bytes",
                String::from_utf8_lossy(entry.name()),
                bytes.len()
            );
        }
    }
    Ok(())
}
