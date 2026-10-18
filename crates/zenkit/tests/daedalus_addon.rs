use zenkit::ErrorKind;
use zenkit::daedalus::addon::*;
use zenkit::daedalus::{DataType, Limits, Script};

fn exercise<T: AddonClass>() -> usize {
    let mut value = T::default();
    let schema = T::SCHEMA;
    for (field, meta) in schema.members.iter().enumerate() {
        for index in 0..meta.capacity {
            let word = (field * 5000 + index + 1) as i32;
            let bytes = [word.to_le_bytes().as_slice(), &[0xff, 0, b'\\', b'n']].concat();
            let expected = match meta.kind {
                MemberKind::Int => MemberValue::Int(-word),
                MemberKind::Float => MemberValue::Float(f32::from_bits(0x3f80_0000 + word as u32)),
                MemberKind::String => MemberValue::Bytes(&bytes),
            };
            value.write_member(field, index, expected, 8).unwrap();
        }
    }
    // Check after all writes: aliases or wrong element indexes cannot hide behind
    // an immediate read of the last store.
    for (field, meta) in schema.members.iter().enumerate() {
        for index in 0..meta.capacity {
            let word = (field * 5000 + index + 1) as i32;
            match value.read_member(field, index).unwrap() {
                MemberValue::Int(actual) => assert_eq!(actual, -word),
                MemberValue::Float(actual) => {
                    assert_eq!(actual.to_bits(), 0x3f80_0000 + word as u32)
                }
                MemberValue::Bytes(actual) => assert_eq!(
                    actual,
                    [word.to_le_bytes().as_slice(), &[0xff, 0, b'\\', b'n']].concat()
                ),
            }
            let wrong = if meta.kind == MemberKind::Int {
                MemberValue::Float(1.0)
            } else {
                MemberValue::Int(2)
            };
            assert_eq!(
                value
                    .write_member(field, index, wrong, 100)
                    .unwrap_err()
                    .kind,
                ErrorKind::WrongObjectType
            );
        }
        assert_eq!(
            value.read_member(field, meta.capacity).unwrap_err().kind,
            ErrorKind::InvalidData
        );
    }
    assert_eq!(
        value.read_member(schema.members.len(), 0).unwrap_err().kind,
        ErrorKind::NotFound
    );
    schema.members.len()
}

#[test]
fn every_class_field_and_array_element_has_distinct_storage() {
    let fields = exercise::<GuildValues>()
        + exercise::<Npc>()
        + exercise::<Mission>()
        + exercise::<Item>()
        + exercise::<Focus>()
        + exercise::<Info>()
        + exercise::<ItemReact>()
        + exercise::<Spell>()
        + exercise::<Svm>()
        + exercise::<Menu>()
        + exercise::<MenuItem>()
        + exercise::<Camera>()
        + exercise::<MusicSystem>()
        + exercise::<MusicTheme>()
        + exercise::<MusicJingle>()
        + exercise::<ParticleEffect>()
        + exercise::<EffectBase>()
        + exercise::<ParticleEffectEmitKey>()
        + exercise::<FightAi>()
        + exercise::<SoundEffect>()
        + exercise::<SoundSystem>();
    assert_eq!(fields, 710);
    assert_eq!(CLASSES.len(), 21);
}

#[derive(Clone)]
struct Record {
    name: Vec<u8>,
    ty: DataType,
    count: u16,
    parent: i32,
}
fn records(schemas: &[&ClassSchema], optional: bool) -> Vec<Record> {
    let mut result = Vec::new();
    for schema in schemas {
        let parent = result.len() as i32;
        result.push(Record {
            name: schema.name.to_vec(),
            ty: DataType::Class,
            count: 0,
            parent: -1,
        });
        for meta in schema.members {
            if !optional && meta.presence_guard.is_some() {
                continue;
            }
            result.push(Record {
                name: meta.name.to_vec(),
                ty: match meta.kind {
                    MemberKind::Int => DataType::Int,
                    MemberKind::Float => DataType::Float,
                    MemberKind::String => DataType::String,
                },
                count: meta.capacity as u16,
                parent,
            });
        }
    }
    result
}
fn load(records: &[Record]) -> Script {
    let mut bytes = vec![50];
    bytes.extend((records.len() as u32).to_le_bytes());
    for index in 0..records.len() {
        bytes.extend((index as u32).to_le_bytes());
    }
    for record in records {
        bytes.extend(1u32.to_le_bytes());
        bytes.extend(&record.name);
        bytes.push(b'\n');
        bytes.extend(0u32.to_le_bytes()); // vary
        let flags = if record.ty == DataType::Class {
            0
        } else {
            4 << 16
        };
        bytes.extend((record.ty.raw() << 12 | u32::from(record.count) | flags).to_le_bytes());
        bytes.extend([0u8; 20]);
        if record.ty == DataType::Class {
            bytes.extend(0i32.to_le_bytes());
        }
        bytes.extend(record.parent.to_le_bytes());
    }
    bytes.extend(0u32.to_le_bytes()); // no bytecode
    Script::from_bytes(&bytes, Limits::default()).unwrap()
}

#[test]
fn all_present_classes_bind_required_and_optional_members() {
    for optional in [false, true] {
        let script = load(&records(CLASSES, optional));
        let bindings = bind_all_present(&script, 710).unwrap();
        assert_eq!(bindings.len(), 21);
        assert_eq!(
            bindings.iter().map(|b| b.members().len()).sum::<usize>(),
            710
        );
        assert_eq!(
            bindings
                .iter()
                .flat_map(|b| b.members())
                .filter(|m| m.is_some())
                .count(),
            if optional { 710 } else { 376 }
        );
        for binding in &bindings {
            for member in binding.members().iter().flatten() {
                let symbol = script.get(member.symbol()).unwrap();
                assert_eq!(symbol.parent(), binding.class().index() as i32);
                assert_eq!(symbol.count() as usize, member.elements());
            }
        }
        assert_eq!(
            bind_all_present(&script, 709).unwrap_err().kind,
            ErrorKind::LimitExceeded
        );
    }
    let empty = load(&[]);
    assert!(bind_all_present(&empty, 0).unwrap().is_empty());
    assert!(
        ClassBinding::bind_present::<Npc>(&empty, 0)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        ClassBinding::bind::<Npc>(&empty, 32).unwrap_err().kind,
        ErrorKind::NotFound
    );
}

#[test]
fn exact_optional_group_guards_and_atomic_validation() {
    let mut base = records(&[Npc::SCHEMA], true);
    let remove = |v: &mut Vec<Record>, name: &[u8]| v.retain(|r| r.name != name);
    remove(&mut base, b"C_NPC.NOFOCUS");
    assert_eq!(
        ClassBinding::bind::<Npc>(&load(&base), 32)
            .unwrap_err()
            .kind,
        ErrorKind::NotFound
    );
    // HITCHANCE controls all three fields. Existing companions are ignored if
    // that guard is absent, rather than guessed from the nominal game version.
    remove(&mut base, b"C_NPC.HITCHANCE");
    let binding = ClassBinding::bind::<Npc>(&load(&base), 32).unwrap();
    assert!(binding.members()[28..31].iter().all(Option::is_none));
    assert!(binding.members()[31].is_some()); // independent EFFECT guard
    let mut wrong = records(&[Npc::SCHEMA], false);
    wrong[1].parent = -1;
    assert_eq!(
        ClassBinding::bind::<Npc>(&load(&wrong), 32)
            .unwrap_err()
            .kind,
        ErrorKind::WrongObjectType
    );
    wrong[1].parent = 0;
    wrong[1].ty = DataType::String;
    assert_eq!(
        ClassBinding::bind::<Npc>(&load(&wrong), 32)
            .unwrap_err()
            .kind,
        ErrorKind::WrongObjectType
    );
}

#[test]
fn binding_uses_script_extent_and_preserves_exact_types_and_bytes() {
    let mut input = records(&[Npc::SCHEMA], true);
    let name = input.iter_mut().find(|r| r.name == b"C_NPC.NAME").unwrap();
    name.count = 2;
    input.iter_mut().find(|r| r.name == b"C_NPC.ID").unwrap().ty = DataType::Function;
    let script = load(&input);
    let binding = ClassBinding::bind::<Npc>(&script, 32).unwrap();
    let mut npc = Npc::default();
    binding
        .write(&mut npc, 0, 0, MemberValue::Int(-10), 0)
        .unwrap();
    assert_eq!(npc.id, -10);
    binding
        .write(&mut npc, 1, 1, MemberValue::Bytes(b"\xff\0raw"), 5)
        .unwrap();
    assert_eq!(npc.name[1], b"\xff\0raw");
    assert_eq!(
        binding
            .write(&mut npc, 1, 1, MemberValue::Bytes(b"longer"), 5)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(npc.name[1], b"\xff\0raw");
    assert_eq!(
        binding
            .write(&mut npc, 1, 2, MemberValue::Bytes(b"x"), 5)
            .unwrap_err()
            .kind,
        ErrorKind::InvalidData
    );
    assert!(npc.name[2].is_empty());
    assert_eq!(
        binding.read(&Item::default(), 0, 0).unwrap_err().kind,
        ErrorKind::WrongObjectType
    );
    input
        .iter_mut()
        .find(|r| r.name == b"C_NPC.NAME")
        .unwrap()
        .count = 6;
    assert_eq!(
        ClassBinding::bind::<Npc>(&load(&input), 32)
            .unwrap_err()
            .kind,
        ErrorKind::InvalidData
    );
    input
        .iter_mut()
        .find(|r| r.name == b"C_NPC.NAME")
        .unwrap()
        .count = 0;
    let zero = ClassBinding::bind::<Npc>(&load(&input), 32).unwrap();
    assert_eq!(
        zero.read(&npc, 1, 0).unwrap_err().kind,
        ErrorKind::InvalidData
    );
}

#[test]
fn enums_preserve_unknown_bits_and_flags_use_named_overlap() {
    let raw = 0xdead_beef;
    assert_eq!(NpcType::from_raw(raw).raw(), raw);
    assert_eq!(NpcType::G1_FRIEND.raw(), 3);
    assert_eq!(NpcType::G2_FRIEND.raw(), 2);
    assert_eq!(FightAiMove::WAIT_EXT.raw(), 24);
    assert_eq!(MenuItemSelectEvent::SELECT_NEXT.raw(), 8);
    let flags = NpcFlag::IMMORTAL | NpcFlag::GHOST;
    assert!(flags.intersects(NpcFlag::FRIENDS | NpcFlag::GHOST));
    assert!(!flags.contains(NpcFlag::FRIENDS | NpcFlag::GHOST));
    assert_eq!((flags & NpcFlag::GHOST).raw(), 4);
    let mut npc = Npc::default();
    npc.write_member(4, 0, MemberValue::Int(raw as i32), 0)
        .unwrap();
    assert_eq!(npc.flags.raw(), raw);
    assert_eq!(damage_type::NUM, 8);
    assert_eq!(npc_attribute::REGENERATE_MANA, 7);
}

#[test]
fn choices_preserve_order_and_fail_without_modifying_data() {
    let mut info = Info::default();
    let limits = ChoiceLimits {
        choices: 2,
        text_bytes: 4,
        total_text_bytes: 6,
    };
    info.add_choice(b"one", 10, limits).unwrap();
    info.add_choice(b"two", -1, limits).unwrap();
    assert_eq!(info.choices()[0].text, b"two");
    assert_eq!(info.choices()[1].function, 10);
    assert_eq!(
        info.add_choice(b"", 3, limits).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(
        info.remove_choice(2).unwrap_err().kind,
        ErrorKind::InvalidData
    );
    assert_eq!(info.remove_choice(0).unwrap().function, -1);
    assert_eq!(
        info.add_choice(b"long", 11, limits).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(info.choices().len(), 1);
    assert!(!format!("{:?}", info.choices()[0]).contains("one"));
    assert!(!format!("{:?}", MemberValue::Bytes(b"private-text")).contains("private-text"));
}

#[test]
fn register_all_present_is_atomic_and_preserves_existing_runtime_keys() {
    use zenkit::daedalus::{DecodeLimits, vm};
    let script = load(&records(CLASSES, true));
    let mut machine = vm::Vm::from_script(
        &script,
        DecodeLimits::default(),
        vm::Policy::default(),
        vm::Limits {
            bindings: 731,
            ..vm::Limits::default()
        },
    )
    .unwrap();
    let all = register_all_present(&mut machine, 710).unwrap();
    assert_eq!(all.len(), 21);
    for item in &all {
        assert_eq!(
            machine
                .state()
                .registered_class(item.binding().class())
                .unwrap(),
            Some(item.class())
        );
    }
    let again = register_all_present(&mut machine, 710).unwrap();
    assert!(all.iter().zip(&again).all(|(a, b)| a.class() == b.class()));

    let mut limited = vm::Vm::from_script(
        &script,
        DecodeLimits::default(),
        vm::Policy::default(),
        vm::Limits {
            bindings: 730,
            ..vm::Limits::default()
        },
    )
    .unwrap();
    let npc = RegisteredAddon::register::<Npc>(&mut limited, 32)
        .unwrap()
        .unwrap();
    let failure = register_all_present(&mut limited, 710).unwrap_err();
    assert_eq!(failure.kind, vm::FaultKind::Limit(vm::LimitKind::Bindings));
    for schema in CLASSES {
        let id = script.find_name(schema.name).unwrap().id();
        let expected = if schema.name == b"C_NPC" {
            Some(npc.class())
        } else {
            None
        };
        assert_eq!(limited.state().registered_class(id).unwrap(), expected);
    }
    assert_eq!(
        RegisteredAddon::register::<Npc>(&mut limited, 32)
            .unwrap()
            .unwrap()
            .class(),
        npc.class()
    );
}
