// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;

static NEXT_RUNTIME: AtomicU64 = AtomicU64::new(1);
pub(super) struct Slot {
    pub values: Vec<Value>,
    pub trap: bool,
    pub locals: bool,
}
pub(super) struct Class {
    pub symbol: SymbolId,
    pub members: Vec<MemberKey>,
    pub cells: usize,
}
pub(super) struct Instance {
    pub symbol: Option<SymbolId>,
    pub class: Option<ClassId>,
    pub backend: InstanceBackend,
    pub values: Vec<Value>,
}

/// Owned mutable cells and instance identity, separate from immutable DAT storage.
/// Host-backed objects stay in Host; extracting this state retains opaque objects.
pub struct RuntimeState<'s> {
    pub(super) script: &'s Script,
    pub(super) owner: u64,
    pub(super) slots: Vec<Slot>,
    pub(super) classes: Vec<Class>,
    pub(super) members: Vec<Option<MemberKey>>,
    pub(super) instances: Vec<Instance>,
    pub(super) cells: usize,
    pub(super) bytes: usize,
    pub(super) limits: Limits,
}
impl fmt::Debug for RuntimeState<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeState")
            .field("symbols", &self.slots.len())
            .field("classes", &self.classes.len())
            .field("instances", &self.instances.len())
            .field("cells", &self.cells)
            .field("string_bytes", &self.bytes)
            .finish()
    }
}
impl<'s> RuntimeState<'s> {
    pub fn new(script: &'s Script, limits: Limits) -> Result<Self> {
        let mut out = Self {
            script,
            owner: NEXT_RUNTIME
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .map_err(|_| fault(FaultKind::Allocation))?,
            slots: Vec::new(),
            classes: Vec::new(),
            members: Vec::new(),
            instances: Vec::new(),
            cells: 0,
            bytes: 0,
            limits,
        };
        out.cells = checked(0, script.len(), limits.cells, LimitKind::Cells)?;
        reserve(&mut out.slots, script.len())?;
        reserve(&mut out.members, script.len())?;
        for s in script.symbols() {
            let n = if s.is_member() {
                0
            } else {
                match s.data_type() {
                    DataType::Int | DataType::Float | DataType::String => usize::from(s.count()),
                    DataType::Instance => 1,
                    DataType::Function if !s.is_const() => usize::from(s.count()),
                    _ => 0,
                }
            };
            out.cells = checked(out.cells, n, limits.cells, LimitKind::Cells)?;
            let mut values = Vec::new();
            reserve(&mut values, n)?;
            match s.initial_value() {
                InitialValue::Integers(v) => values.extend(v.map(Value::Int)),
                InitialValue::FloatBits(v) => {
                    values.extend(v.map(|n| Value::Float(f32::from_bits(n))))
                }
                InitialValue::Strings(v) => {
                    for raw in v {
                        let len = raw.decoded().count();
                        out.bytes =
                            checked(out.bytes, len, limits.string_bytes, LimitKind::Strings)?;
                        let mut bytes = Vec::new();
                        reserve(&mut bytes, len)?;
                        bytes.extend(raw.decoded());
                        values.push(Value::Bytes(bytes));
                    }
                }
                _ => {
                    for _ in 0..n {
                        values.push(default_value(s.data_type())?);
                    }
                }
            }
            out.slots.push(Slot {
                values,
                trap: false,
                locals: false,
            });
            out.members.push(None);
        }
        Ok(out)
    }
    pub fn script(&self) -> &'s Script {
        self.script
    }
    pub fn limits(&self) -> Limits {
        self.limits
    }
    pub fn cell_count(&self) -> usize {
        self.cells
    }
    pub fn string_bytes(&self) -> usize {
        self.bytes
    }
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }
    pub fn binding_count(&self) -> usize {
        self.classes.iter().map(|c| 1 + c.members.len()).sum()
    }
    pub fn runtime_count(&self, symbol: SymbolId) -> Result<usize> {
        let s = self.script.get(symbol)?;
        Ok(if s.is_member() {
            usize::from(s.count())
        } else {
            self.slots[s.index() as usize].values.len()
        })
    }
    pub fn global(&self, symbol: SymbolId, index: usize) -> Result<&Value> {
        let s = self.script.get(symbol)?;
        if s.is_member() {
            return Err(fault(FaultKind::NoContext)
                .at(symbol)
                .with_detail(FaultDetail::Access {
                    operation: FaultOperation::GlobalRead,
                }));
        }
        let values = &self.slots[s.index() as usize].values;
        values.get(index).ok_or_else(|| {
            fault(FaultKind::Index)
                .at(symbol)
                .element(index)
                .with_detail(FaultDetail::Index {
                    operation: FaultOperation::GlobalRead,
                    length: values.len(),
                })
        })
    }
    /// Explicit host edit: unlike a script instruction, this may change constants.
    pub fn set_global(&mut self, symbol: SymbolId, index: usize, value: Value) -> Result<()> {
        let s = self.script.get(symbol)?;
        if s.is_member() {
            return Err(fault(FaultKind::WrongType).at(symbol).types(
                FaultOperation::GlobalWrite,
                ExpectedType::NonMemberStorage,
                GivenType::Symbol(s.into()),
            ));
        }
        if !value_matches(&value, s.data_type()) {
            return Err(fault(FaultKind::WrongType).at(symbol).types(
                FaultOperation::GlobalWrite,
                ExpectedType::Storage(s.data_type()),
                GivenType::Value(value.data_type()),
            ));
        }
        self.check_value(&value)?;
        let old = self.global(symbol, index)?.bytes_len();
        let bytes = checked(
            self.bytes - old,
            value.bytes_len(),
            self.limits.string_bytes,
            LimitKind::Strings,
        )?;
        self.slots[s.index() as usize].values[index] = value;
        self.bytes = bytes;
        Ok(())
    }
    pub fn grow(&mut self, symbol: SymbolId, additional: usize) -> Result<()> {
        let s = self.script.get(symbol)?;
        if s.is_member()
            || !matches!(
                s.data_type(),
                DataType::Int | DataType::Float | DataType::String | DataType::Function
            )
            || s.data_type() == DataType::Function && s.is_const()
        {
            return Err(fault(FaultKind::WrongType).at(symbol).types(
                FaultOperation::Grow,
                ExpectedType::GrowableGlobal,
                GivenType::Symbol(s.into()),
            ));
        }
        let total = checked(self.cells, additional, self.limits.cells, LimitKind::Cells)?;
        let values = &mut self.slots[s.index() as usize].values;
        reserve(values, additional)?;
        for _ in 0..additional {
            values.push(default_value(s.data_type())?);
        }
        self.cells = total;
        Ok(())
    }
    pub fn set_access_trap(&mut self, symbol: SymbolId, enabled: bool) -> Result<()> {
        let s = self.script.get(symbol)?;
        self.slots[s.index() as usize].trap = enabled;
        Ok(())
    }
    pub fn access_trap(&self, symbol: SymbolId) -> Result<bool> {
        let s = self.script.get(symbol)?;
        Ok(self.slots[s.index() as usize].trap)
    }
    pub fn set_recursive_locals(&mut self, symbol: SymbolId, enabled: bool) -> Result<()> {
        let s = self.script.get(symbol)?;
        if s.is_member() || s.data_type() != DataType::Function {
            return Err(fault(FaultKind::WrongType).at(symbol).types(
                FaultOperation::RecursiveLocals,
                ExpectedType::TopLevelFunction,
                GivenType::Symbol(s.into()),
            ));
        }
        self.slots[s.index() as usize].locals = enabled;
        Ok(())
    }
    pub fn recursive_locals(&self, symbol: SymbolId) -> Result<bool> {
        let s = self.script.get(symbol)?;
        Ok(self.slots[s.index() as usize].locals)
    }
    pub fn class_symbol(&self, id: ClassId) -> Result<SymbolId> {
        Ok(self.class(id)?.symbol)
    }
    pub fn bound_members(&self, id: ClassId) -> Result<&[MemberKey]> {
        Ok(&self.class(id)?.members)
    }
    pub fn registered_class(&self, symbol: SymbolId) -> Result<Option<ClassId>> {
        self.script.get(symbol)?;
        Ok(self
            .classes
            .iter()
            .position(|c| c.symbol == symbol)
            .map(|index| ClassId {
                owner: self.owner,
                index,
            }))
    }
    pub(super) fn class(&self, id: ClassId) -> Result<&Class> {
        if id.owner != self.owner {
            return Err(fault(FaultKind::InvalidHandle));
        }
        self.classes
            .get(id.index)
            .ok_or_else(|| fault(FaultKind::InvalidHandle))
    }
    /// Register a complete member set atomically. Identical re-registration is
    /// idempotent; changing an existing layout is rejected, preserving old keys.
    pub fn register_class(&mut self, class: SymbolId, specs: &[MemberSpec<'_>]) -> Result<ClassId> {
        let script = self.script;
        let diagnostic_name_bytes = self.limits.diagnostic_name_bytes;
        self.register_members(
            class,
            specs.len(),
            specs.iter().map(|spec| {
                let symbol = script
                    .find_name(spec.name)
                    .ok_or_else(|| {
                        Fault::missing_name(
                            FaultOperation::RegisterClass,
                            spec.name,
                            diagnostic_name_bytes,
                        )
                    })?
                    .id();
                Ok((symbol, spec.kind, spec.capacity, spec.key))
            }),
        )
    }
    fn register_members(
        &mut self,
        class: SymbolId,
        count: usize,
        entries: impl IntoIterator<Item = Result<(SymbolId, MemberType, usize, u32)>>,
    ) -> Result<ClassId> {
        let s = self.script.get(class)?;
        if s.is_member() || s.data_type() != DataType::Class {
            return Err(fault(FaultKind::WrongType).at(class).with_detail(
                FaultDetail::Registration {
                    class,
                    reason: RegistrationReason::ClassRole { given: s.into() },
                },
            ));
        }
        let existing = self.registered_class(class)?;
        let id = existing.unwrap_or(ClassId {
            owner: self.owner,
            index: self.classes.len(),
        });
        let total = self.binding_count();
        let old = existing.map_or(0, |id| 1 + self.classes[id.index].members.len());
        // Bound the proposed replacement before allocating its scratch map too.
        checked(total - old, 1, self.limits.bindings, LimitKind::Bindings)?;
        checked(
            total - old + 1,
            count,
            self.limits.bindings,
            LimitKind::Bindings,
        )?;
        let mut members = Vec::new();
        reserve(&mut members, count)?;
        let mut offset = 0;
        for entry in entries {
            let (symbol, kind, capacity, key) = entry?;
            let m = self.script.get(symbol)?;
            let reason = if !m.is_member() {
                Some(RegistrationReason::NotMember { given: m.into() })
            } else if m.parent() != s.index() as i32 {
                Some(RegistrationReason::Parent { given: m.parent() })
            } else if !kind.accepts(m.data_type()) {
                Some(RegistrationReason::Type {
                    expected: kind,
                    given: m.data_type(),
                })
            } else if usize::from(m.count()) > capacity {
                Some(RegistrationReason::Capacity {
                    capacity,
                    elements: usize::from(m.count()),
                })
            } else {
                None
            };
            if let Some(reason) = reason {
                return Err(fault(FaultKind::Signature)
                    .at(m.id())
                    .with_detail(FaultDetail::Registration { class, reason }));
            }
            let count = usize::from(m.count());
            members.push(MemberKey {
                symbol: m.id(),
                key,
                kind,
                count,
                class: id,
                offset,
            });
            if !matches!(kind, MemberType::NonScalar(_)) {
                offset = checked(offset, count, self.limits.cells, LimitKind::Cells)?;
            }
        }
        members.sort_unstable_by_key(|m| m.symbol.index());
        if let Some(pair) = members.windows(2).find(|v| v[0].symbol == v[1].symbol) {
            return Err(fault(FaultKind::Signature).at(class).with_detail(
                FaultDetail::Registration {
                    class,
                    reason: RegistrationReason::Duplicate {
                        member: pair[0].symbol,
                    },
                },
            ));
        }
        if let Some(id) = existing {
            let old = &self.class(id)?.members;
            if old.len() != members.len()
                || old.iter().zip(&members).any(|(a, b)| {
                    a.symbol != b.symbol
                        || a.key != b.key
                        || a.kind != b.kind
                        || a.offset != b.offset
                })
            {
                return Err(fault(FaultKind::WrongClass).at(class).with_detail(
                    FaultDetail::Registration {
                        class,
                        reason: RegistrationReason::ChangedLayout,
                    },
                ));
            }
            return Ok(id);
        }
        reserve(&mut self.classes, 1)?;
        for &m in &members {
            self.members[m.symbol.index() as usize] = Some(m);
        }
        self.classes.push(Class {
            symbol: class,
            members,
            cells: offset,
        });
        Ok(id)
    }
    /// Atomically registers a batch. Failure leaves all logical bindings intact;
    /// spare Vec capacity is not part of observable registration state.
    pub fn register_classes(&mut self, classes: &[ClassRegistration<'_>]) -> Result<Vec<ClassId>> {
        checked(0, classes.len(), self.limits.bindings, LimitKind::Bindings)?;
        let mut ids = Vec::new();
        reserve(&mut ids, classes.len())?;
        let start = self.classes.len();
        for c in classes {
            match self.register_class(c.class, c.members) {
                Ok(id) => ids.push(id),
                Err(e) => {
                    while self.classes.len() > start {
                        let class = self.classes.pop().unwrap();
                        for m in class.members {
                            self.members[m.symbol.index() as usize] = None;
                        }
                    }
                    return Err(e);
                }
            }
        }
        Ok(ids)
    }
    /// Bind each declared member by its exact symbol identity. Names may be
    /// absent or duplicated; opaque storage never resolves them through lookup.
    pub fn register_opaque(&mut self, class: SymbolId) -> Result<ClassId> {
        let script = self.script;
        let count = script.class_members(class)?.count();
        self.register_members(
            class,
            count,
            script.class_members(class)?.enumerate().map(|(key, m)| {
                let kind = match m.data_type() {
                    DataType::Int | DataType::Function => MemberType::Int,
                    DataType::Float => MemberType::Float,
                    DataType::String => MemberType::String,
                    kind => MemberType::NonScalar(kind),
                };
                Ok((
                    m.id(),
                    kind,
                    usize::from(m.count()),
                    u32::try_from(key).map_err(|_| fault(FaultKind::Limit(LimitKind::Bindings)))?,
                ))
            }),
        )
    }
    pub(super) fn check_value(&self, value: &Value) -> Result<()> {
        if let Value::Instance(Some(id)) = value {
            self.instance(*id)?;
        }
        Ok(())
    }
    pub(super) fn instance(&self, id: InstanceId) -> Result<&Instance> {
        if id.owner != self.owner {
            return Err(fault(FaultKind::InvalidHandle));
        }
        self.instances
            .get(id.index)
            .ok_or_else(|| fault(FaultKind::InvalidHandle))
    }
    pub fn instance_symbol(&self, id: InstanceId) -> Result<Option<SymbolId>> {
        Ok(self.instance(id)?.symbol)
    }
    pub fn instance_class(&self, id: InstanceId) -> Result<Option<ClassId>> {
        Ok(self.instance(id)?.class)
    }
    pub fn instance_backend(&self, id: InstanceId) -> Result<InstanceBackend> {
        Ok(self.instance(id)?.backend)
    }
    pub fn allocate_host(
        &mut self,
        symbol: SymbolId,
        class: ClassId,
        key: HostKey,
    ) -> Result<InstanceId> {
        self.allocate(symbol, class, InstanceBackend::Host(key))
    }
    pub fn allocate_opaque(&mut self, symbol: SymbolId, class: ClassId) -> Result<InstanceId> {
        self.allocate(symbol, class, InstanceBackend::Opaque)
    }
    fn allocate(
        &mut self,
        symbol: SymbolId,
        class: ClassId,
        backend: InstanceBackend,
    ) -> Result<InstanceId> {
        self.check_instance_symbol(symbol, class)?;
        checked(
            self.instances.len(),
            1,
            self.limits.instances,
            LimitKind::Instances,
        )?;
        let n = if backend == InstanceBackend::Opaque {
            self.class(class)?.cells
        } else {
            0
        };
        let cells = checked(self.cells, n + 1, self.limits.cells, LimitKind::Cells)?;
        let mut values = Vec::new();
        reserve(&mut values, n)?;
        for _ in 0..n {
            values.push(Value::Int(0));
        }
        if backend == InstanceBackend::Opaque {
            for m in &self.class(class)?.members {
                if matches!(m.kind, MemberType::NonScalar(_)) {
                    continue;
                }
                for i in 0..m.count {
                    values[m.offset + i] = match m.kind {
                        MemberType::Int => Value::Int(0),
                        MemberType::Float => Value::Float(0.0),
                        MemberType::String => Value::Bytes(Vec::new()),
                        MemberType::NonScalar(_) => unreachable!(),
                    };
                }
            }
        }
        reserve(&mut self.instances, 1)?;
        let id = InstanceId {
            owner: self.owner,
            index: self.instances.len(),
        };
        self.instances.push(Instance {
            symbol: Some(symbol),
            class: Some(class),
            backend,
            values,
        });
        self.cells = cells;
        self.slots[symbol.index() as usize].values[0] = Value::Instance(Some(id));
        Ok(id)
    }
    pub fn allocate_transient(&mut self, key: HostKey) -> Result<InstanceId> {
        checked(
            self.instances.len(),
            1,
            self.limits.instances,
            LimitKind::Instances,
        )?;
        let cells = checked(self.cells, 1, self.limits.cells, LimitKind::Cells)?;
        reserve(&mut self.instances, 1)?;
        let id = InstanceId {
            owner: self.owner,
            index: self.instances.len(),
        };
        self.instances.push(Instance {
            symbol: None,
            class: None,
            backend: InstanceBackend::Transient(key),
            values: Vec::new(),
        });
        self.cells = cells;
        Ok(id)
    }
    fn check_instance_symbol(&self, symbol: SymbolId, class: ClassId) -> Result<()> {
        let s = self.script.get(symbol)?;
        if s.is_member() || s.data_type() != DataType::Instance {
            return Err(fault(FaultKind::WrongType).at(symbol).types(
                FaultOperation::InstanceBinding,
                ExpectedType::TopLevelInstance,
                GivenType::Symbol(s.into()),
            ));
        }
        let given = self
            .script
            .class_of(symbol)
            .map_err(|error| {
                Fault::from(error)
                    .at(symbol)
                    .with_detail(FaultDetail::Access {
                        operation: FaultOperation::InstanceBinding,
                    })
            })?
            .id();
        let expected = self.class(class)?.symbol;
        if given != expected {
            return Err(fault(FaultKind::WrongClass).at(symbol).with_detail(
                FaultDetail::ClassMismatch {
                    expected,
                    given: Some(given),
                    instance: None,
                },
            ));
        }
        Ok(())
    }
    /// Rebinding preserves all earlier symbol aliases; reverse lookup identifies
    /// the latest successful binding, matching the reference instance metadata.
    pub fn bind_instance(&mut self, id: InstanceId, symbol: SymbolId) -> Result<()> {
        let class = self
            .instance(id)?
            .class
            .ok_or_else(|| fault(FaultKind::WrongClass))?;
        self.check_instance_symbol(symbol, class)?;
        self.instances[id.index].symbol = Some(symbol);
        self.slots[symbol.index() as usize].values[0] = Value::Instance(Some(id));
        Ok(())
    }
    pub fn opaque_member(&self, id: InstanceId, symbol: SymbolId, index: usize) -> Result<&Value> {
        let m = self.resolve_member(id, symbol, index)?;
        if self.instance(id)?.backend != InstanceBackend::Opaque {
            return Err(fault(FaultKind::WrongType));
        }
        Ok(&self.instances[id.index].values[m.offset + index])
    }
    pub fn set_opaque_member(
        &mut self,
        id: InstanceId,
        symbol: SymbolId,
        index: usize,
        value: Value,
    ) -> Result<()> {
        let m = self.resolve_member(id, symbol, index)?;
        if self.instance(id)?.backend != InstanceBackend::Opaque {
            return Err(fault(FaultKind::WrongType));
        }
        let s = self.script.get(symbol)?;
        if !value_matches(&value, s.data_type()) {
            return Err(fault(FaultKind::WrongType).at(symbol).types(
                FaultOperation::MemberWrite,
                ExpectedType::Storage(s.data_type()),
                GivenType::Value(value.data_type()),
            ));
        }
        self.check_value(&value)?;
        let old = self.instances[id.index].values[m.offset + index].bytes_len();
        let bytes = checked(
            self.bytes - old,
            value.bytes_len(),
            self.limits.string_bytes,
            LimitKind::Strings,
        )?;
        self.instances[id.index].values[m.offset + index] = value;
        self.bytes = bytes;
        Ok(())
    }
    pub(super) fn resolve_member(
        &self,
        id: InstanceId,
        symbol: SymbolId,
        index: usize,
    ) -> Result<MemberKey> {
        let s = self.script.get(symbol)?;
        let instance = self.instance(id)?;
        let m = self.members[s.index() as usize]
            .ok_or_else(|| fault(FaultKind::UnboundMember).at(symbol))?;
        if instance.class != Some(m.class) {
            let expected = self.class(m.class)?.symbol;
            let given = instance
                .class
                .map(|c| self.class(c).map(|c| c.symbol))
                .transpose()?;
            return Err(fault(FaultKind::WrongClass).at(symbol).with_detail(
                FaultDetail::ClassMismatch {
                    expected,
                    given,
                    instance: Some(id),
                },
            ));
        }
        if matches!(m.kind, MemberType::NonScalar(_)) {
            return Err(fault(FaultKind::WrongType).at(symbol).types(
                FaultOperation::MemberAccess,
                ExpectedType::ScalarMember,
                GivenType::Symbol(s.into()),
            ));
        }
        if index >= m.count {
            return Err(fault(FaultKind::Index)
                .at(symbol)
                .element(index)
                .with_detail(FaultDetail::Index {
                    operation: FaultOperation::MemberAccess,
                    length: m.count,
                }));
        }
        Ok(m)
    }
    pub fn resolve_function(&self, index: i32) -> Result<Option<SymbolId>> {
        let mut current = index;
        for _ in 0..=self.script.len() {
            let Some(s) = u32::try_from(current)
                .ok()
                .and_then(|i| self.script.symbol(i))
            else {
                return Ok(None);
            };
            if s.is_member() || s.data_type() != DataType::Function {
                return Ok(None);
            }
            if s.is_const() {
                return Ok(Some(s.id()));
            }
            current = self.global(s.id(), 0)?.as_int()?;
        }
        Err(fault(FaultKind::Cycle))
    }
}
