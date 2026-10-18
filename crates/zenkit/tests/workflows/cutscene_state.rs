//! Build and edit cutscene runtime state with shared graph identity.
use zenkit::{
    GameVersion,
    archive::Format,
    cutscene::{self, Context, Manager, Player, PoolItem, Props},
    objects::*,
};
fn main() -> zenkit::Result<()> {
    let mut graph = Graph::new()?;
    let message = graph.insert(Object::Message(ConversationMessage {
        subtype: 2,
        text: b"Complete dialogue bytes: \xff".to_vec(),
        audio_name: b"VOICE.WAV".to_vec(),
        state: None,
    }))?;
    let atomic = graph.insert(Object::Atomic(AtomicBlock {
        message: Link::new(b"message".to_vec(), Some(message)),
        synchronized: None,
    }))?;
    let props = graph.insert(Object::CutsceneProps(Props {
        name: b"SCENE_PROPERTIES".to_vec(),
        distance: 250.,
        range: 500.,
        ..Default::default()
    }))?;
    let context = graph.insert(Object::CutsceneContext(Context {
        block: CutsceneBlock {
            name: b"CONTEXT".to_vec(),
            sub_block0: 0.375,
            child: Link::new(b"content".to_vec(), Some(atomic)),
        },
        props: Link::new(b"props".to_vec(), Some(props)),
        npc: Link::new(b"npc".to_vec(), None),
        main_role: Link::new(b"mainRole".to_vec(), None),
        alt_stage: Link::new(b"alternate".to_vec(), None),
        stage: Link::new(b"stage".to_vec(), None),
        role_count: -1,
        role_vob_count: 7,
        reference: -3,
        actual_block: 11,
        was_triggered: true,
        ..Default::default()
    }))?;
    let player = graph.insert(Object::CutscenePlayer(Player {
        last_process_day: 3,
        last_process_hour: 17,
        playlists: vec![
            Link::new(b"first".to_vec(), Some(context)),
            Link::new(b"alias".to_vec(), Some(context)),
            Link::new(b"empty".to_vec(), None),
        ],
    }))?;
    if let ObjectMut::CutsceneContext(c) = graph.get_mut(context)? {
        c.alt_stage.target = Some(player);
        c.stage.target = Some(context);
    }
    let scene = graph.insert(Object::Cutscene(CutsceneBlock {
        name: b"SCENE".to_vec(),
        sub_block0: -1.25,
        child: Link::new(b"content".to_vec(), Some(atomic)),
    }))?;
    let pool = graph.insert(Object::CutscenePoolItem(PoolItem {
        name: b"SCENE".to_vec(),
        run_behavior: PoolItem::TIMES,
        run_behavior_value: 3,
        times_played: 1,
        deactivated: -2,
        flags: 17,
    }))?;
    let manager = graph.insert(Object::CutsceneManager(Manager {
        pool_items: vec![Link::new(b"pool".to_vec(), Some(pool))],
    }))?;
    graph.add_root(Link::new(b"player".to_vec(), Some(player)))?;
    graph.add_root(Link::new(b"scene".to_vec(), Some(scene)))?;
    graph.add_root(Link::new(b"manager".to_vec(), Some(manager)))?;
    // Both initial derived roots resolve through the same exact Atomic node.
    assert_eq!(
        cutscene::message_id(&graph, scene)?,
        cutscene::message_id(&graph, context)?
    );
    cutscene::set_message(&mut graph, context, None)?;
    assert_eq!(cutscene::message_id(&graph, scene)?, None);
    cutscene::set_message(&mut graph, scene, Some(message))?;
    let document = ObjectArchive {
        metadata: Metadata::new(Format::BinSafe),
        game: GameVersion::Gothic2,
        graph,
    };
    let bytes = document.to_bytes(Limits::default())?;
    let decoded = ObjectArchive::from_bytes(&bytes, document.game, Limits::default())?;
    let ObjectRef::CutscenePlayer(p) = decoded
        .graph
        .get(decoded.graph.roots()[0].target.unwrap())?
    else {
        unreachable!()
    };
    assert_eq!(p.playlists[0].target, p.playlists[1].target);
    let id = p.playlists[0].target.unwrap();
    let ObjectRef::CutsceneContext(c) = decoded.graph.get(id)? else {
        unreachable!()
    };
    assert_eq!(c.stage.target, Some(id));
    assert_eq!(
        (
            c.role_count,
            c.role_vob_count,
            c.reference,
            c.actual_block,
            c.was_triggered
        ),
        (-1, 7, -3, 11, true)
    );
    assert_eq!(
        cutscene::message(&decoded.graph, id)?.unwrap().text,
        b"Complete dialogue bytes: \xff"
    );
    println!(
        "cutscene state: {} nodes, shared player/context cycle and full trailing fields",
        decoded.graph.len()
    );
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
