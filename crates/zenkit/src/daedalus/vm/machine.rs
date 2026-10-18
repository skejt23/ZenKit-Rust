// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;

pub(super) enum StackValue {
    Value(Value),
    Reference(Reference),
}
impl StackValue {
    pub fn bytes(&self) -> usize {
        match self {
            Self::Value(v) => v.bytes_len(),
            Self::Reference(_) => 0,
        }
    }
}
pub(super) struct Frame {
    pub trace: FrameTrace,
    pub result: DataType,
    pub locals: Vec<(SymbolId, Vec<Value>)>,
    pub local_cells: usize,
    pub local_bytes: usize,
}
pub(super) struct Binding {
    pub symbol: SymbolId,
    pub key: u32,
    pub parameters: Vec<Parameter>,
    pub result: DataType,
    pub raw: bool,
    pub address: u32,
}
#[derive(Clone, Copy)]
pub(super) enum DefaultBinding {
    Error,
    Zero(Option<u32>),
    Raw(u32),
}

/// An executing machine owns its mutable state and checked code index while
/// borrowing immutable Script storage. It does not own or borrow Host between calls.
pub struct Vm<'s> {
    pub(super) code: Executable<'s>,
    pub(super) state: RuntimeState<'s>,
    pub(super) policy: Policy,
    pub(super) stack: Vec<StackValue>,
    pub(super) frames: Vec<Frame>,
    pub(super) externals: Vec<Binding>,
    pub(super) overrides: Vec<Binding>,
    pub(super) default: DefaultBinding,
    pub(super) pc: u32,
    pub(super) context: Option<InstanceId>,
    pub(super) temporary_bytes: usize,
    pub(super) snapshot_cells: usize,
    pub(super) fuel: u64,
    pub(super) host_depth: usize,
    pub(super) invocations: usize,
    pub(super) terminal: Option<FaultKind>,
}
impl fmt::Debug for Vm<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Vm")
            .field("pc", &self.pc)
            .field("stack", &self.stack.len())
            .field("frames", &self.frames.len())
            .field("state", &self.state)
            .finish()
    }
}
impl<'s> Vm<'s> {
    pub fn new(code: Executable<'s>, policy: Policy, limits: Limits) -> Result<Self> {
        let state = RuntimeState::new(code.script(), limits)?;
        Self::with_state(code, state, policy)
    }
    /// Runtime-aware validation retains all opcode/role/branch checks, while
    /// PUSHVV extents are checked at execution against possibly grown arrays.
    pub fn from_script(
        script: &'s Script,
        decode: DecodeLimits,
        policy: Policy,
        limits: Limits,
    ) -> Result<Self> {
        Self::new(script.validate_code_inner(decode, false)?, policy, limits)
    }
    pub fn with_state(
        code: Executable<'s>,
        state: RuntimeState<'s>,
        policy: Policy,
    ) -> Result<Self> {
        if !std::ptr::eq(code.script(), state.script) {
            return Err(fault(FaultKind::InvalidHandle));
        }
        Ok(Self {
            code,
            state,
            policy,
            stack: Vec::new(),
            frames: Vec::new(),
            externals: Vec::new(),
            overrides: Vec::new(),
            default: DefaultBinding::Error,
            pc: 0,
            context: None,
            temporary_bytes: 0,
            snapshot_cells: 0,
            fuel: 0,
            host_depth: 0,
            invocations: 0,
            terminal: None,
        })
    }
    pub fn into_state(self) -> RuntimeState<'s> {
        self.state
    }
    pub fn state(&self) -> &RuntimeState<'s> {
        &self.state
    }
    pub fn script(&self) -> &'s Script {
        self.code.script()
    }
    pub fn policy(&self) -> Policy {
        self.policy
    }
    pub fn set_policy(&mut self, policy: Policy) {
        self.policy = policy;
    }
    pub fn pc(&self) -> u32 {
        self.pc
    }
    pub fn fuel_remaining(&self) -> u64 {
        self.fuel
    }
    pub fn stack_len(&self) -> usize {
        self.stack.len()
    }
    pub fn call_depth(&self) -> usize {
        self.frames.len()
    }
    pub fn trace(&self) -> impl DoubleEndedIterator<Item = FrameTrace> + ExactSizeIterator + '_ {
        self.frames.iter().map(|f| f.trace)
    }
    /// All retained stack entries, oldest first, including callers' entries.
    pub fn stack(
        &self,
    ) -> impl DoubleEndedIterator<Item = StackEntry<'_>> + ExactSizeIterator + '_ {
        self.stack.iter().map(|v| match v {
            StackValue::Value(v) => StackEntry::Value(v),
            StackValue::Reference(r) => StackEntry::Reference(*r),
        })
    }
    pub fn current_instance(&self) -> Option<InstanceId> {
        self.context
    }
    pub fn set_current_instance(&mut self, instance: Option<InstanceId>) -> Result<()> {
        if let Some(i) = instance {
            self.state.instance(i)?;
        }
        self.context = instance;
        Ok(())
    }
    pub fn global(&self, name: &[u8]) -> Option<SymbolId> {
        self.script().find_name(name).map(|s| s.id())
    }
    pub fn global_self(&self) -> Option<SymbolId> {
        self.global(b"SELF")
    }
    pub fn global_other(&self) -> Option<SymbolId> {
        self.global(b"OTHER")
    }
    pub fn global_victim(&self) -> Option<SymbolId> {
        self.global(b"VICTIM")
    }
    pub fn global_hero(&self) -> Option<SymbolId> {
        self.global(b"HERO")
    }
    pub fn global_item(&self) -> Option<SymbolId> {
        self.global(b"ITEM")
    }
    pub(super) fn note<T>(&mut self, result: Result<T>) -> Result<T> {
        if let Err(e) = &result
            && e.is_terminal()
            && (self.invocations != 0 || self.host_depth != 0)
        {
            self.terminal = Some(e.kind);
        }
        result
    }
    pub(super) fn free_strings(&self) -> usize {
        self.state
            .limits
            .string_bytes
            .saturating_sub(self.state.bytes)
            .saturating_sub(self.temporary_bytes)
    }
    pub fn register_class(&mut self, class: SymbolId, specs: &[MemberSpec<'_>]) -> Result<ClassId> {
        self.class_edit(|state| state.register_class(class, specs))
    }
    pub fn register_classes(&mut self, classes: &[ClassRegistration<'_>]) -> Result<Vec<ClassId>> {
        self.class_edit(|state| state.register_classes(classes))
    }
    pub fn register_opaque(&mut self, class: SymbolId) -> Result<ClassId> {
        self.class_edit(|state| state.register_opaque(class))
    }
    fn class_edit<T>(
        &mut self,
        edit: impl FnOnce(&mut RuntimeState<'s>) -> Result<T>,
    ) -> Result<T> {
        // Registration has no callbacks; restore the full policy before exposing state.
        let limit = self.state.limits.bindings;
        self.state.limits.bindings = limit.saturating_sub(self.function_binding_count());
        let result = edit(&mut self.state);
        self.state.limits.bindings = limit;
        self.note(result)
    }
    fn function_binding_count(&self) -> usize {
        self.externals
            .iter()
            .chain(&self.overrides)
            .map(|v| 1 + v.parameters.len())
            .sum()
    }
    pub(super) fn external_index(&self, symbol: SymbolId) -> Option<usize> {
        self.externals
            .binary_search_by_key(&symbol.index(), |b| b.symbol.index())
            .ok()
    }
    pub(super) fn override_index(&self, address: u32) -> Option<usize> {
        self.overrides
            .binary_search_by_key(&address, |b| b.address)
            .ok()
    }
    pub fn allocate_host(
        &mut self,
        symbol: SymbolId,
        class: ClassId,
        key: HostKey,
    ) -> Result<InstanceId> {
        let r = self.state.allocate_host(symbol, class, key);
        self.note(r)
    }
    pub fn allocate_opaque(&mut self, symbol: SymbolId, class: ClassId) -> Result<InstanceId> {
        let r = self.state.allocate_opaque(symbol, class);
        self.note(r)
    }
    pub fn allocate_transient(&mut self, key: HostKey) -> Result<InstanceId> {
        let r = self.state.allocate_transient(key);
        self.note(r)
    }
    pub fn bind_instance(&mut self, id: InstanceId, symbol: SymbolId) -> Result<()> {
        self.state.bind_instance(id, symbol)
    }
    pub fn grow(&mut self, symbol: SymbolId, n: usize) -> Result<()> {
        let r = self.state.grow(symbol, n);
        self.note(r)
    }
    pub fn set_access_trap(&mut self, symbol: SymbolId, enabled: bool) -> Result<()> {
        self.state.set_access_trap(symbol, enabled)
    }
    pub fn set_recursive_locals(&mut self, symbol: SymbolId, enabled: bool) -> Result<()> {
        self.state.set_recursive_locals(symbol, enabled)
    }
    pub fn set_global(&mut self, symbol: SymbolId, index: usize, value: Value) -> Result<()> {
        let old = self.state.global(symbol, index)?.bytes_len();
        let r = checked(
            self.state.bytes + self.temporary_bytes - old,
            value.bytes_len(),
            self.state.limits.string_bytes,
            LimitKind::Strings,
        );
        self.note(r)?;
        let r = self.state.set_global(symbol, index, value);
        self.note(r)
    }
    /// Explicit host edit of opaque storage, independent of script CONST policy.
    pub fn set_opaque_member(
        &mut self,
        id: InstanceId,
        symbol: SymbolId,
        index: usize,
        value: Value,
    ) -> Result<()> {
        let old = self.state.opaque_member(id, symbol, index)?.bytes_len();
        let rr = checked(
            self.state.bytes + self.temporary_bytes - old,
            value.bytes_len(),
            self.state.limits.string_bytes,
            LimitKind::Strings,
        );
        self.note(rr)?;
        let rr = self.state.set_opaque_member(id, symbol, index, value);
        self.note(rr)
    }
    pub fn reference(
        &self,
        symbol: SymbolId,
        index: usize,
        context: Option<InstanceId>,
    ) -> Result<Reference> {
        self.script().get(symbol)?;
        if let Some(i) = context {
            self.state.instance(i)?;
        }
        Ok(Reference {
            symbol,
            index,
            context,
        })
    }
    pub fn push_reference(&mut self, symbol: SymbolId, index: usize) -> Result<()> {
        let r = self.reference(symbol, index, self.context)?;
        self.push_ref(r)
    }
    pub fn push_ref(&mut self, r: Reference) -> Result<()> {
        self.reference(r.symbol, r.index, r.context)?;
        self.push_stack(StackValue::Reference(r))
    }
    pub fn push(&mut self, value: Value) -> Result<()> {
        self.state.check_value(&value)?;
        self.push_stack(StackValue::Value(value))
    }
    pub fn push_string(&mut self, bytes: &[u8]) -> Result<()> {
        let r = Value::bytes(bytes, self.free_strings());
        let v = self.note(r)?;
        self.push(v)
    }
    pub(super) fn push_stack(&mut self, value: StackValue) -> Result<()> {
        let r = checked(
            self.stack.len(),
            1,
            self.state.limits.stack,
            LimitKind::Stack,
        );
        self.note(r)?;
        let r = checked(
            self.state.bytes + self.temporary_bytes,
            value.bytes(),
            self.state.limits.string_bytes,
            LimitKind::Strings,
        );
        self.note(r)?;
        let r = reserve(&mut self.stack, 1);
        self.note(r)?;
        self.temporary_bytes += value.bytes();
        self.stack.push(value);
        Ok(())
    }
    pub fn top_is_reference(&self) -> Result<bool> {
        if self.stack_empty() {
            return Err(fault(FaultKind::StackUnderflow));
        }
        Ok(matches!(
            self.stack.last().unwrap(),
            StackValue::Reference(_) | StackValue::Value(Value::Bytes(_))
        ))
    }
    pub fn pop_reference(&mut self) -> Result<StackReference> {
        match self.pop_stack()? {
            StackValue::Reference(r) => Ok(StackReference::Symbol(r)),
            StackValue::Value(Value::Bytes(v)) => Ok(StackReference::Temporary(v)),
            StackValue::Value(v) => Err(fault(FaultKind::WrongType).types(
                FaultOperation::StackPop,
                ExpectedType::Reference,
                GivenType::Value(v.data_type()),
            )),
        }
    }
    pub fn push_stack_reference(&mut self, r: StackReference) -> Result<()> {
        match r {
            StackReference::Symbol(r) => self.push_ref(r),
            StackReference::Temporary(v) => self.push(Value::Bytes(v)),
        }
    }
    pub(super) fn stack_empty(&self) -> bool {
        self.stack.len() <= self.frames.last().map_or(0, |f| f.trace.stack_base)
    }
    pub(super) fn argument_base(&self, count: usize) -> Result<usize> {
        let floor = self.frames.last().map_or(0, |f| f.trace.stack_base);
        let base = self
            .stack
            .len()
            .checked_sub(count)
            .filter(|base| *base >= floor)
            .ok_or_else(|| fault(FaultKind::StackUnderflow))?;
        Ok(base)
    }
    pub(super) fn pop_stack(&mut self) -> Result<StackValue> {
        if self.stack_empty() {
            return Err(fault(FaultKind::StackUnderflow));
        }
        let v = self.stack.pop().unwrap();
        self.temporary_bytes -= v.bytes();
        Ok(v)
    }
    pub(super) fn truncate_stack(&mut self, n: usize) {
        while self.stack.len() > n {
            let v = self.stack.pop().unwrap();
            self.temporary_bytes -= v.bytes();
        }
    }
    pub fn jump(&mut self, pc: u32) -> Result<()> {
        if !self.code.decoded().contains(pc) {
            return Err(
                fault(FaultKind::CodeBoundary).with_detail(FaultDetail::Address { requested: pc })
            );
        }
        self.pc = pc;
        Ok(())
    }
    pub fn pop_int<H: Host>(&mut self, host: &mut H) -> VmResult<i32, H::Error> {
        if self.stack_empty() && self.policy.numeric_underflow_zero {
            return Ok(0);
        }
        let (v, symbol) = self.pop_value_with_symbol(host)?;
        Ok(v.as_int().map_err(|e| e.extracted(symbol))?)
    }
    pub fn pop_float<H: Host>(&mut self, host: &mut H) -> VmResult<f32, H::Error> {
        if self.stack_empty() && self.policy.numeric_underflow_zero {
            return Ok(0.0);
        }
        let (v, symbol) = self.pop_value_with_symbol(host)?;
        Ok(v.as_float().map_err(|e| e.extracted(symbol))?)
    }
    pub fn pop_instance<H: Host>(
        &mut self,
        _host: &mut H,
    ) -> VmResult<Option<InstanceId>, H::Error> {
        let (v, symbol) = match self.pop_stack()? {
            StackValue::Reference(r) => (
                self.state
                    .global(r.symbol, 0)?
                    .try_copy(self.free_strings())?,
                Some(r.symbol),
            ),
            StackValue::Value(v) => (v, None),
        };
        Ok(v.as_instance().map_err(|e| e.extracted(symbol))?)
    }
    pub fn pop_string<H: Host>(&mut self, host: &mut H) -> VmResult<Vec<u8>, H::Error> {
        let (value, symbol) = self.pop_value_with_symbol(host)?;
        let given = value.data_type();
        match value {
            Value::Bytes(v) => Ok(v),
            _ => Err(fault(FaultKind::WrongType)
                .types(
                    FaultOperation::StackPop,
                    ExpectedType::Exact(DataType::String),
                    GivenType::Value(given),
                )
                .extracted(symbol)
                .into()),
        }
    }
    fn pop_value_with_symbol<H: Host>(
        &mut self,
        host: &mut H,
    ) -> VmResult<(Value, Option<SymbolId>), H::Error> {
        match self.pop_stack()? {
            StackValue::Value(v) => Ok((v, None)),
            StackValue::Reference(r) => self.read_reference(host, r).map(|v| (v, Some(r.symbol))),
        }
    }
    pub fn read_reference<H: Host>(
        &mut self,
        host: &mut H,
        r: Reference,
    ) -> VmResult<Value, H::Error> {
        self.reference(r.symbol, r.index, r.context)?;
        let s = self.script().get(r.symbol).map_err(Fault::from)?;
        if !s.is_member() {
            let v = self
                .state
                .global(r.symbol, r.index)?
                .try_copy(self.free_strings());
            return Ok(self.note(v)?);
        }
        let Some(id) = r.context else {
            if self.policy.allow_null_members {
                return Ok(default_value(s.data_type())?);
            }
            return Err(fault(FaultKind::NoContext)
                .at(r.symbol)
                .with_detail(FaultDetail::Access {
                    operation: FaultOperation::ReferenceRead,
                })
                .into());
        };
        if !matches!(
            s.data_type(),
            DataType::Int | DataType::Function | DataType::Float | DataType::String
        ) {
            return Err(fault(FaultKind::WrongType)
                .at(r.symbol)
                .types(
                    FaultOperation::ReferenceRead,
                    ExpectedType::ScalarMember,
                    GivenType::Symbol(s.into()),
                )
                .into());
        }
        let backend = self.state.instance(id)?.backend;
        if r.index >= usize::from(s.count()) {
            return Err(fault(FaultKind::Index)
                .at(r.symbol)
                .element(r.index)
                .with_detail(FaultDetail::Index {
                    operation: FaultOperation::ReferenceRead,
                    length: usize::from(s.count()),
                })
                .into());
        }
        let v = match backend {
            InstanceBackend::Opaque => {
                let v = self
                    .state
                    .opaque_member(id, r.symbol, r.index)?
                    .try_copy(self.free_strings());
                self.note(v)?
            }
            InstanceBackend::Host(key) => {
                let m = self.state.resolve_member(id, r.symbol, r.index)?;
                self.host(host, |h, cx| h.get_member(cx, key, m, r.index))?
            }
            InstanceBackend::Transient(key) => {
                self.host(host, |h, cx| h.get_transient(cx, key, r.symbol, r.index))?
            }
        };
        if !value_matches(&v, s.data_type()) {
            return Err(fault(FaultKind::WrongType)
                .at(r.symbol)
                .types(
                    FaultOperation::ReferenceRead,
                    ExpectedType::Storage(s.data_type()),
                    GivenType::Value(v.data_type()),
                )
                .into());
        }
        self.state.check_value(&v)?;
        let rr = checked(0, v.bytes_len(), self.free_strings(), LimitKind::Strings);
        self.note(rr)?;
        Ok(v)
    }
    pub fn write_reference<H: Host>(
        &mut self,
        host: &mut H,
        r: Reference,
        v: Value,
    ) -> VmResult<(), H::Error> {
        self.reference(r.symbol, r.index, r.context)?;
        let s = self.script().get(r.symbol).map_err(Fault::from)?;
        if s.is_const() && !self.policy.ignore_const {
            return Err(fault(FaultKind::Constant)
                .at(r.symbol)
                .with_detail(FaultDetail::Access {
                    operation: FaultOperation::ReferenceWrite,
                })
                .into());
        }
        if !value_matches(&v, s.data_type()) {
            return Err(fault(FaultKind::WrongType)
                .at(r.symbol)
                .types(
                    FaultOperation::ReferenceWrite,
                    ExpectedType::Storage(s.data_type()),
                    GivenType::Value(v.data_type()),
                )
                .into());
        }
        self.state.check_value(&v)?;
        if !s.is_member() {
            return Ok(self.set_global(r.symbol, r.index, v)?);
        }
        let Some(id) = r.context else {
            if self.policy.allow_null_members {
                return Ok(());
            }
            return Err(fault(FaultKind::NoContext)
                .at(r.symbol)
                .with_detail(FaultDetail::Access {
                    operation: FaultOperation::ReferenceWrite,
                })
                .into());
        };
        if r.index >= usize::from(s.count()) {
            return Err(fault(FaultKind::Index)
                .at(r.symbol)
                .element(r.index)
                .with_detail(FaultDetail::Index {
                    operation: FaultOperation::ReferenceWrite,
                    length: usize::from(s.count()),
                })
                .into());
        }
        if !matches!(
            s.data_type(),
            DataType::Int | DataType::Function | DataType::Float | DataType::String
        ) {
            return Err(fault(FaultKind::WrongType)
                .at(r.symbol)
                .types(
                    FaultOperation::ReferenceWrite,
                    ExpectedType::ScalarMember,
                    GivenType::Symbol(s.into()),
                )
                .into());
        }
        match self.state.instance(id)?.backend {
            InstanceBackend::Opaque => {
                let old = self.state.opaque_member(id, r.symbol, r.index)?.bytes_len();
                let rr = checked(
                    self.state.bytes + self.temporary_bytes - old,
                    v.bytes_len(),
                    self.state.limits.string_bytes,
                    LimitKind::Strings,
                );
                self.note(rr)?;
                let rr = self.state.set_opaque_member(id, r.symbol, r.index, v);
                self.note(rr)?;
                Ok(())
            }
            InstanceBackend::Host(key) => {
                let m = self.state.resolve_member(id, r.symbol, r.index)?;
                self.host_value(host, v, |h, cx, v| h.set_member(cx, key, m, r.index, v))
            }
            InstanceBackend::Transient(key) => self.host_value(host, v, |h, cx, v| {
                h.set_transient(cx, key, r.symbol, r.index, v)
            }),
        }
    }
    fn host_value<H: Host, T>(
        &mut self,
        host: &mut H,
        value: Value,
        call: impl FnOnce(&mut H, &mut HostContext<'_, 's>, Value) -> VmResult<T, H::Error>,
    ) -> VmResult<T, H::Error> {
        let bytes = value.bytes_len();
        let rr = checked(0, bytes, self.free_strings(), LimitKind::Strings);
        self.note(rr)?;
        self.temporary_bytes += bytes;
        let result = self.host(host, |h, cx| call(h, cx, value));
        self.temporary_bytes -= bytes;
        result
    }
    pub(super) fn host<H: Host, T>(
        &mut self,
        host: &mut H,
        call: impl FnOnce(&mut H, &mut HostContext<'_, 's>) -> VmResult<T, H::Error>,
    ) -> VmResult<T, H::Error> {
        if self.invocations == 0 && self.host_depth == 0 {
            self.terminal = None;
            self.fuel = self.state.limits.fuel;
        }
        let rr = checked(
            self.host_depth,
            1,
            self.state.limits.host_depth,
            LimitKind::HostDepth,
        );
        self.note(rr)?;
        self.host_depth += 1;
        let result = call(host, &mut HostContext { vm: self });
        self.host_depth -= 1;
        if let Some(kind) = self.terminal {
            return match result {
                Err(e) if e.fault.kind == kind => Err(e),
                _ => Err(fault(kind).into()),
            };
        }
        if let Err(e) = &result
            && e.fault.is_terminal()
        {
            self.terminal = Some(e.fault.kind);
        }
        result
    }
    pub(super) fn result_type(&self, s: SymbolRef<'_>) -> Result<DataType> {
        if s.has_return() {
            if s.data_type() != DataType::Function {
                return Err(fault(FaultKind::Signature).at(s.id()).types(
                    FaultOperation::Return,
                    ExpectedType::TopLevelFunction,
                    GivenType::Symbol(s.into()),
                ));
            }
            let t = s.return_type()?;
            if matches!(t, DataType::Void | DataType::Class | DataType::Prototype) {
                return Err(fault(FaultKind::Signature).at(s.id()).types(
                    FaultOperation::Return,
                    ExpectedType::ReturnValue,
                    GivenType::Type(t),
                ));
            }
            Ok(t)
        } else {
            Ok(DataType::Void)
        }
    }
    pub(super) fn parameter_count(&self, s: SymbolRef<'_>) -> Result<usize> {
        if s.data_type() == DataType::Function {
            Ok(self.script().parameters(s.id())?.len())
        } else {
            Ok(0)
        }
    }
    fn binding(
        &self,
        symbol: SymbolId,
        key: u32,
        parameters: &[Parameter],
        result: DataType,
        raw: bool,
        over: bool,
    ) -> Result<Binding> {
        let s = self.script().get(symbol)?;
        let operation = if over {
            FaultOperation::OverrideFunction
        } else {
            FaultOperation::RegisterExternal
        };
        if s.is_member()
            || !matches!(
                s.data_type(),
                DataType::Function | DataType::Prototype | DataType::Instance
            )
        {
            return Err(fault(FaultKind::WrongType).at(symbol).types(
                operation,
                ExpectedType::Callable,
                GivenType::Symbol(s.into()),
            ));
        }
        if !raw {
            let expected = self.result_type(s)?;
            if expected != result {
                return Err(fault(FaultKind::Signature).at(symbol).with_detail(
                    FaultDetail::Return {
                        callable: Some(symbol),
                        expected: ReturnExpectation::Type(expected),
                        given: GivenType::Type(result),
                    },
                ));
            }
            let expected = self.parameter_count(s)?;
            if expected != parameters.len() {
                return Err(fault(FaultKind::Signature).at(symbol).with_detail(
                    FaultDetail::Counts {
                        operation,
                        role: CountRole::Arguments,
                        expected,
                        given: parameters.len(),
                    },
                ));
            }
            if s.data_type() == DataType::Function {
                for (position, (p, provided)) in self
                    .script()
                    .parameters(symbol)?
                    .zip(parameters)
                    .enumerate()
                {
                    let matches = match provided {
                        Parameter::InstanceReference => p.data_type() == DataType::Instance,
                        Parameter::Value(t) => {
                            *t == p.data_type()
                                || matches!(
                                    (*t, p.data_type()),
                                    (
                                        DataType::Int | DataType::Function,
                                        DataType::Int | DataType::Function
                                    )
                                )
                        }
                    };
                    if !matches {
                        return Err(fault(FaultKind::Signature).at(p.id()).with_detail(
                            FaultDetail::ParameterType {
                                operation,
                                callable: symbol,
                                position,
                                expected: p.data_type(),
                                given: GivenType::Parameter(*provided),
                            },
                        ));
                    }
                }
            }
        }
        let address = s.address().ok_or_else(|| fault(FaultKind::CodeBoundary))? as u32;
        let old = if over {
            self.override_index(address).map(|i| &self.overrides[i])
        } else {
            self.external_index(symbol).map(|i| &self.externals[i])
        }
        .map_or(0, |b| 1 + b.parameters.len());
        let total = self.state.binding_count() + self.function_binding_count() - old;
        checked(total, 1, self.state.limits.bindings, LimitKind::Bindings)?;
        checked(
            total + 1,
            parameters.len(),
            self.state.limits.bindings,
            LimitKind::Bindings,
        )?;
        let mut owned = Vec::new();
        reserve(&mut owned, parameters.len())?;
        owned.extend_from_slice(parameters);
        Ok(Binding {
            symbol,
            key,
            parameters: owned,
            result: self.result_type(s)?,
            raw,
            address: s.address().ok_or_else(|| fault(FaultKind::CodeBoundary))? as u32,
        })
    }
    pub fn register_external(
        &mut self,
        symbol: SymbolId,
        key: u32,
        parameters: &[Parameter],
        result: DataType,
    ) -> Result<()> {
        let s = self.script().get(symbol)?;
        if !s.is_external() || s.data_type() != DataType::Function {
            return Err(fault(FaultKind::Signature).at(symbol).types(
                FaultOperation::RegisterExternal,
                ExpectedType::ExternalFunction,
                GivenType::Symbol(s.into()),
            ));
        }
        let r = self.binding(symbol, key, parameters, result, false, false);
        let binding = self.note(r)?;
        let r = self.put_binding(binding, false);
        self.note(r)
    }
    pub fn register_external_named(
        &mut self,
        name: &[u8],
        key: u32,
        parameters: &[Parameter],
        result: DataType,
    ) -> Result<Option<SymbolId>> {
        let Some(s) = self.script().find_name(name) else {
            return Ok(None);
        };
        self.register_external(s.id(), key, parameters, result)?;
        Ok(Some(s.id()))
    }
    pub fn override_function(
        &mut self,
        symbol: SymbolId,
        key: u32,
        parameters: &[Parameter],
        result: DataType,
    ) -> Result<()> {
        let s = self.script().get(symbol)?;
        if s.is_external() {
            return Err(fault(FaultKind::Signature).at(symbol).types(
                FaultOperation::OverrideFunction,
                ExpectedType::ScriptCallable,
                GivenType::Symbol(s.into()),
            ));
        }
        let r = self.binding(symbol, key, parameters, result, false, true);
        let b = self.note(r)?;
        let r = self.put_binding(b, true);
        self.note(r)
    }
    pub fn override_raw(&mut self, symbol: SymbolId, key: u32) -> Result<()> {
        let s = self.script().get(symbol)?;
        if s.is_external() {
            return Err(fault(FaultKind::Signature).at(symbol).types(
                FaultOperation::OverrideFunction,
                ExpectedType::ScriptCallable,
                GivenType::Symbol(s.into()),
            ));
        }
        let r = self.binding(symbol, key, &[], DataType::Void, true, true);
        let b = self.note(r)?;
        let r = self.put_binding(b, true);
        self.note(r)
    }
    fn put_binding(&mut self, b: Binding, over: bool) -> Result<()> {
        let list = if over {
            &mut self.overrides
        } else {
            &mut self.externals
        };
        let found =
            list.binary_search_by_key(&if over { b.address } else { b.symbol.index() }, |v| {
                if over { v.address } else { v.symbol.index() }
            });
        match found {
            Ok(i) => list[i] = b,
            Err(i) => {
                reserve(list, 1)?;
                list.insert(i, b);
            }
        }
        Ok(())
    }

    pub fn default_external_zero(&mut self, observer: Option<u32>) {
        self.default = DefaultBinding::Zero(observer);
    }
    pub fn default_external_raw(&mut self, key: u32) {
        self.default = DefaultBinding::Raw(key);
    }
    pub fn clear_default_external(&mut self) {
        self.default = DefaultBinding::Error;
    }
    /// Lenient repair: arithmetic and push failures receive integer
    /// zero; other operations continue with their partial effects intact. This
    /// does not undo pops, host mutations, or failed writes. Resource failures
    /// remain terminal and cannot be made recoverable by this helper.
    pub fn lenient_recovery<E>(&mut self, error: &VmError<E>) -> Result<Recovery> {
        if error.fault.is_terminal() {
            return Ok(Recovery::Fail);
        }
        let Some(ins) = error.instruction else {
            return Ok(Recovery::Fail);
        };
        use Opcode::*;
        if matches!(
            ins.op,
            Add | Sub
                | Mul
                | Div
                | Mod
                | Or
                | Andb
                | Lt
                | Gt
                | Orr
                | And
                | Lsl
                | Lsr
                | Lte
                | Eq
                | Neq
                | Gte
                | Plus
                | Negate
                | Not
                | Cmpl
                | Pushi
                | Pushv
                | Pushvi
                | Pushvv
        ) {
            self.push(Value::Int(0))?;
        }
        Ok(Recovery::Continue)
    }
}

/// Borrowed only for one callback. The host is never retained in this context.
pub struct HostContext<'v, 's> {
    pub(super) vm: &'v mut Vm<'s>,
}
impl<'s> HostContext<'_, 's> {
    pub fn vm(&mut self) -> &mut Vm<'s> {
        self.vm
    }
    pub fn state(&self) -> &RuntimeState<'s> {
        self.vm.state()
    }
    pub fn script(&self) -> &'s Script {
        self.vm.script()
    }
    pub fn call<H: Host>(
        &mut self,
        host: &mut H,
        symbol: SymbolId,
        args: Vec<Argument>,
    ) -> VmResult<Option<Value>, H::Error> {
        self.vm.call(host, symbol, args)
    }
    pub fn push(&mut self, value: Value) -> Result<()> {
        self.vm.push(value)
    }
    pub fn push_reference(&mut self, symbol: SymbolId, index: usize) -> Result<()> {
        self.vm.push_reference(symbol, index)
    }
    pub fn pop_int<H: Host>(&mut self, host: &mut H) -> VmResult<i32, H::Error> {
        self.vm.pop_int(host)
    }
    pub fn pop_float<H: Host>(&mut self, host: &mut H) -> VmResult<f32, H::Error> {
        self.vm.pop_float(host)
    }
    pub fn pop_string<H: Host>(&mut self, host: &mut H) -> VmResult<Vec<u8>, H::Error> {
        self.vm.pop_string(host)
    }
    pub fn pop_instance<H: Host>(
        &mut self,
        host: &mut H,
    ) -> VmResult<Option<InstanceId>, H::Error> {
        self.vm.pop_instance(host)
    }
    pub fn pop_reference(&mut self) -> Result<StackReference> {
        self.vm.pop_reference()
    }
    pub fn push_stack_reference(&mut self, r: StackReference) -> Result<()> {
        self.vm.push_stack_reference(r)
    }
    pub fn current_instance(&self) -> Option<InstanceId> {
        self.vm.current_instance()
    }
    pub fn string_limit(&self) -> usize {
        self.vm.free_strings()
    }
}
