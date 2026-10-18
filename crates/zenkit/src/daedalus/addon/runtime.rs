//! Checked adapters between typed addon storage and VM member callbacks.
use super::{AddonClass, ClassBinding, MemberKind, MemberValue};
use crate::daedalus::vm::{
    self, ClassId, ClassRegistration, ExpectedType, Fault, FaultDetail, FaultKind, FaultOperation,
    GivenType, MemberKey, MemberSpec, MemberType, Value, Vm,
};

/// A class mapping registered with one particular runtime. Host objects remain
/// ordinary typed Rust values; this adapter never stores or borrows them.
#[derive(Debug)]
pub struct RegisteredAddon {
    binding: ClassBinding,
    class: ClassId,
}

fn member_type(kind: MemberKind) -> MemberType {
    match kind {
        MemberKind::Int => MemberType::Int,
        MemberKind::Float => MemberType::Float,
        MemberKind::String => MemberType::String,
    }
}

fn specs(binding: &ClassBinding) -> vm::Result<Vec<MemberSpec<'static>>> {
    let mut specs = Vec::new();
    specs
        .try_reserve_exact(binding.members().len())
        .map_err(|_| Fault::new(FaultKind::Allocation))?;
    for field in binding.members().iter().flatten() {
        let meta = &binding.schema().members[field.ordinal()];
        specs.push(MemberSpec {
            name: meta.name,
            kind: member_type(meta.kind),
            capacity: meta.capacity,
            key: u32::try_from(field.ordinal()).map_err(|_| Fault::new(FaultKind::Index))?,
        });
    }
    Ok(specs)
}

/// Register every present built-in addon class as one atomic VM operation.
/// Missing classes are skipped; invalid present classes and allocation/budget
/// failures leave the runtime's previous registrations intact.
pub fn register_all_present(
    vm: &mut Vm<'_>,
    max_members: usize,
) -> vm::Result<Vec<RegisteredAddon>> {
    let bindings = super::bind_all_present_detailed(vm.script(), max_members)?;
    let mut members = Vec::new();
    members
        .try_reserve_exact(bindings.len())
        .map_err(|_| Fault::new(FaultKind::Allocation))?;
    for binding in &bindings {
        members.push(specs(binding)?);
    }
    let mut requests = Vec::new();
    requests
        .try_reserve_exact(bindings.len())
        .map_err(|_| Fault::new(FaultKind::Allocation))?;
    for (binding, members) in bindings.iter().zip(&members) {
        requests.push(ClassRegistration {
            class: binding.class(),
            members,
        });
    }
    let mut result = Vec::new();
    result
        .try_reserve_exact(bindings.len())
        .map_err(|_| Fault::new(FaultKind::Allocation))?;
    let ids = vm.register_classes(&requests)?;
    for (binding, class) in bindings.into_iter().zip(ids) {
        log_registration(&binding);
        result.push(RegisteredAddon { binding, class });
    }
    Ok(result)
}

fn log_registration(binding: &ClassBinding) {
    crate::logging::emit(
        crate::logging::Level::Debug,
        "zenkit::daedalus::addon",
        format_args!("registered addon class {:?}", binding.schema().name),
    );
}

impl RegisteredAddon {
    /// An absent class returns None. Invalid present classes fail before the VM
    /// registration changes. Existing identical layouts register idempotently.
    pub fn register<T: AddonClass>(
        vm: &mut Vm<'_>,
        max_members: usize,
    ) -> vm::Result<Option<Self>> {
        let Some(binding) = ClassBinding::bind_present_detailed::<T>(vm.script(), max_members)?
        else {
            return Ok(None);
        };
        Self::register_binding(vm, binding).map(Some)
    }
    fn register_binding(vm: &mut Vm<'_>, binding: ClassBinding) -> vm::Result<Self> {
        let specs = specs(&binding)?;
        let class = vm.register_class(binding.class(), &specs)?;
        log_registration(&binding);
        Ok(Self { binding, class })
    }
    pub fn class(&self) -> ClassId {
        self.class
    }
    pub fn binding(&self) -> &ClassBinding {
        &self.binding
    }

    fn check(&self, key: MemberKey) -> vm::Result<usize> {
        let ordinal = key.key as usize;
        let bound = self
            .binding
            .members()
            .get(ordinal)
            .and_then(|m| *m)
            .ok_or_else(|| Fault::new(FaultKind::InvalidHandle))?;
        if key.class != self.class
            || key.symbol != bound.symbol()
            || key.count != bound.elements()
            || key.kind != member_type(self.binding.schema().members[ordinal].kind)
        {
            return Err(Fault::new(FaultKind::InvalidHandle));
        }
        Ok(ordinal)
    }

    /// Return an owned callback value after checking runtime, class and field
    /// identity. String allocation is bounded and fallible; it cannot borrow a
    /// host member across a reentrant callback.
    pub fn read<T: AddonClass>(
        &self,
        instance: &T,
        member: MemberKey,
        index: usize,
        max_bytes: usize,
    ) -> vm::Result<Value> {
        let ordinal = self.check(member)?;
        Ok(
            match self.binding.read_detailed(instance, ordinal, index)? {
                MemberValue::Int(value) => Value::Int(value),
                MemberValue::Float(value) => Value::Float(value),
                MemberValue::Bytes(value) => Value::bytes(value, max_bytes)?,
            },
        )
    }

    /// Apply a VM value to checked host storage. Failure preserves the target.
    pub fn write<T: AddonClass>(
        &self,
        instance: &mut T,
        member: MemberKey,
        index: usize,
        value: &Value,
        max_bytes: usize,
    ) -> vm::Result<()> {
        let ordinal = self.check(member)?;
        let value = match value {
            Value::Int(value) | Value::Function(value) => MemberValue::Int(*value),
            Value::Float(value) => MemberValue::Float(*value),
            Value::Bytes(value) => MemberValue::Bytes(value),
            Value::Instance(_) => {
                return Err(Fault::new(FaultKind::WrongType).with_detail(
                    FaultDetail::TypeMismatch {
                        operation: FaultOperation::MemberWrite,
                        expected: ExpectedType::Member(member_type(
                            self.binding.schema().members[ordinal].kind,
                        )),
                        given: GivenType::Value(crate::daedalus::DataType::Instance),
                    },
                ));
            }
        };
        Ok(self
            .binding
            .write_detailed(instance, ordinal, index, value, max_bytes)?)
    }
}
