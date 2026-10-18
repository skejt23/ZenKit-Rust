//! Compose a caller-defined binary chunk with typed archive fields.
use zenkit::{
    archive::{Format, Header, Limits, Reader, Value, Writer},
    io,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for format in [Format::Ascii, Format::Binary, Format::BinSafe] {
        let mut header = Header::new(format);
        header.save_game = true;
        let mut writer = Writer::new(header, Limits::default())?;
        writer.begin_object(b"root", b"Example", 1)?;

        // Prepare and backpatch the complete raw chunk before appending it.
        let mut chunk = io::Writer::new(128);
        chunk.u16(0x1234, "tag")?;
        chunk.u32(4, "payload length")?;
        chunk.u32(42, "answer")?;
        writer.write(b"chunkLength", Value::Int(chunk.as_ref().len().try_into()?))?;
        writer.unframed(chunk.as_ref())?;
        if writer.is_save_game() {
            writer.write(b"savedState", Value::Int(7))?;
        }
        writer.end_object()?;
        let bytes = writer.finish()?;

        // Full archive readers start at the generic header, not at its end.
        let mut reader = Reader::new(&bytes, Limits::default())?;
        assert_eq!(reader.begin_object()?.unwrap().class_name, b"Example");
        let length = usize::try_from(reader.read_int()?)?;
        let mut raw = reader.unframed(length)?;
        assert_eq!(raw.u16("tag")?, 0x1234);
        assert_eq!(raw.u32("chunk length")?, 4);
        assert_eq!(raw.u32("answer")?, 42);
        assert_eq!(raw.remaining(), 0);
        assert_eq!(reader.read_int()?, 7);
        assert!(reader.end_object()?);
        reader.finish()?;
        println!("{format:?}: header inspection and mixed chunk/archive fields");
    }
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
