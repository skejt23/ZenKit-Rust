use zenkit::{GameVersion, archive::Format, objects::*};
fn main() -> zenkit::Result<()> {
    let mut graph = Graph::new()?;
    let root = graph.insert(Object::Block(CutsceneBlock {
        name: b"cycle".to_vec(),
        ..Default::default()
    }))?;
    if let ObjectMut::Block(block) = graph.get_mut(root)? {
        block.child.target = Some(root);
    }
    graph.add_root(Link::new(b"root".to_vec(), Some(root)))?;
    graph.add_root(Link::new(b"alias".to_vec(), Some(root)))?;
    let document = ObjectArchive {
        metadata: Metadata::new(Format::BinSafe),
        game: GameVersion::Gothic2,
        graph,
    };
    let bytes = document.to_bytes(Limits::default())?;
    let restored = ObjectArchive::from_bytes(&bytes, GameVersion::Gothic2, Limits::default())?;
    assert_eq!(restored.graph.len(), 1);
    assert_eq!(
        restored.graph.roots()[0].target,
        restored.graph.roots()[1].target
    );
    println!(
        "{} owned node, {} roots, {} bytes",
        restored.graph.len(),
        restored.graph.roots().len(),
        bytes.len()
    );
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
