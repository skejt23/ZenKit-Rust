// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;

/// Movable-object fields shared by every MOB subtype. `focus_name` is separate
/// from the containing VOB's name. Strings are uninterpreted archive bytes;
/// hitpoints and damage retain negative values and other script sentinels.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Movable {
    pub focus_name: Vec<u8>,
    pub hp: i32,
    pub damage: i32,
    pub movable: bool,
    pub takeable: bool,
    pub focus_override: bool,
    pub material: SoundMaterial,
    pub visual_destroyed: Vec<u8>,
    pub owner: Vec<u8>,
    pub owner_guild: Vec<u8>,
    pub destroyed: bool,
}
impl fmt::Debug for Movable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Movable")
            .field("focus_name_bytes", &self.focus_name.len())
            .field("hp", &self.hp)
            .field("damage", &self.damage)
            .field("movable", &self.movable)
            .field("takeable", &self.takeable)
            .field("focus_override", &self.focus_override)
            .field("material", &self.material)
            .field("visual_destroyed_bytes", &self.visual_destroyed.len())
            .field("owner_bytes", &self.owner.len())
            .field("owner_guild_bytes", &self.owner_guild.len())
            .field("destroyed", &self.destroyed)
            .finish()
    }
}
/// Interactive fields shared by fire, containers, doors and the four marker
/// subtypes. State counts and script expressions are retained without guessing
/// runtime state-machine or script validity.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Interactive {
    pub movable: Movable,
    pub state_count: i32,
    pub target: Vec<u8>,
    pub item: Vec<u8>,
    pub condition_function: Vec<u8>,
    pub on_state_change_function: Vec<u8>,
    pub rewind: bool,
}
impl fmt::Debug for Interactive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Interactive")
            .field("movable", &self.movable)
            .field("state_count", &self.state_count)
            .field("target_bytes", &self.target.len())
            .field("item_bytes", &self.item.len())
            .field("condition_function_bytes", &self.condition_function.len())
            .field(
                "on_state_change_function_bytes",
                &self.on_state_change_function.len(),
            )
            .field("rewind", &self.rewind)
            .finish()
    }
}
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Fire {
    pub interactive: Interactive,
    pub slot: Vec<u8>,
    pub vob_tree: Vec<u8>,
}
impl fmt::Debug for Fire {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Fire")
            .field("interactive", &self.interactive)
            .field("slot_bytes", &self.slot.len())
            .field("vob_tree_bytes", &self.vob_tree.len())
            .finish()
    }
}
/// A container's textual `contents` expression is independent of its saved item
/// occurrences. `items` must be `Some` exactly in save archives, even when empty.
/// Nulls and repeated identities are valid; every nonnull target must be an
/// [`Item`] VOB. Names and occurrence order are retained, including aliases.
///
/// Occurrences share the graph's cumulative `Limits::links` budget; there is no
/// additional `sequence_items` charge. The signed wire count is derived on write
/// and must fit i32. Direct edits are drafts, checked by graph validation.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Container {
    pub interactive: Interactive,
    pub locked: bool,
    pub key: Vec<u8>,
    pub pick_string: Vec<u8>,
    pub contents: Vec<u8>,
    pub items: Option<Vec<Link>>,
}
impl fmt::Debug for Container {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Container")
            .field("interactive", &self.interactive)
            .field("locked", &self.locked)
            .field("key_bytes", &self.key.len())
            .field("pick_string_bytes", &self.pick_string.len())
            .field("contents_bytes", &self.contents.len())
            .field("item_count", &self.items.as_ref().map(Vec::len))
            .finish()
    }
}
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Door {
    pub interactive: Interactive,
    pub locked: bool,
    pub key: Vec<u8>,
    pub pick_string: Vec<u8>,
}
impl fmt::Debug for Door {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Door")
            .field("interactive", &self.interactive)
            .field("locked", &self.locked)
            .field("key_bytes", &self.key.len())
            .field("pick_string_bytes", &self.pick_string.len())
            .finish()
    }
}
impl VobKind {
    /// Shared interactive fields, without duplicated class or base-MOB state.
    pub fn interactive(&self) -> Option<&Interactive> {
        self.as_ref().interactive()
    }
    pub fn interactive_mut(&mut self) -> Option<&mut Interactive> {
        match self {
            Self::Interactive(v)
            | Self::Ladder(v)
            | Self::Switch(v)
            | Self::Wheel(v)
            | Self::Bed(v) => Some(v),
            Self::Fire(v) => Some(&mut v.interactive),
            Self::Container(v) => Some(&mut v.interactive),
            Self::Door(v) => Some(&mut v.interactive),
            _ => None,
        }
    }
    pub fn movable(&self) -> Option<&Movable> {
        self.as_ref().movable()
    }
    pub fn movable_mut(&mut self) -> Option<&mut Movable> {
        match self {
            Self::Movable(v) => Some(v),
            v => v.interactive_mut().map(|v| &mut v.movable),
        }
    }
}

impl<'a> VobKindRef<'a> {
    pub fn interactive(self) -> Option<&'a Interactive> {
        match self {
            Self::Interactive(v)
            | Self::Ladder(v)
            | Self::Switch(v)
            | Self::Wheel(v)
            | Self::Bed(v) => Some(v),
            Self::Fire(v) => Some(&v.interactive),
            Self::Container(v) => Some(&v.interactive),
            Self::Door(v) => Some(&v.interactive),
            _ => None,
        }
    }
}

impl VobKindMut<'_> {
    pub fn interactive_mut(&mut self) -> Option<&mut Interactive> {
        match self {
            Self::Interactive(v)
            | Self::Ladder(v)
            | Self::Switch(v)
            | Self::Wheel(v)
            | Self::Bed(v) => Some(v),
            Self::Fire(v) => Some(&mut v.interactive),
            Self::Container(v) => Some(&mut v.interactive),
            Self::Door(v) => Some(&mut v.interactive),
            _ => None,
        }
    }
}

impl<'a> VobKindRef<'a> {
    pub fn movable(self) -> Option<&'a Movable> {
        match self {
            Self::Movable(v) => Some(v),
            v => v.interactive().map(|v| &v.movable),
        }
    }
}

impl VobKindMut<'_> {
    pub fn movable_mut(&mut self) -> Option<&mut Movable> {
        match self {
            Self::Movable(v) => Some(v),
            v => v.interactive_mut().map(|v| &mut v.movable),
        }
    }
}

impl<'a> VobKindRef<'a> {
    pub(crate) fn mob_buffers(self) -> impl Iterator<Item = &'a [u8]> {
        let mut fields = [None; 11];
        if let Some(m) = self.movable() {
            fields[..4].copy_from_slice(&[
                Some(m.focus_name.as_slice()),
                Some(&m.visual_destroyed),
                Some(&m.owner),
                Some(&m.owner_guild),
            ]);
        }
        if let Some(i) = self.interactive() {
            fields[4..8].copy_from_slice(&[
                Some(i.target.as_slice()),
                Some(&i.item),
                Some(&i.condition_function),
                Some(&i.on_state_change_function),
            ]);
        }
        match self {
            Self::Fire(v) => {
                fields[8] = Some(&v.slot);
                fields[9] = Some(&v.vob_tree);
            }
            Self::Door(v) => {
                fields[8] = Some(&v.key);
                fields[9] = Some(&v.pick_string);
            }
            Self::Container(v) => {
                fields[8] = Some(&v.key);
                fields[9] = Some(&v.pick_string);
                fields[10] = Some(&v.contents);
            }
            _ => {}
        }
        fields.into_iter().flatten()
    }
}

impl<'a> VobKindRef<'a> {
    pub(crate) fn check_mob(self, save: bool) -> Result<()> {
        if let Self::Container(v) = self {
            if v.items.is_some() != save {
                return Err(bad("container items must match save mode"));
            }
            if v.items
                .as_ref()
                .is_some_and(|v| v.len() > i32::MAX as usize)
            {
                return Err(bad("container item count exceeds i32"));
            }
        }
        Ok(())
    }
}

impl VobKindMut<'_> {
    pub(crate) fn appended_links_mut(&mut self) -> Option<&mut Vec<Link>> {
        match self {
            Self::CodeMaster(v) => v.state.as_mut().map(|s| &mut s.triggered),
            Self::Container(v) => v.items.as_mut(),
            _ => None,
        }
    }
}
