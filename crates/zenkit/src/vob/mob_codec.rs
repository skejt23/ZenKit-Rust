// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::{codec::Fields, *};
use crate::archive::{self, Value as V};

/// Runs after the complete base VOB save tail, yielding only Container items.
pub(super) fn read(
    f: &mut Fields<'_, '_, '_>,
    kind: &mut VobKindMut<'_>,
    links_left: usize,
) -> Result<usize> {
    let save = f.r.header().save_game;
    let Some(m) = kind.movable_mut() else {
        return Ok(0);
    };
    m.focus_name = f.string(b"focusName")?;
    m.hp = f.int(b"hitpoints")?;
    m.damage = f.int(b"damage")?;
    m.movable = f.boolean(b"moveable")?;
    m.takeable = f.boolean(b"takeable")?;
    m.focus_override = f.boolean(b"focusOverride")?;
    m.material = SoundMaterial::from_raw(f.enumeration(b"soundMaterial")?);
    m.visual_destroyed = f.string(b"visualDestroyed")?;
    m.owner = f.string(b"owner")?;
    m.owner_guild = f.string(b"ownerGuild")?;
    m.destroyed = f.boolean(b"isDestroyed")?;
    if let Some(i) = kind.interactive_mut() {
        i.state_count = f.int(b"stateNum")?;
        i.target = f.string(b"triggerTarget")?;
        i.item = f.string(b"useWithItem")?;
        i.condition_function = f.string(b"conditionFunc")?;
        i.on_state_change_function = f.string(b"onStateFunc")?;
        i.rewind = f.boolean(b"rewind")?;
    }
    match kind {
        VobKindMut::Fire(v) => {
            v.slot = f.string(b"fireSlot")?;
            v.vob_tree = f.string(b"fireVobtreeName")?;
        }
        VobKindMut::Door(v) => {
            v.locked = f.boolean(b"locked")?;
            v.key = f.string(b"keyInstance")?;
            v.pick_string = f.string(b"pickLockStr")?;
        }
        VobKindMut::Container(v) => {
            v.locked = f.boolean(b"locked")?;
            v.key = f.string(b"keyInstance")?;
            v.pick_string = f.string(b"pickLockStr")?;
            v.contents = f.string(b"contains")?;
            if save {
                let p = f.r.position();
                let count = f.int(b"NumOfEntries")?;
                let n = usize::try_from(count).map_err(|_| {
                    Error::new(ErrorKind::InvalidData, p, "negative container item count")
                })?;
                if n > links_left {
                    return Err(Error::new(
                        ErrorKind::LimitExceeded,
                        p,
                        "container occurrence count",
                    ));
                }
                // The enclosing iterative frame retains the count. In particular,
                // no declared-count Link allocation precedes the first child.
                v.items = Some(Vec::new());
                return Ok(n);
            }
        }
        _ => {}
    }
    Ok(0)
}
fn string(w: &mut archive::Writer, key: &[u8], value: &[u8]) -> Result<()> {
    w.write(key, V::String(value.into()))
}
pub(super) fn write(w: &mut archive::Writer, kind: VobKindRef<'_>) -> Result<usize> {
    let Some(m) = kind.movable() else {
        return Ok(0);
    };
    string(w, b"focusName", &m.focus_name)?;
    w.write(b"hitpoints", V::Int(m.hp))?;
    w.write(b"damage", V::Int(m.damage))?;
    w.write(b"moveable", V::Bool(m.movable))?;
    w.write(b"takeable", V::Bool(m.takeable))?;
    w.write(b"focusOverride", V::Bool(m.focus_override))?;
    w.write(b"soundMaterial", V::Enum(m.material.raw()))?;
    string(w, b"visualDestroyed", &m.visual_destroyed)?;
    string(w, b"owner", &m.owner)?;
    string(w, b"ownerGuild", &m.owner_guild)?;
    w.write(b"isDestroyed", V::Bool(m.destroyed))?;
    if let Some(i) = kind.interactive() {
        w.write(b"stateNum", V::Int(i.state_count))?;
        string(w, b"triggerTarget", &i.target)?;
        string(w, b"useWithItem", &i.item)?;
        string(w, b"conditionFunc", &i.condition_function)?;
        string(w, b"onStateFunc", &i.on_state_change_function)?;
        w.write(b"rewind", V::Bool(i.rewind))?;
    }
    match kind {
        VobKindRef::Fire(v) => {
            string(w, b"fireSlot", &v.slot)?;
            string(w, b"fireVobtreeName", &v.vob_tree)?;
        }
        VobKindRef::Door(v) => {
            w.write(b"locked", V::Bool(v.locked))?;
            string(w, b"keyInstance", &v.key)?;
            string(w, b"pickLockStr", &v.pick_string)?;
        }
        VobKindRef::Container(v) => {
            w.write(b"locked", V::Bool(v.locked))?;
            string(w, b"keyInstance", &v.key)?;
            string(w, b"pickLockStr", &v.pick_string)?;
            string(w, b"contains", &v.contents)?;
            if let Some(items) = &v.items {
                let count = i32::try_from(items.len())
                    .map_err(|_| bad("container item count exceeds i32"))?;
                w.write(b"NumOfEntries", V::Int(count))?;
                return Ok(items.len());
            }
        }
        _ => {}
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn declared_container_count_yields_without_pending_link_allocation() {
        let mut header = archive::Header::new(archive::Format::Binary);
        header.save_game = true;
        let limits = archive::Limits::default();
        let mut writer = archive::Writer::new(header, limits).unwrap();
        let source = VobKind::Container(Container {
            items: Some(vec![Link::default(); 100]),
            ..Default::default()
        });
        assert_eq!(write(&mut writer, source.as_ref()).unwrap(), 100);
        let bytes = writer.finish().unwrap();
        let mut reader = archive::Reader::new(&bytes, limits).unwrap();
        let mut copied = 0;
        let mut fields = Fields {
            r: &mut reader,
            copied: &mut copied,
            limit: 0,
        };
        let mut kind = VobKind::Container(Container::default());
        assert_eq!(read(&mut fields, &mut kind.as_mut(), 100).unwrap(), 100);
        let VobKind::Container(c) = kind else {
            unreachable!()
        };
        let items = c.items.unwrap();
        assert_eq!(items.len(), 0);
        assert_eq!(items.capacity(), 0);
        assert_eq!(copied, 0);
    }
}
