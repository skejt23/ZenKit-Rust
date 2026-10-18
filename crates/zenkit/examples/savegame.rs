//! Advance a save's day counter and publish it to a new directory.
use std::path::Path;
use zenkit::{
    GameVersion,
    objects::ObjectMut,
    savegame::{SaveSnapshot, SnapshotLimits},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: savegame g1|g2 INPUT_DIRECTORY NEW_DIRECTORY".into());
    }
    let game = match args[0].to_str() {
        Some("g1") => GameVersion::Gothic1,
        Some("g2") => GameVersion::Gothic2,
        _ => return Err("expected g1 or g2".into()),
    };
    let limits = SnapshotLimits::default();
    let mut save = SaveSnapshot::capture(Path::new(&args[1]), game, limits)?;
    let mut state = save.state(limits.objects)?;
    state.fields.day = state
        .fields
        .day
        .checked_add(1)
        .ok_or("day counter overflow")?;
    let mut metadata = save.metadata(limits.objects)?;
    let id = metadata.graph.roots()[0]
        .target
        .ok_or("missing save metadata")?;
    let ObjectMut::SaveMetadata(info) = metadata.graph.get_mut(id)? else {
        return Err("expected save metadata".into());
    };
    info.day = state.fields.day;
    save.replace_state(&state, limits)?;
    save.replace_metadata(&metadata, limits)?;
    save.prepare(limits)?.publish_new(Path::new(&args[2]))?;
    println!("Published save for day {}", state.fields.day);
    Ok(())
}
