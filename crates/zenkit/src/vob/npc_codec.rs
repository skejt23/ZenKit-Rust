// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::{codec::Fields, *};
use crate::{
    archive::{self, Value as V, ValueType as T},
    io, objects,
};

/// Only scalar counters/indices live in traversal frames. No pending link arrays.
#[derive(Clone, Copy, Debug)]
pub(crate) enum NpcPhase {
    Talents,
    Carry,
    Enemy,
    Item {
        remaining: usize,
        index: usize,
        needs_key: Option<bool>,
    },
    Slot {
        remaining: usize,
        index: usize,
    },
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum NpcEdge {
    Talent,
    Vob,
    Item,
}
impl NpcPhase {
    pub(crate) fn edge(self) -> NpcEdge {
        match self {
            Self::Talents => NpcEdge::Talent,
            Self::Carry | Self::Enemy => NpcEdge::Vob,
            _ => NpcEdge::Item,
        }
    }
}
fn reserve<T>(v: &mut Vec<T>, p: usize) -> Result<()> {
    v.try_reserve(1)
        .map_err(|_| Error::new(ErrorKind::LimitExceeded, p, "NPC sequence allocation"))
}
fn read_count(f: &mut Fields<'_, '_, '_>, name: &[u8], limit: usize) -> Result<usize> {
    let p = f.r.position();
    let n = usize::try_from(f.int(name)?)
        .map_err(|_| Error::new(ErrorKind::InvalidData, p, "negative NPC count"))?;
    if n > limit {
        return Err(Error::new(ErrorKind::LimitExceeded, p, "NPC count"));
    }
    Ok(n)
}
fn sequence(
    f: &mut Fields<'_, '_, '_>,
    name: &[u8],
    total: &mut usize,
    limit: usize,
) -> Result<usize> {
    let n = read_count(f, name, limit.saturating_sub(*total))?;
    *total += n;
    Ok(n)
}
fn raw_ints(f: &mut Fields<'_, '_, '_>, name: &[u8], out: &mut [i32]) -> Result<Vec<u8>> {
    let p = f.r.position();
    let size = out.len() * 4;
    let V::Raw(raw) = f.field(name, T::Raw(size))? else {
        unreachable!()
    };
    for (v, bytes) in out.iter_mut().zip(raw[..size].as_chunks::<4>().0) {
        *v = i32::from_le_bytes(*bytes);
    }
    objects::copy(&raw[size..], f.copied, f.limit, p)
}
fn read_state(f: &mut Fields<'_, '_, '_>, prefix: &str) -> Result<NpcAiState> {
    Ok(NpcAiState {
        valid: f.boolean(if prefix == "curState" {
            b"curState.valid"
        } else {
            b"nextState.valid"
        })?,
        name: f.string(if prefix == "curState" {
            b"curState.name"
        } else {
            b"nextState.name"
        })?,
        index: f.int(if prefix == "curState" {
            b"curState.prgIndex"
        } else {
            b"nextState.prgIndex"
        })?,
        routine: f.boolean(if prefix == "curState" {
            b"curState.isRtnState"
        } else {
            b"nextState.isRtnState"
        })?,
    })
}
pub(super) fn read(
    f: &mut Fields<'_, '_, '_>,
    n: &mut NpcData,
    phase: &mut Phase,
    sequences: &mut usize,
    limit: usize,
    links_left: usize,
) -> Result<usize> {
    if matches!(*phase, Phase::Base) {
        n.instance = f.string(b"npcInstance")?;
        n.model_scale = f.vec3(b"modelScale")?;
        n.fatness = f.float(b"modelFatness")?;
        let count = sequence(f, b"numOverlays", sequences, limit)?;
        for _ in 0..count {
            let v = f.string(b"overlay")?;
            reserve(&mut n.overlays, f.r.position())?;
            n.overlays.push(v);
        }
        n.flags = f.int(b"flags")?;
        n.guild = f.int(b"guild")?;
        n.guild_true = f.int(b"guildTrue")?;
        n.level = f.int(b"level")?;
        n.xp = f.int(b"xp")?;
        n.xp_next = f.int(b"xpnl")?;
        n.lp = f.int(b"lp")?;
        let count = read_count(f, b"numTalents", links_left)?;
        *phase = Phase::Npc(NpcPhase::Talents);
        if count != 0 {
            return Ok(count);
        }
    }
    if matches!(*phase, Phase::Npc(NpcPhase::Talents)) {
        n.fight_tactic = f.int(b"fightTactic")?;
        n.fight_mode = f.int(b"fightMode")?;
        n.wounded = f.boolean(b"wounded")?;
        n.mad = f.boolean(b"mad")?;
        n.mad_time = f.int(b"madTime")?;
        n.player = f.boolean(b"player")?;
        for (i, v) in n.attributes.iter_mut().enumerate() {
            *v = f.int(key(b"atr", i).as_bytes())?;
        }
        if let NpcVersion::Gothic2 { hit_chance, .. } = &mut n.version {
            // Original Gothic 2 saves number this one coherent group from 1;
            // output numbers it from 0. Other names remain strict.
            let first = usize::from(
                f.r.header().format != archive::Format::Binary && f.r.has_field_name(b"hc1")?,
            );
            for (i, v) in hit_chance.iter_mut().enumerate() {
                *v = f.int(key(b"hc", i + first).as_bytes())?;
            }
        }
        for (i, v) in n.missions.iter_mut().enumerate() {
            *v = f.int(key(b"mission", i).as_bytes())?;
        }
        n.start_ai = f.string(b"startAIState")?;
        n.script_vars_tail = raw_ints(f, b"scriptVars", n.version.script_vars_mut())?;
        n.waypoint = f.string(b"scriptWp")?;
        n.attitude = f.int(b"attitude")?;
        n.attitude_temp = f.int(b"tmpAttitude")?;
        n.name_nr = f.int(b"nameNr")?;
        let p = f.r.position();
        let V::Raw(raw) = f.field(b"spells", T::Raw(4))? else {
            unreachable!()
        };
        n.spells.copy_from_slice(&raw[..4]);
        n.spells_tail = objects::copy(&raw[4..], f.copied, f.limit, p)?;
        let count = sequence(f, b"NumOfEntries", sequences, limit)?;
        for _ in 0..count {
            let news = NpcNews {
                told: f.boolean(b"told")?,
                spread_time: f.float(b"spread_time")?,
                spread_type: f.int(b"spreadType")?,
                news_id: f.int(b"news_id")?,
                gossip: f.boolean(b"gossip")?,
                guild_victim: f.boolean(b"guildvictim")?,
                witness_name: f.string(b"witnessName")?,
                offender_name: f.string(b"offenderName")?,
                victim_name: f.string(b"victimName")?,
            };
            reserve(&mut n.news, f.r.position())?;
            n.news.push(news);
        }
        if links_left < 2 {
            return Err(Error::new(
                ErrorKind::LimitExceeded,
                f.r.position(),
                "NPC carry/enemy occurrences",
            ));
        }
        *phase = Phase::Npc(NpcPhase::Carry);
        return Ok(1);
    }
    if matches!(*phase, Phase::Npc(NpcPhase::Carry)) {
        *phase = Phase::Npc(NpcPhase::Enemy);
        return Ok(1);
    }
    if matches!(*phase, Phase::Npc(NpcPhase::Enemy)) {
        n.move_lock = f.boolean(b"moveLock")?;
        match &mut n.version {
            NpcVersion::Gothic1 { packed, .. } => {
                for v in packed {
                    *v = f.string(b"packed")?;
                }
            }
            NpcVersion::Gothic2 { packed, .. } => {
                *packed = NpcPacked::from_bytes(f.string(b"packed")?)
            }
        }
        let count = read_count(f, b"itemCount", links_left)?;
        if count != 0 {
            *phase = Phase::Npc(NpcPhase::Item {
                remaining: count,
                index: 0,
                needs_key: Some(false),
            });
            return Ok(1);
        }
        return start_slots(f, n, phase, sequences, limit, links_left);
    }
    match *phase {
        Phase::Npc(NpcPhase::Item {
            remaining,
            index,
            needs_key,
        }) => {
            let has_key = match needs_key {
                Some(v) => v,
                None => f.r.has_field_name(key(b"shortKey", index).as_bytes())?,
            };
            if has_key {
                n.items[index].short_key = Some(f.int(key(b"shortKey", index).as_bytes())?);
            }
            if remaining > 1 {
                *phase = Phase::Npc(NpcPhase::Item {
                    remaining: remaining - 1,
                    index: index + 1,
                    needs_key: Some(false),
                });
                return Ok(1);
            }
            start_slots(f, n, phase, sequences, limit, links_left)
        }
        Phase::Npc(NpcPhase::Slot { remaining, index }) => {
            n.slots[index]
                .entry
                .as_mut()
                .expect("completed slot occurrence")
                .in_inventory = f.boolean(b"inInv")?;
            read_slots(f, n, phase, remaining - 1, links_left)
        }
        _ => unreachable!("NPC definition phase"),
    }
}
fn start_slots(
    f: &mut Fields<'_, '_, '_>,
    n: &mut NpcData,
    phase: &mut Phase,
    sequences: &mut usize,
    limit: usize,
    links_left: usize,
) -> Result<usize> {
    let count = sequence(f, b"numInvSlots", sequences, limit)?;
    read_slots(f, n, phase, count, links_left)
}
fn read_slots(
    f: &mut Fields<'_, '_, '_>,
    n: &mut NpcData,
    phase: &mut Phase,
    mut remaining: usize,
    links_left: usize,
) -> Result<usize> {
    while remaining != 0 {
        let p = f.r.position();
        let used = f.boolean(b"used")?;
        let name = f.string(b"name")?;
        if used && links_left == 0 {
            return Err(Error::new(
                ErrorKind::LimitExceeded,
                p,
                "NPC used slot occurrence",
            ));
        }
        reserve(&mut n.slots, p)?;
        n.slots.push(NpcSlot { name, entry: None });
        if used {
            *phase = Phase::Npc(NpcPhase::Slot {
                remaining,
                index: n.slots.len() - 1,
            });
            return Ok(1);
        }
        remaining -= 1;
    }
    n.current_state = read_state(f, "curState")?;
    n.next_state = read_state(f, "nextState")?;
    n.last_ai_state = f.int(b"lastAIState")?;
    n.has_routine = f.boolean(b"hasRoutine")?;
    n.routine_changed = f.boolean(b"rtnChanged")?;
    n.routine_overlay = f.boolean(b"rtnOverlay")?;
    n.routine_overlay_count = f.int(b"rtnOverlayCount")?;
    n.walkmode_routine = f.int(b"walkmode_routine")?;
    n.weaponmode_routine = f.boolean(b"weaponmode_routine")?;
    n.start_new_routine = f.boolean(b"startNewRoutine")?;
    n.ai_state_driven = f.int(b"aiStateDriven")?;
    n.ai_state_pos = f.vec3(b"aiStatePos")?;
    n.current_routine = f.string(b"curRoutine")?;
    n.respawn = f.boolean(b"respawn")?;
    n.respawn_time = f.int(b"respawnTime")?;
    n.protection_tail = raw_ints(f, b"protection", &mut n.protection)?;
    if let NpcVersion::Gothic2 {
        interruptable_override,
        npc_type,
        spell_mana,
        ..
    } = &mut n.version
    {
        *interruptable_override = f.int(b"bsInterruptableOverride")?;
        *npc_type = f.int(b"npcType")?;
        *spell_mana = f.int(b"spellMana")?;
    }
    *phase = Phase::Done;
    Ok(0)
}
pub(crate) fn append(
    n: &mut NpcData,
    phase: &mut NpcPhase,
    link: Link,
    needs_key: Option<bool>,
    p: usize,
) -> Result<()> {
    match phase {
        NpcPhase::Talents => {
            reserve(&mut n.talents, p)?;
            n.talents.push(link);
        }
        NpcPhase::Carry => n.carry = link,
        NpcPhase::Enemy => n.enemy = link,
        NpcPhase::Item { needs_key: key, .. } => {
            reserve(&mut n.items, p)?;
            n.items.push(NpcItem {
                item: link,
                short_key: None,
            });
            *key = needs_key;
        }
        NpcPhase::Slot { index, .. } => {
            n.slots[*index].entry = Some(NpcSlotEntry {
                item: link,
                in_inventory: false,
            })
        }
    }
    Ok(())
}
pub(crate) fn phase_link(n: &NpcData, phase: NpcPhase, next: usize) -> &Link {
    match phase {
        NpcPhase::Talents => &n.talents[next],
        NpcPhase::Carry => &n.carry,
        NpcPhase::Enemy => &n.enemy,
        NpcPhase::Item { index, .. } => &n.items[index].item,
        NpcPhase::Slot { index, .. } => &n.slots[index].entry.as_ref().expect("used slot").item,
    }
}
fn string(w: &mut archive::Writer, key: &[u8], value: &[u8]) -> Result<()> {
    w.write(key, V::String(value.into()))
}
fn write_count(w: &mut archive::Writer, key: &[u8], n: usize) -> Result<()> {
    let n = i32::try_from(n).map_err(|_| bad("NPC count exceeds i32"))?;
    w.write(key, V::Int(n))
}
fn write_raw(
    w: &mut archive::Writer,
    limits: archive::Limits,
    key: &[u8],
    values: impl Iterator<Item = u8>,
    size: usize,
    tail: &[u8],
) -> Result<()> {
    if w.format() == archive::Format::Binary && !tail.is_empty() {
        return Err(Error::operation(
            ErrorKind::Unsupported,
            "NPC raw tail has no BINARY delimiter",
        ));
    }
    let size = size
        .checked_add(tail.len())
        .ok_or_else(|| Error::operation(ErrorKind::LimitExceeded, "NPC raw bytes"))?;
    super::trigger_codec::raw_size(w, limits, key, size)?;
    let mut raw = io::Writer::new(size);
    for byte in values {
        raw.u8(byte, "NPC raw")?;
    }
    raw.bytes(tail, "NPC raw tail")?;
    w.write(key, V::Raw(raw.into_inner().into()))
}
fn write_state(w: &mut archive::Writer, prefix: &str, s: &NpcAiState) -> Result<()> {
    w.write(
        if prefix == "curState" {
            b"curState.valid"
        } else {
            b"nextState.valid"
        },
        V::Bool(s.valid),
    )?;
    string(
        w,
        if prefix == "curState" {
            b"curState.name"
        } else {
            b"nextState.name"
        },
        &s.name,
    )?;
    w.write(
        if prefix == "curState" {
            b"curState.prgIndex"
        } else {
            b"nextState.prgIndex"
        },
        V::Int(s.index),
    )?;
    w.write(
        if prefix == "curState" {
            b"curState.isRtnState"
        } else {
            b"nextState.isRtnState"
        },
        V::Bool(s.routine),
    )
}
pub(super) fn write(
    w: &mut archive::Writer,
    n: &NpcData,
    phase: &mut Phase,
    limits: archive::Limits,
) -> Result<usize> {
    if matches!(*phase, Phase::Base) {
        string(w, b"npcInstance", &n.instance)?;
        w.write(b"modelScale", V::Vec3(n.model_scale))?;
        w.write(b"modelFatness", V::Float(n.fatness))?;
        write_count(w, b"numOverlays", n.overlays.len())?;
        for v in &n.overlays {
            string(w, b"overlay", v)?;
        }
        w.write(b"flags", V::Int(n.flags))?;
        w.write(b"guild", V::Int(n.guild))?;
        w.write(b"guildTrue", V::Int(n.guild_true))?;
        w.write(b"level", V::Int(n.level))?;
        w.write(b"xp", V::Int(n.xp))?;
        w.write(b"xpnl", V::Int(n.xp_next))?;
        w.write(b"lp", V::Int(n.lp))?;
        write_count(w, b"numTalents", n.talents.len())?;
        *phase = Phase::Npc(NpcPhase::Talents);
        if !n.talents.is_empty() {
            return Ok(n.talents.len());
        }
    }
    if matches!(*phase, Phase::Npc(NpcPhase::Talents)) {
        w.write(b"fightTactic", V::Int(n.fight_tactic))?;
        w.write(b"fightMode", V::Int(n.fight_mode))?;
        w.write(b"wounded", V::Bool(n.wounded))?;
        w.write(b"mad", V::Bool(n.mad))?;
        w.write(b"madTime", V::Int(n.mad_time))?;
        w.write(b"player", V::Bool(n.player))?;
        for (i, v) in n.attributes.iter().enumerate() {
            w.write(key(b"atr", i).as_bytes(), V::Int(*v))?;
        }
        if let NpcVersion::Gothic2 { hit_chance, .. } = &n.version {
            for (i, v) in hit_chance.iter().enumerate() {
                w.write(key(b"hc", i).as_bytes(), V::Int(*v))?;
            }
        }
        for (i, v) in n.missions.iter().enumerate() {
            w.write(key(b"mission", i).as_bytes(), V::Int(*v))?;
        }
        string(w, b"startAIState", &n.start_ai)?;
        let vars = n.version.script_vars();
        write_raw(
            w,
            limits,
            b"scriptVars",
            vars.iter().flat_map(|v| v.to_le_bytes()),
            vars.len() * 4,
            &n.script_vars_tail,
        )?;
        string(w, b"scriptWp", &n.waypoint)?;
        w.write(b"attitude", V::Int(n.attitude))?;
        w.write(b"tmpAttitude", V::Int(n.attitude_temp))?;
        w.write(b"nameNr", V::Int(n.name_nr))?;
        write_raw(
            w,
            limits,
            b"spells",
            n.spells.into_iter(),
            4,
            &n.spells_tail,
        )?;
        write_count(w, b"NumOfEntries", n.news.len())?;
        for v in &n.news {
            w.write(b"told", V::Bool(v.told))?;
            w.write(b"spread_time", V::Float(v.spread_time))?;
            w.write(b"spreadType", V::Int(v.spread_type))?;
            w.write(b"news_id", V::Int(v.news_id))?;
            w.write(b"gossip", V::Bool(v.gossip))?;
            w.write(b"guildvictim", V::Bool(v.guild_victim))?;
            string(w, b"witnessName", &v.witness_name)?;
            string(w, b"offenderName", &v.offender_name)?;
            string(w, b"victimName", &v.victim_name)?;
        }
        *phase = Phase::Npc(NpcPhase::Carry);
        return Ok(1);
    }
    if matches!(*phase, Phase::Npc(NpcPhase::Carry)) {
        *phase = Phase::Npc(NpcPhase::Enemy);
        return Ok(1);
    }
    if matches!(*phase, Phase::Npc(NpcPhase::Enemy)) {
        w.write(b"moveLock", V::Bool(n.move_lock))?;
        match &n.version {
            NpcVersion::Gothic1 { packed, .. } => {
                for v in packed {
                    string(w, b"packed", v)?;
                }
            }
            NpcVersion::Gothic2 { packed, .. } => string(w, b"packed", packed.as_bytes())?,
        }
        write_count(w, b"itemCount", n.items.len())?;
        if !n.items.is_empty() {
            *phase = Phase::Npc(NpcPhase::Item {
                remaining: n.items.len(),
                index: 0,
                needs_key: Some(false),
            });
            return Ok(1);
        }
        write_count(w, b"numInvSlots", n.slots.len())?;
        return write_slots(w, n, phase, 0, limits);
    }
    match *phase {
        Phase::Npc(NpcPhase::Item {
            remaining, index, ..
        }) => {
            if let Some(value) = n.items[index].short_key {
                w.write(key(b"shortKey", index).as_bytes(), V::Int(value))?;
            }
            if remaining > 1 {
                *phase = Phase::Npc(NpcPhase::Item {
                    remaining: remaining - 1,
                    index: index + 1,
                    needs_key: Some(false),
                });
                return Ok(1);
            }
            write_count(w, b"numInvSlots", n.slots.len())?;
            write_slots(w, n, phase, 0, limits)
        }
        Phase::Npc(NpcPhase::Slot { index, .. }) => {
            w.write(
                b"inInv",
                V::Bool(
                    n.slots[index]
                        .entry
                        .as_ref()
                        .expect("used slot")
                        .in_inventory,
                ),
            )?;
            write_slots(w, n, phase, index + 1, limits)
        }
        _ => unreachable!("NPC definition phase"),
    }
}
fn write_slots(
    w: &mut archive::Writer,
    n: &NpcData,
    phase: &mut Phase,
    start: usize,
    limits: archive::Limits,
) -> Result<usize> {
    for (index, s) in n.slots.iter().enumerate().skip(start) {
        w.write(b"used", V::Bool(s.entry.is_some()))?;
        string(w, b"name", &s.name)?;
        if s.entry.is_some() {
            *phase = Phase::Npc(NpcPhase::Slot {
                remaining: n.slots.len() - index,
                index,
            });
            return Ok(1);
        }
    }
    write_state(w, "curState", &n.current_state)?;
    write_state(w, "nextState", &n.next_state)?;
    w.write(b"lastAIState", V::Int(n.last_ai_state))?;
    w.write(b"hasRoutine", V::Bool(n.has_routine))?;
    w.write(b"rtnChanged", V::Bool(n.routine_changed))?;
    w.write(b"rtnOverlay", V::Bool(n.routine_overlay))?;
    w.write(b"rtnOverlayCount", V::Int(n.routine_overlay_count))?;
    w.write(b"walkmode_routine", V::Int(n.walkmode_routine))?;
    w.write(b"weaponmode_routine", V::Bool(n.weaponmode_routine))?;
    w.write(b"startNewRoutine", V::Bool(n.start_new_routine))?;
    w.write(b"aiStateDriven", V::Int(n.ai_state_driven))?;
    w.write(b"aiStatePos", V::Vec3(n.ai_state_pos))?;
    string(w, b"curRoutine", &n.current_routine)?;
    w.write(b"respawn", V::Bool(n.respawn))?;
    w.write(b"respawnTime", V::Int(n.respawn_time))?;
    write_raw(
        w,
        limits,
        b"protection",
        n.protection.iter().flat_map(|v| v.to_le_bytes()),
        32,
        &n.protection_tail,
    )?;
    if let NpcVersion::Gothic2 {
        interruptable_override,
        npc_type,
        spell_mana,
        ..
    } = &n.version
    {
        w.write(b"bsInterruptableOverride", V::Int(*interruptable_override))?;
        w.write(b"npcType", V::Int(*npc_type))?;
        w.write(b"spellMana", V::Int(*spell_mana))?;
    }
    *phase = Phase::Done;
    Ok(0)
}

struct Key {
    bytes: [u8; 32],
    len: usize,
}
impl Key {
    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}
fn key(prefix: &[u8], mut n: usize) -> Key {
    let mut out = Key {
        bytes: [0; 32],
        len: prefix.len(),
    };
    out.bytes[..prefix.len()].copy_from_slice(prefix);
    let mut digits = [0u8; 20];
    let mut start = digits.len();
    loop {
        start -= 1;
        digits[start] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    out.bytes[out.len..out.len + digits.len() - start].copy_from_slice(&digits[start..]);
    out.len += digits.len() - start;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn declared_talents_do_not_allocate_pending_links() {
        let limits = archive::Limits::default();
        let mut writer =
            archive::Writer::new(archive::Header::new(archive::Format::Binary), limits).unwrap();
        let mut source = NpcData::new(GameVersion::Gothic2);
        source.talents = vec![Link::new(Vec::new(), None); 100];
        assert_eq!(
            write(&mut writer, &source, &mut Phase::Base, limits).unwrap(),
            100
        );
        let bytes = writer.finish().unwrap();
        let mut reader = archive::Reader::new(&bytes, limits).unwrap();
        let mut copied = 0;
        let mut sequences = 0;
        let mut f = Fields {
            r: &mut reader,
            copied: &mut copied,
            limit: 0,
        };
        let mut target = NpcData::new(GameVersion::Gothic2);
        let mut phase = Phase::Base;
        assert_eq!(
            read(&mut f, &mut target, &mut phase, &mut sequences, 0, 100).unwrap(),
            100
        );
        assert_eq!((target.talents.len(), target.talents.capacity()), (0, 0));
        assert!(matches!(phase, Phase::Npc(NpcPhase::Talents)));
    }
    #[test]
    fn mutable_links_visit_every_occurrence_once_in_wire_order() {
        let mut v = Vob::new(GameVersion::Gothic2);
        let mut n = NpcData::new(GameVersion::Gothic2);
        n.talents.push(Link::new(Vec::new(), None));
        n.items.push(NpcItem {
            item: Link::new(Vec::new(), None),
            short_key: None,
        });
        n.slots.push(NpcSlot::default());
        n.slots.push(NpcSlot {
            name: Vec::new(),
            entry: Some(NpcSlotEntry {
                item: Link::new(Vec::new(), None),
                in_inventory: false,
            }),
        });
        v.kind = VobKind::Npc(n);
        for (i, l) in v.as_mut().links_mut().enumerate() {
            l.name = i.to_string().into_bytes();
        }
        assert_eq!(
            v.as_ref()
                .links()
                .map(|l| l.name.as_slice())
                .collect::<Vec<_>>(),
            [b"0", b"1", b"2", b"3", b"4"]
        );
    }
    #[test]
    fn stack_decimal_name_handles_the_signed_count_boundary() {
        assert_eq!(key(b"shortKey", 0).as_bytes(), b"shortKey0");
        assert_eq!(
            key(b"shortKey", i32::MAX as usize).as_bytes(),
            b"shortKey2147483647"
        );
    }
}
