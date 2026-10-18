use std::convert::Infallible;
use zenkit::daedalus::{self, DecodeLimits, Script, addon::*, vm::*};

fn word(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend(value.to_le_bytes());
}
fn record(name: &[u8], ty: u32, count: u32, flags: u32, data: &[u8], parent: i32) -> Vec<u8> {
    let mut bytes = Vec::new();
    word(&mut bytes, 1);
    bytes.extend(name);
    bytes.push(10);
    word(&mut bytes, 0);
    word(&mut bytes, ty << 12 | count | flags << 16);
    bytes.extend([0u8; 20]);
    bytes.extend(data);
    word(&mut bytes, parent as u32);
    bytes
}
fn op(code: &mut Vec<u8>, op: u8, operand: u32) {
    code.push(op);
    word(code, operand);
}
fn indexed(code: &mut Vec<u8>, symbol: u32, index: usize) {
    op(code, 245, symbol);
    code.push(index.try_into().unwrap());
}
fn string(field: usize, index: usize) -> Vec<u8> {
    format!("field{field}_element{index}").into_bytes()
}
fn number(field: usize, index: usize) -> i32 {
    -((field * 1000 + index + 1) as i32)
}
fn float_bits(field: usize, index: usize) -> u32 {
    0x7fc0_0000 + (field * 1000 + index + 1) as u32
}

fn program<T: AddonClass>() -> (Script, u32, Vec<(u32, usize, usize)>) {
    let schema = T::SCHEMA;
    let mut symbols = vec![record(schema.name, 4, 0, 0, &0u32.to_le_bytes(), -1)];
    for (field, member) in schema.members.iter().enumerate() {
        let ty = match member.kind {
            MemberKind::Int if field % 2 == 0 => 5,
            MemberKind::Int => 2,
            MemberKind::Float => 1,
            MemberKind::String => 3,
        };
        symbols.push(record(member.name, ty, member.capacity as u32, 4, &[], 0));
    }
    let instance = symbols.len() as u32;
    symbols.push(record(b"TEST_INSTANCE", 7, 0, 1, &0u32.to_le_bytes(), 0));
    let mut stores = Vec::new();
    let mut reads = Vec::new();
    let mut outputs = Vec::new();
    for (field, member) in schema.members.iter().enumerate() {
        for index in 0..member.capacity {
            let (ty, movement) = match member.kind {
                MemberKind::Int => {
                    op(&mut stores, 64, number(field, index) as u32);
                    (2, 9)
                }
                MemberKind::Float => {
                    op(&mut stores, 64, float_bits(field, index));
                    (1, 73)
                }
                MemberKind::String => {
                    let mut bytes = string(field, index);
                    bytes.push(10);
                    let source = symbols.len() as u32;
                    symbols.push(record(
                        format!("S{field}_{index}").as_bytes(),
                        3,
                        1,
                        1,
                        &bytes,
                        -1,
                    ));
                    op(&mut stores, 65, source);
                    (3, 70)
                }
            };
            indexed(&mut stores, field as u32 + 1, index);
            stores.push(movement);
            let target = symbols.len() as u32;
            let initial = if ty == 3 { vec![10] } else { vec![0; 4] };
            symbols.push(record(
                format!("OUT{field}_{index}").as_bytes(),
                ty,
                1,
                0,
                &initial,
                -1,
            ));
            indexed(&mut reads, field as u32 + 1, index);
            op(&mut reads, 65, target);
            reads.push(movement);
            outputs.push((target, field, index));
        }
    }
    stores.extend(reads);
    stores.push(60);
    let mut bytes = vec![50];
    word(&mut bytes, symbols.len() as u32);
    for index in 0..symbols.len() {
        word(&mut bytes, index as u32);
    }
    for symbol in symbols {
        bytes.extend(symbol);
    }
    word(&mut bytes, stores.len() as u32);
    bytes.extend(stores);
    (
        Script::from_bytes(&bytes, daedalus::Limits::default()).unwrap(),
        instance,
        outputs,
    )
}

struct App<T> {
    data: T,
    binding: RegisteredAddon,
    gets: usize,
    sets: usize,
}
impl<T: AddonClass> Host for App<T> {
    type Error = Infallible;
    fn get_member(
        &mut self,
        _: &mut HostContext<'_, '_>,
        key: HostKey,
        member: MemberKey,
        index: usize,
    ) -> VmResult<Value, Self::Error> {
        assert_eq!(key, HostKey(7));
        self.gets += 1;
        Ok(self.binding.read(&self.data, member, index, 64)?)
    }
    fn set_member(
        &mut self,
        _: &mut HostContext<'_, '_>,
        key: HostKey,
        member: MemberKey,
        index: usize,
        value: Value,
    ) -> VmResult<(), Self::Error> {
        assert_eq!(key, HostKey(7));
        self.sets += 1;
        Ok(self
            .binding
            .write(&mut self.data, member, index, &value, 64)?)
    }
}

fn execute<T: AddonClass>() -> usize {
    let (script, instance, outputs) = program::<T>();
    let mut vm = Vm::from_script(
        &script,
        DecodeLimits::default(),
        Policy::default(),
        Limits::default(),
    )
    .unwrap();
    let binding = RegisteredAddon::register::<T>(&mut vm, 710)
        .unwrap()
        .unwrap();
    let instance_symbol = script.symbol(instance).unwrap().id();
    let instance = vm
        .allocate_host(instance_symbol, binding.class(), HostKey(7))
        .unwrap();
    let mut app = App {
        data: T::default(),
        binding,
        gets: 0,
        sets: 0,
    };
    vm.init_instance(&mut app, instance, instance_symbol)
        .unwrap();
    assert_eq!(app.gets, outputs.len());
    assert_eq!(app.sets, outputs.len());
    for &(symbol, field, index) in &outputs {
        let value = vm
            .state()
            .global(script.symbol(symbol).unwrap().id(), 0)
            .unwrap();
        match T::SCHEMA.members[field].kind {
            MemberKind::Int => assert_eq!(value.as_int().unwrap(), number(field, index)),
            MemberKind::Float => assert_eq!(
                value.as_float().unwrap().to_bits(),
                float_bits(field, index)
            ),
            MemberKind::String => assert_eq!(value.as_bytes().unwrap(), string(field, index)),
        }
    }
    assert_eq!(vm.call_depth(), 0);
    assert_eq!(vm.stack_len(), 0);
    assert_eq!(vm.current_instance(), None);
    drop(vm);
    drop(script);
    // Host storage and raw registered metadata survive both VM and Script drop.
    for (_, field, index) in outputs {
        match app.data.read_member(field, index).unwrap() {
            MemberValue::Int(value) => assert_eq!(value, number(field, index)),
            MemberValue::Float(value) => assert_eq!(value.to_bits(), float_bits(field, index)),
            MemberValue::Bytes(value) => assert_eq!(value, string(field, index)),
        }
    }
    T::SCHEMA.members.len()
}

#[test]
fn every_addon_member_executes_script_to_host_and_host_to_script() {
    let fields = execute::<GuildValues>()
        + execute::<Npc>()
        + execute::<Mission>()
        + execute::<Item>()
        + execute::<Focus>()
        + execute::<Info>()
        + execute::<ItemReact>()
        + execute::<Spell>()
        + execute::<Svm>()
        + execute::<Menu>()
        + execute::<MenuItem>()
        + execute::<Camera>()
        + execute::<MusicSystem>()
        + execute::<MusicTheme>()
        + execute::<MusicJingle>()
        + execute::<ParticleEffect>()
        + execute::<EffectBase>()
        + execute::<ParticleEffectEmitKey>()
        + execute::<FightAi>()
        + execute::<SoundEffect>()
        + execute::<SoundSystem>();
    assert_eq!(fields, 710);
}

#[test]
fn addon_bridge_rejects_a_member_from_another_runtime() {
    let (script, symbol, _) = program::<Npc>();
    let make_vm = || {
        Vm::from_script(
            &script,
            DecodeLimits::default(),
            Policy::default(),
            Limits::default(),
        )
        .unwrap()
    };
    let mut first = make_vm();
    let wrong_binding = RegisteredAddon::register::<Npc>(&mut first, 32)
        .unwrap()
        .unwrap();
    let mut second = make_vm();
    let right = RegisteredAddon::register::<Npc>(&mut second, 32)
        .unwrap()
        .unwrap();
    let symbol = script.symbol(symbol).unwrap().id();
    let instance = second
        .allocate_host(symbol, right.class(), HostKey(7))
        .unwrap();
    let mut host = App {
        data: Npc::default(),
        binding: wrong_binding,
        gets: 0,
        sets: 0,
    };
    let failure = second
        .init_instance(&mut host, instance, symbol)
        .unwrap_err();
    assert_eq!(failure.fault.kind, FaultKind::InvalidHandle);
    assert_eq!(host.data.id, 0);
    assert_eq!(second.stack_len(), 0);
    assert_eq!(second.call_depth(), 0);
    assert_eq!(second.current_instance(), None);
}
