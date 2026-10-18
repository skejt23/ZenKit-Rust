//! Typed context for checked addon binding and access errors.
use super::{DataType, Error, MemberKind, SymbolId};
use std::{
    any::{TypeId, type_name},
    fmt,
};

/// A borrowed schema name. Formatting reports its size; bytes are explicit.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SchemaName(&'static [u8]);
impl SchemaName {
    pub(super) fn new(bytes: &'static [u8]) -> Self {
        Self(bytes)
    }
    pub fn as_bytes(self) -> &'static [u8] {
        self.0
    }
}
impl fmt::Debug for SchemaName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SchemaName")
            .field("bytes", &self.0.len())
            .finish()
    }
}

/// The actual Rust type at an erased host-storage check. Type names are
/// descriptive only; identity is determined by `TypeId` within this process.
#[derive(Clone, Copy)]
pub struct HostType(fn() -> (TypeId, &'static str));
impl HostType {
    pub(super) fn of<T: 'static>() -> Self {
        Self(|| (TypeId::of::<T>(), type_name::<T>()))
    }
    pub fn id(self) -> TypeId {
        (self.0)().0
    }
    pub fn name(self) -> &'static str {
        (self.0)().1
    }
}
impl PartialEq for HostType {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}
impl Eq for HostType {}
impl fmt::Debug for HostType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostType")
            .field("id", &self.id())
            .field("name_bytes", &self.name().len())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum BindingMemberFailure {
    NotMember,
    Parent {
        given: i32,
    },
    Type {
        expected: MemberKind,
        given: DataType,
    },
    Capacity {
        capacity: usize,
        elements: usize,
    },
}

/// Allocation-free context. Static schema names are borrowed and runtime names
/// remain recoverable from owner-checked symbol IDs in the original Script.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum BindingDiagnostic {
    MissingClass {
        name: SchemaName,
    },
    ClassRole {
        symbol: SymbolId,
        data_type: DataType,
        member: bool,
    },
    MemberLimit {
        requested: usize,
        limit: usize,
    },
    Allocation {
        members: usize,
    },
    ClassAllocation {
        classes: usize,
    },
    MissingMember {
        class: SymbolId,
        name: SchemaName,
        ordinal: usize,
    },
    Member {
        class: SymbolId,
        member: SymbolId,
        ordinal: usize,
        reason: BindingMemberFailure,
    },
    HostType {
        class: SymbolId,
        expected: HostType,
        given: HostType,
    },
    Ordinal {
        class: SymbolId,
        ordinal: usize,
        fields: usize,
    },
    AbsentMember {
        class: SymbolId,
        ordinal: usize,
        name: SchemaName,
    },
    Index {
        class: SymbolId,
        member: SymbolId,
        ordinal: usize,
        index: usize,
        length: usize,
    },
    ValueType {
        class: SymbolId,
        member: SymbolId,
        ordinal: usize,
        expected: MemberKind,
        given: MemberKind,
    },
    Storage {
        class: SymbolId,
        member: SymbolId,
        ordinal: usize,
        index: usize,
        write: bool,
    },
}

/// Detailed additive API error. The original wire error and its source chain
/// remain intact; legacy binding APIs return that error through `into_error`.
#[derive(Debug)]
pub struct BindingError {
    error: Error,
    diagnostic: BindingDiagnostic,
}
pub type BindingResult<T> = std::result::Result<T, BindingError>;
impl BindingError {
    pub(super) fn new(error: Error, diagnostic: BindingDiagnostic) -> Self {
        Self { error, diagnostic }
    }
    pub fn error(&self) -> &Error {
        &self.error
    }
    pub fn diagnostic(&self) -> &BindingDiagnostic {
        &self.diagnostic
    }
    pub fn into_error(self) -> Error {
        self.error
    }
    pub fn into_parts(self) -> (Error, BindingDiagnostic) {
        (self.error, self.diagnostic)
    }
}
impl fmt::Display for BindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({:?})", self.error, self.diagnostic)
    }
}
impl std::error::Error for BindingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}
