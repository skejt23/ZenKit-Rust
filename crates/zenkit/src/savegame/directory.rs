// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2022-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;
use crate::{io, objects::ObjectArchive, texture};
use std::{
    cmp::Ordering,
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
};

/// Snapshot bounds, separate from limits for decoding individual components.
#[derive(Clone, Copy, Debug)]
pub struct SnapshotLimits {
    /// Regular files plus explicitly retained directories.
    pub entries: usize,
    /// Maximum relative path component count.
    pub depth: usize,
    pub file_bytes: usize,
    pub total_bytes: usize,
    /// Sum of relative host-path encoded byte lengths.
    pub name_bytes: usize,
    pub objects: objects::Limits,
    pub texture: texture::Limits,
}
impl Default for SnapshotLimits {
    fn default() -> Self {
        Self {
            entries: 4096,
            depth: 32,
            file_bytes: 256 * 1024 * 1024,
            total_bytes: 512 * 1024 * 1024,
            name_bytes: 1024 * 1024,
            objects: Default::default(),
            texture: Default::default(),
        }
    }
}
struct StoredFile {
    path: PathBuf,
    bytes: Vec<u8>,
}
/// Private authoritative file bytes, with independent decoded drafts and explicit
/// replacement edits. Unknown files, empty directories and other worlds survive.
///
/// Names use componentwise Unicode uppercase matching (ASCII for non-Unicode
/// names), with exact shared-parent spelling. This conservative collision policy
/// orders Unicode and non-Unicode components in separate classes to keep the
/// catalogue comparison transitive. It performs no Unicode normalization and
/// does not model every filesystem's equivalence rules. Publication also uses
/// create-new files and reports host collisions. Capturing a changing directory
/// is not an atomic filesystem snapshot.
pub struct SaveSnapshot {
    game: GameVersion,
    files: Vec<StoredFile>,
    directories: Vec<PathBuf>,
}
impl fmt::Debug for SaveSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SaveSnapshot")
            .field("game", &self.game)
            .field("files", &self.files.len())
            .field("directories", &self.directories.len())
            .finish()
    }
}
#[derive(Clone, Copy)]
pub struct SnapshotFile<'a> {
    pub path: &'a Path,
    pub bytes: &'a [u8],
}
impl fmt::Debug for SnapshotFile<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SnapshotFile")
            .field(
                "path_bytes",
                &self.path.as_os_str().as_encoded_bytes().len(),
            )
            .field("bytes", &self.bytes.len())
            .finish()
    }
}
fn ioerr(e: std::io::Error, path: &Path) -> Error {
    Error::io(e, CTX).with_host_path(path)
}
fn push<T>(items: &mut Vec<T>, value: T) -> Result<()> {
    items
        .try_reserve(1)
        .map_err(|_| bad(ErrorKind::LimitExceeded))?;
    items.push(value);
    Ok(())
}
fn add(total: &mut usize, n: usize, max: usize) -> Result<()> {
    *total = total
        .checked_add(n)
        .filter(|n| *n <= max)
        .ok_or_else(|| bad(ErrorKind::LimitExceeded))?;
    Ok(())
}
fn component_cmp(a: &OsStr, b: &OsStr) -> Ordering {
    match (a.to_str(), b.to_str()) {
        (Some(a), Some(b)) => a
            .chars()
            .flat_map(char::to_uppercase)
            .cmp(b.chars().flat_map(char::to_uppercase)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a
            .as_encoded_bytes()
            .iter()
            .map(u8::to_ascii_uppercase)
            .cmp(b.as_encoded_bytes().iter().map(u8::to_ascii_uppercase)),
    }
}
fn path_cmp(a: &Path, b: &Path) -> Ordering {
    let (mut a, mut b) = (a.components(), b.components());
    loop {
        match (a.next(), b.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(a), Some(b)) => {
                let order = component_cmp(a.as_os_str(), b.as_os_str());
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}
fn prefix(a: &Path, b: &Path) -> bool {
    let mut b = b.components();
    a.components().all(|a| {
        b.next()
            .is_some_and(|b| component_cmp(a.as_os_str(), b.as_os_str()) == Ordering::Equal)
    })
}
fn relative(path: &Path, limits: SnapshotLimits) -> Result<()> {
    let raw = path.as_os_str().as_encoded_bytes();
    if raw
        .split(|c| *c == b'/' || *c == b'\\')
        .any(|p| p == b"." || p == b"..")
        || raw.last().is_some_and(|c| *c == b'/' || *c == b'\\')
    {
        return Err(bad(ErrorKind::InvalidData));
    }
    let mut count = 0;
    for part in path.components() {
        let Component::Normal(name) = part else {
            return Err(bad(ErrorKind::InvalidData));
        };
        count += 1;
        let bytes = name.as_encoded_bytes();
        if bytes.is_empty()
            || bytes.last().is_some_and(|c| *c == b'.' || *c == b' ')
            || bytes.iter().any(|b| *b < 32 || b"<>:\"/\\|?*".contains(b))
        {
            return Err(bad(ErrorKind::InvalidData));
        }
        let base = bytes.split(|b| *b == b'.').next().unwrap();
        let device = [b"CON".as_slice(), b"PRN", b"AUX", b"NUL", b"CLOCK$"]
            .iter()
            .any(|n| base.eq_ignore_ascii_case(n));
        let port = base.len() >= 4
            && (base[..3].eq_ignore_ascii_case(b"COM") || base[..3].eq_ignore_ascii_case(b"LPT"))
            && ((base.len() == 4 && (b'1'..=b'9').contains(&base[3]))
                || [
                    "\u{00b9}".as_bytes(),
                    "\u{00b2}".as_bytes(),
                    "\u{00b3}".as_bytes(),
                ]
                .contains(&&base[3..]));
        if device || port {
            return Err(bad(ErrorKind::InvalidData));
        }
    }
    if count == 0 {
        return Err(bad(ErrorKind::InvalidData));
    }
    if count > limits.depth || raw.len() > limits.name_bytes {
        return Err(bad(ErrorKind::LimitExceeded));
    }
    Ok(())
}
fn regular(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    true
}
fn metadata(path: &Path) -> Result<fs::Metadata> {
    let value = fs::symlink_metadata(path).map_err(|e| ioerr(e, path))?;
    if !regular(&value) {
        return Err(bad(ErrorKind::Unsupported).with_host_path(path));
    }
    Ok(value)
}
impl SaveSnapshot {
    pub fn game(&self) -> GameVersion {
        self.game
    }
    pub fn files(
        &self,
    ) -> impl DoubleEndedIterator<Item = SnapshotFile<'_>>
    + ExactSizeIterator
    + std::iter::FusedIterator
    + '_ {
        self.files.iter().map(|f| SnapshotFile {
            path: &f.path,
            bytes: &f.bytes,
        })
    }
    pub fn directories(
        &self,
    ) -> impl DoubleEndedIterator<Item = &Path> + ExactSizeIterator + std::iter::FusedIterator + '_
    {
        self.directories.iter().map(PathBuf::as_path)
    }
    fn find(&self, path: &Path) -> Option<usize> {
        self.files
            .iter()
            .position(|f| path_cmp(&f.path, path) == Ordering::Equal)
    }
    pub fn file(&self, path: &Path) -> Option<&[u8]> {
        self.find(path).map(|i| self.files[i].bytes.as_slice())
    }
    fn required(&self, name: &str) -> Result<&[u8]> {
        self.file(Path::new(name))
            .ok_or_else(|| bad(ErrorKind::NotFound))
    }
    /// Move an owned catalogue into a bounded snapshot; no file bytes are cloned.
    pub fn from_files(
        game: GameVersion,
        files: impl IntoIterator<Item = (PathBuf, Vec<u8>)>,
        limits: SnapshotLimits,
    ) -> Result<Self> {
        let mut out = Self {
            game,
            files: Vec::new(),
            directories: Vec::new(),
        };
        let (mut bytes, mut names) = (0, 0);
        for (path, data) in files {
            relative(&path, limits)?;
            if out.files.len() >= limits.entries || data.len() > limits.file_bytes {
                return Err(bad(ErrorKind::LimitExceeded));
            }
            add(&mut bytes, data.len(), limits.total_bytes)?;
            add(
                &mut names,
                path.as_os_str().as_encoded_bytes().len(),
                limits.name_bytes,
            )?;
            push(&mut out.files, StoredFile { path, bytes: data })?;
        }
        out.validate_catalogue(limits)?;
        Ok(out)
    }
    fn validate_catalogue(&self, limits: SnapshotLimits) -> Result<()> {
        self.validate_edit(None, None, limits)
    }
    // Only borrowed path/length descriptors, never a second file-content cache.
    fn validate_edit(
        &self,
        replacement: Option<(usize, usize)>,
        extra: Option<(&Path, usize)>,
        limits: SnapshotLimits,
    ) -> Result<()> {
        let count = self
            .files
            .len()
            .checked_add(self.directories.len())
            .and_then(|n| n.checked_add(usize::from(extra.is_some())))
            .filter(|n| *n <= limits.entries)
            .ok_or_else(|| bad(ErrorKind::LimitExceeded))?;
        let (mut bytes, mut names) = (0, 0);
        let mut paths = Vec::new();
        paths
            .try_reserve_exact(count)
            .map_err(|_| bad(ErrorKind::LimitExceeded))?;
        let files = self
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let len = match replacement {
                    Some((j, len)) if i == j => len,
                    _ => f.bytes.len(),
                };
                (f.path.as_path(), len)
            })
            .chain(extra);
        for (path, len) in files {
            relative(path, limits)?;
            if len > limits.file_bytes {
                return Err(bad(ErrorKind::LimitExceeded));
            }
            add(&mut bytes, len, limits.total_bytes)?;
            add(
                &mut names,
                path.as_os_str().as_encoded_bytes().len(),
                limits.name_bytes,
            )?;
            paths.push((path, true));
        }
        for path in &self.directories {
            relative(path, limits)?;
            add(
                &mut names,
                path.as_os_str().as_encoded_bytes().len(),
                limits.name_bytes,
            )?;
            paths.push((path.as_path(), false));
        }
        paths.sort_unstable_by(|a, b| path_cmp(a.0, b.0));
        for pair in paths.windows(2) {
            if path_cmp(pair[0].0, pair[1].0) == Ordering::Equal
                || (pair[0].1 && prefix(pair[0].0, pair[1].0))
            {
                return Err(bad(ErrorKind::InvalidData));
            }
            for (a, b) in pair[0].0.components().zip(pair[1].0.components()) {
                if component_cmp(a.as_os_str(), b.as_os_str()) != Ordering::Equal {
                    break;
                }
                if a.as_os_str() != b.as_os_str() {
                    return Err(bad(ErrorKind::InvalidData));
                }
            }
        }
        Ok(())
    }
    /// Bounded capture of regular files. Rejects symlinks/reparse points and
    /// rereads each file to detect changes. Requires a stable directory and one
    /// writer: matching reads/metadata cannot prove absence of concurrent races.
    pub fn capture(root: &Path, game: GameVersion, limits: SnapshotLimits) -> Result<Self> {
        if !metadata(root)?.is_dir() {
            return Err(bad(ErrorKind::NotDirectory).with_host_path(root));
        }
        let mut out = Self {
            game,
            files: Vec::new(),
            directories: Vec::new(),
        };
        let mut pending = Vec::new();
        push(&mut pending, PathBuf::new())?;
        let (mut total, mut names) = (0, 0);
        while let Some(dir) = pending.pop() {
            let parent = root.join(&dir);
            for entry in fs::read_dir(&parent).map_err(|e| ioerr(e, &parent))? {
                let entry = entry.map_err(|e| ioerr(e, &parent))?;
                let rel = dir.join(entry.file_name());
                relative(&rel, limits)?;
                if out.files.len() + out.directories.len() >= limits.entries {
                    return Err(bad(ErrorKind::LimitExceeded));
                }
                add(
                    &mut names,
                    rel.as_os_str().as_encoded_bytes().len(),
                    limits.name_bytes,
                )?;
                let path = root.join(&rel);
                let before = metadata(&path)?;
                if before.is_dir() {
                    push(&mut pending, rel.clone())?;
                    push(&mut out.directories, rel)?;
                    continue;
                }
                if !before.is_file() {
                    return Err(bad(ErrorKind::Unsupported).with_host_path(&path));
                }
                let cap = limits.file_bytes.min(limits.total_bytes - total);
                if before.len() > cap as u64 {
                    return Err(bad(ErrorKind::LimitExceeded).with_host_path(&path));
                }
                let mut file = File::open(&path).map_err(|e| ioerr(e, &path))?;
                let bytes =
                    io::read_bounded(&mut file, cap).map_err(|e| e.with_host_path(&path))?;
                file.seek(SeekFrom::Start(0)).map_err(|e| ioerr(e, &path))?;
                let mut buf = [0; 4096];
                for part in bytes.chunks(4096) {
                    file.read_exact(&mut buf[..part.len()])
                        .map_err(|e| ioerr(e, &path))?;
                    if buf[..part.len()] != *part {
                        return Err(bad(ErrorKind::InvalidData).with_host_path(&path));
                    }
                }
                if file.read(&mut buf[..1]).map_err(|e| ioerr(e, &path))? != 0 {
                    return Err(bad(ErrorKind::InvalidData).with_host_path(&path));
                }
                let after = metadata(&path)?;
                if !after.is_file()
                    || before.len() != after.len()
                    || before.len() != bytes.len() as u64
                    || before.modified().map_err(|e| ioerr(e, &path))?
                        != after.modified().map_err(|e| ioerr(e, &path))?
                {
                    return Err(bad(ErrorKind::InvalidData).with_host_path(&path));
                }
                add(&mut total, bytes.len(), limits.total_bytes)?;
                push(&mut out.files, StoredFile { path: rel, bytes })?;
            }
        }
        out.validate_catalogue(limits)?;
        Ok(out)
    }
    pub fn metadata(&self, limits: objects::Limits) -> Result<ObjectArchive> {
        let d = ObjectArchive::from_bytes(self.required("SAVEINFO.SAV")?, self.game, limits)?;
        check_metadata(&d)?;
        crate::logging::emit(
            crate::logging::Level::Info,
            "zenkit::savegame",
            format_args!("loaded SAVEINFO.SAV"),
        );
        Ok(d)
    }
    pub fn state(&self, limits: objects::Limits) -> Result<StateArchive> {
        let state = StateArchive::from_bytes(self.required("SAVEDAT.SAV")?, self.game, limits)?;
        crate::logging::emit(
            crate::logging::Level::Info,
            "zenkit::savegame",
            format_args!("loaded SAVEDAT.SAV"),
        );
        Ok(state)
    }
    pub fn thumbnail(&self, limits: texture::Limits) -> Result<Option<texture::Texture>> {
        self.file(Path::new("THUMB.SAV"))
            .map(|b| {
                let texture = texture::Texture::from_bytes(b, limits)?;
                crate::logging::emit(
                    crate::logging::Level::Info,
                    "zenkit::savegame",
                    format_args!("loaded THUMB.SAV"),
                );
                Ok(texture)
            })
            .transpose()
    }
    /// Load a world by native filename, replacing its extension with `.SAV`.
    ///
    /// Passing `WORLD.PART` selects `WORLD.SAV`. [`Self::load_default_world`]
    /// appends `.SAV` to SAVEINFO's world stem: `WORLD.PART` selects
    /// `WORLD.PART.SAV`.
    pub fn load_world(
        &self,
        name: &Path,
        limits: objects::Limits,
    ) -> Result<Option<ObjectArchive>> {
        let name = world_filename(name)?;
        self.file(&name)
            .map(|bytes| {
                let d = ObjectArchive::from_bytes(bytes, self.game, limits)?;
                check_world(&d)?;
                Ok(d)
            })
            .transpose()
    }
    /// Select SAVEINFO's UTF-8 world stem, preserving every dot before `.SAV`.
    /// Use [`Self::load_default_world_with_names`] for an explicit legacy codec
    /// or [`Self::load_world`] for an independently supplied native filename.
    pub fn load_default_world(&self, limits: objects::Limits) -> Result<Option<ObjectArchive>> {
        self.load_default_world_with_names(
            &io::HostNameEncoding::Utf8,
            SnapshotLimits {
                objects: limits,
                ..Default::default()
            },
        )
    }
    /// Map SAVEINFO's raw world stem through a caller-selected reversible codec.
    ///
    /// `name_bytes` bounds the resulting native filename, including `.SAV`.
    /// Object limits bound metadata/world decoding. Mapping must reproduce the
    /// original raw bytes exactly and compose with literal `.SAV`/`.ZEN` suffixes.
    /// The final filename passes the ordinary single-component/device/reserved
    /// component checks. Missing worlds return `None`; mapping errors are errors.
    /// SAVEHDR is not used to override SAVEINFO selection.
    pub fn load_default_world_with_names(
        &self,
        names: &dyn io::HostNameCodec,
        limits: SnapshotLimits,
    ) -> Result<Option<ObjectArchive>> {
        let d = self.metadata(limits.objects)?;
        let raw = &check_metadata(&d)?.world;
        world_raw(raw)?;
        let cap = limits
            .name_bytes
            .checked_sub(4)
            .ok_or_else(|| bad(ErrorKind::LimitExceeded))?;
        let stem = io::names::decode(names, raw, cap)?;
        let filename = world_names(names, &stem, raw, limits)?;
        self.load_world(&filename, limits.objects)
    }
    /// Check the entire proposed catalogue before modifying the draft. Existing
    /// file casing is preserved, including a case-insensitive replacement query.
    pub fn replace_file(
        &mut self,
        path: &Path,
        bytes: Vec<u8>,
        limits: SnapshotLimits,
    ) -> Result<()> {
        relative(path, limits)?;
        if let Some(i) = self.find(path) {
            self.validate_edit(Some((i, bytes.len())), None, limits)?;
            self.files[i].bytes = bytes;
            return Ok(());
        }
        self.validate_edit(None, Some((path, bytes.len())), limits)?;
        push(
            &mut self.files,
            StoredFile {
                path: path.to_owned(),
                bytes,
            },
        )
    }
    pub fn remove_file(&mut self, path: &Path) -> bool {
        if let Some(i) = self.find(path) {
            self.files.remove(i);
            true
        } else {
            false
        }
    }
    pub fn replace_metadata(&mut self, d: &ObjectArchive, limits: SnapshotLimits) -> Result<()> {
        if d.game != self.game {
            return Err(bad(ErrorKind::InvalidData));
        }
        check_metadata(d)?;
        self.replace_file(
            Path::new("SAVEINFO.SAV"),
            d.to_bytes(limits.objects)?,
            limits,
        )
    }
    pub fn replace_state(&mut self, d: &StateArchive, limits: SnapshotLimits) -> Result<()> {
        if d.game != self.game {
            return Err(bad(ErrorKind::InvalidData));
        }
        self.replace_file(
            Path::new("SAVEDAT.SAV"),
            d.to_bytes(limits.objects)?,
            limits,
        )
    }
    /// None removes the thumbnail; it never manufactures an empty THUMB.SAV.
    pub fn replace_thumbnail(
        &mut self,
        t: Option<&texture::Texture>,
        limits: SnapshotLimits,
    ) -> Result<()> {
        if let Some(t) = t {
            self.replace_file(Path::new("THUMB.SAV"), t.to_bytes(limits.texture)?, limits)
        } else {
            self.remove_file(Path::new("THUMB.SAV"));
            Ok(())
        }
    }
    pub fn replace_world(
        &mut self,
        name: &Path,
        d: &ObjectArchive,
        limits: SnapshotLimits,
    ) -> Result<()> {
        if d.game != self.game {
            return Err(bad(ErrorKind::InvalidData));
        }
        check_world(d)?;
        self.replace_file(&world_filename(name)?, d.to_bytes(limits.objects)?, limits)
    }
    /// Validate the catalogue and required components. Other worlds and unknown
    /// files remain byte-exact and are decoded lazily on explicit request.
    pub fn prepare(self, limits: SnapshotLimits) -> Result<PreparedSave> {
        self.validate_catalogue(limits)?;
        self.metadata(limits.objects)?;
        self.state(limits.objects)?;
        self.thumbnail(limits.texture)?;
        Ok(PreparedSave { snapshot: self })
    }
    /// Select an active world, updating SAVEINFO.WorldName and SAVEHDR while
    /// preserving all other worlds and unknown files. Failure publishes nothing.
    pub fn prepare_active_world(
        self,
        name: &Path,
        world: &ObjectArchive,
        limits: SnapshotLimits,
    ) -> Result<PreparedSave> {
        self.prepare_active_world_with_names(name, world, &io::HostNameEncoding::Utf8, limits)
    }
    /// Select an active world with explicit raw/native filename conversion.
    ///
    /// The selected native stem must round-trip exactly through `names` and
    /// encoding the complete `.SAV`/`.ZEN` filenames must agree with appending
    /// those literal suffixes. The encoded stem cannot contain NUL, CR or LF.
    /// SAVEINFO and SAVEHDR use the same encoded stem. Other catalogue bytes and
    /// existing host spelling survive. Failure creates no prepared output and
    /// performs no filesystem writes. Codec work is caller-controlled; all
    /// returned lengths are checked before library copying/publication.
    pub fn prepare_active_world_with_names(
        mut self,
        name: &Path,
        world: &ObjectArchive,
        names: &dyn io::HostNameCodec,
        limits: SnapshotLimits,
    ) -> Result<PreparedSave> {
        let filename = world_filename(name)?;
        let stem = filename
            .file_stem()
            .ok_or_else(|| bad(ErrorKind::InvalidData))?;
        let raw_cap = limits
            .file_bytes
            .checked_sub(5)
            .ok_or_else(|| bad(ErrorKind::LimitExceeded))?
            .min(limits.objects.copied_bytes)
            .min(limits.objects.archive.field_bytes);
        let encoded = io::names::encode(names, stem, raw_cap)?;
        world_raw(&encoded)?;
        let filename = world_names(names, stem, &encoded, limits)?;
        let mut metadata = self.metadata(limits.objects)?;
        let id = metadata.graph.roots()[0].target.expect("metadata checked");
        let ObjectMut::SaveMetadata(value) = metadata.graph.get_mut(id)? else {
            unreachable!()
        };
        let mut raw = Vec::new();
        raw.try_reserve_exact(encoded.len())
            .map_err(|_| bad(ErrorKind::LimitExceeded))?;
        raw.extend_from_slice(&encoded);
        value.world = raw;
        self.replace_metadata(&metadata, limits)?;
        self.replace_world(&filename, world, limits)?;
        let mut header = Vec::new();
        header
            .try_reserve_exact(
                encoded
                    .len()
                    .checked_add(5)
                    .ok_or_else(|| bad(ErrorKind::LimitExceeded))?,
            )
            .map_err(|_| bad(ErrorKind::LimitExceeded))?;
        header.extend_from_slice(&encoded);
        header.extend_from_slice(b".ZEN\n");
        self.replace_file(Path::new("SAVEHDR.SAV"), header, limits)?;
        self.prepare(limits)
    }
}
fn world_raw(raw: &[u8]) -> Result<()> {
    if raw.is_empty() || raw.iter().any(|b| matches!(b, 0 | b'\r' | b'\n')) {
        return Err(bad(ErrorKind::InvalidData));
    }
    Ok(())
}
fn append_name(stem: &OsStr, suffix: &str, limit: usize) -> Result<OsString> {
    let length = stem
        .as_encoded_bytes()
        .len()
        .checked_add(suffix.len())
        .filter(|length| *length <= limit)
        .ok_or_else(|| bad(ErrorKind::LimitExceeded))?;
    let mut name = OsString::new();
    name.try_reserve_exact(length)
        .map_err(|_| bad(ErrorKind::LimitExceeded))?;
    name.push(stem);
    name.push(suffix);
    Ok(name)
}
fn world_names(
    names: &dyn io::HostNameCodec,
    stem: &OsStr,
    raw: &[u8],
    limits: SnapshotLimits,
) -> Result<PathBuf> {
    // A metadata stem is not an explicit filename: append without replacing
    // any dotted suffix, including a stem whose last character is a dot.
    let mut components = Path::new(stem).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(bad(ErrorKind::InvalidData));
    }
    let filename = PathBuf::from(append_name(stem, ".SAV", limits.name_bytes)?);
    validate_world_filename(&filename, limits)?;
    let raw_cap = raw
        .len()
        .checked_add(4)
        .ok_or_else(|| bad(ErrorKind::LimitExceeded))?;
    for suffix in [".SAV", ".ZEN"] {
        let full = append_name(stem, suffix, limits.name_bytes)?;
        let encoded = io::names::encode(names, &full, raw_cap)?;
        if !encoded.iter().eq(raw.iter().chain(suffix.as_bytes())) {
            return Err(bad(ErrorKind::Unsupported));
        }
    }
    Ok(filename)
}
fn check_metadata(d: &ObjectArchive) -> Result<&SaveMetadata> {
    let [root] = d.graph.roots() else {
        return Err(bad(ErrorKind::InvalidData));
    };
    let Some(id) = root.target else {
        return Err(bad(ErrorKind::WrongObjectType));
    };
    match d.graph.get(id)? {
        ObjectRef::SaveMetadata(v) => Ok(v),
        _ => Err(bad(ErrorKind::WrongObjectType)),
    }
}
fn check_world(d: &ObjectArchive) -> Result<()> {
    let [root] = d.graph.roots() else {
        return Err(bad(ErrorKind::InvalidData));
    };
    let Some(id) = root.target else {
        return Err(bad(ErrorKind::WrongObjectType));
    };
    if matches!(d.graph.get(id)?, ObjectRef::World(_)) {
        Ok(())
    } else {
        Err(bad(ErrorKind::WrongObjectType))
    }
}
fn world_filename(name: &Path) -> Result<PathBuf> {
    relative(name, SnapshotLimits::default())?;
    if name.components().count() != 1 {
        return Err(bad(ErrorKind::InvalidData));
    }
    let out = name.with_extension("SAV");
    validate_world_filename(&out, SnapshotLimits::default())?;
    Ok(out)
}
fn validate_world_filename(name: &Path, limits: SnapshotLimits) -> Result<()> {
    relative(name, limits)?;
    if name.components().count() != 1 {
        return Err(bad(ErrorKind::InvalidData));
    }
    // Typed world operations must never overwrite another save component.
    // Raw replacement remains an explicit operation on the file catalogue.
    if ["SAVEINFO.SAV", "SAVEDAT.SAV", "SAVEHDR.SAV", "THUMB.SAV"]
        .into_iter()
        .any(|component| path_cmp(name, Path::new(component)) == Ordering::Equal)
    {
        return Err(bad(ErrorKind::InvalidData));
    }
    Ok(())
}

#[derive(Debug)]
pub struct PreparedSave {
    snapshot: SaveSnapshot,
}
impl PreparedSave {
    pub fn snapshot(&self) -> &SaveSnapshot {
        &self.snapshot
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublishPhase {
    Preflight,
    Reserve,
    Write,
    Backup,
    Install,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DestinationState {
    Unchanged,
    Partial,
    Absent,
    Restored,
    Unknown,
}
/// Retained locations and state for explicit recovery. Drop deletes nothing.
#[derive(Debug)]
pub struct PublishError {
    pub error: Error,
    pub phase: PublishPhase,
    pub destination: PathBuf,
    pub staging: Option<PathBuf>,
    pub backup: Option<PathBuf>,
    pub destination_state: DestinationState,
    pub rollback_error: Option<Error>,
}
impl fmt::Display for PublishError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "save publication {:?}: {}", self.phase, self.error)
    }
}
impl std::error::Error for PublishError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}
#[derive(Debug)]
pub struct PublishedSave {
    pub destination: PathBuf,
    /// A retained existing-destination backup. Never automatically deleted.
    pub backup: Option<PathBuf>,
}
fn failure(
    error: Error,
    phase: PublishPhase,
    destination: &Path,
    state: DestinationState,
) -> PublishError {
    PublishError {
        error,
        phase,
        destination: destination.to_owned(),
        staging: None,
        backup: None,
        destination_state: state,
        rollback_error: None,
    }
}
fn absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(ioerr(e, path)),
        Ok(_) => Err(ioerr(
            std::io::Error::new(std::io::ErrorKind::AlreadyExists, "publication path exists"),
            path,
        )),
    }
}
fn create_parents(root: &Path, relative: &Path) -> Result<()> {
    let mut path = root.to_owned();
    for c in relative.components() {
        path.push(c);
        match fs::create_dir(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if !metadata(&path)?.is_dir() {
                    return Err(bad(ErrorKind::NotDirectory).with_host_path(&path));
                }
            }
            Err(e) => return Err(ioerr(e, &path)),
        }
    }
    Ok(())
}
fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| ioerr(e, path))?;
    f.write_all(bytes).map_err(|e| ioerr(e, path))?;
    f.sync_all().map_err(|e| ioerr(e, path))
}
// Recovery keeps both original errors and all locations without a new box
// allocation while already reporting a filesystem failure.
#[allow(clippy::result_large_err)]
impl PreparedSave {
    /// Reserve a new directory with create_dir, then write create-new files.
    /// Partial contents are visible until completion and retained on failure.
    /// Requires one writer and stable parent/path entries; no filesystem lock,
    /// atomic directory publication or crash-durability guarantee is claimed.
    pub fn publish_new(
        &self,
        destination: &Path,
    ) -> std::result::Result<PublishedSave, PublishError> {
        self.publish_with(destination, write_file)
    }
    fn publish_with(
        &self,
        destination: &Path,
        mut write: impl FnMut(&Path, &[u8]) -> Result<()>,
    ) -> std::result::Result<PublishedSave, PublishError> {
        fs::create_dir(destination).map_err(|e| {
            failure(
                ioerr(e, destination),
                PublishPhase::Reserve,
                destination,
                DestinationState::Unchanged,
            )
        })?;
        let result = (|| -> Result<()> {
            for d in &self.snapshot.directories {
                create_parents(destination, d)?;
            }
            for file in &self.snapshot.files {
                if let Some(parent) = file.path.parent() {
                    create_parents(destination, parent)?;
                }
                write(&destination.join(&file.path), &file.bytes)?;
            }
            Ok(())
        })();
        result.map_err(|e| {
            failure(
                e,
                PublishPhase::Write,
                destination,
                DestinationState::Partial,
            )
        })?;
        Ok(PublishedSave {
            destination: destination.to_owned(),
            backup: None,
        })
    }
    /// Replace an existing directory under exclusive-writer/stable-parent
    /// assumptions. Staging and backup must be absent distinct siblings. Keeps
    /// the backup on success. Renames have a visibility gap; rollback on install
    /// failure may itself fail. This is not atomic whole-directory replacement.
    pub fn replace_exclusive(
        &self,
        destination: &Path,
        staging: &Path,
        backup: &Path,
    ) -> std::result::Result<PublishedSave, PublishError> {
        self.replace_with(destination, staging, backup, |a, b| fs::rename(a, b))
    }
    fn replace_with(
        &self,
        destination: &Path,
        staging: &Path,
        backup: &Path,
        mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
    ) -> std::result::Result<PublishedSave, PublishError> {
        let preflight = (|| -> Result<()> {
            let parent = destination
                .parent()
                .ok_or_else(|| bad(ErrorKind::InvalidData))?
                .canonicalize()
                .map_err(|e| ioerr(e, destination))?;
            for p in [destination, staging, backup] {
                let name = p.file_name().ok_or_else(|| bad(ErrorKind::InvalidData))?;
                relative(Path::new(name), SnapshotLimits::default())?;
                if p.parent()
                    .ok_or_else(|| bad(ErrorKind::InvalidData))?
                    .canonicalize()
                    .map_err(|e| ioerr(e, p))?
                    != parent
                {
                    return Err(bad(ErrorKind::InvalidData));
                }
            }
            if [destination, staging].into_iter().any(|p| {
                component_cmp(p.file_name().unwrap(), backup.file_name().unwrap())
                    == Ordering::Equal
            }) || component_cmp(
                destination.file_name().unwrap(),
                staging.file_name().unwrap(),
            ) == Ordering::Equal
            {
                return Err(bad(ErrorKind::InvalidData));
            }
            if !metadata(destination)?.is_dir() {
                return Err(bad(ErrorKind::NotDirectory).with_host_path(destination));
            }
            absent(staging)?;
            absent(backup)?;
            Ok(())
        })();
        preflight.map_err(|e| {
            failure(
                e,
                PublishPhase::Preflight,
                destination,
                DestinationState::Unchanged,
            )
        })?;
        self.publish_new(staging).map_err(|mut e| {
            e.staging = Some(staging.to_owned());
            e.destination = destination.to_owned();
            e.destination_state = DestinationState::Unchanged;
            e
        })?;
        rename(destination, backup).map_err(|e| {
            let mut e = failure(
                ioerr(e, destination),
                PublishPhase::Backup,
                destination,
                DestinationState::Unchanged,
            );
            e.staging = Some(staging.to_owned());
            e
        })?;
        if let Err(cause) = rename(staging, destination) {
            let mut e = failure(
                ioerr(cause, staging),
                PublishPhase::Install,
                destination,
                DestinationState::Absent,
            );
            e.staging = Some(staging.to_owned());
            e.backup = Some(backup.to_owned());
            match absent(destination)
                .and_then(|()| rename(backup, destination).map_err(|e| ioerr(e, backup)))
            {
                Ok(()) => {
                    e.destination_state = DestinationState::Restored;
                    e.backup = None;
                }
                Err(rollback) => {
                    e.destination_state = match fs::symlink_metadata(destination) {
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                            DestinationState::Absent
                        }
                        _ => DestinationState::Unknown,
                    };
                    e.rollback_error = Some(rollback);
                }
            }
            return Err(e);
        }
        Ok(PublishedSave {
            destination: destination.to_owned(),
            backup: Some(backup.to_owned()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static N: AtomicU64 = AtomicU64::new(0);
            let base = std::env::temp_dir().canonicalize().unwrap();
            let p = base.join(format!(
                "zenkit-publication-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            assert!(
                self.0.is_absolute()
                    && self
                        .0
                        .starts_with(std::env::temp_dir().canonicalize().unwrap())
            );
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn prepared() -> PreparedSave {
        PreparedSave {
            snapshot: SaveSnapshot::from_files(
                GameVersion::Gothic1,
                [
                    (PathBuf::from("first"), vec![1]),
                    (PathBuf::from("second"), vec![2]),
                ],
                Default::default(),
            )
            .unwrap(),
        }
    }
    fn injected() -> std::io::Error {
        std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "injected publication failure",
        )
    }
    #[test]
    fn write_failure_retains_partial_directory_and_reports_its_location() {
        let t = Temp::new();
        let dest = t.0.join("new");
        let p = prepared();
        let mut count = 0;
        let e = p
            .publish_with(&dest, |path, bytes| {
                count += 1;
                if count == 2 {
                    return Err(ioerr(injected(), path));
                }
                write_file(path, bytes)
            })
            .unwrap_err();
        assert_eq!(e.phase, PublishPhase::Write);
        assert_eq!(e.destination_state, DestinationState::Partial);
        assert_eq!(e.destination, dest);
        assert_eq!(fs::read(dest.join("first")).unwrap(), [1]);
        assert!(!dest.join("second").exists());
    }
    #[test]
    fn failed_backup_leaves_original_and_complete_staging() {
        let t = Temp::new();
        let dest = t.0.join("old");
        let stage = t.0.join("stage");
        let backup = t.0.join("backup");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("old"), [9]).unwrap();
        let e = prepared()
            .replace_with(&dest, &stage, &backup, |_, _| Err(injected()))
            .unwrap_err();
        assert_eq!(e.phase, PublishPhase::Backup);
        assert_eq!(e.destination_state, DestinationState::Unchanged);
        assert_eq!(e.staging, Some(stage.clone()));
        assert_eq!(fs::read(dest.join("old")).unwrap(), [9]);
        assert_eq!(fs::read(stage.join("second")).unwrap(), [2]);
        assert!(!backup.exists());
    }
    #[test]
    fn install_failure_restores_old_directory_or_reports_retained_backup() {
        for fail_rollback in [false, true] {
            let t = Temp::new();
            let dest = t.0.join("old");
            let stage = t.0.join("stage");
            let backup = t.0.join("backup");
            fs::create_dir(&dest).unwrap();
            fs::write(dest.join("old"), [9]).unwrap();
            let mut calls = 0;
            let e = prepared()
                .replace_with(&dest, &stage, &backup, |a, b| {
                    calls += 1;
                    if calls == 2 || (calls == 3 && fail_rollback) {
                        Err(injected())
                    } else {
                        fs::rename(a, b)
                    }
                })
                .unwrap_err();
            assert_eq!(e.phase, PublishPhase::Install);
            assert_eq!(e.staging, Some(stage.clone()));
            assert_eq!(fs::read(stage.join("first")).unwrap(), [1]);
            if fail_rollback {
                assert_eq!(e.destination_state, DestinationState::Absent);
                assert!(e.rollback_error.is_some());
                assert_eq!(e.backup, Some(backup.clone()));
                assert_eq!(fs::read(backup.join("old")).unwrap(), [9]);
                assert!(!dest.exists());
            } else {
                assert_eq!(e.destination_state, DestinationState::Restored);
                assert!(e.rollback_error.is_none());
                assert!(e.backup.is_none());
                assert_eq!(fs::read(dest.join("old")).unwrap(), [9]);
                assert!(!backup.exists());
            }
        }
    }
    #[test]
    fn concurrent_destination_is_unknown_and_never_removed_during_rollback() {
        let t = Temp::new();
        let dest = t.0.join("old");
        let stage = t.0.join("stage");
        let backup = t.0.join("backup");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("old"), [9]).unwrap();
        let mut calls = 0;
        let e = prepared()
            .replace_with(&dest, &stage, &backup, |a, b| {
                calls += 1;
                if calls == 2 {
                    fs::create_dir(b)?;
                    fs::write(b.join("external"), [7])?;
                    Err(injected())
                } else {
                    fs::rename(a, b)
                }
            })
            .unwrap_err();
        assert_eq!(calls, 2);
        assert_eq!(e.destination_state, DestinationState::Unknown);
        assert!(e.rollback_error.is_some());
        assert_eq!(fs::read(dest.join("external")).unwrap(), [7]);
        assert_eq!(fs::read(backup.join("old")).unwrap(), [9]);
    }
}
