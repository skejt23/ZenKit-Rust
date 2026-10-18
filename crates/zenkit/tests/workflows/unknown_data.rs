//! Preserve bounded opaque data, and explicitly choose when to skip a record.
use std::borrow::Cow;
use zenkit::{
    ErrorKind, GameVersion,
    archive::{self, Format, Header, SkipLimits, Value, ValueType},
    objects::ObjectArchive,
};

fn main() -> zenkit::Result<()> {
    // A typed read rejects padding without consuming it; fall back to raw data.
    for format in [Format::Ascii, Format::BinSafe] {
        let mut output = archive::Writer::new(Header::new(format), Default::default())?;
        output.write(b"padded", Value::RawFloat(Cow::Borrowed(&[1., 2., 3.])))?;
        let bytes = output.finish()?;
        let mut input = archive::Reader::new(&bytes, Default::default())?;
        let cursor = input.position();
        assert_eq!(input.read_vec2().unwrap_err().kind, ErrorKind::Unsupported);
        assert_eq!(input.position(), cursor);
        let entry = input.read(ValueType::RawFloat(2))?;
        assert_eq!(entry.value, Value::RawFloat(Cow::Borrowed(&[1., 2., 3.])));
        let mut output = archive::Writer::new(Header::new(format), Default::default())?;
        output.write(entry.name.expect("tagged field name"), entry.value)?;
        input.finish()?;
        let bytes = output.finish()?;
        let mut input = archive::Reader::new(&bytes, Default::default())?;
        assert_eq!(input.read_raw_float(2)?.as_ref(), &[1., 2., 3.]);
        input.finish()?;
    }

    // Skip an unknown class without retaining its data or registering its IDs.
    for format in [Format::Ascii, Format::Binary, Format::BinSafe] {
        let mut output = archive::Writer::new(Header::new(format), Default::default())?;
        output.begin_object(b"extension", b"FutureClass", 9)?;
        output.write(b"opaque", Value::Raw(Cow::Borrowed(&[0, 255, 128])))?;
        output.end_object()?;
        output.write(b"after", Value::Int(77))?;
        let bytes = output.finish()?;
        assert_eq!(
            ObjectArchive::from_bytes(&bytes, GameVersion::Gothic2, Default::default())
                .unwrap_err()
                .kind,
            ErrorKind::Unsupported
        );
        let mut input = archive::Reader::new(&bytes, Default::default())?;
        let skipped = input
            .skip_next_object(SkipLimits::default())?
            .expect("object");
        assert_eq!(skipped.class_name, b"FutureClass");
        assert_eq!(input.read_int()?, 77);
        input.finish()?;
    }
    println!("Preserved padded values and explicitly skipped unknown objects.");
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
