//! Public-consumer controls for diagnostic ownership, precision and propagation.
use std::{any::TypeId, convert::Infallible};
use zenkit::{
    ErrorKind,
    daedalus::{self, DataType, DecodeLimits, Script, SymbolId, addon::*, vm::*},
};

fn word(out: &mut Vec<u8>, n: u32) {
    out.extend(n.to_le_bytes());
}
fn symbol(
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
    out.extend(name);
    out.push(10);
    word(&mut out, vary);
    word(&mut out, ty << 12 | count | flags << 16);
    out.extend([0u8; 20]);
    out.extend(data);
    word(&mut out, parent as u32);
    out
}
fn class(name: &[u8]) -> Vec<u8> {
    symbol(name, 4, 0, 0, 0, &0u32.to_le_bytes(), -1)
}
fn member(name: &[u8], ty: u32, count: u32, parent: i32) -> Vec<u8> {
    symbol(name, ty, count, 4, 0, &[], parent)
}
fn function(name: &[u8], count: u32, flags: u32, result: u32, pc: u32) -> Vec<u8> {
    symbol(name, 5, count, flags, result, &pc.to_le_bytes(), -1)
}
fn script(symbols: Vec<Vec<u8>>, code: &[u8]) -> Script {
    let mut bytes = vec![50];
    word(&mut bytes, symbols.len() as u32);
    for n in 0..symbols.len() {
        word(&mut bytes, n as u32);
    }
    for s in symbols {
        bytes.extend(s);
    }
    word(&mut bytes, code.len() as u32);
    bytes.extend(code);
    Script::from_bytes(&bytes, daedalus::Limits::default()).unwrap()
}
fn id(s: &Script, index: u32) -> SymbolId {
    s.symbol(index).unwrap().id()
}
fn new_vm(s: &Script) -> Vm<'_> {
    Vm::from_script(
        s,
        DecodeLimits::default(),
        Policy::default(),
        Limits::default(),
    )
    .unwrap()
}
fn op(code: &mut Vec<u8>, opcode: u8, value: u32) {
    code.push(opcode);
    word(code, value);
}
fn spec(name: &[u8], kind: MemberType, capacity: usize, key: u32) -> MemberSpec<'_> {
    MemberSpec {
        name,
        kind,
        capacity,
        key,
    }
}

#[test]
fn requested_names_are_owned_raw_bounded_and_do_not_mask_not_found() {
    for (name, cap) in [
        (vec![], 0),
        (vec![0, 255, b'x'], 3),
        (vec![0, 255, b'x'], 2),
        (vec![7], 0),
        (vec![b'x'; 100_000], 100_000),
    ] {
        let error = {
            let s = script(vec![], &[]);
            let mut vm = Vm::from_script(
                &s,
                DecodeLimits::default(),
                Policy::default(),
                Limits {
                    diagnostic_name_bytes: cap,
                    ..Limits::default()
                },
            )
            .unwrap();
            let mut host = NoHost;
            let error = vm.call_named(&mut host, &name, vec![]).unwrap_err();
            assert_eq!(vm.stack_len(), 0);
            error
        };
        assert_eq!(error.fault.kind, FaultKind::NotFound);
        assert!(!error.fault.is_terminal());
        assert_eq!((error.fault.symbol, error.fault.index), (None, None));
        match error.fault.detail().unwrap() {
            FaultDetail::MissingName {
                operation: FaultOperation::Call,
                name: captured,
            } => {
                assert_eq!(captured.original_len(), name.len());
                if name.len() <= cap {
                    assert_eq!(captured.bytes(), Some(name.as_slice()));
                } else {
                    assert!(
                        matches!(captured, RequestedName::Unavailable { reason: NameCaptureFailure::Limit { limit }, .. } if *limit == cap)
                    );
                }
            }
            other => panic!("unexpected detail: {other:?}"),
        }
        assert!(format!("{error:?}").len() < 700);
        assert!(format!("{error}").len() < 500);
    }
}

#[test]
fn missing_member_name_capture_and_optional_lookups_keep_their_contracts() {
    let s = script(vec![class(b"C")], &[]);
    let mut vm = new_vm(&s);
    let requested = vec![0, 255, b'z'];
    let error = vm
        .register_class(id(&s, 0), &[spec(&requested, MemberType::Int, 1, 0)])
        .unwrap_err();
    assert_eq!(error.kind, FaultKind::NotFound);
    assert!(
        matches!(error.detail(), Some(FaultDetail::MissingName { operation: FaultOperation::RegisterClass, name }) if name.bytes() == Some(requested.as_slice()))
    );
    assert_eq!(vm.state().binding_count(), 0);
    assert!(
        vm.register_external_named(b"absent", 0, &[], DataType::Void)
            .unwrap()
            .is_none()
    );
    assert!(
        ClassBinding::bind_present_detailed::<TestClass>(&s, 0)
            .unwrap()
            .is_none()
    );
    assert!(
        RegisteredAddon::register::<TestClass>(&mut vm, 0)
            .unwrap()
            .is_none()
    );
    assert!(vm.state().resolve_function(-1).unwrap().is_none());
    assert!(vm.state().resolve_function(0).unwrap().is_none());
}

#[test]
fn value_access_reports_actual_constraints_including_storage_alternatives() {
    assert_eq!(Value::Function(42).as_int().unwrap(), 42);
    assert_eq!(Value::Int(0x3f80_0000).as_float().unwrap(), 1.0);
    for (error, expected, given) in [
        (
            Value::Bytes(vec![]).as_int().unwrap_err(),
            ExpectedType::IntOrFunction,
            DataType::String,
        ),
        (
            Value::Function(1).as_float().unwrap_err(),
            ExpectedType::FloatOrIntegerBits,
            DataType::Function,
        ),
        (
            Value::Int(1).as_bytes().unwrap_err(),
            ExpectedType::Exact(DataType::String),
            DataType::Int,
        ),
        (
            Value::Float(1.0).as_instance().unwrap_err(),
            ExpectedType::Exact(DataType::Instance),
            DataType::Float,
        ),
    ] {
        assert_eq!(error.kind, FaultKind::WrongType);
        assert!(
            matches!(error.detail(), Some(FaultDetail::TypeMismatch { operation: FaultOperation::ValueAccess, expected: actual, given: GivenType::Value(ty) }) if *actual == expected && *ty == given)
        );
    }
}

#[test]
fn registration_reasons_preserve_precedence_raw_parents_and_atomicity() {
    for parent in [-1, i32::MAX, 2] {
        let s = script(
            vec![class(b"C"), member(b"C.X", 3, 2, parent), class(b"D")],
            &[],
        );
        let mut state = RuntimeState::new(&s, Limits::default()).unwrap();
        let error = state
            .register_class(id(&s, 0), &[spec(b"C.X", MemberType::Float, 0, 1)])
            .unwrap_err();
        assert_eq!(error.kind, FaultKind::Signature);
        assert_eq!(error.symbol, Some(id(&s, 1)));
        assert!(
            matches!(error.detail(), Some(FaultDetail::Registration { class, reason: RegistrationReason::Parent { given } }) if *class == id(&s, 0) && *given == parent)
        );
        assert_eq!(state.binding_count(), 0);
    }
    let s = script(
        vec![
            class(b"C"),
            member(b"C.X", 2, 2, 0),
            symbol(b"GLOBAL", 2, 1, 0, 0, &0u32.to_le_bytes(), -1),
        ],
        &[],
    );
    let mut state = RuntimeState::new(&s, Limits::default()).unwrap();
    for (name, kind, capacity, expected) in [
        (
            b"GLOBAL".as_slice(),
            MemberType::Float,
            0,
            RegistrationReason::NotMember {
                given: s.symbol(2).unwrap().into(),
            },
        ),
        (
            b"C.X".as_slice(),
            MemberType::Float,
            0,
            RegistrationReason::Type {
                expected: MemberType::Float,
                given: DataType::Int,
            },
        ),
        (
            b"C.X".as_slice(),
            MemberType::Int,
            1,
            RegistrationReason::Capacity {
                capacity: 1,
                elements: 2,
            },
        ),
    ] {
        let error = state
            .register_class(id(&s, 0), &[spec(name, kind, capacity, 0)])
            .unwrap_err();
        assert!(
            matches!(error.detail(), Some(FaultDetail::Registration { reason, .. }) if *reason == expected)
        );
        assert_eq!(state.binding_count(), 0);
    }
    let duplicate = state
        .register_class(
            id(&s, 0),
            &[
                spec(b"C.X", MemberType::Int, 2, 0),
                spec(b"C.X", MemberType::Int, 2, 1),
            ],
        )
        .unwrap_err();
    assert_eq!(duplicate.symbol, Some(id(&s, 0)));
    assert!(
        matches!(duplicate.detail(), Some(FaultDetail::Registration { reason: RegistrationReason::Duplicate { member }, .. }) if *member == id(&s, 1))
    );
    let registered = state
        .register_class(id(&s, 0), &[spec(b"C.X", MemberType::Int, 2, 0)])
        .unwrap();
    let changed = state
        .register_class(id(&s, 0), &[spec(b"C.X", MemberType::Int, 2, 1)])
        .unwrap_err();
    assert_eq!(changed.kind, FaultKind::WrongClass);
    assert!(matches!(
        changed.detail(),
        Some(FaultDetail::Registration {
            reason: RegistrationReason::ChangedLayout,
            ..
        })
    ));
    assert_eq!(state.bound_members(registered).unwrap()[0].key, 0);
}

#[test]
fn symbol_index_constant_context_and_wrong_class_facts_survive() {
    let s = script(
        vec![
            class(b"C"),
            member(b"C.X", 2, 1, 0),
            class(b"D"),
            symbol(b"I", 7, 0, 1, 0, &0u32.to_le_bytes(), 2),
            symbol(b"G", 2, 1, 1, 0, &9u32.to_le_bytes(), -1),
        ],
        &[60],
    );
    let mut vm = new_vm(&s);
    let c = vm
        .register_class(id(&s, 0), &[spec(b"C.X", MemberType::Int, 1, 0)])
        .unwrap();
    let d = vm.register_class(id(&s, 2), &[]).unwrap();
    let instance = vm.allocate_host(id(&s, 3), d, HostKey(0)).unwrap();
    let wrong = vm
        .read_reference(
            &mut NoHost,
            vm.reference(id(&s, 1), 0, Some(instance)).unwrap(),
        )
        .unwrap_err();
    assert_eq!(wrong.fault.kind, FaultKind::WrongClass);
    assert!(
        matches!(wrong.fault.detail(), Some(FaultDetail::ClassMismatch { expected, given: Some(given), instance: Some(actual) }) if *expected == id(&s, 0) && *given == id(&s, 2) && *actual == instance)
    );
    let no_context = vm
        .read_reference(&mut NoHost, vm.reference(id(&s, 1), 0, None).unwrap())
        .unwrap_err();
    assert_eq!(no_context.fault.symbol, Some(id(&s, 1)));
    assert!(matches!(
        no_context.fault.detail(),
        Some(FaultDetail::Access {
            operation: FaultOperation::ReferenceRead
        })
    ));
    let index = vm.state().global(id(&s, 4), 1).unwrap_err();
    assert_eq!((index.symbol, index.index), (Some(id(&s, 4)), Some(1)));
    assert!(matches!(
        index.detail(),
        Some(FaultDetail::Index {
            operation: FaultOperation::GlobalRead,
            length: 1
        })
    ));
    let constant = vm
        .write_reference(
            &mut NoHost,
            vm.reference(id(&s, 4), 0, None).unwrap(),
            Value::Float(0.0),
        )
        .unwrap_err();
    assert_eq!(constant.fault.kind, FaultKind::Constant);
    assert!(matches!(
        constant.fault.detail(),
        Some(FaultDetail::Access {
            operation: FaultOperation::ReferenceWrite
        })
    ));
    assert_eq!(vm.state().class_symbol(c).unwrap(), id(&s, 0));
}

#[test]
fn stack_conversion_keeps_referenced_symbol_and_wire_source() {
    let s = script(vec![symbol(b"TEXT", 3, 1, 0, 0, b"raw\xff\n", -1)], &[]);
    let mut vm = new_vm(&s);
    vm.push_ref(vm.reference(id(&s, 0), 0, None).unwrap())
        .unwrap();
    let error = vm.pop_int(&mut NoHost).unwrap_err();
    assert_eq!(error.fault.symbol, Some(id(&s, 0)));
    assert!(matches!(
        error.fault.detail(),
        Some(FaultDetail::TypeMismatch {
            operation: FaultOperation::StackPop,
            expected: ExpectedType::IntOrFunction,
            given: GivenType::Value(DataType::String)
        })
    ));
    assert_eq!(vm.stack_len(), 0);
    let wire = Fault::from(zenkit::Error::new(
        ErrorKind::UnexpectedEof,
        17,
        "fixture DAT",
    ))
    .with_detail(FaultDetail::Address { requested: 9 });
    assert_eq!(wire.kind, FaultKind::Wire(ErrorKind::UnexpectedEof));
    assert_eq!(wire.wire_source().unwrap().offset, Some(17));
    assert!(std::error::Error::source(&wire).is_some());
}

#[test]
fn external_signatures_separate_parameter_position_array_index_and_result() {
    let s = script(
        vec![
            function(b"EXT", 1, 11, 2, u32::MAX),
            symbol(b"EXT.P", 2, 1, 0, 0, &0u32.to_le_bytes(), 0),
        ],
        &[],
    );
    let mut vm = new_vm(&s);
    let result = vm
        .register_external(id(&s, 0), 0, &[], DataType::Float)
        .unwrap_err();
    assert!(
        matches!(result.detail(), Some(FaultDetail::Return { callable: Some(c), expected: ReturnExpectation::Type(DataType::Int), given: GivenType::Type(DataType::Float) }) if *c == id(&s, 0))
    );
    let count = vm
        .register_external(id(&s, 0), 0, &[], DataType::Int)
        .unwrap_err();
    assert!(matches!(
        count.detail(),
        Some(FaultDetail::Counts {
            role: CountRole::Arguments,
            expected: 1,
            given: 0,
            ..
        })
    ));
    let param = vm
        .register_external(
            id(&s, 0),
            0,
            &[Parameter::Value(DataType::String)],
            DataType::Int,
        )
        .unwrap_err();
    assert_eq!((param.symbol, param.index), (Some(id(&s, 1)), None));
    assert!(
        matches!(param.detail(), Some(FaultDetail::ParameterType { callable, position: 0, expected: DataType::Int, given: GivenType::Parameter(Parameter::Value(DataType::String)), .. }) if *callable == id(&s, 0))
    );
    vm.register_external(
        id(&s, 0),
        0,
        &[Parameter::Value(DataType::Function)],
        DataType::Int,
    )
    .unwrap();
}

#[test]
fn runtime_external_argument_context_preserves_cause_after_unwind() {
    let mut code = Vec::new();
    op(&mut code, 65, 3);
    op(&mut code, 62, 1);
    code.push(60);
    let s = script(
        vec![
            function(b"MAIN", 0, 1, 0, 0),
            function(b"EXT", 1, 9, 0, u32::MAX),
            symbol(b"EXT.P", 2, 1, 0, 0, &0u32.to_le_bytes(), 1),
            symbol(b"TEXT", 3, 1, 0, 0, b"x\n", -1),
        ],
        &code,
    );
    let mut vm = new_vm(&s);
    vm.register_external(
        id(&s, 1),
        0,
        &[Parameter::Value(DataType::Int)],
        DataType::Void,
    )
    .unwrap();
    let error = vm.call(&mut NoHost, id(&s, 0), vec![]).unwrap_err();
    assert_eq!(error.fault.kind, FaultKind::WrongType);
    assert_eq!(error.fault.symbol, Some(id(&s, 3)));
    assert_eq!(
        error.argument_context(),
        Some(&ArgumentContext {
            callable: id(&s, 1),
            position: 0,
            parameter: Some(id(&s, 2))
        })
    );
    assert!(matches!(
        error.fault.detail(),
        Some(FaultDetail::TypeMismatch {
            operation: FaultOperation::StackPop,
            given: GivenType::Value(DataType::String),
            ..
        })
    ));
    assert_eq!(error.pc, Some(5));
    assert_eq!(error.frames.len(), 2);
    assert_eq!((vm.stack_len(), vm.call_depth()), (0, 0));
}

#[test]
fn callback_return_tags_and_strict_stack_counts_remain_distinct() {
    struct WrongReturn;
    impl Host for WrongReturn {
        type Error = Infallible;
        fn call(
            &mut self,
            _: &mut HostContext<'_, '_>,
            _: HostCall,
        ) -> VmResult<HostReturn, Self::Error> {
            Ok(HostReturn::Value(Value::Float(1.0)))
        }
    }
    let mut code = Vec::new();
    op(&mut code, 62, 1);
    code.push(60);
    let s = script(
        vec![
            function(b"MAIN", 0, 1, 0, 0),
            function(b"EXT", 0, 11, 2, u32::MAX),
        ],
        &code,
    );
    let mut vm = new_vm(&s);
    vm.register_external(id(&s, 1), 0, &[], DataType::Int)
        .unwrap();
    let error = vm.call(&mut WrongReturn, id(&s, 0), vec![]).unwrap_err();
    assert_eq!(error.fault.kind, FaultKind::InvalidReturn);
    assert!(
        matches!(error.fault.detail(), Some(FaultDetail::Return { callable: Some(c), expected: ReturnExpectation::Type(DataType::Int), given: GivenType::Value(DataType::Float) }) if *c == id(&s, 1))
    );
    let s = script(vec![function(b"EMPTY", 0, 3, 2, 0)], &[60]);
    let mut vm = new_vm(&s);
    vm.set_policy(Policy {
        repair_returns: false,
        ..Policy::default()
    });
    let error = vm.call(&mut NoHost, id(&s, 0), vec![]).unwrap_err();
    assert!(matches!(
        error.fault.detail(),
        Some(FaultDetail::Counts {
            operation: FaultOperation::Return,
            role: CountRole::ReturnStack,
            expected: 1,
            given: 0
        })
    ));
}

#[derive(Default)]
struct TestClass {
    value: i32,
}
const SCHEMA: &ClassSchema = &ClassSchema {
    name: b"C_TEST",
    members: &[
        Member {
            name: b"C_TEST.X",
            kind: MemberKind::Int,
            capacity: 1,
            presence_guard: None,
        },
        Member {
            name: b"C_TEST.OPTIONAL",
            kind: MemberKind::String,
            capacity: 1,
            presence_guard: Some(b"GUARD"),
        },
    ],
};
impl AddonClass for TestClass {
    const SCHEMA: &'static ClassSchema = SCHEMA;
    fn read_member(&self, ordinal: usize, _: usize) -> zenkit::Result<MemberValue<'_>> {
        if ordinal == 0 {
            Ok(MemberValue::Int(self.value))
        } else {
            Err(zenkit::Error::operation(ErrorKind::NotFound, "custom read"))
        }
    }
    fn write_member(
        &mut self,
        _: usize,
        _: usize,
        value: MemberValue<'_>,
        _: usize,
    ) -> zenkit::Result<()> {
        // Custom storage intentionally accepts float-to-int conversion.
        match value {
            MemberValue::Int(n) => self.value = n,
            MemberValue::Float(n) => self.value = n as i32,
            _ => {
                return Err(zenkit::Error::new(
                    ErrorKind::WrongObjectType,
                    23,
                    "custom write",
                ));
            }
        }
        Ok(())
    }
}
#[derive(Default)]
struct OtherType;
impl AddonClass for OtherType {
    const SCHEMA: &'static ClassSchema = SCHEMA;
    fn read_member(&self, _: usize, _: usize) -> zenkit::Result<MemberValue<'_>> {
        panic!("wrong type must fail before callback")
    }
    fn write_member(
        &mut self,
        _: usize,
        _: usize,
        _: MemberValue<'_>,
        _: usize,
    ) -> zenkit::Result<()> {
        panic!("wrong type must fail before callback")
    }
}

#[test]
fn detailed_addon_access_preserves_custom_successes_and_legacy_errors() {
    let s = script(vec![class(b"C_TEST"), member(b"C_TEST.X", 2, 1, 0)], &[]);
    let binding = ClassBinding::bind_detailed::<TestClass>(&s, 2).unwrap();
    let error = binding
        .read_detailed(&OtherType, usize::MAX, usize::MAX)
        .unwrap_err();
    assert_eq!(error.error().kind, ErrorKind::WrongObjectType);
    match error.diagnostic() {
        BindingDiagnostic::HostType {
            class,
            expected,
            given,
        } => {
            assert_eq!(*class, id(&s, 0));
            assert_eq!(expected.id(), TypeId::of::<TestClass>());
            assert_eq!(given.id(), TypeId::of::<OtherType>());
            assert!(expected.name().ends_with("TestClass"));
        }
        other => panic!("unexpected detail: {other:?}"),
    }
    assert!(matches!(
        binding
            .read_detailed(&TestClass::default(), 2, 0)
            .unwrap_err()
            .diagnostic(),
        BindingDiagnostic::Ordinal {
            ordinal: 2,
            fields: 2,
            ..
        }
    ));
    assert!(
        matches!(binding.read_detailed(&TestClass::default(), 1, 99).unwrap_err().diagnostic(), BindingDiagnostic::AbsentMember { ordinal: 1, name, .. } if name.as_bytes() == b"C_TEST.OPTIONAL")
    );
    assert!(matches!(
        binding
            .read_detailed(&TestClass::default(), 0, 1)
            .unwrap_err()
            .diagnostic(),
        BindingDiagnostic::Index {
            index: 1,
            length: 1,
            ..
        }
    ));
    let mut host = TestClass::default();
    binding
        .write_detailed(&mut host, 0, 0, MemberValue::Float(7.0), 0)
        .unwrap();
    assert_eq!(host.value, 7);
    let error = binding
        .write_detailed(&mut host, 0, 0, MemberValue::Bytes(b"no"), 0)
        .unwrap_err();
    assert!(matches!(
        error.diagnostic(),
        BindingDiagnostic::ValueType {
            expected: MemberKind::Int,
            given: MemberKind::String,
            ..
        }
    ));
    assert_eq!(error.error().offset, Some(23));
    assert!(std::error::Error::source(&error).is_some());
    assert_eq!(
        binding
            .write(&mut host, 0, 0, MemberValue::Bytes(b"no"), 0)
            .unwrap_err()
            .offset,
        Some(23)
    );
    assert_eq!(host.value, 7);
}

#[test]
fn addon_registration_errors_reach_vm_without_losing_wire_provenance() {
    let empty = script(vec![], &[]);
    let missing = ClassBinding::bind_detailed::<TestClass>(&empty, 2).unwrap_err();
    assert!(
        matches!(missing.diagnostic(), BindingDiagnostic::MissingClass { name } if name.as_bytes() == b"C_TEST")
    );
    for (symbols, expected_kind, reason) in [
        (vec![class(b"C_TEST")], ErrorKind::NotFound, 0),
        (
            vec![class(b"C_TEST"), member(b"C_TEST.X", 2, 1, -77)],
            ErrorKind::WrongObjectType,
            1,
        ),
        (
            vec![class(b"C_TEST"), member(b"C_TEST.X", 3, 1, 0)],
            ErrorKind::WrongObjectType,
            2,
        ),
        (
            vec![class(b"C_TEST"), member(b"C_TEST.X", 2, 2, 0)],
            ErrorKind::InvalidData,
            3,
        ),
    ] {
        let s = script(symbols, &[]);
        let mut vm = new_vm(&s);
        let fault = RegisteredAddon::register::<TestClass>(&mut vm, 2).unwrap_err();
        assert_eq!(fault.kind, FaultKind::Wire(expected_kind));
        assert_eq!(
            fault.wire_source().unwrap().context,
            "Daedalus addon member"
        );
        assert_eq!((fault.symbol, fault.index), (None, None));
        match (reason, fault.detail()) {
            (
                0,
                Some(FaultDetail::Addon(BindingDiagnostic::MissingMember {
                    name, ordinal: 0, ..
                })),
            ) => assert_eq!(name.as_bytes(), b"C_TEST.X"),
            (
                1,
                Some(FaultDetail::Addon(BindingDiagnostic::Member {
                    reason: BindingMemberFailure::Parent { given: -77 },
                    ..
                })),
            )
            | (
                2,
                Some(FaultDetail::Addon(BindingDiagnostic::Member {
                    reason:
                        BindingMemberFailure::Type {
                            expected: MemberKind::Int,
                            given: DataType::String,
                        },
                    ..
                })),
            )
            | (
                3,
                Some(FaultDetail::Addon(BindingDiagnostic::Member {
                    reason:
                        BindingMemberFailure::Capacity {
                            capacity: 1,
                            elements: 2,
                        },
                    ..
                })),
            ) => {}
            other => panic!("unexpected detail: {other:?}"),
        }
        assert_eq!(vm.state().binding_count(), 0);
    }
}

#[test]
fn shared_address_override_uses_the_registered_signature_owner() {
    let mut code = Vec::new();
    op(&mut code, 65, 5);
    op(&mut code, 61, 11);
    code.extend([60, 60]);
    let s = script(
        vec![
            function(b"MAIN", 0, 1, 0, 0),
            function(b"A", 1, 1, 0, 11),
            symbol(b"A.P", 2, 1, 0, 0, &0u32.to_le_bytes(), 1),
            function(b"B", 1, 1, 0, 11),
            symbol(b"B.P", 3, 1, 0, 0, b"\n", 3),
            symbol(b"TEXT", 3, 1, 0, 0, b"x\n", -1),
        ],
        &code,
    );
    assert_eq!(s.find_address(11).unwrap().id(), id(&s, 3));
    let mut vm = new_vm(&s);
    vm.override_function(
        id(&s, 1),
        0,
        &[Parameter::Value(DataType::Int)],
        DataType::Void,
    )
    .unwrap();
    let error = vm.call(&mut NoHost, id(&s, 0), vec![]).unwrap_err();
    assert_eq!(error.fault.kind, FaultKind::WrongType);
    assert_eq!(error.fault.symbol, Some(id(&s, 5)));
    assert_eq!(
        error.argument_context(),
        Some(&ArgumentContext {
            callable: id(&s, 1),
            position: 0,
            parameter: Some(id(&s, 2))
        })
    );
}

#[test]
fn invalid_instance_ancestry_keeps_requested_symbol_and_exact_wire_offset() {
    for parent in [-1, i32::MAX, 1] {
        let s = script(
            vec![
                class(b"C"),
                symbol(b"P", 6, 0, 1, 0, &0u32.to_le_bytes(), parent),
                symbol(b"I", 7, 0, 1, 0, &0u32.to_le_bytes(), 1),
            ],
            &[60],
        );
        let baseline = s.class_of(id(&s, 2)).unwrap_err();
        let mut vm = new_vm(&s);
        let class = vm.register_class(id(&s, 0), &[]).unwrap();
        let fault = vm.allocate_host(id(&s, 2), class, HostKey(0)).unwrap_err();
        assert_eq!(fault.kind, FaultKind::Wire(baseline.kind));
        assert_eq!(fault.symbol, Some(id(&s, 2)));
        let wire = fault.wire_source().unwrap();
        assert_eq!(
            (wire.offset, wire.context),
            (baseline.offset, baseline.context)
        );
        assert!(matches!(
            fault.detail(),
            Some(FaultDetail::Access {
                operation: FaultOperation::InstanceBinding
            })
        ));
        let fault = vm
            .init_opaque_instance(&mut NoHost, id(&s, 2))
            .unwrap_err()
            .fault;
        assert_eq!(fault.symbol, Some(id(&s, 2)));
        assert_eq!(fault.wire_source().unwrap().offset, baseline.offset);
        assert_eq!(vm.state().instance_count(), 0);
    }
}

#[test]
fn external_argument_failure_keeps_preexisting_host_cause_and_trace() {
    struct FailingHost {
        origin: SymbolId,
    }
    impl Host for FailingHost {
        type Error = std::io::Error;
        fn get_member(
            &mut self,
            _: &mut HostContext<'_, '_>,
            _: HostKey,
            _: MemberKey,
            _: usize,
        ) -> VmResult<Value, Self::Error> {
            let mut error = VmError::host(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "retained host error",
            ));
            error.fault = error
                .fault
                .at(self.origin)
                .element(7)
                .with_detail(FaultDetail::Address { requested: 999 });
            error.pc = Some(777);
            error.frames.push(FrameTrace {
                symbol: self.origin,
                return_pc: 321,
                stack_base: 0,
                context: None,
            });
            Err(error)
        }
    }
    let mut code = Vec::new();
    op(&mut code, 65, 4);
    op(&mut code, 62, 1);
    code.push(60);
    let s = script(
        vec![
            function(b"MAIN", 0, 1, 0, 0),
            function(b"EXT", 1, 9, 0, u32::MAX),
            symbol(b"EXT.P", 2, 1, 0, 0, &0u32.to_le_bytes(), 1),
            class(b"C"),
            member(b"C.X", 2, 1, 3),
            symbol(b"I", 7, 0, 1, 0, &0u32.to_le_bytes(), 3),
        ],
        &code,
    );
    let mut vm = new_vm(&s);
    let class = vm
        .register_class(id(&s, 3), &[spec(b"C.X", MemberType::Int, 1, 0)])
        .unwrap();
    let instance = vm.allocate_host(id(&s, 5), class, HostKey(0)).unwrap();
    vm.set_current_instance(Some(instance)).unwrap();
    vm.register_external(
        id(&s, 1),
        0,
        &[Parameter::Value(DataType::Int)],
        DataType::Void,
    )
    .unwrap();
    let error = vm
        .call(&mut FailingHost { origin: id(&s, 4) }, id(&s, 0), vec![])
        .unwrap_err();
    assert_eq!(error.fault.kind, FaultKind::Host);
    assert_eq!(
        (error.fault.symbol, error.fault.index),
        (Some(id(&s, 4)), Some(7))
    );
    assert!(matches!(
        error.fault.detail(),
        Some(FaultDetail::Address { requested: 999 })
    ));
    assert_eq!(
        error.host_source().unwrap().kind(),
        std::io::ErrorKind::PermissionDenied
    );
    assert_eq!(error.pc, Some(777));
    assert_eq!(error.frames.len(), 1);
    assert_eq!(error.frames[0].return_pc, 321);
    assert_eq!(error.argument_context().unwrap().parameter, Some(id(&s, 2)));
    assert_eq!(
        (vm.call_depth(), vm.stack_len(), vm.current_instance()),
        (0, 0, Some(instance))
    );
}

#[test]
fn inline_diagnostics_and_iterative_frames_fit_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            // Recursive script frames are iterative; their limit error crosses the
            // same large-error paths without requiring recursive Rust stack growth.
            let mut code = Vec::new();
            op(&mut code, 61, 0);
            code.push(60);
            let s = script(vec![function(b"RECURSE", 0, 1, 0, 0)], &code);
            let mut vm = Vm::from_script(
                &s,
                DecodeLimits::default(),
                Policy::default(),
                Limits {
                    frames: 512,
                    trace_frames: 2,
                    ..Limits::default()
                },
            )
            .unwrap();
            let error = vm.call(&mut NoHost, id(&s, 0), vec![]).unwrap_err();
            assert_eq!(error.fault.kind, FaultKind::Limit(LimitKind::Frames));
            assert!(error.trace_truncated);
            assert_eq!(error.frames.len(), 2);
            assert_eq!((vm.call_depth(), vm.stack_len()), (0, 0));
            let layout = [
                std::mem::size_of::<Fault>(),
                std::mem::size_of::<VmError<Infallible>>(),
                std::mem::size_of::<BindingError>(),
            ];
            eprintln!(
                "diagnostic layout bytes (Fault, VmError<Infallible>, BindingError): {layout:?}"
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn declared_call_arguments_retain_callable_and_zero_based_position() {
    let s = script(
        vec![
            function(b"F", 1, 1, 0, 0),
            symbol(b"F.P", 2, 1, 0, 0, &0u32.to_le_bytes(), 0),
        ],
        &[60],
    );
    let mut vm = new_vm(&s);
    let error = vm
        .call(
            &mut NoHost,
            id(&s, 0),
            vec![Argument::Value(Value::Float(1.0))],
        )
        .unwrap_err();
    assert_eq!(
        (error.fault.kind, error.fault.symbol, error.fault.index),
        (FaultKind::Signature, Some(id(&s, 1)), None)
    );
    assert!(
        matches!(error.fault.detail(), Some(FaultDetail::ParameterType { operation: FaultOperation::Call, callable, position: 0, expected: DataType::Int, given: GivenType::Value(DataType::Float) }) if *callable == id(&s, 0))
    );
    let error = vm.call(&mut NoHost, id(&s, 0), vec![]).unwrap_err();
    assert!(matches!(
        error.fault.detail(),
        Some(FaultDetail::Counts {
            operation: FaultOperation::Call,
            role: CountRole::Arguments,
            expected: 1,
            given: 0
        })
    ));
    vm.call(
        &mut NoHost,
        id(&s, 0),
        vec![Argument::Value(Value::Function(3))],
    )
    .unwrap();
}

#[test]
fn detailed_addon_branch_precedence_and_bridge_access_keep_old_kinds() {
    let s = script(
        vec![
            class(b"C_TEST"),
            symbol(b"C_TEST.X", 2, 1, 0, 0, &0u32.to_le_bytes(), -1),
        ],
        &[],
    );
    let error = ClassBinding::bind_detailed::<TestClass>(&s, 2).unwrap_err();
    assert!(matches!(
        error.diagnostic(),
        BindingDiagnostic::Member {
            reason: BindingMemberFailure::NotMember,
            ..
        }
    ));
    let error = ClassBinding::bind_detailed::<TestClass>(&s, 1).unwrap_err();
    assert!(matches!(
        error.diagnostic(),
        BindingDiagnostic::MemberLimit {
            requested: 2,
            limit: 1
        }
    ));
    assert_eq!(error.error().kind, ErrorKind::LimitExceeded);
    let s = script(
        vec![symbol(b"C_TEST", 2, 1, 0, 0, &0u32.to_le_bytes(), -1)],
        &[],
    );
    let error = ClassBinding::bind_detailed::<TestClass>(&s, 0).unwrap_err();
    assert!(matches!(
        error.diagnostic(),
        BindingDiagnostic::ClassRole {
            data_type: DataType::Int,
            member: false,
            ..
        }
    ));
    let s = script(vec![class(b"C_TEST"), member(b"C_TEST.X", 2, 1, 0)], &[]);
    let mut vm = new_vm(&s);
    let registered = RegisteredAddon::register::<TestClass>(&mut vm, 2)
        .unwrap()
        .unwrap();
    let key = vm.state().bound_members(registered.class()).unwrap()[0];
    let fault = registered.read(&OtherType, key, 0, 0).unwrap_err();
    assert_eq!(fault.kind, FaultKind::Wire(ErrorKind::WrongObjectType));
    assert!(matches!(
        fault.detail(),
        Some(FaultDetail::Addon(BindingDiagnostic::HostType { .. }))
    ));
    let fault = registered
        .write(&mut OtherType, key, 0, &Value::Instance(None), 0)
        .unwrap_err();
    assert_eq!(fault.kind, FaultKind::WrongType);
    assert!(matches!(
        fault.detail(),
        Some(FaultDetail::TypeMismatch {
            operation: FaultOperation::MemberWrite,
            expected: ExpectedType::Member(MemberType::Int),
            given: GivenType::Value(DataType::Instance)
        })
    ));
    let fault = registered
        .read(&TestClass::default(), key, 1, 0)
        .unwrap_err();
    assert_eq!(fault.kind, FaultKind::Wire(ErrorKind::InvalidData));
    assert!(matches!(
        fault.detail(),
        Some(FaultDetail::Addon(BindingDiagnostic::Index {
            index: 1,
            length: 1,
            ..
        }))
    ));
}

#[test]
fn raw_callback_failures_describe_the_actual_tag_contract() {
    struct ReturningValue;
    impl Host for ReturningValue {
        type Error = Infallible;
        fn call(
            &mut self,
            _: &mut HostContext<'_, '_>,
            _: HostCall,
        ) -> VmResult<HostReturn, Self::Error> {
            Ok(HostReturn::Value(Value::Int(1)))
        }
    }
    let mut code = Vec::new();
    op(&mut code, 62, 1);
    code.push(60);
    let s = script(
        vec![
            function(b"MAIN", 0, 1, 0, 0),
            function(b"EXT", 0, 9, 0, u32::MAX),
        ],
        &code,
    );
    let mut vm = new_vm(&s);
    vm.default_external_raw(0);
    let error = vm.call(&mut ReturningValue, id(&s, 0), vec![]).unwrap_err();
    assert_eq!(
        (error.fault.kind, error.fault.symbol),
        (FaultKind::InvalidReturn, None)
    );
    assert!(
        matches!(error.fault.detail(), Some(FaultDetail::Return { callable: Some(c), expected: ReturnExpectation::StackHandledOrVoid, given: GivenType::Value(DataType::Int) }) if *c == id(&s, 1))
    );
    let error = StackReference::Temporary(vec![]).symbol().unwrap_err();
    assert!(matches!(
        error.detail(),
        Some(FaultDetail::TypeMismatch {
            expected: ExpectedType::SymbolReference,
            given: GivenType::Value(DataType::String),
            ..
        })
    ));
}

#[test]
fn diagnostic_errors_preserve_four_level_string_reentry_on_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut code = Vec::new();
            op(&mut code, 65, 4);
            op(&mut code, 65, 5);
            op(&mut code, 62, 1);
            code.push(60);
            let s = script(
                vec![
                    function(b"F", 0, 3, 3, 0),
                    function(b"EXT", 2, 11, 3, u32::MAX),
                    symbol(b"A", 3, 1, 0, 0, b"\n", -1),
                    symbol(b"B", 3, 1, 0, 0, b"\n", -1),
                    symbol(b"LEFT", 3, 1, 0, 0, b"alpha\n", -1),
                    symbol(b"RIGHT", 3, 1, 0, 0, b"beta\n", -1),
                ],
                &code,
            );
            struct Reentrant {
                function: SymbolId,
                depth: usize,
                calls: usize,
            }
            impl Host for Reentrant {
                type Error = Infallible;
                fn call(
                    &mut self,
                    cx: &mut HostContext<'_, '_>,
                    call: HostCall,
                ) -> VmResult<HostReturn, Self::Error> {
                    let mut args = call.arguments.into_iter();
                    let Some(Argument::Value(Value::Bytes(mut a))) = args.next() else {
                        panic!("first owned string")
                    };
                    let Some(Argument::Value(Value::Bytes(b))) = args.next() else {
                        panic!("second owned string")
                    };
                    assert!(args.next().is_none());
                    assert_eq!(a, b"alpha");
                    assert_eq!(b, b"beta");
                    self.depth += 1;
                    self.calls += 1;
                    let nested = if self.depth < 4 {
                        cx.call(self, self.function, vec![])
                    } else {
                        Ok(Some(Value::Bytes(b"leaf".to_vec())))
                    };
                    self.depth -= 1;
                    let nested = nested?.unwrap();
                    a.extend(b);
                    a.extend(nested.as_bytes()?);
                    Ok(HostReturn::Value(Value::Bytes(a)))
                }
            }
            let mut vm = new_vm(&s);
            vm.register_external(
                id(&s, 1),
                19,
                &[Parameter::Value(DataType::String); 2],
                DataType::String,
            )
            .unwrap();
            let mut host = Reentrant {
                function: id(&s, 0),
                depth: 0,
                calls: 0,
            };
            let result = vm.call(&mut host, id(&s, 0), vec![]).unwrap().unwrap();
            let mut expected = b"alphabeta".repeat(4);
            expected.extend(b"leaf");
            assert_eq!(result.as_bytes().unwrap(), expected);
            assert_eq!(
                (host.depth, host.calls, vm.call_depth(), vm.stack_len()),
                (0, 4, 0, 0)
            );
            let mut vm = Vm::from_script(
                &s,
                DecodeLimits::default(),
                Policy::default(),
                Limits {
                    host_depth: 2,
                    ..Limits::default()
                },
            )
            .unwrap();
            vm.register_external(
                id(&s, 1),
                19,
                &[Parameter::Value(DataType::String); 2],
                DataType::String,
            )
            .unwrap();
            let error = vm.call(&mut host, id(&s, 0), vec![]).unwrap_err();
            assert_eq!(error.fault.kind, FaultKind::Limit(LimitKind::HostDepth));
            assert_eq!((host.depth, vm.call_depth(), vm.stack_len()), (0, 0, 0));
        })
        .unwrap()
        .join()
        .unwrap();
}
