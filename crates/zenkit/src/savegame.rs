//! Complete SAVEINFO/SAVEDAT storage and bounded save-directory snapshots.
//!
//! SAVEDAT scalar fields surround one shared-graph manager occurrence. Its
//! archive flag is retained, not used to invent an absent normal-mode schema.
//! Raw strings remain bytes; counts, identities and game variants are checked.
//! `objects::Limits` applies across the complete SAVEDAT session: `copied_bytes`
//! includes metadata, all raw state strings, the 1,764 guild bytes and any tail;
//! `sequence_items` includes infos, missions, history records, log topics/entries,
//! symbols and G2 symbol values. Graph occurrences use the shared link budget.
// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2022-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
pub(crate) mod codec;
mod directory;
use crate::{
    Error, ErrorKind, GameVersion, Result,
    archive::Format,
    cutscene::Manager,
    objects::{self, Graph, Link, Metadata, ObjectMut, ObjectRef},
};
pub use directory::*;
use std::fmt;

pub(crate) const CTX: &str = "savegame";
pub(crate) fn bad(kind: ErrorKind) -> Error {
    Error::operation(kind, CTX)
}
#[derive(Clone, Default, PartialEq)]
pub struct Gothic2Metadata {
    pub version_point: i32,
    pub version_int: i32,
    pub application: Vec<u8>,
}
impl fmt::Debug for Gothic2Metadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Gothic2Metadata")
            .field("version_point", &self.version_point)
            .field("version_int", &self.version_int)
            .field("application_bytes", &self.application.len())
            .finish()
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum MetadataVersion {
    Gothic1,
    Gothic2(Gothic2Metadata),
}
#[derive(Clone, PartialEq)]
pub struct SaveMetadata {
    pub title: Vec<u8>,
    pub world: Vec<u8>,
    pub day: i32,
    pub hour: i32,
    pub minute: i32,
    pub save_date: Vec<u8>,
    pub version_major: i32,
    pub version_minor: i32,
    pub play_time_seconds: i32,
    pub version: MetadataVersion,
}
impl SaveMetadata {
    pub fn new(game: GameVersion) -> Self {
        Self {
            title: Vec::new(),
            world: Vec::new(),
            day: 0,
            hour: 0,
            minute: 0,
            save_date: Vec::new(),
            version_major: 0,
            version_minor: 0,
            play_time_seconds: 0,
            version: match game {
                GameVersion::Gothic1 => MetadataVersion::Gothic1,
                GameVersion::Gothic2 => MetadataVersion::Gothic2(Default::default()),
            },
        }
    }
    pub fn game(&self) -> GameVersion {
        match self.version {
            MetadataVersion::Gothic1 => GameVersion::Gothic1,
            MetadataVersion::Gothic2(_) => GameVersion::Gothic2,
        }
    }
    pub(crate) fn buffers(&self) -> impl Iterator<Item = &[u8]> {
        [&self.title[..], &self.world, &self.save_date]
            .into_iter()
            .chain(match &self.version {
                MetadataVersion::Gothic2(g) => Some(g.application.as_slice()),
                _ => None,
            })
    }
}
impl fmt::Debug for SaveMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SaveMetadata")
            .field("title_bytes", &self.title.len())
            .field("world_bytes", &self.world.len())
            .field("save_date_bytes", &self.save_date.len())
            .field("day", &self.day)
            .field("hour", &self.hour)
            .field("minute", &self.minute)
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Info {
    pub name: Vec<u8>,
    pub told: bool,
}
impl fmt::Debug for Info {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Info")
            .field("name_bytes", &self.name.len())
            .field("told", &self.told)
            .finish()
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MissionHistory {
    pub vob_id: i32,
    pub status: i32,
    pub start_time: f32,
}
#[derive(Clone, Default, PartialEq)]
pub struct Mission {
    pub name: Vec<u8>,
    pub id: i32,
    pub available: bool,
    pub history: Vec<MissionHistory>,
    pub status_index: i32,
}
impl fmt::Debug for Mission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mission")
            .field("name_bytes", &self.name.len())
            .field("id", &self.id)
            .field("available", &self.available)
            .field("history", &self.history.len())
            .field("status_index", &self.status_index)
            .finish()
    }
}
macro_rules! raw_enum {($name:ident,$($constant:ident=$value:expr),* $(,)?)=>{
    #[derive(Clone,Copy,Debug,Default,PartialEq,Eq)] pub struct $name(u32);
    impl $name {$(pub const $constant:Self=Self($value);)* pub const fn from_raw(value:u32)->Self{Self(value)} pub const fn raw(self)->u32{self.0}}
    impl From<u32> for $name {fn from(value:u32)->Self{Self(value)}}
    impl From<$name> for u32 {fn from(value:$name)->Self{value.0}}
};}
raw_enum!(TopicSection, MISSIONS = 0, NOTES = 1);
raw_enum!(
    TopicStatus,
    FREE = 0,
    RUNNING = 1,
    SUCCESS = 2,
    FAILURE = 3,
    OBSOLETE = 4
);
#[derive(Clone, Default, PartialEq, Eq)]
pub struct LogTopic {
    pub description: Vec<u8>,
    pub section: TopicSection,
    pub status: TopicStatus,
    pub entries: Vec<Vec<u8>>,
    /// Independent redundant field; no equality/nonnegative invariant is inferred.
    pub manager_entry_count: i32,
}
impl fmt::Debug for LogTopic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LogTopic")
            .field("description_bytes", &self.description.len())
            .field("section", &self.section)
            .field("status", &self.status)
            .field("entries", &self.entries.len())
            .field("manager_entry_count", &self.manager_entry_count)
            .finish()
    }
}
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Symbol<T> {
    pub name: Vec<u8>,
    pub value: T,
}
impl<T: fmt::Debug> fmt::Debug for Symbol<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Symbol")
            .field("name_bytes", &self.name.len())
            .field("value", &self.value)
            .finish()
    }
}
/// Coherent tagged spelling for one G2 symbol. Empty/singleton and untagged
/// groups parse as Indexed because no distinction exists on their wire.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ValueNames {
    #[default]
    Indexed,
    RepeatedZero,
}
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SymbolValues {
    pub values: Vec<i32>,
    pub names: ValueNames,
}
impl fmt::Debug for SymbolValues {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SymbolValues")
            .field("values", &self.values.len())
            .field("names", &self.names)
            .finish()
    }
}
#[derive(Clone, PartialEq, Eq)]
pub enum Symbols {
    Gothic1(Vec<Symbol<i32>>),
    Gothic2(Vec<Symbol<SymbolValues>>),
}
impl fmt::Debug for Symbols {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gothic1(v) => f.debug_tuple("Gothic1").field(&v.len()).finish(),
            Self::Gothic2(v) => f.debug_tuple("Gothic2").field(&v.len()).finish(),
        }
    }
}
#[derive(Clone, PartialEq)]
pub struct StateData {
    pub day: i32,
    pub hour: i32,
    pub minute: i32,
    pub infos: Vec<Info>,
    pub missions: Vec<Mission>,
    pub log: Vec<LogTopic>,
    pub symbols: Symbols,
    pub guild_attitudes: [[u8; 42]; 42],
    /// Complete extra bytes of a tagged guildTable. BINARY output rejects a tail.
    pub guild_tail: Vec<u8>,
}
impl StateData {
    pub fn new(game: GameVersion) -> Self {
        Self {
            day: 0,
            hour: 0,
            minute: 0,
            infos: Vec::new(),
            missions: Vec::new(),
            log: Vec::new(),
            symbols: match game {
                GameVersion::Gothic1 => Symbols::Gothic1(Vec::new()),
                GameVersion::Gothic2 => Symbols::Gothic2(Vec::new()),
            },
            guild_attitudes: [[0; 42]; 42],
            guild_tail: Vec::new(),
        }
    }
}
impl fmt::Debug for StateData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StateData")
            .field("day", &self.day)
            .field("hour", &self.hour)
            .field("minute", &self.minute)
            .field("infos", &self.infos.len())
            .field("missions", &self.missions.len())
            .field("log", &self.log.len())
            .field("symbols", &self.symbols)
            .field("guild_tail_bytes", &self.guild_tail.len())
            .finish()
    }
}
/// Editable SAVEDAT document. Exactly one graph root is its nullable exact
/// CutsceneManager occurrence; no second copy of that Link lives in `fields`.
/// Direct edits can make an invalid draft; validation includes all orphan nodes.
#[derive(Debug)]
pub struct StateArchive {
    pub metadata: Metadata,
    pub game: GameVersion,
    pub fields: StateData,
    pub graph: Graph,
}
impl StateArchive {
    pub fn new(game: GameVersion, format: Format) -> Result<Self> {
        let mut graph = Graph::new()?;
        graph.add_root(Link::default())?;
        let mut metadata = Metadata::new(format);
        metadata.save_game = true;
        Ok(Self {
            metadata,
            game,
            fields: StateData::new(game),
            graph,
        })
    }
    pub fn manager(&self) -> Result<Option<&Manager>> {
        let [link] = self.graph.roots() else {
            return Err(bad(ErrorKind::InvalidData));
        };
        link.target
            .map(|id| match self.graph.get(id)? {
                ObjectRef::CutsceneManager(v) => Ok(v),
                _ => Err(bad(ErrorKind::WrongObjectType)),
            })
            .transpose()
    }
    pub fn from_bytes(bytes: &[u8], game: GameVersion, limits: objects::Limits) -> Result<Self> {
        codec::read(bytes, game, limits)
    }
    pub fn validate(&self, limits: objects::Limits) -> Result<()> {
        self.manager()?;
        let (copied, sequences) = codec::validate(&self.fields, self.game, limits)?;
        objects::validate_parts(
            &self.metadata,
            self.game,
            &self.graph,
            objects::Limits {
                copied_bytes: limits.copied_bytes - copied,
                sequence_items: limits.sequence_items - sequences,
                ..limits
            },
        )
    }
    pub fn to_bytes(&self, limits: objects::Limits) -> Result<Vec<u8>> {
        self.validate(limits)?;
        codec::write(self, limits)
    }
}
