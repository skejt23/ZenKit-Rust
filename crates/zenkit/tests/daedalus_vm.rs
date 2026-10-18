use std::convert::Infallible;
use zenkit::daedalus::{self, DataType, DecodeLimits, Script, SymbolId, vm::*};

fn word(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn sym(
    name: &[u8],
    ty: u32,
    count: u32,
    flags: u32,
    vary: u32,
    data: &[u8],
    parent: i32,
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
    word(&mut out, parent as u32);
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

#[test]
fn actual_host_api_supports_same_handler_reentry_and_member_bridge() {
    let mut code = Vec::new();
    op(&mut code, 62, 1);
    code.push(60);
    op(&mut code, 64, 42);
    op(&mut code, 65, 3);
    code.extend([9, 60]);
    let script = program(
        vec![
            sym(b"F", 5, 0, 3, 2, &0u32.to_le_bytes(), -1),
            sym(b"EXT", 5, 0, 11, 2, &u32::MAX.to_le_bytes(), -1),
            sym(b"C", 4, 0, 0, 0, &0u32.to_le_bytes(), -1),
            sym(b"C.VALUE", 2, 1, 4, 0, &[], 2),
            sym(b"I", 7, 0, 1, 0, &6u32.to_le_bytes(), 2),
            sym(b"SELF", 7, 0, 0, 0, &0u32.to_le_bytes(), 2),
        ],
        code,
    );
    struct App {
        function: SymbolId,
        depth: usize,
        calls: usize,
        value: i32,
    }
    impl Host for App {
        type Error = Infallible;
        fn call(
            &mut self,
            cx: &mut HostContext<'_, '_>,
            call: HostCall,
        ) -> VmResult<HostReturn, Self::Error> {
            assert_eq!(call.binding, 10);
            assert!(call.arguments.is_empty());
            self.calls += 1;
            self.depth += 1;
            let value = if self.depth < 3 {
                cx.call(self, self.function, Vec::new())?
                    .unwrap()
                    .as_int()?
                    + 1
            } else {
                7
            };
            self.depth -= 1;
            Ok(HostReturn::Value(Value::Int(value)))
        }
        fn get_member(
            &mut self,
            _cx: &mut HostContext<'_, '_>,
            key: HostKey,
            member: MemberKey,
            index: usize,
        ) -> VmResult<Value, Self::Error> {
            assert_eq!(key, HostKey(99));
            assert_eq!((member.key, index), (4, 0));
            Ok(Value::Int(self.value))
        }
        fn set_member(
            &mut self,
            cx: &mut HostContext<'_, '_>,
            key: HostKey,
            member: MemberKey,
            index: usize,
            value: Value,
        ) -> VmResult<(), Self::Error> {
            assert_eq!(key, HostKey(99));
            assert_eq!((member.key, index), (4, 0));
            let nested = cx
                .call(self, self.function, Vec::new())?
                .unwrap()
                .as_int()?;
            self.value = value.as_int()? + nested;
            Ok(())
        }
    }
    let mut host = App {
        function: id(&script, 0),
        depth: 0,
        calls: 0,
        value: 0,
    };
    let mut vm = Vm::new(
        script.validate_code(DecodeLimits::default()).unwrap(),
        Policy::default(),
        Limits::default(),
    )
    .unwrap();
    vm.register_external(id(&script, 1), 10, &[], DataType::Int)
        .unwrap();
    assert_eq!(
        vm.call(&mut host, id(&script, 0), Vec::new())
            .unwrap()
            .unwrap()
            .as_int()
            .unwrap(),
        9
    );
    assert_eq!(host.calls, 3);
    assert_eq!((vm.call_depth(), vm.stack_len()), (0, 0));
    let class = vm
        .register_class(
            id(&script, 2),
            &[MemberSpec {
                name: b"C.VALUE",
                kind: MemberType::Int,
                capacity: 1,
                key: 4,
            }],
        )
        .unwrap();
    let instance = vm
        .allocate_host(id(&script, 4), class, HostKey(99))
        .unwrap();
    vm.init_instance(&mut host, instance, id(&script, 4))
        .unwrap();
    assert_eq!(host.value, 51);
    assert_eq!(host.calls, 6);
    assert_eq!(vm.current_instance(), None);
    assert_eq!(
        vm.state()
            .global(id(&script, 5), 0)
            .unwrap()
            .as_instance()
            .unwrap(),
        None
    );
    let r = vm.reference(id(&script, 3), 0, Some(instance)).unwrap();
    assert_eq!(
        vm.read_reference(&mut host, r).unwrap().as_int().unwrap(),
        51
    );
    let state = vm.into_state();
    assert_eq!(
        state.instance_backend(instance).unwrap(),
        InstanceBackend::Host(HostKey(99))
    );
    drop(state);
    assert_eq!(host.value, 51);
}

#[test]
fn runtime_growth_can_enable_previously_out_of_range_indexed_bytecode() {
    let mut code = Vec::new();
    code.push(245);
    word(&mut code, 1);
    code.push(1);
    code.push(60);
    let script = program(
        vec![
            sym(b"F", 5, 0, 3, 2, &0u32.to_le_bytes(), -1),
            sym(b"A", 2, 1, 0, 0, &10u32.to_le_bytes(), -1),
        ],
        code,
    );
    assert!(script.validate_code(DecodeLimits::default()).is_err());
    let mut vm = Vm::from_script(
        &script,
        DecodeLimits::default(),
        Policy::default(),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(
        vm.call(&mut NoHost, id(&script, 0), Vec::new())
            .unwrap_err()
            .fault
            .kind,
        FaultKind::Index
    );
    vm.grow(id(&script, 1), 1).unwrap();
    vm.set_global(id(&script, 1), 1, Value::Int(37)).unwrap();
    assert_eq!(
        vm.call(&mut NoHost, id(&script, 0), Vec::new())
            .unwrap()
            .unwrap()
            .as_int()
            .unwrap(),
        37
    );
    assert_eq!(script.symbol(1).unwrap().count(), 1);
}

#[test]
fn fuel_failure_cannot_be_recovered() {
    let mut code = Vec::new();
    op(&mut code, 75, 0);
    let script = program(vec![sym(b"F", 5, 0, 1, 0, &0u32.to_le_bytes(), -1)], code);
    struct Recover(usize);
    impl Host for Recover {
        type Error = Infallible;
        fn exception(
            &mut self,
            _: &mut HostContext<'_, '_>,
            _: &VmError<Self::Error>,
        ) -> VmResult<Recovery, Self::Error> {
            self.0 += 1;
            Ok(Recovery::Continue)
        }
    }
    let mut vm = Vm::new(
        script.validate_code(DecodeLimits::default()).unwrap(),
        Policy::default(),
        Limits {
            fuel: 3,
            ..Limits::default()
        },
    )
    .unwrap();
    let mut host = Recover(0);
    let e = vm.call(&mut host, id(&script, 0), Vec::new()).unwrap_err();
    assert_eq!(e.fault.kind, FaultKind::Limit(LimitKind::Fuel));
    assert_eq!(host.0, 0);
    assert_eq!(vm.call_depth(), 0);
}

fn string_packet_script(arguments: u32) -> Script {
    let mut code = Vec::new();
    for _ in 0..arguments {
        op(&mut code, 65, arguments + 2);
    }
    op(&mut code, 62, 1);
    code.push(60);
    let mut symbols = vec![
        sym(b"F", 5, 0, 1, 0, &0u32.to_le_bytes(), -1),
        sym(b"EXT", 5, arguments, 9, 0, &u32::MAX.to_le_bytes(), -1),
    ];
    for _ in 0..arguments {
        symbols.push(sym(b"PARAM", 3, 1, 0, 0, b"\n", -1));
    }
    symbols.push(sym(b"TEXT", 3, 1, 0, 0, b"four\n", -1));
    program(symbols, code)
}

#[test]
fn callback_arguments_stay_charged_through_registered_and_default_reentry() {
    struct App {
        calls: usize,
        remaining: usize,
    }
    impl Host for App {
        type Error = Infallible;
        fn call(
            &mut self,
            cx: &mut HostContext<'_, '_>,
            call: HostCall,
        ) -> VmResult<HostReturn, Self::Error> {
            self.calls += 1;
            self.remaining = cx.string_limit();
            assert_eq!(call.arguments.len(), 2);
            // Deliberately swallow this resource failure: it still terminates the invocation.
            assert_eq!(
                cx.vm().push_string(b"x").unwrap_err().kind,
                FaultKind::Limit(LimitKind::Strings)
            );
            Ok(HostReturn::Void)
        }
    }
    let script = string_packet_script(2);
    for registered in [false, true] {
        let mut vm = Vm::from_script(
            &script,
            DecodeLimits::default(),
            Policy::default(),
            Limits {
                string_bytes: 12,
                ..Limits::default()
            },
        )
        .unwrap();
        if registered {
            vm.register_external(
                id(&script, 1),
                1,
                &[Parameter::Value(DataType::String); 2],
                DataType::Void,
            )
            .unwrap();
        } else {
            vm.default_external_zero(Some(1));
        }
        let mut host = App {
            calls: 0,
            remaining: usize::MAX,
        };
        let err = vm.call(&mut host, id(&script, 0), vec![]).unwrap_err();
        assert_eq!(err.fault.kind, FaultKind::Limit(LimitKind::Strings));
        assert_eq!((host.calls, host.remaining), (1, 0));
        assert_eq!((vm.stack_len(), vm.call_depth()), (0, 0));
        vm.push_string(b"12345678").unwrap();
        assert_eq!(vm.pop_string(&mut NoHost).unwrap(), b"12345678");
    }
    let script = string_packet_script(3);
    let mut vm = Vm::from_script(
        &script,
        DecodeLimits::default(),
        Policy::default(),
        Limits {
            string_bytes: 12,
            ..Limits::default()
        },
    )
    .unwrap();
    vm.default_external_zero(Some(1));
    let mut host = App {
        calls: 0,
        remaining: usize::MAX,
    };
    assert_eq!(
        vm.call(&mut host, id(&script, 0), vec![])
            .unwrap_err()
            .fault
            .kind,
        FaultKind::Limit(LimitKind::Strings)
    );
    assert_eq!(host.calls, 0);
    vm.push_string(b"12345678").unwrap();
}

#[test]
fn owned_temporary_references_can_be_popped_and_requeued_without_fake_symbols() {
    let script = program(
        vec![sym(b"F", 5, 0, 1, 0, &0u32.to_le_bytes(), -1)],
        vec![60],
    );
    let mut vm = Vm::from_script(
        &script,
        DecodeLimits::default(),
        Policy::default(),
        Limits {
            string_bytes: 4,
            ..Limits::default()
        },
    )
    .unwrap();
    vm.push_string(b"a\0bc").unwrap();
    assert!(vm.top_is_reference().unwrap());
    let temporary = vm.pop_reference().unwrap();
    assert!(matches!(&temporary,StackReference::Temporary(v) if v==b"a\0bc"));
    vm.push_stack_reference(temporary).unwrap();
    assert_eq!(vm.pop_string(&mut NoHost).unwrap(), b"a\0bc");
}

#[test]
fn host_depth_is_terminal_even_if_nested_error_is_swallowed() {
    let mut code = Vec::new();
    op(&mut code, 62, 1);
    code.push(60);
    let script = program(
        vec![
            sym(b"F", 5, 0, 1, 0, &0u32.to_le_bytes(), -1),
            sym(b"EXT", 5, 0, 9, 0, &u32::MAX.to_le_bytes(), -1),
        ],
        code,
    );
    struct App {
        f: SymbolId,
        calls: usize,
        errors: usize,
    }
    impl Host for App {
        type Error = Infallible;
        fn call(
            &mut self,
            cx: &mut HostContext<'_, '_>,
            _: HostCall,
        ) -> VmResult<HostReturn, Self::Error> {
            self.calls += 1;
            let _ = cx.call(self, self.f, vec![]);
            Ok(HostReturn::Void)
        }
        fn exception(
            &mut self,
            _: &mut HostContext<'_, '_>,
            _: &VmError<Self::Error>,
        ) -> VmResult<Recovery, Self::Error> {
            self.errors += 1;
            Ok(Recovery::Continue)
        }
    }
    let mut vm = Vm::from_script(
        &script,
        DecodeLimits::default(),
        Policy::default(),
        Limits {
            host_depth: 2,
            ..Limits::default()
        },
    )
    .unwrap();
    vm.register_external(id(&script, 1), 0, &[], DataType::Void)
        .unwrap();
    let mut host = App {
        f: id(&script, 0),
        calls: 0,
        errors: 0,
    };
    let err = vm.call(&mut host, id(&script, 0), vec![]).unwrap_err();
    assert_eq!(err.fault.kind, FaultKind::Limit(LimitKind::HostDepth));
    assert_eq!((host.calls, host.errors), (2, 0));
    assert_eq!((vm.stack_len(), vm.call_depth()), (0, 0));
}

#[test]
fn nested_calls_cannot_take_values_below_the_current_frame_floor() {
    let mut code = Vec::new();
    op(&mut code, 61, 6);
    code.push(60);
    op(&mut code, 64, 5);
    code.push(60);
    let script = program(
        vec![
            sym(b"OUTER", 5, 0, 3, 2, &0u32.to_le_bytes(), -1),
            sym(b"INNER", 5, 1, 3, 2, &6u32.to_le_bytes(), -1),
            sym(b"ARG", 2, 1, 0, 0, &0u32.to_le_bytes(), -1),
        ],
        code,
    );
    let mut vm = Vm::from_script(
        &script,
        DecodeLimits::default(),
        Policy::default(),
        Limits::default(),
    )
    .unwrap();
    vm.push(Value::Int(123)).unwrap();
    assert_eq!(
        vm.call(&mut NoHost, id(&script, 0), vec![])
            .unwrap_err()
            .fault
            .kind,
        FaultKind::StackUnderflow
    );
    assert_eq!(vm.pop_int(&mut NoHost).unwrap(), 123);
    assert_eq!((vm.call_depth(), vm.stack_len()), (0, 0));
}

#[test]
fn discarding_a_return_does_not_copy_its_referenced_string() {
    let mut code = Vec::new();
    op(&mut code, 65, 1);
    code.push(60);
    let script = program(
        vec![
            sym(b"F", 5, 0, 3, 3, &0u32.to_le_bytes(), -1),
            sym(b"S", 3, 1, 0, 0, b"large\n", -1),
        ],
        code,
    );
    let mut vm = Vm::from_script(
        &script,
        DecodeLimits::default(),
        Policy::default(),
        Limits {
            string_bytes: 5,
            ..Limits::default()
        },
    )
    .unwrap();
    assert_eq!(
        vm.call(&mut NoHost, id(&script, 0), vec![])
            .unwrap_err()
            .fault
            .kind,
        FaultKind::Limit(LimitKind::Strings)
    );
    vm.call_discard(&mut NoHost, id(&script, 0), vec![])
        .unwrap();
    assert_eq!(vm.stack_len(), 0);
}

#[test]
fn recursive_local_snapshots_restore_grown_storage_before_return_resolution() {
    let mut code = Vec::new();
    op(&mut code, 62, 2);
    op(&mut code, 65, 1);
    code.push(60);
    let script = program(
        vec![
            sym(b"F", 5, 0, 3, 2, &0u32.to_le_bytes(), -1),
            sym(b"F.LOCAL", 2, 1, 0, 0, &0u32.to_le_bytes(), 0),
            sym(b"EXT", 5, 0, 9, 0, &u32::MAX.to_le_bytes(), -1),
        ],
        code,
    );
    struct App {
        f: SymbolId,
        local: SymbolId,
        depth: usize,
        nested: Option<i32>,
    }
    impl Host for App {
        type Error = Infallible;
        fn call(
            &mut self,
            cx: &mut HostContext<'_, '_>,
            _: HostCall,
        ) -> VmResult<HostReturn, Self::Error> {
            self.depth += 1;
            let result = if self.depth == 1 {
                cx.vm().set_global(self.local, 0, Value::Int(10))?;
                let value = cx.call(self, self.f, vec![])?;
                self.nested = Some(value.unwrap().as_int()?);
                Ok(())
            } else {
                cx.vm().set_global(self.local, 0, Value::Int(20))?;
                cx.vm().grow(self.local, 1)?;
                cx.vm().set_global(self.local, 1, Value::Int(99))
            };
            self.depth -= 1;
            result?;
            Ok(HostReturn::Void)
        }
    }
    let mut vm = Vm::from_script(
        &script,
        DecodeLimits::default(),
        Policy::default(),
        Limits {
            snapshot_cells: 2,
            ..Limits::default()
        },
    )
    .unwrap();
    let cells = vm.state().cell_count();
    vm.register_external(id(&script, 2), 0, &[], DataType::Void)
        .unwrap();
    vm.set_recursive_locals(id(&script, 0), true).unwrap();
    let mut host = App {
        f: id(&script, 0),
        local: id(&script, 1),
        depth: 0,
        nested: None,
    };
    assert_eq!(
        vm.call(&mut host, id(&script, 0), vec![])
            .unwrap()
            .unwrap()
            .as_int()
            .unwrap(),
        10
    );
    assert_eq!(host.nested, Some(10));
    assert_eq!(vm.state().runtime_count(id(&script, 1)).unwrap(), 1);
    assert_eq!(vm.state().cell_count(), cells);
}

#[test]
fn overrides_default_function_values_and_raw_stack_routes_are_distinct() {
    let mut code = Vec::new();
    op(&mut code, 61, 6);
    code.push(60);
    op(&mut code, 64, 5);
    code.push(60);
    let script = program(
        vec![
            sym(b"F", 5, 0, 3, 2, &0u32.to_le_bytes(), -1),
            sym(b"TARGET", 5, 0, 3, 2, &6u32.to_le_bytes(), -1),
        ],
        code,
    );
    struct App;
    impl Host for App {
        type Error = Infallible;
        fn call(
            &mut self,
            cx: &mut HostContext<'_, '_>,
            call: HostCall,
        ) -> VmResult<HostReturn, Self::Error> {
            match call.kind {
                CallKind::Override => Ok(HostReturn::Value(Value::Int(41))),
                CallKind::RawOverride => {
                    cx.push(Value::Int(77))?;
                    Ok(HostReturn::StackHandled)
                }
                CallKind::RawDefault => {
                    assert_eq!(cx.pop_int(self)?, 42);
                    cx.push(Value::Function(88))?;
                    Ok(HostReturn::StackHandled)
                }
                CallKind::External => {
                    let Argument::Value(v) = call.arguments.into_iter().next().unwrap() else {
                        panic!()
                    };
                    Ok(HostReturn::Value(Value::Int(v.as_int()? + 1)))
                }
                _ => panic!(),
            }
        }
    }
    let mut vm = Vm::from_script(
        &script,
        DecodeLimits::default(),
        Policy::default(),
        Limits::default(),
    )
    .unwrap();
    vm.override_function(id(&script, 1), 0, &[], DataType::Int)
        .unwrap();
    assert_eq!(
        vm.call(&mut App, id(&script, 0), vec![])
            .unwrap()
            .unwrap()
            .as_int()
            .unwrap(),
        41
    );
    assert_eq!(
        vm.call(&mut App, id(&script, 1), vec![])
            .unwrap()
            .unwrap()
            .as_int()
            .unwrap(),
        5
    );
    vm.override_raw(id(&script, 1), 0).unwrap();
    assert_eq!(
        vm.call(&mut App, id(&script, 0), vec![])
            .unwrap()
            .unwrap()
            .as_int()
            .unwrap(),
        77
    );
    let mut code = Vec::new();
    op(&mut code, 64, 42);
    op(&mut code, 62, 1);
    code.push(60);
    let script = program(
        vec![
            sym(b"F", 5, 0, 3, 5, &0u32.to_le_bytes(), -1),
            sym(b"EXT", 5, 1, 11, 5, &u32::MAX.to_le_bytes(), -1),
            sym(b"ARG", 5, 1, 0, 0, &0u32.to_le_bytes(), -1),
        ],
        code,
    );
    for mode in 0..3 {
        let mut vm = Vm::from_script(
            &script,
            DecodeLimits::default(),
            Policy::default(),
            Limits::default(),
        )
        .unwrap();
        match mode {
            0 => vm.default_external_zero(None),
            1 => vm.default_external_raw(0),
            _ => vm
                .register_external(
                    id(&script, 1),
                    0,
                    &[Parameter::Value(DataType::Function)],
                    DataType::Function,
                )
                .unwrap(),
        }
        assert_eq!(
            vm.call(&mut App, id(&script, 0), vec![])
                .unwrap()
                .unwrap()
                .as_int()
                .unwrap(),
            [0, 88, 43][mode]
        );
        assert_eq!(vm.stack_len(), 0);
    }
}

#[test]
fn scalar_traps_replace_access_but_indexed_pushes_bypass_them() {
    let mut code = Vec::new();
    op(&mut code, 65, 2);
    code.push(60);
    code.push(245);
    word(&mut code, 2);
    code.extend([0, 60]);
    let script = program(
        vec![
            sym(b"SCALAR", 5, 0, 3, 2, &0u32.to_le_bytes(), -1),
            sym(b"INDEXED", 5, 0, 3, 2, &6u32.to_le_bytes(), -1),
            sym(b"X", 2, 1, 0, 0, &3u32.to_le_bytes(), -1),
        ],
        code,
    );
    struct App(usize);
    impl Host for App {
        type Error = Infallible;
        fn access_trap(
            &mut self,
            cx: &mut HostContext<'_, '_>,
            r: Reference,
        ) -> VmResult<Trap, Self::Error> {
            self.0 += 1;
            assert_eq!(r.index(), 0);
            cx.push(Value::Int(9))?;
            Ok(Trap::Handled)
        }
    }
    let mut vm = Vm::from_script(
        &script,
        DecodeLimits::default(),
        Policy::default(),
        Limits::default(),
    )
    .unwrap();
    vm.set_access_trap(id(&script, 2), true).unwrap();
    let mut host = App(0);
    assert_eq!(
        vm.call(&mut host, id(&script, 0), vec![])
            .unwrap()
            .unwrap()
            .as_int()
            .unwrap(),
        9
    );
    assert_eq!(
        vm.call(&mut host, id(&script, 1), vec![])
            .unwrap()
            .unwrap()
            .as_int()
            .unwrap(),
        3
    );
    assert_eq!(host.0, 1);
}

#[test]
fn host_error_sources_and_external_frames_survive_recovery_or_propagation() {
    #[derive(Debug)]
    struct HostFailure;
    impl std::fmt::Display for HostFailure {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("host failed")
        }
    }
    impl std::error::Error for HostFailure {}
    struct App {
        recover: bool,
        errors: usize,
    }
    impl Host for App {
        type Error = HostFailure;
        fn call(
            &mut self,
            _: &mut HostContext<'_, '_>,
            _: HostCall,
        ) -> VmResult<HostReturn, Self::Error> {
            Err(VmError::host(HostFailure))
        }
        fn exception(
            &mut self,
            cx: &mut HostContext<'_, '_>,
            e: &VmError<Self::Error>,
        ) -> VmResult<Recovery, Self::Error> {
            self.errors += 1;
            assert!(e.host_source().is_some());
            assert_eq!(e.frames.len(), 2);
            if self.recover {
                Ok(cx.vm().lenient_recovery(e)?)
            } else {
                Ok(Recovery::Fail)
            }
        }
    }
    let mut code = Vec::new();
    op(&mut code, 62, 1);
    code.push(60);
    let script = program(
        vec![
            sym(b"F", 5, 0, 3, 2, &0u32.to_le_bytes(), -1),
            sym(b"EXT", 5, 0, 11, 2, &u32::MAX.to_le_bytes(), -1),
        ],
        code,
    );
    let mut vm = Vm::from_script(
        &script,
        DecodeLimits::default(),
        Policy::default(),
        Limits::default(),
    )
    .unwrap();
    vm.register_external(id(&script, 1), 0, &[], DataType::Int)
        .unwrap();
    let mut host = App {
        recover: false,
        errors: 0,
    };
    let error = vm.call(&mut host, id(&script, 0), vec![]).unwrap_err();
    assert_eq!(error.host_source().unwrap().to_string(), "host failed");
    assert_eq!(error.pc, Some(0));
    assert_eq!(error.frames[0].symbol, id(&script, 1));
    assert_eq!((vm.call_depth(), vm.stack_len()), (0, 0));
    host.recover = true;
    assert_eq!(
        vm.call(&mut host, id(&script, 0), vec![])
            .unwrap()
            .unwrap()
            .as_int()
            .unwrap(),
        0
    );
    assert_eq!(host.errors, 2);
}

#[test]
fn member_setter_values_stay_charged_during_reentry_and_binding_caps_are_shared() {
    let script = program(
        vec![
            sym(b"C", 4, 0, 0, 0, &0u32.to_le_bytes(), -1),
            sym(b"C.S", 3, 1, 4, 0, &[], 0),
            sym(b"I", 7, 0, 1, 0, &0u32.to_le_bytes(), 0),
            sym(b"EXT", 5, 0, 9, 0, &u32::MAX.to_le_bytes(), -1),
        ],
        vec![60],
    );
    struct App;
    impl Host for App {
        type Error = Infallible;
        fn set_member(
            &mut self,
            cx: &mut HostContext<'_, '_>,
            _: HostKey,
            _: MemberKey,
            _: usize,
            v: Value,
        ) -> VmResult<(), Self::Error> {
            assert_eq!(v.as_bytes()?, b"four");
            assert_eq!(cx.string_limit(), 0);
            cx.vm().push_string(b"x")?;
            Ok(())
        }
    }
    let mut vm = Vm::from_script(
        &script,
        DecodeLimits::default(),
        Policy::default(),
        Limits {
            string_bytes: 4,
            bindings: 2,
            ..Limits::default()
        },
    )
    .unwrap();
    let specs = [MemberSpec {
        name: b"C.S",
        kind: MemberType::String,
        capacity: 1,
        key: 0,
    }];
    let class = vm.register_class(id(&script, 0), &specs).unwrap();
    assert_eq!(
        vm.register_external(id(&script, 3), 0, &[], DataType::Void)
            .unwrap_err()
            .kind,
        FaultKind::Limit(LimitKind::Bindings)
    );
    let instance = vm.allocate_host(id(&script, 2), class, HostKey(0)).unwrap();
    let reference = vm.reference(id(&script, 1), 0, Some(instance)).unwrap();
    assert_eq!(
        vm.write_reference(&mut App, reference, Value::Bytes(b"four".to_vec()))
            .unwrap_err()
            .fault
            .kind,
        FaultKind::Limit(LimitKind::Strings)
    );
    vm.push_string(b"four").unwrap();
    assert_eq!(
        vm.register_class(id(&script, 0), &[specs[0]; 3])
            .unwrap_err()
            .kind,
        FaultKind::Limit(LimitKind::Bindings)
    );
    assert_eq!(
        vm.state().registered_class(id(&script, 0)).unwrap(),
        Some(class)
    );
}

#[test]
fn opaque_members_use_exact_ids_for_unnamed_and_duplicate_names() {
    let mut unnamed = sym(b"", 2, 1, 4, 0, &[], 0);
    unnamed[..4].copy_from_slice(&0u32.to_le_bytes());
    unnamed.remove(4);
    let script = program(
        vec![
            sym(b"C", 4, 0, 0, 0, &0u32.to_le_bytes(), -1),
            unnamed,
            sym(b"SAME", 2, 1, 4, 0, &[], 0),
            sym(b"SAME", 3, 1, 4, 0, &[], 0),
            sym(b"I", 7, 0, 1, 0, &0u32.to_le_bytes(), 0),
        ],
        vec![60],
    );
    let mut state = RuntimeState::new(&script, Limits::default()).unwrap();
    let class = state.register_opaque(id(&script, 0)).unwrap();
    let instance = state.allocate_opaque(id(&script, 4), class).unwrap();
    for index in [1, 2] {
        state
            .set_opaque_member(
                instance,
                id(&script, index),
                0,
                Value::Int(index as i32 * 10),
            )
            .unwrap();
    }
    state
        .set_opaque_member(
            instance,
            id(&script, 3),
            0,
            Value::Bytes(b"unique".to_vec()),
        )
        .unwrap();
    assert_eq!(
        state
            .opaque_member(instance, id(&script, 1), 0)
            .unwrap()
            .as_int()
            .unwrap(),
        10
    );
    assert_eq!(
        state
            .opaque_member(instance, id(&script, 2), 0)
            .unwrap()
            .as_int()
            .unwrap(),
        20
    );
    assert_eq!(
        state
            .opaque_member(instance, id(&script, 3), 0)
            .unwrap()
            .as_bytes()
            .unwrap(),
        b"unique"
    );
    assert_eq!(
        state
            .bound_members(class)
            .unwrap()
            .iter()
            .map(|m| m.symbol)
            .collect::<Vec<_>>(),
        vec![id(&script, 1), id(&script, 2), id(&script, 3)]
    );
    let mut short = RuntimeState::new(
        &script,
        Limits {
            bindings: 3,
            ..Limits::default()
        },
    )
    .unwrap();
    assert_eq!(
        short.register_opaque(id(&script, 0)).unwrap_err().kind,
        FaultKind::Limit(LimitKind::Bindings)
    );
    assert_eq!(short.registered_class(id(&script, 0)).unwrap(), None);
}

#[test]
fn wire_faults_keep_dat_provenance_and_host_errors_keep_their_original_source() {
    let fault = Fault::from(zenkit::Error::new(
        zenkit::ErrorKind::UnexpectedEof,
        123,
        "synthetic DAT",
    ));
    let wire = fault.wire_source().unwrap();
    assert_eq!(
        (wire.kind, wire.offset, wire.context),
        (zenkit::ErrorKind::UnexpectedEof, Some(123), "synthetic DAT")
    );
    let source = std::error::Error::source(&fault).unwrap();
    assert_eq!(
        source.to_string(),
        "UnexpectedEof: synthetic DAT at byte 123"
    );
    let error = VmError::host(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "host I/O",
    ));
    assert_eq!(
        error.host_source().unwrap().kind(),
        std::io::ErrorKind::PermissionDenied
    );
    assert_eq!(
        std::error::Error::source(&error).unwrap().to_string(),
        "host I/O"
    );
    assert_eq!(error.pc, None);
}

#[test]
fn opaque_non_scalar_metadata_registers_without_inventing_accessible_values() {
    let mut symbols = vec![sym(b"C", 4, 0, 0, 0, &0u32.to_le_bytes(), -1)];
    for ty in 0..8 {
        symbols.push(sym(format!("C.M{ty}").as_bytes(), ty, 2, 4, 0, &[], 0));
    }
    symbols.push(sym(b"I", 7, 0, 1, 0, &0u32.to_le_bytes(), 0));
    let script = program(symbols, vec![60]);
    let mut vm = Vm::from_script(
        &script,
        DecodeLimits::default(),
        Policy::default(),
        Limits::default(),
    )
    .unwrap();
    let before = vm.state().cell_count();
    let class = vm.register_opaque(id(&script, 0)).unwrap();
    let instance = vm.allocate_opaque(id(&script, 9), class).unwrap();
    assert_eq!(vm.state().cell_count(), before + 9);
    assert_eq!(vm.state().bound_members(class).unwrap().len(), 8);
    for (raw, ty) in [
        (0, DataType::Void),
        (4, DataType::Class),
        (6, DataType::Prototype),
        (7, DataType::Instance),
    ] {
        let symbol = id(&script, raw + 1);
        let member = vm
            .state()
            .bound_members(class)
            .unwrap()
            .iter()
            .find(|m| m.symbol == symbol)
            .unwrap();
        assert_eq!((member.kind, member.count), (MemberType::NonScalar(ty), 2));
        assert_eq!(
            vm.state()
                .opaque_member(instance, symbol, 0)
                .unwrap_err()
                .kind,
            FaultKind::WrongType
        );
        assert_eq!(
            vm.set_opaque_member(instance, symbol, 0, Value::Int(9))
                .unwrap_err()
                .kind,
            FaultKind::WrongType
        );
        let reference = vm.reference(symbol, 0, Some(instance)).unwrap();
        assert_eq!(
            vm.read_reference(&mut NoHost, reference)
                .unwrap_err()
                .fault
                .kind,
            FaultKind::WrongType
        );
    }
    let mut fresh = RuntimeState::new(&script, Limits::default()).unwrap();
    assert_eq!(
        fresh
            .register_class(
                id(&script, 0),
                &[MemberSpec {
                    name: b"C.M2",
                    kind: MemberType::NonScalar(DataType::Int),
                    capacity: 2,
                    key: 0
                }]
            )
            .unwrap_err()
            .kind,
        FaultKind::Signature
    );
    assert_eq!(fresh.registered_class(id(&script, 0)).unwrap(), None);
}

#[test]
fn global_references_are_script_relative_while_captured_instances_are_runtime_owned() {
    let script = program(
        vec![
            sym(b"GLOBAL", 2, 1, 0, 0, &0u32.to_le_bytes(), -1),
            sym(b"C", 4, 0, 0, 0, &0u32.to_le_bytes(), -1),
            sym(b"I", 7, 0, 1, 0, &0u32.to_le_bytes(), 1),
        ],
        vec![60],
    );
    let mut a = Vm::from_script(
        &script,
        DecodeLimits::default(),
        Policy::default(),
        Limits::default(),
    )
    .unwrap();
    let mut b = Vm::from_script(
        &script,
        DecodeLimits::default(),
        Policy::default(),
        Limits::default(),
    )
    .unwrap();
    let reference = a.reference(id(&script, 0), 0, None).unwrap();
    b.set_global(id(&script, 0), 0, Value::Int(19)).unwrap();
    assert_eq!(
        b.read_reference(&mut NoHost, reference)
            .unwrap()
            .as_int()
            .unwrap(),
        19
    );
    let class = a.register_opaque(id(&script, 1)).unwrap();
    let instance = a.allocate_opaque(id(&script, 2), class).unwrap();
    let captured = a.reference(id(&script, 0), 0, Some(instance)).unwrap();
    assert_eq!(
        b.push_ref(captured).unwrap_err().kind,
        FaultKind::InvalidHandle
    );
    assert_eq!(
        b.read_reference(&mut NoHost, captured)
            .unwrap_err()
            .fault
            .kind,
        FaultKind::InvalidHandle
    );
    assert_eq!(
        b.write_reference(&mut NoHost, captured, Value::Int(1))
            .unwrap_err()
            .fault
            .kind,
        FaultKind::InvalidHandle
    );
    println!(
        "Fault={} VmError<Infallible>={} VmError<io::Error>={}",
        std::mem::size_of::<Fault>(),
        std::mem::size_of::<VmError<Infallible>>(),
        std::mem::size_of::<VmError<std::io::Error>>()
    );
}
