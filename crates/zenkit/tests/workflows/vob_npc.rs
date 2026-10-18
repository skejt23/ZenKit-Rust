//! Construct an NPC inventory with occurrence-local shortcuts and an AI cycle.
use zenkit::{GameVersion, archive::Format, objects::*, vob::*};
fn main() -> zenkit::Result<()> {
    let game = GameVersion::Gothic2;
    let mut graph = Graph::new()?;
    let mut npc = Vob::new(game);
    npc.base.physics_enabled = false;
    npc.base.state = Some(SaveState::default());
    npc.kind = VobKind::Npc(NpcData::new(game));
    let npc_id = graph.insert(Object::Vob(npc))?;
    let talent = graph.insert(Object::NpcTalent(NpcTalent {
        talent: 1,
        value: 25,
        skill: 2,
    }))?;
    let ai = graph.insert(Object::AiMove(AiMove {
        vob: Link::new(b"vob".to_vec(), Some(npc_id)),
        owner: Link::new(b"owner".to_vec(), Some(npc_id)),
    }))?;
    let mut item = Vob::new(game);
    item.base.physics_enabled = false;
    item.base.state = Some(SaveState::default());
    item.base.ai = Some(Link::new(b"ai".to_vec(), Some(ai)));
    item.kind = VobKind::Item(Item {
        instance: b"ITMI_EXAMPLE".to_vec(),
        state: Some(ItemState {
            amount: 3,
            flags: 0x200,
        }),
    });
    let item_id = graph.insert(Object::Vob(item))?;
    let npc = graph.npc_mut(npc_id)?;
    npc.instance = b"EXAMPLE_NPC".to_vec();
    npc.talents
        .push(Link::new(b"talent".to_vec(), Some(talent)));
    npc.carry.name = b"carryVob".to_vec();
    npc.enemy = Link::new(b"enemy".to_vec(), Some(npc_id));
    npc.items.push(NpcItem {
        item: Link::new(b"item0".to_vec(), Some(item_id)),
        short_key: Some(7),
    });
    npc.items.push(NpcItem {
        item: Link::new(b"alias".to_vec(), Some(item_id)),
        short_key: Some(-3),
    });
    if let NpcVersion::Gothic2 { packed, .. } = &mut npc.version {
        *packed = NpcPacked::from_fields([b""; 9], 8)?;
    }
    graph.add_root(Link::new(b"npc".to_vec(), Some(npc_id)))?;
    let mut metadata = Metadata::new(Format::BinSafe);
    metadata.save_game = true;
    let doc = ObjectArchive {
        metadata,
        game,
        graph,
    };
    let wire = doc.to_bytes(Limits::default())?;
    let read = ObjectArchive::from_bytes(&wire, game, Limits::default())?;
    let root = read.graph.roots()[0].target.unwrap();
    let npc = read.graph.npc(root)?;
    assert_eq!(npc.items[0].item.target, npc.items[1].item.target);
    assert_eq!(npc.items[0].short_key, Some(7));
    assert_eq!(npc.items[1].short_key, Some(-3));
    assert_eq!(npc.enemy.target, Some(root));
    println!(
        "NPC graph preserves {} nodes and occurrence-local shortcuts",
        read.graph.len()
    );
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
