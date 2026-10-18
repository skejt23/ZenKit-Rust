use zenkit::{ErrorKind, daedalus::*, io::Reader};

// Independent primitive wire construction. No production DAT encoder exists.
fn word(out: &mut Vec<u8>, n: u32) {
    out.extend_from_slice(&n.to_le_bytes());
}
fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn symbol(
    name: Option<(u32, &[u8], u8)>,
    vary: u32,
    properties: u32,
    source: [u32; 5],
    data: &[u8],
    parent: i32,
) -> Vec<u8> {
    let mut out = Vec::new();
    word(&mut out, name.map_or(0, |n| n.0));
    if let Some((_, name, delimiter)) = name {
        out.extend_from_slice(name);
        out.push(delimiter);
    }
    word(&mut out, vary);
    word(&mut out, properties);
    for v in source {
        word(&mut out, v);
    }
    out.extend_from_slice(data);
    word(&mut out, parent as u32);
    out
}
fn s(name: &[u8], ty: u32, count: u32, flags: u32, data: &[u8], parent: i32) -> Vec<u8> {
    symbol(
        Some((1, name, 10)),
        0,
        count | ty << 12 | flags << 16,
        [0; 5],
        data,
        parent,
    )
}
fn dat(symbols: &[Vec<u8>], sort: &[u32], code: &[u8]) -> Vec<u8> {
    assert_eq!(symbols.len(), sort.len());
    let mut out = vec![50];
    word(&mut out, symbols.len() as u32);
    for v in sort {
        word(&mut out, *v);
    }
    for v in symbols {
        out.extend_from_slice(v);
    }
    word(&mut out, code.len() as u32);
    out.extend_from_slice(code);
    out
}
fn parse(bytes: &[u8]) -> Script {
    Script::from_bytes(bytes, Limits::default()).unwrap()
}
fn decode_limits() -> DecodeLimits {
    DecodeLimits::default()
}

#[test]
fn complete_words_names_delimiters_and_typed_initializers_are_retained() {
    let mut syms = vec![
        symbol(
            None,
            0x12345678,
            0xffff0000,
            [0xffffffff, 0x80000002, 0x40000003, 0xff000004, 0x80000005],
            &[],
            -99,
        ),
        symbol(
            Some((7, b"\xffTEMP", 13)),
            0x1234,
            0xab000000 | 1 << 12 | 3,
            [1, 2, 3, 4, 5],
            &words(&[0x80000000, 0x7fc12345, 0x7f800000]),
            -1,
        ),
        s(b"INTS", 2, 2, 0, &words(&[0x80000000, 0xffffffff]), -1),
        s(b"STRS", 3, 3, 1, b"a\\n\\t\\q\\\0\\\\n\r\n", -1),
        symbol(
            Some((1, b"CLASS", 0)),
            88,
            4 << 12 | 17,
            [0; 5],
            &words(&[0xffffffe0]),
            123,
        ),
        symbol(
            Some((1, b"FUNCTION", 10)),
            2,
            5 << 12 | 1 << 16,
            [0; 5],
            &words(&[0]),
            -7,
        ),
        s(b"PROTOTYPE", 6, 0, 0, &words(&[0]), 4),
        s(b"INSTANCE", 7, 0, 1, &words(&[0]), 6),
    ];
    for ty in 0..8 {
        syms.push(symbol(
            Some((1, b"MEMBER", 10)),
            0xdeadbeef,
            ty << 12 | 4 << 16 | 4095,
            [0; 5],
            &[],
            4,
        ));
    }
    let bytes = dat(&syms, &(0..syms.len() as u32).collect::<Vec<_>>(), &[60]);
    let script = parse(&bytes);
    assert_eq!(script.original_bytes(), bytes);
    assert_eq!(script.len(), 16);
    assert_eq!(
        script.symbol(0).unwrap().source_location().raw_words(),
        [0xffffffff, 0x80000002, 0x40000003, 0xff000004, 0x80000005]
    );
    assert_eq!(
        script.symbol(0).unwrap().source_location().file_index(),
        0x7ffff
    );
    assert_eq!(script.symbol(0).unwrap().source_location().char_start(), 4);
    assert_eq!(script.symbol(0).unwrap().vary(), 0x12345678);
    assert_eq!(script.symbol(0).unwrap().parent(), -99);
    let generated = script.symbol(1).unwrap();
    assert!(generated.is_generated());
    assert_eq!(generated.name_marker(), 7);
    assert_eq!(generated.raw_name().unwrap().delimiter(), 13);
    assert_eq!(generated.logical_name().collect::<Vec<_>>(), b"$TEMP");
    assert_eq!(generated.float_bits_at(0).unwrap(), 0x80000000);
    assert_eq!(generated.float_bits_at(1).unwrap(), 0x7fc12345);
    assert_eq!(generated.float_bits_at(2).unwrap(), 0x7f800000);
    assert_eq!(script.symbol(2).unwrap().int_at(0).unwrap(), i32::MIN);
    assert_eq!(script.symbol(2).unwrap().int_at(1).unwrap(), -1);
    let strings = script.symbol(3).unwrap();
    assert_eq!(
        strings.string_at(0).unwrap().decoded().collect::<Vec<_>>(),
        b"a\n\t\\q\\"
    );
    assert_eq!(
        strings.string_at(1).unwrap().decoded().collect::<Vec<_>>(),
        b"\\\n"
    );
    assert!(strings.string_at(2).unwrap().raw().is_empty());
    assert_eq!(strings.string_at(2).unwrap().delimiter(), 10);
    assert_eq!(script.symbol(4).unwrap().class_size(), Some(88));
    assert_eq!(script.symbol(4).unwrap().class_offset(), Some(-32));
    assert_eq!(
        script.symbol(5).unwrap().return_type().unwrap(),
        DataType::Int
    );
    for member in script.symbols().skip(8) {
        assert!(member.is_member());
        assert_eq!(member.member_offset(), Some(0xdeadbeef));
        assert!(matches!(member.initial_value(), InitialValue::None));
    }
    assert_eq!(
        generated.int_at(0).unwrap_err().kind,
        ErrorKind::WrongObjectType
    );
    assert_eq!(
        generated.float_bits_at(3).unwrap_err().kind,
        ErrorKind::InvalidData
    );
}

#[test]
fn names_duplicates_owner_ids_and_address_aliases_are_explicit() {
    let syms = [
        s(b"foo", 2, 0, 0, &[], -1),
        s(b"FOO", 2, 0, 0, &[], -1),
        s(b"foo", 2, 0, 0, &[], -1),
        s(b"\xffA", 2, 0, 0, &[], -1),
        s(b"$a", 2, 0, 0, &[], -1),
        symbol(None, 0, 0, [0; 5], &[], -1),
        symbol(Some((9, b"", 0)), 0, 0, [0; 5], &[], -1),
        s(b"EXT", 5, 0, 9, &words(&[0xffffffff]), -1),
        s(b"ALIAS", 5, 0, 9, &words(&[0xffffffff]), -1),
    ];
    let bytes = dat(&syms, &[0, 1, 2, 3, 4, 5, 6, 7, 8], &[]);
    let script = parse(&bytes);
    assert_eq!(script.find_raw_name(b"foo").unwrap().index(), 2);
    assert_eq!(
        script
            .name_matches(b"FoO")
            .map(|s| s.index())
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(script.find_name(b"\xffa").unwrap().index(), 4);
    assert_eq!(script.find_raw_name(b"").unwrap().index(), 6);
    assert_eq!(script.find_address(u32::MAX).unwrap().index(), 8);
    assert_eq!(script.address_matches(u32::MAX).count(), 2);
    let id = script.symbol(0).unwrap().id();
    let moved = script;
    assert_eq!(moved.get(id).unwrap().index(), 0);
    assert_eq!(
        parse(&bytes).get(id).unwrap_err().kind,
        ErrorKind::InvalidHandle
    );
    assert_eq!(
        moved
            .parameters(parse(&bytes).symbol(0).unwrap().id())
            .err()
            .unwrap()
            .kind,
        ErrorKind::InvalidHandle
    );
    assert!(moved.find_name(b"absent").is_none());
}

#[test]
fn parameters_are_positional_and_locals_reaching_eof_survive() {
    let bytes = dat(
        &[
            s(b"FUNC", 5, 1, 1, &words(&[0]), -1),
            s(b"NOT_A_NAMED_PARAMETER", 2, 1, 0, &words(&[3]), 999),
            s(b"FUNC.X", 2, 0, 0, &[], -8),
            s(b"FUNC.Y", 3, 0, 0, &[], 900),
        ],
        &[0, 1, 2, 3],
        &[60],
    );
    let script = parse(&bytes);
    let f = script.symbol(0).unwrap().id();
    assert_eq!(
        script
            .parameters(f)
            .unwrap()
            .map(|s| s.index())
            .collect::<Vec<_>>(),
        [1]
    );
    assert_eq!(
        script
            .locals(f)
            .unwrap()
            .map(|s| s.index())
            .collect::<Vec<_>>(),
        [2, 3]
    );
    let bytes = dat(
        &[
            s(b"F", 5, 0, 1, &words(&[0]), -1),
            s(b"F", 2, 0, 0, &[], -1),
        ],
        &[0, 1],
        &[60],
    );
    let script = parse(&bytes);
    assert_eq!(
        script
            .locals(script.symbol(0).unwrap().id())
            .unwrap()
            .count(),
        0
    );
    let script = parse(&dat(&[s(b"F", 5, 4095, 1, &words(&[0]), -1)], &[0], &[60]));
    assert_eq!(
        script
            .parameters(script.symbol(0).unwrap().id())
            .err()
            .unwrap()
            .kind,
        ErrorKind::InvalidData
    );
}

#[test]
fn ancestry_handles_forward_prototypes_and_checks_only_meaningful_parent_roles() {
    let bytes = dat(
        &[
            s(b"INSTANCE", 7, 0, 1, &words(&[0]), 2),
            s(b"GLOBAL_WITH_PARENT", 2, 0, 0, &[], 3),
            s(b"PROTO", 6, 0, 0, &words(&[0]), 4),
            s(b"OTHERCLASS", 4, 0, 0, &words(&[0]), -77),
            s(b"CLASS", 4, 0, 0, &words(&[0]), 900),
            s(b"MEMBER", 2, 7, 4, &[], 4),
            s(b"$INSTANCE_HELP", 7, 0, 0, &words(&[0]), -1),
        ],
        &[0, 1, 2, 3, 4, 5, 6],
        &[60],
    );
    let script = parse(&bytes);
    let class = script.symbol(4).unwrap().id();
    assert_eq!(
        script
            .class_of(script.symbol(0).unwrap().id())
            .unwrap()
            .id(),
        class
    );
    assert_eq!(
        script
            .class_of(script.symbol(5).unwrap().id())
            .unwrap()
            .id(),
        class
    );
    assert_eq!(
        script
            .class_members(class)
            .unwrap()
            .map(|s| s.index())
            .collect::<Vec<_>>(),
        [5]
    );
    assert_eq!(
        script
            .instances_of(class, script.len())
            .unwrap()
            .map(|s| s.index())
            .collect::<Vec<_>>(),
        [0]
    );
    assert_eq!(
        script
            .instances_of(class, script.len() - 1)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(
        script
            .class_of(script.symbol(6).unwrap().id())
            .unwrap_err()
            .kind,
        ErrorKind::InvalidData
    );
    script.validate_code(decode_limits()).unwrap(); // helper has no class; execution checks do not infer one
    let cyclic = parse(&dat(
        &[
            s(b"A", 6, 0, 0, &words(&[0]), 1),
            s(b"B", 6, 0, 0, &words(&[0]), 0),
            s(b"C", 4, 0, 0, &words(&[0]), -1),
        ],
        &[0, 1, 2],
        &[60],
    ));
    assert_eq!(
        cyclic
            .class_of(cyclic.symbol(0).unwrap().id())
            .unwrap_err()
            .kind,
        ErrorKind::CycleDetected
    );
    assert_eq!(
        cyclic
            .instances_of(cyclic.symbol(2).unwrap().id(), 3)
            .unwrap_err()
            .kind,
        ErrorKind::CycleDetected
    );
}

#[test]
fn sort_metadata_is_retained_before_separate_checked_validation() {
    let syms = [s(b"B", 2, 0, 0, &[], -1), s(b"A", 2, 0, 0, &[], -1)];
    parse(&dat(&syms, &[1, 0], &[]))
        .validate_sort_table(2)
        .unwrap();
    for sort in [[0, 1], [1, 1], [u32::MAX, 0]] {
        let script = parse(&dat(&syms, &sort, &[]));
        assert_eq!(script.sort_table(), sort);
        assert_eq!(
            script.validate_sort_table(2).unwrap_err().kind,
            ErrorKind::InvalidData
        );
    }
    assert_eq!(
        parse(&dat(&syms, &[1, 0], &[]))
            .validate_sort_table(1)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
}

#[test]
fn all_42_opcode_numbers_and_widths_are_independently_checked() {
    let codes = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 30, 31, 32,
        33, 45, 60, 61, 62, 64, 65, 67, 70, 71, 72, 73, 74, 75, 76, 80, 245,
    ];
    let mut code = Vec::new();
    let mut offsets = Vec::new();
    for &op in &codes {
        offsets.push(code.len() as u32);
        code.push(op);
        if [61, 62, 64, 65, 67, 75, 76, 80, 245].contains(&op) {
            code.extend_from_slice(&0x87654321u32.to_le_bytes());
        }
        if op == 245 {
            code.push(239);
        }
    }
    let script = parse(&dat(&[], &[], &code));
    let decoded = script
        .decode_code(DecodeLimits { instructions: 42 })
        .unwrap();
    assert_eq!(decoded.offsets(), offsets);
    for (n, instruction) in decoded.instructions().enumerate() {
        assert_eq!(instruction.op.raw(), codes[n]);
        let expected = match codes[n] {
            61 | 75 | 76 => Operand::Address(0x87654321),
            64 => Operand::Immediate(0x87654321u32 as i32),
            62 | 65 | 67 | 80 => Operand::Symbol(0x87654321),
            245 => Operand::Element {
                symbol: 0x87654321,
                index: 239,
            },
            _ => Operand::None,
        };
        assert_eq!(instruction.operand, expected);
    }
    assert_eq!(
        script
            .decode_code(DecodeLimits { instructions: 41 })
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(Opcode::ALL.len(), 42);
    assert_eq!(Opcode::from_raw(10), None);
}

#[test]
fn decode_and_executable_validation_have_distinct_boundaries_and_roles() {
    let script = parse(&dat(&[], &[], &[64, 45, 0, 0, 0, 60]));
    assert_eq!(script.decode_at(1).unwrap().op, Opcode::Nop);
    assert_eq!(
        script
            .decode_code(decode_limits())
            .unwrap()
            .instruction_at(1)
            .unwrap_err()
            .kind,
        ErrorKind::InvalidData
    );
    script.validate_code(decode_limits()).unwrap();
    let opaque = parse(&dat(&[], &[], &[255]));
    assert_eq!(opaque.original_bytes().last(), Some(&255));
    assert_eq!(
        opaque.decode_at(0).unwrap_err().kind,
        ErrorKind::Unsupported
    );
    let movss = parse(&dat(&[], &[], &[71]));
    assert_eq!(movss.decode_code(decode_limits()).unwrap().len(), 1);
    assert_eq!(
        movss.validate_code(decode_limits()).unwrap_err().kind,
        ErrorKind::Unsupported
    );
    let script = parse(&dat(
        &[
            s(b"F", 5, 0, 1, &words(&[0]), -1),
            s(b"EXTERNAL", 5, 0, 9, &words(&[u32::MAX]), -1),
            s(b"INSTANCE", 7, 0, 0, &words(&[0]), -1),
            s(b"ARRAY", 2, 2, 0, &words(&[8, 9]), -1),
        ],
        &[0, 1, 2, 3],
        &[
            61, 0, 0, 0, 0, 62, 1, 0, 0, 0, 67, 2, 0, 0, 0, 80, 2, 0, 0, 0, 245, 3, 0, 0, 0, 1, 60,
        ],
    ));
    script.validate_code(decode_limits()).unwrap();
    assert_eq!(script.find_address(u32::MAX).unwrap().index(), 1);
    for code in [
        &[75, 1, 0, 0, 0, 60][..],
        &[61, 5, 0, 0, 0, 60],
        &[65, 99, 0, 0, 0],
        &[62, 0, 0, 0, 0],
        &[80, 0, 0, 0, 0],
        &[245, 0, 0, 0, 0, 1],
    ] {
        let script = parse(&dat(&[s(b"I", 2, 1, 0, &words(&[0]), -1)], &[0], code));
        assert!(script.validate_code(decode_limits()).is_err());
    }
}

#[test]
fn truncated_instruction_operand_offsets_and_iterator_fusion_are_truthful() {
    for code in [&[64][..], &[61, 1, 2], &[245, 0, 0, 0, 0]] {
        let script = parse(&dat(&[], &[], code));
        let mut iter = script.instructions(decode_limits());
        let e = iter.next().unwrap().unwrap_err();
        assert_eq!(e.kind, ErrorKind::UnexpectedEof);
        assert_eq!(e.offset, Some(script.code_offset() + 1));
        assert!(iter.next().is_none());
        assert!(iter.next().is_none());
    }
    let script = parse(&dat(&[], &[], &[45, 45]));
    let mut iter = script.instructions(DecodeLimits { instructions: 0 });
    assert_eq!(
        iter.next().unwrap().unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    assert!(iter.next().is_none());
}

#[test]
fn calls_use_last_alias_including_nonconst_instance_and_reject_external_shadow() {
    let mut symbols = vec![
        s(b"CLASS", 4, 0, 0, &words(&[0]), -1),
        s(b"FUNCTION", 5, 0, 1, &words(&[5]), -1),
        s(b"MUTABLE_INSTANCE", 7, 0, 0, &words(&[5]), 0),
    ];
    let code = [61, 5, 0, 0, 0, 60];
    let script = parse(&dat(&symbols, &[0, 1, 2], &code));
    assert_eq!(script.find_address(5).unwrap().index(), 2);
    assert!(!script.find_address(5).unwrap().is_const());
    script.validate_code(decode_limits()).unwrap();
    symbols.push(s(b"EXTERNAL_SHADOW", 5, 0, 9, &words(&[5]), -1));
    let script = parse(&dat(&symbols, &[0, 1, 2, 3], &code));
    assert_eq!(script.find_address(5).unwrap().index(), 3);
    let error = script.validate_code(decode_limits()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::WrongObjectType);
    assert_eq!(error.offset, Some(script.code_offset() + 1));
}

#[test]
fn unknown_function_return_type_and_nonconst_function_address_stay_inspectable() {
    let script = parse(&dat(
        &[symbol(
            Some((3, b"F", 0)),
            0x12345678,
            5 << 12,
            [0; 5],
            &words(&[0x87654321]),
            -11,
        )],
        &[0],
        &[],
    ));
    let f = script.symbol(0).unwrap();
    assert_eq!(f.return_type_raw(), Some(0x12345678));
    assert_eq!(f.return_type().unwrap_err().kind, ErrorKind::Unsupported);
    assert_eq!(f.return_type().unwrap_err().offset, Some(f.offset() + 6));
    assert_eq!(f.address(), Some(0x87654321u32 as i32));
    assert!(script.find_address(0x87654321).is_none());
    assert_eq!(f.int_at(0).unwrap_err().kind, ErrorKind::WrongObjectType);
}

#[test]
fn exact_aggregate_limits_and_preflight_apply_before_retaining_buffers() {
    let bytes = dat(
        &[
            s(b"I", 2, 2, 0, &words(&[1, 2]), -1),
            s(b"S", 3, 1, 0, b"hello\n", -1),
            s(b"F", 5, 0, 1, &words(&[0]), -1),
        ],
        &[0, 1, 2],
        &[60],
    );
    let exact = Limits {
        bytes: bytes.len(),
        copied_bytes: bytes.len(),
        symbols: 3,
        values: 4,
        string_bytes: 5,
        code_bytes: 1,
        index_items: 7,
    };
    Script::from_bytes(&bytes, exact).unwrap();
    for limits in [
        Limits {
            bytes: bytes.len() - 1,
            ..exact
        },
        Limits {
            copied_bytes: bytes.len() - 1,
            ..exact
        },
        Limits {
            symbols: 2,
            ..exact
        },
        Limits { values: 3, ..exact },
        Limits {
            string_bytes: 4,
            ..exact
        },
        Limits {
            code_bytes: 0,
            ..exact
        },
        Limits {
            index_items: 6,
            ..exact
        },
    ] {
        assert_eq!(
            Script::from_bytes(&bytes, limits).unwrap_err().kind,
            ErrorKind::LimitExceeded
        );
    }
    let mut count = vec![50];
    word(&mut count, u32::MAX);
    word(&mut count, 0);
    assert_eq!(
        Script::from_bytes(
            &count,
            Limits {
                symbols: usize::MAX,
                ..Limits::default()
            }
        )
        .unwrap_err()
        .kind,
        ErrorKind::UnexpectedEof
    );
}

#[test]
fn full_buffer_is_strict_prefix_read_is_atomic_and_offsets_include_parent() {
    let bytes = dat(&[s(b"X", 2, 1, 0, &words(&[7]), -1)], &[0], &[60]);
    let mut extended = vec![9, 8, 7];
    extended.extend_from_slice(&bytes);
    extended.extend_from_slice(b"tail");
    let mut reader = Reader::new(&extended);
    reader.take_bytes(3, "prefix").unwrap();
    let value = Script::read(
        &mut reader,
        Limits {
            bytes: bytes.len(),
            copied_bytes: bytes.len(),
            ..Limits::default()
        },
    )
    .unwrap();
    assert_eq!(value.offset(), 3);
    assert_eq!(value.symbol(0).unwrap().offset(), 12);
    assert_eq!(reader.remaining(), 4);
    assert_eq!(value.original_bytes(), bytes);
    assert_eq!(value.code_offset(), 3 + bytes.len() - 1);
    assert_eq!(
        Script::from_bytes(&extended[3..], Limits::default())
            .unwrap_err()
            .kind,
        ErrorKind::InvalidData
    );
    for end in 0..bytes.len() {
        let mut input = vec![0, 0, 0];
        input.extend_from_slice(&bytes[..end]);
        let mut r = Reader::new(&input);
        r.take_bytes(3, "prefix").unwrap();
        let error = Script::read(&mut r, Limits::default()).unwrap_err();
        assert_eq!(r.position(), 3);
        assert!(error.offset.unwrap() >= 3);
    }
}

#[test]
fn unknown_type_rejects_at_properties_and_const_string_parent_is_not_shifted() {
    let bytes = dat(&[symbol(None, 0, 8 << 12, [0; 5], &[], -1)], &[0], &[]);
    let error = Script::from_bytes(&bytes, Limits::default()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unsupported);
    assert_eq!(error.offset, Some(17));
    let bytes = dat(&[s(b"STR", 3, 1, 1, b"text\r", 10)], &[0], &[]);
    let script = parse(&bytes);
    assert_eq!(script.symbol(0).unwrap().parent(), 10);
    assert_eq!(
        script.symbol(0).unwrap().string_at(0).unwrap().delimiter(),
        13
    );
    assert_eq!(script.original_bytes(), bytes);
}

#[test]
fn version_is_opaque_and_debug_never_prints_large_raw_payloads() {
    let payload = vec![b'x'; 100_000];
    let mut value = payload.clone();
    value.push(10);
    let mut bytes = dat(&[s(&payload, 3, 1, 0, &value, -1)], &[0], &[45; 1000]);
    bytes[0] = 231;
    let script = parse(&bytes);
    assert_eq!(script.version(), 231);
    assert!(format!("{script:?}").len() < 200);
    assert!(format!("{:?}", script.symbol(0).unwrap()).len() < 200);
    assert!(format!("{:?}", script.symbol(0).unwrap().raw_name().unwrap()).len() < 200);
    assert!(format!("{:?}", script.symbol(0).unwrap().initial_value()).len() < 100);
}

#[test]
fn long_forward_ancestry_is_iterative_on_a_small_worker_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let n = 10_000;
            let mut symbols = Vec::new();
            symbols.push(s(b"INSTANCE", 7, 0, 1, &words(&[0]), 1));
            for i in 1..n {
                symbols.push(s(b"P", 6, 0, 0, &words(&[0]), i + 1));
            }
            symbols.push(s(b"CLASS", 4, 0, 0, &words(&[0]), -1));
            let script = parse(&dat(&symbols, &(0..=n as u32).collect::<Vec<_>>(), &[60]));
            let class = script.symbol(n as u32).unwrap().id();
            assert_eq!(
                script
                    .class_of(script.symbol(0).unwrap().id())
                    .unwrap()
                    .id(),
                class
            );
            assert_eq!(
                script.instances_of(class, n as usize + 1).unwrap().count(),
                1
            );
        })
        .unwrap()
        .join()
        .unwrap();
}
