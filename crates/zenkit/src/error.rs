use std::{
    fmt,
    path::{Path, PathBuf},
};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    Io,
    UnexpectedEof,
    InvalidData,
    Unsupported,
    LimitExceeded,
    NotFound,
    NotDirectory,
    /// An owner-bound handle belongs to another owner or does not identify a value.
    InvalidHandle,
    /// A graph edge or typed accessor targets a class its schema does not accept.
    WrongObjectType,
    /// A wire reference precedes or lacks its object definition.
    UnresolvedReference,
    /// A convenience traversal encountered a legal stored graph cycle.
    CycleDetected,
}

/// A parsing, serialization, or filesystem operation failure.
///
/// `offset` identifies an absolute byte position when the operation has a byte
/// stream. In-memory tree operations and filesystem metadata failures have no
/// byte offset. The original I/O error remains available through
/// [`std::error::Error::source`]. Paths are diagnostic context, not decoded names.
#[derive(Debug)]
pub struct Error {
    /// Broad failure category; match non-exhaustively for future additions.
    pub kind: ErrorKind,
    /// Absolute input byte position for a byte-specific failure, otherwise `None`.
    pub offset: Option<usize>,
    /// Static operation or field description, supplemented by paths and I/O source.
    pub context: &'static str,
    source: Option<std::io::Error>,
    host_path: Option<PathBuf>,
    virtual_path: Option<Vec<u8>>,
}

impl Error {
    /// Create an operation failure with no meaningful input byte location.
    pub fn operation(kind: ErrorKind, context: &'static str) -> Self {
        Self {
            kind,
            offset: None,
            context,
            source: None,
            host_path: None,
            virtual_path: None,
        }
    }
    /// Create a failure at an absolute input byte offset.
    pub fn new(kind: ErrorKind, offset: usize, context: &'static str) -> Self {
        Self {
            kind,
            offset: Some(offset),
            context,
            source: None,
            host_path: None,
            virtual_path: None,
        }
    }

    /// Wrap an I/O error without discarding its source chain.
    pub fn io(source: std::io::Error, context: &'static str) -> Self {
        Self {
            kind: ErrorKind::Io,
            offset: None,
            context,
            source: Some(source),
            host_path: None,
            virtual_path: None,
        }
    }

    /// Attach owned runtime labels without changing the structured failure.
    ///
    /// Moves the error and labels into a [`ContextError`] without allocation.
    /// The caller controls label size; paths, offsets and I/O sources are preserved.
    pub fn with_context(self, resource: String, detail: Option<String>) -> ContextError<Self> {
        ContextError::new(self, resource, detail)
    }

    /// The failing host path, when available; nested failures keep the child path.
    pub fn host_path(&self) -> Option<&Path> {
        self.host_path.as_deref()
    }

    /// The requested virtual path as raw bytes. Oversized invalid paths are omitted
    /// rather than allocating unbounded error context (maximum 16,705 bytes).
    pub fn virtual_path(&self) -> Option<&[u8]> {
        self.virtual_path.as_deref()
    }

    pub(crate) fn with_host_path(mut self, path: &Path) -> Self {
        if self.host_path.is_none() {
            self.host_path = Some(path.to_owned());
        }
        self
    }

    pub(crate) fn with_virtual_path(mut self, path: &[u8]) -> Self {
        if self.virtual_path.is_none() && path.len() <= 257 * 65 {
            self.virtual_path = Some(path.to_vec());
        }
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.context)?;
        if let Some(offset) = self.offset {
            write!(f, " at byte {offset}")?;
        }
        if let Some(path) = &self.host_path {
            write!(f, " in host path {}", path.display())?;
        }
        if let Some(path) = &self.virtual_path {
            write!(f, " at virtual path b\"")?;
            for byte in path {
                for escaped in std::ascii::escape_default(*byte) {
                    write!(f, "{}", char::from(escaped))?;
                }
            }
            write!(f, "\"")?;
        }
        if let Some(source) = &self.source {
            write!(f, ": {source}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_ref().map(|e| e as &dyn std::error::Error)
    }
}

/// An error with owned runtime resource and optional detail labels.
///
/// Stores `E` and caller-allocated labels inline without allocation or truncation.
/// Labels supplement the underlying error's paths and offsets. `None` and an
/// empty detail remain distinct. Display wording is not a stable API.
///
/// Accepts any `E`, including borrowed values. Display requires `E: Display`;
/// the standard error trait requires `E: Error + 'static` and exposes `E` as
/// its immediate source. [`Self::into_inner`] discards the labels.
///
/// Box the wrapper or its source if a smaller outer error is needed.
///
/// ```
/// use zenkit::{ContextError, ErrorKind, io::Reader};
/// let asset = format!("asset {}", 7);
/// let result = Reader::new(&[1]).u32("header")
///     .map_err(|error| error.with_context(asset, Some("loading preview".into())));
/// let error: ContextError = result.unwrap_err();
/// assert_eq!(error.resource(), "asset 7");
/// assert_eq!(error.detail(), Some("loading preview"));
/// assert_eq!(error.inner().kind, ErrorKind::UnexpectedEof);
/// assert_eq!(error.inner().offset, Some(0));
/// assert!(std::error::Error::source(&error).unwrap().is::<zenkit::Error>());
/// ```
#[derive(Debug)]
pub struct ContextError<E = Error> {
    inner: E,
    resource: String,
    detail: Option<String>,
}

impl<E> ContextError<E> {
    /// Move an error and already-owned labels into a wrapper without allocation.
    pub fn new(inner: E, resource: String, detail: Option<String>) -> Self {
        Self {
            inner,
            resource,
            detail,
        }
    }

    /// Runtime resource description, preserved exactly as supplied.
    pub fn resource(&self) -> &str {
        &self.resource
    }

    /// Optional runtime detail; an empty detail remains present.
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    /// Borrow the typed underlying value, without losing its structured fields.
    pub fn inner(&self) -> &E {
        &self.inner
    }

    /// Recover the underlying value, explicitly discarding the owned labels.
    pub fn into_inner(self) -> E {
        self.inner
    }

    /// Recover every value, in constructor argument order, without cloning.
    pub fn into_parts(self) -> (E, String, Option<String>) {
        (self.inner, self.resource, self.detail)
    }
}

impl<E: fmt::Display> fmt::Display for ContextError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.resource)?;
        if let Some(detail) = &self.detail {
            write!(f, " [context: {detail}]")?;
        }
        write!(f, ": {}", self.inner)
    }
}

impl<E: std::error::Error + 'static> std::error::Error for ContextError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.inner)
    }
}
