// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;

/// Recognized Daedalus opcodes, including the unsupported MOVSS instruction.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(u8)]
pub enum Opcode {
    Add = 0,
    Sub = 1,
    Mul = 2,
    Div = 3,
    Mod = 4,
    Or = 5,
    Andb = 6,
    Lt = 7,
    Gt = 8,
    Movi = 9,
    Orr = 11,
    And = 12,
    Lsl = 13,
    Lsr = 14,
    Lte = 15,
    Eq = 16,
    Neq = 17,
    Gte = 18,
    Addmovi = 19,
    Submovi = 20,
    Mulmovi = 21,
    Divmovi = 22,
    Plus = 30,
    Negate = 31,
    Not = 32,
    Cmpl = 33,
    Nop = 45,
    Rsr = 60,
    Bl = 61,
    Be = 62,
    Pushi = 64,
    Pushv = 65,
    Pushvi = 67,
    Movs = 70,
    Movss = 71,
    Movvf = 72,
    Movf = 73,
    Movvi = 74,
    B = 75,
    Bz = 76,
    Gmovi = 80,
    Pushvv = 245,
}
impl Opcode {
    pub const ALL: [Self; 42] = [
        Self::Add,
        Self::Sub,
        Self::Mul,
        Self::Div,
        Self::Mod,
        Self::Or,
        Self::Andb,
        Self::Lt,
        Self::Gt,
        Self::Movi,
        Self::Orr,
        Self::And,
        Self::Lsl,
        Self::Lsr,
        Self::Lte,
        Self::Eq,
        Self::Neq,
        Self::Gte,
        Self::Addmovi,
        Self::Submovi,
        Self::Mulmovi,
        Self::Divmovi,
        Self::Plus,
        Self::Negate,
        Self::Not,
        Self::Cmpl,
        Self::Nop,
        Self::Rsr,
        Self::Bl,
        Self::Be,
        Self::Pushi,
        Self::Pushv,
        Self::Pushvi,
        Self::Movs,
        Self::Movss,
        Self::Movvf,
        Self::Movf,
        Self::Movvi,
        Self::B,
        Self::Bz,
        Self::Gmovi,
        Self::Pushvv,
    ];
    pub fn from_raw(value: u8) -> Option<Self> {
        OPCODES[usize::from(value)]
    }
    pub fn raw(self) -> u8 {
        self as u8
    }
    pub fn size(self) -> u8 {
        match self {
            Self::Pushvv => 6,
            Self::Bl
            | Self::Be
            | Self::Pushi
            | Self::Pushv
            | Self::Pushvi
            | Self::B
            | Self::Bz
            | Self::Gmovi => 5,
            _ => 1,
        }
    }
    /// Whether the VM supports executing this opcode.
    pub fn is_implemented(self) -> bool {
        self != Self::Movss
    }
}
/// Opcode by raw byte value.
const OPCODES: [Option<Opcode>; 256] = {
    let mut table = [None; 256];
    let mut i = 0;
    while i < Opcode::ALL.len() {
        table[Opcode::ALL[i] as usize] = Some(Opcode::ALL[i]);
        i += 1;
    }
    table
};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operand {
    None,
    Address(u32),
    Immediate(i32),
    Symbol(u32),
    Element { symbol: u32, index: u8 },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Instruction {
    /// Offset relative to the start of bytecode, not an absolute DAT position.
    pub offset: u32,
    pub op: Opcode,
    pub operand: Operand,
}
impl Instruction {
    pub fn size(self) -> u8 {
        self.op.size()
    }
}
/// Limits for a complete instruction index, which keeps one u32 per instruction.
///
/// `instructions` also bounds an [`Executable`]: it keeps
/// [`EXECUTABLE_BYTES_PER_INSTRUCTION`] bytes per instruction (the index plus
/// one decoded step) and 4 bytes per symbol. Size this limit for that memory
/// when validating for execution; the default allows about 80 MB.
#[derive(Clone, Copy, Debug)]
pub struct DecodeLimits {
    pub instructions: usize,
}
/// Bytes an [`Executable`] keeps per instruction: a u32 offset and a decoded step.
pub const EXECUTABLE_BYTES_PER_INSTRUCTION: usize = 4 + std::mem::size_of::<Step>();
const _: () = assert!(EXECUTABLE_BYTES_PER_INSTRUCTION == 20);
impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            instructions: 4_000_000,
        }
    }
}

impl Script {
    /// Decode at an arbitrary byte offset without proving it is an instruction
    /// boundary. Use [`DecodedCode::instruction_at`] for boundary-checked access.
    pub fn decode_at(&self, offset: u32) -> Result<Instruction> {
        let bytes = self
            .code()
            .get(offset as usize..)
            .ok_or_else(|| at(ErrorKind::InvalidData, self.code_offset() + offset as usize))?;
        let raw = *bytes.first().ok_or_else(|| {
            at(
                ErrorKind::UnexpectedEof,
                self.code_offset() + offset as usize,
            )
        })?;
        let op = Opcode::from_raw(raw)
            .ok_or_else(|| at(ErrorKind::Unsupported, self.code_offset() + offset as usize))?;
        if bytes.len() < usize::from(op.size()) {
            return Err(at(
                ErrorKind::UnexpectedEof,
                self.code_offset() + offset as usize + 1,
            ));
        }
        let word = || u32::from_le_bytes(bytes[1..5].try_into().unwrap());
        let operand = match op {
            Opcode::Bl | Opcode::B | Opcode::Bz => Operand::Address(word()),
            Opcode::Pushi => Operand::Immediate(word() as i32),
            Opcode::Be | Opcode::Pushv | Opcode::Pushvi | Opcode::Gmovi => Operand::Symbol(word()),
            Opcode::Pushvv => Operand::Element {
                symbol: word(),
                index: bytes[5],
            },
            _ => Operand::None,
        };
        Ok(Instruction {
            offset,
            op,
            operand,
        })
    }
    /// Allocation-free sequential decode; stops permanently after the first error.
    /// The explicit instruction cap also bounds work on long runs of one-byte code.
    pub fn instructions(&self, limits: DecodeLimits) -> Instructions<'_> {
        Instructions {
            script: self,
            offset: 0,
            count: 0,
            limit: limits.instructions,
            failed: false,
        }
    }
    /// Linear decode from byte zero, retaining a fallibly allocated boundary index.
    /// All 42 known opcodes, including MOVSS, are structurally accepted here.
    pub fn decode_code(&self, limits: DecodeLimits) -> Result<DecodedCode<'_>> {
        let mut offsets = Vec::new();
        for result in self.instructions(limits) {
            let ins = result?;
            reserve(&mut offsets, 1, self.code_offset() + ins.offset as usize)?;
            offsets.push(ins.offset);
        }
        Ok(DecodedCode {
            script: self,
            offsets,
        })
    }
    /// Check complete decoding, branch/call boundaries, callable entrypoints,
    /// symbol operands, BE's external function role, GMOVI's instance role and
    /// PUSHVV element bounds. MOVSS is explicitly unsupported for execution.
    /// PUSHV/PUSHVI only require an existing symbol (count zero can be valid).
    /// External sentinel addresses and irrelevant parent words remain valid.
    /// BL uses the last address alias, exactly as address lookup does: nonmember
    /// prototypes/instances (including nonconst instances) or const functions.
    /// An external last alias rejects the call; earlier aliases are not substituted.
    /// This does not prove stack/control-flow safety or provide execution.
    pub fn validate_code(&self, limits: DecodeLimits) -> Result<Executable<'_>> {
        self.validate_code_inner(limits, true)
    }
    pub(crate) fn validate_code_inner(
        &self,
        limits: DecodeLimits,
        static_extents: bool,
    ) -> Result<Executable<'_>> {
        let decoded = self.decode_code(limits)?;
        for s in self.symbols() {
            if s.is_member() || s.is_external() {
                continue;
            }
            if (s.data_type() == DataType::Prototype
                || s.is_const() && matches!(s.data_type(), DataType::Function | DataType::Instance))
                && s.address().is_none_or(|a| !decoded.contains(a as u32))
            {
                return Err(at(ErrorKind::InvalidData, s.data_offset()));
            }
        }
        let mut steps = Vec::new();
        reserve(&mut steps, decoded.len(), self.code_offset())?;
        for ins in decoded.instructions() {
            let position = self.code_offset() + ins.offset as usize;
            if !ins.op.is_implemented() {
                return Err(at(ErrorKind::Unsupported, position));
            }
            let mut link = 0;
            match ins.operand {
                Operand::Address(address) => {
                    let Some(target) = decoded.index_of(address) else {
                        return Err(at(ErrorKind::InvalidData, position + 1));
                    };
                    link = target as u32;
                    if ins.op == Opcode::Bl {
                        let target = self
                            .find_address(address)
                            .ok_or_else(|| at(ErrorKind::UnresolvedReference, position + 1))?;
                        if target.is_external() {
                            return Err(at(ErrorKind::WrongObjectType, position + 1));
                        }
                        link = target.index();
                    }
                }
                Operand::Symbol(index) | Operand::Element { symbol: index, .. } => {
                    let symbol = self
                        .symbol(index)
                        .ok_or_else(|| at(ErrorKind::UnresolvedReference, position + 1))?;
                    if ins.op == Opcode::Be
                        && (symbol.is_member()
                            || symbol.data_type() != DataType::Function
                            || !symbol.is_external())
                        || ins.op == Opcode::Gmovi
                            && (symbol.is_member() || symbol.data_type() != DataType::Instance)
                    {
                        return Err(at(ErrorKind::WrongObjectType, position + 1));
                    }
                    if let Operand::Element { index, .. } = ins.operand
                        && static_extents
                        && u16::from(index) >= symbol.count()
                    {
                        return Err(at(ErrorKind::InvalidData, position + 5));
                    }
                }
                _ => {}
            }
            steps.push(Step {
                operand: ins.operand,
                link,
                op: ins.op,
            });
        }
        let mut entries = Vec::new();
        reserve(&mut entries, self.len(), self.code_offset())?;
        entries.extend(self.symbols().map(|s| {
            s.address()
                .and_then(|a| decoded.index_of(a as u32))
                .map_or(NO_ENTRY, |i| i as u32)
        }));
        Ok(Executable {
            decoded,
            steps,
            entries,
        })
    }
}
pub struct Instructions<'a> {
    script: &'a Script,
    offset: usize,
    count: usize,
    limit: usize,
    failed: bool,
}
impl Iterator for Instructions<'_> {
    type Item = Result<Instruction>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.offset == self.script.code().len() {
            return None;
        }
        if self.count == self.limit {
            self.failed = true;
            return Some(Err(at(
                ErrorKind::LimitExceeded,
                self.script.code_offset() + self.offset,
            )));
        }
        let result = self.script.decode_at(self.offset as u32);
        match &result {
            Ok(ins) => {
                self.offset += usize::from(ins.size());
                self.count += 1;
            }
            Err(_) => self.failed = true,
        }
        Some(result)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (
            0,
            Some(if self.failed {
                0
            } else {
                self.script.code().len() - self.offset
            }),
        )
    }
}
impl FusedIterator for Instructions<'_> {}

/// A fully decoded linear stream with known instruction boundaries.
pub struct DecodedCode<'a> {
    script: &'a Script,
    offsets: Vec<u32>,
}
impl fmt::Debug for DecodedCode<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DecodedCode")
            .field("instructions", &self.len())
            .finish()
    }
}
impl<'a> DecodedCode<'a> {
    pub fn script(&self) -> &'a Script {
        self.script
    }
    pub fn len(&self) -> usize {
        self.offsets.len()
    }
    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }
    pub fn offsets(&self) -> &[u32] {
        &self.offsets
    }
    pub fn contains(&self, offset: u32) -> bool {
        self.index_of(offset).is_some()
    }
    /// Position of the instruction starting at `offset` in [`Self::offsets`].
    pub(crate) fn index_of(&self, offset: u32) -> Option<usize> {
        self.offsets.binary_search(&offset).ok()
    }
    pub fn instruction_at(&self, offset: u32) -> Result<Instruction> {
        if !self.contains(offset) {
            return Err(at(
                ErrorKind::InvalidData,
                self.script.code_offset() + offset as usize,
            ));
        }
        self.script.decode_at(offset)
    }
    pub fn instructions(
        &self,
    ) -> impl ExactSizeIterator<Item = Instruction> + DoubleEndedIterator + FusedIterator + '_ {
        self.offsets.iter().map(|&offset| {
            self.script
                .decode_at(offset)
                .expect("immutable complete decoded index")
        })
    }
}
/// One validated instruction without its offset, which is kept in the index.
/// `link` holds a resolved operand: for B and BZ the target's instruction
/// position, for BL the callee's symbol index, otherwise zero.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Step {
    pub operand: Operand,
    pub link: u32,
    pub op: Opcode,
}
/// Symbol without an instruction at its address.
const NO_ENTRY: u32 = u32::MAX;

/// Checked instruction roles and targets; not an executing VM or a stack proof.
/// Keeps every instruction decoded with its branch target and call target
/// resolved, plus each symbol's entry position, so execution does not search.
pub struct Executable<'a> {
    decoded: DecodedCode<'a>,
    steps: Vec<Step>,
    entries: Vec<u32>,
}
impl fmt::Debug for Executable<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Executable")
            .field("decoded", &self.decoded)
            .finish()
    }
}
impl<'a> Executable<'a> {
    pub fn decoded(&self) -> &DecodedCode<'a> {
        &self.decoded
    }
    pub fn script(&self) -> &'a Script {
        self.decoded.script
    }
    pub fn instruction_at(&self, offset: u32) -> Result<Instruction> {
        self.decoded.instruction_at(offset)
    }
    /// Position of the instruction at `offset`, trying `hint` before searching.
    /// Fails exactly like [`Self::instruction_at`] for a non-boundary offset.
    #[inline]
    pub(crate) fn locate(&self, hint: usize, offset: u32) -> Result<usize> {
        if self.decoded.offsets.get(hint) == Some(&offset) {
            return Ok(hint);
        }
        self.decoded.index_of(offset).ok_or_else(|| {
            at(
                ErrorKind::InvalidData,
                self.script().code_offset() + offset as usize,
            )
        })
    }
    /// The instruction at a position returned by [`Self::locate`] or
    /// [`Self::entry`], with its resolved link.
    #[inline]
    pub(crate) fn step(&self, position: usize) -> (Instruction, u32) {
        let s = self.steps[position];
        (
            Instruction {
                offset: self.decoded.offsets[position],
                op: s.op,
                operand: s.operand,
            },
            s.link,
        )
    }
    /// Instruction position of a symbol's address, if one starts there.
    #[inline]
    pub(crate) fn entry(&self, symbol: u32) -> Option<usize> {
        self.entries
            .get(symbol as usize)
            .filter(|&&i| i != NO_ENTRY)
            .map(|&i| i as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(out: &mut Vec<u8>, n: u32) {
        out.extend_from_slice(&n.to_le_bytes());
    }
    fn symbol(name: &[u8], ty: u32, flags: u32, address: u32) -> Vec<u8> {
        let mut out = Vec::new();
        word(&mut out, 1);
        out.extend_from_slice(name);
        out.push(10);
        word(&mut out, 0);
        word(&mut out, ty << 12 | flags << 16);
        for _ in 0..5 {
            word(&mut out, 0);
        }
        word(&mut out, address);
        word(&mut out, u32::MAX);
        out
    }
    fn script(symbols: &[Vec<u8>], code: &[u8]) -> Script {
        let mut bytes = vec![50];
        word(&mut bytes, symbols.len() as u32);
        for i in 0..symbols.len() {
            word(&mut bytes, i as u32);
        }
        for s in symbols {
            bytes.extend_from_slice(s);
        }
        word(&mut bytes, code.len() as u32);
        bytes.extend_from_slice(code);
        Script::from_bytes(&bytes, Limits::default()).unwrap()
    }

    #[test]
    fn opcode_table_matches_every_raw_byte() {
        for raw in 0..=u8::MAX {
            assert_eq!(
                Opcode::from_raw(raw),
                Opcode::ALL.into_iter().find(|op| op.raw() == raw)
            );
        }
    }

    #[test]
    fn steps_keep_each_instruction_with_resolved_branch_and_call_targets() {
        // 0: F: PUSHI 7; BZ 15; B 0;  15: G: BL 0; BL 15; RSR
        let mut code = vec![64, 7, 0, 0, 0, 76];
        word(&mut code, 15);
        code.push(75);
        word(&mut code, 0);
        code.push(61);
        word(&mut code, 0);
        code.push(61);
        word(&mut code, 15);
        code.push(60);
        let script = script(
            &[
                symbol(b"F", 5, 1, 0),
                symbol(b"G", 5, 1, 15),
                // A later alias of F's address; BL takes the last one.
                symbol(b"P", 5, 1, 0),
                symbol(b"E", 5, 9, u32::MAX),
                // Not an address alias, but its address starts an instruction.
                symbol(b"MID", 5, 0, 5),
            ],
            &code,
        );
        let code = script.validate_code(DecodeLimits::default()).unwrap();
        let decoded: Vec<Instruction> = code.decoded().instructions().collect();
        assert_eq!(decoded.len(), 6);
        let links: Vec<u32> = (0..decoded.len())
            .map(|i| {
                let (ins, link) = code.step(i);
                assert_eq!(ins, decoded[i]);
                link
            })
            .collect();
        // BZ 15 and B 0 hold target positions; BL holds the last alias's symbol.
        assert_eq!(links, [0, 3, 0, 2, 1, 0]);
        assert_eq!(
            (0..5).map(|s| code.entry(s)).collect::<Vec<_>>(),
            [Some(0), Some(3), Some(0), None, Some(1)]
        );
        assert_eq!(code.entry(5), None);
    }

    #[test]
    fn locate_checks_its_hint_and_fails_like_instruction_lookup() {
        let script = script(&[], &[64, 1, 0, 0, 0, 45, 60]);
        let code = script.validate_code(DecodeLimits::default()).unwrap();
        assert_eq!(code.locate(1, 5).unwrap(), 1);
        // A stale or out-of-range hint falls back to the index.
        assert_eq!(code.locate(0, 6).unwrap(), 2);
        assert_eq!(code.locate(99, 0).unwrap(), 0);
        for offset in [1, 7, u32::MAX] {
            let located = code.locate(0, offset).unwrap_err();
            let looked_up = code.instruction_at(offset).unwrap_err();
            assert_eq!(located.kind, looked_up.kind);
            assert_eq!(located.offset, looked_up.offset);
        }
    }
}
