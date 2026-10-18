//! Convert raw names to host filenames using a Latin-1 codec.
//! Choose the encoding required by your data.
use std::{borrow::Cow, ffi::OsStr};
use zenkit::{Error, ErrorKind, Result, io::HostNameCodec};

struct Latin1;
fn error(kind: ErrorKind) -> Error {
    Error::operation(kind, "application Latin-1 filename codec")
}
impl HostNameCodec for Latin1 {
    fn encode<'a>(&self, name: &'a OsStr, maximum: usize) -> Result<Cow<'a, [u8]>> {
        let text = name.to_str().ok_or_else(|| error(ErrorKind::Unsupported))?;
        let mut bytes = Vec::new();
        for character in text.chars() {
            if bytes.len() == maximum {
                return Err(error(ErrorKind::LimitExceeded));
            }
            let byte = u8::try_from(character as u32).map_err(|_| error(ErrorKind::Unsupported))?;
            bytes
                .try_reserve(1)
                .map_err(|_| error(ErrorKind::LimitExceeded))?;
            bytes.push(byte);
        }
        Ok(Cow::Owned(bytes))
    }
    fn decode<'a>(&self, name: &'a [u8], maximum: usize) -> Result<Cow<'a, OsStr>> {
        let length = name.iter().try_fold(0usize, |total, &byte| {
            total
                .checked_add(char::from(byte).len_utf8())
                .filter(|length| *length <= maximum)
                .ok_or_else(|| error(ErrorKind::LimitExceeded))
        })?;
        let mut text = String::new();
        text.try_reserve_exact(length)
            .map_err(|_| error(ErrorKind::LimitExceeded))?;
        text.extend(name.iter().copied().map(char::from));
        Ok(Cow::Owned(text.into()))
    }
}

fn main() -> Result<()> {
    let codec: &dyn HostNameCodec = &Latin1;
    let host = OsStr::new("Caf\u{e9}.ZEN");
    let raw = codec.encode(host, 64)?;
    assert_eq!(raw.as_ref(), b"Caf\xe9.ZEN");
    assert_eq!(&*codec.decode(&raw, 64)?, host);

    // Use the same codec with SaveSnapshot::load_default_world_with_names and
    // prepare_active_world_with_names. Other archive strings remain raw bytes.
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
