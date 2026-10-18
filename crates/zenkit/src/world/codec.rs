// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;
use crate::{
    Error, ErrorKind, Result,
    archive::{self, Value as V, ValueType as T},
    objects::{self, Limits as ObjectLimits, ObjectMut, ObjectRef},
};
const CTX: &str = "world";
fn bad(kind: ErrorKind) -> Error {
    Error::operation(kind, CTX)
}
fn at(kind: ErrorKind, p: usize) -> Error {
    Error::new(kind, p, CTX)
}
fn push<T>(v: &mut Vec<T>, value: T) -> Result<()> {
    v.try_reserve(1)
        .map_err(|_| bad(ErrorKind::LimitExceeded))?;
    v.push(value);
    Ok(())
}
fn i32n(n: usize) -> Result<i32> {
    i32::try_from(n).map_err(|_| bad(ErrorKind::LimitExceeded))
}
pub(crate) struct ReadBudget<'a, 'input> {
    pub copied: &'a mut usize,
    pub geometry: &'a mut mesh::Budget<'input>,
    pub sections: &'a mut usize,
    pub limits: ObjectLimits,
    pub links_left: usize,
}
struct Fields<'r, 'a, 'b, 'input> {
    r: &'r mut archive::Reader<'a>,
    b: &'r mut ReadBudget<'b, 'input>,
}
impl<'a> Fields<'_, 'a, '_, '_> {
    fn field(&mut self, name: &[u8], t: T) -> Result<V<'a>> {
        let p = self.r.position();
        let e = self.r.read(t)?;
        if e.name.is_some_and(|n| {
            n != name
                && !matches!(
                    (name, n),
                    (b"isRaining", b"isRaining\t\t ") | (b"rainCtr", b"rainCtr\t\t ")
                )
        }) {
            return Err(at(ErrorKind::InvalidData, p));
        }
        Ok(e.value)
    }
    fn int(&mut self, n: &[u8]) -> Result<i32> {
        let V::Int(v) = self.field(n, T::Int)? else {
            unreachable!()
        };
        Ok(v)
    }
    fn float(&mut self, n: &[u8]) -> Result<f32> {
        let V::Float(v) = self.field(n, T::Float)? else {
            unreachable!()
        };
        Ok(v)
    }
    fn boolean(&mut self, n: &[u8]) -> Result<bool> {
        let V::Bool(v) = self.field(n, T::Bool)? else {
            unreachable!()
        };
        Ok(v)
    }
    fn vec3(&mut self, n: &[u8]) -> Result<Vec3> {
        let V::Vec3(v) = self.field(n, T::Vec3)? else {
            unreachable!()
        };
        Ok(v)
    }
    fn string(&mut self, n: &[u8]) -> Result<Vec<u8>> {
        let p = self.r.position();
        let V::String(v) = self.field(n, T::String)? else {
            unreachable!()
        };
        objects::copy(&v, self.b.copied, self.b.limits.copied_bytes, p)
    }
    fn count(&mut self, n: &[u8], factor: usize) -> Result<usize> {
        let p = self.r.position();
        let n = usize::try_from(self.int(n)?).map_err(|_| at(ErrorKind::InvalidData, p))?;
        if n.checked_mul(factor).is_none_or(|n| n > self.b.links_left) {
            return Err(at(ErrorKind::LimitExceeded, p));
        }
        Ok(n)
    }
    fn children(&mut self) -> Result<(usize, Option<Vec<u8>>)> {
        read_children(
            self.r,
            self.b.copied,
            self.b.limits.copied_bytes,
            self.b.links_left,
        )
    }
}
pub(crate) fn read_children(
    reader: &mut archive::Reader<'_>,
    copied: &mut usize,
    copied_limit: usize,
    links_left: usize,
) -> Result<(usize, Option<Vec<u8>>)> {
    let p = reader.position();
    let e = reader.read(T::Int)?;
    let name = e
        .name
        .map(|n| {
            if !child_name(n) {
                return Err(at(ErrorKind::InvalidData, p));
            }
            objects::copy(n, copied, copied_limit, p)
        })
        .transpose()?;
    let V::Int(n) = e.value else { unreachable!() };
    let n = usize::try_from(n).map_err(|_| at(ErrorKind::InvalidData, p))?;
    if n > links_left {
        return Err(at(ErrorKind::LimitExceeded, p));
    }
    Ok((n, name))
}
fn child_name(n: &[u8]) -> bool {
    n.strip_prefix(b"childs")
        .is_some_and(|v| !v.is_empty() && v.iter().all(u8::is_ascii_digit))
}
#[derive(Debug, Default)]
pub(crate) struct WorldPhase {
    step: u8,
    section: usize,
    remaining: usize,
    roots: usize,
    last: usize,
    stack: Vec<(usize, usize)>,
    counts: Vec<usize>,
    ordinal: usize,
}
#[derive(Debug, Default)]
pub(crate) struct WayPhase {
    step: u8,
    remaining: usize,
    index: usize,
    left: Option<Link>,
}
#[derive(Debug)]
pub(crate) enum Phase {
    World(WorldPhase),
    Way(WayPhase),
    None,
}
impl Phase {
    pub(crate) fn new(node: ObjectRef<'_>) -> Self {
        match node {
            ObjectRef::World(_) => Self::World(Default::default()),
            ObjectRef::WayNet(_) => Self::Way(Default::default()),
            _ => Self::None,
        }
    }
}
#[derive(Clone, Copy)]
pub(crate) enum Edge {
    Vob,
    WayPoint,
    WayNet,
    Player,
    Sky,
    Npc,
}
pub(crate) fn edge(phase: &Phase, node: ObjectRef<'_>) -> Edge {
    match phase {
        Phase::Way(_) => Edge::WayPoint,
        Phase::World(p) => match p.step {
            1 => Edge::Vob,
            2 => {
                let ObjectRef::World(w) = node else {
                    unreachable!()
                };
                match w.sections[p.section].data {
                    SectionData::WayNet(_) => Edge::WayNet,
                    SectionData::CutscenePlayer(_) => Edge::Player,
                    SectionData::SkyController(_) => Edge::Sky,
                    _ => unreachable!(),
                }
            }
            4 | 5 => Edge::Npc,
            _ => unreachable!(),
        },
        _ => unreachable!(),
    }
}
pub(crate) fn read_prefix(
    r: &mut archive::Reader<'_>,
    node: ObjectMut<'_>,
    mut b: ReadBudget<'_, '_>,
) -> Result<()> {
    let mut f = Fields { r, b: &mut b };
    match node {
        ObjectMut::WayPoint(v) => {
            v.name = f.string(b"wpName")?;
            v.water_depth = f.int(b"waterDepth")?;
            v.under_water = f.boolean(b"underWater")?;
            v.position = f.vec3(b"position")?;
            v.direction = f.vec3(b"direction")?;
        }
        ObjectMut::SkyController(v) => {
            v.master_time = f.float(b"masterTime")?;
            v.rain_weight = f.float(b"rainWeight")?;
            v.rain_start = f.float(b"rainStart")?;
            v.rain_stop = f.float(b"rainStop")?;
            v.rain_sct_timer = f.float(b"rainSctTimer")?;
            v.rain_snd_vol = f.float(b"rainSndVol")?;
            v.day_ctr = f.float(b"dayCtr")?;
            if let SkyVersion::Gothic2(g) = &mut v.version {
                g.fade_scale = f.float(b"fadeScale")?;
                g.render_lightning = f.boolean(b"renderLightning")?;
                g.is_raining = f.boolean(b"isRaining")?;
                g.rain_ctr = f.int(b"rainCtr")?;
            }
        }
        ObjectMut::World(_) | ObjectMut::WayNet(_) => {}
        _ => unreachable!(),
    };
    Ok(())
}
pub(crate) fn read_phase(
    r: &mut archive::Reader<'_>,
    node: ObjectMut<'_>,
    phase: &mut Phase,
    game: GameVersion,
    mut b: ReadBudget<'_, '_>,
) -> Result<usize> {
    match phase {
        Phase::None => Ok(0),
        Phase::Way(p) => {
            let ObjectMut::WayNet(w) = node else {
                unreachable!()
            };
            let mut f = Fields { r, b: &mut b };
            loop {
                match p.step {
                    0 => {
                        w.version = f.int(b"waynetVersion")?;
                        p.remaining = f.count(b"numWaypoints", 1)?;
                        p.step = 1;
                    }
                    1 => {
                        if p.remaining > 0 {
                            return Ok(1);
                        }
                        p.remaining = f.count(b"numWays", 2)?;
                        p.step = 2;
                    }
                    2 | 3 => {
                        if p.remaining > 0 {
                            return Ok(1);
                        }
                        p.step = 4;
                    }
                    _ => return Ok(0),
                }
            }
        }
        Phase::World(p) => {
            let ObjectMut::World(w) = node else {
                unreachable!()
            };
            loop {
                match p.step {
                    0 => {
                        if r.has_object_end()? {
                            if w.state.is_some() {
                                return Err(at(ErrorKind::InvalidData, r.position()));
                            }
                            return Ok(0);
                        }
                        let pos = r.position();
                        let h = r
                            .begin_object()?
                            .ok_or_else(|| at(ErrorKind::InvalidData, pos))?;
                        if !h.is_empty() {
                            return Err(at(ErrorKind::Unsupported, pos));
                        }
                        *b.sections = b
                            .sections
                            .checked_add(1)
                            .filter(|n| *n <= b.limits.world.sections)
                            .ok_or_else(|| at(ErrorKind::LimitExceeded, pos))?;
                        let frame = SectionFrame {
                            class_name: objects::copy(
                                h.class_name,
                                b.copied,
                                b.limits.copied_bytes,
                                pos,
                            )?,
                            version: h.version,
                            index: h.index,
                        };
                        p.section = w.sections.len();
                        let data = match h.object_name {
                            b"MeshAndBsp" => {
                                let mut head = r.unframed(8)?;
                                let ver = head.u32(CTX)?;
                                let expected = if game == GameVersion::Gothic1 {
                                    0x2090000
                                } else {
                                    0x4090000
                                };
                                if ver != expected {
                                    return Err(at(ErrorKind::Unsupported, head.position() - 4));
                                }
                                let size = head.u32(CTX)? as usize;
                                if size > b.limits.world.geometry.bytes {
                                    return Err(at(ErrorKind::LimitExceeded, head.position() - 4));
                                }
                                let mut body = r.unframed(size)?;
                                let wide = r.header().user == Some(b"XZEN".as_slice());
                                let mesh = mesh::msh::read(&mut body, game, wide, b.geometry)?;
                                let bsp = bsp::read_budget(
                                    &mut body,
                                    game,
                                    b.limits.world.bsp_depth,
                                    b.geometry,
                                )?;
                                if body.remaining() != 0 {
                                    return Err(at(ErrorKind::Unsupported, body.position()));
                                }
                                {
                                    let g = MeshAndBsp { mesh, bsp };
                                    check_geometry_indices(&g)?;
                                    SectionData::MeshAndBsp(g)
                                }
                            }
                            b"VobTree" => {
                                let mut f = Fields { r, b: &mut b };
                                let (n, name) = f.children()?;
                                p.roots = n;
                                p.step = 1;
                                SectionData::VobTree(Hierarchy {
                                    root_count_name: name,
                                    entries: Vec::new(),
                                })
                            }
                            b"WayNet" => {
                                p.step = 2;
                                SectionData::WayNet(Link::new(Vec::new(), None))
                            }
                            b"CutscenePlayer" => {
                                p.step = 2;
                                SectionData::CutscenePlayer(Link::new(Vec::new(), None))
                            }
                            b"SkyCtrl" => {
                                p.step = 2;
                                SectionData::SkyController(Link::new(Vec::new(), None))
                            }
                            b"EndMarker" => {
                                p.step = 3;
                                SectionData::End
                            }
                            _ => return Err(at(ErrorKind::Unsupported, pos)),
                        };
                        push(&mut w.sections, Section { frame, data })?;
                        if p.step == 2 {
                            return Ok(1);
                        }
                        if p.step == 1 && p.roots > 0 {
                            if b.limits.world.hierarchy_depth == 0 {
                                return Err(at(ErrorKind::LimitExceeded, r.position()));
                            }
                            return Ok(1);
                        }
                        if !r.end_object()? {
                            return Err(at(ErrorKind::Unsupported, r.position()));
                        }
                        if p.step == 1 {
                            p.step = 0;
                        }
                    }
                    1 => {
                        let SectionData::VobTree(tree) = &mut w.sections[p.section].data else {
                            unreachable!()
                        };
                        let mut f = Fields { r, b: &mut b };
                        let (n, name) = f.children()?;
                        tree.entries[p.last].children_name = name;
                        if n > 0 {
                            if p.stack.len() + 2 > b.limits.world.hierarchy_depth {
                                return Err(at(ErrorKind::LimitExceeded, r.position()));
                            }
                            push(&mut p.stack, (p.last, n))?;
                        }
                        while p.stack.last().is_some_and(|(_, n)| *n == 0) {
                            p.stack.pop();
                        }
                        if !p.stack.is_empty() || p.roots > 0 {
                            return Ok(1);
                        }
                        if !r.end_object()? {
                            return Err(at(ErrorKind::Unsupported, r.position()));
                        }
                        p.step = 0;
                    }
                    2 => {
                        if !r.end_object()? {
                            return Err(at(ErrorKind::Unsupported, r.position()));
                        }
                        p.step = 0;
                    }
                    3 => {
                        if w.state.is_none() {
                            p.step = 7;
                            return Ok(0);
                        }
                        let mut f = Fields { r, b: &mut b };
                        p.remaining = f.count(b"npcCount", 1)?;
                        p.step = 4;
                        if p.remaining > 0 {
                            return Ok(1);
                        }
                    }
                    4 => {
                        if p.remaining > 0 {
                            return Ok(1);
                        }
                        let mut f = Fields { r, b: &mut b };
                        p.remaining = f.count(b"NoOfEntries", 1)?;
                        p.step = 5;
                        if p.remaining > 0 {
                            return Ok(1);
                        }
                        p.step = 6;
                    }
                    5 => {
                        let state = w.state.as_mut().expect("save");
                        let s = state.spawns.last_mut().expect("parsed spawn occurrence");
                        let mut f = Fields { r, b: &mut b };
                        s.position = f.vec3(b"spawnPos")?;
                        s.timer = f.float(b"timer")?;
                        if p.remaining > 0 {
                            return Ok(1);
                        }
                        p.step = 6;
                    }
                    6 => {
                        let state = w.state.as_mut().expect("save");
                        let mut f = Fields { r, b: &mut b };
                        state.spawning_enabled = f.boolean(b"spawningEnabled")?;
                        if game == GameVersion::Gothic2 {
                            state.spawn_flags = Some(f.int(b"spawnFlags")?);
                        }
                        p.step = 7;
                        return Ok(0);
                    }
                    _ => return Ok(0),
                }
            }
        }
    }
}
pub(crate) fn append(node: ObjectMut<'_>, phase: &mut Phase, link: Link) -> Result<()> {
    match phase {
        Phase::Way(p) => {
            let ObjectMut::WayNet(w) = node else {
                unreachable!()
            };
            match p.step {
                1 => {
                    push(&mut w.points, link)?;
                    p.remaining -= 1;
                }
                2 => {
                    p.left = Some(link);
                    p.step = 3;
                }
                3 => {
                    push(
                        &mut w.edges,
                        Way {
                            left: p.left.take().expect("left endpoint"),
                            right: link,
                        },
                    )?;
                    p.remaining -= 1;
                    p.step = 2;
                }
                _ => unreachable!(),
            }
        }
        Phase::World(p) => {
            let ObjectMut::World(w) = node else {
                unreachable!()
            };
            match p.step {
                1 => {
                    let SectionData::VobTree(tree) = &mut w.sections[p.section].data else {
                        unreachable!()
                    };
                    let parent = p.stack.last().map(|(i, _)| *i);
                    if let Some((_, n)) = p.stack.last_mut() {
                        *n -= 1;
                    } else {
                        p.roots -= 1;
                    }
                    p.last = tree.entries.len();
                    push(
                        &mut tree.entries,
                        HierarchyEntry {
                            object: link,
                            parent,
                            children_name: None,
                        },
                    )?;
                }
                2 => {
                    let target = match &mut w.sections[p.section].data {
                        SectionData::WayNet(v)
                        | SectionData::CutscenePlayer(v)
                        | SectionData::SkyController(v) => v,
                        _ => unreachable!(),
                    };
                    *target = link;
                }
                4 => {
                    push(&mut w.state.as_mut().expect("save").npcs, link)?;
                    p.remaining -= 1;
                }
                5 => {
                    push(
                        &mut w.state.as_mut().expect("save").spawns,
                        Spawn {
                            npc: link,
                            position: Vec3::default(),
                            timer: 0.,
                        },
                    )?;
                    p.remaining -= 1;
                }
                _ => unreachable!(),
            }
        }
        Phase::None => unreachable!(),
    };
    Ok(())
}
fn text(w: &mut archive::Writer, n: &[u8], v: &[u8]) -> Result<()> {
    w.write(n, V::String(v.into()))
}
pub(crate) fn write_prefix(w: &mut archive::Writer, node: ObjectRef<'_>) -> Result<()> {
    match node {
        ObjectRef::WayPoint(v) => {
            text(w, b"wpName", &v.name)?;
            w.write(b"waterDepth", V::Int(v.water_depth))?;
            w.write(b"underWater", V::Bool(v.under_water))?;
            w.write(b"position", V::Vec3(v.position))?;
            w.write(b"direction", V::Vec3(v.direction))?;
        }
        ObjectRef::SkyController(v) => {
            for (n, x) in [
                (b"masterTime".as_slice(), v.master_time),
                (b"rainWeight", v.rain_weight),
                (b"rainStart", v.rain_start),
                (b"rainStop", v.rain_stop),
                (b"rainSctTimer", v.rain_sct_timer),
                (b"rainSndVol", v.rain_snd_vol),
                (b"dayCtr", v.day_ctr),
            ] {
                w.write(n, V::Float(x))?;
            }
            if let SkyVersion::Gothic2(g) = v.version {
                w.write(b"fadeScale", V::Float(g.fade_scale))?;
                w.write(b"renderLightning", V::Bool(g.render_lightning))?;
                w.write(b"isRaining", V::Bool(g.is_raining))?;
                w.write(b"rainCtr", V::Int(g.rain_ctr))?;
            }
        }
        ObjectRef::World(_) | ObjectRef::WayNet(_) => {}
        _ => unreachable!(),
    };
    Ok(())
}
pub(crate) struct ChildName {
    bytes: [u8; 32],
    len: usize,
}
impl ChildName {
    pub(crate) fn new(mut n: usize) -> Self {
        let mut out = Self {
            bytes: [0; 32],
            len: 6,
        };
        out.bytes[..6].copy_from_slice(b"childs");
        let mut digits = [0; 20];
        let mut i = 20;
        loop {
            i -= 1;
            digits[i] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        out.bytes[6..6 + 20 - i].copy_from_slice(&digits[i..]);
        out.len += 20 - i;
        out
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}
fn write_count(w: &mut archive::Writer, name: &[u8], n: usize) -> Result<()> {
    w.write(name, V::Int(i32n(n)?))
}
pub(crate) fn write_phase(
    w: &mut archive::Writer,
    node: ObjectRef<'_>,
    phase: &mut Phase,
    limits: ObjectLimits,
) -> Result<usize> {
    match phase {
        Phase::None => Ok(0),
        Phase::Way(p) => {
            let ObjectRef::WayNet(n) = node else {
                unreachable!()
            };
            loop {
                match p.step {
                    0 => {
                        w.write(b"waynetVersion", V::Int(n.version))?;
                        write_count(w, b"numWaypoints", n.points.len())?;
                        p.index = 0;
                        p.step = 1;
                    }
                    1 => {
                        if p.index < n.points.len() {
                            return Ok(1);
                        }
                        write_count(w, b"numWays", n.edges.len())?;
                        p.index = 0;
                        p.step = 2;
                    }
                    2 | 3 => {
                        if p.index < n.edges.len() {
                            return Ok(1);
                        }
                        p.step = 4;
                    }
                    _ => return Ok(0),
                }
            }
        }
        Phase::World(p) => {
            let ObjectRef::World(n) = node else {
                unreachable!()
            };
            loop {
                match p.step {
                    0 => {
                        let Some(section) = n.sections.get(p.section) else {
                            return Ok(0);
                        };
                        w.begin_anonymous(
                            section.data.name(),
                            &section.frame.class_name,
                            section.frame.version,
                            section.frame.index,
                        )?;
                        match &section.data {
                            SectionData::MeshAndBsp(g) => {
                                let mut raw = crate::io::Writer::new(limits.world.geometry.bytes);
                                mesh::msh::write(&g.mesh, &mut raw, limits.world.geometry)?;
                                g.bsp.write(
                                    &mut raw,
                                    limits.world.bsp_depth,
                                    limits.world.geometry,
                                )?;
                                let bytes = raw.into_inner();
                                let len = u32::try_from(bytes.len())
                                    .map_err(|_| bad(ErrorKind::LimitExceeded))?;
                                let version: u32 = if g.mesh.game == GameVersion::Gothic1 {
                                    0x2090000
                                } else {
                                    0x4090000
                                };
                                let mut head = [0; 8];
                                head[..4].copy_from_slice(&version.to_le_bytes());
                                head[4..].copy_from_slice(&len.to_le_bytes());
                                w.unframed(&head)?;
                                w.unframed(&bytes)?;
                            }
                            SectionData::VobTree(t) => {
                                p.counts = hierarchy_counts(t, limits.world.hierarchy_depth)?;
                                write_count(
                                    w,
                                    t.root_count_name.as_deref().unwrap_or(b"childs0"),
                                    t.entries.iter().filter(|e| e.parent.is_none()).count(),
                                )?;
                                p.last = 0;
                                p.ordinal = 0;
                                p.step = 1;
                                if !t.entries.is_empty() {
                                    return Ok(1);
                                }
                            }
                            SectionData::WayNet(_)
                            | SectionData::CutscenePlayer(_)
                            | SectionData::SkyController(_) => {
                                p.step = 2;
                                return Ok(1);
                            }
                            SectionData::End => {
                                p.step = 3;
                            }
                        }
                        w.end_object()?;
                        if p.step != 3 {
                            p.section += 1;
                            p.step = 0;
                        }
                    }
                    1 => {
                        let SectionData::VobTree(t) = &n.sections[p.section].data else {
                            unreachable!()
                        };
                        let e = &t.entries[p.last];
                        if e.parent.is_none() {
                            p.ordinal = 0;
                        }
                        let name = ChildName::new(p.ordinal);
                        write_count(
                            w,
                            e.children_name.as_deref().unwrap_or(name.bytes()),
                            p.counts[p.last],
                        )?;
                        p.ordinal += 1;
                        p.last += 1;
                        if p.last < t.entries.len() {
                            return Ok(1);
                        }
                        p.counts.clear();
                        w.end_object()?;
                        p.section += 1;
                        p.step = 0;
                    }
                    2 => {
                        w.end_object()?;
                        p.section += 1;
                        p.step = 0;
                    }
                    3 => {
                        let Some(s) = &n.state else { return Ok(0) };
                        write_count(w, b"npcCount", s.npcs.len())?;
                        p.last = 0;
                        p.step = 4;
                        if !s.npcs.is_empty() {
                            return Ok(1);
                        }
                    }
                    4 => {
                        let s = n.state.as_ref().expect("save");
                        if p.last < s.npcs.len() {
                            return Ok(1);
                        }
                        write_count(w, b"NoOfEntries", s.spawns.len())?;
                        p.last = 0;
                        p.step = 5;
                        if !s.spawns.is_empty() {
                            return Ok(1);
                        }
                        p.step = 6;
                    }
                    5 => {
                        let s = n.state.as_ref().expect("save");
                        let spawn = &s.spawns[p.last];
                        w.write(b"spawnPos", V::Vec3(spawn.position))?;
                        w.write(b"timer", V::Float(spawn.timer))?;
                        p.last += 1;
                        if p.last < s.spawns.len() {
                            return Ok(1);
                        }
                        p.step = 6;
                    }
                    6 => {
                        let s = n.state.as_ref().expect("save");
                        w.write(b"spawningEnabled", V::Bool(s.spawning_enabled))?;
                        if let Some(v) = s.spawn_flags {
                            w.write(b"spawnFlags", V::Int(v))?;
                        }
                        p.step = 7;
                        return Ok(0);
                    }
                    _ => return Ok(0),
                }
            }
        }
    }
}
/// Return the next direct occurrence and advance only phases with no post-link scalar.
pub(crate) fn write_link<'a>(node: ObjectRef<'a>, phase: &mut Phase) -> &'a Link {
    match phase {
        Phase::Way(p) => {
            let ObjectRef::WayNet(w) = node else {
                unreachable!()
            };
            match p.step {
                1 => {
                    let i = p.index;
                    p.index += 1;
                    &w.points[i]
                }
                2 => {
                    p.step = 3;
                    &w.edges[p.index].left
                }
                3 => {
                    p.step = 2;
                    let i = p.index;
                    p.index += 1;
                    &w.edges[i].right
                }
                _ => unreachable!(),
            }
        }
        Phase::World(p) => {
            let ObjectRef::World(w) = node else {
                unreachable!()
            };
            match p.step {
                1 => {
                    let SectionData::VobTree(t) = &w.sections[p.section].data else {
                        unreachable!()
                    };
                    &t.entries[p.last].object
                }
                2 => match &w.sections[p.section].data {
                    SectionData::WayNet(l)
                    | SectionData::CutscenePlayer(l)
                    | SectionData::SkyController(l) => l,
                    _ => unreachable!(),
                },
                4 => {
                    let i = p.last;
                    p.last += 1;
                    &w.state.as_ref().expect("save").npcs[i]
                }
                5 => &w.state.as_ref().expect("save").spawns[p.last].npc,
                _ => unreachable!(),
            }
        }
        Phase::None => unreachable!(),
    }
}
pub(crate) fn hierarchy_counts(t: &Hierarchy, depth: usize) -> Result<Vec<usize>> {
    let mut counts = Vec::new();
    counts
        .try_reserve_exact(t.entries.len())
        .map_err(|_| bad(ErrorKind::LimitExceeded))?;
    counts.resize(t.entries.len(), 0usize);
    let mut stack = Vec::new();
    for (i, e) in t.entries.iter().enumerate() {
        if let Some(parent) = e.parent {
            while stack.last().is_some_and(|v| *v != parent) {
                stack.pop();
            }
            if stack.last() != Some(&parent) {
                return Err(bad(ErrorKind::InvalidData));
            }
            counts[parent] = counts[parent]
                .checked_add(1)
                .ok_or_else(|| bad(ErrorKind::LimitExceeded))?;
        } else {
            stack.clear();
        }
        if stack.len() >= depth {
            return Err(bad(ErrorKind::LimitExceeded));
        }
        push(&mut stack, i)?;
        if e.children_name.as_ref().is_some_and(|n| !child_name(n)) {
            return Err(bad(ErrorKind::InvalidData));
        }
    }
    if t.root_count_name.as_ref().is_some_and(|n| !child_name(n)) {
        return Err(bad(ErrorKind::InvalidData));
    }
    for n in &counts {
        i32n(*n)?;
    }
    Ok(counts)
}
pub(crate) fn edges<'a>(node: ObjectRef<'a>) -> impl Iterator<Item = (Edge, &'a Link)> {
    let world = if let ObjectRef::World(w) = node {
        Some(w)
    } else {
        None
    };
    let way = if let ObjectRef::WayNet(w) = node {
        Some(w)
    } else {
        None
    };
    world
        .into_iter()
        .flat_map(|w| {
            w.sections
                .iter()
                .flat_map(|s| {
                    let fixed = match &s.data {
                        SectionData::WayNet(l) => Some((Edge::WayNet, l)),
                        SectionData::CutscenePlayer(l) => Some((Edge::Player, l)),
                        SectionData::SkyController(l) => Some((Edge::Sky, l)),
                        _ => None,
                    };
                    let entries = match &s.data {
                        SectionData::VobTree(t) => t.entries.as_slice(),
                        _ => &[],
                    };
                    fixed
                        .into_iter()
                        .chain(entries.iter().map(|e| (Edge::Vob, &e.object)))
                })
                .chain(
                    w.state
                        .iter()
                        .flat_map(|s| s.npcs.iter().chain(s.spawns.iter().map(|v| &v.npc)))
                        .map(|l| (Edge::Npc, l)),
                )
        })
        .chain(
            way.into_iter()
                .flat_map(|w| w.links().map(|l| (Edge::WayPoint, l))),
        )
}
pub(crate) fn validate(
    w: &World,
    save: bool,
    limits: ObjectLimits,
    geometry: &mut mesh::Budget,
    sections: &mut usize,
    copied: &mut usize,
) -> Result<()> {
    if w.state.is_some() != save {
        return Err(bad(ErrorKind::InvalidData));
    }
    *sections = sections
        .checked_add(w.sections.len())
        .filter(|n| *n <= limits.world.sections)
        .ok_or_else(|| bad(ErrorKind::LimitExceeded))?;
    let mut charge = |bytes: &[u8]| -> Result<()> {
        *copied = copied
            .checked_add(bytes.len())
            .filter(|n| *n <= limits.copied_bytes)
            .ok_or_else(|| bad(ErrorKind::LimitExceeded))?;
        Ok(())
    };
    for (i, s) in w.sections.iter().enumerate() {
        if !(s.frame.class_name.is_empty() || s.frame.class_name == b"%") {
            return Err(bad(ErrorKind::Unsupported));
        }
        charge(&s.frame.class_name)?;
        match &s.data {
            SectionData::MeshAndBsp(g) => {
                if g.mesh.game != g.bsp.game {
                    return Err(bad(ErrorKind::InvalidData));
                }
                check_geometry_indices(g)?;
                mesh::msh::validate(&g.mesh, geometry)?;
                g.bsp.validate_budget(limits.world.bsp_depth, geometry)?;
            }
            SectionData::VobTree(t) => {
                if t.entries.len() > limits.links {
                    return Err(bad(ErrorKind::LimitExceeded));
                }
                hierarchy_counts(t, limits.world.hierarchy_depth)?;
                i32n(t.entries.iter().filter(|e| e.parent.is_none()).count())?;
                if let Some(n) = &t.root_count_name {
                    charge(n)?;
                }
                for e in &t.entries {
                    if let Some(n) = &e.children_name {
                        charge(n)?;
                    }
                }
            }
            SectionData::End if i + 1 != w.sections.len() => {
                return Err(bad(ErrorKind::InvalidData));
            }
            _ => {}
        }
    }
    if let Some(s) = &w.state {
        if !w
            .sections
            .last()
            .is_some_and(|s| matches!(s.data, SectionData::End))
        {
            return Err(bad(ErrorKind::InvalidData));
        }
        i32n(s.npcs.len())?;
        i32n(s.spawns.len())?;
    }
    Ok(())
}

fn check_geometry_indices(g: &MeshAndBsp) -> Result<()> {
    g.bsp.validate_mesh_polygons(g.mesh.polygons.len())
}
