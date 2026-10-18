// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
//! Embed a VM with host storage, reentrant calls and deferred initialization.
use std::{convert::Infallible, error::Error};
use zenkit::daedalus::{self, DataType, DecodeLimits, Script, SymbolId, vm::*};

struct Game {
    constant: SymbolId,
    value: i32,
}
impl Host for Game {
    type Error = Infallible;
    fn call(
        &mut self,
        cx: &mut HostContext<'_, '_>,
        call: HostCall,
    ) -> VmResult<HostReturn, Self::Error> {
        assert_eq!(call.binding, 10);
        // HostContext borrows the VM only. Reborrow this same Game explicitly.
        let constant = cx
            .call(self, self.constant, Vec::new())?
            .unwrap()
            .as_int()?;
        Ok(HostReturn::Value(Value::Int(constant + 35)))
    }
    fn get_member(
        &mut self,
        _: &mut HostContext<'_, '_>,
        key: HostKey,
        member: MemberKey,
        index: usize,
    ) -> VmResult<Value, Self::Error> {
        assert_eq!((key, member.key, index), (HostKey(7), 0, 0));
        Ok(Value::Int(self.value))
    }
    fn set_member(
        &mut self,
        _: &mut HostContext<'_, '_>,
        key: HostKey,
        member: MemberKey,
        index: usize,
        value: Value,
    ) -> VmResult<(), Self::Error> {
        assert_eq!((key, member.key, index), (HostKey(7), 0, 0));
        self.value = value.as_int()?;
        Ok(())
    }
}

fn main() -> std::result::Result<(), Box<dyn Error>> {
    let script = example_script()?;
    let symbol = |name: &[u8]| script.find_name(name).unwrap().id();
    let mut game = Game {
        constant: symbol(b"CONSTANT"),
        value: 0,
    };
    let mut vm = Vm::from_script(
        &script,
        DecodeLimits::default(),
        Policy::default(),
        Limits::default(),
    )?;
    vm.register_external(symbol(b"HOST_VALUE"), 10, &[], DataType::Int)?;
    let class = vm.register_class(
        symbol(b"C_COUNTER"),
        &[MemberSpec {
            name: b"C_COUNTER.VALUE",
            kind: MemberType::Int,
            capacity: 1,
            key: 0,
        }],
    )?;
    let instance = vm.allocate_host(symbol(b"COUNTER"), class, HostKey(7))?;
    assert_eq!(game.value, 0); // Allocation does not execute script instructions.
    vm.init_instance(&mut game, instance, symbol(b"COUNTER"))?;
    let member = vm.reference(symbol(b"C_COUNTER.VALUE"), 0, Some(instance))?;
    assert_eq!(vm.read_reference(&mut game, member)?.as_int()?, 42);
    assert_eq!(vm.current_instance(), None);
    assert_eq!(vm.state().global(symbol(b"SELF"), 0)?.as_instance()?, None);
    drop(vm);
    drop(script);
    assert_eq!(game.value, 42);
    println!("Host counter after VM destruction: {}", game.value);
    Ok(())
}

// Build a sample DAT in memory.
fn example_script() -> zenkit::Result<Script> {
    fn word(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }
    #[allow(clippy::too_many_arguments)]
    fn symbol(
        out: &mut Vec<u8>,
        name: &[u8],
        ty: u32,
        count: u32,
        flags: u32,
        vary: u32,
        data: Option<u32>,
        parent: i32,
    ) {
        word(out, 1);
        out.extend_from_slice(name);
        out.push(b'\n');
        word(out, vary);
        word(out, count | ty << 12 | flags << 16);
        for _ in 0..5 {
            word(out, 0);
        }
        if let Some(data) = data {
            word(out, data);
        }
        word(out, parent as u32);
    }
    let mut bytes = vec![50];
    word(&mut bytes, 7);
    for index in 0..7 {
        word(&mut bytes, index);
    }
    symbol(&mut bytes, b"ENTRY", 5, 0, 3, 2, Some(0), -1);
    symbol(&mut bytes, b"HOST_VALUE", 5, 0, 11, 2, Some(u32::MAX), -1);
    symbol(&mut bytes, b"CONSTANT", 5, 0, 3, 2, Some(6), -1);
    symbol(&mut bytes, b"C_COUNTER", 4, 0, 0, 0, Some(0), -1);
    symbol(&mut bytes, b"C_COUNTER.VALUE", 2, 1, 4, 0, None, 3);
    symbol(&mut bytes, b"COUNTER", 7, 0, 1, 0, Some(12), 3);
    symbol(&mut bytes, b"SELF", 7, 0, 0, 0, Some(0), 3);
    let code = [
        62, 1, 0, 0, 0, 60, 64, 7, 0, 0, 0, 60, 62, 1, 0, 0, 0, 65, 4, 0, 0, 0, 9, 60,
    ];
    word(&mut bytes, code.len() as u32);
    bytes.extend(code);
    Script::from_bytes(&bytes, daedalus::Limits::default())
}

#[test]
fn workflow() {
    main().unwrap();
}
