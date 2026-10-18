use zenkit::{GameVersion, archive::Format, objects::*, vob::*};

fn main() -> zenkit::Result<()> {
    let game = GameVersion::Gothic2;
    let mut graph = Graph::new()?;
    let mut chest = Vob::new(game);
    chest.base.physics_enabled = false;
    chest.base.state = Some(SaveState::default());
    chest.kind = VobKind::Container(Container {
        interactive: Interactive {
            movable: Movable {
                focus_name: b"CHEST".to_vec(),
                material: SoundMaterial::WOOD,
                ..Default::default()
            },
            state_count: 2,
            ..Default::default()
        },
        contents: b"ITMI_GOLD:10".to_vec(),
        items: Some(Vec::new()),
        ..Default::default()
    });
    let chest_id = graph.insert(Object::Vob(chest))?;
    let mut gold = Vob::new(game);
    gold.base.physics_enabled = false;
    gold.base.state = Some(SaveState::default());
    gold.kind = VobKind::Item(Item {
        instance: b"ITMI_GOLD".to_vec(),
        state: Some(ItemState {
            amount: 10,
            flags: 0,
        }),
    });
    let gold_id = graph.insert(Object::Vob(gold))?;
    let chest = graph.container_mut(chest_id)?;
    chest.items = Some(vec![
        Link::new(b"item".to_vec(), Some(gold_id)),
        Link::new(b"same_item".to_vec(), Some(gold_id)),
        Link::new(b"empty".to_vec(), None),
    ]);
    graph.add_root(Link::new(b"chest".to_vec(), Some(chest_id)))?;
    let mut metadata = Metadata::new(Format::BinSafe);
    metadata.save_game = true;
    let document = ObjectArchive {
        metadata,
        game,
        graph,
    };
    let restored = ObjectArchive::from_bytes(
        &document.to_bytes(Limits::default())?,
        game,
        Limits::default(),
    )?;
    let chest = restored
        .graph
        .container(restored.graph.roots()[0].target.unwrap())?;
    let items = chest.items.as_ref().unwrap();
    assert_eq!(items[0].target, items[1].target);
    assert!(items[2].target.is_none());
    let gold = restored.graph.item(items[0].target.unwrap())?;
    assert_eq!(gold.state.unwrap().amount, 10);
    assert_eq!(chest.contents, b"ITMI_GOLD:10");
    println!("Restored a saved container, shared Item identity and a null occurrence");
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
