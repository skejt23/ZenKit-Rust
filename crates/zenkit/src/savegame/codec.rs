// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2022-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;
use crate::archive::{self, Value as V, ValueType as T};

fn at(kind: ErrorKind, p: usize) -> Error {
    Error::new(kind, p, CTX)
}
fn count(total: &mut usize, n: usize, max: usize) -> Result<()> {
    *total = total
        .checked_add(n)
        .filter(|n| *n <= max)
        .ok_or_else(|| bad(ErrorKind::LimitExceeded))?;
    Ok(())
}
fn signed(n: usize) -> Result<i32> {
    i32::try_from(n).map_err(|_| bad(ErrorKind::LimitExceeded))
}
struct Fields<'r, 'a> {
    r: &'r mut archive::Reader<'a>,
    copied: &'r mut usize,
    sequences: &'r mut usize,
    limits: objects::Limits,
}
impl<'a> Fields<'_, 'a> {
    fn field(&mut self, name: &[u8], kind: T) -> Result<V<'a>> {
        let p = self.r.position();
        let entry = self.r.read(kind)?;
        if entry.name.is_some_and(|n| n != name) {
            return Err(at(ErrorKind::InvalidData, p));
        }
        Ok(entry.value)
    }
    fn int(&mut self, name: &[u8]) -> Result<i32> {
        let V::Int(v) = self.field(name, T::Int)? else {
            unreachable!()
        };
        Ok(v)
    }
    fn boolean(&mut self, name: &[u8]) -> Result<bool> {
        let V::Bool(v) = self.field(name, T::Bool)? else {
            unreachable!()
        };
        Ok(v)
    }
    fn enumeration(&mut self, name: &[u8]) -> Result<u32> {
        let V::Enum(v) = self.field(name, T::Enum)? else {
            unreachable!()
        };
        Ok(v)
    }
    fn float(&mut self, name: &[u8]) -> Result<f32> {
        let V::Float(v) = self.field(name, T::Float)? else {
            unreachable!()
        };
        Ok(v)
    }
    fn string(&mut self, name: &[u8]) -> Result<Vec<u8>> {
        let p = self.r.position();
        let V::String(v) = self.field(name, T::String)? else {
            unreachable!()
        };
        objects::copy(&v, self.copied, self.limits.copied_bytes, p)
    }
    fn array<Ty>(&mut self, name: &[u8], minimum: usize) -> Result<(usize, Vec<Ty>)> {
        let p = self.r.position();
        let n = usize::try_from(self.int(name)?).map_err(|_| at(ErrorKind::InvalidData, p))?;
        count(self.sequences, n, self.limits.sequence_items).map_err(|e| at(e.kind, p))?;
        if n > self.r.remaining_bytes() / minimum {
            return Err(at(ErrorKind::UnexpectedEof, p));
        }
        let mut v = Vec::new();
        v.try_reserve_exact(n)
            .map_err(|_| at(ErrorKind::LimitExceeded, p))?;
        Ok((n, v))
    }
}
// Fixed stack storage covers both usize indices plus all source prefixes.
struct Name {
    bytes: [u8; 64],
    len: usize,
}
impl Name {
    fn new(prefix: &[u8], first: usize, second: Option<usize>, suffix: &[u8]) -> Self {
        let mut n = Self {
            bytes: [0; 64],
            len: 0,
        };
        n.append(prefix);
        n.number(first);
        if let Some(v) = second {
            n.append(b"_");
            n.number(v);
        }
        n.append(suffix);
        n
    }
    fn append(&mut self, v: &[u8]) {
        self.bytes[self.len..self.len + v.len()].copy_from_slice(v);
        self.len += v.len();
    }
    fn number(&mut self, mut value: usize) {
        let mut digits = [0; 20];
        let mut i = 20;
        loop {
            i -= 1;
            digits[i] = b'0' + (value % 10) as u8;
            value /= 10;
            if value == 0 {
                break;
            }
        }
        self.append(&digits[i..]);
    }
    fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}
pub(crate) fn read_metadata(
    r: &mut archive::Reader<'_>,
    game: GameVersion,
    copied: &mut usize,
    max: usize,
) -> Result<SaveMetadata> {
    let mut sequences = 0;
    let mut f = Fields {
        r,
        copied,
        sequences: &mut sequences,
        limits: objects::Limits {
            copied_bytes: max,
            ..Default::default()
        },
    };
    Ok(SaveMetadata {
        title: f.string(b"Title")?,
        world: f.string(b"WorldName")?,
        day: f.int(b"TimeDay")?,
        hour: f.int(b"TimeHour")?,
        minute: f.int(b"TimeMin")?,
        save_date: f.string(b"SaveDate")?,
        version_major: f.int(b"VersionMajor")?,
        version_minor: f.int(b"VersionMinor")?,
        play_time_seconds: f.int(b"PlayTimeSeconds")?,
        version: match game {
            GameVersion::Gothic1 => MetadataVersion::Gothic1,
            GameVersion::Gothic2 => MetadataVersion::Gothic2(Gothic2Metadata {
                version_point: f.int(b"VersionPoint")?,
                version_int: f.int(b"VersionInt")?,
                application: f.string(b"VersionAppName")?,
            }),
        },
    })
}
fn string(w: &mut archive::Writer, n: &[u8], v: &[u8]) -> Result<()> {
    w.write(n, V::String(v.into()))
}
pub(crate) fn write_metadata(v: &SaveMetadata, w: &mut archive::Writer) -> Result<()> {
    string(w, b"Title", &v.title)?;
    string(w, b"WorldName", &v.world)?;
    for (n, v) in [
        (b"TimeDay".as_slice(), v.day),
        (b"TimeHour", v.hour),
        (b"TimeMin", v.minute),
    ] {
        w.write(n, V::Int(v))?;
    }
    string(w, b"SaveDate", &v.save_date)?;
    for (n, v) in [
        (b"VersionMajor".as_slice(), v.version_major),
        (b"VersionMinor", v.version_minor),
        (b"PlayTimeSeconds", v.play_time_seconds),
    ] {
        w.write(n, V::Int(v))?;
    }
    if let MetadataVersion::Gothic2(g) = &v.version {
        w.write(b"VersionPoint", V::Int(g.version_point))?;
        w.write(b"VersionInt", V::Int(g.version_int))?;
        string(w, b"VersionAppName", &g.application)?;
    }
    Ok(())
}
fn prefix(f: &mut Fields<'_, '_>, s: &mut StateData) -> Result<()> {
    s.day = f.int(b"day")?;
    s.hour = f.int(b"hour")?;
    s.minute = f.int(b"min")?;
    let (entries, values) = f.array(b"NumOfEntries", 2)?;
    s.infos = values;
    for _ in 0..entries {
        s.infos.push(Info {
            told: f.boolean(b"Told")?,
            name: f.string(b"InstName")?,
        });
    }
    let (entries, values) = f.array(b"NumOfEntries", 14)?;
    s.missions = values;
    for _ in 0..entries {
        let name = f.string(b"name")?;
        let id = f.int(b"ID")?;
        let available = f.boolean(b"Av")?;
        let (entries, mut history) = f.array(b"NumInList", 12)?;
        for _ in 0..entries {
            history.push(MissionHistory {
                vob_id: f.int(b"VobIDL")?,
                status: f.int(b"StatusL")?,
                start_time: f.float(b"startTimeL")?,
            });
        }
        s.missions.push(Mission {
            name,
            id,
            available,
            history,
            status_index: f.int(b"StatusIndex")?,
        });
    }
    let (entries, values) = f.array(b"LOGMANAGERTOPICCOUNT", 11)?;
    s.log = values;
    for _ in 0..entries {
        let description = f.string(b"TOPICDESCRIPTION")?;
        let section = f.enumeration(b"TOPICSECTION")?.into();
        let status = f.enumeration(b"TOPICSTATUS")?.into();
        let (entry_count, mut entries) = f.array(b"LOGTOPICENTRYCOUNT", 1)?;
        let manager_entry_count = f.int(b"LOGMANAGERENTRYCOUNT")?;
        for _ in 0..entry_count {
            entries.push(f.string(b"ENTRYDESCRIPTION")?);
        }
        s.log.push(LogTopic {
            description,
            section,
            status,
            entries,
            manager_entry_count,
        });
    }
    Ok(())
}
fn suffix(f: &mut Fields<'_, '_>, s: &mut StateData) -> Result<()> {
    match &mut s.symbols {
        Symbols::Gothic1(symbols) => {
            let (entries, values) = f.array(b"numSymbols", 5)?;
            *symbols = values;
            for i in 0..entries {
                symbols.push(Symbol {
                    name: f.string(Name::new(b"symName", i, None, b"").bytes())?,
                    value: f.int(Name::new(b"symValue", i, None, b"").bytes())?,
                });
            }
        }
        Symbols::Gothic2(symbols) => {
            let (entries, values) = f.array(b"numSymbols", 5)?;
            *symbols = values;
            for i in 0..entries {
                let name = f.string(Name::new(b"symName", i, None, b"").bytes())?;
                let (entries, mut values) =
                    f.array(Name::new(b"symName", i, None, b"cnt").bytes(), 4)?;
                let mut names = ValueNames::Indexed;
                for j in 0..entries {
                    if j == 1
                        && f.r.header().format != Format::Binary
                        && f.r
                            .has_field_name(Name::new(b"symValue", i, Some(0), b"").bytes())?
                    {
                        names = ValueNames::RepeatedZero;
                    }
                    let index = if names == ValueNames::Indexed { j } else { 0 };
                    values.push(f.int(Name::new(b"symValue", i, Some(index), b"").bytes())?);
                }
                symbols.push(Symbol {
                    name,
                    value: SymbolValues { values, names },
                });
            }
        }
    }
    let p = f.r.position();
    let V::Raw(bytes) = f.field(b"guildTable", T::Raw(1764))? else {
        unreachable!()
    };
    count(f.copied, 1764, f.limits.copied_bytes).map_err(|e| at(e.kind, p))?;
    for (row, bytes) in s
        .guild_attitudes
        .iter_mut()
        .zip(bytes[..1764].as_chunks::<42>().0)
    {
        row.copy_from_slice(bytes);
    }
    s.guild_tail = objects::copy(&bytes[1764..], f.copied, f.limits.copied_bytes, p)?;
    Ok(())
}
pub(crate) fn read(
    bytes: &[u8],
    game: GameVersion,
    limits: objects::Limits,
) -> Result<StateArchive> {
    let (metadata, mut p) = objects::Parser::new(bytes, game, limits)?;
    let mut fields = StateData::new(game);
    prefix(
        &mut Fields {
            r: &mut p.reader,
            copied: &mut p.copied,
            sequences: &mut p.sequences,
            limits,
        },
        &mut fields,
    )?;
    let at = p.reader.position();
    if !p.read_root()? {
        return Err(Error::new(ErrorKind::InvalidData, at, CTX));
    }
    if let Some(id) = p.graph().roots()[0].target
        && !matches!(p.graph().get(id)?, ObjectRef::CutsceneManager(_))
    {
        return Err(Error::new(ErrorKind::WrongObjectType, at, CTX));
    }
    suffix(
        &mut Fields {
            r: &mut p.reader,
            copied: &mut p.copied,
            sequences: &mut p.sequences,
            limits,
        },
        &mut fields,
    )?;
    let graph = p.finish()?;
    let result = StateArchive {
        metadata,
        game,
        fields,
        graph,
    };
    result.manager()?;
    Ok(result)
}
pub(crate) fn validate(
    s: &StateData,
    game: GameVersion,
    limits: objects::Limits,
) -> Result<(usize, usize)> {
    let mut copied = 1764usize;
    let mut seq = 0;
    let mut bytes = |v: &[u8]| count(&mut copied, v.len(), limits.copied_bytes);
    let mut slots = |n: usize| {
        signed(n)?;
        count(&mut seq, n, limits.sequence_items)
    };
    bytes(&s.guild_tail)?;
    slots(s.infos.len())?;
    slots(s.missions.len())?;
    slots(s.log.len())?;
    for v in &s.infos {
        bytes(&v.name)?;
    }
    for v in &s.missions {
        bytes(&v.name)?;
        slots(v.history.len())?;
    }
    for v in &s.log {
        bytes(&v.description)?;
        slots(v.entries.len())?;
        for e in &v.entries {
            bytes(e)?;
        }
    }
    match (&s.symbols, game) {
        (Symbols::Gothic1(v), GameVersion::Gothic1) => {
            slots(v.len())?;
            for x in v {
                bytes(&x.name)?;
            }
        }
        (Symbols::Gothic2(v), GameVersion::Gothic2) => {
            slots(v.len())?;
            for x in v {
                bytes(&x.name)?;
                slots(x.value.values.len())?;
            }
        }
        _ => return Err(bad(ErrorKind::InvalidData)),
    }
    Ok((copied, seq))
}
fn n(w: &mut archive::Writer, name: &[u8], count: usize) -> Result<()> {
    w.write(name, V::Int(signed(count)?))
}
pub(crate) fn write(s: &StateArchive, limits: objects::Limits) -> Result<Vec<u8>> {
    let mut e = objects::Encoder::new(&s.metadata, s.game, &s.graph, limits)?;
    let f = &s.fields;
    let w = &mut e.output;
    for (name, value) in [
        (b"day".as_slice(), f.day),
        (b"hour", f.hour),
        (b"min", f.minute),
    ] {
        w.write(name, V::Int(value))?;
    }
    n(w, b"NumOfEntries", f.infos.len())?;
    for v in &f.infos {
        w.write(b"Told", V::Bool(v.told))?;
        string(w, b"InstName", &v.name)?;
    }
    n(w, b"NumOfEntries", f.missions.len())?;
    for v in &f.missions {
        string(w, b"name", &v.name)?;
        w.write(b"ID", V::Int(v.id))?;
        w.write(b"Av", V::Bool(v.available))?;
        n(w, b"NumInList", v.history.len())?;
        for h in &v.history {
            w.write(b"VobIDL", V::Int(h.vob_id))?;
            w.write(b"StatusL", V::Int(h.status))?;
            w.write(b"startTimeL", V::Float(h.start_time))?;
        }
        w.write(b"StatusIndex", V::Int(v.status_index))?;
    }
    n(w, b"LOGMANAGERTOPICCOUNT", f.log.len())?;
    for v in &f.log {
        string(w, b"TOPICDESCRIPTION", &v.description)?;
        w.write(b"TOPICSECTION", V::Enum(v.section.raw()))?;
        w.write(b"TOPICSTATUS", V::Enum(v.status.raw()))?;
        n(w, b"LOGTOPICENTRYCOUNT", v.entries.len())?;
        w.write(b"LOGMANAGERENTRYCOUNT", V::Int(v.manager_entry_count))?;
        for x in &v.entries {
            string(w, b"ENTRYDESCRIPTION", x)?;
        }
    }
    e.link(&s.graph.roots()[0])?;
    let w = &mut e.output;
    match &f.symbols {
        Symbols::Gothic1(v) => {
            n(w, b"numSymbols", v.len())?;
            for (i, v) in v.iter().enumerate() {
                string(w, Name::new(b"symName", i, None, b"").bytes(), &v.name)?;
                w.write(
                    Name::new(b"symValue", i, None, b"").bytes(),
                    V::Int(v.value),
                )?;
            }
        }
        Symbols::Gothic2(v) => {
            n(w, b"numSymbols", v.len())?;
            for (i, v) in v.iter().enumerate() {
                string(w, Name::new(b"symName", i, None, b"").bytes(), &v.name)?;
                n(
                    w,
                    Name::new(b"symName", i, None, b"cnt").bytes(),
                    v.value.values.len(),
                )?;
                for (j, x) in v.value.values.iter().enumerate() {
                    w.write(
                        Name::new(
                            b"symValue",
                            i,
                            Some(if v.value.names == ValueNames::Indexed {
                                j
                            } else {
                                0
                            }),
                            b"",
                        )
                        .bytes(),
                        V::Int(*x),
                    )?;
                }
            }
        }
    }
    let len = 1764usize
        .checked_add(f.guild_tail.len())
        .ok_or_else(|| bad(ErrorKind::LimitExceeded))?;
    if s.metadata.format == Format::Binary && !f.guild_tail.is_empty() {
        return Err(bad(ErrorKind::Unsupported));
    }
    let field_len = if s.metadata.format == Format::Ascii {
        len.checked_mul(2)
            .and_then(|n| n.checked_add(b"guildTable=raw:".len()))
            .ok_or_else(|| bad(ErrorKind::LimitExceeded))?
    } else {
        len
    };
    if len > limits.archive.bytes
        || (s.metadata.format != Format::Binary
            && (field_len > limits.archive.field_bytes
                || (s.metadata.format == Format::BinSafe && len > u16::MAX as usize)))
    {
        return Err(bad(ErrorKind::LimitExceeded));
    }
    let mut raw = Vec::new();
    raw.try_reserve_exact(len)
        .map_err(|_| bad(ErrorKind::LimitExceeded))?;
    for row in &f.guild_attitudes {
        raw.extend_from_slice(row);
    }
    raw.extend_from_slice(&f.guild_tail);
    w.write(b"guildTable", V::Raw(raw.into()))?;
    e.finish()
}
