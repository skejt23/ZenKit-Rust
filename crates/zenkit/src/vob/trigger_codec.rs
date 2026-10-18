// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::{codec::Fields, *};
use crate::{
    animation::Sample,
    archive::{self, Value as V, ValueType as T},
    io,
    objects::{self, ObjectMut, ObjectRef},
    values::Quat,
};

/// Definition-local continuation. References never execute another definition's
/// phase. Each phase consumes only scalar fields and yields a bounded sequence.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum Phase {
    #[default]
    Base,
    Trigger,
    Npc(super::npc_codec::NpcPhase),
    Camera {
        positions: usize,
    },
    Done,
}
pub(crate) struct ReadBudget<'a> {
    pub copied: &'a mut usize,
    pub sequences: &'a mut usize,
    pub limits: objects::Limits,
    pub links_left: usize,
}
fn sequence(total: &mut usize, n: usize, limit: usize, p: usize) -> Result<()> {
    *total = total
        .checked_add(n)
        .filter(|v| *v <= limit)
        .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, p, "object sequence items"))?;
    Ok(())
}
fn reserve<T>(v: &mut Vec<T>, n: usize, p: usize) -> Result<()> {
    v.try_reserve_exact(n)
        .map_err(|_| Error::new(ErrorKind::LimitExceeded, p, "trigger sequence allocation"))
}
fn raw_flag(f: &mut Fields<'_, '_, '_>, name: &[u8]) -> Result<(u8, Vec<u8>)> {
    let p = f.r.position();
    let V::Raw(v) = f.field(name, T::Raw(1))? else {
        unreachable!()
    };
    Ok((v[0], objects::copy(&v[1..], f.copied, f.limit, p)?))
}
fn trigger_start(
    f: &mut Fields<'_, '_, '_>,
    t: &mut Trigger,
    game: GameVersion,
    save: bool,
) -> Result<()> {
    t.target = f.string(b"triggerTarget")?;
    let (bits, tail) = raw_flag(f, b"flags")?;
    t.flags = TriggerFlags::from_raw(bits);
    t.flags_tail = tail;
    let (bits, tail) = raw_flag(f, b"filterFlags")?;
    t.filter = TriggerFilter::from_raw(bits);
    t.filter_tail = tail;
    t.vob_target = f.string(b"respondToVobName")?;
    t.max_activation_count = f.int(b"numCanBeActivated")?;
    t.retrigger_delay = f.float(b"retriggerWaitSec")?;
    t.damage_threshold = f.float(b"damageThreshold")?;
    t.fire_delay = f.float(b"fireDelaySec")?;
    if save {
        // Occurrence name is populated by graph parsing, and charged there.
        t.state = Some(TriggerState {
            next_time: f.float(b"nextTimeTriggerable")?,
            other: Link::default(),
            activation_count: 0,
            enabled: (game == GameVersion::Gothic2).then_some(false),
        });
    }
    Ok(())
}
pub(crate) fn read_phase(
    r: &mut archive::Reader<'_>,
    mut node: ObjectMut<'_>,
    phase: &mut Phase,
    b: ReadBudget<'_>,
) -> Result<usize> {
    let save = r.header().save_game;
    if matches!(*phase, Phase::Done) {
        return Ok(0);
    }
    if matches!(*phase, Phase::Base) {
        super::read_suffix(r, node.reborrow(), b.copied, b.limits.copied_bytes)?;
    }
    let ObjectMut::Vob(mut v) = node else {
        *phase = Phase::Done;
        return Ok(0);
    };
    let game = v.game();
    let mut f = Fields {
        r,
        copied: b.copied,
        limit: b.limits.copied_bytes,
    };
    if let VobKindMut::Npc(n) = &mut v.kind {
        return super::npc_codec::read(
            &mut f,
            n,
            phase,
            b.sequences,
            b.limits.sequence_items,
            b.links_left,
        );
    }
    if matches!(v.kind, VobKindMut::Camera(_) | VobKindMut::CameraFrame(_)) {
        return super::camera_codec::read(&mut f, &mut v.kind, phase, game, b.links_left);
    }
    if matches!(*phase, Phase::Base) {
        if v.kind.movable().is_some() {
            *phase = Phase::Done;
            return super::mob_codec::read(&mut f, &mut v.kind, b.links_left);
        }
        if let Some(t) = v.kind.trigger_mut() {
            trigger_start(&mut f, t, game, save)?;
            if save {
                *phase = Phase::Trigger;
                return Ok(1);
            }
        } else if let VobKindMut::CodeMaster(c) = &mut v.kind {
            c.target = f.string(b"triggerTarget")?;
            c.ordered = f.boolean(b"orderRelevant")?;
            c.first_false_is_failure = f.boolean(b"firstFalseIsFailure")?;
            c.failure_target = f.string(b"triggerTargetFailure")?;
            c.untrigger_cancels = f.boolean(b"untriggerCancels")?;
            let p = f.r.position();
            let n = usize::from(f.byte(b"numSlaves")?);
            sequence(b.sequences, n, b.limits.sequence_items, p)?;
            // Grow only after each field has been parsed; a tiny malformed input
            // never reserves its entire claimed sequence before its first field.
            for i in 0..n {
                let name = f.string(format!("slaveVobName{i}").as_bytes())?;
                reserve(&mut c.slaves, 1, p)?;
                c.slaves.push(name);
            }
            *phase = Phase::Done;
            if save && game == GameVersion::Gothic2 {
                let triggered_count = f.byte(b"numSlavesTriggered")?;
                if n > b.links_left {
                    return Err(Error::new(
                        ErrorKind::LimitExceeded,
                        p,
                        "code-master occurrence count",
                    ));
                }
                c.state = Some(CodeMasterState {
                    triggered_count,
                    // The frame retains the declared count. Append occurrences
                    // only as they complete, like Library, so nested definitions
                    // cannot multiply uncharged pending Link allocations.
                    triggered: Vec::new(),
                });
                return Ok(n);
            }
            return Ok(0);
        }
    } else if let Some(t) = v.kind.trigger_mut() {
        let s = t.state.as_mut().expect("save continuation");
        s.activation_count = f.int(b"countCanBeActivated")?;
        if game == GameVersion::Gothic2 {
            s.enabled = Some(f.boolean(b"isEnabled")?);
        }
    }
    derived_read(
        &mut f,
        &mut v.kind,
        game,
        save,
        b.sequences,
        b.limits.sequence_items,
    )?;
    *phase = Phase::Done;
    Ok(0)
}
fn derived_read(
    f: &mut Fields<'_, '_, '_>,
    kind: &mut VobKindMut<'_>,
    game: GameVersion,
    save: bool,
    sequences: &mut usize,
    limit: usize,
) -> Result<()> {
    match kind {
        VobKindMut::TriggerScript(v) => v.function = f.string(b"scriptFunc")?,
        VobKindMut::TriggerChangeLevel(v) => {
            v.level = f.string(b"levelName")?;
            v.start_vob = f.string(b"startVobName")?;
        }
        VobKindMut::TriggerWorldStart(v) => {
            v.target = f.string(b"triggerTarget")?;
            v.fire_once = f.boolean(b"fireOnlyFirstTime")?;
            v.has_fired = if save && game == GameVersion::Gothic2 {
                Some(f.boolean(b"hasFired")?)
            } else {
                None
            };
        }
        VobKindMut::TriggerUntouch(v) => v.target = f.string(b"triggerTarget")?,
        VobKindMut::TriggerList(v) => {
            v.mode = TriggerBatch::from_raw(f.enumeration(b"listProcess")?);
            let p = f.r.position();
            let entry = f.r.read(T::Byte)?;
            if entry
                .name
                .is_some_and(|n| n != b"numTarget" && n != b"numTargets")
            {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    p,
                    "trigger-list count field name",
                ));
            }
            let V::Byte(n) = entry.value else {
                unreachable!()
            };
            let n = usize::from(n);
            sequence(sequences, n, limit, p)?;
            for i in 0..n {
                let name = f.string(format!("triggerTarget{i}").as_bytes())?;
                let delay = f.float(format!("fireDelay{i}").as_bytes())?;
                reserve(&mut v.targets, 1, p)?;
                v.targets.push(TriggerTarget { name, delay });
            }
            if save {
                v.state = Some(TriggerListState {
                    active_target: f.byte(b"actTarget")?,
                    send_on_trigger: f.boolean(b"sendOnTrigger")?,
                });
            }
        }
        VobKindMut::Mover(v) => {
            v.behavior = MoverBehavior::from_raw(f.enumeration(b"moverBehavior")?);
            v.touch_damage = f.float(b"touchBlockerDamage")?;
            v.stay_open = f.float(b"stayOpenTimeSec")?;
            v.locked = f.boolean(b"moverLocked")?;
            v.auto_link = f.boolean(b"autoLinkEnabled")?;
            if game == GameVersion::Gothic2 {
                v.auto_rotate = Some(f.boolean(b"autoRotate")?);
            }
            let p = f.r.position();
            let V::Word(n) = f.field(b"numKeyframes", T::Word)? else {
                unreachable!()
            };
            let n = usize::from(n);
            sequence(sequences, n, limit, p)?;
            if n != 0 {
                let speed = f.float(b"moveSpeed")?;
                let lerp = MoverLerp::from_raw(f.enumeration(b"posLerpType")?);
                let speed_mode = MoverSpeed::from_raw(f.enumeration(b"speedType")?);
                let p = f.r.position();
                let size = n
                    .checked_mul(28)
                    .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, p, "mover sample bytes"))?;
                let V::Raw(raw) = f.field(b"keyframes", T::Raw(size))? else {
                    unreachable!()
                };
                let tail = objects::copy(&raw[size..], f.copied, f.limit, p)?;
                let mut samples = Vec::new();
                reserve(&mut samples, n, p)?;
                for bytes in raw[..size].as_chunks::<28>().0 {
                    let mut values = [0.; 7];
                    for (out, chunk) in values.iter_mut().zip(bytes.as_chunks::<4>().0) {
                        *out = f32::from_le_bytes(*chunk);
                    }
                    samples.push(Sample {
                        position: Vec3::from([values[0], values[1], values[2]]),
                        rotation: Quat::from_xyzw([values[3], values[4], values[5], values[6]]),
                    });
                }
                v.motion = Some(MoverMotion {
                    speed,
                    lerp,
                    speed_mode,
                    samples,
                    tail,
                });
            }
            if save {
                v.state = Some(MoverState {
                    key_position_delta: f.vec3(b"actKeyPosDelta")?,
                    keyframe_fraction: f.float(b"actKeyframeF")?,
                    keyframe: f.int(b"actKeyframe")?,
                    next_keyframe: f.int(b"nextKeyframe")?,
                    speed_unit: f.float(b"moveSpeedUnit")?,
                    advance_direction: f.float(b"advanceDir")?,
                    state: f.enumeration(b"moverState")?,
                    trigger_events: f.int(b"numTriggerEvents")?,
                    stay_open_until: f.float(b"stayOpenTimeDest")?,
                });
            }
            v.sounds = MoverSounds {
                open_start: f.string(b"sfxOpenStart")?,
                open_end: f.string(b"sfxOpenEnd")?,
                moving: f.string(b"sfxMoving")?,
                close_start: f.string(b"sfxCloseStart")?,
                close_end: f.string(b"sfxCloseEnd")?,
                lock: f.string(b"sfxLock")?,
                unlock: f.string(b"sfxUnlock")?,
                use_locked: f.string(b"sfxUseLocked")?,
            };
        }
        _ => {}
    }
    Ok(())
}
pub(super) fn raw_size(
    w: &archive::Writer,
    limits: archive::Limits,
    name: &[u8],
    size: usize,
) -> Result<()> {
    let fits = match w.format() {
        archive::Format::Ascii => size
            .checked_mul(2)
            .and_then(|n| n.checked_add(name.len() + 5))
            .is_some_and(|n| n <= limits.field_bytes),
        archive::Format::BinSafe => size <= limits.field_bytes.min(u16::MAX as usize),
        archive::Format::Binary => size <= limits.field_bytes,
    };
    if !fits || size > limits.bytes {
        return Err(Error::operation(
            ErrorKind::LimitExceeded,
            "VOB raw field bytes",
        ));
    }
    Ok(())
}
fn write_flag(
    w: &mut archive::Writer,
    limits: archive::Limits,
    name: &[u8],
    bits: u8,
    tail: &[u8],
) -> Result<()> {
    if w.format() == archive::Format::Binary && !tail.is_empty() {
        return Err(Error::operation(
            ErrorKind::Unsupported,
            "trigger flag tail has no BINARY delimiter",
        ));
    }
    let size = tail
        .len()
        .checked_add(1)
        .ok_or_else(|| Error::operation(ErrorKind::LimitExceeded, "trigger flag bytes"))?;
    raw_size(w, limits, name, size)?;
    let mut raw = io::Writer::new(size);
    raw.u8(bits, "trigger flags")?;
    raw.bytes(tail, "trigger flags")?;
    w.write(name, V::Raw(raw.into_inner().into()))
}
fn string(w: &mut archive::Writer, name: &[u8], value: &[u8]) -> Result<()> {
    w.write(name, V::String(value.into()))
}
pub(crate) fn write_phase(
    w: &mut archive::Writer,
    node: ObjectRef<'_>,
    phase: &mut Phase,
    limits: archive::Limits,
) -> Result<usize> {
    if matches!(*phase, Phase::Done) {
        return Ok(0);
    }
    if matches!(*phase, Phase::Base) {
        super::write_suffix(w, node)?;
    }
    let ObjectRef::Vob(v) = node else {
        *phase = Phase::Done;
        return Ok(0);
    };
    if let VobKindRef::Npc(n) = &v.kind {
        return super::npc_codec::write(w, n, phase, limits);
    }
    if matches!(v.kind, VobKindRef::Camera(_) | VobKindRef::CameraFrame(_)) {
        return super::camera_codec::write(w, v.kind, phase, limits);
    }
    if matches!(*phase, Phase::Base) {
        if v.kind.movable().is_some() {
            *phase = Phase::Done;
            return super::mob_codec::write(w, v.kind);
        }
        if let Some(t) = v.kind.trigger() {
            string(w, b"triggerTarget", &t.target)?;
            write_flag(w, limits, b"flags", t.flags.raw(), &t.flags_tail)?;
            write_flag(w, limits, b"filterFlags", t.filter.raw(), &t.filter_tail)?;
            string(w, b"respondToVobName", &t.vob_target)?;
            w.write(b"numCanBeActivated", V::Int(t.max_activation_count))?;
            w.write(b"retriggerWaitSec", V::Float(t.retrigger_delay))?;
            w.write(b"damageThreshold", V::Float(t.damage_threshold))?;
            w.write(b"fireDelaySec", V::Float(t.fire_delay))?;
            if let Some(s) = &t.state {
                w.write(b"nextTimeTriggerable", V::Float(s.next_time))?;
                *phase = Phase::Trigger;
                return Ok(1);
            }
        } else if let VobKindRef::CodeMaster(c) = &v.kind {
            string(w, b"triggerTarget", &c.target)?;
            w.write(b"orderRelevant", V::Bool(c.ordered))?;
            w.write(b"firstFalseIsFailure", V::Bool(c.first_false_is_failure))?;
            string(w, b"triggerTargetFailure", &c.failure_target)?;
            w.write(b"untriggerCancels", V::Bool(c.untrigger_cancels))?;
            w.write(b"numSlaves", V::Byte(c.slaves.len() as u8))?;
            for (i, name) in c.slaves.iter().enumerate() {
                string(w, format!("slaveVobName{i}").as_bytes(), name)?;
            }
            *phase = Phase::Done;
            if let Some(s) = &c.state {
                w.write(b"numSlavesTriggered", V::Byte(s.triggered_count))?;
                return Ok(s.triggered.len());
            }
            return Ok(0);
        }
    } else if let Some(t) = v.kind.trigger() {
        let s = t.state.as_ref().expect("validated save state");
        w.write(b"countCanBeActivated", V::Int(s.activation_count))?;
        if let Some(enabled) = s.enabled {
            w.write(b"isEnabled", V::Bool(enabled))?;
        }
    }
    derived_write(w, v.kind, limits)?;
    *phase = Phase::Done;
    Ok(0)
}
fn derived_write(
    w: &mut archive::Writer,
    kind: VobKindRef<'_>,
    limits: archive::Limits,
) -> Result<()> {
    match kind {
        VobKindRef::TriggerScript(v) => string(w, b"scriptFunc", &v.function)?,
        VobKindRef::TriggerChangeLevel(v) => {
            string(w, b"levelName", &v.level)?;
            string(w, b"startVobName", &v.start_vob)?;
        }
        VobKindRef::TriggerWorldStart(v) => {
            string(w, b"triggerTarget", &v.target)?;
            w.write(b"fireOnlyFirstTime", V::Bool(v.fire_once))?;
            if let Some(fired) = v.has_fired {
                w.write(b"hasFired", V::Bool(fired))?;
            }
        }
        VobKindRef::TriggerUntouch(v) => string(w, b"triggerTarget", &v.target)?,
        VobKindRef::TriggerList(v) => {
            w.write(b"listProcess", V::Enum(v.mode.raw()))?;
            w.write(b"numTarget", V::Byte(v.targets.len() as u8))?;
            for (i, t) in v.targets.iter().enumerate() {
                string(w, format!("triggerTarget{i}").as_bytes(), &t.name)?;
                w.write(format!("fireDelay{i}").as_bytes(), V::Float(t.delay))?;
            }
            if let Some(s) = v.state {
                w.write(b"actTarget", V::Byte(s.active_target))?;
                w.write(b"sendOnTrigger", V::Bool(s.send_on_trigger))?;
            }
        }
        VobKindRef::Mover(v) => {
            w.write(b"moverBehavior", V::Enum(v.behavior.raw()))?;
            w.write(b"touchBlockerDamage", V::Float(v.touch_damage))?;
            w.write(b"stayOpenTimeSec", V::Float(v.stay_open))?;
            w.write(b"moverLocked", V::Bool(v.locked))?;
            w.write(b"autoLinkEnabled", V::Bool(v.auto_link))?;
            if let Some(rotate) = v.auto_rotate {
                w.write(b"autoRotate", V::Bool(rotate))?;
            }
            w.write(
                b"numKeyframes",
                V::Word(v.motion.as_ref().map_or(0, |m| m.samples.len() as u16)),
            )?;
            if let Some(m) = &v.motion {
                w.write(b"moveSpeed", V::Float(m.speed))?;
                w.write(b"posLerpType", V::Enum(m.lerp.raw()))?;
                w.write(b"speedType", V::Enum(m.speed_mode.raw()))?;
                if w.format() == archive::Format::Binary && !m.tail.is_empty() {
                    return Err(Error::operation(
                        ErrorKind::Unsupported,
                        "mover tail has no BINARY delimiter",
                    ));
                }
                let size = m
                    .samples
                    .len()
                    .checked_mul(28)
                    .and_then(|n| n.checked_add(m.tail.len()))
                    .ok_or_else(|| {
                        Error::operation(ErrorKind::LimitExceeded, "mover sample bytes")
                    })?;
                raw_size(w, limits, b"keyframes", size)?;
                let mut raw = io::Writer::new(size);
                for s in &m.samples {
                    s.position.write(&mut raw, "mover position")?;
                    for value in s.rotation.to_xyzw() {
                        raw.f32(value, "mover rotation")?;
                    }
                }
                raw.bytes(&m.tail, "mover tail")?;
                w.write(b"keyframes", V::Raw(raw.into_inner().into()))?;
            }
            if let Some(s) = v.state {
                w.write(b"actKeyPosDelta", V::Vec3(s.key_position_delta))?;
                w.write(b"actKeyframeF", V::Float(s.keyframe_fraction))?;
                w.write(b"actKeyframe", V::Int(s.keyframe))?;
                w.write(b"nextKeyframe", V::Int(s.next_keyframe))?;
                w.write(b"moveSpeedUnit", V::Float(s.speed_unit))?;
                w.write(b"advanceDir", V::Float(s.advance_direction))?;
                w.write(b"moverState", V::Enum(s.state))?;
                w.write(b"numTriggerEvents", V::Int(s.trigger_events))?;
                w.write(b"stayOpenTimeDest", V::Float(s.stay_open_until))?;
            }
            for (name, value) in [
                b"sfxOpenStart".as_slice(),
                b"sfxOpenEnd",
                b"sfxMoving",
                b"sfxCloseStart",
                b"sfxCloseEnd",
                b"sfxLock",
                b"sfxUnlock",
                b"sfxUseLocked",
            ]
            .into_iter()
            .zip(v.sounds.buffers())
            {
                string(w, name, value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objects::Object;

    #[test]
    fn code_master_phase_yields_count_without_allocating_pending_links() {
        let game = GameVersion::Gothic2;
        let mut source = Vob::new(game);
        source.base.physics_enabled = false;
        source.base.state = Some(SaveState::default());
        source.kind = VobKind::CodeMaster(CodeMaster {
            slaves: vec![Vec::new(); 100],
            state: Some(CodeMasterState {
                triggered_count: 99,
                triggered: vec![Link::default(); 100],
            }),
            ..Default::default()
        });
        let mut header = archive::Header::new(archive::Format::Binary);
        header.save_game = true;
        let limits = objects::Limits::default();
        let mut writer = archive::Writer::new(header, limits.archive).unwrap();
        assert_eq!(
            write_phase(
                &mut writer,
                Object::Vob(source).as_ref(),
                &mut Phase::Base,
                limits.archive
            )
            .unwrap(),
            100
        );
        let bytes = writer.finish().unwrap();
        let mut reader = archive::Reader::new(&bytes, limits.archive).unwrap();
        let mut vob = Vob::new(game);
        vob.base.physics_enabled = false;
        vob.kind = VobKind::CodeMaster(CodeMaster::default());
        let mut node = Object::Vob(vob);
        let mut phase = Phase::Base;
        let (mut copied, mut sequences) = (0, 0);
        let count = read_phase(
            &mut reader,
            node.as_mut(),
            &mut phase,
            ReadBudget {
                copied: &mut copied,
                sequences: &mut sequences,
                limits,
                links_left: 100,
            },
        )
        .unwrap();
        assert_eq!(count, 100);
        assert_eq!(sequences, 100);
        let Object::Vob(vob) = node else {
            unreachable!()
        };
        let VobKind::CodeMaster(master) = vob.kind else {
            unreachable!()
        };
        let state = master.state.unwrap();
        assert_eq!(state.triggered_count, 99);
        assert_eq!(state.triggered.len(), 0);
        assert_eq!(state.triggered.capacity(), 0);
    }
}
