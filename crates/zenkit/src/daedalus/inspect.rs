// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;
use std::cmp::Ordering as Cmp;

fn logical(raw: &[u8]) -> impl Iterator<Item = u8> + '_ {
    raw.iter()
        .enumerate()
        .map(|(i, &b)| if i == 0 && b == 255 { b'$' } else { b })
}
fn folded(raw: &[u8]) -> impl Iterator<Item = u8> + '_ {
    logical(raw).map(|b| b.to_ascii_uppercase())
}

impl Script {
    fn raw(&self, index: u32) -> &[u8] {
        self.symbols[index as usize]
            .name
            .map_or(&[], |s| &self.bytes[s.range()])
    }
    pub(super) fn build_indexes(&mut self) -> Result<()> {
        let names = self.symbols.iter().filter(|s| s.name.is_some()).count();
        let addresses = self
            .symbols
            .iter()
            .filter(|s| {
                !s.member()
                    && (matches!(s.data_type(), DataType::Instance | DataType::Prototype)
                        || s.data_type() == DataType::Function && s.constant())
            })
            .count();
        let mut raw = Vec::new();
        reserve(&mut raw, names, self.offset)?;
        let mut fold = Vec::new();
        reserve(&mut fold, names, self.offset)?;
        let mut addr = Vec::new();
        reserve(&mut addr, addresses, self.offset)?;
        for s in self.symbols() {
            if s.raw_name().is_some() {
                raw.push(s.index());
                fold.push(s.index());
            }
            if s.address().is_some() && (s.data_type() != DataType::Function || s.is_const()) {
                addr.push(s.index());
            }
        }
        raw.sort_unstable_by(|a, b| self.raw(*a).cmp(self.raw(*b)).then(a.cmp(b)));
        fold.sort_unstable_by(|a, b| {
            folded(self.raw(*a))
                .cmp(folded(self.raw(*b)))
                .then(a.cmp(b))
        });
        addr.sort_unstable_by_key(|i| (self.symbol(*i).unwrap().address().unwrap() as u32, *i));
        self.raw_names = raw;
        self.names = fold;
        self.addresses = addr;
        Ok(())
    }
    fn equal_range<'a>(&'a self, indexes: &'a [u32], cmp: impl Fn(u32) -> Cmp) -> &'a [u32] {
        let start = indexes.partition_point(|&i| cmp(i).is_lt());
        let count = indexes[start..].partition_point(|&i| cmp(i).is_eq());
        &indexes[start..start + count]
    }
    /// Exact raw-name duplicates in symbol order; absent names are not indexed.
    pub fn raw_name_matches(
        &self,
        name: &[u8],
    ) -> impl ExactSizeIterator<Item = SymbolRef<'_>> + DoubleEndedIterator + FusedIterator {
        self.equal_range(&self.raw_names, |i| self.raw(i).cmp(name))
            .iter()
            .map(|&index| SymbolRef {
                script: self,
                index,
            })
    }
    pub fn find_raw_name(&self, name: &[u8]) -> Option<SymbolRef<'_>> {
        self.raw_name_matches(name).next_back()
    }
    /// Generated-name mapping and ASCII folding on both stored and query names.
    /// Non-ASCII bytes remain unchanged; no locale or Unicode conversion occurs.
    pub fn name_matches(
        &self,
        name: &[u8],
    ) -> impl ExactSizeIterator<Item = SymbolRef<'_>> + DoubleEndedIterator + FusedIterator {
        self.equal_range(&self.names, |i| folded(self.raw(i)).cmp(folded(name)))
            .iter()
            .map(|&index| SymbolRef {
                script: self,
                index,
            })
    }
    pub fn find_name(&self, name: &[u8]) -> Option<SymbolRef<'_>> {
        self.name_matches(name).next_back()
    }
    /// Address aliases for nonmember prototypes/instances and constant functions.
    /// Addresses are compared as stored u32 bits, including external sentinels.
    pub fn address_matches(
        &self,
        address: u32,
    ) -> impl ExactSizeIterator<Item = SymbolRef<'_>> + DoubleEndedIterator + FusedIterator {
        self.equal_range(&self.addresses, |i| {
            (self.symbol(i).unwrap().address().unwrap() as u32).cmp(&address)
        })
        .iter()
        .map(|&index| SymbolRef {
            script: self,
            index,
        })
    }
    pub fn find_address(&self, address: u32) -> Option<SymbolRef<'_>> {
        self.address_matches(address).next_back()
    }

    /// Check the retained sort table for a permutation and nondecreasing *raw*
    /// byte names (missing names compare as empty). Generated FF is not rewritten.
    /// Uses one byte of fallible scratch per symbol, capped by `max_symbols`.
    pub fn validate_sort_table(&self, max_symbols: usize) -> Result<()> {
        if self.len() > max_symbols {
            return Err(at(ErrorKind::LimitExceeded, self.offset + 5));
        }
        let mut seen = Vec::new();
        reserve(&mut seen, self.len(), self.offset + 5)?;
        seen.resize(self.len(), false);
        let mut previous = None;
        for (n, &index) in self.sort.iter().enumerate() {
            let offset = self.offset + 5 + n * 4;
            if index as usize >= self.len() || seen[index as usize] {
                return Err(at(ErrorKind::InvalidData, offset));
            }
            if previous.is_some_and(|p| self.raw(p) > self.raw(index)) {
                return Err(at(ErrorKind::InvalidData, offset));
            }
            seen[index as usize] = true;
            previous = Some(index);
        }
        Ok(())
    }
    /// Positional parameter window; no invented name/parent/type convention.
    pub fn parameters(
        &self,
        function: SymbolId,
    ) -> Result<impl ExactSizeIterator<Item = SymbolRef<'_>> + DoubleEndedIterator + FusedIterator>
    {
        let f = self.get(function)?;
        if f.is_member() || f.data_type() != DataType::Function {
            return Err(at(ErrorKind::WrongObjectType, f.offset()));
        }
        let start = f.index as usize + 1;
        let end = start + usize::from(f.count());
        if end > self.len() {
            return Err(at(ErrorKind::InvalidData, f.offset()));
        }
        Ok((start..end).map(|i| self.symbol(i as u32).unwrap()))
    }
    /// Contiguous logical-name `function_name.` run immediately after parameters.
    /// A run ending at the last symbol is retained. Comparison is case-sensitive.
    pub fn locals(
        &self,
        function: SymbolId,
    ) -> Result<impl ExactSizeIterator<Item = SymbolRef<'_>> + DoubleEndedIterator + FusedIterator>
    {
        let f = self.get(function)?;
        let _parameters = self.parameters(function)?;
        let start = f.index as usize + 1 + usize::from(f.count());
        let name = self.raw(f.index);
        let count = self.symbols[start..]
            .iter()
            .enumerate()
            .take_while(|(n, _)| {
                let local = self.raw((start + n) as u32);
                local.len() > name.len()
                    && local[name.len()] == b'.'
                    && logical(&local[..name.len()]).eq(logical(name))
            })
            .count();
        Ok((start..start + count).map(|i| self.symbol(i as u32).unwrap()))
    }
    /// Members in storage order. Only the requested class is checked.
    pub fn class_members(
        &self,
        class: SymbolId,
    ) -> Result<impl DoubleEndedIterator<Item = SymbolRef<'_>> + FusedIterator> {
        let c = self.get(class)?;
        if c.is_member() || c.data_type() != DataType::Class {
            return Err(at(ErrorKind::WrongObjectType, c.offset()));
        }
        Ok(self
            .symbols()
            .filter(move |s| s.is_member() && s.parent() == c.index as i32))
    }
    /// Resolve an exact class, member, prototype or instance to its class.
    /// Prototype chains may point forward. Bad parents/cycles are checked only
    /// along the requested path; no assumption is made about unrelated parent words.
    pub fn class_of(&self, id: SymbolId) -> Result<SymbolRef<'_>> {
        let mut current = self.get(id)?;
        for _ in 0..=self.len() {
            if !current.is_member() && current.data_type() == DataType::Class {
                return Ok(current);
            }
            let member = current.is_member();
            if !member
                && !matches!(
                    current.data_type(),
                    DataType::Instance | DataType::Prototype
                )
            {
                return Err(at(ErrorKind::WrongObjectType, current.offset()));
            }
            let parent = current.parent();
            let next = u32::try_from(parent)
                .ok()
                .and_then(|p| self.symbol(p))
                .ok_or_else(|| at(ErrorKind::InvalidData, current.parent_offset()))?;
            if next.is_member()
                || !(next.data_type() == DataType::Class
                    || !member && next.data_type() == DataType::Prototype)
            {
                return Err(at(ErrorKind::WrongObjectType, current.parent_offset()));
            }
            current = next;
        }
        Err(at(ErrorKind::CycleDetected, current.parent_offset()))
    }
    /// Enumerate constant instances through checked prototype ancestry, including
    /// forward definitions. Validates all prototypes and constant instances.
    /// One memo table and one reusable path of at most `max_symbols` entries are
    /// allocated fallibly; work is linear in the symbol table, not quadratic.
    pub fn instances_of(&self, class: SymbolId, max_symbols: usize) -> Result<Instances<'_>> {
        let c = self.get(class)?;
        if c.is_member() || c.data_type() != DataType::Class {
            return Err(at(ErrorKind::WrongObjectType, c.offset()));
        }
        if self.len() > max_symbols {
            return Err(at(ErrorKind::LimitExceeded, self.offset));
        }
        #[derive(Clone, Copy)]
        enum State {
            Unseen,
            Visiting,
            Class(u32),
        }
        let mut memo = Vec::new();
        reserve(&mut memo, self.len(), self.offset)?;
        memo.resize(self.len(), State::Unseen);
        let mut path = Vec::new();
        reserve(&mut path, self.len(), self.offset)?;
        let mut matches = Vec::new();
        for candidate in self.symbols() {
            if candidate.is_member()
                || !(candidate.data_type() == DataType::Prototype
                    || candidate.data_type() == DataType::Instance && candidate.is_const())
            {
                continue;
            }
            path.clear();
            let mut current = candidate;
            let class = loop {
                if !current.is_member() && current.data_type() == DataType::Class {
                    break current.index;
                }
                match memo[current.index as usize] {
                    State::Class(c) => break c,
                    State::Visiting => {
                        return Err(at(ErrorKind::CycleDetected, current.parent_offset()));
                    }
                    State::Unseen => {}
                }
                memo[current.index as usize] = State::Visiting;
                path.push(current.index);
                let next = u32::try_from(current.parent())
                    .ok()
                    .and_then(|p| self.symbol(p))
                    .ok_or_else(|| at(ErrorKind::InvalidData, current.parent_offset()))?;
                if next.is_member()
                    || !matches!(next.data_type(), DataType::Class | DataType::Prototype)
                {
                    return Err(at(ErrorKind::WrongObjectType, current.parent_offset()));
                }
                current = next;
            };
            for &index in &path {
                memo[index as usize] = State::Class(class);
            }
            if class == c.index && candidate.data_type() == DataType::Instance {
                reserve(&mut matches, 1, candidate.offset())?;
                matches.push(candidate.index);
            }
        }
        Ok(Instances {
            script: self,
            indices: matches,
            cursor: 0,
        })
    }
}

/// Owned bounded result indices borrowing their Script; iteration yields views.
pub struct Instances<'a> {
    script: &'a Script,
    indices: Vec<u32>,
    cursor: usize,
}
impl fmt::Debug for Instances<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Instances")
            .field("remaining", &self.len())
            .finish()
    }
}
impl<'a> Iterator for Instances<'a> {
    type Item = SymbolRef<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        let index = *self.indices.get(self.cursor)?;
        self.cursor += 1;
        self.script.symbol(index)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.indices.len() - self.cursor;
        (n, Some(n))
    }
}
impl DoubleEndedIterator for Instances<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.cursor == self.indices.len() {
            None
        } else {
            self.script.symbol(self.indices.pop().unwrap())
        }
    }
}
impl ExactSizeIterator for Instances<'_> {}
impl FusedIterator for Instances<'_> {}
