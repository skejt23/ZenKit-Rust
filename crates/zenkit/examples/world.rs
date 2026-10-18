//! Load a world and list its VOBs in hierarchy order.
use std::io::Write;
use zenkit::{
    GameVersion,
    objects::{Limits, ObjectArchive},
    world::SectionData,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: world g1|g2 INPUT.zen".into());
    }
    let game = match args[0].to_str() {
        Some("g1") => GameVersion::Gothic1,
        Some("g2") => GameVersion::Gothic2,
        _ => return Err("expected g1 or g2".into()),
    };
    let limits = Limits::default();
    let bytes = zenkit::io::read_bounded(std::fs::File::open(&args[1])?, limits.archive.bytes)?;
    let document = ObjectArchive::from_bytes(&bytes, game, limits)?;
    list_vobs(&document, std::io::stdout().lock())
}

fn list_vobs(
    document: &ObjectArchive,
    mut output: impl Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let [root] = document.graph.roots() else {
        return Err("expected one world root".into());
    };
    let world = document
        .graph
        .world(root.target.ok_or("missing world root")?)?;
    let mut count = 0;
    for (section_index, section) in world.sections.iter().enumerate() {
        let SectionData::VobTree(tree) = &section.data else {
            continue;
        };
        writeln!(output, "VobTree section {section_index}")?;
        for (index, entry) in tree.entries.iter().enumerate() {
            // Parents refer to entry indices within this section; None is a root.
            write!(output, "{index}: parent={:?}", entry.parent)?;
            let Some(id) = entry.object.target else {
                writeln!(output, " <null>")?;
                continue;
            };
            let vob = document.graph.vob(id)?;
            let base = vob.base();
            writeln!(
                output,
                " name={:?} class={} position=({}, {}, {})",
                String::from_utf8_lossy(base.name.as_deref().unwrap_or_default()),
                String::from_utf8_lossy(vob.kind().class_name()),
                base.position[0],
                base.position[1],
                base.position[2],
            )?;
            count += 1;
        }
    }
    writeln!(output, "{count} VOB occurrences")?;
    Ok(())
}

#[cfg(test)]
#[path = "../tests/world_example.inc"]
mod tests;
