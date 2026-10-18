//! Inspect a synthetic DAT without executing it.
use zenkit::{
    Result,
    daedalus::{DecodeLimits, InitialValue, Limits, Script},
};

fn main() -> Result<()> {
    let limits = Limits::default();
    let bytes = {
        let mut bytes = vec![50];
        for word in [1u32, 0, 1] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(b"ANSWER\n");
        for word in [0u32, 0x2001, 0, 0, 0, 0, 0, 42, u32::MAX, 1] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.push(60);
        bytes
    };
    let script = Script::from_bytes(&bytes, limits)?;
    println!(
        "DAT version {}, {} symbols, {} code bytes",
        script.version(),
        script.len(),
        script.code().len()
    );
    if let Some(symbol) = script.find_name(b"answer") {
        if let InitialValue::Integers(values) = symbol.initial_value() {
            println!("ANSWER initial values: {:?}", values.collect::<Vec<_>>());
        }
        assert_eq!(script.get(symbol.id())?.index(), symbol.index());
    }
    let decoded = script.decode_code(DecodeLimits::default())?;
    println!("{} decoded instructions", decoded.len());
    match script.validate_code(DecodeLimits::default()) {
        Ok(_) => println!("Instruction roles and targets validated; execution was not performed."),
        Err(error) => println!("Raw DAT remains available; executable validation: {error}"),
    }
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
