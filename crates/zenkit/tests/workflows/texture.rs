use zenkit::texture::{Builder, Format, Limits, Texture};
fn main() -> zenkit::Result<()> {
    let limits = Limits::default();
    let mut builder = Builder::new(5, 7, limits)?;
    builder.push(Format::R8G8B8A8, &[255, 0, 0, 255].repeat(35), None)?;
    builder.push(Format::R8G8B8A8, &[0, 255, 0, 255].repeat(6), None)?;
    let mut texture = builder.build_converted(Format::Dxt5, None)?;
    texture.set_reference_size(10, 14);
    texture.set_average_color(0xffff0000);
    let encoded = texture.to_bytes(limits)?;
    let reopened = Texture::from_bytes(&encoded, limits)?;
    assert_eq!(reopened, texture);
    assert_eq!(reopened.mip(0).unwrap().rgba8(limits)?.len(), 140);
    let rgba = reopened.converted(Format::R8G8B8A8, None, limits)?;
    assert_eq!(rgba.metadata(), texture.metadata());
    assert_eq!(&texture.to_dds(limits)?[..4], b"DDS ");
    println!("texture create/compress/read/convert/DDS workflow passed");
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
