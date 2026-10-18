use zenkit::{
    GameVersion,
    archive::Format,
    cutscene::{self, Library},
    objects::*,
};
fn main() -> zenkit::Result<()> {
    let mut graph = Graph::new()?;
    let message = graph.insert(Object::Message(ConversationMessage {
        subtype: 0,
        text: b"The complete line, including raw bytes: \xff".to_vec(),
        audio_name: b"LINE.WAV".to_vec(),
        state: None,
    }))?;
    let atomic = graph.insert(Object::Atomic(AtomicBlock {
        message: Link::new(b"message".to_vec(), Some(message)),
        synchronized: None,
    }))?;
    let block = graph.insert(Object::Block(CutsceneBlock {
        name: b"DIA_EXAMPLE".to_vec(),
        sub_block0: 0.,
        child: Link::new(b"child".to_vec(), Some(atomic)),
    }))?;
    let library = graph.insert(Object::Library(Library {
        blocks: vec![
            Link::new(b"first".to_vec(), Some(block)),
            Link::new(b"alias".to_vec(), Some(block)),
        ],
    }))?;
    graph.add_root(Link::new(b"library".to_vec(), Some(library)))?;
    let doc = ObjectArchive {
        metadata: Metadata::new(Format::BinSafe),
        game: GameVersion::Gothic2,
        graph,
    };
    let bytes = doc.to_bytes(Limits::default())?;
    let mut restored = ObjectArchive::from_bytes(&bytes, doc.game, Limits::default())?;
    let root = restored.graph.roots()[0].target.expect("library root");
    let ObjectRef::Library(library) = restored.graph.get(root)? else {
        unreachable!()
    };
    let block = library
        .block_by_name(&restored.graph, b"DIA_EXAMPLE")?
        .expect("exact name");
    assert_eq!(library.block_by_name(&restored.graph, b"DIA_EXAMPL")?, None);
    assert_eq!(library.blocks[0].target, library.blocks[1].target);
    assert!(cutscene::message(&restored.graph, block)?.is_some());
    cutscene::set_message(&mut restored.graph, block, None)?;
    assert_eq!(cutscene::message(&restored.graph, block)?, None);
    restored.to_bytes(Limits::default())?;
    println!("Cutscene create/read/lookup/shared mutation/write workflow passed");
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
