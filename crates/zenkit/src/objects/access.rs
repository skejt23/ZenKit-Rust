//! Typed accessors for graph payloads.
use super::*;
use crate::vob::{Container, Item, NpcData, VobKindMut, VobKindRef, VobMut, VobRef};

impl Graph {
    /// Borrow a VOB of any subtype, including a base VOB.
    /// A foreign handle returns `InvalidHandle`; a non-VOB returns `WrongObjectType`.
    #[inline]
    pub fn vob(&self, id: ObjectId) -> Result<VobRef<'_>> {
        match self.get(id)? {
            ObjectRef::Vob(value) => Ok(value),
            _ => Err(bad(ErrorKind::WrongObjectType, "VOB graph access")),
        }
    }

    /// Edit a VOB's common fields or current subtype. Structural replacement
    /// remains explicit through [`Self::replace_vob_kind`].
    #[inline]
    pub fn vob_mut(&mut self, id: ObjectId) -> Result<VobMut<'_>> {
        match self.get_mut(id)? {
            ObjectMut::Vob(value) => Ok(value),
            _ => Err(bad(ErrorKind::WrongObjectType, "VOB graph access")),
        }
    }

    /// Borrow the exact NPC subtype, without materializing an owned VOB.
    #[inline]
    pub fn npc(&self, id: ObjectId) -> Result<&NpcData> {
        match self.get(id)? {
            ObjectRef::Vob(value) => match value.kind() {
                VobKindRef::Npc(value) => Ok(value),
                _ => Err(bad(ErrorKind::WrongObjectType, "NPC graph access")),
            },
            _ => Err(bad(ErrorKind::WrongObjectType, "NPC graph access")),
        }
    }

    /// Edit an NPC. Read other nodes' IDs/scalars before borrowing it mutably.
    ///
    /// ```
    /// use zenkit::{GameVersion, objects::{Graph, Object}, vob::*};
    /// let mut graph = Graph::new()?;
    /// let mut value = Vob::new(GameVersion::Gothic2);
    /// value.kind = VobKind::Npc(NpcData::new(GameVersion::Gothic2));
    /// let id = graph.insert(Object::Vob(value))?;
    /// let npc = graph.npc_mut(id)?;
    /// npc.guild = 42;
    /// npc.enemy.target = Some(id);
    /// assert_eq!(graph.npc(id)?.guild, 42);
    /// # Ok::<(), zenkit::Error>(())
    /// ```
    ///
    /// Structural changes cannot overlap a subsequently used payload borrow:
    ///
    /// ```compile_fail,E0499
    /// use zenkit::{GameVersion, objects::{Graph, Object}, vob::*};
    /// let mut graph = Graph::new()?;
    /// let mut value = Vob::new(GameVersion::Gothic2);
    /// value.kind = VobKind::Npc(NpcData::new(GameVersion::Gothic2));
    /// let id = graph.insert(Object::Vob(value))?;
    /// let npc = graph.npc_mut(id)?;
    /// graph.replace_vob_kind(id, VobKind::Base)?;
    /// npc.guild = 42;
    /// # Ok::<(), zenkit::Error>(())
    /// ```
    #[inline]
    pub fn npc_mut(&mut self, id: ObjectId) -> Result<&mut NpcData> {
        match self.get_mut(id)? {
            ObjectMut::Vob(value) => match value.into_parts().1 {
                VobKindMut::Npc(value) => Ok(value),
                _ => Err(bad(ErrorKind::WrongObjectType, "NPC graph access")),
            },
            _ => Err(bad(ErrorKind::WrongObjectType, "NPC graph access")),
        }
    }
}

macro_rules! vob_accessor {
    ($read:ident, $edit:ident, $variant:ident, $ty:ty, $context:literal) => {
        impl Graph {
            #[doc = concat!("Borrow the exact `", stringify!($variant), "` subtype. A foreign handle returns `InvalidHandle`; another schema returns `WrongObjectType`.")]
            #[inline]
            pub fn $read(&self, id: ObjectId) -> Result<&$ty> {
                match self.get(id)? {
                    ObjectRef::Vob(value) => match value.kind() {
                        VobKindRef::$variant(value) => Ok(value),
                        _ => Err(bad(ErrorKind::WrongObjectType, $context)),
                    },
                    _ => Err(bad(ErrorKind::WrongObjectType, $context)),
                }
            }
            #[doc = concat!("Edit the exact `", stringify!($variant), "` subtype. Borrowing does not allocate, clone, validate the draft or change its class. Error behavior matches [`Self::", stringify!($read), "`].")]
            #[inline]
            pub fn $edit(&mut self, id: ObjectId) -> Result<&mut $ty> {
                match self.get_mut(id)? {
                    ObjectMut::Vob(value) => match value.into_parts().1 {
                        VobKindMut::$variant(value) => Ok(value),
                        _ => Err(bad(ErrorKind::WrongObjectType, $context)),
                    },
                    _ => Err(bad(ErrorKind::WrongObjectType, $context)),
                }
            }
        }
    };
}
vob_accessor!(item, item_mut, Item, Item, "Item graph access");
vob_accessor!(
    container,
    container_mut,
    Container,
    Container,
    "Container graph access"
);

macro_rules! object_accessor {
    ($read:ident, $edit:ident, $variant:ident, $ty:ty, $context:literal) => {
        impl Graph {
            #[doc = concat!("Borrow the exact `", stringify!($variant), "` schema. A foreign handle returns `InvalidHandle`; another schema returns `WrongObjectType`.")]
            #[inline]
            pub fn $read(&self, id: ObjectId) -> Result<&$ty> {
                match self.get(id)? {
                    ObjectRef::$variant(value) => Ok(value),
                    _ => Err(bad(ErrorKind::WrongObjectType, $context)),
                }
            }
            #[doc = concat!("Edit the exact `", stringify!($variant), "` schema without allocation or cloning. Draft validation remains explicit. Error behavior matches [`Self::", stringify!($read), "`].")]
            #[inline]
            pub fn $edit(&mut self, id: ObjectId) -> Result<&mut $ty> {
                match self.get_mut(id)? {
                    ObjectMut::$variant(value) => Ok(value),
                    _ => Err(bad(ErrorKind::WrongObjectType, $context)),
                }
            }
        }
    };
}
object_accessor!(
    material,
    material_mut,
    Material,
    Material,
    "Material graph access"
);
object_accessor!(world, world_mut, World, world::World, "World graph access");
