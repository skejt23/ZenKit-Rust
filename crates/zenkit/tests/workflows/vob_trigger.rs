use zenkit::{GameVersion, archive::Format, objects::*, vob::*};

fn main() -> zenkit::Result<()> {
    let game = GameVersion::Gothic2;
    let mut graph = Graph::new()?;
    let mut trigger = Trigger {
        target: b"DOOR".to_vec(),
        state: Some(TriggerState::new(game)),
        ..Default::default()
    };
    trigger.flags = TriggerFlags::from_raw(0x80);
    trigger.flags.set_start_enabled(true); // The reserved high bit survives.
    let mut vob = Vob::new(game);
    vob.base.physics_enabled = false;
    vob.base.state = Some(SaveState::default());
    vob.kind = VobKind::TriggerScript(TriggerScript {
        trigger,
        function: b"ON_DOOR".to_vec(),
    });
    let id = graph.insert(Object::Vob(vob))?;
    let ObjectMut::Vob(mut v) = graph.get_mut(id)? else {
        unreachable!()
    };
    v.kind_mut()
        .trigger_mut()
        .unwrap()
        .state
        .as_mut()
        .unwrap()
        .other
        .target = Some(id);

    let mut master = Vob::new(game);
    master.base.physics_enabled = false;
    master.base.state = Some(SaveState::default());
    master.kind = VobKind::CodeMaster(CodeMaster {
        slaves: vec![b"DOOR".to_vec(), b"ALIAS".to_vec()],
        state: Some(CodeMasterState {
            triggered_count: 1,
            triggered: vec![
                Link::new(b"slaveTriggered0".to_vec(), Some(id)),
                Link::new(b"slaveTriggered1".to_vec(), Some(id)),
            ],
        }),
        ..Default::default()
    });
    let master = graph.insert(Object::Vob(master))?;
    graph.add_root(Link::new(b"master".to_vec(), Some(master)))?;
    let mut metadata = Metadata::new(Format::BinSafe);
    metadata.save_game = true;
    let document = ObjectArchive {
        metadata,
        game,
        graph,
    };
    let bytes = document.to_bytes(Limits::default())?;
    let restored = ObjectArchive::from_bytes(&bytes, game, Limits::default())?;
    let ObjectRef::Vob(master) = restored
        .graph
        .get(restored.graph.roots()[0].target.unwrap())?
    else {
        unreachable!()
    };
    let VobKindRef::CodeMaster(master) = &master.kind() else {
        unreachable!()
    };
    let links = &master.state.as_ref().unwrap().triggered;
    assert_eq!(links[0].target, links[1].target);
    let ObjectRef::Vob(trigger) = restored.graph.get(links[0].target.unwrap())? else {
        unreachable!()
    };
    let trigger = trigger.kind().trigger().unwrap();
    assert_eq!(trigger.flags.raw(), 0x81);
    assert_eq!(
        trigger.state.as_ref().unwrap().other.target,
        links[0].target
    );
    println!(
        "Restored a CodeMaster definition, two shared links and a trigger self-reference with its scalar suffix"
    );
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
