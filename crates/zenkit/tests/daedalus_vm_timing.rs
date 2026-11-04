//! Instruction dispatch timing. Ignored by default; run with
//! `cargo test --release --test daedalus_vm_timing -- --ignored --nocapture`.
//! The installed-script measurement reads the DAT named by `ZENKIT_GOTHIC_DAT`.
use std::convert::Infallible;
use std::time::Instant;
use zenkit::daedalus::{self, DataType, DecodeLimits, Script, SymbolId, vm::*};

fn word(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn sym(name: &[u8], ty: u32, count: u32, flags: u32, vary: u32, data: &[u8]) -> Vec<u8> {
    member(name, ty, count, flags, vary, data, u32::MAX)
}
fn member(
    name: &[u8],
    ty: u32,
    count: u32,
    flags: u32,
    vary: u32,
    data: &[u8],
    parent: u32,
) -> Vec<u8> {
    let mut out = Vec::new();
    word(&mut out, 1);
    out.extend_from_slice(name);
    out.push(10);
    word(&mut out, vary);
    word(&mut out, count | ty << 12 | flags << 16);
    for _ in 0..5 {
        word(&mut out, 0);
    }
    out.extend_from_slice(data);
    word(&mut out, parent);
    out
}
fn program(symbols: Vec<Vec<u8>>, code: Vec<u8>) -> Script {
    let mut bytes = vec![50];
    word(&mut bytes, symbols.len() as u32);
    for i in 0..symbols.len() {
        word(&mut bytes, i as u32);
    }
    for s in symbols {
        bytes.extend(s);
    }
    word(&mut bytes, code.len() as u32);
    bytes.extend(code);
    Script::from_bytes(&bytes, daedalus::Limits::default()).unwrap()
}
fn op(code: &mut Vec<u8>, opcode: u8, value: u32) {
    code.push(opcode);
    word(code, value);
}
fn id(script: &Script, index: u32) -> SymbolId {
    script.symbol(index).unwrap().id()
}

struct Ext;
impl Host for Ext {
    type Error = Infallible;
    fn call(
        &mut self,
        _: &mut HostContext<'_, '_>,
        call: HostCall,
    ) -> VmResult<HostReturn, Self::Error> {
        let Some(Argument::Value(Value::Int(n))) = call.arguments.first() else {
            return Ok(HostReturn::Value(Value::Int(0)));
        };
        Ok(HostReturn::Value(Value::Int(n / 2)))
    }
    fn get_member(
        &mut self,
        _: &mut HostContext<'_, '_>,
        _: HostKey,
        _: MemberKey,
        _: usize,
    ) -> VmResult<Value, Self::Error> {
        Ok(Value::Int(1))
    }
}

/// `while (I < N) { ACC += body(I); I += 1; }` where body is arithmetic, a
/// script call, an external call, a host-backed member read or an external
/// call with an instance argument.
fn loop_script(body: u8) -> Script {
    const I: u32 = 1;
    const N: u32 = 2;
    const ACC: u32 = 3;
    const G_X: u32 = 5;
    const EXT: u32 = 6;
    const C_V: u32 = 9;
    const H: u32 = 10;
    const EXT_I: u32 = 11;
    let mut code = Vec::new();
    // G(x): return x + 1
    let g = code.len() as u32;
    op(&mut code, 65, G_X);
    code.push(9);
    op(&mut code, 64, 1);
    op(&mut code, 65, G_X);
    code.extend([0, 60]);
    let f = code.len() as u32;
    let top = code.len() as u32;
    op(&mut code, 65, N);
    op(&mut code, 65, I);
    code.push(7);
    let exit = code.len();
    op(&mut code, 76, 0);
    match body {
        0 => {
            op(&mut code, 64, 3);
            op(&mut code, 65, I);
            code.push(2);
        }
        1 => {
            op(&mut code, 65, I);
            op(&mut code, 61, g);
        }
        2 => {
            op(&mut code, 65, I);
            op(&mut code, 62, EXT);
        }
        3 => {
            op(&mut code, 80, H);
            op(&mut code, 65, C_V);
        }
        _ => {
            op(&mut code, 67, H);
            op(&mut code, 62, EXT_I);
        }
    }
    op(&mut code, 65, ACC);
    code.push(0);
    op(&mut code, 65, ACC);
    code.push(9);
    op(&mut code, 64, 1);
    op(&mut code, 65, I);
    code.push(19);
    op(&mut code, 75, top);
    let end = code.len() as u32;
    code[exit + 1..exit + 5].copy_from_slice(&end.to_le_bytes());
    code.push(60);
    program(
        vec![
            sym(b"F", 5, 0, 1, 0, &f.to_le_bytes()),
            sym(b"I", 2, 1, 0, 0, &0u32.to_le_bytes()),
            sym(b"N", 2, 1, 0, 0, &0u32.to_le_bytes()),
            sym(b"ACC", 2, 1, 0, 0, &0u32.to_le_bytes()),
            sym(b"G", 5, 1, 3, 2, &g.to_le_bytes()),
            sym(b"G.X", 2, 1, 0, 0, &0u32.to_le_bytes()),
            sym(b"EXT", 5, 1, 11, 2, &u32::MAX.to_le_bytes()),
            sym(b"EXT.X", 2, 1, 0, 0, &0u32.to_le_bytes()),
            sym(b"C", 4, 1, 0, 0, &0u32.to_le_bytes()),
            member(b"C.V", 2, 1, 4, 0, &[], 8),
            member(b"H", 7, 0, 0, 0, &0u32.to_le_bytes(), 8),
            sym(b"EXT_I", 5, 1, 11, 2, &u32::MAX.to_le_bytes()),
            member(b"EXT_I.P", 7, 0, 0, 0, &0u32.to_le_bytes(), 8),
        ],
        code,
    )
}

#[test]
#[ignore = "timing measurement; run with --release --ignored --nocapture"]
fn synthetic_loop_dispatch_cost() {
    const ITERATIONS: i32 = 200_000;
    for (body, name) in [
        (0, "arithmetic"),
        (1, "script call"),
        (2, "external call"),
        (3, "host member read"),
        (4, "external call with instance argument"),
    ] {
        let script = loop_script(body);
        let mut vm = Vm::from_script(
            &script,
            DecodeLimits::default(),
            Policy::default(),
            Limits {
                fuel: u64::MAX,
                ..Limits::default()
            },
        )
        .unwrap();
        vm.register_external(
            id(&script, 6),
            1,
            &[Parameter::Value(DataType::Int)],
            DataType::Int,
        )
        .unwrap();
        vm.register_external(
            id(&script, 11),
            2,
            &[Parameter::InstanceReference],
            DataType::Int,
        )
        .unwrap();
        let class = vm
            .register_class(
                id(&script, 8),
                &[MemberSpec {
                    name: b"C.V",
                    kind: MemberType::Int,
                    capacity: 1,
                    key: 0,
                }],
            )
            .unwrap();
        let instance = vm
            .allocate_host(id(&script, 10), class, HostKey(1))
            .unwrap();
        vm.set_global(id(&script, 10), 0, Value::Instance(Some(instance)))
            .unwrap();
        let mut best = f64::MAX;
        let mut count = 0;
        for _ in 0..5 {
            vm.set_global(id(&script, 1), 0, Value::Int(0)).unwrap();
            vm.set_global(id(&script, 2), 0, Value::Int(ITERATIONS))
                .unwrap();
            let started = Instant::now();
            vm.call(&mut Ext, id(&script, 0), Vec::new()).unwrap();
            let elapsed = started.elapsed().as_secs_f64();
            count = u64::MAX - vm.fuel_remaining();
            best = best.min(elapsed * 1e9 / count as f64);
            if body == 3 {
                // Every iteration read the member through the host.
                let acc = vm.state().global(id(&script, 3), 0).unwrap().as_int();
                assert_eq!(acc.unwrap(), ITERATIONS);
                vm.set_global(id(&script, 3), 0, Value::Int(0)).unwrap();
            }
        }
        println!("synthetic {name}: {count} instructions, {best:.1} ns per instruction");
    }
    // Endless loops until the fuel runs out: the per-instruction floor of
    // dispatch and of single stack operations.
    let mut branch = Vec::new();
    op(&mut branch, 75, 0);
    let mut immediate = Vec::new();
    op(&mut immediate, 64, 0);
    op(&mut immediate, 76, 0);
    let mut global = Vec::new();
    op(&mut global, 65, 1);
    op(&mut global, 76, 0);
    let mut store = Vec::new();
    op(&mut store, 64, 0);
    op(&mut store, 65, 1);
    store.push(9);
    op(&mut store, 75, 0);
    for (name, code) in [
        ("branch", branch),
        ("PUSHI+BZ", immediate),
        ("PUSHV+BZ", global),
        ("PUSHI+PUSHV+MOVI+B", store),
    ] {
        println!(
            "synthetic {name}: {:.1} ns per instruction",
            fuel_loop(code)
        );
    }
}

fn fuel_loop(code: Vec<u8>) -> f64 {
    const FUEL: u64 = 10_000_000;
    let script = program(
        vec![
            sym(b"F", 5, 0, 1, 0, &0u32.to_le_bytes()),
            sym(b"I", 2, 1, 0, 0, &0u32.to_le_bytes()),
        ],
        code,
    );
    let mut vm = Vm::from_script(
        &script,
        DecodeLimits::default(),
        Policy::default(),
        Limits {
            fuel: FUEL,
            ..Limits::default()
        },
    )
    .unwrap();
    let started = Instant::now();
    let e = vm
        .call(&mut NoHost, id(&script, 0), Vec::new())
        .unwrap_err();
    assert_eq!(e.fault.kind, FaultKind::Limit(LimitKind::Fuel));
    started.elapsed().as_secs_f64() * 1e9 / FUEL as f64
}

#[test]
#[ignore = "timing measurement on the DAT named by ZENKIT_GOTHIC_DAT"]
fn installed_state_loop_dispatch_cost() {
    let Some(path) = std::env::var_os("ZENKIT_GOTHIC_DAT") else {
        println!("ZENKIT_GOTHIC_DAT unset");
        return;
    };
    let bytes = std::fs::read(path).unwrap();
    let script = Script::from_bytes(&bytes, daedalus::Limits::default()).unwrap();
    let started = Instant::now();
    let mut vm = Vm::from_script(
        &script,
        DecodeLimits::default(),
        Policy {
            allow_null_members: true,
            ..Policy::default()
        },
        Limits::default(),
    )
    .unwrap();
    println!(
        "VM for {} instructions created in {:?}",
        script.decode_code(DecodeLimits::default()).unwrap().len(),
        started.elapsed()
    );
    vm.default_external_zero(None);
    let functions: Vec<SymbolId> = script
        .symbols()
        .filter(|s| !s.is_member() && !s.is_external() && s.data_type() == DataType::Function)
        .filter(|s| {
            let name = s.raw_name().map(|n| n.raw().to_ascii_uppercase());
            name.is_some_and(|n| n.starts_with(b"ZS_") && n.ends_with(b"_LOOP"))
        })
        .filter(|s| script.parameters(s.id()).is_ok_and(|p| p.len() == 0))
        .map(|s| s.id())
        .collect();
    let limit = Limits::default().fuel;
    let mut instructions = 0;
    let mut faults = 0;
    for &f in &functions {
        let result = vm.call(&mut NoHost, f, Vec::new());
        faults += usize::from(result.is_err());
        instructions += limit - vm.fuel_remaining();
    }
    const ROUNDS: u32 = 2000;
    let started = Instant::now();
    for _ in 0..ROUNDS {
        for &f in &functions {
            let _ = vm.call(&mut NoHost, f, Vec::new());
        }
    }
    let elapsed = started.elapsed();
    let per_instruction = elapsed.as_secs_f64() * 1e9 / (instructions * u64::from(ROUNDS)) as f64;
    println!(
        "installed: {} loops ({faults} faulted), {instructions} instructions per round, \
         {:?} per call, {per_instruction:.1} ns per instruction",
        functions.len(),
        elapsed / (ROUNDS * functions.len() as u32)
    );
}
