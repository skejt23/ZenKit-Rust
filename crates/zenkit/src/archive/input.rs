// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;
use std::ops::Range;

/// Bounded physical bytes used to resolve absolute embedded BIN_SAFE table pointers.
/// This capability does not enlarge an archive's separate sequential body window.
#[derive(Clone, Copy)]
pub(crate) struct AddressSpace<'a> {
    pub(super) bytes: &'a [u8],
    pub(super) origin: usize,
}
impl std::fmt::Debug for AddressSpace<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AddressSpace")
            .field("origin", &self.origin)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}
impl<'a> AddressSpace<'a> {
    pub fn new(bytes: &'a [u8], origin: usize) -> Result<Self> {
        origin
            .checked_add(bytes.len())
            .ok_or_else(|| output_error(ErrorKind::InvalidData))?;
        Ok(Self { bytes, origin })
    }
    /// Borrow exactly this IO reader's window, including bytes before its cursor.
    /// No enclosing bytes outside that window are made accessible.
    pub fn from_reader(reader: &crate::io::Reader<'a>) -> Self {
        let (bytes, origin) = reader.view();
        Self { bytes, origin }
    }
    pub fn range(self) -> Range<usize> {
        self.origin..self.origin + self.bytes.len()
    }
    pub(super) fn reader(self, range: Range<usize>) -> Result<crate::io::Reader<'a>> {
        let start = range
            .start
            .checked_sub(self.origin)
            .ok_or_else(|| output_error(ErrorKind::InvalidData))?;
        let end = range
            .end
            .checked_sub(self.origin)
            .ok_or_else(|| output_error(ErrorKind::InvalidData))?;
        let bytes = self
            .bytes
            .get(start..end)
            .ok_or_else(|| output_error(ErrorKind::InvalidData))?;
        crate::io::Reader::with_origin(bytes, range.start)
    }
    pub(crate) fn contains(self, reader: &crate::io::Reader<'_>) -> bool {
        let (bytes, origin) = reader.view();
        let Some(start) = origin.checked_sub(self.origin) else {
            return false;
        };
        let Some(end) = start.checked_add(bytes.len()) else {
            return false;
        };
        self.bytes
            .get(start..end)
            .is_some_and(|v| std::ptr::eq(v.as_ptr(), bytes.as_ptr()))
    }
}

/// ASCII header framing. `Required` is mandatory for embedded MSH's unframed
/// count, whose low byte may itself be LF; no alternate framing is guessed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AsciiSeparator {
    Optional,
    Required,
}

/// Cumulative out-of-band table inspection work across embedded archives.
/// Repeated inspection of a shared table is charged repeatedly. Table key slots
/// are separately capped by `Limits.keys`; the constructor can simultaneously
/// own two key vectors, at most `keys * (size_of::<Option<&[u8]>>() +
/// size_of::<&[u8]>())` logical bytes, excluding allocator capacity overhead.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TableBudget {
    entries_limit: usize,
    bytes_limit: usize,
    entries: usize,
    bytes: usize,
}
impl TableBudget {
    pub fn new(entries: usize, bytes: usize) -> Self {
        Self {
            entries_limit: entries,
            bytes_limit: bytes,
            entries: 0,
            bytes: 0,
        }
    }
    #[cfg(test)]
    pub(crate) fn entries(&self) -> usize {
        self.entries
    }
    #[cfg(test)]
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }
    pub(super) fn charge(&mut self, entries: usize, bytes: usize, at: usize) -> Result<()> {
        let entries = self
            .entries
            .checked_add(entries)
            .ok_or_else(|| limited(at))?;
        let bytes = self.bytes.checked_add(bytes).ok_or_else(|| limited(at))?;
        if entries > self.entries_limit || bytes > self.bytes_limit {
            return Err(limited(at));
        }
        self.entries = entries;
        self.bytes = bytes;
        Ok(())
    }
}
