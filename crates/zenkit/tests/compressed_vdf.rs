use zenkit::{
    ErrorKind, Limits,
    vfs::{Compression, ReadOptions, Vdf, WriteOptions},
};

const UNION: &[u8] = include_bytes!("../tests/fixtures/synthetic-union-g2.vdf");
const PILOT: &[u8] = include_bytes!("../tests/fixtures/synthetic-vdf-minimal-g2.vdf");

fn parse(bytes: &[u8]) -> zenkit::Result<Vdf> {
    Vdf::from_bytes(bytes, ReadOptions::default())
}
fn word(bytes: &[u8], at: usize) -> usize {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}
fn set(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn compressed(archive: &Vdf) -> Vec<u8> {
    archive
        .to_bytes_with_options(WriteOptions {
            compression: Compression::Union,
            ..WriteOptions::default()
        })
        .unwrap()
}
fn file<'a>(archive: &'a Vdf, name: &[u8]) -> &'a [u8] {
    archive.payload(archive.find(name).unwrap()).unwrap()
}

// Independently specified one-byte zlib member from RFC 1950 framing/DEFLATE.
// The fixture has no writer dependency and is small enough for every truncation.
fn small() -> Vec<u8> {
    let mut out = PILOT[..296].to_vec();
    set(&mut out, 272, 1);
    set(&mut out, 276, 1);
    set(&mut out, 292, 0xa0);
    out.extend_from_slice(&[b' '; 64]);
    out[296..304].copy_from_slice(b"TINY.DAT");
    for value in [376u32, 1, 0x40000000, 0, 1, 8192, 1, 1, 9, 8192] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&[0x78, 0x9c, 0x4b, 0x04, 0x00, 0x00, 0x62, 0x00, 0x62]);
    let size = out.len() as u32;
    set(&mut out, 284, size);
    out
}

#[test]
fn independent_zlib_fixtures_cover_both_games_empty_multiblock_and_raw_audio() {
    for input in [
        UNION,
        include_bytes!("../tests/fixtures/synthetic-union-g1.vdf").as_slice(),
    ] {
        let archive = parse(input).unwrap();
        assert_eq!(archive.compression(), Compression::Union);
        assert_eq!(file(&archive, b"EMPTY.BIN"), b"");
        assert_eq!(file(&archive, b"TINY.DAT"), b"a");
        assert_eq!(
            file(&archive, b"EXACT.BIN"),
            b"exact-block-12345".repeat(1024)
        );
        assert_eq!(file(&archive, b"SOUND.OGG"), b"OggS\0\xff\x80\0raw audio");
        let mut noisy = Vec::new();
        let mut state = 0x12345678u32;
        for _ in 0..8192 + 123 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            noisy.push(state as u8);
        }
        assert_eq!(file(&archive, b"MULTI.BIN"), noisy);
        let mut wave = b"RIFF".to_vec();
        wave.extend((0..=255).cycle().take(256 * 33));
        assert_eq!(file(&archive, b"VOICE.WAV"), wave);
    }
}

#[test]
fn writer_converts_both_directions_and_preserves_metadata() {
    for input in [PILOT, UNION] {
        let archive = parse(input).unwrap();
        for encoded in [archive.to_bytes().unwrap(), compressed(&archive)] {
            let again = parse(&encoded).unwrap();
            assert_eq!(&encoded[..284], &input[..284]);
            assert_eq!(word(&encoded, 284), encoded.len());
            assert_eq!(archive.signature(), again.signature());
            for index in 0..archive.entries().len() {
                let a = &archive.entries()[index];
                let b = &again.entries()[index];
                assert_eq!(a.name(), b.name());
                assert_eq!(a.flags(), b.flags());
                assert_eq!(a.attributes(), b.attributes());
                assert_eq!(a.parent(), b.parent());
                assert_eq!(archive.payload(index), again.payload(index));
            }
        }
    }
}

#[test]
fn every_truncation_is_rejected_including_zlib_trailer() {
    let input = small();
    assert_eq!(file(&parse(&input).unwrap(), b"TINY.DAT"), b"a");
    for size in 0..input.len() {
        assert!(parse(&input[..size]).is_err(), "accepted length {size}");
    }
}

#[test]
fn rejects_header_and_block_inconsistencies_without_raw_fallback() {
    let input = small();
    for (at, value) in [
        (376, 0),
        (376, 2),
        (380, 0),
        (380, u32::MAX),
        (384, 0),
        (384, 2),
        (384, u32::MAX),
        (388, 0),
        (388, 2),
        (392, 0),
        (392, u32::MAX),
        (396, 0),
        (396, 8191),
        (360, 297),
    ] {
        let mut bytes = input.clone();
        set(&mut bytes, at, value);
        assert!(parse(&bytes).is_err(), "accepted {at} = {value}");
    }
    for at in [400, 402, 408] {
        let mut bytes = input.clone();
        bytes[at] ^= 0xff;
        assert_eq!(parse(&bytes).unwrap_err().kind, ErrorKind::InvalidData);
    }
}

#[test]
fn requires_exact_zlib_consumption_and_decoded_length() {
    let mut bytes = small();
    bytes.push(0);
    set(&mut bytes, 392, 10);
    assert_eq!(parse(&bytes).unwrap_err().kind, ErrorKind::InvalidData);
    let mut bytes = small();
    for at in [364, 376, 388] {
        set(&mut bytes, at, 2);
    }
    assert_eq!(parse(&bytes).unwrap_err().kind, ErrorKind::InvalidData);
    let mut bytes = small();
    bytes.truncate(400);
    // A valid zlib member for "ab" must not fit a one-byte decoded declaration.
    bytes.extend_from_slice(&[0x78, 0x9c, 0x4b, 0x4c, 0x02, 0x00, 0x01, 0x26, 0x00, 0xc4]);
    set(&mut bytes, 392, 10);
    assert_eq!(parse(&bytes).unwrap_err().kind, ErrorKind::InvalidData);
}

#[test]
fn every_multiblock_source_length_is_checked() {
    let start = word(UNION, 360);
    let last = start + 24 + word(UNION, start + 16);
    for (at, length) in [(start + 12, 8191), (last, 124), (last + 8, 4096)] {
        let mut bytes = UNION.to_vec();
        set(&mut bytes, at, length);
        assert_eq!(parse(&bytes).unwrap_err().kind, ErrorKind::InvalidData);
    }
}

#[test]
fn encoded_stream_cannot_cross_next_file_or_catalog_boundary() {
    let mut bytes = UNION.to_vec();
    let first = word(&bytes, 360);
    // Claim bytes occupied by the following EMPTY.BIN stream as first-block data.
    let next = word(&bytes, 440);
    set(&mut bytes, first + 16, (next - first) as u32);
    assert_eq!(parse(&bytes).unwrap_err().kind, ErrorKind::UnexpectedEof);
    let mut bytes = small();
    // A payload before the catalog may not interpret the catalog as zlib data.
    let catalog = bytes[296..376].to_vec();
    let stream = bytes[376..].to_vec();
    bytes.truncate(296);
    bytes.extend_from_slice(&stream[..20]);
    bytes.extend_from_slice(&catalog);
    let at = 316;
    set(&mut bytes, 288, at as u32);
    set(&mut bytes, at + 64, 296);
    assert_eq!(parse(&bytes).unwrap_err().kind, ErrorKind::UnexpectedEof);
}

#[test]
fn empty_stream_requires_canonical_lengths_and_stays_empty() {
    let mut bytes = small();
    bytes.truncate(388);
    set(&mut bytes, 364, 0);
    set(&mut bytes, 376, 0);
    set(&mut bytes, 384, 0);
    let archive = parse(&bytes).unwrap();
    assert_eq!(file(&archive, b"TINY.DAT"), b"");
    assert_eq!(compressed(&archive).len(), 388);
    let limits = Limits {
        compression_blocks: 0,
        ..Limits::default()
    };
    assert!(Vdf::from_bytes(&bytes, ReadOptions { limits }).is_ok());
    let limits = Limits {
        compression_input_bytes: 11,
        ..Limits::default()
    };
    assert_eq!(
        Vdf::from_bytes(&bytes, ReadOptions { limits })
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    for (at, value) in [(380, 0), (384, 1), (376, 1)] {
        let mut bad = bytes.clone();
        set(&mut bad, at, value);
        assert!(parse(&bad).is_err());
    }
}

#[test]
fn bounds_decompressed_payloads_blocks_and_output() {
    for limits in [
        Limits {
            file_payload_bytes: 8192,
            ..Limits::default()
        },
        Limits {
            payload_bytes: 9000,
            ..Limits::default()
        },
        Limits {
            compression_blocks: 1,
            ..Limits::default()
        },
        Limits {
            compression_block_bytes: 8191,
            ..Limits::default()
        },
        Limits {
            compression_input_bytes: 100,
            ..Limits::default()
        },
    ] {
        assert_eq!(
            Vdf::from_bytes(UNION, ReadOptions { limits })
                .unwrap_err()
                .kind,
            ErrorKind::LimitExceeded
        );
    }
    let archive = parse(UNION).unwrap();
    for mode in [Compression::None, Compression::Union] {
        for limit in [295, 1000] {
            let options = WriteOptions {
                compression: mode,
                output_bytes: limit,
            };
            assert_eq!(
                archive.to_bytes_with_options(options).unwrap_err().kind,
                ErrorKind::LimitExceeded
            );
        }
    }
}

#[test]
fn raw_audio_that_looks_like_a_stream_is_not_decompressed() {
    let mut bytes = small();
    bytes[296..304].copy_from_slice(b"TINY.WaV");
    set(&mut bytes, 364, 33);
    let archive = parse(&bytes).unwrap();
    assert_eq!(file(&archive, b"TINY.WAV"), &bytes[376..]);
    let again = parse(&compressed(&archive)).unwrap();
    assert_eq!(file(&again, b"TINY.WAV"), &bytes[376..]);
}

#[test]
fn aliasing_streams_count_against_aggregate_work_limit() {
    let source = small();
    let mut bytes = source[..376].to_vec();
    bytes.extend_from_slice(&source[296..376]);
    bytes[376] = b'B';
    bytes.extend_from_slice(&source[376..]);
    set(&mut bytes, 272, 2);
    set(&mut bytes, 276, 2);
    set(&mut bytes, 360, 456);
    set(&mut bytes, 368, 0);
    set(&mut bytes, 440, 456);
    assert!(parse(&bytes).is_ok());
    let limits = Limits {
        compression_blocks: 1,
        ..Limits::default()
    };
    assert_eq!(
        Vdf::from_bytes(&bytes, ReadOptions { limits })
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    let limits = Limits {
        payload_bytes: 1,
        ..Limits::default()
    };
    assert_eq!(
        Vdf::from_bytes(&bytes, ReadOptions { limits })
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    let limits = Limits {
        compression_input_bytes: 33,
        ..Limits::default()
    };
    assert_eq!(
        Vdf::from_bytes(&bytes, ReadOptions { limits })
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
}

#[test]
fn compressed_writer_refuses_unknown_trailers() {
    let mut bytes = UNION.to_vec();
    bytes.push(42);
    let archive = parse(&bytes).unwrap();
    assert_eq!(archive.to_bytes().unwrap_err().kind, ErrorKind::Unsupported);
    assert_eq!(
        archive
            .to_bytes_with_options(WriteOptions {
                compression: Compression::Union,
                ..WriteOptions::default()
            })
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
}
