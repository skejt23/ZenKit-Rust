//! Inspect a compiled DAT without executing it.
use zenkit::daedalus::{Limits, Script};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("usage: daedalus INPUT.dat")?;
    let limits = Limits::default();
    let bytes = zenkit::io::read_bounded(std::fs::File::open(path)?, limits.bytes)?;
    let script = Script::from_bytes(&bytes, limits)?;
    println!(
        "DAT version {}, {} symbols, {} code bytes",
        script.version(),
        script.len(),
        script.code().len()
    );
    for symbol in script.symbols() {
        if let Some(name) = symbol.raw_name() {
            println!(
                "{}: {:?}",
                String::from_utf8_lossy(name.raw()),
                symbol.data_type()
            );
        }
    }
    Ok(())
}
