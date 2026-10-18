use std::io::SeekFrom;
use zenkit::{
    ErrorKind,
    io::{Reader, Writer},
    values::*,
};

fn tree() -> Obb {
    Obb {
        center: Vec3::from([32.0, 64.0, 128.0]),
        axes: [
            Vec3::from([1.0, 1.0, 0.0]),
            Vec3::from([-1.0, 1.0, 0.0]),
            Vec3::from([0.0, 0.0, 2.0]),
        ],
        half_width: Vec3::from([2.0, 3.0, 4.0]),
        children: vec![
            Obb {
                center: Vec3::splat(-10.0),
                axes: Mat3::IDENTITY.columns,
                half_width: Vec3::from([1.0, 2.0, 3.0]),
                children: vec![Obb::default()],
            },
            Obb {
                center: Vec3::splat(99.0),
                ..Obb::default()
            },
        ],
    }
}
fn encoded(value: &Obb) -> Vec<u8> {
    let mut out = Writer::new(1024 * 1024);
    value.write(&mut out, ObbLimits::default(), "tree").unwrap();
    out.into_inner()
}

#[test]
fn date_is_a_lossless_record_not_a_calendar_or_timestamp() {
    let bytes = [0xff; 16];
    let value = Date::read(&mut Reader::new(&bytes), "date").unwrap();
    assert_eq!(
        value,
        Date {
            year: u32::MAX,
            month: u16::MAX,
            day: u16::MAX,
            hour: u16::MAX,
            minute: u16::MAX,
            second: u16::MAX,
            reserved: u16::MAX
        }
    );
    let mut out = Writer::new(16);
    value.write(&mut out, "date").unwrap();
    assert_eq!(out.as_ref(), bytes);
    assert_eq!(Date::default().reserved, 0);
}

#[test]
fn vectors_matrices_and_bounds_preserve_arbitrary_float_bits() {
    let bits = [
        0x80000000u32,
        0,
        0x7fc01234,
        0xff800000,
        0x7f800000,
        1,
        u32::MAX,
    ];
    macro_rules! check {
        ($ty:ty, $size:expr) => {{
            let bytes: Vec<_> = (0..$size / 4)
                .flat_map(|i| bits[i % bits.len()].to_le_bytes())
                .collect();
            let value = <$ty>::read(&mut Reader::new(&bytes), "value").unwrap();
            let mut out = Writer::new($size);
            value.write(&mut out, "value").unwrap();
            assert_eq!(out.as_ref(), bytes);
        }};
    }
    check!(Vec2, 8);
    check!(Vec3, 12);
    check!(Vec4, 16);
    check!(Mat3, 36);
    check!(Mat4, 64);
    check!(Aabb, 24);
}

#[test]
fn fixed_records_roll_back_at_every_truncation_and_report_absolute_field_offsets() {
    macro_rules! check {
        ($ty:ty, $size:expr) => {{
            for length in 0..$size {
                let bytes = vec![0; length + 7];
                let mut parent = Reader::new(&bytes);
                parent.take_bytes(4, "prefix").unwrap();
                let mut child = parent.substream(length + 3, "chunk").unwrap();
                child.take_bytes(3, "tag").unwrap();
                let error = <$ty>::read(&mut child, "value").unwrap_err();
                assert_eq!(error.kind, ErrorKind::UnexpectedEof);
                assert_eq!(error.context, "value");
                assert!(error.offset.unwrap() >= 7 && error.offset.unwrap() <= length + 7);
                assert_eq!(child.position(), 7);
            }
        }};
    }
    check!(Date, 16);
    check!(Vec2, 8);
    check!(Vec3, 12);
    check!(Vec4, 16);
    check!(Mat3, 36);
    check!(Mat4, 64);
    check!(Aabb, 24);
    let mut parent = Reader::new(&[0; 20]);
    parent.take_bytes(5, "prefix").unwrap();
    let mut child = parent.substream(15, "date").unwrap();
    assert_eq!(Date::read(&mut child, "date").unwrap_err().offset, Some(19));
    assert_eq!(child.position(), 5);
}

#[test]
fn fixed_record_writes_roll_back_overwrites_at_every_output_limit() {
    macro_rules! check {
        ($ty:ty, $size:expr) => {{
            for limit in 4..$size + 2 {
                let mut out = Writer::new(limit);
                out.bytes(b"keep", "prefix").unwrap();
                out.seek_to(SeekFrom::Start(2), "seek").unwrap();
                let error = <$ty>::default().write(&mut out, "value").unwrap_err();
                assert_eq!(error.kind, ErrorKind::LimitExceeded);
                assert_eq!(error.offset, None);
                assert_eq!(out.position(), 2);
                assert_eq!(out.as_ref(), b"keep");
            }
        }};
    }
    check!(Date, 16);
    check!(Vec2, 8);
    check!(Vec3, 12);
    check!(Vec4, 16);
    check!(Mat3, 36);
    check!(Mat4, 64);
    check!(Aabb, 24);
}

#[test]
fn nonsymmetric_matrices_have_explicit_columns_and_row_major_wire_order() {
    let bytes: Vec<_> = (1..=9).flat_map(|i| (i as f32).to_le_bytes()).collect();
    let matrix = Mat3::read(&mut Reader::new(&bytes), "matrix").unwrap();
    assert_eq!(
        matrix.columns,
        [
            Vec3::from([1.0, 4.0, 7.0]),
            Vec3::from([2.0, 5.0, 8.0]),
            Vec3::from([3.0, 6.0, 9.0])
        ]
    );
    assert_eq!(
        matrix.to_rows(),
        [
            Vec3::from([1.0, 2.0, 3.0]),
            Vec3::from([4.0, 5.0, 6.0]),
            Vec3::from([7.0, 8.0, 9.0])
        ]
    );
    assert_eq!(matrix.transpose().transpose(), matrix);
    assert_eq!(Mat3::from_rows(matrix.to_rows()), matrix);
    assert_eq!(Mat3::IDENTITY, Mat3::IDENTITY.transpose());
    let rows = std::array::from_fn(|r| Vec4::from(std::array::from_fn(|c| (r * 4 + c + 1) as f32)));
    let matrix = Mat4::from_rows(rows);
    assert_eq!(matrix[3][0], 4.0);
    assert_eq!(matrix[0][3], 13.0);
    assert_eq!(matrix.transpose().transpose(), matrix);
    let mut out = Writer::new(64);
    matrix.write(&mut out, "matrix").unwrap();
    assert_eq!(
        out.as_ref(),
        (1..=16)
            .flat_map(|i| (i as f32).to_le_bytes())
            .collect::<Vec<_>>()
    );
    assert_eq!(Mat4::IDENTITY[3].to_array(), [0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn native_value_operations_and_safe_indexing() {
    let mut pair = Vec2::from([2.0, -3.0]);
    pair[0] = 4.0;
    assert_eq!(<[f32; 2]>::from(pair), [4.0, -3.0]);
    assert_eq!((pair * -2.0).to_array(), [-8.0, 6.0]);
    assert_eq!((pair * Vec2::from([3.0, 5.0])).to_array(), [12.0, -15.0]);
    assert_eq!(Vec2::splat(7.0).to_array(), [7.0, 7.0]);
    assert_eq!(Vec2::default(), Vec2::ZERO);

    let mut vector = Vec3::from([2.0, 3.0, 4.0]);
    vector[1] = 5.0;
    assert_eq!(<[f32; 3]>::from(vector), [2.0, 5.0, 4.0]);
    assert_eq!((vector * 2.0).to_array(), [4.0, 10.0, 8.0]);
    assert_eq!((vector * vector).to_array(), [4.0, 25.0, 16.0]);
    assert_eq!(Vec3::splat(7.0).to_array(), [7.0, 7.0, 7.0]);
    assert_eq!(Vec3::default(), Vec3::ZERO);

    let mut quad = Vec4::from([2.0, -3.0, 4.0, -5.0]);
    quad[3] = 6.0;
    let mut components = quad.to_array();
    components[0] = 99.0;
    assert_eq!(components, [99.0, -3.0, 4.0, 6.0]);
    assert_eq!(<[f32; 4]>::from(quad), [2.0, -3.0, 4.0, 6.0]);
    assert_eq!((quad * -2.0).to_array(), [-4.0, 6.0, -8.0, -12.0]);
    assert_eq!(
        (quad * Vec4::from([3.0, 5.0, -7.0, 9.0])).to_array(),
        [6.0, -15.0, -28.0, 54.0]
    );
    assert_eq!(Vec4::splat(7.0).to_array(), [7.0; 4]);
    assert_eq!(Vec4::default(), Vec4::ZERO);

    let mut matrix = Mat3::IDENTITY;
    matrix[2][0] = 8.0;
    let mut columns = matrix.columns;
    columns[2][0] = 9.0;
    assert_eq!(matrix[2][0], 8.0);
    assert_eq!(columns[2][0], 9.0);
    assert_eq!(matrix.to_rows()[0], Vec3::from([1.0, 0.0, 8.0]));

    assert!(std::panic::catch_unwind(|| pair[2]).is_err());
    assert!(std::panic::catch_unwind(|| vector[3]).is_err());
    assert!(std::panic::catch_unwind(|| quad[4]).is_err());
    assert!(std::panic::catch_unwind(|| Mat3::IDENTITY[3]).is_err());
    assert!(std::panic::catch_unwind(|| Mat4::IDENTITY[4]).is_err());
    assert!(std::panic::catch_unwind(|| Color::default()[4]).is_err());
}

#[test]
fn colors_wrap_explicitly_and_quaternions_name_their_component_order() {
    let color = Color::from_rgba([250, 128, 2, 255]);
    assert_eq!(color.to_bgra(), [2, 128, 250, 255]);
    assert_eq!(Color::from_bgra(color.to_bgra()), color);
    assert_eq!((color * 2).to_rgba(), [244, 0, 4, 254]);
    assert_eq!(
        (color * Color::from([2, 2, 2, 2])).to_rgba(),
        [244, 0, 4, 254]
    );
    let quat = Quat::from_wxyz([1.0, 2.0, 3.0, 4.0]);
    assert_eq!(quat.to_xyzw(), [2.0, 3.0, 4.0, 1.0]);
    assert_eq!(Quat::from_xyzw(quat.to_xyzw()), quat);
    assert_eq!(Quat::IDENTITY.to_wxyz(), [1.0, 0.0, 0.0, 0.0]);
    assert_eq!(Quat::default().to_wxyz(), [0.0; 4]);
}

#[test]
fn obb_round_trip_preserves_nested_child_order_and_all_float_bits() {
    let value = tree();
    let bytes = encoded(&value);
    assert_eq!(bytes.len(), 62 * 4);
    assert_eq!(&bytes[60..62], &[2, 0]);
    assert_eq!(&bytes[122..124], &[1, 0]);
    assert_eq!(
        Obb::read(
            &mut Reader::new(&bytes),
            ObbLimits { nodes: 4, depth: 3 },
            "tree"
        )
        .unwrap(),
        value
    );
    let mut bytes = bytes;
    for offset in [0, 64, 128, 188] {
        bytes[offset..offset + 4].copy_from_slice(&0x7fc12345u32.to_le_bytes());
    }
    let value = Obb::read(&mut Reader::new(&bytes), ObbLimits::default(), "tree").unwrap();
    assert_eq!(encoded(&value), bytes);
}

#[test]
fn obb_read_is_atomic_for_every_truncated_record() {
    let bytes = [b"tag".as_slice(), &encoded(&tree())].concat();
    for length in 3..bytes.len() {
        let mut parent = Reader::new(&bytes[..length]);
        parent.take_bytes(3, "prefix").unwrap();
        let mut child = parent.substream(length - 3, "chunk").unwrap();
        let error = Obb::read(&mut child, ObbLimits::default(), "tree").unwrap_err();
        assert_eq!(error.kind, ErrorKind::UnexpectedEof);
        assert!(error.offset.unwrap() >= 3 && error.offset.unwrap() <= length);
        assert_eq!(child.position(), 3);
    }
}

#[test]
fn obb_node_and_depth_budgets_are_cumulative_and_have_absolute_offsets() {
    let bytes = [b"tag".as_slice(), &encoded(&tree())].concat();
    for (limits, offset) in [
        (
            ObbLimits {
                nodes: 0,
                depth: 128,
            },
            3,
        ),
        (
            ObbLimits {
                nodes: 100,
                depth: 0,
            },
            3,
        ),
        (
            ObbLimits {
                nodes: 1,
                depth: 128,
            },
            63,
        ),
        (
            ObbLimits {
                nodes: 100,
                depth: 1,
            },
            63,
        ),
        (
            ObbLimits {
                nodes: 100,
                depth: 2,
            },
            125,
        ),
        (
            ObbLimits {
                nodes: 3,
                depth: 128,
            },
            189,
        ),
    ] {
        let mut input = Reader::new(&bytes);
        input.take_bytes(3, "prefix").unwrap();
        let error = Obb::read(&mut input, limits, "tree").unwrap_err();
        assert_eq!(error.kind, ErrorKind::LimitExceeded);
        assert_eq!(error.offset, Some(offset));
        assert_eq!(input.position(), 3);
    }
}

#[test]
fn obb_hard_depth_cap_applies_even_to_unbounded_requested_limits() {
    let mut value = Obb::default();
    for _ in 1..Obb::MAX_DEPTH {
        value = Obb {
            children: vec![value],
            ..Obb::default()
        };
    }
    let limits = ObbLimits {
        nodes: usize::MAX,
        depth: usize::MAX,
    };
    let mut output = Writer::new(100_000);
    value.write(&mut output, limits, "tree").unwrap();
    Obb::read(&mut Reader::new(output.as_ref()), limits, "tree").unwrap();
    let parent = Obb {
        children: vec![value],
        ..Obb::default()
    };
    let before = output.as_slice().to_vec();
    assert_eq!(
        parent.write(&mut output, limits, "tree").unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(output.as_ref(), before);
    let mut too_deep = vec![0; 62];
    too_deep[60] = 1;
    too_deep.extend_from_slice(&before);
    let mut input = Reader::new(&too_deep);
    assert_eq!(
        Obb::read(&mut input, limits, "tree").unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(input.position(), 0);
}

#[test]
fn obb_max_depth_and_late_failure_fit_a_small_worker_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            // A root with a depth-127 branch and a final sibling exercises both
            // parent unwinding and cleanup of an already completed deep branch.
            let mut bytes = vec![0; 62];
            bytes[60] = 2;
            for depth in 1..Obb::MAX_DEPTH {
                bytes.extend_from_slice(&[0; 60]);
                bytes.extend_from_slice(&u16::from(depth + 1 < Obb::MAX_DEPTH).to_le_bytes());
            }
            bytes.extend_from_slice(&[0; 62]);
            let limits = ObbLimits {
                nodes: Obb::MAX_DEPTH + 1,
                depth: Obb::MAX_DEPTH,
            };
            let tree = Obb::read(&mut Reader::new(&bytes), limits, "tree").unwrap();
            let mut output = Writer::new(bytes.len());
            tree.write(&mut output, limits, "tree").unwrap();
            assert_eq!(output.as_slice(), bytes);

            for length in [bytes.len() - 63, bytes.len() - 1] {
                let mut input = Reader::new(&bytes[..length]);
                let error = Obb::read(&mut input, limits, "tree").unwrap_err();
                assert_eq!(error.kind, ErrorKind::UnexpectedEof);
                assert_eq!(input.position(), 0);
            }
            let mut output = Writer::new(bytes.len() - 1);
            output.bytes(b"keep", "prefix").unwrap();
            output.seek_to(SeekFrom::Start(0), "seek").unwrap();
            assert_eq!(
                tree.write(&mut output, limits, "tree").unwrap_err().kind,
                ErrorKind::LimitExceeded
            );
            assert_eq!(output.as_slice(), b"keep");
            assert_eq!(output.position(), 0);

            let parent = Obb {
                children: vec![tree],
                ..Obb::default()
            };
            assert_eq!(
                parent
                    .write(&mut Writer::new(100_000), ObbLimits::default(), "tree")
                    .unwrap_err()
                    .kind,
                ErrorKind::LimitExceeded
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn obb_writer_rejects_counts_that_do_not_fit_the_wire_and_rolls_back() {
    let value = Obb {
        children: vec![Obb::default(); 65_536],
        ..Obb::default()
    };
    let mut output = Writer::new(10_000_000);
    output.bytes(b"keep", "prefix").unwrap();
    output.seek_to(SeekFrom::Start(1), "seek").unwrap();
    let error = value
        .write(&mut output, ObbLimits::default(), "tree")
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::LimitExceeded);
    assert_eq!(error.offset, None);
    assert_eq!(output.as_ref(), b"keep");
    assert_eq!(output.position(), 1);
}

#[test]
fn obb_output_limits_and_policy_failures_are_atomic_for_overwrites() {
    let value = tree();
    for limit in 4..250 {
        let mut output = Writer::new(limit);
        output.bytes(b"keep", "prefix").unwrap();
        output.seek_to(SeekFrom::Start(2), "seek").unwrap();
        let error = value
            .write(&mut output, ObbLimits::default(), "tree")
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::LimitExceeded);
        assert_eq!(error.offset, None);
        assert_eq!(output.as_ref(), b"keep");
        assert_eq!(output.position(), 2);
    }
    for limits in [
        ObbLimits { nodes: 3, depth: 3 },
        ObbLimits { nodes: 4, depth: 2 },
        ObbLimits { nodes: 0, depth: 0 },
    ] {
        let mut output = Writer::new(1000);
        output.bytes(b"keep", "prefix").unwrap();
        assert_eq!(
            value.write(&mut output, limits, "tree").unwrap_err().kind,
            ErrorKind::LimitExceeded
        );
        assert_eq!(output.as_ref(), b"keep");
    }
}

#[test]
fn obb_bounds_cover_independently_enumerated_corners_including_negative_space() {
    let value = tree();
    assert_eq!(
        value.as_aabb().unwrap(),
        Aabb {
            min: Vec3::from([27.0, 59.0, 120.0]),
            max: Vec3::from([37.0, 69.0, 136.0])
        }
    );
    let value = &value.children[0];
    assert_eq!(
        value.as_aabb().unwrap(),
        Aabb {
            min: Vec3::from([-11.0, -12.0, -13.0]),
            max: Vec3::from([-9.0, -8.0, -7.0])
        }
    );
    assert_eq!(Obb::default().as_aabb().unwrap(), Aabb::ZERO);
    // Independent arithmetic path in f64 for exact integer corners.
    for value in [tree(), tree().children.remove(0)] {
        let bounds = value.as_aabb().unwrap();
        for signs in 0..8 {
            for coordinate in 0..3 {
                let corner = f64::from(value.center[coordinate])
                    + (0..3)
                        .map(|axis| {
                            f64::from(value.axes[axis][coordinate])
                                * f64::from(value.half_width[axis])
                                * if signs & (1 << axis) == 0 { -1.0 } else { 1.0 }
                        })
                        .sum::<f64>();
                assert!(
                    f64::from(bounds.min[coordinate]) <= corner
                        && corner <= f64::from(bounds.max[coordinate])
                );
            }
        }
    }
}

#[test]
fn geometric_validation_does_not_restrict_lossless_serialization() {
    for value in [
        Obb {
            center: Vec3::splat(f32::NAN),
            ..Obb::default()
        },
        Obb {
            axes: [Vec3::splat(f32::INFINITY); 3],
            ..Obb::default()
        },
        Obb {
            half_width: Vec3::splat(-1.0),
            ..Obb::default()
        },
        Obb {
            axes: [Vec3::splat(f32::MAX); 3],
            half_width: Vec3::splat(2.0),
            ..Obb::default()
        },
    ] {
        assert_eq!(value.as_aabb().unwrap_err().kind, ErrorKind::InvalidData);
        assert_eq!(value.as_aabb().unwrap_err().offset, None);
        let bytes = encoded(&value);
        assert_eq!(
            encoded(&Obb::read(&mut Reader::new(&bytes), ObbLimits::default(), "tree").unwrap()),
            bytes
        );
    }
}

#[test]
fn obb_debug_is_compact_and_does_not_recurse_into_children() {
    let value = Obb {
        children: vec![Obb::default(); 1000],
        ..Obb::default()
    };
    let debug = format!("{value:?}");
    assert!(debug.contains("children: 1000"));
    assert!(debug.len() < 300);
    assert_eq!(debug.matches("Obb").count(), 1);
}
