//! Typed host-owned game classes and checked DAT member bindings.
//!
//! Strings remain bytes and enum wrappers retain unknown words. Fixed arrays
//! have the capacities defined by each class schema; a DAT may declare
//! fewer elements. Optional fields are selected by symbol presence, not game
//! version. These values do not contain pointers into a VM or a Script.
//!
//! Detailed errors retain the wire error and typed context without allocation.
#![allow(clippy::result_large_err)]
// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2022-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT

use super::{DataType, Script, SymbolId};
use crate::{Error, ErrorKind, Result};
use std::fmt;

mod choices;
mod diagnostic;
mod runtime;
pub use choices::{ChoiceLimits, InfoChoice};
pub use diagnostic::{
    BindingDiagnostic, BindingError, BindingMemberFailure, BindingResult, HostType, SchemaName,
};
pub use enums::*;
pub use runtime::{RegisteredAddon, register_all_present};

fn error(kind: ErrorKind) -> Error {
    Error::operation(kind, "Daedalus addon member")
}

/// Script types accepted by a host field. Integer fields also accept FUNCTION.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemberKind {
    Int,
    Float,
    String,
}
impl MemberKind {
    pub fn accepts(self, ty: DataType) -> bool {
        matches!(
            (self, ty),
            (Self::Int, DataType::Int | DataType::Function)
                | (Self::Float, DataType::Float)
                | (Self::String, DataType::String)
        )
    }
}

/// A value borrowed from host storage, independent of VM temporary strings.
#[derive(Clone, Copy, PartialEq)]
pub enum MemberValue<'a> {
    Int(i32),
    Float(f32),
    Bytes(&'a [u8]),
}
impl fmt::Debug for MemberValue<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(value) => f.debug_tuple("Int").field(value).finish(),
            Self::Float(value) => f.debug_tuple("Float").field(value).finish(),
            Self::Bytes(bytes) => f.debug_struct("Bytes").field("len", &bytes.len()).finish(),
        }
    }
}
impl MemberValue<'_> {
    pub fn kind(self) -> MemberKind {
        match self {
            Self::Int(_) => MemberKind::Int,
            Self::Float(_) => MemberKind::Float,
            Self::Bytes(_) => MemberKind::String,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Member {
    pub name: &'static [u8],
    pub kind: MemberKind,
    pub capacity: usize,
    /// This member is required only when this controlling symbol exists.
    pub presence_guard: Option<&'static [u8]>,
}

#[derive(Debug)]
pub struct ClassSchema {
    pub name: &'static [u8],
    pub members: &'static [Member],
}

/// Safe field access for a typed, host-owned script class.
///
/// Member ordinals correspond to `SCHEMA.members`. Access checks the host array
/// capacity. A [`ClassBinding`] additionally enforces the DAT's declared count.
/// A string write checks `max_string_bytes` and reserves before changing the
/// target. Hosts own their overall memory policy; this is a per-write limit.
pub trait AddonClass: Default + 'static {
    const SCHEMA: &'static ClassSchema;
    fn read_member(&self, member: usize, index: usize) -> Result<MemberValue<'_>>;
    fn write_member(
        &mut self,
        member: usize,
        index: usize,
        value: MemberValue<'_>,
        max_string_bytes: usize,
    ) -> Result<()>;
}

/// An individual validated field, including its immutable DAT extent.
#[derive(Clone, Copy, Debug)]
pub struct BoundMember {
    symbol: SymbolId,
    ordinal: usize,
    elements: usize,
}
impl BoundMember {
    pub fn symbol(self) -> SymbolId {
        self.symbol
    }
    pub fn ordinal(self) -> usize {
        self.ordinal
    }
    pub fn elements(self) -> usize {
        self.elements
    }
}

/// A complete, atomically validated class mapping. No Script state is mutated.
pub struct ClassBinding {
    class: SymbolId,
    schema: &'static ClassSchema,
    host_type: HostType,
    members: Vec<Option<BoundMember>>,
}
impl fmt::Debug for ClassBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClassBinding")
            .field("class", &self.class)
            .field("members", &self.members.len())
            .finish()
    }
}
impl ClassBinding {
    /// Absence is distinct from an invalid present class or missing required field.
    pub fn bind_present<T: AddonClass>(
        script: &Script,
        max_members: usize,
    ) -> Result<Option<Self>> {
        Self::bind_present_detailed::<T>(script, max_members).map_err(BindingError::into_error)
    }
    pub fn bind_present_detailed<T: AddonClass>(
        script: &Script,
        max_members: usize,
    ) -> BindingResult<Option<Self>> {
        if script.find_name(T::SCHEMA.name).is_none() {
            return Ok(None);
        }
        Self::bind_detailed::<T>(script, max_members).map(Some)
    }
    pub fn bind<T: AddonClass>(script: &Script, max_members: usize) -> Result<Self> {
        Self::bind_detailed::<T>(script, max_members).map_err(BindingError::into_error)
    }
    /// Validate atomically with typed context while preserving legacy error kinds.
    pub fn bind_detailed<T: AddonClass>(
        script: &Script,
        max_members: usize,
    ) -> BindingResult<Self> {
        let schema = T::SCHEMA;
        let class = script.find_name(schema.name).ok_or_else(|| {
            BindingError::new(
                error(ErrorKind::NotFound),
                BindingDiagnostic::MissingClass {
                    name: SchemaName::new(schema.name),
                },
            )
        })?;
        if class.is_member() || class.data_type() != DataType::Class {
            return Err(BindingError::new(
                error(ErrorKind::WrongObjectType),
                BindingDiagnostic::ClassRole {
                    symbol: class.id(),
                    data_type: class.data_type(),
                    member: class.is_member(),
                },
            ));
        }
        if schema.members.len() > max_members {
            return Err(BindingError::new(
                error(ErrorKind::LimitExceeded),
                BindingDiagnostic::MemberLimit {
                    requested: schema.members.len(),
                    limit: max_members,
                },
            ));
        }
        let mut members = Vec::new();
        members
            .try_reserve_exact(schema.members.len())
            .map_err(|_| {
                BindingError::new(
                    error(ErrorKind::LimitExceeded),
                    BindingDiagnostic::Allocation {
                        members: schema.members.len(),
                    },
                )
            })?;
        for (ordinal, field) in schema.members.iter().enumerate() {
            if field
                .presence_guard
                .is_some_and(|g| script.find_name(g).is_none())
            {
                members.push(None);
                continue;
            }
            let symbol = script.find_name(field.name).ok_or_else(|| {
                BindingError::new(
                    error(ErrorKind::NotFound),
                    BindingDiagnostic::MissingMember {
                        class: class.id(),
                        name: SchemaName::new(field.name),
                        ordinal,
                    },
                )
            })?;
            let fail = |kind, reason| {
                BindingError::new(
                    error(kind),
                    BindingDiagnostic::Member {
                        class: class.id(),
                        member: symbol.id(),
                        ordinal,
                        reason,
                    },
                )
            };
            if !symbol.is_member() {
                return Err(fail(
                    ErrorKind::WrongObjectType,
                    BindingMemberFailure::NotMember,
                ));
            }
            if symbol.parent() != class.index() as i32 {
                return Err(fail(
                    ErrorKind::WrongObjectType,
                    BindingMemberFailure::Parent {
                        given: symbol.parent(),
                    },
                ));
            }
            if !field.kind.accepts(symbol.data_type()) {
                return Err(fail(
                    ErrorKind::WrongObjectType,
                    BindingMemberFailure::Type {
                        expected: field.kind,
                        given: symbol.data_type(),
                    },
                ));
            }
            if usize::from(symbol.count()) > field.capacity {
                return Err(fail(
                    ErrorKind::InvalidData,
                    BindingMemberFailure::Capacity {
                        capacity: field.capacity,
                        elements: usize::from(symbol.count()),
                    },
                ));
            }
            members.push(Some(BoundMember {
                symbol: symbol.id(),
                ordinal,
                elements: usize::from(symbol.count()),
            }));
        }
        Ok(Self {
            class: class.id(),
            schema,
            host_type: HostType::of::<T>(),
            members,
        })
    }
    pub fn class(&self) -> SymbolId {
        self.class
    }
    pub fn schema(&self) -> &'static ClassSchema {
        self.schema
    }
    /// One entry per host field; None means its optional presence guard was absent.
    pub fn members(&self) -> &[Option<BoundMember>] {
        &self.members
    }
    pub fn is_for<T: AddonClass>(&self) -> bool {
        self.host_type.id() == HostType::of::<T>().id()
    }
    fn checked<T: AddonClass>(&self, ordinal: usize, index: usize) -> BindingResult<BoundMember> {
        if !self.is_for::<T>() {
            return Err(BindingError::new(
                error(ErrorKind::WrongObjectType),
                BindingDiagnostic::HostType {
                    class: self.class,
                    expected: self.host_type,
                    given: HostType::of::<T>(),
                },
            ));
        }
        let field = self
            .members
            .get(ordinal)
            .ok_or_else(|| {
                BindingError::new(
                    error(ErrorKind::InvalidData),
                    BindingDiagnostic::Ordinal {
                        class: self.class,
                        ordinal,
                        fields: self.members.len(),
                    },
                )
            })?
            .ok_or_else(|| {
                BindingError::new(
                    error(ErrorKind::NotFound),
                    BindingDiagnostic::AbsentMember {
                        class: self.class,
                        ordinal,
                        name: SchemaName::new(self.schema.members[ordinal].name),
                    },
                )
            })?;
        if index >= field.elements {
            return Err(BindingError::new(
                error(ErrorKind::InvalidData),
                BindingDiagnostic::Index {
                    class: self.class,
                    member: field.symbol,
                    ordinal,
                    index,
                    length: field.elements,
                },
            ));
        }
        Ok(field)
    }
    pub fn read<'a, T: AddonClass>(
        &self,
        instance: &'a T,
        ordinal: usize,
        index: usize,
    ) -> Result<MemberValue<'a>> {
        self.read_detailed(instance, ordinal, index)
            .map_err(BindingError::into_error)
    }
    pub fn read_detailed<'a, T: AddonClass>(
        &self,
        instance: &'a T,
        ordinal: usize,
        index: usize,
    ) -> BindingResult<MemberValue<'a>> {
        let field = self.checked::<T>(ordinal, index)?;
        instance.read_member(ordinal, index).map_err(|error| {
            BindingError::new(
                error,
                BindingDiagnostic::Storage {
                    class: self.class,
                    member: field.symbol,
                    ordinal,
                    index,
                    write: false,
                },
            )
        })
    }
    pub fn write<T: AddonClass>(
        &self,
        instance: &mut T,
        ordinal: usize,
        index: usize,
        value: MemberValue<'_>,
        max_string_bytes: usize,
    ) -> Result<()> {
        self.write_detailed(instance, ordinal, index, value, max_string_bytes)
            .map_err(BindingError::into_error)
    }
    /// Invoke custom storage unchanged, then enrich any error. Custom conversions
    /// accepted by an AddonClass remain accepted.
    pub fn write_detailed<T: AddonClass>(
        &self,
        instance: &mut T,
        ordinal: usize,
        index: usize,
        value: MemberValue<'_>,
        max_string_bytes: usize,
    ) -> BindingResult<()> {
        let field = self.checked::<T>(ordinal, index)?;
        instance
            .write_member(ordinal, index, value, max_string_bytes)
            .map_err(|error| {
                let expected = self.schema.members[ordinal].kind;
                let given = value.kind();
                let diagnostic = if error.kind == ErrorKind::WrongObjectType && expected != given {
                    BindingDiagnostic::ValueType {
                        class: self.class,
                        member: field.symbol,
                        ordinal,
                        expected,
                        given,
                    }
                } else {
                    BindingDiagnostic::Storage {
                        class: self.class,
                        member: field.symbol,
                        ordinal,
                        index,
                        write: true,
                    }
                };
                BindingError::new(error, diagnostic)
            })
    }
}

trait Cell {
    fn read(&self) -> MemberValue<'_>;
    fn write(&mut self, value: MemberValue<'_>, max_bytes: usize) -> Result<()>;
}
trait Field {
    fn read(&self, index: usize) -> Result<MemberValue<'_>>;
    fn write(&mut self, index: usize, value: MemberValue<'_>, max_bytes: usize) -> Result<()>;
}
macro_rules! scalar_field {
    ($ty:ty) => {
        impl Field for $ty {
            fn read(&self, index: usize) -> Result<MemberValue<'_>> {
                if index != 0 {
                    return Err(error(ErrorKind::InvalidData));
                }
                Ok(Cell::read(self))
            }
            fn write(
                &mut self,
                index: usize,
                value: MemberValue<'_>,
                max_bytes: usize,
            ) -> Result<()> {
                if index != 0 {
                    return Err(error(ErrorKind::InvalidData));
                }
                Cell::write(self, value, max_bytes)
            }
        }
    };
}
impl Cell for i32 {
    fn read(&self) -> MemberValue<'_> {
        MemberValue::Int(*self)
    }
    fn write(&mut self, value: MemberValue<'_>, _: usize) -> Result<()> {
        let MemberValue::Int(v) = value else {
            return Err(error(ErrorKind::WrongObjectType));
        };
        *self = v;
        Ok(())
    }
}
impl Cell for f32 {
    fn read(&self) -> MemberValue<'_> {
        MemberValue::Float(*self)
    }
    fn write(&mut self, value: MemberValue<'_>, _: usize) -> Result<()> {
        let MemberValue::Float(v) = value else {
            return Err(error(ErrorKind::WrongObjectType));
        };
        *self = v;
        Ok(())
    }
}
impl Cell for Vec<u8> {
    fn read(&self) -> MemberValue<'_> {
        MemberValue::Bytes(self)
    }
    fn write(&mut self, value: MemberValue<'_>, max_bytes: usize) -> Result<()> {
        let MemberValue::Bytes(v) = value else {
            return Err(error(ErrorKind::WrongObjectType));
        };
        if v.len() > max_bytes {
            return Err(error(ErrorKind::LimitExceeded));
        }
        self.try_reserve(v.len().saturating_sub(self.len()))
            .map_err(|_| error(ErrorKind::LimitExceeded))?;
        self.clear();
        self.extend_from_slice(v);
        Ok(())
    }
}
scalar_field!(i32);
scalar_field!(f32);
scalar_field!(Vec<u8>);
mod enums;
impl<T: Cell, const N: usize> Field for [T; N] {
    fn read(&self, index: usize) -> Result<MemberValue<'_>> {
        Ok(Cell::read(
            self.get(index)
                .ok_or_else(|| error(ErrorKind::InvalidData))?,
        ))
    }
    fn write(&mut self, index: usize, value: MemberValue<'_>, max_bytes: usize) -> Result<()> {
        Cell::write(
            self.get_mut(index)
                .ok_or_else(|| error(ErrorKind::InvalidData))?,
            value,
            max_bytes,
        )
    }
}

macro_rules! define_class {
    ($name:ident, $script:literal, { $($ordinal:literal => $field:ident : $ty:ty = $init:expr,
        $member:literal, $kind:ident, $capacity:literal, $guard:expr;)* }, { $($extra:ident : $extra_ty:ty = $extra_init:expr;)* }) => {
        pub struct $name { $(pub $field: $ty,)* $(pub(super) $extra: $extra_ty,)* }
        impl Default for $name {
            fn default() -> Self { Self { $($field: $init,)* $($extra: $extra_init,)* } }
        }
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.debug_struct(stringify!($name)).finish_non_exhaustive() }
        }
        impl AddonClass for $name {
            const SCHEMA: &'static ClassSchema = &ClassSchema { name: $script, members: &[
                $(Member { name: $member, kind: MemberKind::$kind, capacity: $capacity, presence_guard: $guard },)*
            ]};
            fn read_member(&self, member: usize, index: usize) -> Result<MemberValue<'_>> {
                match member { $($ordinal => Field::read(&self.$field, index),)* _ => Err(error(ErrorKind::NotFound)) }
            }
            fn write_member(&mut self, member: usize, index: usize, value: MemberValue<'_>, max_bytes: usize) -> Result<()> {
                match member { $($ordinal => Field::write(&mut self.$field, index, value, max_bytes),)* _ => Err(error(ErrorKind::NotFound)) }
            }
        }
    };
}

mod classes;
pub use classes::*;
