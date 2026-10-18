// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::{CTX, ErrorKind, Format, Limits, Result, Texture, Writer, bad, sum};
pub(super) fn write(t: &Texture, limits: Limits) -> Result<Vec<u8>> {
    let size = t
        .mips
        .iter()
        .try_fold(128 + if t.palette.is_some() { 1024 } else { 0 }, |n, m| {
            sum(n, m.len())
        })?;
    if size > limits.bytes {
        return Err(bad(ErrorKind::LimitExceeded));
    }
    let f = t.format;
    let compressed = f.is_compressed();
    let multiple = t.mips.len() > 1;
    let mut header = [0u32; 31];
    header[0] = 124;
    header[1] = 0x1007 | if compressed { 0x80000 } else { 8 } | if multiple { 0x20000 } else { 0 };
    header[2] = t.metadata.height;
    header[3] = t.metadata.width;
    header[4] = u32::try_from(if compressed {
        t.mips[0].len()
    } else {
        f.byte_len(t.metadata.width, 1)?
    })
    .map_err(|_| bad(ErrorKind::LimitExceeded))?;
    header[6] = if multiple { t.mips.len() as u32 } else { 0 };
    header[18] = 32;
    header[26] = 0x1000 | if multiple { 0x400008 } else { 0 };
    if compressed {
        header[19] = 4;
        header[20] = u32::from_le_bytes([b'D', b'X', b'T', b'1' + (f as u8 - Format::Dxt1 as u8)]);
    } else if f == Format::P8 {
        header[19] = 0x20;
        header[21] = 8;
    } else {
        let (r, g, b, a) = match f {
            Format::B8G8R8A8 => (0xff0000, 0xff00, 0xff, 0xff000000),
            Format::R8G8B8A8 => (0xff, 0xff00, 0xff0000, 0xff000000),
            Format::A8B8G8R8 => (0xff000000, 0xff0000, 0xff00, 0xff),
            Format::A8R8G8B8 => (0xff00, 0xff0000, 0xff000000, 0xff),
            Format::B8G8R8 => (0xff0000, 0xff00, 0xff, 0),
            Format::R8G8B8 => (0xff, 0xff00, 0xff0000, 0),
            Format::A4R4G4B4 => (0xf00, 0xf0, 0xf, 0xf000),
            Format::A1R5G5B5 => (0x7c00, 0x3e0, 0x1f, 0x8000),
            Format::R5G6B5 => (0xf800, 0x7e0, 0x1f, 0),
            _ => unreachable!("compressed and palette handled"),
        };
        header[19] = 0x40 | u32::from(a != 0);
        header[21] = f.unit() as u32 * 8;
        header[22..26].copy_from_slice(&[r, g, b, a]);
    }
    let mut w = Writer::new(limits.bytes);
    w.bytes(b"DDS ", CTX)?;
    for v in header {
        w.u32(v, CTX)?;
    }
    // Legacy P8 DDS palettes store entries as RGBA bytes.
    if let Some(p) = &t.palette {
        for c in p.iter() {
            w.bytes(&c.to_rgba(), CTX)?;
        }
    }
    for m in &t.mips {
        w.bytes(m, CTX)?;
    }
    Ok(w.into_inner())
}
