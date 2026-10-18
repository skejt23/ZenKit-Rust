//! Change NPC guilds in a world or object archive and save the result.
use zenkit::{
    GameVersion,
    objects::{Limits, ObjectArchive, ObjectRef},
    vob::VobKindRef,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 4 {
        return Err("usage: object_editing g1|g2 INPUT.zen OUTPUT.zen GUILD".into());
    }
    let game = match args[0].to_str() {
        Some("g1") => GameVersion::Gothic1,
        Some("g2") => GameVersion::Gothic2,
        _ => return Err("expected g1 or g2".into()),
    };
    let guild: i32 = args[3].to_str().ok_or("invalid guild")?.parse()?;
    let limits = Limits::default();
    let bytes = zenkit::io::read_bounded(std::fs::File::open(&args[1])?, limits.archive.bytes)?;
    let mut document = ObjectArchive::from_bytes(&bytes, game, limits)?;
    let npcs: Vec<_> = document
        .graph
        .objects()
        .filter_map(|(id, object)| match object {
            ObjectRef::Vob(vob) if matches!(vob.kind(), VobKindRef::Npc(_)) => Some(id),
            _ => None,
        })
        .collect();
    for id in &npcs {
        document.graph.npc_mut(*id)?.guild = guild;
    }
    std::fs::write(&args[2], document.to_bytes(limits)?)?;
    println!("Updated {} NPCs", npcs.len());
    Ok(())
}
