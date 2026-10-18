//! Bounded Daedalus execution with an externally owned, reentrant host.
//!
//! The VM borrows immutable DAT data and owns runtime state. InstanceId, ClassId
//! and MemberKey belong to that runtime. Callbacks receive owned arguments and a
//! context borrowing only the VM; nested calls reborrow the host.
//!
//! [`Vm::from_script`] validates instruction boundaries, symbol roles and branch
//! targets before constructing mutable cells. Indexed array bounds are checked
//! during execution to allow [`Vm::grow`], unlike [`Script::validate_code`].
//! The VM executes 41 instructions; `MOVSS` is rejected at construction with
//! [`FaultKind::Wire`] containing [`crate::ErrorKind::Unsupported`].
//!
//! Script calls use iterative frames. Host reentry uses the host thread's Rust
//! stack and is separately limited by [`Limits::host_depth`]; callers remain
//! responsible for a suitable thread stack and their own callback stack usage.
//! Fuel and resource failures are terminal for an invocation, even if a host
//! swallows a nested error. A subsequent independent invocation starts afresh.
//! Host code itself is not sandboxed or metered: arbitrary host allocations,
//! retained callback values and I/O are outside the VM's storage limits.
//!
//! Integer arithmetic wraps for add/subtract/multiply/negate; division by zero,
//! signed division overflow and shifts outside `0..32` are explicit faults.
//! Binary arithmetic pops its left operand first.
//! Float stack extraction preserves integer bit patterns, rather than converting
//! them numerically. Strings are raw bytes. Nonconstant FUNCTION storage starts
//! at zero with its declared extent.
//!
//! References capture an instance context and remain late-bound to runtime cells.
//! Recursion-local restoration therefore precedes resolving a returned reference.
//! Temporary string references transfer owned bytes through [`StackReference`].
//! Host-backed objects belong to Host and survive VM destruction; opaque objects
//! belong to [`RuntimeState`] and survive [`Vm::into_state`]. Instance identities
//! are stable and append-only. Rebinding keeps old symbol aliases and changes only
//! the instance's reverse lookup. Class layouts cannot be replaced incompatibly.
//!
//! Errors unwind frames, temporaries and saved contexts, including initialization's
//! SELF binding. They do not roll back global values, host effects or new instance
//! bindings. Recovery observes partial instruction effects.
//! [`Vm::lenient_recovery`] supplies integer zero after arithmetic or push
//! failures and continues other operations with their partial effects intact.
//! Resource failures remain terminal. Panics from Host propagate to the caller.
//!
//! Errors retain typed details, PC/instruction, an owned host source and a bounded
//! trace. Fixed facts do not allocate. Missing names are copied fallibly under
//! [`Limits::diagnostic_name_bytes`]; capture failure leaves the primary fault intact.
#![allow(clippy::result_large_err)]
// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT

mod diagnostic;
mod execute;
pub use diagnostic::{
    ArgumentContext, CountRole, ExpectedType, FaultDetail, FaultOperation, GivenType,
    NameCaptureFailure, RegistrationReason, RequestedName, ReturnExpectation, SymbolFacts,
};
mod machine;
mod state;

use super::{
    DataType, DecodeLimits, Executable, InitialValue, Instruction, Opcode, Operand, Script,
    SymbolId, SymbolRef,
};
pub use machine::{HostContext, Vm};
pub use state::RuntimeState;
use std::{
    convert::Infallible,
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};

pub type Result<T> = std::result::Result<T, Fault>;
pub type VmResult<T, E> = std::result::Result<T, VmError<E>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LimitKind {
    Cells,
    Strings,
    Instances,
    Bindings,
    Stack,
    Frames,
    HostDepth,
    Fuel,
    Snapshots,
    Trace,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum FaultKind {
    InvalidHandle,
    WrongType,
    Index,
    NotFound,
    UnboundMember,
    WrongClass,
    NoContext,
    Constant,
    Signature,
    StackUnderflow,
    CodeBoundary,
    Unsupported,
    DivisionByZero,
    DivisionOverflow,
    ShiftCount,
    Cycle,
    MissingExternal,
    InvalidReturn,
    Host,
    Limit(LimitKind),
    Allocation,
    Wire(crate::ErrorKind),
}
#[derive(Debug)]
pub struct Fault {
    pub kind: FaultKind,
    pub symbol: Option<SymbolId>,
    pub index: Option<usize>,
    wire: Option<WireFault>,
    detail: Option<FaultDetail>,
}
impl Fault {
    pub fn new(kind: FaultKind) -> Self {
        Self {
            kind,
            symbol: None,
            index: None,
            wire: None,
            detail: None,
        }
    }
    pub fn at(mut self, symbol: SymbolId) -> Self {
        self.symbol = Some(symbol);
        self
    }
    pub fn element(mut self, index: usize) -> Self {
        self.index = Some(index);
        self
    }
    pub fn detail(&self) -> Option<&FaultDetail> {
        self.detail.as_ref()
    }
    /// Attach application-owned typed context without changing the primary fault.
    pub fn with_detail(mut self, detail: FaultDetail) -> Self {
        self.detail = Some(detail);
        self
    }
    fn types(self, operation: FaultOperation, expected: ExpectedType, given: GivenType) -> Self {
        self.with_detail(FaultDetail::TypeMismatch {
            operation,
            expected,
            given,
        })
    }
    fn missing_name(operation: FaultOperation, name: &[u8], limit: usize) -> Self {
        Self::new(FaultKind::NotFound).with_detail(FaultDetail::MissingName {
            operation,
            name: RequestedName::capture(name, limit),
        })
    }
    fn extracted(mut self, symbol: Option<SymbolId>) -> Self {
        if self.symbol.is_none() {
            self.symbol = symbol;
        }
        if let Some(FaultDetail::TypeMismatch { operation, .. }) = &mut self.detail
            && *operation == FaultOperation::ValueAccess
        {
            *operation = FaultOperation::StackPop;
        }
        self
    }
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.kind,
            FaultKind::Limit(_)
                | FaultKind::Allocation
                | FaultKind::Wire(crate::ErrorKind::LimitExceeded)
        )
    }
}
/// Structured provenance from an in-memory DAT operation. Filesystem paths and
/// IO-source chains are not runtime wire diagnostics; use VmError::host for host IO.
#[derive(Clone, Copy, Debug)]
pub struct WireFault {
    pub kind: crate::ErrorKind,
    pub offset: Option<usize>,
    pub context: &'static str,
}
impl fmt::Display for WireFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.context)?;
        if let Some(offset) = self.offset {
            write!(f, " at byte {offset}")?;
        }
        Ok(())
    }
}
impl std::error::Error for WireFault {}
impl Fault {
    pub fn wire_source(&self) -> Option<&WireFault> {
        self.wire.as_ref()
    }
}
impl From<crate::Error> for Fault {
    fn from(e: crate::Error) -> Self {
        let kind = if e.kind == crate::ErrorKind::InvalidHandle {
            FaultKind::InvalidHandle
        } else {
            FaultKind::Wire(e.kind)
        };
        Self {
            kind,
            symbol: None,
            index: None,
            wire: Some(WireFault {
                kind: e.kind,
                offset: e.offset,
                context: e.context,
            }),
            detail: None,
        }
    }
}
impl From<super::addon::BindingError> for Fault {
    fn from(error: super::addon::BindingError) -> Self {
        let (wire, diagnostic) = error.into_parts();
        Self::from(wire).with_detail(FaultDetail::Addon(diagnostic))
    }
}
impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Daedalus runtime: {:?}", self.kind)?;
        if let Some(detail) = &self.detail {
            write!(f, " ({detail:?})")?;
        }
        Ok(())
    }
}
impl std::error::Error for Fault {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.wire.as_ref().map(|x| x as _)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FrameTrace {
    pub symbol: SymbolId,
    pub return_pc: u32,
    pub stack_base: usize,
    pub context: Option<InstanceId>,
}
#[derive(Debug)]
pub struct VmError<E> {
    pub fault: Fault,
    /// Bytecode offset, never an input-file offset.
    pub pc: Option<u32>,
    pub instruction: Option<Instruction>,
    pub frames: Vec<FrameTrace>,
    pub trace_truncated: bool,
    host: Option<E>,
    argument: Option<ArgumentContext>,
}
impl<E> VmError<E> {
    pub fn host(source: E) -> Self {
        Self {
            fault: Fault::new(FaultKind::Host),
            pc: None,
            instruction: None,
            frames: Vec::new(),
            trace_truncated: false,
            host: Some(source),
            argument: None,
        }
    }
    pub fn host_source(&self) -> Option<&E> {
        self.host.as_ref()
    }
    pub fn argument_context(&self) -> Option<&ArgumentContext> {
        self.argument.as_ref()
    }
}
impl<E> From<Fault> for VmError<E> {
    fn from(fault: Fault) -> Self {
        Self {
            fault,
            pc: None,
            instruction: None,
            frames: Vec::new(),
            trace_truncated: false,
            host: None,
            argument: None,
        }
    }
}
impl<E: fmt::Display> fmt::Display for VmError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.fault)?;
        if let Some(pc) = self.pc {
            write!(f, " at PC {pc}")?;
        }
        if let Some(e) = &self.host {
            write!(f, ": {e}")?;
        }
        Ok(())
    }
}
impl<E: std::error::Error + 'static> std::error::Error for VmError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.host.as_ref().map(|x| x as _).or(Some(&self.fault))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Aggregate runtime value cells, including global descriptors and instances.
    pub cells: usize,
    /// Aggregate retained and temporarily copied string bytes.
    /// Includes globals, opaque members, stack strings, local snapshots and
    /// callback argument/setter packets while callbacks are active.
    pub string_bytes: usize,
    /// Runtime-owned instance descriptors, including host and transient handles.
    pub instances: usize,
    /// Aggregate classes, registered members, callback bindings and parameter
    /// descriptors. Batch registration failures retain prior logical bindings.
    pub bindings: usize,
    /// Total stack entries across all active script and host invocations.
    pub stack: usize,
    /// Simultaneously active script and typed callback frames.
    pub frames: usize,
    /// Simultaneously active Host hooks, including member, trap and error hooks.
    pub host_depth: usize,
    /// Shared by a top-level call and all reentrant calls it makes.
    pub fuel: u64,
    /// Saved local value cells plus one descriptor per saved local array.
    pub snapshot_cells: usize,
    /// Maximum error-trace frames; truncation is reported without masking faults.
    pub trace_frames: usize,
    /// Per library-created missing-name fault, outside retained VM strings.
    /// Names are copied fallibly and completely or marked explicitly unavailable.
    pub diagnostic_name_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            cells: 4_000_000,
            string_bytes: 64 * 1024 * 1024,
            instances: 100_000,
            bindings: 1_000_000,
            stack: 2048,
            frames: 4096,
            host_depth: 32,
            fuel: 10_000_000,
            snapshot_cells: 1_000_000,
            trace_frames: 64,
            diagnostic_name_bytes: 1024 * 1024,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Policy {
    /// Null member reads yield zero/empty; writes are ignored. Otherwise they fail.
    pub allow_null_members: bool,
    /// Permit script stores to constants. Explicit host edits always permit them;
    /// MOVVI instance assignments are permitted independently of this setting.
    pub ignore_const: bool,
    /// Return zero for numeric empty-stack pops; other pops always fail.
    pub numeric_underflow_zero: bool,
    /// Repair missing/excess declared return values.
    pub repair_returns: bool,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            allow_null_members: false,
            ignore_const: false,
            numeric_underflow_zero: true,
            repair_returns: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct InstanceId {
    owner: u64,
    index: usize,
}
impl InstanceId {
    pub fn index(self) -> usize {
        self.index
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ClassId {
    owner: u64,
    index: usize,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct HostKey(pub u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemberType {
    Int,
    Float,
    String,
    /// Retained opaque-layout metadata without a scalar value accessor.
    /// Only VOID/CLASS/PROTOTYPE/INSTANCE are accepted here. Instance-member
    /// access reports WrongType because no scalar storage exists for these types.
    NonScalar(DataType),
}
impl MemberType {
    pub fn accepts(self, t: DataType) -> bool {
        matches!(
            (self, t),
            (Self::Int, DataType::Int | DataType::Function)
                | (Self::Float, DataType::Float)
                | (Self::String, DataType::String)
        ) || matches!(self,Self::NonScalar(kind) if kind==t && matches!(kind,DataType::Void|DataType::Class|DataType::Prototype|DataType::Instance))
    }
}
#[derive(Clone, Copy)]
pub struct MemberSpec<'a> {
    pub name: &'a [u8],
    pub kind: MemberType,
    pub capacity: usize,
    pub key: u32,
}
impl fmt::Debug for MemberSpec<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemberSpec")
            .field("name_bytes", &self.name.len())
            .field("kind", &self.kind)
            .field("capacity", &self.capacity)
            .field("key", &self.key)
            .finish()
    }
}
#[derive(Clone, Copy)]
pub struct ClassRegistration<'a> {
    pub class: SymbolId,
    pub members: &'a [MemberSpec<'a>],
}
impl fmt::Debug for ClassRegistration<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClassRegistration")
            .field("class", &self.class)
            .field("members", &self.members.len())
            .finish()
    }
}
#[derive(Clone, Copy, Debug)]
pub struct MemberKey {
    pub symbol: SymbolId,
    pub key: u32,
    pub kind: MemberType,
    pub count: usize,
    pub class: ClassId,
    offset: usize,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstanceBackend {
    Opaque,
    Host(HostKey),
    Transient(HostKey),
}

/// Owned callback values. Bytes are never implicitly UTF-8 decoded.
#[derive(PartialEq)]
pub enum Value {
    Int(i32),
    Float(f32),
    Bytes(Vec<u8>),
    Instance(Option<InstanceId>),
    Function(i32),
}
impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(x) => f.debug_tuple("Int").field(x).finish(),
            Self::Float(x) => f.debug_tuple("FloatBits").field(&x.to_bits()).finish(),
            Self::Bytes(x) => f.debug_tuple("Bytes").field(&x.len()).finish(),
            Self::Instance(x) => f.debug_tuple("Instance").field(x).finish(),
            Self::Function(x) => f.debug_tuple("Function").field(x).finish(),
        }
    }
}
impl Value {
    pub fn data_type(&self) -> DataType {
        match self {
            Self::Int(_) => DataType::Int,
            Self::Float(_) => DataType::Float,
            Self::Bytes(_) => DataType::String,
            Self::Instance(_) => DataType::Instance,
            Self::Function(_) => DataType::Function,
        }
    }
    pub fn bytes(bytes: &[u8], limit: usize) -> Result<Self> {
        Ok(Self::Bytes(copy_bytes(bytes, limit)?))
    }
    pub fn as_int(&self) -> Result<i32> {
        match self {
            Self::Int(n) | Self::Function(n) => Ok(*n),
            _ => Err(fault(FaultKind::WrongType).types(
                FaultOperation::ValueAccess,
                ExpectedType::IntOrFunction,
                GivenType::Value(self.data_type()),
            )),
        }
    }
    pub fn as_float(&self) -> Result<f32> {
        match self {
            Self::Float(n) => Ok(*n),
            Self::Int(n) => Ok(f32::from_bits(*n as u32)),
            _ => Err(fault(FaultKind::WrongType).types(
                FaultOperation::ValueAccess,
                ExpectedType::FloatOrIntegerBits,
                GivenType::Value(self.data_type()),
            )),
        }
    }
    pub fn as_bytes(&self) -> Result<&[u8]> {
        match self {
            Self::Bytes(v) => Ok(v),
            _ => Err(fault(FaultKind::WrongType).types(
                FaultOperation::ValueAccess,
                ExpectedType::Exact(DataType::String),
                GivenType::Value(self.data_type()),
            )),
        }
    }
    pub fn as_instance(&self) -> Result<Option<InstanceId>> {
        match self {
            Self::Instance(v) => Ok(*v),
            _ => Err(fault(FaultKind::WrongType).types(
                FaultOperation::ValueAccess,
                ExpectedType::Exact(DataType::Instance),
                GivenType::Value(self.data_type()),
            )),
        }
    }
    fn bytes_len(&self) -> usize {
        if let Self::Bytes(v) = self {
            v.len()
        } else {
            0
        }
    }
    fn try_copy(&self, limit: usize) -> Result<Self> {
        Ok(match self {
            Self::Int(v) => Self::Int(*v),
            Self::Float(v) => Self::Float(*v),
            Self::Bytes(v) => Self::Bytes(copy_bytes(v, limit)?),
            Self::Instance(v) => Self::Instance(*v),
            Self::Function(v) => Self::Function(*v),
        })
    }
}
fn default_value(t: DataType) -> Result<Value> {
    Ok(match t {
        DataType::Int => Value::Int(0),
        DataType::Function => Value::Function(0),
        DataType::Float => Value::Float(0.0),
        DataType::String => Value::Bytes(Vec::new()),
        DataType::Instance => Value::Instance(None),
        _ => {
            return Err(fault(FaultKind::WrongType).types(
                FaultOperation::Return,
                ExpectedType::ReturnValue,
                GivenType::Type(t),
            ));
        }
    })
}
fn value_matches(v: &Value, t: DataType) -> bool {
    matches!(
        (v, t),
        (
            Value::Int(_) | Value::Function(_),
            DataType::Int | DataType::Function
        ) | (Value::Float(_), DataType::Float)
            | (Value::Bytes(_), DataType::String)
            | (Value::Instance(_), DataType::Instance)
    )
}
fn fault(kind: FaultKind) -> Fault {
    Fault::new(kind)
}
fn reserve<T>(v: &mut Vec<T>, n: usize) -> Result<()> {
    v.try_reserve(n).map_err(|_| fault(FaultKind::Allocation))
}
fn checked(n: usize, add: usize, limit: usize, kind: LimitKind) -> Result<usize> {
    n.checked_add(add)
        .filter(|n| *n <= limit)
        .ok_or_else(|| fault(FaultKind::Limit(kind)))
}
fn copy_bytes(v: &[u8], limit: usize) -> Result<Vec<u8>> {
    checked(0, v.len(), limit, LimitKind::Strings)?;
    let mut out = Vec::new();
    reserve(&mut out, v.len())?;
    out.extend_from_slice(v);
    Ok(out)
}

/// Script-relative symbol reference with context captured when it was pushed.
/// A global reference with no context may resolve in another runtime borrowing
/// the same Script. A captured InstanceId additionally restricts it to that
/// instance's runtime. Foreign Script identities always fail.
#[derive(Clone, Copy, Debug)]
pub struct Reference {
    symbol: SymbolId,
    index: usize,
    context: Option<InstanceId>,
}
impl Reference {
    pub fn symbol(self) -> SymbolId {
        self.symbol
    }
    pub fn index(self) -> usize {
        self.index
    }
    pub fn context(self) -> Option<InstanceId> {
        self.context
    }
}
/// A popped reference transfers its temporary bytes to the caller. Symbol
/// references stay late-bound; temporary references never fabricate a DAT ID.
pub enum StackReference {
    Symbol(Reference),
    Temporary(Vec<u8>),
}
impl fmt::Debug for StackReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Symbol(r) => f.debug_tuple("Symbol").field(r).finish(),
            Self::Temporary(v) => f.debug_tuple("Temporary").field(&v.len()).finish(),
        }
    }
}
impl StackReference {
    pub fn symbol(self) -> Result<Reference> {
        match self {
            Self::Symbol(r) => Ok(r),
            Self::Temporary(_) => Err(fault(FaultKind::WrongType).types(
                FaultOperation::Reference,
                ExpectedType::SymbolReference,
                GivenType::Value(DataType::String),
            )),
        }
    }
}
/// Read-only stack inspection does not resolve references or call Host. A view
/// borrows Vm, so Rust prevents retaining it across a reentrant callback.
#[derive(Debug)]
pub enum StackEntry<'a> {
    Value(&'a Value),
    Reference(Reference),
}
#[derive(Debug)]
pub enum Argument {
    Value(Value),
    Reference(Reference),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Parameter {
    Value(DataType),
    InstanceReference,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallKind {
    External,
    Override,
    RawOverride,
    DefaultExternal,
    RawDefault,
}
#[derive(Debug)]
pub struct HostCall {
    pub binding: u32,
    pub symbol: SymbolId,
    pub kind: CallKind,
    pub arguments: Vec<Argument>,
}
#[derive(Debug)]
pub enum HostReturn {
    Void,
    Value(Value),
    StackHandled,
}
#[derive(Clone, Copy, Debug)]
pub enum Recovery {
    Fail,
    Continue,
    Return,
    Jump(u32),
}
#[derive(Debug)]
pub enum Trap {
    Default,
    Value(Value),
    Reference(Reference),
    Handled,
}

/// Host ownership stays outside the VM. Every callback may explicitly reenter.
/// Return an owned string before invoking another callback; no member borrow may
/// cross `cx.call(self, ...)`. Limits/fuel errors are terminal and bypass recovery.
pub trait Host: Sized {
    type Error: std::error::Error + 'static;
    fn call(
        &mut self,
        _cx: &mut HostContext<'_, '_>,
        _call: HostCall,
    ) -> VmResult<HostReturn, Self::Error> {
        Err(fault(FaultKind::MissingExternal).into())
    }
    fn get_member(
        &mut self,
        _cx: &mut HostContext<'_, '_>,
        _key: HostKey,
        _member: MemberKey,
        _index: usize,
    ) -> VmResult<Value, Self::Error> {
        Err(fault(FaultKind::UnboundMember).into())
    }
    fn set_member(
        &mut self,
        _cx: &mut HostContext<'_, '_>,
        _key: HostKey,
        _member: MemberKey,
        _index: usize,
        _value: Value,
    ) -> VmResult<(), Self::Error> {
        Err(fault(FaultKind::UnboundMember).into())
    }
    fn get_transient(
        &mut self,
        _cx: &mut HostContext<'_, '_>,
        _key: HostKey,
        _symbol: SymbolId,
        _index: usize,
    ) -> VmResult<Value, Self::Error> {
        Err(fault(FaultKind::UnboundMember).into())
    }
    fn set_transient(
        &mut self,
        _cx: &mut HostContext<'_, '_>,
        _key: HostKey,
        _symbol: SymbolId,
        _index: usize,
        _value: Value,
    ) -> VmResult<(), Self::Error> {
        Err(fault(FaultKind::UnboundMember).into())
    }
    fn access_trap(
        &mut self,
        _cx: &mut HostContext<'_, '_>,
        _reference: Reference,
    ) -> VmResult<Trap, Self::Error> {
        Ok(Trap::Default)
    }
    fn exception(
        &mut self,
        _cx: &mut HostContext<'_, '_>,
        _error: &VmError<Self::Error>,
    ) -> VmResult<Recovery, Self::Error> {
        Ok(Recovery::Fail)
    }
}
#[derive(Debug, Default)]
pub struct NoHost;
impl Host for NoHost {
    type Error = Infallible;
}
