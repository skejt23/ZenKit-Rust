//! Explicit, reversible conversion between raw names and native host names.
use crate::{Error, ErrorKind, Result};
use std::{borrow::Cow, ffi::OsStr};

/// Application-selected conversion of one filename component or world stem.
///
/// Archive text remains raw bytes. Callers choose a strict codec; no locale
/// guessing, case folding, Unicode normalization or lossy replacement occurs.
/// Non-Unicode host names are supported. Returned borrows follow the input.
///
/// Implementations must honor output budgets before allocation where possible
/// and return an error for unrepresentable input. Host lengths use
/// [`OsStr::as_encoded_bytes`], solely for measurement, never reconstruction.
/// The library rechecks lengths and exact inverse mappings. Callback resources
/// and side effects remain the application's responsibility.
///
/// Save-world operations additionally check that appending ASCII `.SAV`/`.ZEN`
/// to this stem agrees with encoding the complete native filename. Encoded
/// SAVEHDR stems cannot contain NUL, CR or LF.
pub trait HostNameCodec {
    fn encode<'a>(&self, name: &'a OsStr, max_bytes: usize) -> Result<Cow<'a, [u8]>>;
    fn decode<'a>(&self, name: &'a [u8], max_host_bytes: usize) -> Result<Cow<'a, OsStr>>;
}

/// Strict built-in host-name policies. Legacy code pages require an explicit
/// application implementation of [`HostNameCodec`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HostNameEncoding {
    /// Refuse non-ASCII names instead of guessing a legacy code page.
    #[default]
    Ascii,
    /// Store/interpret UTF-8 bytes explicitly; no legacy code-page conversion.
    Utf8,
}

fn bad(kind: ErrorKind) -> Error {
    Error::operation(kind, "host name codec")
}
fn bounded(length: usize, maximum: usize) -> Result<()> {
    if length > maximum {
        Err(bad(ErrorKind::LimitExceeded))
    } else {
        Ok(())
    }
}
impl HostNameCodec for HostNameEncoding {
    fn encode<'a>(&self, name: &'a OsStr, max_bytes: usize) -> Result<Cow<'a, [u8]>> {
        let text = name.to_str().ok_or_else(|| bad(ErrorKind::Unsupported))?;
        bounded(text.len(), max_bytes)?;
        if *self == Self::Ascii && !text.is_ascii() {
            return Err(bad(ErrorKind::Unsupported));
        }
        Ok(Cow::Borrowed(text.as_bytes()))
    }
    fn decode<'a>(&self, name: &'a [u8], max_host_bytes: usize) -> Result<Cow<'a, OsStr>> {
        bounded(name.len(), max_host_bytes)?;
        let text = std::str::from_utf8(name).map_err(|_| bad(ErrorKind::Unsupported))?;
        if *self == Self::Ascii && !text.is_ascii() {
            return Err(bad(ErrorKind::Unsupported));
        }
        Ok(Cow::Borrowed(OsStr::new(text)))
    }
}

pub(crate) fn encode<'a>(
    codec: &dyn HostNameCodec,
    name: &'a OsStr,
    maximum: usize,
) -> Result<Cow<'a, [u8]>> {
    let raw = codec.encode(name, maximum)?;
    bounded(raw.len(), maximum)?;
    let host_limit = name.as_encoded_bytes().len();
    let inverse = codec.decode(&raw, host_limit)?;
    bounded(inverse.as_encoded_bytes().len(), host_limit)?;
    if &*inverse != name {
        return Err(bad(ErrorKind::InvalidData));
    }
    Ok(raw)
}

pub(crate) fn decode<'a>(
    codec: &dyn HostNameCodec,
    raw: &'a [u8],
    maximum: usize,
) -> Result<Cow<'a, OsStr>> {
    let host = codec.decode(raw, maximum)?;
    bounded(host.as_encoded_bytes().len(), maximum)?;
    let inverse = codec.encode(&host, raw.len())?;
    bounded(inverse.len(), raw.len())?;
    if inverse.as_ref() != raw {
        return Err(bad(ErrorKind::InvalidData));
    }
    Ok(host)
}
