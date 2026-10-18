// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::{Format, Palette, Result, product, zeroed};
fn expand(v: u16, max: u16) -> u8 {
    (u32::from(v) * 255 / u32::from(max)) as u8
}
fn quant(v: u8, max: u16) -> u16 {
    ((u32::from(v) * u32::from(max) + 127) / 255) as u16
}
pub(super) fn decode(
    format: Format,
    width: u32,
    height: u32,
    bytes: &[u8],
    palette: Option<&Palette>,
) -> Result<Vec<u8>> {
    if format.is_compressed() {
        return super::bc::decode(format, width, height, bytes);
    }
    let mut out = zeroed(product(product(width as usize, height as usize)?, 4)?)?;
    for (p, v) in out
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(bytes.chunks_exact(format.unit()))
    {
        let color = match format {
            Format::B8G8R8A8 => [v[2], v[1], v[0], v[3]],
            Format::R8G8B8A8 => [v[0], v[1], v[2], v[3]],
            Format::A8B8G8R8 => [v[3], v[2], v[1], v[0]],
            Format::A8R8G8B8 => [v[1], v[2], v[3], v[0]],
            Format::B8G8R8 => [v[2], v[1], v[0], 255],
            Format::R8G8B8 => [v[0], v[1], v[2], 255],
            Format::P8 => palette.expect("validated P8 palette")[usize::from(v[0])].to_rgba(),
            Format::A4R4G4B4 => {
                let n = u16::from_le_bytes([v[0], v[1]]);
                [
                    ((n >> 8) & 15) as u8 * 17,
                    ((n >> 4) & 15) as u8 * 17,
                    (n & 15) as u8 * 17,
                    (n >> 12) as u8 * 17,
                ]
            }
            Format::A1R5G5B5 => {
                let n = u16::from_le_bytes([v[0], v[1]]);
                [
                    expand((n >> 10) & 31, 31),
                    expand((n >> 5) & 31, 31),
                    expand(n & 31, 31),
                    if n & 0x8000 == 0 { 0 } else { 255 },
                ]
            }
            Format::R5G6B5 => {
                let n = u16::from_le_bytes([v[0], v[1]]);
                [
                    expand(n >> 11, 31),
                    expand((n >> 5) & 63, 63),
                    expand(n & 31, 31),
                    255,
                ]
            }
            _ => unreachable!("compressed format handled above"),
        };
        p.copy_from_slice(&color);
    }
    Ok(out)
}
pub(super) fn encode(
    format: Format,
    width: u32,
    height: u32,
    rgba: &[u8],
    palette: Option<&Palette>,
) -> Result<Vec<u8>> {
    if format.is_compressed() {
        return super::bc::encode(format, width, height, rgba);
    }
    let mut out = zeroed(format.byte_len(width, height)?)?;
    for (p, c) in out
        .chunks_exact_mut(format.unit())
        .zip(rgba.as_chunks::<4>().0.iter())
    {
        match format {
            Format::B8G8R8A8 => p.copy_from_slice(&[c[2], c[1], c[0], c[3]]),
            Format::R8G8B8A8 => p.copy_from_slice(c),
            Format::A8B8G8R8 => p.copy_from_slice(&[c[3], c[2], c[1], c[0]]),
            Format::A8R8G8B8 => p.copy_from_slice(&[c[3], c[0], c[1], c[2]]),
            Format::B8G8R8 => p.copy_from_slice(&[c[2], c[1], c[0]]),
            Format::R8G8B8 => p.copy_from_slice(&c[..3]),
            Format::P8 => {
                let palette = palette.expect("validated P8 palette");
                p[0] = palette
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, color)| {
                        color
                            .to_rgba()
                            .iter()
                            .zip(c)
                            .map(|(a, b)| {
                                let d = i32::from(*a) - i32::from(*b);
                                (d * d) as u32
                            })
                            .sum::<u32>()
                    })
                    .expect("nonempty palette")
                    .0 as u8;
            }
            Format::A4R4G4B4 => p.copy_from_slice(
                &(quant(c[3], 15) << 12
                    | quant(c[0], 15) << 8
                    | quant(c[1], 15) << 4
                    | quant(c[2], 15))
                .to_le_bytes(),
            ),
            Format::A1R5G5B5 => p.copy_from_slice(
                &(quant(c[3], 1) << 15
                    | quant(c[0], 31) << 10
                    | quant(c[1], 31) << 5
                    | quant(c[2], 31))
                .to_le_bytes(),
            ),
            Format::R5G6B5 => p.copy_from_slice(
                &(quant(c[0], 31) << 11 | quant(c[1], 63) << 5 | quant(c[2], 31)).to_le_bytes(),
            ),
            _ => unreachable!("compressed format handled above"),
        }
    }
    Ok(out)
}
