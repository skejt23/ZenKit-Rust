//! Cutscene libraries backed by the shared owner-checked object graph.
//!
//! Libraries retain block occurrence order and aliases. Lookup is exact and
//! returns the first matching raw name.
//! Read/write through [`crate::objects::ObjectArchive`] in all archive encodings.
//! Graph cycles are valid storage, but message convenience traversal reports
//! [`ErrorKind::CycleDetected`]. Helpers check their links; draft edits still need
//! whole-document validation.
//! Libraries require `zCCSLib` version zero and Block members. An atomic block's
//! message may be null. Runtime state records use version zero in both games and
//! normal/save modes. Context stages accept any supported object.
//!
//! ```
//! use zenkit::{GameVersion, archive::Format, cutscene::Library, objects::*};
//! let mut graph = Graph::new()?;
//! let library = graph.insert(Object::Library(Library::default()))?;
//! graph.add_root(Link::new(b"library".to_vec(), Some(library)))?;
//! let archive = ObjectArchive { metadata: Metadata::new(Format::BinSafe),
//!     game: GameVersion::Gothic2, graph };
//! let bytes = archive.to_bytes(Limits::default())?;
//! assert_eq!(ObjectArchive::from_bytes(&bytes, archive.game, Limits::default())?.graph.len(), 1);
//! # Ok::<(), zenkit::Error>(())
//! ```
// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use crate::{
    Error, ErrorKind, Result,
    objects::{ConversationMessage, Graph, Link, ObjectId, ObjectMut, ObjectRef},
};
use std::fmt;
mod state;
pub use state::*;
fn error(kind: ErrorKind) -> Error {
    Error::operation(kind, "cutscene graph traversal")
}
/// Ordered non-null links to CutsceneBlock nodes. Multiple occurrences can share
/// one block, and duplicate names remain distinct; the vector is never sorted.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Library {
    pub blocks: Vec<Link>,
}
impl fmt::Debug for Library {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Library")
            .field("blocks", &self.blocks.len())
            .finish()
    }
}
impl Library {
    /// Exact byte-name lookup, returning the first matching occurrence in wire
    /// order. Checks every library member's handle and Block kind, but does not
    /// validate block descendants or other graph nodes. No allocation or sorting.
    pub fn block_by_name(&self, graph: &Graph, name: &[u8]) -> Result<Option<ObjectId>> {
        let mut found = None;
        for link in &self.blocks {
            let id = link.target.ok_or_else(|| error(ErrorKind::InvalidData))?;
            let ObjectRef::Block(block) = graph.get(id)? else {
                return Err(error(ErrorKind::WrongObjectType));
            };
            if found.is_none() && block.name == name {
                found = Some(id);
            }
        }
        Ok(found)
    }
}
fn terminal_atomic(graph: &Graph, start: ObjectId) -> Result<ObjectId> {
    let mut id = match graph.get(start)? {
        ObjectRef::Cutscene(block) => block
            .child
            .target
            .ok_or_else(|| error(ErrorKind::InvalidData))?,
        ObjectRef::CutsceneContext(context) => context
            .block
            .child
            .target
            .ok_or_else(|| error(ErrorKind::InvalidData))?,
        _ => start,
    };
    // Validate each visited handle before the bounded-walk check: a foreign final
    // edge must remain InvalidHandle rather than being mislabeled as a cycle.
    for hops in 0..=graph.len() {
        match graph.get(id)? {
            ObjectRef::Atomic(_) => return Ok(id),
            ObjectRef::Block(block) => {
                if hops == graph.len() {
                    return Err(error(ErrorKind::CycleDetected));
                }
                id = block
                    .child
                    .target
                    .ok_or_else(|| error(ErrorKind::InvalidData))?;
            }
            _ => return Err(error(ErrorKind::WrongObjectType)),
        }
    }
    unreachable!("bounded graph walk")
}
/// Resolve a block/atomic chain to its message ID. The initial root may also
/// be Cutscene or Context; their actual child path still accepts only exact
/// Atomic/Block nodes, never a derived Cutscene/Context descendant. A null atomic message is
/// `Ok(None)`; invalid handles, wrong kinds, null block children and cycles are
/// errors. The allocation-free walk is bounded by the number of graph nodes.
pub fn message_id(graph: &Graph, start: ObjectId) -> Result<Option<ObjectId>> {
    let id = terminal_atomic(graph, start)?;
    let ObjectRef::Atomic(atomic) = graph.get(id)? else {
        unreachable!()
    };
    if let Some(message) = atomic.message.target
        && !matches!(graph.get(message)?, ObjectRef::Message(_))
    {
        return Err(error(ErrorKind::WrongObjectType));
    }
    Ok(atomic.message.target)
}
/// Borrow the resolved message without copying its raw text/audio name.
pub fn message(graph: &Graph, start: ObjectId) -> Result<Option<&ConversationMessage>> {
    message_id(graph, start)?
        .map(|id| match graph.get(id)? {
            ObjectRef::Message(v) => Ok(v),
            _ => unreachable!(),
        })
        .transpose()
}
/// Replace the terminal atomic message target, preserving its occurrence name.
/// New target and complete block-to-atomic path are checked before mutation; any
/// error leaves the graph untouched. `None` explicitly removes the message. All
/// aliases of that atomic node observe the change. Other graph nodes are not
/// implicitly validated, and a bad old message target may be repaired this way.
pub fn set_message(graph: &mut Graph, start: ObjectId, target: Option<ObjectId>) -> Result<()> {
    if let Some(id) = target
        && !matches!(graph.get(id)?, ObjectRef::Message(_))
    {
        return Err(error(ErrorKind::WrongObjectType));
    }
    let atomic = terminal_atomic(graph, start)?;
    let ObjectMut::Atomic(v) = graph.get_mut(atomic)? else {
        unreachable!()
    };
    v.message.target = target;
    Ok(())
}
