//! Use caller-owned devices with a bounded snapshot and borrowed output storage.
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use zenkit::io::{Reader, Writer, read_bounded};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut device = Cursor::new(b"prefix\x78\x56\x34\x12suffix");
    device.seek(SeekFrom::Start(6))?;
    let bytes = read_bounded((&mut device).take(4), 1024)?;
    let value = Reader::new(&bytes).u32("value")?;
    let mut suffix = String::new();
    device.read_to_string(&mut suffix)?;
    assert_eq!(suffix, "suffix");
    let mut output = Writer::new(1024);
    output.u32(value + 1, "edited value")?;
    let mut storage = [0xaa; 12];
    let mut destination = Cursor::new(storage.as_mut_slice());
    destination.seek(SeekFrom::Start(4))?;
    // Writes may fail after partial output; buffered sinks also need flushing.
    destination.write_all(output.as_slice())?;
    destination.flush()?;
    assert_eq!(
        storage,
        [
            0xaa, 0xaa, 0xaa, 0xaa, 0x79, 0x56, 0x34, 0x12, 0xaa, 0xaa, 0xaa, 0xaa
        ]
    );
    println!("Read a bounded frame and published edited bytes into borrowed storage");
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
