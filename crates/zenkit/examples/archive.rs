//! Inspect root object headers without loading their class schemas.
use zenkit::archive::{Limits, Reader, SkipLimits};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("usage: archive INPUT.zen")?;
    let limits = Limits::default();
    let bytes = zenkit::io::read_bounded(std::fs::File::open(path)?, limits.bytes)?;
    let mut reader = Reader::new(&bytes, limits)?;
    println!(
        "{:?}, save game: {}",
        reader.header().format,
        reader.header().save_game
    );
    while let Some(object) = reader.skip_next_object(SkipLimits::default())? {
        println!(
            "{}: {}",
            String::from_utf8_lossy(object.object_name),
            String::from_utf8_lossy(object.class_name)
        );
    }
    reader.finish()?;
    Ok(())
}
