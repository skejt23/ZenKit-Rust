use zenkit::{GameVersion, archive::Format, objects::*, vob::*};

fn main() -> zenkit::Result<()> {
    let game = GameVersion::Gothic2;
    let mut graph = Graph::new()?;
    let mut vob = Vob::new(game);
    vob.base.name = Some(b"MOVING_STONE".to_vec());
    vob.base.visual_name = Some(b"STONE.3DS".to_vec());
    vob.base.state = Some(SaveState {
        rigid_body: Some(RigidBody::default()),
        ..Default::default()
    });
    let id = graph.insert(Object::Vob(vob))?;
    let visual = graph.insert(Object::Visual(Visual::new(VisualKind::Mesh)))?;
    let ai = graph.insert(Object::AiMove(AiMove {
        vob: Link::new(b"vob".to_vec(), Some(id)),
        owner: Link::new(b"owner".to_vec(), None),
    }))?;
    if let ObjectMut::Vob(mut vob) = graph.get_mut(id)? {
        vob.base_mut().visual = Some(Link::new(b"visual".to_vec(), Some(visual)));
        vob.base_mut().ai = Some(Link::new(b"ai".to_vec(), Some(ai)));
    }
    graph.add_root(Link::new(b"root".to_vec(), Some(id)))?;
    let mut metadata = Metadata::new(Format::BinSafe);
    metadata.save_game = true;
    let document = ObjectArchive {
        metadata,
        game,
        graph,
    };
    let bytes = document.to_bytes(Limits::default())?;
    let restored = ObjectArchive::from_bytes(&bytes, game, Limits::default())?;
    let root = restored.graph.roots()[0].target.unwrap();
    let ObjectRef::Vob(vob) = restored.graph.get(root)? else {
        unreachable!()
    };
    let ObjectRef::AiMove(ai) = restored
        .graph
        .get(vob.base().ai.as_ref().unwrap().target.unwrap())?
    else {
        unreachable!()
    };
    assert_eq!(ai.vob.target, Some(root));
    assert!(vob.base().state.unwrap().rigid_body.is_some());
    println!(
        "Restored {} objects with shared identity and a save-state suffix",
        restored.graph.len()
    );
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
