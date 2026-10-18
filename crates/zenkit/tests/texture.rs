use zenkit::{
    ErrorKind,
    io::Reader,
    texture::{Builder, Format, Limits, Metadata, Palette, Texture},
    values::Color,
};
fn palette() -> Box<Palette> {
    Box::new(std::array::from_fn(|i| {
        Color::from_rgba([i as u8, 255 - i as u8, (i * 17) as u8, (i * 29) as u8])
    }))
}
fn raw(f: Format, w: u32, h: u32, bytes: Vec<u8>) -> Texture {
    Texture::from_parts(
        f,
        Metadata::new(w, h),
        if f == Format::P8 {
            Some(palette())
        } else {
            None
        },
        vec![bytes],
        Limits::default(),
    )
    .unwrap()
}
fn wire(format: u32, w: u32, h: u32, count: u32, payload: &[u8]) -> Vec<u8> {
    let mut v = b"ZTEX".to_vec();
    for x in [0, format, w, h, count, 123, 456, 0x11223344] {
        v.extend(x.to_le_bytes());
    }
    v.extend(payload);
    v
}
#[test]
fn every_raw_format_and_mip_order_round_trip() {
    for code in 0..15 {
        let f = Format::try_from(code).unwrap();
        let mips = (0..5)
            .map(|i| vec![i as u8; f.byte_len((8 >> i).max(1), (4 >> i).max(1)).unwrap()])
            .collect();
        let t = Texture::from_parts(
            f,
            Metadata::new(8, 4),
            if f == Format::P8 {
                Some(palette())
            } else {
                None
            },
            mips,
            Limits::default(),
        )
        .unwrap();
        let bytes = t.to_bytes(Limits::default()).unwrap();
        let start = 36 + if f == Format::P8 { 1024 } else { 0 };
        assert_eq!(bytes[start], 4);
        assert_eq!(Texture::from_bytes(&bytes, Limits::default()).unwrap(), t);
        assert_eq!(t.mips().next_back().unwrap().index(), 4);
        assert_eq!(t.mips().len(), 5);
    }
}
#[test]
fn count_zero_is_retained_and_tail_dimensions_clamp() {
    let bytes = wire(1, 1, 1, 0, &[1, 2, 3, 4]);
    let t = Texture::from_bytes(&bytes, Limits::default()).unwrap();
    assert_eq!(t.mipmap_count(), 1);
    assert_eq!(t.declared_mipmap_count(), 0);
    assert_eq!(t.to_bytes(Limits::default()).unwrap(), bytes);
    assert_eq!(
        t.converted(Format::B8G8R8A8, None, Limits::default())
            .unwrap()
            .declared_mipmap_count(),
        0
    );
    let t = Texture::from_parts(
        Format::R8G8B8A8,
        Metadata::new(1, 1),
        None,
        vec![vec![0; 4]; 40],
        Limits {
            mipmaps: 40,
            ..Limits::default()
        },
    )
    .unwrap();
    assert_eq!(
        (t.mip(39).unwrap().width(), t.mip(39).unwrap().height()),
        (1, 1)
    );
    assert!(t.mip(40).is_none());
}
#[test]
fn malformed_headers_and_complete_input_policy() {
    for (offset, value, kind) in [
        (4, 1u32, ErrorKind::Unsupported),
        (8, 15, ErrorKind::Unsupported),
        (12, 0, ErrorKind::InvalidData),
        (16, 0, ErrorKind::InvalidData),
        (20, 100, ErrorKind::LimitExceeded),
    ] {
        let mut b = wire(1, 1, 1, 1, &[1, 2, 3, 4]);
        b[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        let e = Texture::from_bytes(&b, Limits::default()).unwrap_err();
        assert_eq!(e.kind, kind);
        assert_eq!(e.offset, Some(offset));
    }
    let mut b = wire(1, 1, 1, 1, &[0; 4]);
    b.push(42);
    assert_eq!(
        Texture::from_bytes(&b, Limits::default())
            .unwrap_err()
            .offset,
        Some(40)
    );
    let mut r = Reader::new(&b);
    Texture::read(&mut r, Limits::default()).unwrap();
    assert_eq!(r.position(), 40);
}
#[test]
fn every_truncation_rolls_back_embedded_reader() {
    let p = palette();
    let mut payload = Vec::new();
    for c in p.iter() {
        payload.extend(c.to_bgra());
    }
    payload.extend([7, 8, 9, 10]);
    let b = wire(9, 2, 2, 1, &payload);
    for end in 0..b.len() {
        let mut embedded = vec![9; 7];
        embedded.extend(&b[..end]);
        let mut outer = Reader::new(&embedded);
        outer.take_bytes(7, "prefix").unwrap();
        assert!(Texture::read(&mut outer, Limits::default()).is_err());
        assert_eq!(outer.position(), 7);
    }
}
#[test]
fn limits_and_invalid_parts_are_checked() {
    assert!(Format::R8G8B8A8.byte_len(u32::MAX, u32::MAX).is_err());
    assert_eq!(Format::Dxt1.byte_len(5, 7).unwrap(), 32);
    assert_eq!(Format::Dxt5.byte_len(1, 1).unwrap(), 16);
    assert!(Format::P8.byte_len(0, 1).is_err());
    assert!(
        Texture::from_parts(
            Format::P8,
            Metadata::new(1, 1),
            None,
            vec![vec![0]],
            Limits::default()
        )
        .is_err()
    );
    assert!(
        Texture::from_parts(
            Format::R8G8B8A8,
            Metadata::new(1, 1),
            Some(palette()),
            vec![vec![0; 4]],
            Limits::default()
        )
        .is_err()
    );
    assert!(
        Texture::from_parts(
            Format::R8G8B8A8,
            Metadata::new(1, 1),
            None,
            vec![],
            Limits::default()
        )
        .is_err()
    );
    let t = raw(Format::R8G8B8A8, 1, 1, vec![0; 4]);
    assert!(
        t.to_bytes(Limits {
            bytes: 39,
            ..Limits::default()
        })
        .is_err()
    );
    assert!(
        t.mip(0)
            .unwrap()
            .rgba8(Limits {
                rgba_bytes: 3,
                ..Limits::default()
            })
            .is_err()
    );
    assert!(
        t.converted(
            Format::R8G8B8,
            None,
            Limits {
                conversion_bytes: 42,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(
        Texture::from_bytes(
            &t.to_bytes(Limits::default()).unwrap(),
            Limits {
                dimension: 0,
                ..Limits::default()
            }
        )
        .is_err()
    );
}
#[test]
fn byte_channels_decode_and_encode_exactly() {
    let rgba = [11, 22, 33, 44, 55, 66, 77, 88];
    let source = raw(Format::R8G8B8A8, 2, 1, rgba.to_vec());
    for (code, expected) in [
        (0, vec![33, 22, 11, 44, 77, 66, 55, 88]),
        (1, rgba.to_vec()),
        (2, vec![44, 33, 22, 11, 88, 77, 66, 55]),
        (3, vec![44, 11, 22, 33, 88, 55, 66, 77]),
        (4, vec![33, 22, 11, 77, 66, 55]),
        (5, vec![11, 22, 33, 55, 66, 77]),
    ] {
        let f = Format::try_from(code).unwrap();
        let t = source.converted(f, None, Limits::default()).unwrap();
        assert_eq!(t.mip(0).unwrap().bytes(), expected);
        let mut wanted = rgba;
        if code >= 4 {
            wanted[3] = 255;
            wanted[7] = 255;
        }
        assert_eq!(t.mip(0).unwrap().rgba8(Limits::default()).unwrap(), wanted);
    }
}
#[test]
fn packed_channels_endpoints_and_alpha() {
    assert_eq!(
        raw(Format::R5G6B5, 1, 1, vec![255, 255])
            .mip(0)
            .unwrap()
            .rgba8(Limits::default())
            .unwrap(),
        [255; 4]
    );
    assert_eq!(
        raw(Format::A4R4G4B4, 1, 1, vec![0x23, 0x41])
            .mip(0)
            .unwrap()
            .rgba8(Limits::default())
            .unwrap(),
        [17, 34, 51, 68]
    );
    assert_eq!(
        raw(Format::A1R5G5B5, 1, 1, vec![0, 0xfc])
            .mip(0)
            .unwrap()
            .rgba8(Limits::default())
            .unwrap(),
        [255, 0, 0, 255]
    );
    let source = raw(Format::R8G8B8A8, 1, 1, vec![255, 0, 0, 128]);
    assert_eq!(
        source
            .converted(Format::A1R5G5B5, None, Limits::default())
            .unwrap()
            .mip(0)
            .unwrap()
            .bytes(),
        [0, 0xfc]
    );
}
#[test]
fn palette_alpha_and_explicit_remapping() {
    let t = raw(Format::P8, 2, 1, vec![2, 4]);
    let p = t.palette().unwrap();
    let expected = [p[2].to_rgba(), p[4].to_rgba()].concat();
    assert_eq!(
        t.mip(0).unwrap().rgba8(Limits::default()).unwrap(),
        expected
    );
    assert_eq!(
        t.converted(Format::P8, Some(p), Limits::default()).unwrap(),
        t
    );
    let mut other = *p;
    other.swap(2, 4);
    assert_eq!(
        t.converted(Format::P8, Some(&other), Limits::default())
            .unwrap()
            .mip(0)
            .unwrap()
            .bytes(),
        [4, 2]
    );
    assert!(t.converted(Format::P8, None, Limits::default()).is_err());
    assert!(
        t.converted(Format::R8G8B8A8, Some(p), Limits::default())
            .is_err()
    );
}
fn bc_color(a: u16, b: u16, indices: u32) -> Vec<u8> {
    [
        a.to_le_bytes().as_slice(),
        b.to_le_bytes().as_slice(),
        indices.to_le_bytes().as_slice(),
    ]
    .concat()
}
#[test]
fn bc1_modes_and_partial_blocks() {
    let block = bc_color(0xf800, 0x001f, 0xe4e4e4e4);
    let expected = [
        [255, 0, 0, 255],
        [0, 0, 255, 255],
        [170, 0, 85, 255],
        [85, 0, 170, 255],
    ]
    .concat();
    assert_eq!(
        &raw(Format::Dxt1, 4, 4, block.clone())
            .mip(0)
            .unwrap()
            .rgba8(Limits::default())
            .unwrap()[..16],
        expected
    );
    let transparent = bc_color(0, 0xffff, u32::MAX);
    assert_eq!(
        raw(Format::Dxt1, 1, 1, transparent)
            .mip(0)
            .unwrap()
            .rgba8(Limits::default())
            .unwrap(),
        [0; 4]
    );
    let tex = raw(Format::Dxt1, 5, 7, block.repeat(4));
    assert_eq!(
        tex.mip(0).unwrap().rgba8(Limits::default()).unwrap().len(),
        5 * 7 * 4
    );
}
#[test]
fn bc2_explicit_alpha_and_bc3_both_modes() {
    let mut b = vec![0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe];
    b.extend(bc_color(0xf800, 0, 0));
    let out = raw(Format::Dxt3, 4, 4, b)
        .mip(0)
        .unwrap()
        .rgba8(Limits::default())
        .unwrap();
    for (i, p) in out.as_chunks::<4>().0.iter().enumerate() {
        assert_eq!(*p, [255, 0, 0, i as u8 * 17]);
    }
    for (a, b, expected) in [
        (210, 0, [210, 0, 180, 150, 120, 90, 60, 30]),
        (10, 110, [10, 110, 30, 50, 70, 90, 0, 255]),
    ] {
        let mut bits = 0u64;
        for i in 0..16 {
            bits |= ((i % 8) as u64) << (3 * i);
        }
        let mut block = vec![a, b];
        block.extend(&bits.to_le_bytes()[..6]);
        block.extend(bc_color(0xf800, 0, 0));
        let out = raw(Format::Dxt5, 4, 4, block)
            .mip(0)
            .unwrap()
            .rgba8(Limits::default())
            .unwrap();
        for (i, p) in out.as_chunks::<4>().0.iter().enumerate() {
            assert_eq!(p[3], expected[i % 8]);
        }
    }
}
#[test]
fn premultiplied_decode_defines_zero_and_rounding() {
    let mut block = vec![0x80; 8];
    block.extend(bc_color(0x8000, 0, 0));
    let out = raw(Format::Dxt2, 4, 4, block)
        .mip(0)
        .unwrap()
        .rgba8(Limits::default())
        .unwrap();
    assert_eq!(&out[..4], &[0, 0, 0, 0]);
    assert_eq!(&out[4..8], &[248, 0, 0, 136]);
}
#[test]
fn builder_is_atomic_and_preserves_order() {
    let mut b = Builder::new(2, 2, Limits::default()).unwrap();
    assert!(b.push(Format::R8G8B8A8, &[0; 3], None).is_err());
    assert_eq!(b.mipmap_count(), 0);
    b.push(Format::R8G8B8A8, &[1; 16], None).unwrap();
    b.push(Format::R8G8B8A8, &[2; 4], None).unwrap();
    let t = b.build_converted(Format::R8G8B8A8, None).unwrap();
    assert_eq!(t.mip(0).unwrap().bytes(), [1; 16]);
    assert_eq!(t.mip(1).unwrap().bytes(), [2; 4]);
    assert_eq!(t.metadata().average_color, 0);
    let mut limited = Builder::new(
        1,
        1,
        Limits {
            bytes: 39,
            ..Limits::default()
        },
    )
    .unwrap();
    assert!(limited.push(Format::R8G8B8A8, &[0; 4], None).is_err());
    assert_eq!(limited.mipmap_count(), 0);
}
#[test]
fn conversion_aggregate_budget_not_just_one_mip() {
    let t = Texture::from_parts(
        Format::R8G8B8,
        Metadata::new(1, 1),
        None,
        vec![vec![0; 3]; 2],
        Limits::default(),
    )
    .unwrap();
    assert!(
        t.converted(
            Format::R8G8B8A8,
            None,
            Limits {
                rgba_bytes: 7,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(
        t.converted(
            Format::R8G8B8A8,
            None,
            Limits {
                conversion_bytes: 47,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(
        t.converted(
            Format::R8G8B8A8,
            None,
            Limits {
                conversion_bytes: 48,
                ..Limits::default()
            }
        )
        .is_ok()
    );
}
#[test]
fn compression_quality_control_colors_and_gradients() {
    let cases = [
        [[255, 0, 0, 255]; 16],
        std::array::from_fn(|i| {
            if i % 2 == 0 {
                [255, 0, 0, 255]
            } else {
                [0, 255, 0, 255]
            }
        }),
        std::array::from_fn(|i| [i as u8 * 17, i as u8 * 17, i as u8 * 17, 255]),
        std::array::from_fn(|i| [i as u8 * 17, 255 - i as u8 * 17, 64, 255]),
        std::array::from_fn(|i| {
            if i % 4 == 0 {
                [255, 255, 255, 0]
            } else {
                [255, 0, 0, 255]
            }
        }),
    ];
    for (case_index, pixels) in cases.iter().enumerate() {
        let t = raw(Format::R8G8B8A8, 4, 4, pixels.as_flattened().to_vec());
        for f in [
            Format::Dxt1,
            Format::Dxt2,
            Format::Dxt3,
            Format::Dxt4,
            Format::Dxt5,
        ] {
            let encoded = t.converted(f, None, Limits::default()).unwrap();
            let decoded = encoded.mip(0).unwrap().rgba8(Limits::default()).unwrap();
            let mut error = 0u64;
            let mut n = 0;
            for (a, b) in pixels.iter().zip(decoded.as_chunks::<4>().0.iter()) {
                assert_eq!(a[3], b[3]);
                if a[3] != 0 {
                    for c in 0..3 {
                        let d = i32::from(a[c]) - i32::from(b[c]);
                        error += (d * d) as u64;
                        n += 1;
                    }
                }
            }
            let maximum = if matches!(case_index, 0 | 1 | 4) {
                4
            } else {
                700
            };
            assert!(error <= maximum * n, "case{case_index} {f:?}: {error}/{n}");
            assert_eq!(encoded, t.converted(f, None, Limits::default()).unwrap());
        }
    }
}
#[test]
fn bc3_interior_alpha_mode_and_low_alpha_premultiplication() {
    let alphas = [
        0, 255, 100, 110, 120, 130, 100, 110, 120, 130, 0, 255, 100, 110, 120, 130,
    ];
    let pixels = alphas.map(|a| [255, 255, 255, a]);
    let source = raw(Format::R8G8B8A8, 4, 4, pixels.as_flattened().to_vec());
    let t = source
        .converted(Format::Dxt5, None, Limits::default())
        .unwrap();
    assert!(t.mip(0).unwrap().bytes()[0] <= t.mip(0).unwrap().bytes()[1]);
    let decoded = t.mip(0).unwrap().rgba8(Limits::default()).unwrap();
    assert!(
        alphas
            .iter()
            .zip(decoded.as_chunks::<4>().0.iter())
            .map(|(a, p)| i32::from(*a).abs_diff(i32::from(p[3])))
            .sum::<u32>()
            <= 20
    );
    let low = raw(Format::R8G8B8A8, 4, 4, [255, 255, 255, 9].repeat(16))
        .converted(Format::Dxt2, None, Limits::default())
        .unwrap()
        .mip(0)
        .unwrap()
        .rgba8(Limits::default())
        .unwrap();
    assert_eq!(low[3], 17);
    assert!(low[0] >= 230 && low[1] >= 230 && low[2] >= 230);
}
#[test]
fn dds_masks_palette_and_payload_are_independent() {
    for (code, masks) in [
        (0, [0xff0000, 0xff00, 0xff, 0xff000000]),
        (1, [0xff, 0xff00, 0xff0000, 0xff000000]),
        (2, [0xff000000, 0xff0000, 0xff00, 0xff]),
        (3, [0xff00, 0xff0000, 0xff000000, 0xff]),
        (4, [0xff0000, 0xff00, 0xff, 0]),
        (5, [0xff, 0xff00, 0xff0000, 0]),
    ] {
        let f = Format::try_from(code).unwrap();
        let data = vec![7; f.byte_len(1, 1).unwrap()];
        let t = raw(f, 1, 1, data.clone());
        let dds = t.to_dds(Limits::default()).unwrap();
        assert_eq!(&dds[..4], b"DDS ");
        for (i, m) in masks.iter().enumerate() {
            assert_eq!(
                u32::from_le_bytes(dds[92 + i * 4..96 + i * 4].try_into().unwrap()),
                *m
            );
        }
        assert_eq!(&dds[128..], data);
    }
    let t = raw(Format::P8, 1, 1, vec![1]);
    let d = t.to_dds(Limits::default()).unwrap();
    assert_eq!(u32::from_le_bytes(d[88..92].try_into().unwrap()), 8);
    assert_eq!(&d[132..136], t.palette().unwrap()[1].to_rgba());
    assert_eq!(d[1152], 1);
    assert!(
        t.to_dds(Limits {
            bytes: 1152,
            ..Limits::default()
        })
        .is_err()
    );
}
#[test]
fn compact_debug_and_mutation_contract() {
    let mut t = raw(Format::R8G8B8A8, 64, 64, vec![255; 64 * 64 * 4]);
    assert!(format!("{t:?}").len() < 300);
    assert!(format!("{:?}", t.mip(0).unwrap()).len() < 100);
    assert!(t.replace_mip(0, vec![1]).is_err());
    assert_eq!(t.mip(0).unwrap().bytes()[0], 255);
    assert_eq!(
        t.replace_mip(1, vec![]).unwrap_err().kind,
        ErrorKind::NotFound
    );
    t.set_reference_size(9, 10);
    t.set_average_color(11);
    let converted = t
        .converted(Format::R8G8B8, None, Limits::default())
        .unwrap();
    assert_eq!(converted.metadata(), t.metadata());
}

#[test]
fn transparent_hidden_rgb_does_not_ruin_visible_compression() {
    let pixels = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 0, 0],
        [255, 255, 255, 0],
    ]
    .repeat(4);
    let source = raw(Format::R8G8B8A8, 4, 4, pixels.as_flattened().to_vec());
    for format in [Format::Dxt3, Format::Dxt5] {
        let converted = source.converted(format, None, Limits::default()).unwrap();
        let decoded = converted.mip(0).unwrap().rgba8(Limits::default()).unwrap();
        for (a, b) in pixels.iter().zip(decoded.as_chunks::<4>().0.iter()) {
            if a[3] != 0 {
                assert_eq!(a, b);
            }
        }
        let hidden = raw(Format::R8G8B8A8, 4, 4, [255, 255, 255, 0].repeat(16))
            .converted(format, None, Limits::default())
            .unwrap();
        assert_eq!(
            hidden.mip(0).unwrap().rgba8(Limits::default()).unwrap(),
            vec![0; 64]
        );
    }
}

#[test]
fn low_alpha_pixels_do_not_dominate_opaque_bc3_colors() {
    let pixels = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 0, 4],
        [255, 255, 255, 4],
    ]
    .repeat(4);
    let source = raw(Format::R8G8B8A8, 4, 4, pixels.as_flattened().to_vec());
    for format in [Format::Dxt3, Format::Dxt5] {
        let converted = source.converted(format, None, Limits::default()).unwrap();
        let decoded = converted.mip(0).unwrap().rgba8(Limits::default()).unwrap();
        for (a, b) in pixels.iter().zip(decoded.as_chunks::<4>().0.iter()) {
            if a[3] == 255 {
                assert_eq!(a, b);
            }
        }
    }
}

#[test]
fn translucent_gradient_retains_alpha_and_meets_visible_color_error_budget() {
    // Authored independently of the encoder: a color ramp with five opacity
    // bands. A choice limited to pixel/range endpoints exceeds this budget.
    let pixels: [[u8; 4]; 16] = std::array::from_fn(|i| {
        [
            i as u8 * 17,
            255 - i as u8 * 17,
            i as u8 * 11,
            17 * (1 + i as u8 % 5),
        ]
    });
    let source = raw(Format::R8G8B8A8, 4, 4, pixels.as_flattened().to_vec());
    for format in [Format::Dxt3, Format::Dxt5] {
        let converted = source.converted(format, None, Limits::default()).unwrap();
        let decoded = converted.mip(0).unwrap().rgba8(Limits::default()).unwrap();
        let mut weighted_error = 0u64;
        let mut total_weight = 0u64;
        for (original, reconstructed) in pixels.iter().zip(decoded.as_chunks::<4>().0) {
            if format == Format::Dxt3 {
                assert_eq!(original[3], reconstructed[3]);
            } else {
                assert!(original[3].abs_diff(reconstructed[3]) <= 7);
            }
            let weight = u64::from(original[3]).pow(2);
            total_weight += 3 * weight;
            for c in 0..3 {
                let delta = i32::from(original[c]) - i32::from(reconstructed[c]);
                weighted_error += (delta * delta) as u64 * weight;
            }
        }
        assert!(
            weighted_error <= 230 * total_weight,
            "{format:?}: {weighted_error}/{total_weight}"
        );
    }
}

#[test]
fn unaligned_solid_color_remains_bounded_and_deterministic() {
    // A non-RGB565 solid keeps nonzero quantization error but supplies only one
    // palette index, so endpoint fitting must handle a singular assignment.
    let pixels = [13, 47, 91, 170].repeat(16);
    let source = raw(Format::R8G8B8A8, 4, 4, pixels.clone());
    for format in [
        Format::Dxt1,
        Format::Dxt2,
        Format::Dxt3,
        Format::Dxt4,
        Format::Dxt5,
    ] {
        let converted = source.converted(format, None, Limits::default()).unwrap();
        assert_eq!(
            converted,
            source.converted(format, None, Limits::default()).unwrap()
        );
        let decoded = converted.mip(0).unwrap().rgba8(Limits::default()).unwrap();
        for (original, reconstructed) in pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(decoded.as_chunks::<4>().0)
        {
            assert_eq!(
                reconstructed[3],
                if format == Format::Dxt1 { 255 } else { 170 }
            );
            for c in 0..3 {
                assert!(original[c].abs_diff(reconstructed[c]) <= 8, "{format:?}");
            }
        }
    }
}

#[test]
fn compression_recovers_interior_colors_without_observed_endpoints() {
    // RGB565 endpoints [255,130,66] and [0,65,198] yield these two
    // four-color interpolants exactly. Neither endpoint is in the input.
    let pixels = [[170, 108, 110, 170], [85, 86, 154, 170]].repeat(8);
    let source = raw(Format::R8G8B8A8, 4, 4, pixels.as_flattened().to_vec());
    for format in [Format::Dxt1, Format::Dxt3, Format::Dxt5] {
        let decoded = source
            .converted(format, None, Limits::default())
            .unwrap()
            .mip(0)
            .unwrap()
            .rgba8(Limits::default())
            .unwrap();
        for (original, reconstructed) in pixels.iter().zip(decoded.as_chunks::<4>().0) {
            assert_eq!(&original[..3], &reconstructed[..3], "{format:?}");
            assert_eq!(
                reconstructed[3],
                if format == Format::Dxt1 { 255 } else { 170 }
            );
        }
    }
    // With BC1 transparency, one endpoint and the half-way color suffice.
    // Hidden RGB does not supply a fitting endpoint.
    let cutout = [
        [255, 130, 66, 255],
        [127, 97, 132, 255],
        [0, 255, 0, 0],
        [255, 0, 255, 0],
    ]
    .repeat(4);
    let source = raw(Format::R8G8B8A8, 4, 4, cutout.as_flattened().to_vec());
    let decoded = source
        .converted(Format::Dxt1, None, Limits::default())
        .unwrap()
        .mip(0)
        .unwrap()
        .rgba8(Limits::default())
        .unwrap();
    for (original, reconstructed) in cutout.iter().zip(decoded.as_chunks::<4>().0) {
        if original[3] != 0 {
            assert_eq!(original, reconstructed);
        } else {
            assert_eq!(*reconstructed, [0; 4]);
        }
    }
}

#[test]
fn impossible_large_mip_count_fails_before_long_iteration() {
    let bytes = wire(1, 1, 1, u32::MAX, &[]);
    let error = Texture::from_bytes(
        &bytes,
        Limits {
            mipmaps: usize::MAX,
            ..Limits::default()
        },
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::UnexpectedEof);
    assert_eq!(error.offset, Some(36));
}

#[test]
fn dds_rejects_extra_tail_levels_without_changing_tex() {
    let texture = Texture::from_parts(
        Format::R8G8B8A8,
        Metadata::new(1, 1),
        None,
        vec![vec![0; 4]; 2],
        Limits::default(),
    )
    .unwrap();
    assert_eq!(
        texture.to_dds(Limits::default()).unwrap_err().kind,
        ErrorKind::Unsupported
    );
    let bytes = texture.to_bytes(Limits::default()).unwrap();
    assert_eq!(
        Texture::from_bytes(&bytes, Limits::default())
            .unwrap()
            .mipmap_count(),
        2
    );
    assert_eq!(
        texture
            .converted(Format::R8G8B8, None, Limits::default())
            .unwrap()
            .mipmap_count(),
        2
    );
}
