//! Binary math, date and bounds records.
//!
//! Fixed-size records and [`Obb`] preserve floating-point bit patterns on input
//! and output, including NaNs and signed zero. Reading and writing a whole record
//! is atomic on error. Readers report absolute offsets; writers report no input
//! offset. Geometric validation is separate from lossless serialization.
//! Color and quaternion component order is chosen by each file format.
//!
//! ```
//! use zenkit::{io::{Reader, Writer}, values::{Date, Mat3, Vec3}};
//! let date = Date { year: 2026, month: 10, day: 14, reserved: 0xabcd,
//!                   ..Date::default() };
//! let matrix = Mat3::from_rows([
//!     Vec3::from([1.0, 2.0, 3.0]), Vec3::from([4.0, 5.0, 6.0]),
//!     Vec3::from([7.0, 8.0, 9.0]),
//! ]);
//! let mut output = Writer::new(52);
//! date.write(&mut output, "date")?;
//! matrix.write(&mut output, "matrix")?;
//! let mut input = Reader::new(output.as_ref());
//! assert_eq!(Date::read(&mut input, "date")?, date);
//! assert_eq!(Mat3::read(&mut input, "matrix")?, matrix);
//! assert_eq!(matrix[1][0], 2.0); // column, then row
//! # Ok::<(), zenkit::Error>(())
//! ```
// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT

use crate::{
    Error, ErrorKind, Result,
    io::{Reader, Writer},
};
use std::{
    fmt,
    ops::{Index, IndexMut, Mul},
};

fn read_floats<const N: usize>(input: &mut Reader<'_>, context: &'static str) -> Result<[f32; N]> {
    let mut candidate = input.clone();
    let mut values = [0.0; N];
    for value in &mut values {
        *value = candidate.f32(context)?;
    }
    *input = candidate;
    Ok(values)
}

fn write_floats<const N: usize>(
    values: [f32; N],
    output: &mut Writer,
    context: &'static str,
) -> Result<()> {
    output.bytes(values.map(f32::to_le_bytes).as_flattened(), context)
}

macro_rules! vector {
    ($name:ident, $n:literal, $($field:ident:$index:literal),+) => {
        #[doc = concat!(stringify!($n), " float components in declaration order. Default is zero.")]
        /// Vector multiplication is component-wise; scalar multiplication scales
        /// every component. Indexing follows declaration order and panics if out
        /// of range. No implicit normalization or finite-value checks occur.
        #[derive(Clone, Copy, Debug, Default, PartialEq)]
        pub struct $name { $(pub $field: f32),+ }
        impl $name {
            /// All components zero.
            pub const ZERO: Self = Self { $($field: 0.0),+ };
            /// Fill every component with the same value.
            pub const fn splat(value: f32) -> Self { Self { $($field: value),+ } }
            /// Components in declaration order.
            pub const fn to_array(self) -> [f32; $n] { [$(self.$field),+] }
            /// Read consecutive little-endian float32 components atomically.
            pub fn read(input: &mut Reader<'_>, context: &'static str) -> Result<Self> {
                Ok(Self::from(read_floats::<$n>(input, context)?))
            }
            /// Write consecutive little-endian float32 components atomically.
            pub fn write(&self, output: &mut Writer, context: &'static str) -> Result<()> {
                write_floats(self.to_array(), output, context)
            }
        }
        impl From<[f32; $n]> for $name {
            fn from(value: [f32; $n]) -> Self { Self { $($field: value[$index]),+ } }
        }
        impl From<$name> for [f32; $n] { fn from(value: $name) -> Self { value.to_array() } }
        impl Index<usize> for $name {
            type Output = f32;
            fn index(&self, index: usize) -> &Self::Output {
                match index { $($index => &self.$field,)+ _ => panic!("vector index out of bounds") }
            }
        }
        impl IndexMut<usize> for $name {
            fn index_mut(&mut self, index: usize) -> &mut Self::Output {
                match index { $($index => &mut self.$field,)+ _ => panic!("vector index out of bounds") }
            }
        }
        impl Mul<f32> for $name {
            type Output = Self;
            fn mul(self, scalar: f32) -> Self { Self { $($field: self.$field * scalar),+ } }
        }
        impl Mul for $name {
            type Output = Self;
            fn mul(self, other: Self) -> Self { Self { $($field: self.$field * other.$field),+ } }
        }
    };
}
vector!(Vec2, 2, x:0, y:1);
vector!(Vec3, 3, x:0, y:1, z:2);
vector!(Vec4, 4, x:0, y:1, z:2, w:3);

/// Four byte channels. Default is transparent black; array conversions are RGBA.
///
/// Multiplication wraps each channel modulo 256; it does not normalize to 0..1
/// or perform alpha compositing. Wire encodings choose their channel order
/// explicitly (for example [`Self::to_bgra`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}
impl Color {
    pub const fn from_rgba([r, g, b, a]: [u8; 4]) -> Self {
        Self { r, g, b, a }
    }
    pub const fn to_rgba(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }
    pub const fn from_bgra([b, g, r, a]: [u8; 4]) -> Self {
        Self { r, g, b, a }
    }
    pub const fn to_bgra(self) -> [u8; 4] {
        [self.b, self.g, self.r, self.a]
    }
}
impl From<[u8; 4]> for Color {
    fn from(value: [u8; 4]) -> Self {
        Self::from_rgba(value)
    }
}
impl From<Color> for [u8; 4] {
    fn from(value: Color) -> Self {
        value.to_rgba()
    }
}
impl Index<usize> for Color {
    type Output = u8;
    fn index(&self, index: usize) -> &u8 {
        match index {
            0 => &self.r,
            1 => &self.g,
            2 => &self.b,
            3 => &self.a,
            _ => panic!("color index out of bounds"),
        }
    }
}
impl IndexMut<usize> for Color {
    fn index_mut(&mut self, index: usize) -> &mut u8 {
        match index {
            0 => &mut self.r,
            1 => &mut self.g,
            2 => &mut self.b,
            3 => &mut self.a,
            _ => panic!("color index out of bounds"),
        }
    }
}
impl Mul<u8> for Color {
    type Output = Self;
    fn mul(self, scalar: u8) -> Self {
        Self::from_rgba(self.to_rgba().map(|v| v.wrapping_mul(scalar)))
    }
}
impl Mul for Color {
    type Output = Self;
    fn mul(self, other: Self) -> Self {
        Self::from_rgba(std::array::from_fn(|i| self[i].wrapping_mul(other[i])))
    }
}

/// Quaternion components, without implicit normalization or a universal wire order.
///
/// Default is four zeroes; [`Self::IDENTITY`] is the unit rotation.
/// Explicit conversions distinguish WXYZ from XYZW.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Quat {
    pub w: f32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
impl Quat {
    pub const IDENTITY: Self = Self {
        w: 1.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    pub const fn from_wxyz([w, x, y, z]: [f32; 4]) -> Self {
        Self { w, x, y, z }
    }
    pub const fn to_wxyz(self) -> [f32; 4] {
        [self.w, self.x, self.y, self.z]
    }
    pub const fn from_xyzw([x, y, z, w]: [f32; 4]) -> Self {
        Self { w, x, y, z }
    }
    pub const fn to_xyzw(self) -> [f32; 4] {
        [self.x, self.y, self.z, self.w]
    }
}

macro_rules! matrix {
    ($name:ident, $vector:ident, $n:literal, $count:literal) => {
        #[doc = concat!(stringify!($n), " by ", stringify!($n), " matrix stored as columns, serialized as rows.")]
        /// Indexing is `[column][row]`. Default is the zero matrix, not identity.
        #[derive(Clone, Copy, Debug, Default, PartialEq)]
        pub struct $name { pub columns: [$vector; $n] }
        impl $name {
            pub const fn from_columns(columns: [$vector; $n]) -> Self { Self { columns } }
            pub fn from_rows(rows: [$vector; $n]) -> Self { Self::from_columns(rows).transpose() }
            pub fn to_rows(self) -> [$vector; $n] { self.transpose().columns }
            pub fn transpose(self) -> Self {
                Self { columns: std::array::from_fn(|column| $vector::from(std::array::from_fn(|row| self[row][column]))) }
            }
            /// Read row-major little-endian floats, preserving all float bits.
            pub fn read(input: &mut Reader<'_>, context: &'static str) -> Result<Self> {
                let values = read_floats::<$count>(input, context)?;
                Ok(Self::from_rows(std::array::from_fn(|row| $vector::from(std::array::from_fn(|column| values[row * $n + column])))))
            }
            /// Write row-major little-endian floats, atomically on failure.
            pub fn write(&self, output: &mut Writer, context: &'static str) -> Result<()> {
                write_floats::<$count>(std::array::from_fn(|i| self[i % $n][i / $n]), output, context)
            }
        }
        impl From<[$vector; $n]> for $name { fn from(columns: [$vector; $n]) -> Self { Self::from_columns(columns) } }
        impl From<$name> for [$vector; $n] { fn from(value: $name) -> Self { value.columns } }
        impl Index<usize> for $name {
            type Output = $vector;
            fn index(&self, index: usize) -> &$vector { &self.columns[index] }
        }
        impl IndexMut<usize> for $name { fn index_mut(&mut self, index: usize) -> &mut $vector { &mut self.columns[index] } }
    };
}
matrix!(Mat3, Vec3, 3, 9);
matrix!(Mat4, Vec4, 4, 16);
impl Mat3 {
    pub const IDENTITY: Self = Self::from_columns([
        Vec3 {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        },
        Vec3 {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        },
        Vec3 {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        },
    ]);
}
impl Mat4 {
    pub const IDENTITY: Self = Self::from_columns([
        Vec4 {
            x: 1.0,
            y: 0.0,
            z: 0.0,
            w: 0.0,
        },
        Vec4 {
            x: 0.0,
            y: 1.0,
            z: 0.0,
            w: 0.0,
        },
        Vec4 {
            x: 0.0,
            y: 0.0,
            z: 1.0,
            w: 0.0,
        },
        Vec4 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        },
    ]);
}

/// The 16-byte ZenGin date record, not a Unix timestamp or validated calendar date.
///
/// All field values are preserved, including the final reserved word.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Date {
    pub year: u32,
    pub month: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
    pub reserved: u16,
}
impl Date {
    pub fn read(input: &mut Reader<'_>, context: &'static str) -> Result<Self> {
        let mut candidate = input.clone();
        let value = Self {
            year: candidate.u32(context)?,
            month: candidate.u16(context)?,
            day: candidate.u16(context)?,
            hour: candidate.u16(context)?,
            minute: candidate.u16(context)?,
            second: candidate.u16(context)?,
            reserved: candidate.u16(context)?,
        };
        *input = candidate;
        Ok(value)
    }
    pub fn write(&self, output: &mut Writer, context: &'static str) -> Result<()> {
        let mut bytes = [0; 16];
        bytes[..4].copy_from_slice(&self.year.to_le_bytes());
        for (pair, value) in bytes[4..].as_chunks_mut::<2>().0.iter_mut().zip([
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second,
            self.reserved,
        ]) {
            pair.copy_from_slice(&value.to_le_bytes());
        }
        output.bytes(&bytes, context)
    }
}

/// Two corners in minimum-then-maximum wire order; no geometric validation on I/O.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}
impl Aabb {
    pub const ZERO: Self = Self {
        min: Vec3::ZERO,
        max: Vec3::ZERO,
    };
    pub fn read(input: &mut Reader<'_>, context: &'static str) -> Result<Self> {
        let [x0, y0, z0, x1, y1, z1] = read_floats(input, context)?;
        Ok(Self {
            min: Vec3::from([x0, y0, z0]),
            max: Vec3::from([x1, y1, z1]),
        })
    }
    pub fn write(&self, output: &mut Writer, context: &'static str) -> Result<()> {
        write_floats(
            [
                self.min.x, self.min.y, self.min.z, self.max.x, self.max.y, self.max.z,
            ],
            output,
            context,
        )
    }
}

/// Limits for a complete OBB tree, including its root.
///
/// `depth` counts the root as one. A hard maximum of [`Obb::MAX_DEPTH`] applies
/// even when `depth` is larger, bounding parsed ownership depth and cleanup.
/// Parsing and writing use explicit traversal state rather than recursive calls.
/// Zero nodes or depth rejects every tree. These are logical limits, not a
/// process allocator quota; input is already bounded by its [`Reader`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObbLimits {
    pub nodes: usize,
    pub depth: usize,
}
impl Default for ObbLimits {
    fn default() -> Self {
        Self {
            nodes: 100_000,
            depth: Obb::MAX_DEPTH,
        }
    }
}

/// An owned oriented box tree. Each wire node is 15 floats, a u16 child count,
/// then its children in order. Axes are not normalized automatically.
///
/// Parsed trees have bounded depth. Manually constructed trees can exceed it;
/// writing rejects those trees, but callers remain responsible for keeping their
/// own recursively owned values safe to clone, compare and drop. Debug output
/// includes only this node and its immediate child count.
#[derive(Clone, Default, PartialEq)]
pub struct Obb {
    pub center: Vec3,
    pub axes: [Vec3; 3],
    pub half_width: Vec3,
    pub children: Vec<Obb>,
}
impl fmt::Debug for Obb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Obb")
            .field("center", &self.center)
            .field("axes", &self.axes)
            .field("half_width", &self.half_width)
            .field("children", &self.children.len())
            .finish()
    }
}
impl Obb {
    pub const MAX_DEPTH: usize = 128;
    const NODE_BYTES: usize = 62;

    /// Parse one complete tree, preserving the reader cursor on every error.
    pub fn read(input: &mut Reader<'_>, limits: ObbLimits, context: &'static str) -> Result<Self> {
        let mut candidate = input.clone();
        let mut budget = limits.nodes;
        let depth = limits.depth.min(Self::MAX_DEPTH);
        let (mut value, mut children) =
            Self::read_node_header(&mut candidate, depth, &mut budget, context)?;
        let mut parents: Vec<(Self, usize)> = Vec::new();
        if children != 0 {
            parents
                .try_reserve_exact(depth.min(limits.nodes))
                .map_err(|_| Error::new(ErrorKind::LimitExceeded, candidate.position(), context))?;
        }
        loop {
            if children != 0 {
                parents.push((value, children));
                (value, children) = Self::read_node_header(
                    &mut candidate,
                    depth - parents.len(),
                    &mut budget,
                    context,
                )?;
            } else if let Some((mut parent, remaining)) = parents.pop() {
                // Each parent's exact child capacity was reserved at its header.
                parent.children.push(value);
                value = parent;
                children = remaining - 1;
            } else {
                *input = candidate;
                return Ok(value);
            }
        }
    }
    fn read_node_header(
        input: &mut Reader<'_>,
        depth: usize,
        budget: &mut usize,
        context: &'static str,
    ) -> Result<(Self, usize)> {
        if depth == 0 || *budget == 0 {
            return Err(Error::new(
                ErrorKind::LimitExceeded,
                input.position(),
                context,
            ));
        }
        *budget -= 1;
        let mut value = Self {
            center: Vec3::read(input, context)?,
            axes: [
                Vec3::read(input, context)?,
                Vec3::read(input, context)?,
                Vec3::read(input, context)?,
            ],
            half_width: Vec3::read(input, context)?,
            children: Vec::new(),
        };
        let count_offset = input.position();
        let count = usize::from(input.u16(context)?);
        if count > *budget || (count != 0 && depth == 1) {
            return Err(Error::new(ErrorKind::LimitExceeded, count_offset, context));
        }
        // Every child needs at least one full node, regardless of descendants.
        if count > input.remaining() / Self::NODE_BYTES {
            return Err(Error::new(
                ErrorKind::UnexpectedEof,
                input.position(),
                context,
            ));
        }
        value
            .children
            .try_reserve_exact(count)
            .map_err(|_| Error::new(ErrorKind::LimitExceeded, count_offset, context))?;
        Ok((value, count))
    }

    /// Serialize a complete tree atomically, including overwrite failures.
    ///
    /// Rejects more than 65535 immediate children and all node/depth/output limit
    /// violations before changing output. A bounded temporary buffer holds the
    /// record; the destination commits it in one call. Float bits are untouched.
    pub fn write(
        &self,
        output: &mut Writer,
        limits: ObbLimits,
        context: &'static str,
    ) -> Result<()> {
        let depth = limits.depth.min(Self::MAX_DEPTH);
        if depth == 0 || limits.nodes == 0 {
            return Err(Error::operation(ErrorKind::LimitExceeded, context));
        }
        let mut remaining = limits.nodes;
        let mut parents: Vec<std::slice::Iter<'_, Self>> = Vec::new();
        parents
            .try_reserve_exact(depth.min(limits.nodes))
            .map_err(|_| Error::operation(ErrorKind::LimitExceeded, context))?;
        let mut record = Writer::new(output.limit() - output.position());
        let mut current = Some(self);
        while let Some(node) = current {
            if parents.len() >= depth
                || remaining == 0
                || node.children.len() > usize::from(u16::MAX)
            {
                return Err(Error::operation(ErrorKind::LimitExceeded, context));
            }
            remaining -= 1;
            node.center.write(&mut record, context)?;
            for axis in &node.axes {
                axis.write(&mut record, context)?;
            }
            node.half_width.write(&mut record, context)?;
            record.u16(node.children.len() as u16, context)?;
            if !node.children.is_empty() {
                parents.push(node.children.iter());
            }
            current = loop {
                match parents.last_mut() {
                    Some(children) => {
                        if let Some(child) = children.next() {
                            break Some(child);
                        }
                        parents.pop();
                    }
                    None => break None,
                }
            };
        }
        output.bytes(record.as_slice(), context)
    }

    /// Bound this node's eight corners; descendants do not affect the result.
    ///
    /// Axes may be nonunit, nonorthogonal or zero. Negative half widths, any
    /// nonfinite input, or nonfinite f32 corner arithmetic give `InvalidData`.
    /// Zero extents are valid. Corner operations use f32 arithmetic without
    /// normalization or fused multiply-add. The result encloses these computed
    /// corners, not an outward-rounded exact-real geometry interval.
    pub fn as_aabb(&self) -> Result<Aabb> {
        let invalid = || Error::operation(ErrorKind::InvalidData, "OBB geometry");
        if self
            .center
            .to_array()
            .into_iter()
            .chain(self.axes.into_iter().flat_map(Vec3::to_array))
            .chain(self.half_width.to_array())
            .any(|v| !v.is_finite())
            || self.half_width.to_array().into_iter().any(|v| v < 0.0)
        {
            return Err(invalid());
        }
        let mut result = Aabb {
            min: Vec3::splat(f32::INFINITY),
            max: Vec3::splat(f32::NEG_INFINITY),
        };
        for x in [-1.0, 1.0] {
            for y in [-1.0, 1.0] {
                for z in [-1.0, 1.0] {
                    let a = self.axes[0] * self.half_width.x * x;
                    let b = self.axes[1] * self.half_width.y * y;
                    let c = self.axes[2] * self.half_width.z * z;
                    for i in 0..3 {
                        let point = self.center[i] + (a[i] + b[i] + c[i]);
                        if !point.is_finite() {
                            return Err(invalid());
                        }
                        result.min[i] = result.min[i].min(point);
                        result.max[i] = result.max[i].max(point);
                    }
                }
            }
        }
        Ok(result)
    }
}
