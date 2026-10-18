// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
//! Composition uses persistent metadata and shared payload ownership. Public views
//! borrow the tree, so mutation cannot leave a dangling node handle.
use super::{
    Compression, Contents, DIRECTORY, HEADER, LAST, RECORD, ReadOptions, SIGNATURES, Vdf,
    compression, raw_audio, space,
};
pub use crate::io::HostNameEncoding;
use crate::{Error, ErrorKind, GameVersion, Limits, Result, io};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt,
    path::Path,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_DEPTH: usize = 256;

fn limited(value: usize, maximum: usize, context: &'static str) -> Result<()> {
    if value > maximum {
        return Err(error(ErrorKind::LimitExceeded, context));
    }
    Ok(())
}

fn error(kind: ErrorKind, context: &'static str) -> Error {
    Error::operation(kind, context)
}

/// Unix seconds interpreted as UTC. Node timestamps may precede 1980; VDF
/// output requires 1980..=2107 and floors odd seconds to DOS's two-second tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(i64);

impl Timestamp {
    /// Construct an instant from any signed Unix second count, including `-1`.
    /// Missing node metadata is `None`, never a sentinel instant.
    pub const fn from_unix_seconds(seconds: i64) -> Self {
        Self(seconds)
    }
    /// Return signed whole seconds since 1970-01-01 00:00:00 UTC.
    pub const fn unix_seconds(self) -> i64 {
        self.0
    }

    /// Convert a system clock instant, flooring fractional seconds toward the past.
    ///
    /// # Errors
    /// Returns [`ErrorKind::InvalidData`] if whole seconds do not fit in `i64`.
    pub fn from_system_time(time: SystemTime) -> Result<Self> {
        let seconds = match time.duration_since(UNIX_EPOCH) {
            Ok(d) => i64::try_from(d.as_secs()).ok(),
            Err(e) => i64::try_from(e.duration().as_secs())
                .ok()
                .and_then(|v| v.checked_add(i64::from(e.duration().subsec_nanos() != 0)))
                .and_then(i64::checked_neg),
        };
        seconds
            .map(Self)
            .ok_or_else(|| error(ErrorKind::InvalidData, "VFS timestamp range"))
    }

    /// Decode a packed DOS timestamp as UTC with Gregorian calendar validation.
    ///
    /// # Errors
    /// Returns [`ErrorKind::InvalidData`] for invalid month/day/clock fields.
    /// Zero is not an absent timestamp; it is an invalid DOS date.
    pub fn from_dos(dos: u32) -> Result<Self> {
        let year = 1980 + (dos >> 25) as i64;
        let month = ((dos >> 21) & 15) as usize;
        let day = ((dos >> 16) & 31) as i64;
        let hour = ((dos >> 11) & 31) as i64;
        let minute = ((dos >> 5) & 63) as i64;
        let second = ((dos & 31) * 2) as i64;
        if !(1..=12).contains(&month)
            || day == 0
            || day > days_in_month(year, month)
            || hour > 23
            || minute > 59
            || second > 59
        {
            return Err(error(ErrorKind::InvalidData, "VFS DOS calendar timestamp"));
        }
        let days: i64 = (1970..year).map(days_in_year).sum::<i64>()
            + (1..month).map(|m| days_in_month(year, m)).sum::<i64>()
            + day
            - 1;
        Ok(Self(days * 86400 + hour * 3600 + minute * 60 + second))
    }

    /// Encode as UTC DOS date/time, flooring odd seconds to the two-second tick.
    ///
    /// # Errors
    /// Returns [`ErrorKind::InvalidData`] outside 1980-01-01 through 2107-12-31.
    /// There is no dependency on the host timezone or current time.
    ///
    /// ```
    /// use zenkit::vfs::Timestamp;
    /// let time = Timestamp::from_unix_seconds(1_728_907_201);
    /// let rounded = Timestamp::from_dos(time.to_dos()?)?;
    /// assert_eq!(rounded.unix_seconds(), 1_728_907_200);
    /// assert!(Timestamp::from_unix_seconds(-1).to_dos().is_err());
    /// # Ok::<(), zenkit::Error>(())
    /// ```
    pub fn to_dos(self) -> Result<u32> {
        // Reject before doing calendar work; arbitrary i64 inputs remain constant work.
        if self.0 < 315_532_800 || self.0 >= 4_354_819_200 {
            return Err(error(
                ErrorKind::InvalidData,
                "VFS timestamp outside DOS 1980..2107",
            ));
        }
        let mut days = self.0 / 86400;
        let mut year = 1970;
        while days >= days_in_year(year) {
            days -= days_in_year(year);
            year += 1;
        }
        let mut month = 1;
        while days >= days_in_month(year, month) {
            days -= days_in_month(year, month);
            month += 1;
        }
        let seconds = self.0 % 86400;
        Ok(((year as u32 - 1980) << 25)
            | ((month as u32) << 21)
            | (((days + 1) as u32) << 16)
            | (((seconds / 3600) as u32) << 11)
            | ((((seconds % 3600) / 60) as u32) << 5)
            | ((seconds % 60) as u32 / 2))
    }
}

fn days_in_year(year: i64) -> i64 {
    if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
        366
    } else {
        365
    }
}
fn days_in_month(year: i64, month: usize) -> i64 {
    match month {
        2 => 28 + i64::from(days_in_year(year) == 366),
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Collision handling for files and file/directory conflicts.
///
/// Two directories always merge, preserving the existing directory's metadata.
/// Conditional policies replace only when **both** timestamps are known and
/// the strict comparison succeeds; equal or unknown timestamps retain existing
/// data. A new name is inserted regardless of the policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overwrite {
    /// Retain the existing conflicting node.
    KeepExisting,
    /// Replace the existing conflicting node regardless of timestamps.
    ReplaceExisting,
    /// Replace only when the existing timestamp is later than the incoming one.
    ReplaceIfExistingNewer,
    /// Replace only when the existing timestamp is earlier than the incoming one.
    ReplaceIfExistingOlder,
}

impl Overwrite {
    fn replaces(self, existing: Option<Timestamp>, incoming: Option<Timestamp>) -> bool {
        match (self, existing, incoming) {
            (Self::ReplaceExisting, _, _) => true,
            (Self::ReplaceIfExistingNewer, Some(existing), Some(incoming)) => existing > incoming,
            (Self::ReplaceIfExistingOlder, Some(existing), Some(incoming)) => existing < incoming,
            _ => false,
        }
    }
}

/// The committed result of [`Vfs::insert_file`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertOutcome {
    /// No conflicting node existed; a file was added.
    Inserted,
    /// A conflicting file or directory subtree was replaced with the file.
    /// This is reported even when the incoming bytes equal the previous bytes.
    Replaced,
    /// The policy retained the conflicting node; the incoming buffer was dropped.
    KeptExisting,
}

/// Counts of source-entry decisions in a successful mount, including directories.
///
/// Counts each source entry once, including skipped descendants and duplicates
/// in catalog order. Excludes the archive root and a host directory root whose
/// children are mounted; a single host file counts once.
/// [`Vfs::mount_node`] counts the supplied named node and all its descendants.
/// Counts decisions, not net changes. Removed destination descendants do not
/// contribute. Errors return no report and leave the tree unchanged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MountReport {
    /// Source entries added without a conflict at the time they were processed.
    pub inserted: usize,
    /// Source entries replacing conflicting nodes, including file/directory conflicts.
    /// Children of a replacement directory count separately as inserted entries.
    pub replaced: usize,
    /// Source entries rejected by policy, including every descendant of a rejected directory.
    pub skipped: usize,
    /// Source directories merged into existing directories; existing metadata is kept.
    /// Each child is counted separately according to its own decision.
    pub merged_directories: usize,
}

#[derive(Clone)]
enum Payload {
    Archive(Arc<Vdf>, usize),
    Owned(Arc<Vec<u8>>),
}

impl Payload {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Archive(archive, index) => archive
                .payload(*index)
                .expect("file index validated at mount"),
            Self::Owned(bytes) => bytes,
        }
    }
}

#[derive(Clone)]
enum Data {
    Directory(BTreeMap<Vec<u8>, Arc<Node>>),
    File(Payload),
}

/// An immutable tree node with shared payload ownership.
///
/// Tree lookups borrow nodes. Cloning a node creates an owned snapshot that keeps
/// its descendants and payloads alive after the source tree changes or is dropped.
/// A directory clone copies its immediate child map and shares descendants;
/// payload bytes are never copied by cloning. Names remain raw bytes.
/// Use [`Vfs::mount_node`] to compose a named snapshot into another tree.
#[derive(Clone)]
pub struct Node {
    name: Vec<u8>,
    raw_name: [u8; 64],
    timestamp: Option<Timestamp>,
    attributes: u32,
    extra_flags: u32,
    data: Data,
}

impl Drop for Node {
    fn drop(&mut self) {
        let Data::Directory(children) = &mut self.data else {
            return;
        };
        if children.is_empty() {
            return;
        }
        // Releasing an old persistent root must not recursively drop hundreds
        // of BTreeMaps/Arcs on the caller's stack. Iterator frames retain only
        // one iterator per depth; shared descendants keep their other owners.
        let mut pending = vec![std::mem::take(children).into_values()];
        while let Some(children) = pending.last_mut() {
            if let Some(child) = children.next() {
                if let Some(mut child) = Arc::into_inner(child)
                    && let Data::Directory(children) = &mut child.data
                    && !children.is_empty()
                {
                    pending.push(std::mem::take(children).into_values());
                }
                // Unique nodes now have empty child maps, so their Drop is shallow.
            } else {
                pending.pop();
            }
        }
    }
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("Node");
        debug
            .field("name", &self.name)
            .field("timestamp", &self.timestamp)
            .field("attributes", &self.attributes)
            .field("extra_flags", &self.extra_flags);
        match &self.data {
            Data::Directory(children) => debug.field("children", &children.len()),
            Data::File(payload) => debug.field("payload_bytes", &payload.bytes().len()),
        };
        debug.finish()
    }
}

impl Node {
    /// Logical name bytes after trimming trailing C whitespace; the root is `/`.
    pub fn name(&self) -> &[u8] {
        &self.name
    }
    /// Original 64-byte catalog name field, including padding. New names are
    /// space-padded; the synthetic root has an all-space field.
    pub fn raw_name(&self) -> &[u8; 64] {
        &self.raw_name
    }
    /// Modification time, or `None` for missing metadata. Archive and host mounts
    /// have known times; the root starts without one. Saving uses the separately
    /// selected [`SaveOptions::timestamp`], not this per-node value.
    pub fn timestamp(&self) -> Option<Timestamp> {
        self.timestamp
    }
    /// Preserved catalog attributes; new and host nodes use zero.
    pub fn attributes(&self) -> u32 {
        self.attributes
    }
    /// Unknown catalog flag bits, excluding regenerated directory/last bits.
    pub fn extra_flags(&self) -> u32 {
        self.extra_flags
    }
    /// Whether this node has children rather than a file payload.
    pub fn is_directory(&self) -> bool {
        matches!(self.data, Data::Directory(_))
    }
    /// Borrow exact file bytes without copying. An empty file is `Some(&[])`;
    /// a directory is `None`. The reference cannot outlive the borrowed node.
    pub fn payload(&self) -> Option<&[u8]> {
        match &self.data {
            Data::File(payload) => Some(payload.bytes()),
            _ => None,
        }
    }
    /// Iterate direct children in ascending normalized-name byte order.
    /// Files produce an empty iterator; `.rev()` gives descending order.
    pub fn children(&self) -> impl DoubleEndedIterator<Item = &Node> {
        let children = match &self.data {
            Data::Directory(c) => Some(c.values()),
            _ => None,
        };
        children.into_iter().flatten().map(Arc::as_ref)
    }
    /// Look up one direct child using ASCII case folding and trailing C whitespace
    /// trimming. Invalid names, absent children, and calls on files return `None`.
    /// This accepts a basename only; see [`Vfs`] for the complete name rules.
    pub fn child(&self, name: &[u8]) -> Option<&Node> {
        if valid_name(name).is_err() {
            return None;
        }
        match &self.data {
            Data::Directory(c) => c.get(&key(name)).map(Arc::as_ref),
            _ => None,
        }
    }
}

fn trimmed(name: &[u8]) -> &[u8] {
    &name[..name.iter().rposition(|b| !space(*b)).map_or(0, |i| i + 1)]
}
fn key(name: &[u8]) -> Vec<u8> {
    trimmed(name).iter().map(u8::to_ascii_lowercase).collect()
}
fn valid_name(name: &[u8]) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || trimmed(name).is_empty()
        || name.contains(&0)
        || name.contains(&b'/')
        || trimmed(name) == b"."
        || trimmed(name) == b".."
    {
        return Err(error(
            ErrorKind::InvalidData,
            "VFS name must be 1..64 bytes without NUL, slash or dot components",
        ));
    }
    Ok(())
}
fn node(name: &[u8], timestamp: Option<Timestamp>, data: Data) -> Result<Arc<Node>> {
    valid_name(name)?;
    let mut raw_name = [b' '; 64];
    raw_name[..name.len()].copy_from_slice(name);
    Ok(Arc::new(Node {
        name: trimmed(name).to_vec(),
        raw_name,
        timestamp,
        attributes: 0,
        extra_flags: 0,
        data,
    }))
}
fn components(path: &[u8], maximum: usize) -> Result<Vec<Vec<u8>>> {
    // Cap before allocating keys, including adversarial repeated separators.
    limited(
        path.len(),
        (maximum.min(MAX_DEPTH) + 1) * 65,
        "VFS path bytes",
    )?;
    let mut parts = Vec::new();
    for part in path.split(|b| *b == b'/').filter(|p| !p.is_empty()) {
        valid_name(part)?;
        limited(parts.len() + 1, maximum.min(MAX_DEPTH), "VFS path nesting")?;
        parts.push(part.to_vec());
    }
    Ok(parts)
}
fn directory_mut(node: &mut Arc<Node>) -> Result<&mut BTreeMap<Vec<u8>, Arc<Node>>> {
    match &mut Arc::make_mut(node).data {
        Data::Directory(c) => Ok(c),
        _ => Err(error(ErrorKind::NotDirectory, "VFS parent is a file")),
    }
}
fn at_mut<'a>(root: &'a mut Arc<Node>, path: &[Vec<u8>]) -> Result<&'a mut Arc<Node>> {
    let mut current = root;
    for part in path {
        current = directory_mut(current)?
            .get_mut(&key(part))
            .ok_or_else(|| error(ErrorKind::NotFound, "VFS parent path"))?;
    }
    Ok(current)
}
fn insert_node(
    children: &mut BTreeMap<Vec<u8>, Arc<Node>>,
    incoming: Arc<Node>,
    policy: Overwrite,
) -> InsertOutcome {
    let k = key(&incoming.name);
    let outcome = match children.get(&k) {
        None => InsertOutcome::Inserted,
        Some(existing) if policy.replaces(existing.timestamp, incoming.timestamp) => {
            InsertOutcome::Replaced
        }
        Some(_) => InsertOutcome::KeptExisting,
    };
    if outcome != InsertOutcome::KeptExisting {
        children.insert(k, incoming);
    }
    outcome
}

fn subtree_len(node: &Node) -> usize {
    // Source trees have already passed entry/depth limits before merging.
    let mut pending = vec![node];
    let mut count = 0;
    while let Some(node) = pending.pop() {
        count += 1;
        pending.extend(node.children());
    }
    count
}

fn merge(
    children: &mut BTreeMap<Vec<u8>, Arc<Node>>,
    incoming: Arc<Node>,
    policy: Overwrite,
    report: &mut MountReport,
) {
    struct Frame<'a> {
        key: Vec<u8>,
        children: std::collections::btree_map::Values<'a, Vec<u8>, Arc<Node>>,
    }
    // Borrow the immutable source throughout. Iterator frames use depth-sized
    // storage; revisiting bounded destination paths avoids recursive Rust frames
    // and mutable aliases. Copy-on-write keeps same-tree snapshots immutable.
    let mut frames: Vec<Frame<'_>> = Vec::new();
    let mut first = Some(&incoming);
    'walk: loop {
        let source = if let Some(source) = first.take() {
            source
        } else {
            loop {
                let Some(frame) = frames.last_mut() else {
                    break 'walk;
                };
                if let Some(child) = frame.children.next() {
                    break child;
                }
                frames.pop();
            }
        };
        let mut destination = &mut *children;
        for frame in &frames {
            destination = directory_mut(
                destination
                    .get_mut(&frame.key)
                    .expect("merged directory exists"),
            )
            .expect("merged directory remains a directory");
        }
        let k = key(&source.name);
        if let Data::Directory(source_children) = &source.data
            && destination.get(&k).is_some_and(|node| node.is_directory())
        {
            report.merged_directories += 1;
            frames.push(Frame {
                key: k,
                children: source_children.values(),
            });
            continue;
        }
        let count = subtree_len(source);
        match insert_node(destination, source.clone(), policy) {
            InsertOutcome::Inserted => report.inserted += count,
            InsertOutcome::Replaced => {
                report.replaced += 1;
                report.inserted += count - 1;
            }
            InsertOutcome::KeptExisting => report.skipped += count,
        }
    }
}

/// Host snapshot options. Defaults replace existing nodes and require ASCII names.
#[derive(Clone, Copy, Debug)]
pub struct HostOptions {
    /// Conflict policy applied after the entire host snapshot has been validated.
    pub overwrite: Overwrite,
    /// Host filename conversion; never guesses a legacy game code page.
    pub names: HostNameEncoding,
}
impl Default for HostOptions {
    fn default() -> Self {
        Self {
            overwrite: Overwrite::ReplaceExisting,
            names: HostNameEncoding::Ascii,
        }
    }
}

/// Explicit choices for serializing a composed tree into one VDF archive.
#[derive(Clone, Debug)]
pub struct SaveOptions {
    /// Select the Gothic 1 or Gothic 2 archive signature.
    pub game: GameVersion,
    /// VDF has one timestamp; selecting this explicitly replaces all node times
    /// on a future mount. No machine-local timezone or current-time default.
    pub timestamp: Timestamp,
    /// Ordinary output or runtime-selected Union compression.
    pub compression: Compression,
    /// Maximum final output size, also bounded by VDF's 32-bit offsets.
    pub output_bytes: usize,
    /// `None` preserves the sole comment from surviving archive-backed file nodes,
    /// or zeroes if none survive. Empty/fully discarded sources contribute none.
    /// Distinct surviving comments require an explicit selected output comment.
    pub comment: Option<[u8; 256]>,
}
impl SaveOptions {
    /// Select the required game/time, ordinary compression, a 256 MiB output cap,
    /// and automatic surviving-source comment selection.
    pub fn new(game: GameVersion, timestamp: Timestamp) -> Self {
        Self {
            game,
            timestamp,
            compression: Compression::None,
            output_bytes: 256 * 1024 * 1024,
            comment: None,
        }
    }
}

/// Owned virtual tree with shared immutable payloads and borrowed node views.
///
/// Virtual paths use `/`, ignore empty components (including leading/trailing
/// separators), and compare names with ASCII case folding. Each name is 1..64
/// raw bytes before trimming trailing C whitespace (` \t\r\n\x0b\x0c`). Empty,
/// NUL-containing, slash-containing, and trimmed `.`/`..` names are invalid.
/// Non-ASCII bytes are preserved; backslash is a literal name byte.
/// [`Self::remove`] additionally treats a trailing slash as a missing child.
///
/// Mutations are atomic on error. Cloning shares tree nodes and payload storage;
/// subsequent changes copy metadata along affected paths. Each insertion/mount
/// validates the complete candidate tree, including retained archive catalogs.
/// Repeated single-file insertions can therefore take quadratic metadata work.
/// Metadata edits do not copy file bytes.
///
/// [`Limits`] bound entries, depth, incoming input, per-file/logical payloads and
/// retained storage. Depth is additionally capped at 256. Every retained archive's
/// input and decoded buffers are charged even when some of its files were removed.
/// Separately parsed archives have already consumed their reader's decompression
/// budget. Candidate/source/old-tree/output buffers may coexist: these are format
/// operation limits, not a process-wide memory allocator quota.
///
/// # Compose, save, and reopen
///
/// ```
/// use zenkit::{GameVersion, vfs::{Compression, Overwrite, ReadOptions, SaveOptions,
///     Timestamp, Vdf, Vfs}};
/// let mut files = Vfs::default();
/// files.mkdir(b"DATA", None)?;
/// files.insert_file(b"DATA/config.txt", b"enabled=true".to_vec(), None,
///     Overwrite::KeepExisting)?;
/// assert_eq!(files.resolve(b"//data/config.TXT ").unwrap().timestamp(), None);
/// let time = Timestamp::from_unix_seconds(1_728_907_200);
/// let mut options = SaveOptions::new(GameVersion::Gothic2, time);
/// options.compression = Compression::Union;
/// let archive = Vdf::from_owned(files.to_bytes(options)?, ReadOptions::default())?;
/// // Payload borrows the archive, not this temporary EntryRef.
/// let bytes = archive.resolve(b"data/config.txt").unwrap().payload().unwrap();
/// assert_eq!(bytes, b"enabled=true");
/// let mut mounted = Vfs::default();
/// mounted.mount_archive(archive, b"/", Overwrite::ReplaceExisting)?;
/// assert_eq!(mounted.resolve(b"DATA/config.txt").unwrap().timestamp(), Some(time));
/// # Ok::<(), zenkit::Error>(())
/// ```
#[derive(Clone, Debug)]
pub struct Vfs {
    root: Arc<Node>,
    limits: Limits,
}
impl Default for Vfs {
    fn default() -> Self {
        Self::new(Limits::default())
    }
}

impl Vfs {
    /// Create an empty root with explicit limits and no timestamp.
    pub fn new(limits: Limits) -> Self {
        Self {
            root: Arc::new(Node {
                name: b"/".to_vec(),
                raw_name: [b' '; 64],
                timestamp: None,
                attributes: 0,
                extra_flags: 0,
                data: Data::Directory(BTreeMap::new()),
            }),
            limits,
        }
    }
    /// Borrow the synthetic root directory. It is always present and cannot be removed.
    pub fn root(&self) -> &Node {
        &self.root
    }
    /// Resolve a raw-byte path according to the normalization rules on [`Vfs`].
    /// An empty or separator-only path returns the root. Invalid/over-limit paths,
    /// missing components, and traversal through a file return `None`.
    /// Leading/repeated/trailing separators are ignored, even after a file name.
    /// Each component uses a direct ordered-map lookup; this does not search by basename.
    pub fn resolve(&self, path: &[u8]) -> Option<&Node> {
        let parts = components(path, self.limits.nesting).ok()?;
        let mut current = self.root();
        for part in parts {
            current = current.child(&part)?;
        }
        Some(current)
    }
    /// Search recursively by one basename, not a path. Invalid names return `None`.
    ///
    /// At each visited directory, direct children win; then directories are visited
    /// in reverse normalized-name order, depth first. Worst-case work visits the
    /// whole tree. This differs from the low-level [`Vdf::find`] path lookup.
    ///
    /// ```
    /// use zenkit::vfs::{Overwrite, Vfs};
    /// let mut files = Vfs::default();
    /// files.mkdir(b"nested", None)?;
    /// files.insert_file(b"nested/config", vec![42], None, Overwrite::KeepExisting)?;
    /// assert!(files.resolve(b"config").is_none());
    /// assert_eq!(files.find(b"CONFIG ").unwrap().payload(), Some(&[42][..]));
    /// assert!(files.find(b"nested/config").is_none());
    /// # Ok::<(), zenkit::Error>(())
    /// ```
    pub fn find(&self, name: &[u8]) -> Option<&Node> {
        if valid_name(name).is_err() {
            return None;
        }
        let mut stack = vec![self.root()];
        while let Some(directory) = stack.pop() {
            if let Some(found) = directory.child(name) {
                return Some(found);
            }
            stack.extend(directory.children().filter(|n| n.is_directory()));
        }
        None
    }
    fn publish(&mut self, root: Arc<Node>) -> Result<()> {
        self.validate(&root)?;
        self.root = root;
        Ok(())
    }

    fn validate(&self, root: &Node) -> Result<()> {
        self.validate_nodes(root.children().map(|n| (n, 1)))
    }

    fn validate_nodes<'a>(&self, nodes: impl IntoIterator<Item = (&'a Node, usize)>) -> Result<()> {
        let mut stack = Vec::new();
        let mut count = 0;
        // Check each entry before queueing it: a source with generous limits must
        // not allocate a wide worklist before the destination's smaller cap rejects it.
        let mut schedule = |stack: &mut Vec<(&'a Node, usize)>, node: &'a Node, depth| {
            count = add(count, 1, self.limits.entries, "VFS entries")?;
            limited(depth, self.limits.nesting.min(MAX_DEPTH), "VFS nesting")?;
            stack.push((node, depth));
            Ok::<(), Error>(())
        };
        for (node, depth) in nodes {
            schedule(&mut stack, node, depth)?;
        }
        let (mut payload, mut retained_input, mut retained_decoded) = (0usize, 0usize, 0usize);
        let mut archives = BTreeSet::new();
        let mut buffers = BTreeSet::new();
        while let Some((n, depth)) = stack.pop() {
            if let Data::File(data) = &n.data {
                limited(
                    data.bytes().len(),
                    self.limits.file_payload_bytes,
                    "VFS file payload",
                )?;
                payload = add(
                    payload,
                    data.bytes().len(),
                    self.limits.payload_bytes,
                    "VFS logical payload",
                )?;
                match data {
                    Payload::Owned(bytes) if buffers.insert(Arc::as_ptr(bytes)) => {
                        retained_input = add(
                            retained_input,
                            bytes.len(),
                            self.limits.input_bytes,
                            "VFS retained input",
                        )?;
                    }
                    Payload::Archive(archive, _) if archives.insert(Arc::as_ptr(archive)) => {
                        retained_input = add(
                            retained_input,
                            archive.bytes.len(),
                            self.limits.input_bytes,
                            "VFS retained input",
                        )?;
                        // Arc<Vdf> retains even removed files' decoded storage. Charge it all.
                        for entry in &archive.entries {
                            if let Contents::File {
                                decoded: Some(bytes),
                                ..
                            } = &entry.contents
                            {
                                retained_decoded = add(
                                    retained_decoded,
                                    bytes.len(),
                                    self.limits.payload_bytes,
                                    "VFS retained decompressed storage",
                                )?;
                            }
                        }
                    }
                    _ => {}
                }
            }
            for child in n.children() {
                schedule(&mut stack, child, depth + 1)?;
            }
        }
        Ok(())
    }

    /// Mount one owned named node and its descendants into an existing directory.
    ///
    /// Clone a node from [`Self::resolve`] or [`Node::child`] to obtain a snapshot.
    /// The input is consumed even on error. Payloads and unchanged descendants
    /// stay shared and remain valid after changes to the source tree.
    ///
    /// Mounts the named node under `parent`, preserving its raw catalog field,
    /// timestamp, attributes, flags and empty directories. Two directories merge,
    /// retaining the destination's metadata; other conflicts follow
    /// `policy`. Every source node contributes to the returned [`MountReport`].
    /// The synthetic `/` root is not a mountable basename; select a named node.
    ///
    /// Before applying overwrite policy, the entire incoming subtree must fit
    /// this VFS's entry, destination-relative depth, per-file/logical payload and
    /// retained storage limits, even if policy would skip it. The full resulting
    /// tree must also fit. Shared allocations are charged once per validation;
    /// archive-backed payloads retain and charge the complete source archive and
    /// its decoded storage, including entries outside the mounted subtree.
    /// Compression limits apply during decoding. Mounting copies changed-path
    /// metadata without copying or re-encoding payloads.
    ///
    /// ```
    /// use zenkit::vfs::{Overwrite, Vfs};
    /// let mut source = Vfs::default();
    /// source.mkdir(b"mod/empty", None)?;
    /// source.insert_file(b"mod/data", vec![1, 2, 3], None, Overwrite::KeepExisting)?;
    /// let subtree = source.resolve(b"mod").unwrap().clone();
    /// drop(source);
    /// let mut files = Vfs::default();
    /// files.mkdir(b"mods", None)?;
    /// let report = files.mount_node(subtree, b"mods", Overwrite::KeepExisting)?;
    /// assert_eq!(report.inserted, 3);
    /// assert_eq!(files.resolve(b"mods/mod/data").unwrap().payload(), Some(&[1, 2, 3][..]));
    /// assert!(files.resolve(b"mods/mod/empty").unwrap().is_directory());
    /// # Ok::<(), zenkit::Error>(())
    /// ```
    ///
    /// # Errors
    /// Invalid/missing/non-directory parent, the synthetic root as source, or
    /// exceeded source/candidate limits. No partial tree is published. Tree-policy
    /// errors include the requested virtual parent and have no input byte offset.
    pub fn mount_node(
        &mut self,
        incoming: Node,
        parent: &[u8],
        policy: Overwrite,
    ) -> Result<MountReport> {
        (|| {
            let parts = components(parent, self.limits.nesting)?;
            let mut root = self.root.clone();
            let destination = directory_mut(at_mut(&mut root, &parts)?)?;
            valid_name(incoming.name())?;
            self.validate_nodes(std::iter::once((&incoming, parts.len() + 1)))?;
            let mut report = MountReport::default();
            merge(destination, Arc::new(incoming), policy, &mut report);
            self.publish(root)?;
            Ok(report)
        })()
        .map_err(|e: Error| e.with_virtual_path(parent))
    }

    /// Create missing directories atomically, retaining existing directory metadata.
    /// New directories use `timestamp`; `None` explicitly represents absent metadata.
    /// An empty/root path succeeds without a change. Empty directories are valid
    /// in memory but cannot be serialized by [`Self::to_bytes`].
    ///
    /// # Errors
    /// Invalid names, file conflicts (`NotDirectory`), or exceeded limits. Errors
    /// include the requested virtual path when bounded; the original tree survives.
    pub fn mkdir(&mut self, path: &[u8], timestamp: Option<Timestamp>) -> Result<()> {
        (|| {
            let parts = components(path, self.limits.nesting)?;
            let mut root = self.root.clone();
            let mut current = &mut root;
            for part in parts {
                current = directory_mut(current)?.entry(key(&part)).or_insert(node(
                    &part,
                    timestamp,
                    Data::Directory(BTreeMap::new()),
                )?);
                if !current.is_directory() {
                    return Err(error(
                        ErrorKind::NotDirectory,
                        "VFS mkdir conflicts with file",
                    ));
                }
            }
            self.publish(root)
        })()
        .map_err(|e: Error| e.with_virtual_path(path))
    }
    /// Insert into an existing parent. Payload ownership moves without a byte copy.
    /// `None` means absent timestamp metadata. Conflicts use [`Overwrite`]; a skipped
    /// conflict succeeds and consumes/drops the incoming buffer. Parent directories
    /// are not created. See [`Vfs`] for validation cost and atomicity.
    /// The outcome includes directory replacement; equal bytes still count as
    /// replacement.
    ///
    /// ```
    /// use zenkit::vfs::{InsertOutcome, Overwrite, Vfs};
    /// let mut files = Vfs::default();
    /// assert_eq!(files.insert_file(b"config", vec![1], None, Overwrite::KeepExisting)?,
    ///     InsertOutcome::Inserted);
    /// assert_eq!(files.insert_file(b"CONFIG", vec![2], None, Overwrite::KeepExisting)?,
    ///     InsertOutcome::KeptExisting);
    /// assert_eq!(files.insert_file(b"config", vec![2], None, Overwrite::ReplaceExisting)?,
    ///     InsertOutcome::Replaced);
    /// # Ok::<(), zenkit::Error>(())
    /// ```
    ///
    /// # Errors
    /// Invalid/root paths, missing/non-directory parents, or incoming/candidate
    /// resource-limit violations. Incoming limits apply even if policy would skip it.
    pub fn insert_file(
        &mut self,
        path: &[u8],
        bytes: Vec<u8>,
        timestamp: Option<Timestamp>,
        policy: Overwrite,
    ) -> Result<InsertOutcome> {
        (|| {
            limited(
                bytes.len(),
                self.limits.file_payload_bytes,
                "VFS file payload",
            )?;
            let parts = components(path, self.limits.nesting)?;
            let (name, parent) = parts
                .split_last()
                .ok_or_else(|| error(ErrorKind::InvalidData, "VFS cannot replace root"))?;
            let incoming = node(name, timestamp, Data::File(Payload::Owned(Arc::new(bytes))))?;
            let mut root = self.root.clone();
            let outcome = insert_node(directory_mut(at_mut(&mut root, parent)?)?, incoming, policy);
            self.publish(root)?;
            Ok(outcome)
        })()
        .map_err(|e: Error| e.with_virtual_path(path))
    }
    /// Remove a file or directory subtree. Returns whether an entry was removed.
    /// Root, missing paths, traversal through a file, and trailing-slash paths return
    /// `false`. Removes metadata through copy-on-write; payloads may remain owned by
    /// clones or other nodes from the same archive.
    ///
    /// # Errors
    /// Invalid or over-limit virtual paths. The tree is unchanged on error.
    pub fn remove(&mut self, path: &[u8]) -> Result<bool> {
        (|| {
            // The reference treats a trailing separator as an empty child name.
            if path.last() == Some(&b'/') {
                return Ok(false);
            }
            let parts = components(path, self.limits.nesting)?;
            let Some((name, parent)) = parts.split_last() else {
                return Ok(false);
            };
            let mut root = self.root.clone();
            let target = match at_mut(&mut root, parent) {
                Ok(n) => n,
                Err(e) if matches!(e.kind, ErrorKind::NotFound | ErrorKind::NotDirectory) => {
                    return Ok(false);
                }
                Err(e) => return Err(e),
            };
            let children = match &mut Arc::make_mut(target).data {
                Data::Directory(c) => c,
                _ => return Ok(false),
            };
            let removed = children.remove(&key(name)).is_some();
            if removed {
                self.root = root;
            }
            Ok(removed)
        })()
        .map_err(|e: Error| e.with_virtual_path(path))
    }

    /// Mount a validated archive snapshot. Its storage is shared by file nodes.
    /// Refuses unknown nonzero source gaps/trailers rather than discarding them.
    /// `parent` must already exist as a directory. All source entries are validated
    /// before collisions are applied in original catalog order; directory pairs
    /// merge. The archive's validated UTC DOS timestamp becomes `Some` for every
    /// new node. Ownership of the archive moves even if the operation fails.
    /// Returns a [`MountReport`] counting all source catalog entries, including
    /// duplicates, directory merges, and descendants of skipped directories.
    ///
    /// ```
    /// use zenkit::{GameVersion, vfs::{Overwrite, ReadOptions, SaveOptions, Timestamp, Vdf, Vfs}};
    /// let mut source = Vfs::default();
    /// source.mkdir(b"DATA", None)?;
    /// source.insert_file(b"DATA/config", vec![1], None, Overwrite::KeepExisting)?;
    /// let bytes = source.to_bytes(SaveOptions::new(GameVersion::Gothic2,
    ///     Timestamp::from_unix_seconds(1_728_907_200)))?;
    /// let mut files = Vfs::default();
    /// let report = files.mount_archive(Vdf::from_bytes(&bytes, ReadOptions::default())?,
    ///     b"", Overwrite::KeepExisting)?;
    /// assert_eq!(report.inserted, 2); // directory and file
    /// let report = files.mount_archive(Vdf::from_bytes(&bytes, ReadOptions::default())?,
    ///     b"", Overwrite::KeepExisting)?;
    /// assert_eq!((report.merged_directories, report.skipped), (1, 1));
    /// # Ok::<(), zenkit::Error>(())
    /// ```
    ///
    /// # Errors
    /// Invalid/missing/non-directory parent, invalid source names or DOS timestamp,
    /// unsupported unknown source bytes, or exceeded source/candidate limits.
    /// Error leaves this tree unchanged. Errors about source bytes retain their
    /// actual archive offsets; tree-policy failures have no byte offset.
    pub fn mount_archive(
        &mut self,
        archive: Vdf,
        parent: &[u8],
        policy: Overwrite,
    ) -> Result<MountReport> {
        (|| {
            let path = components(parent, self.limits.nesting)?;
            let mut root = self.root.clone();
            let destination = directory_mut(at_mut(&mut root, &path)?)?;
            archive.validate_rewrite()?;
            let timestamp = Timestamp::from_dos(archive.timestamp_dos)?;
            let archive = Arc::new(archive);
            limited(
                archive.entries.len(),
                self.limits.entries,
                "VFS mount entries",
            )?;
            limited(
                archive.bytes.len(),
                self.limits.input_bytes,
                "VFS mount input",
            )?;
            let mut child_indices = vec![Vec::new(); archive.entries.len() + 1];
            for (i, entry) in archive.entries.iter().enumerate() {
                child_indices[entry.parent.unwrap_or(archive.entries.len())].push(i);
            }
            // Validate every source entry before any policy can skip a subtree.
            let mut pending: Vec<_> = child_indices[archive.entries.len()]
                .iter()
                .map(|&i| (i, path.len() + 1))
                .collect();
            let mut incoming_payload = 0;
            while let Some((index, depth)) = pending.pop() {
                limited(
                    depth,
                    self.limits.nesting.min(MAX_DEPTH),
                    "VFS mount nesting",
                )?;
                valid_name(archive.entries[index].name())?;
                if let Some(bytes) = archive.payload(index) {
                    limited(
                        bytes.len(),
                        self.limits.file_payload_bytes,
                        "VFS mount file payload",
                    )?;
                    incoming_payload = add(
                        incoming_payload,
                        bytes.len(),
                        self.limits.payload_bytes,
                        "VFS mount total payload",
                    )?;
                }
                pending.extend(child_indices[index].iter().map(|&i| (i, depth + 1)));
            }
            fn mount_entry(
                destination: &mut BTreeMap<Vec<u8>, Arc<Node>>,
                index: usize,
                archive: &Arc<Vdf>,
                children: &[Vec<usize>],
                time: Timestamp,
                policy: Overwrite,
                report: &mut MountReport,
            ) {
                let entry = &archive.entries[index];
                let k = key(entry.name());
                let existing = destination.get(&k);
                let merge_directory =
                    entry.is_directory() && existing.is_some_and(|n| n.is_directory());
                if !merge_directory {
                    if existing.is_some_and(|n| !policy.replaces(n.timestamp, Some(time))) {
                        fn count_subtree(index: usize, children: &[Vec<usize>]) -> usize {
                            1 + children[index]
                                .iter()
                                .map(|&child| count_subtree(child, children))
                                .sum::<usize>()
                        }
                        report.skipped += count_subtree(index, children);
                        return;
                    }
                    if existing.is_some() {
                        report.replaced += 1;
                    } else {
                        report.inserted += 1;
                    }
                    let data = if entry.is_directory() {
                        Data::Directory(BTreeMap::new())
                    } else {
                        Data::File(Payload::Archive(archive.clone(), index))
                    };
                    destination.insert(
                        k.clone(),
                        Arc::new(Node {
                            name: entry.name().to_vec(),
                            raw_name: entry.raw_name,
                            timestamp: Some(time),
                            attributes: entry.attributes,
                            extra_flags: entry.flags & !(DIRECTORY | LAST),
                            data,
                        }),
                    );
                } else {
                    report.merged_directories += 1;
                }
                if entry.is_directory() {
                    let dest = directory_mut(destination.get_mut(&k).expect("inserted directory"))
                        .expect("directory checked");
                    for &child in &children[index] {
                        // Apply duplicates against actual destination state, not a
                        // pre-deduplicated temporary directory.
                        mount_entry(dest, child, archive, children, time, policy, report);
                    }
                }
            }
            let mut report = MountReport::default();
            for &index in &child_indices[archive.entries.len()] {
                mount_entry(
                    destination,
                    index,
                    &archive,
                    &child_indices,
                    timestamp,
                    policy,
                    &mut report,
                );
            }
            self.publish(root)?;
            Ok(report)
        })()
        .map_err(|e: Error| e.with_virtual_path(parent))
    }

    /// Read and mount one host archive using this tree's [`Limits`].
    /// All decompression completes under the read limits before [`Self::mount_archive`].
    /// For restrictive host traversal/link rules use [`Self::mount_host`] instead;
    /// this convenience method opens the selected archive with ordinary filesystem I/O.
    /// Returns the same source-entry [`MountReport`] as [`Self::mount_archive`].
    ///
    /// # Errors
    /// Any read/parse or [`Self::mount_archive`] failure. Errors retain the host path,
    /// requested virtual parent, and real parser offsets/I/O source where available.
    pub fn mount_archive_path(
        &mut self,
        path: impl AsRef<Path>,
        parent: &[u8],
        policy: Overwrite,
    ) -> Result<MountReport> {
        let path = path.as_ref();
        (|| {
            self.mount_archive(
                Vdf::from_path(
                    path,
                    ReadOptions {
                        limits: self.limits,
                    },
                )?,
                parent,
                policy,
            )
        })()
        .map_err(|e: Error| e.with_host_path(path).with_virtual_path(parent))
    }

    /// Mount the children of a host directory, or a single host file. Reads a
    /// bounded snapshot before committing. Symlinks and Windows reparse points
    /// are rejected. Concurrent hostile host mutation is outside this snapshot API.
    /// Requires a trusted, quiescent host tree; it is not a filesystem sandbox.
    /// Directory children are mounted under the existing virtual `parent`; a file
    /// uses its host basename. Empty files are included. Host times become `Some`.
    /// The [`MountReport`] counts source files and directories; the directory root
    /// whose children are mounted is excluded. A single-file root counts once.
    ///
    /// # Errors
    /// I/O/metadata failure, invalid/non-directory virtual parent, unsupported host
    /// names/links/special files, folded-name collisions, or exceeded limits.
    /// No partial tree is published. Errors retain the deepest available host path,
    /// requested virtual parent, and original I/O source. No host files are written.
    pub fn mount_host(
        &mut self,
        host: impl AsRef<Path>,
        parent: &[u8],
        options: HostOptions,
    ) -> Result<MountReport> {
        self.mount_host_with_names(host, parent, options.overwrite, &options.names)
    }

    /// Mount a host snapshot with an explicit reversible filename codec.
    ///
    /// Each encoded component must fit the VDF 64-byte name field, pass the
    /// ordinary raw-name rules and decode back to the exact original `OsStr`.
    /// Distinct incoming names colliding under trimmed ASCII comparison are
    /// rejected, including inputs the overwrite policy would otherwise skip.
    /// All checks complete before publishing the candidate tree. Callback work
    /// follows [`io::HostNameCodec`]'s application-controlled resource contract.
    pub fn mount_host_with_names(
        &mut self,
        host: impl AsRef<Path>,
        parent: &[u8],
        overwrite: Overwrite,
        names: &dyn io::HostNameCodec,
    ) -> Result<MountReport> {
        let host = host.as_ref();
        (|| {
            let parts = components(parent, self.limits.nesting)?;
            let mut root = self.root.clone();
            let destination = directory_mut(at_mut(&mut root, &parts)?)?;
            let mut budget = HostBudget {
                entries: 0,
                bytes: 0,
                limits: self.limits,
                names,
            };
            let metadata = host_metadata(host)?;
            let incoming = if metadata.is_dir() {
                load_children(host, parts.len() + 1, &mut budget)?
            } else {
                let n = load_host(host, metadata, parts.len() + 1, &mut budget)?;
                BTreeMap::from([(key(n.name()), n)])
            };
            let mut report = MountReport::default();
            for child in incoming.into_values() {
                merge(destination, child, overwrite, &mut report);
            }
            self.publish(root)?;
            Ok(report)
        })()
        .map_err(|e: Error| e.with_host_path(host).with_virtual_path(parent))
    }

    /// Build the complete output in memory; failure leaves the tree unchanged and
    /// exposes no partial output. Empty directories cannot be represented by the
    /// VDF catalog contract and are rejected. An empty root is supported.
    /// Output selects one timestamp for all nodes on remount, preserves raw names,
    /// attributes and nonstructural flags, and regenerates catalog offsets/flags.
    /// Raw WAV/OGG payloads remain uncompressed even with Union output.
    ///
    /// # Errors
    /// Invalid DOS output date, empty child directories, conflicting surviving
    /// archive comments without an explicit selection, or exceeded output limits.
    /// Serialization takes work proportional to metadata/payloads plus compression,
    /// and allocates a complete output buffer. It does not write a host file.
    pub fn to_bytes(&self, options: SaveOptions) -> Result<Vec<u8>> {
        let dos = options.timestamp.to_dos()?;
        let mut records: Vec<(&Node, Option<usize>, bool)> = Vec::new();
        let mut queue = VecDeque::from([(self.root(), None)]);
        let mut child_offsets = BTreeMap::new();
        while let Some((directory, parent_index)) = queue.pop_front() {
            let children: Vec<_> = directory.children().collect();
            if parent_index.is_some() && children.is_empty() {
                return Err(error(
                    ErrorKind::Unsupported,
                    "VDF cannot represent empty directory",
                ));
            }
            if let Some(parent) = parent_index {
                child_offsets.insert(parent, records.len());
            }
            let last = children.len().saturating_sub(1);
            for (i, child) in children.into_iter().enumerate() {
                let index = records.len();
                records.push((child, parent_index, i == last));
                if child.is_directory() {
                    queue.push_back((child, Some(index)));
                }
            }
        }
        let size = records
            .len()
            .checked_mul(RECORD)
            .and_then(|v| v.checked_add(HEADER))
            .ok_or_else(|| error(ErrorKind::LimitExceeded, "VDF output catalog size"))?;
        limited(
            size,
            options.output_bytes.min(u32::MAX as usize),
            "VDF output bytes",
        )?;
        let mut comments = BTreeSet::new();
        for (node, _, _) in &records {
            if let Data::File(Payload::Archive(archive, _)) = &node.data {
                comments.insert(&archive.bytes[..256]);
            }
        }
        let comment = if let Some(comment) = options.comment {
            comment
        } else if comments.len() > 1 {
            return Err(error(
                ErrorKind::Unsupported,
                "VFS distinct source comments require explicit output comment",
            ));
        } else {
            let mut value = [0; 256];
            if let Some(source) = comments.first() {
                value.copy_from_slice(source);
            }
            value
        };
        let mut output = vec![0; size];
        output[..256].copy_from_slice(&comment);
        output[256..272].copy_from_slice(
            &SIGNATURES[match options.game {
                GameVersion::Gothic1 => 0,
                GameVersion::Gothic2 => 1,
            }],
        );
        put(&mut output, 272, records.len() as u32);
        put(
            &mut output,
            276,
            records.iter().filter(|(n, _, _)| !n.is_directory()).count() as u32,
        );
        put(&mut output, 280, dos);
        put(&mut output, 288, HEADER as u32);
        put(
            &mut output,
            292,
            if options.compression == Compression::Union {
                0xa0
            } else {
                0x50
            },
        );
        for (index, (node, _, last)) in records.iter().enumerate() {
            let at = HEADER + index * RECORD;
            output[at..at + 64].copy_from_slice(node.raw_name());
            put(
                &mut output,
                at + 72,
                node.extra_flags
                    | if node.is_directory() { DIRECTORY } else { 0 }
                    | if *last { LAST } else { 0 },
            );
            put(&mut output, at + 76, node.attributes);
            if let Some(payload) = node.payload() {
                limited(payload.len(), u32::MAX as usize, "VDF file length")?;
                let offset = output.len() as u32;
                put(&mut output, at + 64, offset);
                put(&mut output, at + 68, payload.len() as u32);
                if options.compression == Compression::Union && !raw_audio(node.name()) {
                    compression::encode(payload, &mut output, options.output_bytes)?;
                } else {
                    compression::append(&mut output, payload, options.output_bytes)?;
                }
            } else {
                put(&mut output, at + 64, child_offsets[&index] as u32);
            }
        }
        let length = output.len() as u32;
        put(&mut output, 284, length);
        Ok(output)
    }
}

fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn add(current: usize, next: usize, maximum: usize, context: &'static str) -> Result<usize> {
    current
        .checked_add(next)
        .filter(|n| *n <= maximum)
        .ok_or_else(|| error(ErrorKind::LimitExceeded, context))
}
fn host_metadata(path: &Path) -> Result<std::fs::Metadata> {
    (|| {
        let metadata =
            std::fs::symlink_metadata(path).map_err(|e| Error::io(e, "VFS host metadata"))?;
        #[cfg(windows)]
        let reparse = {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let reparse = false;
        if metadata.file_type().is_symlink() || reparse {
            return Err(error(
                ErrorKind::Unsupported,
                "VFS host symlink/reparse point",
            ));
        }
        if !metadata.is_dir() && !metadata.is_file() {
            return Err(error(ErrorKind::Unsupported, "VFS host special file"));
        }
        Ok(metadata)
    })()
    .map_err(|e: Error| e.with_host_path(path))
}
struct HostBudget<'a> {
    entries: usize,
    bytes: usize,
    limits: Limits,
    names: &'a dyn io::HostNameCodec,
}
fn load_children(
    path: &Path,
    depth: usize,
    budget: &mut HostBudget<'_>,
) -> Result<BTreeMap<Vec<u8>, Arc<Node>>> {
    (|| {
        let mut children = BTreeMap::new();
        for item in std::fs::read_dir(path).map_err(|e| Error::io(e, "VFS host directory"))? {
            let path = item
                .map_err(|e| Error::io(e, "VFS host directory entry").with_host_path(path))?
                .path();
            let child = load_host(&path, host_metadata(&path)?, depth, budget)?;
            if children.insert(key(child.name()), child).is_some() {
                return Err(
                    error(ErrorKind::InvalidData, "VFS colliding host names").with_host_path(&path)
                );
            }
        }
        Ok(children)
    })()
    .map_err(|e: Error| e.with_host_path(path))
}
fn load_host(
    path: &Path,
    metadata: std::fs::Metadata,
    depth: usize,
    budget: &mut HostBudget<'_>,
) -> Result<Arc<Node>> {
    (|| {
        budget.entries = add(budget.entries, 1, budget.limits.entries, "VFS host entries")?;
        limited(
            depth,
            budget.limits.nesting.min(MAX_DEPTH),
            "VFS host nesting",
        )?;
        let host_name = path
            .file_name()
            .ok_or_else(|| error(ErrorKind::InvalidData, "VFS host filename"))?;
        let name = io::names::encode(budget.names, host_name, 64)?;
        valid_name(&name)?;
        let timestamp = Timestamp::from_system_time(
            metadata
                .modified()
                .map_err(|e| Error::io(e, "VFS host modification time"))?,
        )?;
        let data = if metadata.is_dir() {
            Data::Directory(load_children(path, depth + 1, budget)?)
        } else {
            let remaining = budget
                .limits
                .input_bytes
                .min(budget.limits.payload_bytes)
                .saturating_sub(budget.bytes)
                .min(budget.limits.file_payload_bytes);
            if metadata.len() > remaining as u64 {
                return Err(error(ErrorKind::LimitExceeded, "VFS host file size"));
            }
            let file = std::fs::File::open(path).map_err(|e| Error::io(e, "VFS host file"))?;
            let bytes = io::read_bounded(file, remaining)?;
            budget.bytes = add(
                budget.bytes,
                bytes.len(),
                budget.limits.input_bytes.min(budget.limits.payload_bytes),
                "VFS host total bytes",
            )?;
            Data::File(Payload::Owned(Arc::new(bytes)))
        };
        node(&name, Some(timestamp), data)
    })()
    .map_err(|e: Error| e.with_host_path(path))
}
