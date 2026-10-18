// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::machine::{Binding, DefaultBinding, Frame};
use super::*;
enum Flow {
    Next,
    Jump,
    Return,
}

fn returned(result: &HostReturn) -> GivenType {
    match result {
        HostReturn::Value(value) => GivenType::Value(value.data_type()),
        HostReturn::Void => GivenType::Void,
        HostReturn::StackHandled => GivenType::StackHandled,
    }
}

impl<'s> Vm<'s> {
    pub fn call_named<H: Host>(
        &mut self,
        host: &mut H,
        name: &[u8],
        args: Vec<Argument>,
    ) -> VmResult<Option<Value>, H::Error> {
        let id = self
            .script()
            .find_name(name)
            .ok_or_else(|| {
                Fault::missing_name(
                    FaultOperation::Call,
                    name,
                    self.state.limits.diagnostic_name_bytes,
                )
            })?
            .id();
        self.call(host, id, args)
    }
    /// Checked host call. Direct calls bypass address overrides, as in the source.
    /// Errors restore control/context and recursive snapshots, not global/host edits.
    pub fn call<H: Host>(
        &mut self,
        host: &mut H,
        symbol: SymbolId,
        args: Vec<Argument>,
    ) -> VmResult<Option<Value>, H::Error> {
        self.call_inner(host, symbol, args, false)
    }
    /// Call and discard the returned stack entry without resolving a reference
    /// or copying its bytes.
    pub fn call_discard<H: Host>(
        &mut self,
        host: &mut H,
        symbol: SymbolId,
        args: Vec<Argument>,
    ) -> VmResult<(), H::Error> {
        self.call_inner(host, symbol, args, true).map(|_| ())
    }
    fn call_inner<H: Host>(
        &mut self,
        host: &mut H,
        symbol: SymbolId,
        args: Vec<Argument>,
        discard: bool,
    ) -> VmResult<Option<Value>, H::Error> {
        let s = self.script().get(symbol).map_err(Fault::from)?;
        self.validate_call(s, &args)?;
        let base = self.stack.len();
        let depth = self.frames.len();
        let pc = self.pc;
        let context = self.context;
        let outer = self.invocations == 0 && self.host_depth == 0;
        if outer {
            self.fuel = self.state.limits.fuel;
            self.terminal = None;
        }
        self.invocations += 1;
        let result = (|| {
            for a in args {
                match a {
                    Argument::Value(v) => self.push(v)?,
                    Argument::Reference(r) => self.push_ref(r)?,
                }
            }
            let ty = self.result_type(s)?;
            self.enter(symbol, pc)?;
            self.drive(host, depth)?;
            if let Some(kind) = self.terminal {
                return Err(fault(kind).into());
            }
            let result = if ty == DataType::Void || discard {
                None
            } else {
                Some(self.pop_typed(host, ty)?)
            };
            Ok(result)
        })();
        let result = result.map_err(|e| self.annotate(e));
        self.unwind(depth, base, pc, context);
        self.invocations -= 1;
        result
    }
    // Keep validation temporaries off the stack held across host reentry.
    #[inline(never)]
    fn validate_call(&self, s: SymbolRef<'_>, args: &[Argument]) -> Result<()> {
        if s.is_member() || s.data_type() != DataType::Function {
            return Err(fault(FaultKind::WrongType).at(s.id()).types(
                FaultOperation::Call,
                ExpectedType::TopLevelFunction,
                GivenType::Symbol(s.into()),
            ));
        }
        let params = self.script().parameters(s.id())?;
        if params.len() != args.len() {
            return Err(fault(FaultKind::Signature)
                .at(s.id())
                .with_detail(FaultDetail::Counts {
                    operation: FaultOperation::Call,
                    role: CountRole::Arguments,
                    expected: params.len(),
                    given: args.len(),
                }));
        }
        for (position, (p, a)) in params.zip(args).enumerate() {
            let good = match a {
                Argument::Value(v) => {
                    self.state.check_value(v)?;
                    value_matches(v, p.data_type())
                }
                Argument::Reference(r) => {
                    self.reference(r.symbol, r.index, r.context)?;
                    self.script().get(r.symbol)?.data_type() == p.data_type()
                }
            };
            if !good {
                let given = match a {
                    Argument::Value(v) => GivenType::Value(v.data_type()),
                    Argument::Reference(r) => {
                        GivenType::Reference(self.script().get(r.symbol)?.data_type())
                    }
                };
                return Err(fault(FaultKind::Signature).at(p.id()).with_detail(
                    FaultDetail::ParameterType {
                        operation: FaultOperation::Call,
                        callable: s.id(),
                        position,
                        expected: p.data_type(),
                        given,
                    },
                ));
            }
        }
        Ok(())
    }
    /// Checked low-level call for an existing argument stack and any callable
    /// function/prototype/instance entry. Leaves its repaired result on the stack.
    pub fn call_from_stack<H: Host>(
        &mut self,
        host: &mut H,
        symbol: SymbolId,
    ) -> VmResult<(), H::Error> {
        let s = self.script().get(symbol).map_err(Fault::from)?;
        let n = self.parameter_count(s)?;
        let base = self.argument_base(n)?;
        let depth = self.frames.len();
        let pc = self.pc;
        let context = self.context;
        if self.invocations == 0 && self.host_depth == 0 {
            self.fuel = self.state.limits.fuel;
            self.terminal = None;
        }
        self.invocations += 1;
        let result = (|| {
            self.enter(symbol, pc)?;
            self.drive(host, depth)
        })();
        let result = result.map_err(|e| self.annotate(e));
        if result.is_err() {
            self.unwind(depth, base, pc, context);
        }
        self.invocations -= 1;
        result
    }
    /// Bind and initialize an existing instance. Old symbol aliases remain valid.
    /// SELF/current context are restored even if initialization fails; data edits
    /// and the new binding remain visible for inspection or retry.
    pub fn init_instance<H: Host>(
        &mut self,
        host: &mut H,
        id: InstanceId,
        symbol: SymbolId,
    ) -> VmResult<(), H::Error> {
        self.bind_instance(id, symbol)?;
        let old = self.context;
        let self_symbol = self.global_self();
        let old_self = if let Some(s) = self_symbol {
            Some(
                self.state
                    .global(s, 0)?
                    .as_instance()
                    .map_err(|error| error.at(s))?,
            )
        } else {
            None
        };
        self.context = Some(id);
        if let Some(s) = self_symbol {
            self.set_global(s, 0, Value::Instance(Some(id)))?;
        }
        let result = self.call_from_stack(host, symbol);
        self.context = old;
        if let (Some(s), Some(v)) = (self_symbol, old_self) {
            self.set_global(s, 0, Value::Instance(v))?;
        }
        result
    }
    pub fn init_opaque_instance<H: Host>(
        &mut self,
        host: &mut H,
        symbol: SymbolId,
    ) -> VmResult<InstanceId, H::Error> {
        let class = self
            .script()
            .class_of(symbol)
            .map_err(|error| {
                let fault = Fault::from(error);
                if self.script().get(symbol).is_ok() {
                    fault.at(symbol).with_detail(FaultDetail::Access {
                        operation: FaultOperation::InstanceBinding,
                    })
                } else {
                    fault
                }
            })?
            .id();
        let binding = if let Some(c) = self.state.registered_class(class)? {
            c
        } else {
            self.register_opaque(class)?
        };
        let id = self.allocate_opaque(symbol, binding)?;
        self.init_instance(host, id, symbol)?;
        Ok(id)
    }
    fn entry(&self, s: SymbolRef<'_>) -> Result<u32> {
        if s.is_member()
            || s.is_external()
            || !matches!(
                s.data_type(),
                DataType::Function | DataType::Instance | DataType::Prototype
            )
        {
            return Err(fault(FaultKind::WrongType).at(s.id()).types(
                FaultOperation::Call,
                ExpectedType::ScriptCallable,
                GivenType::Symbol(s.into()),
            ));
        }
        let pc = s
            .address()
            .ok_or_else(|| fault(FaultKind::CodeBoundary).at(s.id()))? as u32;
        if !self.code.decoded().contains(pc) {
            return Err(fault(FaultKind::CodeBoundary)
                .at(s.id())
                .with_detail(FaultDetail::Address { requested: pc }));
        }
        Ok(pc)
    }
    fn make_frame(&mut self, symbol: SymbolId, return_pc: u32) -> Result<()> {
        let s = self.script().get(symbol)?;
        let count = self.parameter_count(s)?;
        let base = self.argument_base(count)?;
        let rr = checked(
            self.frames.len(),
            1,
            self.state.limits.frames,
            LimitKind::Frames,
        );
        self.note(rr)?;
        let rr = reserve(&mut self.frames, 1);
        self.note(rr)?;
        let result = self.result_type(s)?;
        let mut locals = Vec::new();
        let mut local_cells = 0;
        let mut local_bytes = 0;
        if self.state.recursive_locals(symbol)?
            && self.frames.iter().any(|f| f.trace.symbol == symbol)
        {
            for l in self.script().locals(symbol)? {
                let values = &self.state.slots[l.index() as usize].values;
                // The saved descriptor is retained even when its array is empty.
                local_cells = checked(
                    local_cells,
                    values.len() + 1,
                    self.state
                        .limits
                        .snapshot_cells
                        .saturating_sub(self.snapshot_cells),
                    LimitKind::Snapshots,
                )?;
                let mut copied = Vec::new();
                reserve(&mut copied, values.len())?;
                for v in values {
                    local_bytes = checked(
                        local_bytes,
                        v.bytes_len(),
                        self.free_strings(),
                        LimitKind::Strings,
                    )?;
                    copied.push(v.try_copy(v.bytes_len())?);
                }
                reserve(&mut locals, 1)?;
                locals.push((l.id(), copied));
            }
        }
        self.temporary_bytes += local_bytes;
        self.snapshot_cells += local_cells;
        self.frames.push(Frame {
            trace: FrameTrace {
                symbol,
                return_pc,
                stack_base: base,
                context: self.context,
            },
            result,
            locals,
            local_cells,
            local_bytes,
        });
        Ok(())
    }
    fn enter(&mut self, symbol: SymbolId, return_pc: u32) -> Result<()> {
        let pc = self.entry(self.script().get(symbol)?)?;
        let rr = self.make_frame(symbol, return_pc);
        self.note(rr)?;
        self.pc = pc;
        Ok(())
    }
    fn restore_locals(&mut self, frame: &mut Frame) {
        for (symbol, values) in frame.locals.drain(..) {
            let slot = &mut self.state.slots[symbol.index() as usize];
            let before = slot.values.iter().map(Value::bytes_len).sum::<usize>();
            let after = values.iter().map(Value::bytes_len).sum::<usize>();
            self.state.bytes = self.state.bytes - before + after;
            self.state.cells = self.state.cells - slot.values.len() + values.len();
            slot.values = values;
        }
        self.snapshot_cells -= frame.local_cells;
        self.temporary_bytes -= frame.local_bytes;
    }
    fn leave(&mut self) -> Result<()> {
        let mut frame = self
            .frames
            .pop()
            .ok_or_else(|| fault(FaultKind::StackUnderflow))?;
        let base = frame.trace.stack_base;
        let remaining = self.stack.len().saturating_sub(base);
        let outcome = if frame.result == DataType::Void {
            self.truncate_stack(base);
            Ok(())
        } else if remaining == 1 {
            Ok(())
        } else if !self.policy.repair_returns {
            Err(fault(FaultKind::InvalidReturn)
                .at(frame.trace.symbol)
                .with_detail(FaultDetail::Counts {
                    operation: FaultOperation::Return,
                    role: CountRole::ReturnStack,
                    expected: 1,
                    given: remaining,
                }))
        } else if remaining == 0 {
            let value = default_value(frame.result)?;
            self.push(value)
        } else {
            let value = self.stack.pop().unwrap();
            self.temporary_bytes -= value.bytes();
            self.truncate_stack(base);
            self.push_stack(value)
        };
        self.restore_locals(&mut frame);
        self.pc = frame.trace.return_pc;
        self.context = frame.trace.context;
        outcome
    }
    fn unwind(&mut self, depth: usize, base: usize, pc: u32, context: Option<InstanceId>) {
        while self.frames.len() > depth {
            let mut frame = self.frames.pop().unwrap();
            self.restore_locals(&mut frame);
        }
        self.truncate_stack(base);
        self.pc = pc;
        self.context = context;
    }
    fn annotate<E>(&self, mut e: VmError<E>) -> VmError<E> {
        if e.pc.is_none() {
            e.pc = Some(self.pc);
            e.instruction = self.code.instruction_at(self.pc).ok();
        }
        if e.frames.is_empty() {
            let n = self.frames.len().min(self.state.limits.trace_frames);
            if e.frames.try_reserve(n).is_ok() {
                e.frames
                    .extend(self.frames.iter().rev().take(n).map(|f| f.trace));
            } else {
                e.trace_truncated = true;
            }
            e.trace_truncated |= n < self.frames.len();
        }
        e
    }
    fn pop_typed<H: Host>(&mut self, host: &mut H, ty: DataType) -> VmResult<Value, H::Error> {
        Ok(match ty {
            DataType::Int => Value::Int(self.pop_int(host)?),
            DataType::Function => Value::Function(self.pop_int(host)?),
            DataType::Float => Value::Float(self.pop_float(host)?),
            DataType::String => Value::Bytes(self.pop_string(host)?),
            DataType::Instance => Value::Instance(self.pop_instance(host)?),
            _ => {
                return Err(fault(FaultKind::WrongType)
                    .types(
                        FaultOperation::StackPop,
                        ExpectedType::ReturnValue,
                        GivenType::Type(ty),
                    )
                    .into());
            }
        })
    }
    fn drive<H: Host>(&mut self, host: &mut H, depth: usize) -> VmResult<(), H::Error> {
        while self.frames.len() > depth {
            if let Some(kind) = self.terminal {
                return Err(self.annotate(fault(kind).into()));
            }
            if self.fuel == 0 {
                self.terminal = Some(FaultKind::Limit(LimitKind::Fuel));
                return Err(self.annotate(fault(FaultKind::Limit(LimitKind::Fuel)).into()));
            }
            self.fuel -= 1;
            let ins = self.code.instruction_at(self.pc).map_err(Fault::from)?;
            let result = self.execute(host, ins);
            match result {
                Ok(Flow::Next) => {
                    self.pc = self
                        .pc
                        .checked_add(u32::from(ins.size()))
                        .ok_or_else(|| fault(FaultKind::CodeBoundary))?
                }
                Ok(Flow::Jump) => {}
                Ok(Flow::Return) => self.leave()?,
                Err(e) => self.recover(host, ins, e)?,
            }
        }
        Ok(())
    }
    #[cold]
    #[inline(never)]
    fn recover<H: Host>(
        &mut self,
        host: &mut H,
        ins: Instruction,
        e: VmError<H::Error>,
    ) -> VmResult<(), H::Error> {
        let e = self.annotate(e);
        if e.fault.is_terminal() {
            self.terminal = Some(e.fault.kind);
            return Err(e);
        }
        let old_pc = self.pc;
        match self.host(host, |h, cx| h.exception(cx, &e))? {
            Recovery::Fail => return Err(e),
            Recovery::Continue => {
                if self.pc == old_pc {
                    self.pc = self
                        .pc
                        .checked_add(u32::from(ins.size()))
                        .ok_or_else(|| fault(FaultKind::CodeBoundary))?;
                }
            }
            Recovery::Return => self.leave()?,
            Recovery::Jump(pc) => self.jump(pc)?,
        }
        Ok(())
    }
    fn copied_binding(&mut self, index: usize, over: bool) -> Result<Binding> {
        let b = if over {
            &self.overrides[index]
        } else {
            &self.externals[index]
        };
        let mut parameters = Vec::new();
        reserve(&mut parameters, b.parameters.len())?;
        parameters.extend_from_slice(&b.parameters);
        Ok(Binding {
            symbol: b.symbol,
            key: b.key,
            parameters,
            result: b.result,
            raw: b.raw,
            address: b.address,
        })
    }
    fn arguments<H: Host>(
        &mut self,
        host: &mut H,
        callable: SymbolId,
        params: &[Parameter],
    ) -> VmResult<(Vec<Argument>, usize), H::Error> {
        let mut args = Vec::new();
        let rr = reserve(&mut args, params.len());
        self.note(rr)?;
        let mut bytes = 0;
        let result = (|| {
            for (position, &p) in params.iter().enumerate().rev() {
                // Charge each owned value before resolving the next reference:
                // repeated references must not allocate an oversized packet first.
                let value = (|| -> VmResult<Argument, H::Error> {
                    Ok(match p {
                        Parameter::InstanceReference => {
                            let r = self.pop_reference()?.symbol()?;
                            let given = self
                                .script()
                                .get(r.symbol)
                                .map_err(Fault::from)?
                                .data_type();
                            if given != DataType::Instance {
                                return Err(fault(FaultKind::WrongType)
                                    .at(r.symbol)
                                    .types(
                                        FaultOperation::StackPop,
                                        ExpectedType::Exact(DataType::Instance),
                                        GivenType::Reference(given),
                                    )
                                    .into());
                            }
                            Argument::Reference(r)
                        }
                        Parameter::Value(t) => Argument::Value(self.pop_typed(host, t)?),
                    })
                })()
                .map_err(|mut error| {
                    if error.argument.is_none() {
                        let parameter = self
                            .script()
                            .parameters(callable)
                            .ok()
                            .and_then(|mut p| p.nth(position))
                            .map(|p| p.id());
                        error.argument = Some(ArgumentContext {
                            callable,
                            position,
                            parameter,
                        });
                    }
                    error
                })?;
                let added = match &value {
                    Argument::Value(v) => v.bytes_len(),
                    Argument::Reference(_) => 0,
                };
                let rr = checked(0, added, self.free_strings(), LimitKind::Strings);
                self.note(rr)?;
                self.temporary_bytes += added;
                bytes += added;
                args.push(value);
            }
            Ok(())
        })();
        if let Err(e) = result {
            self.temporary_bytes -= bytes;
            return Err(e);
        }
        args.reverse();
        Ok((args, bytes))
    }
    fn invoke<H: Host>(
        &mut self,
        host: &mut H,
        selected: SymbolId,
        b: Binding,
        kind: CallKind,
        next: u32,
    ) -> VmResult<(), H::Error> {
        if b.raw {
            return self.invoke_raw(host, selected, b.key, kind);
        }
        let depth = self.frames.len();
        let old_pc = self.pc;
        let old_context = self.context;
        self.make_frame(b.symbol, next)?;
        let base = self.frames.last().unwrap().trace.stack_base;
        let result = (|| {
            let (arguments, bytes) = self.arguments(host, b.symbol, &b.parameters)?;
            let result = self.host(host, |h, cx| {
                h.call(
                    cx,
                    HostCall {
                        binding: b.key,
                        symbol: selected,
                        kind,
                        arguments,
                    },
                )
            });
            self.temporary_bytes -= bytes;
            let result = result?;
            let given = returned(&result);
            match result {
                HostReturn::Void if b.result == DataType::Void => {}
                HostReturn::Value(value) if value_matches(&value, b.result) => self.push(value)?,
                _ => {
                    return Err(fault(FaultKind::InvalidReturn)
                        .at(selected)
                        .with_detail(FaultDetail::Return {
                            callable: Some(selected),
                            expected: ReturnExpectation::Type(b.result),
                            given,
                        })
                        .into());
                }
            }
            self.leave()?;
            Ok(())
        })()
        .map_err(|e| self.annotate(e));
        if result.is_err() {
            self.unwind(depth, base, old_pc, old_context);
            if b.result != DataType::Void && self.terminal.is_none() {
                self.push(default_value(b.result)?)?;
            }
        }
        result
    }
    #[inline(never)]
    fn invoke_raw<H: Host>(
        &mut self,
        host: &mut H,
        selected: SymbolId,
        key: u32,
        kind: CallKind,
    ) -> VmResult<(), H::Error> {
        let result = self.host(host, |h, cx| {
            h.call(
                cx,
                HostCall {
                    binding: key,
                    symbol: selected,
                    kind,
                    arguments: Vec::new(),
                },
            )
        })?;
        if !matches!(result, HostReturn::StackHandled | HostReturn::Void) {
            return Err(fault(FaultKind::InvalidReturn)
                .with_detail(FaultDetail::Return {
                    callable: Some(selected),
                    expected: ReturnExpectation::StackHandledOrVoid,
                    given: returned(&result),
                })
                .into());
        }
        Ok(())
    }
    fn external<H: Host>(
        &mut self,
        host: &mut H,
        symbol: SymbolId,
        next: u32,
    ) -> VmResult<(), H::Error> {
        if let Some(i) = self.external_index(symbol) {
            let rr = self.copied_binding(i, false);
            let b = self.note(rr)?;
            return self.invoke(host, symbol, b, CallKind::External, next);
        }
        self.external_default(host, symbol)
    }
    #[inline(never)]
    fn external_default<H: Host>(
        &mut self,
        host: &mut H,
        symbol: SymbolId,
    ) -> VmResult<(), H::Error> {
        match self.default {
            DefaultBinding::Error => Err(fault(FaultKind::MissingExternal).at(symbol).into()),
            DefaultBinding::Raw(key) => {
                let r = self.host(host, |h, cx| {
                    h.call(
                        cx,
                        HostCall {
                            binding: key,
                            symbol,
                            kind: CallKind::RawDefault,
                            arguments: Vec::new(),
                        },
                    )
                })?;
                if !matches!(r, HostReturn::StackHandled | HostReturn::Void) {
                    return Err(fault(FaultKind::InvalidReturn)
                        .with_detail(FaultDetail::Return {
                            callable: Some(symbol),
                            expected: ReturnExpectation::StackHandledOrVoid,
                            given: returned(&r),
                        })
                        .into());
                }
                Ok(())
            }
            DefaultBinding::Zero(observer) => {
                let mut params = Vec::new();
                for p in self.script().parameters(symbol).map_err(Fault::from)? {
                    reserve(&mut params, 1)?;
                    params.push(Parameter::Value(p.data_type()));
                }
                let ty = self.result_type(self.script().get(symbol).map_err(Fault::from)?)?;
                let (args, bytes) = self.arguments(host, symbol, &params)?;
                let result = (|| {
                    if ty != DataType::Void {
                        self.push(default_value(ty)?)?;
                    }
                    if let Some(key) = observer {
                        let r = self.host(host, |h, cx| {
                            h.call(
                                cx,
                                HostCall {
                                    binding: key,
                                    symbol,
                                    kind: CallKind::DefaultExternal,
                                    arguments: args,
                                },
                            )
                        })?;
                        if !matches!(r, HostReturn::Void | HostReturn::StackHandled) {
                            return Err(fault(FaultKind::InvalidReturn)
                                .with_detail(FaultDetail::Return {
                                    callable: Some(symbol),
                                    expected: ReturnExpectation::StackHandledOrVoid,
                                    given: returned(&r),
                                })
                                .into());
                        }
                    }
                    Ok(())
                })();
                self.temporary_bytes -= bytes;
                result
            }
        }
    }
    fn execute<H: Host>(&mut self, host: &mut H, ins: Instruction) -> VmResult<Flow, H::Error> {
        use Opcode::*;
        match ins.op {
            Nop => Ok(Flow::Next),
            Rsr => Ok(Flow::Return),
            Movss => Err(fault(FaultKind::Unsupported).into()),
            Pushi => {
                let Operand::Immediate(n) = ins.operand else {
                    unreachable!()
                };
                self.push(Value::Int(n))?;
                Ok(Flow::Next)
            }
            Pushv | Pushvi | Pushvv => self.exec_push(host, ins),
            B | Bz => self.exec_branch(host, ins),
            Bl => self.exec_call(host, ins),
            Be => self.exec_external(host, ins),
            Gmovi => {
                let Operand::Symbol(i) = ins.operand else {
                    unreachable!()
                };
                let id = self.symbol_id(i)?;
                self.context = self.state.global(id, 0)?.as_instance()?;
                Ok(Flow::Next)
            }
            Movi | Movvf | Movf | Movs | Movvi => self.exec_move(host, ins.op),
            Addmovi | Submovi | Mulmovi | Divmovi => self.exec_compound(host, ins.op),
            Plus | Negate | Not | Cmpl => self.exec_unary(host, ins.op),
            Add | Sub | Mul | Div | Mod | Or | Andb | Lt | Gt | Lsl | Lsr | Lte | Eq | Neq
            | Gte | Orr | And => self.exec_binary(host, ins.op),
        }
    }
    fn symbol_id(&self, index: u32) -> Result<SymbolId> {
        self.script()
            .symbol(index)
            .map(|s| s.id())
            .ok_or_else(|| fault(FaultKind::InvalidHandle))
    }
    fn exec_push<H: Host>(&mut self, host: &mut H, ins: Instruction) -> VmResult<Flow, H::Error> {
        let (id, index) = match ins.operand {
            Operand::Symbol(i) => (self.symbol_id(i)?, 0),
            Operand::Element { symbol: i, index } => (self.symbol_id(i)?, usize::from(index)),
            _ => unreachable!(),
        };
        let reference = self.reference(id, index, self.context)?;
        if ins.op == Opcode::Pushvv && index >= self.state.runtime_count(id)? {
            return Err(fault(FaultKind::Index)
                .at(id)
                .element(index)
                .with_detail(FaultDetail::Index {
                    operation: FaultOperation::Reference,
                    length: self.state.runtime_count(id)?,
                })
                .into());
        }
        if ins.op != Opcode::Pushvv && self.state.access_trap(id)? {
            match self.host(host, |h, cx| h.access_trap(cx, reference))? {
                Trap::Default => self.push_ref(reference)?,
                Trap::Value(v) => self.push(v)?,
                Trap::Reference(r) => self.push_ref(r)?,
                Trap::Handled => {}
            }
        } else {
            self.push_ref(reference)?;
        }
        Ok(Flow::Next)
    }
    fn exec_branch<H: Host>(&mut self, host: &mut H, ins: Instruction) -> VmResult<Flow, H::Error> {
        let Operand::Address(pc) = ins.operand else {
            unreachable!()
        };
        if ins.op == Opcode::B || self.pop_int(host)? == 0 {
            self.jump(pc)?;
            Ok(Flow::Jump)
        } else {
            Ok(Flow::Next)
        }
    }
    fn exec_call<H: Host>(&mut self, host: &mut H, ins: Instruction) -> VmResult<Flow, H::Error> {
        let Operand::Address(pc) = ins.operand else {
            unreachable!()
        };
        let id = self
            .script()
            .find_address(pc)
            .ok_or_else(|| {
                fault(FaultKind::NotFound).with_detail(FaultDetail::Address { requested: pc })
            })?
            .id();
        if let Some(i) = self.override_index(pc) {
            let rr = self.copied_binding(i, true);
            let b = self.note(rr)?;
            let kind = if b.raw {
                CallKind::RawOverride
            } else {
                CallKind::Override
            };
            let raw = b.raw;
            let result = self.invoke(host, id, b, kind, ins.offset + u32::from(ins.size()));
            if raw {
                self.guard_return(id, result)?;
                Ok(Flow::Next)
            } else {
                result?;
                Ok(Flow::Jump)
            }
        } else {
            self.enter(id, ins.offset + u32::from(ins.size()))?;
            Ok(Flow::Jump)
        }
    }
    fn exec_external<H: Host>(
        &mut self,
        host: &mut H,
        ins: Instruction,
    ) -> VmResult<Flow, H::Error> {
        let Operand::Symbol(i) = ins.operand else {
            unreachable!()
        };
        let id = self.symbol_id(i)?;
        let framed = self.external_index(id).is_some();
        let result = self.external(host, id, ins.offset + u32::from(ins.size()));
        if framed {
            result?;
        } else {
            self.guard_return(id, result)?;
        }
        Ok(if framed { Flow::Jump } else { Flow::Next })
    }
    fn guard_return<E>(&mut self, symbol: SymbolId, result: VmResult<(), E>) -> VmResult<(), E> {
        if let Err(e) = &result
            && self.terminal.is_none()
            && !e.fault.is_terminal()
        {
            let ty = self.result_type(self.script().get(symbol).map_err(Fault::from)?)?;
            if ty != DataType::Void {
                self.push(default_value(ty)?)?;
            }
        }
        result
    }
    fn exec_move<H: Host>(&mut self, host: &mut H, op: Opcode) -> VmResult<Flow, H::Error> {
        use Opcode::*;
        let target = self.pop_reference()?;
        if let StackReference::Temporary(_) = target {
            if op != Movs {
                return Err(fault(FaultKind::WrongType)
                    .types(
                        FaultOperation::ReferenceWrite,
                        ExpectedType::SymbolReference,
                        GivenType::Value(DataType::String),
                    )
                    .into());
            }
            self.pop_string(host)?;
            return Ok(Flow::Next);
        }
        let target = target.symbol()?;
        if op == Movvi {
            let value = self.pop_instance(host)?;
            self.set_global(target.symbol, 0, Value::Instance(value))?;
        } else {
            let value = match op {
                Movi | Movvf => Value::Int(self.pop_int(host)?),
                Movf => Value::Float(self.pop_float(host)?),
                Movs => Value::Bytes(self.pop_string(host)?),
                _ => unreachable!(),
            };
            self.write_reference(host, target, value)?;
        }
        Ok(Flow::Next)
    }
    fn exec_compound<H: Host>(&mut self, host: &mut H, op: Opcode) -> VmResult<Flow, H::Error> {
        use Opcode::*;
        let target = self.pop_reference()?.symbol()?;
        let b = self.pop_int(host)?;
        if op == Divmovi && b == 0 {
            return Err(fault(FaultKind::DivisionByZero).into());
        }
        let s = self.script().get(target.symbol).map_err(Fault::from)?;
        if s.is_const() && !self.policy.ignore_const {
            return Err(fault(FaultKind::Constant)
                .at(target.symbol)
                .with_detail(FaultDetail::Access {
                    operation: FaultOperation::ReferenceWrite,
                })
                .into());
        }
        if s.is_member() && target.context.is_none() && self.policy.allow_null_members {
            return Ok(Flow::Next);
        }
        let a = self
            .read_reference(host, target)?
            .as_int()
            .map_err(|e| e.extracted(Some(target.symbol)))?;
        let value = match op {
            Addmovi => a.wrapping_add(b),
            Submovi => a.wrapping_sub(b),
            Mulmovi => a.wrapping_mul(b),
            Divmovi => a
                .checked_div(b)
                .ok_or_else(|| fault(FaultKind::DivisionOverflow))?,
            _ => unreachable!(),
        };
        self.write_reference(host, target, Value::Int(value))?;
        Ok(Flow::Next)
    }
    fn exec_unary<H: Host>(&mut self, host: &mut H, op: Opcode) -> VmResult<Flow, H::Error> {
        use Opcode::*;
        let a = self.pop_int(host)?;
        let n = match op {
            Plus => a,
            Negate => a.wrapping_neg(),
            Not => i32::from(a == 0),
            Cmpl => !a,
            _ => unreachable!(),
        };
        self.push(Value::Int(n))?;
        Ok(Flow::Next)
    }
    fn exec_binary<H: Host>(&mut self, host: &mut H, op: Opcode) -> VmResult<Flow, H::Error> {
        use Opcode::*;
        let a = self.pop_int(host)?;
        let b = self.pop_int(host)?;
        let n = match op {
            Add => a.wrapping_add(b),
            Sub => a.wrapping_sub(b),
            Mul => a.wrapping_mul(b),
            Div | Mod => {
                if b == 0 {
                    return Err(fault(FaultKind::DivisionByZero).into());
                }
                if a == i32::MIN && b == -1 {
                    return Err(fault(FaultKind::DivisionOverflow).into());
                }
                if op == Div { a / b } else { a % b }
            }
            Or => a | b,
            Andb => a & b,
            Lt => i32::from(a < b),
            Gt => i32::from(a > b),
            Lte => i32::from(a <= b),
            Gte => i32::from(a >= b),
            Eq => i32::from(a == b),
            Neq => i32::from(a != b),
            Orr => i32::from(a != 0 || b != 0),
            And => i32::from(a != 0 && b != 0),
            Lsl | Lsr => {
                if !(0..32).contains(&b) {
                    return Err(fault(FaultKind::ShiftCount).into());
                }
                if op == Lsl {
                    a.wrapping_shl(b as u32)
                } else {
                    a >> b
                }
            }
            _ => unreachable!(),
        };
        self.push(Value::Int(n))?;
        Ok(Flow::Next)
    }
}
