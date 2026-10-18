use zenkit::{
    GameVersion,
    archive::{self, Format},
    objects::*,
    values::Color,
    vob::*,
};

fn main() -> zenkit::Result<()> {
    let game = GameVersion::Gothic2;
    let mut light = LightPreset::new(game);
    light.preset = b"TORCH".to_vec();
    light.range = 1500.0;
    let animation = light.animation.as_mut().unwrap();
    // Loading retains this exact spelling. Explicit edits choose canonical text.
    animation.range_scale = RangeAnimation::from_bytes(b" 0.5  1.25 ".to_vec());
    assert_eq!(
        animation.range_scale.decoded()?.collect::<Vec<_>>(),
        [0.5, 1.25]
    );
    animation.colors.set_values(
        &[
            Color::from_rgba([255, 127, 32, 255]),
            Color::from_rgba([224, 96, 16, 255]),
        ],
        128,
    )?;

    let mut vob = Vob::new(game);
    vob.base.name = Some(b"FIRE_LIGHT".to_vec());
    vob.kind = VobKind::Light(light.clone());
    let mut graph = Graph::new()?;
    let id = graph.insert(Object::Vob(vob))?;
    graph.add_root(Link::new(b"light".to_vec(), Some(id)))?;
    graph.add_root(Link::new(b"alias".to_vec(), Some(id)))?;
    let document = ObjectArchive {
        game,
        graph,
        metadata: Metadata::new(Format::BinSafe),
    };
    let bytes = document.to_bytes(Limits::default())?;
    let restored = ObjectArchive::from_bytes(&bytes, game, Limits::default())?;
    assert_eq!(
        restored.graph.roots()[0].target,
        restored.graph.roots()[1].target
    );
    let ObjectRef::Vob(vob) = restored
        .graph
        .get(restored.graph.roots()[0].target.unwrap())?
    else {
        unreachable!()
    };
    assert_eq!(vob.kind(), VobKind::Light(light.clone()).as_ref());

    // Standalone presets are field streams, with no invented object class.
    // These helpers consume partially on failure: `?` discards the stream here.
    let mut writer = archive::Writer::new(
        archive::Header::new(Format::Ascii),
        archive::Limits::default(),
    )?;
    light.write(&mut writer, game)?;
    let bytes = writer.finish()?;
    let mut reader = archive::Reader::new(&bytes, archive::Limits::default())?;
    assert_eq!(LightPreset::read(&mut reader, game, 1024)?, light);
    reader.finish()?;
    println!("Restored a shared light and a standalone preset without changing raw animation text");
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
