use zenkit::{
    GameVersion,
    archive::{self, Format, Header},
    material::{Material, MaterialRecord},
    model_script::{ModelScript, SyntaxMode},
};

fn main() -> zenkit::Result<()> {
    let mut writer = archive::Writer::new(Header::new(Format::BinSafe), Default::default())?;
    MaterialRecord {
        prefix: b"external name".to_vec(),
        prefix_field_name: Some(b"material".to_vec()),
        object_name: b"%".to_vec(),
        material: Material::new(GameVersion::Gothic1),
    }
    .write(&mut writer, GameVersion::Gothic1, 4096)?;
    let bytes = writer.finish()?;
    let mut reader = archive::Reader::new(&bytes, Default::default())?;
    let selected = MaterialRecord::read_auto(&mut reader, 4096)?;
    assert_eq!(selected.material.version.game(), GameVersion::Gothic1);
    assert_eq!(selected.prefix, b"external name");
    reader.finish()?;

    let source = b"Model(\"EXAMPLE\"){aniEnum{}}";
    let script = ModelScript::from_bytes(source, SyntaxMode::Strict, Default::default())?;
    assert_eq!(script.model_name.as_deref(), Some(b"EXAMPLE".as_slice()));
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
