// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
//! Ordinary/Union VDF archives and a bounded, owned virtual filesystem.
use crate::{Error, ErrorKind, Limits, Result, compression, io};
use std::{fmt, io::Read, ops::Range, path::Path};

mod tree;
pub use tree::{
    HostNameEncoding, HostOptions, InsertOutcome, MountReport, Node, Overwrite, SaveOptions,
    Timestamp, Vfs,
};

const HEADER: usize = 296;
const RECORD: usize = 80;
const DIRECTORY: u32 = 0x8000_0000;
const LAST: u32 = 0x4000_0000;
const SIGNATURES: [[u8; 16]; 3] = [
    *b"PSVDSC_V2.00\r\n\r\n",
    *b"PSVDSC_V2.00\n\r\n\r",
    *b"PSVDSC_V2.00\x1a\x1a\x1a\x1a",
];

/// Finite parsing/decompression policy; defaults use [`Limits::default`].
#[derive(Clone, Copy, Debug, Default)]
pub struct ReadOptions {
    /// Input, catalog, decoded payload and decompression work limits.
    pub limits: Limits,
}

/// Runtime output format; compression is available in every build.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Compression {
    /// Ordinary uncompressed VDF payloads.
    #[default]
    None,
    /// Union block-zlib streams; WAV/OGG payloads remain raw.
    Union,
}

/// Serialization policy for an existing archive; defaults to ordinary output.
#[derive(Clone, Copy, Debug)]
pub struct WriteOptions {
    /// Runtime output format; decoded file bytes are preserved.
    pub compression: Compression,
    /// Maximum serialized output size, checked before extending the output buffer.
    pub output_bytes: usize,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            compression: Compression::None,
            output_bytes: 256 * 1024 * 1024,
        }
    }
}

enum Contents {
    Directory {
        first_child: usize,
    },
    File {
        stored: Range<usize>,
        decoded: Option<Vec<u8>>,
    },
}

/// Catalog metadata borrowed from an archive. Names remain raw bytes.
/// Use [`EntryRef`] to access metadata and payload through the same archive-bound view.
pub struct Entry {
    raw_name: [u8; 64],
    name_len: usize,
    contents: Contents,
    flags: u32,
    attributes: u32,
    parent: Option<usize>,
}

impl fmt::Debug for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("Entry");
        debug
            .field("name", &self.name())
            .field("flags", &self.flags)
            .field("attributes", &self.attributes)
            .field("parent", &self.parent);
        match &self.contents {
            Contents::Directory { first_child } => debug.field("first_child", first_child),
            Contents::File { stored, decoded } => debug.field(
                "payload_bytes",
                &decoded.as_ref().map_or(stored.len(), Vec::len),
            ),
        };
        debug.finish()
    }
}

impl Entry {
    /// Raw name bytes after trimming trailing C whitespace; no text decoding.
    pub fn name(&self) -> &[u8] {
        &self.raw_name[..self.name_len]
    }
    /// Whether the entry is a directory rather than a file.
    pub fn is_directory(&self) -> bool {
        matches!(self.contents, Contents::Directory { .. })
    }
    /// Uninterpreted catalog attribute bits, preserved by rewriting.
    pub fn attributes(&self) -> u32 {
        self.attributes
    }
    /// Complete catalog flags, including structural directory and last-child bits.
    pub fn flags(&self) -> u32 {
        self.flags
    }
    /// Parent catalog index, or `None` for a root entry.
    pub fn parent(&self) -> Option<usize> {
        self.parent
    }
}

/// An immutable, validated archive owning input and decoded payload buffers.
///
/// [`Self::resolve`] returns an archive-bound view for ordinary use. Catalog
/// indexes from [`Self::find`] and [`Self::entries`] are low-level values that
/// must be used only with this archive. Cloning a VFS shares its mounted archives;
/// this type does not clone the input or decompress on each payload lookup.
pub struct Vdf {
    bytes: Vec<u8>,
    entries: Vec<Entry>,
    catalog_offset: usize,
    timestamp_dos: u32,
    compression: Compression,
}

impl fmt::Debug for Vdf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Vdf")
            .field("input_bytes", &self.bytes.len())
            .field("entries", &self.entries.len())
            .field("catalog_offset", &self.catalog_offset)
            .field("timestamp_dos", &self.timestamp_dos)
            .field("compression", &self.compression)
            .finish()
    }
}

/// A copyable view tied to the archive that owns its metadata and payload.
///
/// References returned by this view borrow the archive, not the temporary view.
/// [`Vdf::resolve`] and bound navigation retain the correct owner automatically.
/// Low-level [`Vdf::entry`] still requires an index from that same archive.
///
/// ```compile_fail
/// # use zenkit::vfs::{ReadOptions, Vdf};
/// # fn cannot_escape(bytes: &[u8]) -> zenkit::Result<()> {
/// let payload = {
///     let archive = Vdf::from_bytes(bytes, ReadOptions::default())?;
///     archive.resolve(b"data.bin").unwrap().payload().unwrap()
/// };
/// println!("{}", payload.len()); // owning archive has been dropped
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy)]
pub struct EntryRef<'a> {
    archive: &'a Vdf,
    index: usize,
}

impl fmt::Debug for EntryRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EntryRef")
            .field("index", &self.index)
            .field("metadata", &self.metadata())
            .finish()
    }
}

impl<'a> EntryRef<'a> {
    /// Catalog metadata borrowed from the owning archive.
    pub fn metadata(self) -> &'a Entry {
        &self.archive.entries[self.index]
    }
    /// Raw name bytes with trailing C whitespace removed; no text decoding.
    pub fn name(self) -> &'a [u8] {
        self.metadata().name()
    }
    /// Whether this entry is a directory.
    pub fn is_directory(self) -> bool {
        self.metadata().is_directory()
    }
    /// Exact decoded payload, or `None` for a directory; no allocation or copy.
    pub fn payload(self) -> Option<&'a [u8]> {
        self.archive.payload(self.index)
    }
    /// Parent view from the same archive, or `None` for a root entry.
    pub fn parent(self) -> Option<Self> {
        self.metadata().parent().map(|index| Self {
            archive: self.archive,
            index,
        })
    }
    /// Low-level catalog index, valid only within the owning archive.
    pub fn index(self) -> usize {
        self.index
    }
}

fn invalid(offset: usize, field: &'static str) -> Error {
    Error::new(ErrorKind::InvalidData, offset, field)
}

fn limited(value: usize, maximum: usize, offset: usize, field: &'static str) -> Result<()> {
    if value > maximum {
        return Err(Error::new(ErrorKind::LimitExceeded, offset, field));
    }
    Ok(())
}

// Match names using ASCII case folding and C whitespace.
fn space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c)
}

fn raw_audio(name: &[u8]) -> bool {
    name.get(name.len().saturating_sub(4)..)
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case(b".wav") || extension.eq_ignore_ascii_case(b".ogg")
        })
}

impl Vdf {
    /// Open and parse an archive, retaining the host path in any error.
    ///
    /// # Errors
    /// I/O failure or any error from [`Self::from_reader`]. This uses ordinary file
    /// opening semantics, including host symlink following. Regular-file metadata
    /// supplies an allocation hint; the byte limit is checked against actual input.
    /// Capturing a changing file is not an atomic filesystem snapshot.
    pub fn from_path(path: impl AsRef<Path>, options: ReadOptions) -> Result<Self> {
        let path = path.as_ref();
        let file =
            std::fs::File::open(path).map_err(|e| Error::io(e, "open VDF").with_host_path(path))?;
        let size_hint = file
            .metadata()
            .ok()
            .filter(|metadata| metadata.is_file())
            .and_then(|metadata| usize::try_from(metadata.len()).ok())
            .unwrap_or(0);
        io::read_bounded_hint(file, options.limits.input_bytes, size_hint)
            .and_then(|bytes| Self::from_owned(bytes, options))
            .map_err(|e| e.with_host_path(path))
    }

    /// Read a finite owned snapshot, then validate/decompress it eagerly.
    ///
    /// # Errors
    /// I/O failure, exceeded [`ReadOptions::limits`], malformed/truncated catalog or
    /// compressed streams, or unsupported format/signature. Parser offsets refer to
    /// the beginning of this archive snapshot, not the reader's previous position.
    pub fn from_reader(reader: impl Read, options: ReadOptions) -> Result<Self> {
        Self::from_owned(
            io::read_bounded(reader, options.limits.input_bytes)?,
            options,
        )
    }

    /// Copy a borrowed archive into owned storage, then validate/decompress eagerly.
    /// Use [`Self::from_owned`] to move an existing buffer without copying input bytes.
    ///
    /// # Errors
    /// The same format/resource errors as [`Self::from_reader`], without I/O errors.
    pub fn from_bytes(bytes: &[u8], options: ReadOptions) -> Result<Self> {
        limited(
            bytes.len(),
            options.limits.input_bytes,
            0,
            "VDF input bytes",
        )?;
        Self::from_owned(bytes.to_vec(), options)
    }

    /// Take ownership of archive bytes without copying the input buffer.
    /// Compressed payloads are separately decoded during parsing; ordinary payloads
    /// borrow ranges within this buffer. Unknown gap/trailer bytes remain owned.
    ///
    /// # Errors
    /// Malformed/truncated data, unsupported signatures/format, or exceeded parsing
    /// and decompression limits. A failure drops the moved buffer.
    pub fn from_owned(bytes: Vec<u8>, options: ReadOptions) -> Result<Self> {
        let limits = options.limits;
        limited(bytes.len(), limits.input_bytes, 0, "VDF input bytes")?;
        let mut reader = io::Reader::new(&bytes);
        reader.take_bytes(256, "VDF comment")?;
        let signature = reader.take_bytes(16, "VDF signature")?;
        if !SIGNATURES.iter().any(|s| s == signature) {
            return Err(invalid(256, "VDF signature"));
        }
        let count = reader.u32("VDF entry count")? as usize;
        let file_count = reader.u32("VDF file count")? as usize;
        let timestamp_dos = reader.u32("VDF DOS timestamp")?;
        reader.u32("VDF declared size")?;
        let catalog = reader.u32("VDF catalog offset")? as usize;
        let catalog_offset = if catalog == 0 { HEADER } else { catalog };
        let volume = reader.u32("VDF volume flags")?;
        let compression = match volume {
            0x50 => Compression::None,
            0xa0 => Compression::Union,
            _ => return Err(Error::new(ErrorKind::Unsupported, 292, "VDF volume flags")),
        };
        limited(count, limits.entries, 272, "VDF entry count")?;
        if file_count > count || catalog_offset < HEADER {
            return Err(invalid(288, "VDF catalog"));
        }
        let catalog_size = count
            .checked_mul(RECORD)
            .ok_or_else(|| invalid(272, "VDF catalog size"))?;
        let catalog_end = catalog_offset
            .checked_add(catalog_size)
            .ok_or_else(|| invalid(288, "VDF catalog extent"))?;
        if catalog_end > bytes.len() {
            return Err(Error::new(
                ErrorKind::UnexpectedEof,
                catalog_offset,
                "VDF catalog extent",
            ));
        }
        reader.take_bytes(catalog_offset - HEADER, "VDF header padding")?;
        let mut catalog_reader = reader.substream(catalog_size, "VDF catalog")?;
        let mut entries = Vec::with_capacity(count);
        let mut payload_total = 0usize;
        let mut files = 0;
        for _ in 0..count {
            let at = catalog_reader.position();
            let mut raw_name = [0; 64];
            raw_name.copy_from_slice(catalog_reader.take_bytes(64, "VDF entry name")?);
            let name_len = raw_name
                .iter()
                .rposition(|b| !space(*b))
                .map_or(64, |i| i + 1);
            let offset = catalog_reader.u32("VDF entry offset")? as usize;
            let size = catalog_reader.u32("VDF entry size")? as usize;
            let flags = catalog_reader.u32("VDF entry flags")?;
            let attributes = catalog_reader.u32("VDF entry attributes")?;
            let contents = if flags & DIRECTORY != 0 {
                if size != 0 {
                    return Err(Error::new(
                        ErrorKind::Unsupported,
                        at + 68,
                        "VDF nonzero directory size",
                    ));
                }
                if offset >= count {
                    return Err(invalid(at + 64, "VDF child index"));
                }
                Contents::Directory {
                    first_child: offset,
                }
            } else {
                files += 1;
                limited(size, limits.file_payload_bytes, at + 68, "VDF file payload")?;
                let end = offset
                    .checked_add(size)
                    .ok_or_else(|| invalid(at + 64, "VDF payload extent"))?;
                if offset < HEADER || (offset >= catalog_offset && offset < catalog_end) {
                    return Err(invalid(at + 64, "VDF payload overlaps metadata"));
                }
                if offset > bytes.len() || (compression == Compression::None && end > bytes.len()) {
                    return Err(Error::new(
                        ErrorKind::UnexpectedEof,
                        offset,
                        "VDF payload extent",
                    ));
                }
                payload_total = payload_total
                    .checked_add(size)
                    .ok_or_else(|| invalid(at + 68, "VDF total payload"))?;
                limited(
                    payload_total,
                    limits.payload_bytes,
                    at + 68,
                    "VDF total payload",
                )?;
                Contents::File {
                    stored: offset..end,
                    decoded: None,
                }
            };
            entries.push(Entry {
                raw_name,
                name_len,
                contents,
                flags,
                attributes,
                parent: None,
            });
        }
        if files != file_count {
            return Err(invalid(276, "VDF file count mismatch"));
        }

        // Bound encoded streams at metadata and the next distinct file start.
        // An aliased start is allowed, but decoded bytes and work count for every entry.
        let mut starts: Vec<_> = entries
            .iter()
            .filter_map(|entry| match &entry.contents {
                Contents::File { stored, .. } => Some(stored.start),
                _ => None,
            })
            .collect();
        starts.push(catalog_offset);
        starts.push(bytes.len());
        starts.sort_unstable();
        starts.dedup();
        let mut compression_budget = compression::Budget::default();
        for (index, entry) in entries.iter_mut().enumerate() {
            let is_raw_audio = raw_audio(entry.name());
            if let Contents::File { stored, decoded } = &mut entry.contents {
                let at = catalog_offset + index * RECORD;
                if compression == Compression::Union {
                    let bound = starts
                        .get(starts.partition_point(|start| *start <= stored.start))
                        .copied()
                        .unwrap_or(bytes.len());
                    if is_raw_audio {
                        if stored.end > bound {
                            return Err(Error::new(
                                ErrorKind::UnexpectedEof,
                                stored.start,
                                "VDF raw audio extent",
                            ));
                        }
                    } else {
                        let (payload, end) = compression::decode(
                            &bytes,
                            stored.start..bound,
                            stored.len(),
                            limits,
                            &mut compression_budget,
                        )?;
                        stored.end = end;
                        *decoded = Some(payload);
                    }
                } else if stored.start < catalog_end && stored.end > catalog_offset {
                    return Err(invalid(at + 64, "VDF payload overlaps metadata"));
                }
            }
        }

        // A stack of sibling lists follows directory offsets, never catalog adjacency assumptions.
        let mut visited = vec![false; count];
        let mut stack = Vec::new();
        if count != 0 {
            stack.push((0usize, None, 0usize));
        }
        while let Some((mut index, parent, depth)) = stack.pop() {
            limited(
                depth,
                limits.nesting,
                catalog_offset,
                "VDF directory nesting",
            )?;
            loop {
                let at = catalog_offset + index * RECORD;
                if index >= count {
                    return Err(invalid(at, "VDF missing sibling terminator"));
                }
                if visited[index] {
                    return Err(invalid(at, "VDF cycle or shared catalog entry"));
                }
                visited[index] = true;
                let entry = &mut entries[index];
                entry.parent = parent;
                if let Contents::Directory { first_child } = entry.contents {
                    stack.push((first_child, Some(index), depth + 1));
                }
                if entry.flags & LAST != 0 {
                    break;
                }
                index += 1;
            }
        }
        if visited.contains(&false) {
            return Err(invalid(catalog_offset, "VDF unreachable catalog entries"));
        }
        crate::logging::emit(
            crate::logging::Level::Debug,
            "zenkit::vfs",
            format_args!(
                "loaded VDF: signature {signature:?}, compression {compression:?}, {count} entries, {files} files"
            ),
        );
        if crate::logging::enabled(crate::logging::Level::Trace) {
            for (index, entry) in entries.iter().enumerate() {
                crate::logging::emit(
                    crate::logging::Level::Trace,
                    "zenkit::vfs",
                    format_args!(
                        "catalog entry {index}: name {:?}, flags {:#x}, attributes {:#x}",
                        &entry.raw_name[..entry.name_len],
                        entry.flags,
                        entry.attributes
                    ),
                );
            }
        }
        Ok(Self {
            bytes,
            entries,
            catalog_offset,
            timestamp_dos,
            compression,
        })
    }

    /// Low-level catalog metadata in original order, including duplicate names.
    /// Indexes belong only to this archive; prefer [`Self::entry_views`] or
    /// [`Self::resolve`] to keep metadata and payload access tied to one owner.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    /// Borrow an entry by its index in this archive, or `None` if out of range.
    /// Prefer [`Self::resolve`] when starting with a path.
    pub fn entry(&self, index: usize) -> Option<EntryRef<'_>> {
        self.entries.get(index).map(|_| EntryRef {
            archive: self,
            index,
        })
    }
    /// Iterate archive-bound views in original catalog order, including duplicates.
    /// Use `.rev()` for reverse catalog order without collecting or copying entries.
    pub fn entry_views(&self) -> impl ExactSizeIterator<Item = EntryRef<'_>> + DoubleEndedIterator {
        (0..self.entries.len()).map(|index| EntryRef {
            archive: self,
            index,
        })
    }
    /// Resolve a raw-byte path to a view tied to this archive.
    ///
    /// Uses the same normalization and first-duplicate behavior as [`Self::find`].
    /// Unlike [`Vfs::find`], this follows a path; it does not search by basename.
    pub fn resolve(&self, path: &[u8]) -> Option<EntryRef<'_>> {
        self.find(path).map(|index| EntryRef {
            archive: self,
            index,
        })
    }
    /// Raw packed DOS header timestamp. Parsing preserves even invalid dates;
    /// [`Vfs::mount_archive`] validates it before assigning node timestamps.
    pub fn timestamp_dos(&self) -> u32 {
        self.timestamp_dos
    }
    /// Original 16-byte format signature, including line-ending/control bytes.
    pub fn signature(&self) -> &[u8] {
        &self.bytes[256..272]
    }
    /// Detected payload format of this archive.
    pub fn compression(&self) -> Compression {
        self.compression
    }

    /// Returns exact decoded payload bytes without copying. Directories and invalid
    /// catalog indexes return `None`; empty files return `Some(&[])`.
    /// This is a low-level index API: indexes must come from this same archive.
    /// Prefer [`EntryRef::payload`] when beginning with a path or entry view.
    pub fn payload(&self, index: usize) -> Option<&[u8]> {
        match &self.entries.get(index)?.contents {
            Contents::File { stored, decoded } => {
                Some(decoded.as_deref().unwrap_or(&self.bytes[stored.clone()]))
            }
            Contents::Directory { .. } => None,
        }
    }

    /// Low-level path lookup returning an index within this archive.
    ///
    /// Paths use slash separators, ignore empty components and trim trailing C
    /// whitespace in each component. Names compare with ASCII case folding;
    /// other bytes and backslashes are literal. An empty path, a missing component,
    /// or traversal through a file returns `None`. The first matching sibling in
    /// catalog order wins; duplicate names are not merged. Unlike [`Vfs`] mutation
    /// paths, this low-level lookup can address odd names retained from an archive.
    /// Each component scans the catalog, so worst-case work is depth times entries.
    ///
    /// Prefer [`Self::resolve`] for an archive-bound view. [`Vfs::find`] instead
    /// searches recursively by basename; it does not interpret a path.
    pub fn find(&self, path: &[u8]) -> Option<usize> {
        let mut parent = None;
        let mut found = None;
        for component in path.split(|b| *b == b'/').filter(|s| !s.is_empty()) {
            let end = component
                .iter()
                .rposition(|b| !space(*b))
                .map_or(0, |i| i + 1);
            let index = self.entries.iter().position(|e| {
                e.parent == parent && e.name().eq_ignore_ascii_case(&component[..end])
            })?;
            parent = Some(index);
            found = Some(index);
        }
        found
    }

    /// Serialize while preserving raw names, attributes, flags and payloads.
    /// Catalog/file offsets and declared size are regenerated. Unknown gap/trailer bytes
    /// are rejected; zero padding may be omitted. This defaults to ordinary output.
    ///
    /// # Errors
    /// Unsupported unknown source bytes or output size beyond the default cap.
    /// A failure returns no partial buffer and does not modify this archive.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        self.to_bytes_with_options(WriteOptions::default())
    }

    /// Convert to an ordinary or Union archive. Raw audio stays uncompressed in Union
    /// output; all other files use independent 8192-byte zlib blocks, including a
    /// canonical zero-block stream for empty files. Compression bytes may change.
    /// A complete output buffer is allocated; work is proportional to catalog and
    /// payload sizes plus compression. Source metadata is preserved, including the
    /// raw DOS header timestamp; no calendar conversion is performed here.
    ///
    /// # Errors
    /// Unsupported unknown source gaps/trailers or output beyond the selected cap
    /// or VDF's 32-bit offsets. No partial output is exposed and `self` is unchanged.
    pub fn to_bytes_with_options(&self, options: WriteOptions) -> Result<Vec<u8>> {
        self.validate_rewrite()?;
        self.encode(options)
    }

    fn validate_rewrite(&self) -> Result<()> {
        let mut ranges = vec![
            0..HEADER,
            self.catalog_offset..self.catalog_offset + self.entries.len() * RECORD,
        ];
        for entry in &self.entries {
            if let Contents::File { stored, .. } = &entry.contents {
                ranges.push(stored.clone());
            }
        }
        ranges.sort_by_key(|r| r.start);
        let mut covered = 0;
        for range in ranges {
            if range.start > covered && self.bytes[covered..range.start].iter().any(|b| *b != 0) {
                return Err(Error::new(
                    ErrorKind::Unsupported,
                    covered,
                    "VDF unknown gap bytes cannot be discarded",
                ));
            }
            covered = covered.max(range.end);
        }
        if self.bytes[covered..].iter().any(|b| *b != 0) {
            return Err(Error::new(
                ErrorKind::Unsupported,
                covered,
                "VDF unknown trailing bytes cannot be discarded",
            ));
        }
        Ok(())
    }

    fn encode(&self, options: WriteOptions) -> Result<Vec<u8>> {
        let catalog_size = self
            .entries
            .len()
            .checked_mul(RECORD)
            .ok_or_else(|| Error::operation(ErrorKind::LimitExceeded, "VDF output catalog"))?;
        let size = HEADER
            .checked_add(catalog_size)
            .ok_or_else(|| Error::operation(ErrorKind::LimitExceeded, "VDF output size"))?;
        let mut capacity = size;
        if options.compression == Compression::None {
            // The final length is known; avoid growing another large snapshot.
            for index in 0..self.entries.len() {
                if let Some(payload) = self.payload(index) {
                    capacity = capacity.checked_add(payload.len()).ok_or_else(|| {
                        Error::operation(ErrorKind::LimitExceeded, "VDF output size")
                    })?;
                }
            }
        }
        if capacity > options.output_bytes.min(u32::MAX as usize) {
            return Err(Error::operation(
                ErrorKind::LimitExceeded,
                "VDF output bytes",
            ));
        }
        let mut output = Vec::new();
        output
            .try_reserve_exact(capacity)
            .map_err(|_| Error::operation(ErrorKind::LimitExceeded, "VDF output allocation"))?;
        output.extend_from_slice(&self.bytes[..HEADER]);
        output[288..292].copy_from_slice(&(HEADER as u32).to_le_bytes());
        output[292..296].copy_from_slice(
            &(match options.compression {
                Compression::None => 0x50u32,
                Compression::Union => 0xa0,
            })
            .to_le_bytes(),
        );
        for index in 0..self.entries.len() {
            let at = self.catalog_offset + index * RECORD;
            output.extend_from_slice(&self.bytes[at..at + RECORD]);
            if let Some(payload) = self.payload(index) {
                let out_at = HEADER + index * RECORD + 68;
                output[out_at..out_at + 4].copy_from_slice(&(payload.len() as u32).to_le_bytes());
            }
        }
        for (index, entry) in self.entries.iter().enumerate() {
            if let Some(payload) = self.payload(index) {
                let offset = output.len() as u32;
                let out_at = HEADER + index * RECORD + 64;
                output[out_at..out_at + 4].copy_from_slice(&offset.to_le_bytes());
                if options.compression == Compression::Union && !raw_audio(entry.name()) {
                    compression::encode(payload, &mut output, options.output_bytes)?;
                } else {
                    compression::append(&mut output, payload, options.output_bytes)?;
                }
            }
        }
        let size32 = output.len() as u32;
        output[284..288].copy_from_slice(&size32.to_le_bytes());
        Ok(output)
    }
}
