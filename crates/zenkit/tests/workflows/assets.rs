use zenkit::{
    GameVersion,
    archive::Format,
    font::{Font, Glyph},
    material::{Material, MaterialGroup, TextVec2},
    objects::{Graph, Limits, Link, Metadata, Object, ObjectArchive},
};
fn main() -> zenkit::Result<()> {
    let game = GameVersion::Gothic2;
    let mut material = Material::new(game);
    material.name = b"STONE\xff".to_vec();
    material.group = MaterialGroup::STONE;
    material.texture_scale = TextVec2::from_value([512.0, 256.0].into())?;
    let mut graph = Graph::new()?;
    let id = graph.insert(Object::Material(material))?;
    graph.add_root(Link::new(b"material".to_vec(), Some(id)))?;
    graph.add_root(Link::new(b"alias".to_vec(), Some(id)))?;
    let document = ObjectArchive {
        metadata: Metadata::new(Format::BinSafe),
        game,
        graph,
    };
    let bytes = document.to_bytes(Limits::default())?;
    let loaded = ObjectArchive::from_bytes(&bytes, game, Limits::default())?;
    assert_eq!(
        loaded.graph.roots()[0].target,
        loaded.graph.roots()[1].target
    );
    let font = Font {
        name: b"FONT\xff.TGA".to_vec(),
        height: 17,
        glyphs: vec![Glyph::default(); 256],
    };
    let bytes = font.to_bytes(Default::default())?;
    assert_eq!(Font::from_bytes(&bytes, Default::default())?, font);
    println!(
        "Shared versioned material and {} glyphs round-trip",
        font.glyphs.len()
    );
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
