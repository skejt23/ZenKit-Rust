// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;
use crate::world::{Hierarchy, HierarchyEntry};

const FAILED: &str = "failed object archive session";
const TREE: &str = "standalone VOB tree";

/// Schema-directed archive composition with one shared object-identity table.
///
/// Object and tree occurrences are returned to the caller; the resulting graph's
/// root list is empty. Keep the returned occurrences with the graph. They retain
/// valid owner-bound IDs when the graph is moved. Unknown classes and unresolved
/// future references use the same policy as [`ObjectArchive`].
///
/// A propagated error or callback panic poisons the session. Later operations,
/// finalization and extraction then fail; discard it. Earlier returned values
/// are provisional until successful finalization/extraction validates the graph.
///
/// ```
/// use zenkit::{GameVersion, archive::{Format, Value}, objects::*, vob::Vob,
///     world::{Hierarchy, HierarchyEntry}};
/// let game = GameVersion::Gothic2;
/// let limits = Limits::default();
/// let mut graph = Graph::new()?;
/// let id = graph.insert(Object::Vob(Vob::new(game)))?;
/// let object = Link::new(b"vob".to_vec(), Some(id));
/// let tree = Hierarchy { root_count_name: None, entries: vec![HierarchyEntry {
///     object: object.clone(), parent: None, children_name: None,
/// }] };
/// let mut writer = WriteSession::new(&Metadata::new(Format::BinSafe), game, &graph, limits)?;
/// writer.write_object(&object)?;
/// writer.with_fields(|w| w.write(b"between", Value::Int(42)))?;
/// writer.write_vob_tree(&tree)?; // References the same definition.
/// let bytes = writer.finish()?;
/// let mut reader = ReadSession::new(&bytes, game, limits)?;
/// let first = reader.read_object()?.unwrap();
/// assert_eq!(reader.with_fields(|r| r.read_int())?, 42);
/// let tree = reader.read_vob_tree()?;
/// assert_eq!(first.target, tree.entries[0].object.target);
/// let graph = reader.finish()?;
/// assert_eq!(graph.len(), 1);
/// assert!(matches!(graph.get(first.target.unwrap())?, ObjectRef::Vob(_)));
/// # Ok::<(), zenkit::Error>(())
/// ```
pub struct ReadSession<'a> {
    parser: Parser<'a>,
    metadata: Metadata,
    roots: usize,
    entry_depth: usize,
    failed: bool,
}
impl fmt::Debug for ReadSession<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReadSession")
            .field("position", &self.parser.reader.position())
            .field("roots", &self.roots)
            .field("failed", &self.failed)
            .finish_non_exhaustive()
    }
}
impl<'a> ReadSession<'a> {
    pub fn new(bytes: &'a [u8], game: GameVersion, limits: Limits) -> Result<Self> {
        Self::from_reader(archive::Reader::new(bytes, limits.archive)?, game, limits)
    }

    /// Take an already-open archive without resetting its cursor, frames, limits
    /// or counters. `limits.archive` must equal the reader's configured limits.
    /// Embedded readers retain physical offsets and their enclosing address space.
    /// The identity cache starts empty: all definitions/references that need
    /// shared identity must pass through this same session, not primitive reads.
    pub fn from_reader(
        reader: archive::Reader<'a>,
        game: GameVersion,
        limits: Limits,
    ) -> Result<Self> {
        let entry_depth = reader.depth();
        let (metadata, parser) = Parser::from_reader(reader, game, limits)?;
        Ok(Self {
            parser,
            metadata,
            roots: 0,
            entry_depth,
            failed: false,
        })
    }
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    /// Inspect completed nodes while the session is healthy. The graph remains
    /// provisional until finalization/extraction performs deferred validation.
    pub fn graph(&self) -> Result<&Graph> {
        self.check()?;
        Ok(&self.parser.graph)
    }
    fn check(&self) -> Result<()> {
        if self.failed {
            Err(at(
                ErrorKind::InvalidData,
                self.parser.reader.position(),
                FAILED,
            ))
        } else {
            Ok(())
        }
    }
    fn operation<T>(&mut self, operation: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        self.check()?;
        self.failed = true;
        let value = operation(self)?;
        self.failed = false;
        Ok(value)
    }
    /// Compose caller-defined fields/frames or inline codecs such as RigidBody.
    /// Keep the same archive; do not replace it or use primitive class records to
    /// create graph identities. Preserve pre-existing enclosing frames when using
    /// `into_parts`. The archive bounds custom fields; the caller budgets any
    /// retained custom values outside the graph. An error or panic poisons the
    /// session, including a panic caught by the caller outside this method.
    pub fn with_fields<T>(
        &mut self,
        read: impl FnOnce(&mut archive::Reader<'a>) -> Result<T>,
    ) -> Result<T> {
        self.operation(|s| read(&mut s.parser.reader))
    }
    fn root(&mut self, p: usize) -> Result<()> {
        self.roots = self
            .roots
            .checked_add(1)
            .filter(|n| *n <= self.parser.limits.roots)
            .ok_or_else(|| at(ErrorKind::LimitExceeded, p, "session root occurrences"))?;
        Ok(())
    }
    /// Read one object occurrence. Tagged formats return `None` when the next
    /// record is not an object. BINARY has no generic record discriminator: call
    /// only where the enclosing schema expects an object; primitive bytes can
    /// fail or resemble an object header. A wire null is `Some(Link { target:
    /// None, .. })`. Each successful
    /// occurrence, including a null or alias, consumes one cumulative root slot.
    pub fn read_object(&mut self) -> Result<Option<Link>> {
        self.operation(|s| {
            let p = s.parser.reader.position();
            let Some(header) = s.parser.reader.begin_object()? else {
                return Ok(None);
            };
            s.root(p)?;
            s.parser.open(header, p).map(Some)
        })
    }
    /// Read exactly one bare VOB tree: one object and its child count, followed
    /// by its descendants in preorder. No world wrapper or root-count field is
    /// consumed. Null occurrences and their descendants are preserved; non-VOB
    /// targets are rejected. Child-count spellings are retained in tagged input.
    /// One tree consumes one root slot; every object/link and name uses the shared
    /// graph budgets. Hierarchy depth is independent from object framing depth.
    pub fn read_vob_tree(&mut self) -> Result<Hierarchy> {
        self.operation(|s| {
            let p = s.parser.reader.position();
            s.root(p)?;
            let limits = s.parser.limits;
            if limits.world.hierarchy_depth == 0 {
                return Err(at(ErrorKind::LimitExceeded, p, TREE));
            }
            let mut tree = Hierarchy::default();
            let mut stack: Vec<(usize, usize)> = Vec::new();
            let mut pending = 1usize;
            while pending > 0 {
                let p = s.parser.reader.position();
                let parent = stack.last_mut().map(|(index, remaining)| {
                    *remaining -= 1;
                    *index
                });
                let header = s
                    .parser
                    .reader
                    .begin_object()?
                    .ok_or_else(|| at(ErrorKind::InvalidData, p, TREE))?;
                let object = s.parser.open(header, p)?;
                s.parser
                    .graph
                    .world_edge(&object, world::codec::Edge::Vob)
                    .map_err(|e| at(e.kind, p, TREE))?;
                let left = limits.links - s.parser.links;
                let count_pos = s.parser.reader.position();
                let (children, children_name) = world::codec::read_children(
                    &mut s.parser.reader,
                    &mut s.parser.copied,
                    limits.copied_bytes,
                    left,
                )?;
                pending = (pending - 1)
                    .checked_add(children)
                    .filter(|n| *n <= left)
                    .ok_or_else(|| at(ErrorKind::LimitExceeded, count_pos, TREE))?;
                let index = tree.entries.len();
                reserve(&mut tree.entries, 1).map_err(|e| at(e.kind, p, TREE))?;
                tree.entries.push(HierarchyEntry {
                    object,
                    parent,
                    children_name,
                });
                if children > 0 {
                    if stack.len() + 1 >= limits.world.hierarchy_depth {
                        return Err(at(ErrorKind::LimitExceeded, count_pos, TREE));
                    }
                    reserve(&mut stack, 1).map_err(|e| at(e.kind, count_pos, TREE))?;
                    stack.push((index, children));
                }
                while stack.last().is_some_and(|(_, remaining)| *remaining == 0) {
                    stack.pop();
                }
            }
            Ok(tree)
        })
    }
    /// Validate graph closure and return the unchanged current archive position
    /// plus its owned graph. The depth must match constructor entry depth: caller
    /// frames may remain open, but extra session frames must be closed. No EOF is
    /// required and following primitive fields remain unread. The identity cache
    /// ends here; starting another session cannot reference earlier definitions.
    pub fn into_parts(self) -> Result<(archive::Reader<'a>, Graph)> {
        self.check()?;
        if self.parser.reader.depth() != self.entry_depth {
            return Err(at(
                ErrorKind::InvalidData,
                self.parser.reader.position(),
                "session enclosing frames",
            ));
        }
        self.parser.into_parts()
    }
    /// Validate the graph and finish the entire archive (closed frames and EOF).
    /// Use `into_parts` when a caller-owned frame or suffix remains.
    pub fn finish(self) -> Result<Graph> {
        self.check()?;
        self.parser.finish()
    }
}

/// Compose objects and bare trees using one writer and reference cache.
///
/// The entire source graph, including orphans and its unused root list, is
/// validated once. Graph roots are not emitted automatically. Each explicit
/// object/tree occurrence then charges additional links and retained names;
/// these charges accumulate across calls. The root-call limit is independent
/// from the graph's source root-list limit. Primitive custom fields use the
/// archive's own limits; their caller-owned allocations are outside graph budgets.
/// A propagated error or panic poisons the session; discard it.
pub struct WriteSession<'a> {
    encoder: Encoder<'a>,
    usage: Usage,
    roots: usize,
    entry_depth: usize,
    failed: bool,
}
impl fmt::Debug for WriteSession<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WriteSession")
            .field("output", &self.encoder.output)
            .field("roots", &self.roots)
            .field("failed", &self.failed)
            .finish_non_exhaustive()
    }
}
impl<'a> WriteSession<'a> {
    pub fn new(
        metadata: &Metadata,
        game: GameVersion,
        graph: &'a Graph,
        limits: Limits,
    ) -> Result<Self> {
        Self::from_writer(
            archive::Writer::new(metadata.as_header(), limits.archive)?,
            game,
            graph,
            limits,
        )
    }
    /// Take an already-open writer, retaining its framing, counters and actual
    /// save/XZEN header context. `limits.archive` must match its configured limits.
    /// The identity cache starts empty; manually emitted class records cannot
    /// seed it. Use one session for all graph definitions and references.
    pub fn from_writer(
        output: archive::Writer,
        game: GameVersion,
        graph: &'a Graph,
        limits: Limits,
    ) -> Result<Self> {
        let (save, wide_g1, metadata_bytes, archive_limits) = output.session_header();
        if archive_limits != limits.archive {
            return Err(bad(
                ErrorKind::InvalidData,
                "archive session limits mismatch",
            ));
        }
        let entry_depth = output.depth();
        let copied_bytes = limits
            .copied_bytes
            .checked_sub(metadata_bytes)
            .ok_or_else(|| bad(ErrorKind::LimitExceeded, "object copied bytes"))?;
        let mut usage = graph.validate_usage(
            save,
            Limits {
                copied_bytes,
                ..limits
            },
        )?;
        usage.copied += metadata_bytes;
        validate_game(graph, game, wide_g1)?;
        Ok(Self {
            encoder: Encoder {
                output,
                graph,
                game,
                ids: HashMap::new(),
                limits,
            },
            usage,
            roots: 0,
            entry_depth,
            failed: false,
        })
    }
    fn check(&self) -> Result<()> {
        if self.failed {
            Err(bad(ErrorKind::InvalidData, FAILED))
        } else {
            Ok(())
        }
    }
    fn operation<T>(&mut self, operation: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        self.check()?;
        self.failed = true;
        let value = operation(self)?;
        self.failed = false;
        Ok(value)
    }
    /// Compose custom fields/frames or inline codecs with the same archive.
    /// Do not replace the writer or manually emit game-class definitions that
    /// require shared graph identity. Preserve caller frames for `into_writer`.
    /// Propagated errors and callback panics poison the session.
    pub fn with_fields<T>(
        &mut self,
        write: impl FnOnce(&mut archive::Writer) -> Result<T>,
    ) -> Result<T> {
        self.operation(|s| write(&mut s.encoder.output))
    }
    fn root(&mut self) -> Result<()> {
        self.roots = self
            .roots
            .checked_add(1)
            .filter(|n| *n <= self.encoder.limits.roots)
            .ok_or_else(|| bad(ErrorKind::LimitExceeded, "session root occurrences"))?;
        Ok(())
    }
    fn charge(&mut self, link: &Link) -> Result<()> {
        link_count(&mut self.usage.links, 1, self.encoder.limits.links)?;
        count(
            &mut self.usage.copied,
            link.name.len(),
            self.encoder.limits.copied_bytes,
        )?;
        if let Some(id) = link.target {
            self.encoder.graph.slot(id)?;
        }
        Ok(())
    }
    /// Emit one named occurrence. Repeated targets reference earlier definitions;
    /// temporary Links are accepted and need not live for the session's lifetime.
    pub fn write_object(&mut self, object: &Link) -> Result<()> {
        self.operation(|s| {
            s.root()?;
            s.charge(object)?;
            s.encoder.link(object)
        })
    }
    /// Emit exactly one bare VOB tree, without a world wrapper/root-count field.
    /// The hierarchy must be nonempty, have exactly one root at entry zero, and
    /// have `root_count_name == None`. All targets must be VOBs or explicit nulls.
    /// Preorder topology and hierarchy depth are checked before output. Tagged
    /// child-count names are preserved; missing names use `childs0`, `childs1`, ...
    /// starting anew for each tree. BINARY omits field names as usual.
    pub fn write_vob_tree(&mut self, tree: &Hierarchy) -> Result<()> {
        self.operation(|s| {
            s.root()?;
            if tree.root_count_name.is_some()
                || tree.entries.is_empty()
                || tree.entries[0].parent.is_some()
                || tree.entries[1..].iter().any(|e| e.parent.is_none())
            {
                return Err(bad(ErrorKind::InvalidData, TREE));
            }
            if tree.entries.len() > s.encoder.limits.links.saturating_sub(s.usage.links) {
                return Err(bad(ErrorKind::LimitExceeded, TREE));
            }
            for entry in &tree.entries {
                s.charge(&entry.object)?;
                s.encoder
                    .graph
                    .world_edge(&entry.object, world::codec::Edge::Vob)?;
                if let Some(name) = &entry.children_name {
                    count(
                        &mut s.usage.copied,
                        name.len(),
                        s.encoder.limits.copied_bytes,
                    )?;
                }
            }
            let counts =
                world::codec::hierarchy_counts(tree, s.encoder.limits.world.hierarchy_depth)?;
            for (ordinal, (entry, children)) in tree.entries.iter().zip(counts).enumerate() {
                s.encoder.link(&entry.object)?;
                let canonical;
                let name = if let Some(name) = &entry.children_name {
                    name.as_slice()
                } else {
                    canonical = world::codec::ChildName::new(ordinal);
                    canonical.bytes()
                };
                let children =
                    i32::try_from(children).map_err(|_| bad(ErrorKind::LimitExceeded, TREE))?;
                s.encoder
                    .output
                    .write(name, archive::Value::Int(children))?;
            }
            Ok(())
        })
    }
    /// Return a healthy writer at its current position, preserving caller frames
    /// and allowing a suffix. Depth must match constructor entry depth. The
    /// identity cache ends here; a later session starts new object definitions.
    pub fn into_writer(self) -> Result<archive::Writer> {
        self.check()?;
        if self.encoder.output.depth() != self.entry_depth {
            return Err(bad(ErrorKind::InvalidData, "session enclosing frames"));
        }
        Ok(self.encoder.output)
    }
    /// Finalize the complete archive, requiring all frames to be closed.
    pub fn finish(self) -> Result<Vec<u8>> {
        self.check()?;
        self.encoder.finish()
    }
}
