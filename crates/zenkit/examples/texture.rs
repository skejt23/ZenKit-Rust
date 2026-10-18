//! Export a TEX texture to DDS.
use zenkit::texture::{Limits, Texture};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: texture INPUT.tex OUTPUT.dds".into());
    }
    let limits = Limits::default();
    let bytes = zenkit::io::read_bounded(std::fs::File::open(&args[0])?, limits.bytes)?;
    let texture = Texture::from_bytes(&bytes, limits)?;
    std::fs::write(&args[1], texture.to_dds(limits)?)?;
    Ok(())
}
