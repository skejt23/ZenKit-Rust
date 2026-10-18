//! Execute a parameterless function from a DAT that needs no host bindings.
use zenkit::daedalus::{
    self, DecodeLimits, Script,
    vm::{Limits, NoHost, Policy, Vm},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: daedalus_vm INPUT.dat FUNCTION".into());
    }
    let parse_limits = daedalus::Limits::default();
    let bytes = zenkit::io::read_bounded(std::fs::File::open(&args[0])?, parse_limits.bytes)?;
    let script = Script::from_bytes(&bytes, parse_limits)?;
    let mut vm = Vm::from_script(
        &script,
        DecodeLimits::default(),
        Policy::default(),
        Limits::default(),
    )?;
    let result = vm.call_named(&mut NoHost, args[1].as_bytes(), Vec::new())?;
    println!("{result:?}");
    Ok(())
}
