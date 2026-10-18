use zenkit::archive::{Format, Header, Limits, Reader, Value, Writer};
fn main() -> zenkit::Result<()> {
    for format in [Format::Ascii, Format::Binary, Format::BinSafe] {
        let mut output = Writer::new(Header::new(format), Limits::default())?;
        let root = output.begin_object(b"root", b"Example", 7)?;
        output.write(b"message", Value::String(b"Gothic".as_slice().into()))?;
        output.write(b"answer", Value::Int(42))?;
        output.write_reference(b"again", root)?;
        output.end_object()?;
        let bytes = output.finish()?;
        let mut input = Reader::new(&bytes, Limits::default())?;
        assert_eq!(input.begin_object()?.unwrap().version, 7);
        assert_eq!(
            std::str::from_utf8(&input.read_string()?).unwrap(),
            "Gothic"
        );
        assert_eq!(input.read_int()?, 42);
        assert!(input.begin_object()?.unwrap().is_reference());
        assert!(input.end_object()?);
        assert!(input.end_object()?);
        input.finish()?;
        println!("{format:?}: {} bytes", bytes.len());
    }
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
