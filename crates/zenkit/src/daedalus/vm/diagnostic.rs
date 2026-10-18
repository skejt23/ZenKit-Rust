//! Typed failure context. Library-created dynamic names have a per-error cap.
use super::{DataType, InstanceId, MemberType, Parameter, SymbolId, SymbolRef};
use std::fmt;

/// Why a complete requested name could not be retained. The primary fault stays
/// unchanged; this describes diagnostic incompleteness, not execution failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum NameCaptureFailure {
    Limit { limit: usize },
    Allocation,
}

/// Exact raw requested bytes, or an explicit reason capture was unavailable.
/// Debug/Display never render name bytes. Use [`Self::bytes`] deliberately.
#[non_exhaustive]
pub enum RequestedName {
    Complete(Vec<u8>),
    Unavailable {
        original_len: usize,
        reason: NameCaptureFailure,
    },
}
impl RequestedName {
    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Complete(bytes) => Some(bytes),
            Self::Unavailable { .. } => None,
        }
    }
    pub fn original_len(&self) -> usize {
        match self {
            Self::Complete(bytes) => bytes.len(),
            Self::Unavailable { original_len, .. } => *original_len,
        }
    }
    pub(super) fn capture(bytes: &[u8], limit: usize) -> Self {
        Self::capture_using(bytes, limit, |out, count| {
            out.try_reserve_exact(count).map_err(|_| ())
        })
    }
    fn capture_using(
        bytes: &[u8],
        limit: usize,
        reserve: impl FnOnce(&mut Vec<u8>, usize) -> Result<(), ()>,
    ) -> Self {
        let failure = |reason| Self::Unavailable {
            original_len: bytes.len(),
            reason,
        };
        if bytes.len() > limit {
            return failure(NameCaptureFailure::Limit { limit });
        }
        let mut owned = Vec::new();
        if reserve(&mut owned, bytes.len()).is_err() {
            return failure(NameCaptureFailure::Allocation);
        }
        owned.extend_from_slice(bytes);
        Self::Complete(owned)
    }
}
impl fmt::Debug for RequestedName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Complete(bytes) => f
                .debug_struct("Complete")
                .field("bytes", &bytes.len())
                .finish(),
            Self::Unavailable {
                original_len,
                reason,
            } => f
                .debug_struct("Unavailable")
                .field("original_len", original_len)
                .field("reason", reason)
                .finish(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum FaultOperation {
    ValueAccess,
    GlobalRead,
    GlobalWrite,
    Grow,
    RecursiveLocals,
    RegisterClass,
    RegisterExternal,
    OverrideFunction,
    Call,
    InstanceBinding,
    MemberRead,
    MemberWrite,
    MemberAccess,
    Reference,
    ReferenceRead,
    ReferenceWrite,
    StackPop,
    Return,
}

/// Constraints include the VM's accepted bit-pattern/storage alternatives.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExpectedType {
    Exact(DataType),
    /// Declared storage type; INT/FUNCTION share integer cells.
    Storage(DataType),
    NonMemberStorage,
    IntOrFunction,
    FloatOrIntegerBits,
    ScalarMember,
    GrowableGlobal,
    TopLevelClass,
    TopLevelFunction,
    TopLevelInstance,
    Callable,
    ExternalFunction,
    ScriptCallable,
    Reference,
    ReturnValue,
    SymbolReference,
    Member(MemberType),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SymbolFacts {
    pub data_type: DataType,
    pub member: bool,
    pub constant: bool,
    pub external: bool,
}
impl From<SymbolRef<'_>> for SymbolFacts {
    fn from(symbol: SymbolRef<'_>) -> Self {
        Self {
            data_type: symbol.data_type(),
            member: symbol.is_member(),
            constant: symbol.is_const(),
            external: symbol.is_external(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum GivenType {
    Type(DataType),
    Value(DataType),
    Symbol(SymbolFacts),
    Parameter(Parameter),
    Reference(DataType),
    Void,
    StackHandled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ReturnExpectation {
    Type(DataType),
    /// Raw callbacks manipulate the stack themselves and may return either tag.
    StackHandledOrVoid,
}

/// The external argument conversion through which an error propagated. This
/// does not replace the cause's symbol, array index, typed detail or host source.
/// Nested conversion context wins over an enclosing conversion boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArgumentContext {
    /// Owner of the signature governing conversion, including address overrides.
    pub callable: SymbolId,
    /// Zero-based position in that signature.
    pub position: usize,
    pub parameter: Option<SymbolId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CountRole {
    Arguments,
    ReturnStack,
    MemberCapacity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RegistrationReason {
    ClassRole {
        given: SymbolFacts,
    },
    NotMember {
        given: SymbolFacts,
    },
    Parent {
        given: i32,
    },
    Type {
        expected: MemberType,
        given: DataType,
    },
    Capacity {
        capacity: usize,
        elements: usize,
    },
    Duplicate {
        member: SymbolId,
    },
    ChangedLayout,
}

/// Additive context. Existing Fault.kind/symbol/index and wire provenance remain
/// authoritative. A parameter position never replaces the array-element index.
#[derive(Debug)]
#[non_exhaustive]
pub enum FaultDetail {
    Addon(super::super::addon::BindingDiagnostic),
    MissingName {
        operation: FaultOperation,
        name: RequestedName,
    },
    TypeMismatch {
        operation: FaultOperation,
        expected: ExpectedType,
        given: GivenType,
    },
    ParameterType {
        operation: FaultOperation,
        callable: SymbolId,
        /// Zero-based position in the containing callable's signature.
        position: usize,
        expected: DataType,
        given: GivenType,
    },
    Counts {
        operation: FaultOperation,
        role: CountRole,
        expected: usize,
        given: usize,
    },
    Registration {
        class: SymbolId,
        reason: RegistrationReason,
    },
    ClassMismatch {
        expected: SymbolId,
        given: Option<SymbolId>,
        instance: Option<InstanceId>,
    },
    Index {
        operation: FaultOperation,
        length: usize,
    },
    Access {
        operation: FaultOperation,
    },
    Address {
        requested: u32,
    },
    Return {
        callable: Option<SymbolId>,
        expected: ReturnExpectation,
        given: GivenType,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_capture_checks_cap_before_allocation_and_reports_capture_failure() {
        let limited = RequestedName::capture_using(b"raw\xff", 3, |_, _| {
            panic!("limit must precede reserve")
        });
        assert!(matches!(
            limited,
            RequestedName::Unavailable {
                original_len: 4,
                reason: NameCaptureFailure::Limit { limit: 3 }
            }
        ));
        let failed = RequestedName::capture_using(b"raw\xff", 4, |_, _| Err(()));
        assert!(matches!(
            failed,
            RequestedName::Unavailable {
                original_len: 4,
                reason: NameCaptureFailure::Allocation
            }
        ));
        assert!(failed.bytes().is_none());
        assert_eq!(failed.original_len(), 4);
        let complete = RequestedName::capture(b"raw\xff", 4);
        assert_eq!(complete.bytes(), Some(b"raw\xff".as_slice()));
        assert_eq!(complete.original_len(), 4);
        assert_eq!(format!("{complete:?}"), "Complete { bytes: 4 }");
    }
}
