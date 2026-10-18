// BC decoding expands RGB565 endpoints by bit replication and rounds
// interpolated channels down. Encoding searches endpoint candidates.
// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2006 Simon Brown                          si@sjbrown.co.uk
// SPDX-License-Identifier: MIT
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be included
// in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
// IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
// CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
// TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
// SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
use super::{Format, Result, product, zeroed};
fn rgb565(v: u16) -> [u8; 4] {
    let r = (v >> 11) as u8;
    let g = ((v >> 5) & 63) as u8;
    let b = (v & 31) as u8;
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
        255,
    ]
}
fn pack(c: [u8; 4]) -> u16 {
    let q = |v: u8, max: u32| ((u32::from(v) * max + 127) / 255) as u16;
    q(c[0], 31) << 11 | q(c[1], 63) << 5 | q(c[2], 31)
}
fn pack_ratio(rgb: [i128; 3], denominator: i128) -> u16 {
    // Both callers supply a positive denominator. Quantize the exact rational
    // endpoint directly to RGB565, including clipping, with no float rounding.
    let mut result = 0;
    for (c, max, shift) in [(0, 31, 11), (1, 63, 5), (2, 31, 0)] {
        let numerator = rgb[c].clamp(0, 255 * denominator);
        let value = (2 * numerator * max + 255 * denominator) / (510 * denominator);
        result |= (value as u16) << shift;
    }
    result
}
fn colors(a: u16, b: u16, bc1: bool) -> [[u8; 4]; 4] {
    let mut p = [rgb565(a), rgb565(b), [0, 0, 0, 255], [0, 0, 0, 255]];
    for c in [0, 1, 2] {
        let x = u16::from(p[0][c]);
        let y = u16::from(p[1][c]);
        if bc1 && a <= b {
            p[2][c] = ((x + y) / 2) as u8;
        } else {
            p[2][c] = ((2 * x + y) / 3) as u8;
            p[3][c] = ((x + 2 * y) / 3) as u8;
        }
    }
    if bc1 && a <= b {
        p[3] = [0; 4];
    }
    p
}
fn alphas(a: u8, b: u8) -> [u8; 8] {
    let mut p = [a, b, 0, 0, 0, 0, 0, 0];
    if a > b {
        for i in 1..7 {
            p[i + 1] = (((7 - i) as u16 * u16::from(a) + i as u16 * u16::from(b)) / 7) as u8;
        }
    } else {
        for i in 1..5 {
            p[i + 1] = (((5 - i) as u16 * u16::from(a) + i as u16 * u16::from(b)) / 5) as u8;
        }
        p[7] = 255;
    }
    p
}
fn decode_block(format: Format, block: &[u8]) -> [[u8; 4]; 16] {
    let color = if format == Format::Dxt1 {
        block
    } else {
        &block[8..]
    };
    let a = u16::from_le_bytes([color[0], color[1]]);
    let b = u16::from_le_bytes([color[2], color[3]]);
    let palette = colors(a, b, format == Format::Dxt1);
    let indices = u32::from_le_bytes([color[4], color[5], color[6], color[7]]);
    let mut result = [[0; 4]; 16];
    for (i, p) in result.iter_mut().enumerate() {
        *p = palette[((indices >> (2 * i)) & 3) as usize];
    }
    match format {
        Format::Dxt2 | Format::Dxt3 => {
            for (i, p) in result.iter_mut().enumerate() {
                p[3] = ((block[i / 2] >> (4 * (i % 2))) & 15) * 17;
            }
        }
        Format::Dxt4 | Format::Dxt5 => {
            let p = alphas(block[0], block[1]);
            let mut indices = 0u64;
            for (i, b) in block[2..8].iter().enumerate() {
                indices |= u64::from(*b) << (8 * i);
            }
            for (i, c) in result.iter_mut().enumerate() {
                c[3] = p[((indices >> (3 * i)) & 7) as usize];
            }
        }
        _ => {}
    }
    result
}
pub(super) fn decode(format: Format, width: u32, height: u32, bytes: &[u8]) -> Result<Vec<u8>> {
    let mut out = zeroed(product(product(width as usize, height as usize)?, 4)?)?;
    let bw = width.div_ceil(4) as usize;
    for (i, block) in bytes.chunks_exact(format.unit()).enumerate() {
        let pixels = decode_block(format, block);
        let bx = (i % bw) * 4;
        let by = (i / bw) * 4;
        for (y, row) in pixels.as_chunks::<4>().0.iter().enumerate() {
            for (x, pixel) in row.iter().enumerate() {
                if bx + x >= width as usize || by + y >= height as usize {
                    continue;
                }
                let mut p = *pixel;
                if format.is_premultiplied() {
                    for c in &mut p[..3] {
                        *c = if pixel[3] == 0 {
                            0
                        } else {
                            ((u32::from(*c) * 255 + u32::from(pixel[3]) / 2) / u32::from(pixel[3]))
                                .min(255) as u8
                        };
                    }
                }
                let offset = ((by + y) * width as usize + bx + x) * 4;
                out[offset..offset + 4].copy_from_slice(&p);
            }
        }
    }
    Ok(out)
}
fn distance(a: &[u8; 4], b: &[u8; 4]) -> u32 {
    (0..3)
        .map(|c| {
            let d = i32::from(a[c]) - i32::from(b[c]);
            (d * d) as u32
        })
        .sum()
}
fn color_candidate(
    pixels: &[[u8; 4]; 16],
    mut a: u16,
    mut b: u16,
    transparent: bool,
    bc1: bool,
    alpha_weighted: bool,
) -> (u64, [u8; 8]) {
    if transparent {
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
    } else {
        if a < b {
            std::mem::swap(&mut a, &mut b);
        }
        if a == b {
            if a == 0 {
                a = 1;
            } else {
                b -= 1;
            }
        }
    }
    let palette = colors(a, b, bc1);
    let mut indices = 0u32;
    let mut error = 0;
    for (i, p) in pixels.iter().enumerate() {
        let index = if transparent && p[3] < 128 {
            3
        } else {
            let count = if transparent { 3 } else { 4 };
            (0..count)
                .min_by_key(|&j| distance(p, &palette[j]))
                .expect("nonempty color palette")
        };
        if if bc1 { p[3] >= 128 } else { p[3] != 0 } {
            let weight = if alpha_weighted {
                u64::from(p[3]).pow(2)
            } else {
                1
            };
            error += u64::from(distance(p, &palette[index])) * weight;
        }
        indices |= (index as u32) << (i * 2);
    }
    let mut out = [0; 8];
    out[..2].copy_from_slice(&a.to_le_bytes());
    out[2..4].copy_from_slice(&b.to_le_bytes());
    out[4..].copy_from_slice(&indices.to_le_bytes());
    (error, out)
}
fn refine_color(
    pixels: &[[u8; 4]; 16],
    mut best: (u64, [u8; 8]),
    transparent: bool,
    bc1: bool,
    alpha_weighted: bool,
) -> (u64, [u8; 8]) {
    // Fit endpoints to the selected palette indices, then score the quantized
    // result with the actual decoder. Keep the existing candidate unless that
    // score improves. Fixed iterations and stack arrays bound the work/storage.
    for _ in 0..8 {
        if best.0 == 0 {
            break;
        }
        let indices = u32::from_le_bytes(best.1[4..].try_into().expect("color indices"));
        let scale = if transparent { 2 } else { 3 };
        let (mut aa, mut ab, mut bb) = (0u64, 0u64, 0u64);
        let (mut ax, mut bx) = ([0u64; 3], [0u64; 3]);
        for (i, p) in pixels.iter().enumerate() {
            if if bc1 { p[3] < 128 } else { p[3] == 0 } {
                continue;
            }
            let index = (indices >> (2 * i)) & 3;
            let (a, b) = match (transparent, index) {
                (_, 0) => (scale, 0),
                (_, 1) => (0, scale),
                (true, 2) => (1, 1),
                (true, _) => continue,
                (false, 2) => (2, 1),
                (false, _) => (1, 2),
            };
            let weight = if alpha_weighted {
                u64::from(p[3]).pow(2)
            } else {
                1
            };
            aa += weight * a * a;
            ab += weight * a * b;
            bb += weight * b * b;
            for c in 0..3 {
                ax[c] += weight * a * scale * u64::from(p[c]);
                bx[c] += weight * b * scale * u64::from(p[c]);
            }
        }
        // At most sixteen pixels, coefficients <= 3 and weights <= 255^2:
        // these normal-equation products fit u64 exactly. Singular assignments
        // (including a solid palette index) retain the existing candidate.
        let determinant = aa * bb - ab * ab;
        if determinant == 0 {
            break;
        }
        let x = std::array::from_fn(|c| {
            i128::from(ax[c]) * i128::from(bb) - i128::from(bx[c]) * i128::from(ab)
        });
        let y = std::array::from_fn(|c| {
            i128::from(bx[c]) * i128::from(aa) - i128::from(ax[c]) * i128::from(ab)
        });
        let a = pack_ratio(x, i128::from(determinant));
        let b = pack_ratio(y, i128::from(determinant));
        let candidate = color_candidate(pixels, a, b, transparent, bc1, alpha_weighted);
        if candidate.0 >= best.0 {
            break;
        }
        best = candidate;
    }
    best
}
fn fit_pair(
    pixels: &[[u8; 4]; 16],
    a: [u8; 4],
    b: [u8; 4],
    transparent: bool,
    bc1: bool,
    alpha_weighted: bool,
) -> (u64, [u8; 8]) {
    let mut best = color_candidate(pixels, pack(a), pack(b), transparent, bc1, alpha_weighted);
    // The observed extreme colors may be interior palette entries, with neither
    // endpoint present in the pixels. Consider each ordered pair of positions:
    // three seeds for BC1 cutouts, six for a four-color palette. Retain the
    // original seed too, so neither extrapolation nor refinement can worsen it.
    let scale = if transparent { 2 } else { 3 };
    for high in 1..=scale {
        for low in 0..high {
            if best.0 == 0 {
                return best;
            }
            let x = std::array::from_fn(|c| {
                (scale - low) * i128::from(a[c]) - (scale - high) * i128::from(b[c])
            });
            let y = std::array::from_fn(|c| high * i128::from(b[c]) - low * i128::from(a[c]));
            let candidate = color_candidate(
                pixels,
                pack_ratio(x, high - low),
                pack_ratio(y, high - low),
                transparent,
                bc1,
                alpha_weighted,
            );
            let candidate = refine_color(pixels, candidate, transparent, bc1, alpha_weighted);
            if candidate.0 < best.0 {
                best = candidate;
            }
        }
    }
    best
}
fn encode_color(pixels: &[[u8; 4]; 16], bc1: bool, alpha_weighted: bool) -> [u8; 8] {
    let transparent = bc1 && pixels.iter().any(|p| p[3] < 128);
    let valid = |p: &[u8; 4]| if bc1 { p[3] >= 128 } else { p[3] != 0 };
    if !pixels.iter().any(valid) && !bc1 {
        return [0; 8];
    }
    let mut low = [255; 4];
    let mut high = [0; 4];
    let mut pair = ([0; 4], [0; 4]);
    let mut farthest = 0;
    for a in pixels.iter().filter(|p| valid(p)) {
        for c in 0..3 {
            low[c] = low[c].min(a[c]);
            high[c] = high[c].max(a[c]);
        }
        for b in pixels.iter().filter(|p| valid(p)) {
            let d = distance(a, b);
            if d >= farthest {
                farthest = d;
                pair = (*a, *b);
            }
        }
    }
    let mut best = fit_pair(pixels, pair.0, pair.1, transparent, bc1, alpha_weighted);
    let second = fit_pair(pixels, low, high, transparent, bc1, alpha_weighted);
    if second.0 < best.0 {
        best = second;
    }
    if alpha_weighted {
        let max_alpha = pixels.iter().map(|p| p[3]).max().unwrap_or(0);
        let mut pair = ([0; 4], [0; 4]);
        let mut farthest = 0;
        for a in pixels.iter().filter(|p| p[3] == max_alpha) {
            for b in pixels.iter().filter(|p| p[3] == max_alpha) {
                let d = distance(a, b);
                if d >= farthest {
                    farthest = d;
                    pair = (*a, *b);
                }
            }
        }
        let candidate = fit_pair(pixels, pair.0, pair.1, transparent, bc1, true);
        if candidate.0 < best.0 {
            best = candidate;
        }
    }
    best.1
}
fn alpha_candidate(pixels: &[[u8; 4]; 16], a: u8, b: u8) -> (u32, [u8; 8]) {
    let palette = alphas(a, b);
    let mut bits = 0u64;
    let mut error = 0;
    for (i, p) in pixels.iter().enumerate() {
        let j = (0..8)
            .min_by_key(|&j| {
                let d = i32::from(p[3]) - i32::from(palette[j]);
                d * d
            })
            .expect("nonempty alpha palette");
        let d = i32::from(p[3]) - i32::from(palette[j]);
        error += (d * d) as u32;
        bits |= (j as u64) << (3 * i);
    }
    let mut out = [0; 8];
    out[0] = a;
    out[1] = b;
    out[2..].copy_from_slice(&bits.to_le_bytes()[..6]);
    (error, out)
}
fn encode_alpha(pixels: &[[u8; 4]; 16]) -> [u8; 8] {
    let min = pixels.iter().map(|p| p[3]).min().unwrap_or(0);
    let max = pixels.iter().map(|p| p[3]).max().unwrap_or(0);
    let inner_min = pixels
        .iter()
        .map(|p| p[3])
        .filter(|&a| a != 0 && a != 255)
        .min()
        .unwrap_or(0);
    let inner_max = pixels
        .iter()
        .map(|p| p[3])
        .filter(|&a| a != 0 && a != 255)
        .max()
        .unwrap_or(255);
    let first = alpha_candidate(pixels, max, min);
    let second = alpha_candidate(pixels, inner_min, inner_max);
    if second.0 < first.0 {
        second.1
    } else {
        first.1
    }
}
pub(super) fn encode(format: Format, width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    let mut out = zeroed(format.byte_len(width, height)?)?;
    let bw = width.div_ceil(4) as usize;
    for (i, block) in out.chunks_exact_mut(format.unit()).enumerate() {
        let bx = (i % bw) * 4;
        let by = (i / bw) * 4;
        let mut pixels = [[0; 4]; 16];
        // Replicate border pixels into the unused portion of partial edge blocks.
        for (y, row) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            for (x, p) in row.iter_mut().enumerate() {
                let ix = (bx + x).min(width as usize - 1);
                let iy = (by + y).min(height as usize - 1);
                let k = (iy * width as usize + ix) * 4;
                p.copy_from_slice(&rgba[k..k + 4]);
            }
        }
        if format == Format::Dxt1 {
            block.copy_from_slice(&encode_color(&pixels, true, false));
            continue;
        }
        if matches!(format, Format::Dxt2 | Format::Dxt3) {
            for (j, p) in pixels.iter().enumerate() {
                let alpha = ((u16::from(p[3]) * 15 + 127) / 255) as u8;
                block[j / 2] |= alpha << (4 * (j % 2));
            }
        } else {
            block[..8].copy_from_slice(&encode_alpha(&pixels));
        }
        // Fit only colors visible under reconstructed alpha. Premultiplication
        // also uses it, avoiding a separate alpha-quantization darkening error.
        let decoded = decode_block(format, block);
        for (p, d) in pixels.iter_mut().zip(decoded) {
            if format.is_premultiplied() {
                for c in &mut p[..3] {
                    *c = ((u16::from(*c) * u16::from(d[3]) + 127) / 255) as u8;
                }
            }
            p[3] = d[3];
        }
        block[8..].copy_from_slice(&encode_color(&pixels, false, !format.is_premultiplied()));
    }
    Ok(out)
}
