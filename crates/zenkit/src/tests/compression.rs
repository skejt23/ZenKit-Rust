use crate::{
    ErrorKind, Limits,
    compression::{Budget, decode, encode},
};

const TINY: &[u8] = include_bytes!("../../tests/fixtures/synthetic-zipped-tiny.bin");
const EMPTY: &[u8] = include_bytes!("../../tests/fixtures/synthetic-zipped-empty.bin");
const SMALL: &[u8] = include_bytes!("../../tests/fixtures/synthetic-zipped-small-blocks.bin");
const MULTI: &[u8] = include_bytes!("../../tests/fixtures/synthetic-zipped-multiblock.bin");
const EXACT: &[u8] = include_bytes!("../../tests/fixtures/synthetic-zipped-exact.bin");

fn set(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn independent_frames_decode_without_consuming_the_following_frame() {
    let multi: Vec<_> = (0..8192 + 379)
        .map(|i| ((i * 73 + i / 257) & 255) as u8)
        .collect();
    for (encoded, expected) in [
        (TINY, b"a".to_vec()),
        (EMPTY, vec![]),
        (SMALL, b"\0\x80\xffraw\n".repeat(9)),
        (MULTI, multi),
        (EXACT, b"whole-block-12345".repeat(1024)),
    ] {
        let mut bytes = vec![0x55; 17];
        bytes.extend(encoded);
        bytes.extend(EMPTY);
        bytes.extend(b"suffix");
        let mut budget = Budget::default();
        let (decoded, end) = decode(
            &bytes,
            17..bytes.len(),
            expected.len(),
            Limits::default(),
            &mut budget,
        )
        .unwrap();
        assert_eq!(decoded, expected);
        assert_eq!(end, 17 + encoded.len());
        let (empty, next) =
            decode(&bytes, end..bytes.len(), 0, Limits::default(), &mut budget).unwrap();
        assert!(empty.is_empty());
        assert_eq!(&bytes[next..], b"suffix");
    }
}

#[test]
fn every_truncation_reports_an_absolute_physical_offset() {
    for count in 0..TINY.len() {
        let mut bytes = vec![0x55; 13];
        bytes.extend(&TINY[..count]);
        let error = decode(
            &bytes,
            13..bytes.len(),
            1,
            Limits::default(),
            &mut Budget::default(),
        )
        .unwrap_err();
        assert_eq!(error.kind, ErrorKind::UnexpectedEof, "{count}: {error}");
        assert!((13..=bytes.len()).contains(&error.offset.unwrap()));
    }
}

#[test]
fn decoded_sizes_and_compression_work_remain_bounded() {
    for limits in [
        Limits {
            file_payload_bytes: 0,
            ..Limits::default()
        },
        Limits {
            payload_bytes: 0,
            ..Limits::default()
        },
        Limits {
            compression_blocks: 0,
            ..Limits::default()
        },
        Limits {
            compression_block_bytes: 8191,
            ..Limits::default()
        },
        Limits {
            compression_input_bytes: TINY.len() - 1,
            ..Limits::default()
        },
    ] {
        assert_eq!(
            decode(TINY, 0..TINY.len(), 1, limits, &mut Budget::default())
                .unwrap_err()
                .kind,
            ErrorKind::LimitExceeded
        );
    }
    let limits = Limits {
        compression_blocks: 1,
        ..Limits::default()
    };
    let mut budget = Budget::default();
    decode(TINY, 0..TINY.len(), 1, limits, &mut budget).unwrap();
    assert_eq!(
        decode(TINY, 0..TINY.len(), 1, limits, &mut budget)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
}

#[test]
fn malformed_headers_and_checksums_never_fall_back_to_raw_bytes() {
    for (at, value) in [(0, 2), (4, 0), (8, 2), (12, 2), (16, 0), (20, 8191)] {
        let mut wire = TINY.to_vec();
        set(&mut wire, at, value);
        assert_eq!(
            decode(
                &wire,
                0..wire.len(),
                1,
                Limits::default(),
                &mut Budget::default()
            )
            .unwrap_err()
            .kind,
            ErrorKind::InvalidData
        );
    }
    let mut bytes = vec![0; 11];
    bytes.extend(TINY);
    *bytes.last_mut().unwrap() ^= 1;
    let error = decode(
        &bytes,
        11..bytes.len(),
        1,
        Limits::default(),
        &mut Budget::default(),
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidData);
    assert_eq!(error.offset, Some(11 + 24));
}

#[test]
fn corruption_in_a_later_block_rejects_the_entire_payload() {
    let mut bytes = SMALL.to_vec();
    let first_size = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    let second_payload = 24 + first_size + 12;
    bytes[second_payload] ^= 0xff;
    let error = decode(
        &bytes,
        0..bytes.len(),
        63,
        Limits::default(),
        &mut Budget::default(),
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidData);
    assert_eq!(error.offset, Some(second_payload));
}

#[test]
fn impossible_block_counts_fail_before_descriptor_allocation() {
    let mut bytes = EMPTY.to_vec();
    set(&mut bytes, 0, u32::MAX);
    set(&mut bytes, 4, 1);
    set(&mut bytes, 8, u32::MAX);
    let limits = Limits {
        file_payload_bytes: usize::MAX,
        payload_bytes: usize::MAX,
        compression_input_bytes: usize::MAX,
        compression_blocks: usize::MAX,
        ..Limits::default()
    };
    let error = decode(
        &bytes,
        0..bytes.len(),
        u32::MAX as usize,
        limits,
        &mut Budget::default(),
    )
    .unwrap_err();
    assert!(matches!(
        error.kind,
        ErrorKind::UnexpectedEof | ErrorKind::InvalidData
    ));
}

#[test]
fn encoder_handles_empty_partial_and_exact_block_boundaries() {
    for size in [0, 1, 8191, 8192, 8193, 16384] {
        let payload: Vec<_> = (0..size).map(|i| (i * 73) as u8).collect();
        let mut bytes = Vec::new();
        encode(&payload, &mut bytes, usize::MAX).unwrap();
        let (decoded, end) = decode(
            &bytes,
            0..bytes.len(),
            size,
            Limits::default(),
            &mut Budget::default(),
        )
        .unwrap();
        assert_eq!(decoded, payload);
        assert_eq!(end, bytes.len());
    }
}
