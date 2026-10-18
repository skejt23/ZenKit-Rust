// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;
use archive::{Value as V, ValueType as T};

#[derive(Clone, Copy, Debug)]
pub(super) enum ContextPhase {
    Block,
    Props,
    Roles,
    Stages,
    Done,
}
struct Fields<'r, 'a, 'c> {
    r: &'r mut archive::Reader<'a>,
    copied: &'c mut usize,
    limit: usize,
}
impl<'a> Fields<'_, 'a, '_> {
    fn field(&mut self, name: &[u8], ty: T) -> Result<V<'a>> {
        let p = self.r.position();
        let v = self.r.read(ty)?;
        if v.name.is_some_and(|n| n != name) {
            return Err(at(ErrorKind::InvalidData, p, "cutscene state field name"));
        }
        Ok(v.value)
    }
    fn string(&mut self, name: &[u8]) -> Result<Vec<u8>> {
        let p = self.r.position();
        let V::String(v) = self.field(name, T::String)? else {
            unreachable!()
        };
        copy(&v, self.copied, self.limit, p)
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
    fn float(&mut self, name: &[u8]) -> Result<f32> {
        let V::Float(v) = self.field(name, T::Float)? else {
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
    fn count(&mut self, name: &[u8], left: usize) -> Result<usize> {
        let p = self.r.position();
        let n = usize::try_from(self.int(name)?).map_err(|_| {
            at(
                ErrorKind::InvalidData,
                p,
                "negative cutscene occurrence count",
            )
        })?;
        if n > left {
            return Err(at(ErrorKind::LimitExceeded, p, "cutscene occurrence count"));
        }
        Ok(n)
    }
    fn block(&mut self, b: &mut CutsceneBlock) -> Result<()> {
        b.name = self.string(b"blockName")?;
        let p = self.r.position();
        if self.int(b"numOfBlocks")? != 1 {
            return Err(at(
                ErrorKind::Unsupported,
                p,
                "cutscene block count must be one",
            ));
        }
        b.sub_block0 = self.float(b"subBlock0")?;
        Ok(())
    }
}
pub(super) fn read_prefix(
    r: &mut archive::Reader<'_>,
    node: ObjectMut<'_>,
    copied: &mut usize,
    limit: usize,
    links_left: usize,
) -> Result<usize> {
    let mut f = Fields { r, copied, limit };
    match node {
        ObjectMut::CutsceneProps(v) => {
            v.name = f.string(b"name")?;
            v.global = f.boolean(b"globalCutscene")?;
            v.looped = f.boolean(b"csLoop")?;
            v.has_to_be_triggered = f.boolean(b"hasToBeTriggerd")?;
            v.distance = f.float(b"distance")?;
            v.range = f.float(b"range")?;
            v.locked_block_count = f.int(b"numLockedBlocks")?;
            v.run_behavior = f.enumeration(b"runBehaviour")?;
            v.run_behavior_value = f.int(b"runBehaviourValue")?;
            v.stage_name = f.string(b"StageName")?;
            v.script_function_on_stop = f.string(b"scriptFuncOnStop")?;
            Ok(0)
        }
        ObjectMut::Cutscene(v) => {
            f.block(v)?;
            Ok(1)
        }
        ObjectMut::CutsceneContext(v) => {
            f.block(&mut v.block)?;
            Ok(1)
        }
        ObjectMut::CutscenePlayer(v) => {
            v.last_process_day = f.int(b"lastProcessDay")?;
            v.last_process_hour = f.int(b"lastProcessHour")?;
            f.count(b"playListCount", links_left)
        }
        ObjectMut::CutscenePoolItem(v) => {
            v.name = f.string(b"itemName")?;
            v.run_behavior = f.enumeration(b"runBehaviour")?;
            v.run_behavior_value = f.int(b"runBehaviourValue")?;
            v.times_played = f.int(b"numPlayed")?;
            v.deactivated = f.int(b"deactivated")?;
            v.flags = f.int(b"flags")?;
            Ok(0)
        }
        ObjectMut::CutsceneManager(_) => f.count(b"poolCount", links_left),
        _ => unreachable!(),
    }
}
pub(super) fn read_phase(
    r: &mut archive::Reader<'_>,
    node: ObjectMut<'_>,
    phase: &mut ContextPhase,
    copied: &mut usize,
    limit: usize,
) -> Result<usize> {
    let ObjectMut::CutsceneContext(c) = node else {
        unreachable!()
    };
    let mut f = Fields { r, copied, limit };
    Ok(match phase {
        ContextPhase::Block => {
            *phase = ContextPhase::Props;
            1
        }
        ContextPhase::Props => {
            c.role_count = f.int(b"numOfRoles")?;
            c.role_vob_count = f.int(b"numOfRoleVobs")?;
            *phase = ContextPhase::Roles;
            2
        }
        ContextPhase::Roles => {
            c.is_cutscene = f.boolean(b"isCutscene")?;
            c.reference = f.int(b"reference")?;
            c.actual_block = f.int(b"actualBlock")?;
            *phase = ContextPhase::Stages;
            2
        }
        ContextPhase::Stages => {
            c.was_triggered = f.boolean(b"wasTriggered")?;
            *phase = ContextPhase::Done;
            0
        }
        ContextPhase::Done => 0,
    })
}
fn string(w: &mut archive::Writer, name: &[u8], v: &[u8]) -> Result<()> {
    w.write(name, V::String(v.into()))
}
fn block(w: &mut archive::Writer, b: &CutsceneBlock) -> Result<()> {
    string(w, b"blockName", &b.name)?;
    w.write(b"numOfBlocks", V::Int(1))?;
    w.write(b"subBlock0", V::Float(b.sub_block0))
}
fn write_count(w: &mut archive::Writer, name: &[u8], n: usize) -> Result<()> {
    let n = i32::try_from(n)
        .map_err(|_| bad(ErrorKind::LimitExceeded, "cutscene list count exceeds i32"))?;
    w.write(name, V::Int(n))
}
pub(super) fn write_prefix(w: &mut archive::Writer, node: ObjectRef<'_>) -> Result<()> {
    match node {
        ObjectRef::CutsceneProps(v) => {
            string(w, b"name", &v.name)?;
            w.write(b"globalCutscene", V::Bool(v.global))?;
            w.write(b"csLoop", V::Bool(v.looped))?;
            w.write(b"hasToBeTriggerd", V::Bool(v.has_to_be_triggered))?;
            w.write(b"distance", V::Float(v.distance))?;
            w.write(b"range", V::Float(v.range))?;
            w.write(b"numLockedBlocks", V::Int(v.locked_block_count))?;
            w.write(b"runBehaviour", V::Enum(v.run_behavior))?;
            w.write(b"runBehaviourValue", V::Int(v.run_behavior_value))?;
            string(w, b"StageName", &v.stage_name)?;
            string(w, b"scriptFuncOnStop", &v.script_function_on_stop)?;
        }
        ObjectRef::Cutscene(v) => block(w, v)?,
        ObjectRef::CutsceneContext(v) => block(w, &v.block)?,
        ObjectRef::CutscenePlayer(v) => {
            w.write(b"lastProcessDay", V::Int(v.last_process_day))?;
            w.write(b"lastProcessHour", V::Int(v.last_process_hour))?;
            write_count(w, b"playListCount", v.playlists.len())?;
        }
        ObjectRef::CutscenePoolItem(v) => {
            string(w, b"itemName", &v.name)?;
            w.write(b"runBehaviour", V::Enum(v.run_behavior))?;
            w.write(b"runBehaviourValue", V::Int(v.run_behavior_value))?;
            w.write(b"numPlayed", V::Int(v.times_played))?;
            w.write(b"deactivated", V::Int(v.deactivated))?;
            w.write(b"flags", V::Int(v.flags))?;
        }
        ObjectRef::CutsceneManager(v) => write_count(w, b"poolCount", v.pool_items.len())?,
        _ => unreachable!(),
    }
    Ok(())
}
pub(super) fn write_phase(
    w: &mut archive::Writer,
    node: ObjectRef<'_>,
    phase: &mut ContextPhase,
) -> Result<usize> {
    let ObjectRef::CutsceneContext(c) = node else {
        unreachable!()
    };
    Ok(match phase {
        ContextPhase::Block => {
            *phase = ContextPhase::Props;
            1
        }
        ContextPhase::Props => {
            w.write(b"numOfRoles", V::Int(c.role_count))?;
            w.write(b"numOfRoleVobs", V::Int(c.role_vob_count))?;
            *phase = ContextPhase::Roles;
            2
        }
        ContextPhase::Roles => {
            w.write(b"isCutscene", V::Bool(c.is_cutscene))?;
            w.write(b"reference", V::Int(c.reference))?;
            w.write(b"actualBlock", V::Int(c.actual_block))?;
            *phase = ContextPhase::Stages;
            2
        }
        ContextPhase::Stages => {
            w.write(b"wasTriggered", V::Bool(c.was_triggered))?;
            *phase = ContextPhase::Done;
            0
        }
        ContextPhase::Done => 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn declared_lists_do_not_allocate_pending_occurrences() {
        for player in [false, true] {
            let limits = archive::Limits::default();
            let mut w = archive::Writer::new(archive::Header::new(archive::Format::Binary), limits)
                .unwrap();
            let node = if player {
                Object::CutscenePlayer(crate::cutscene::Player {
                    playlists: vec![Link::new(Vec::new(), None); 100],
                    ..Default::default()
                })
            } else {
                Object::CutsceneManager(crate::cutscene::Manager {
                    pool_items: vec![Link::new(Vec::new(), None); 100],
                })
            };
            write_prefix(&mut w, node.as_ref()).unwrap();
            let bytes = w.finish().unwrap();
            for cap in [99, 100] {
                let mut r = archive::Reader::new(&bytes, limits).unwrap();
                let mut node = if player {
                    Object::CutscenePlayer(Default::default())
                } else {
                    Object::CutsceneManager(Default::default())
                };
                let mut copied = 0;
                let result = read_prefix(&mut r, node.as_mut(), &mut copied, 0, cap);
                if cap == 99 {
                    assert_eq!(result.unwrap_err().kind, ErrorKind::LimitExceeded);
                } else {
                    assert_eq!(result.unwrap(), 100);
                }
                let links = match node {
                    Object::CutscenePlayer(v) => v.playlists,
                    Object::CutsceneManager(v) => v.pool_items,
                    _ => unreachable!(),
                };
                assert_eq!((links.len(), links.capacity()), (0, 0));
            }
        }
    }
}
