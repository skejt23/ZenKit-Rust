// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
//! Union ZippedStream framing and zlib compression.
use crate::{Error, ErrorKind, Limits, Result, io::Reader};
use miniz_oxide::{
    deflate::compress_to_vec_zlib,
    inflate::{
        TINFLStatus,
        core::{DecompressorOxide, decompress, inflate_flags},
    },
};
use std::ops::Range;

const BLOCK_SIZE: usize = 8192;

fn invalid(offset: usize, field: &'static str) -> Error {
    Error::new(ErrorKind::InvalidData, offset, field)
}

fn limited(value: usize, maximum: usize, offset: usize, field: &'static str) -> Result<()> {
    if value > maximum {
        return Err(Error::new(ErrorKind::LimitExceeded, offset, field));
    }
    Ok(())
}

#[derive(Default)]
pub(super) struct Budget {
    blocks: usize,
    encoded_bytes: usize,
}

pub(super) fn decode(
    bytes: &[u8],
    extent: Range<usize>,
    expected_size: usize,
    limits: Limits,
    budget: &mut Budget,
) -> Result<(Vec<u8>, usize)> {
    let mut whole = Reader::new(bytes);
    whole.take_bytes(extent.start, "Union stream offset")?;
    let mut reader = whole.substream(extent.len(), "Union stream extent")?;
    let length = reader.u32("Union decoded length")? as usize;
    let block_size = reader.u32("Union block size")? as usize;
    let block_count = reader.u32("Union block count")? as usize;
    if length != expected_size || block_size == 0 {
        return Err(invalid(
            extent.start,
            "Union stream/catalog length or block size",
        ));
    }
    limited(
        length,
        limits.file_payload_bytes.min(limits.payload_bytes),
        extent.start,
        "Union decoded payload",
    )?;
    limited(
        block_size,
        limits.compression_block_bytes,
        extent.start + 4,
        "Union block size",
    )?;
    // Avoid length + block_size - 1, which can overflow for file-controlled lengths.
    if block_count != length.div_ceil(block_size) {
        return Err(invalid(extent.start + 8, "Union block count mismatch"));
    }
    budget.blocks = budget
        .blocks
        .checked_add(block_count)
        .ok_or_else(|| invalid(extent.start + 8, "Union total block count"))?;
    limited(
        budget.blocks,
        limits.compression_blocks,
        extent.start + 8,
        "Union total block count",
    )?;

    // Every block needs a 12-byte header and a nonempty encoded payload. Reject
    // impossible framing and input budgets before count-controlled allocation.
    let minimum = block_count
        .checked_mul(13)
        .and_then(|n| n.checked_add(12))
        .ok_or_else(|| invalid(extent.start, "Union minimum stream size"))?;
    limited(
        budget
            .encoded_bytes
            .checked_add(minimum)
            .ok_or_else(|| invalid(extent.start, "Union total encoded bytes"))?,
        limits.compression_input_bytes,
        extent.start,
        "Union total encoded bytes",
    )?;
    if minimum - 12 > reader.remaining() {
        return Err(Error::new(
            ErrorKind::UnexpectedEof,
            reader.position(),
            "Union minimum block framing",
        ));
    }
    // Framing is fully checked before allocating decoded output or invoking zlib.
    let mut blocks = Vec::new();
    blocks.try_reserve_exact(block_count).map_err(|_| {
        Error::new(
            ErrorKind::LimitExceeded,
            extent.start + 8,
            "Union block descriptor allocation",
        )
    })?;
    for index in 0..block_count {
        let at = reader.position();
        let source_len = reader.u32("Union block decoded length")? as usize;
        let encoded_len = reader.u32("Union block encoded length")? as usize;
        let nominal_size = reader.u32("Union block nominal size")? as usize;
        let expected = (length - index * block_size).min(block_size);
        if source_len != expected || nominal_size != block_size || encoded_len == 0 {
            return Err(invalid(at, "Union block lengths"));
        }
        limited(
            encoded_len,
            limits.compression_block_bytes,
            at + 4,
            "Union encoded block length",
        )?;
        let start = reader.position();
        reader.take_bytes(encoded_len, "Union encoded block extent")?;
        blocks.push((start..reader.position(), source_len));
    }
    let end = reader.position();
    budget.encoded_bytes = budget
        .encoded_bytes
        .checked_add(end - extent.start)
        .ok_or_else(|| invalid(extent.start, "Union total encoded bytes"))?;
    limited(
        budget.encoded_bytes,
        limits.compression_input_bytes,
        extent.start,
        "Union total encoded bytes",
    )?;
    let mut output = Vec::new();
    output.try_reserve_exact(length).map_err(|_| {
        Error::new(
            ErrorKind::LimitExceeded,
            extent.start,
            "Union decoded allocation",
        )
    })?;
    output.resize(length, 0);
    if output.is_empty() {
        return Ok((output, end));
    }
    let mut position = 0;
    let mut state = Box::<DecompressorOxide>::default();
    for (range, source_len) in blocks {
        state.init();
        let (status, consumed, written) = decompress(
            &mut state,
            &bytes[range.clone()],
            &mut output[position..position + source_len],
            0,
            inflate_flags::TINFL_FLAG_PARSE_ZLIB_HEADER
                | inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
        );
        if status != TINFLStatus::Done || consumed != range.len() || written != source_len {
            return Err(Error::new(
                ErrorKind::InvalidData,
                range.start,
                "Union zlib stream, checksum or exact lengths",
            ));
        }
        position += source_len;
    }
    Ok((output, end))
}

pub(super) fn append(output: &mut Vec<u8>, bytes: &[u8], limit: usize) -> Result<()> {
    let end = output
        .len()
        .checked_add(bytes.len())
        .ok_or_else(|| Error::operation(ErrorKind::LimitExceeded, "VDF output size"))?;
    if end > limit.min(u32::MAX as usize) {
        return Err(Error::operation(
            ErrorKind::LimitExceeded,
            "VDF output bytes",
        ));
    }
    output.extend_from_slice(bytes);
    Ok(())
}

fn word(output: &mut Vec<u8>, value: usize, limit: usize) -> Result<()> {
    let value = u32::try_from(value)
        .map_err(|_| Error::operation(ErrorKind::LimitExceeded, "Union 32-bit length"))?;
    append(output, &value.to_le_bytes(), limit)
}

pub(super) fn encode(payload: &[u8], output: &mut Vec<u8>, limit: usize) -> Result<()> {
    word(output, payload.len(), limit)?;
    word(output, BLOCK_SIZE, limit)?;
    word(output, payload.len().div_ceil(BLOCK_SIZE), limit)?;
    for source in payload.chunks(BLOCK_SIZE) {
        // Fixed block size bounds codec allocations.
        let encoded = compress_to_vec_zlib(source, 6);
        word(output, source.len(), limit)?;
        word(output, encoded.len(), limit)?;
        word(output, BLOCK_SIZE, limit)?;
        append(output, &encoded, limit)?;
    }
    Ok(())
}
