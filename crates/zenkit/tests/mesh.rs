use zenkit::{
    ErrorKind,
    GameVersion::{self, Gothic1, Gothic2},
    io::Reader,
    mesh::*,
    values::*,
};
const MSH1: &[u8] = include_bytes!("../tests/fixtures/synthetic-mesh-msh-g1.msh");
const MSH2: &[u8] = include_bytes!("../tests/fixtures/synthetic-mesh-msh-g2.msh");
const MRM1: &[u8] = include_bytes!("../tests/fixtures/synthetic-mesh-mrm-g1.mrm");
const MRM2: &[u8] = include_bytes!("../tests/fixtures/synthetic-mesh-mrm-g2.mrm");
const SKIN1: &[u8] = include_bytes!("../tests/fixtures/synthetic-mesh-skin-g1.skin");
const SKIN2: &[u8] = include_bytes!("../tests/fixtures/synthetic-mesh-skin-g2.skin");
const MDH1: &[u8] = include_bytes!("../tests/fixtures/synthetic-mesh-mdh-g1.mdh");
const MDM1: &[u8] = include_bytes!("../tests/fixtures/synthetic-mesh-mdm-g1.mdm");
const MDM2: &[u8] = include_bytes!("../tests/fixtures/synthetic-mesh-mdm-g2.mdm");
const MDL1: &[u8] = include_bytes!("../tests/fixtures/synthetic-mesh-mdl-g1.mdl");
const MDL2: &[u8] = include_bytes!("../tests/fixtures/synthetic-mesh-mdl-g2.mdl");
fn mrm() -> MultiResolutionMesh {
    MultiResolutionMesh::from_bytes(MRM1, Gothic1, Limits::default()).unwrap()
}
fn msh() -> Mesh {
    Mesh::from_bytes(MSH1, Gothic1, false, Limits::default()).unwrap()
}
fn append_chunk_tail(bytes: &[u8], wanted: u16, tail: &[u8]) -> Vec<u8> {
    let mut raw = bytes.to_vec();
    let mut offset = 0;
    loop {
        let tag = u16::from_le_bytes(raw[offset..offset + 2].try_into().unwrap());
        let size = u32::from_le_bytes(raw[offset + 2..offset + 6].try_into().unwrap());
        let end = offset + 6 + size as usize;
        if tag == wanted {
            raw[offset + 2..offset + 6].copy_from_slice(&(size + tail.len() as u32).to_le_bytes());
            raw.splice(end..end, tail.iter().copied());
            return raw;
        }
        offset = end;
    }
}

#[test]
fn material_chunk_alpha_and_tail_are_bounded_to_their_chunk_and_preserved() {
    let limits = Limits::default();
    for (source, game) in [(MSH1, Gothic1), (MSH2, Gothic2)] {
        for tail in [b"\x01".as_slice(), b"\x00\xff\r\n\x60\xb0\x00\x00\x00\x00"] {
            let bytes = append_chunk_tail(source, 0xb020, tail);
            let mesh = Mesh::from_bytes(&bytes, game, false, limits).unwrap();
            if game == Gothic2 {
                assert_eq!(mesh.alpha_test, Some(tail[0]));
                assert_eq!(mesh.material_tail, tail[1..]);
            } else {
                assert_eq!(mesh.alpha_test, None);
                assert_eq!(mesh.material_tail, tail);
            }
            assert_eq!(mesh.vertices.len(), 4);
            assert_eq!(mesh.polygons.len(), 2);
            assert_eq!(mesh.to_bytes(limits).unwrap(), bytes);
            for n in 0..bytes.len() {
                let mut reader = Reader::new(&bytes[..n]);
                assert!(
                    Mesh::read(&mut reader, game, false, limits).is_err(),
                    "prefix {n}"
                );
                assert_eq!(reader.position(), 0);
            }
        }
        let bytes = append_chunk_tail(source, 0xb060, &[1]);
        assert_eq!(
            Mesh::from_bytes(&bytes, game, false, limits)
                .unwrap_err()
                .kind,
            ErrorKind::Unsupported
        );
    }
}

#[test]
fn g2_alpha_test_decodes_raw_values_and_edits_without_changing_other_bytes() {
    let limits = Limits::default();
    for value in [0, 1, 7, 255] {
        for suffix in [b"".as_slice(), b"\xff\x00\x60\xb0\x00\x00\x00\x00"] {
            let mut tail = vec![value];
            tail.extend_from_slice(suffix);
            let bytes = append_chunk_tail(MSH2, 0xb020, &tail);
            let mut mesh = Mesh::from_bytes(&bytes, Gothic2, false, limits).unwrap();
            assert_eq!(mesh.alpha_test, Some(value));
            assert_eq!(mesh.alpha_test_enabled(), value != 0);
            assert_eq!(mesh.material_tail, suffix);
            assert_eq!(mesh.to_bytes(limits).unwrap(), bytes);

            let edited = u8::from(value == 0);
            mesh.alpha_test = Some(edited);
            tail[0] = edited;
            let expected = append_chunk_tail(MSH2, 0xb020, &tail);
            assert_eq!(mesh.to_bytes(limits).unwrap(), expected);
            let loaded = Mesh::from_bytes(&expected, Gothic2, false, limits).unwrap();
            assert_eq!(loaded, mesh);
            assert_eq!(loaded.alpha_test_enabled(), edited != 0);
        }
    }
}

#[test]
fn missing_g2_alpha_test_retains_its_encoding_and_can_be_set_explicitly() {
    let limits = Limits::default();
    let mut mesh = Mesh::from_bytes(MSH2, Gothic2, false, limits).unwrap();
    assert_eq!(mesh.alpha_test, None);
    assert!(mesh.alpha_test_enabled());
    assert!(mesh.material_tail.is_empty());
    assert_eq!(mesh.to_bytes(limits).unwrap(), MSH2);

    mesh.alpha_test = Some(0);
    assert!(!mesh.alpha_test_enabled());
    assert_eq!(
        mesh.to_bytes(limits).unwrap(),
        append_chunk_tail(MSH2, 0xb020, &[0])
    );
}

#[test]
fn new_g2_meshes_write_the_enabled_alpha_test_byte() {
    let limits = Limits::default();
    for game in [Gothic1, Gothic2] {
        let mesh = Mesh::new(game);
        assert_eq!(mesh.alpha_test, (game == Gothic2).then_some(1));
        assert!(mesh.alpha_test_enabled());
        let bytes = mesh.to_bytes(limits).unwrap();
        let loaded = Mesh::from_bytes(&bytes, game, false, limits).unwrap();
        assert_eq!(loaded, mesh);
        if game == Gothic2 {
            let mut without_alpha = mesh.clone();
            without_alpha.alpha_test = None;
            assert_eq!(
                bytes,
                append_chunk_tail(&without_alpha.to_bytes(limits).unwrap(), 0xb020, &[1])
            );
        }
    }
}

#[test]
fn invalid_alpha_test_presence_is_rejected_before_writing() {
    let limits = Limits::default();
    let mut mesh = Mesh::new(Gothic1);
    mesh.alpha_test = Some(0);
    assert_eq!(
        mesh.validate(limits).unwrap_err().kind,
        ErrorKind::InvalidData
    );
    assert_eq!(
        mesh.to_bytes(limits).unwrap_err().kind,
        ErrorKind::InvalidData
    );

    let mut mesh = Mesh::new(Gothic2);
    mesh.alpha_test = None;
    mesh.material_tail = vec![0];
    assert_eq!(
        mesh.validate(limits).unwrap_err().kind,
        ErrorKind::InvalidData
    );
    assert_eq!(
        mesh.to_bytes(limits).unwrap_err().kind,
        ErrorKind::InvalidData
    );
}

#[test]
fn material_tail_counts_toward_owned_bytes_on_read_and_edit() {
    let mut mesh = msh();
    let mut low = 0;
    let mut high = Limits::default().owned_bytes;
    while low < high {
        let middle = low + (high - low) / 2;
        if mesh
            .validate(Limits {
                owned_bytes: middle,
                ..Limits::default()
            })
            .is_ok()
        {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    let exact = Limits {
        owned_bytes: low,
        ..Limits::default()
    };
    Mesh::from_bytes(MSH1, Gothic1, false, exact).unwrap();
    let tail = [0xff; 4096];
    let bytes = append_chunk_tail(MSH1, 0xb020, &tail);
    let mut reader = Reader::new(&bytes);
    assert_eq!(
        Mesh::read(&mut reader, Gothic1, false, exact)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(reader.position(), 0);
    mesh.material_tail = tail.to_vec();
    assert_eq!(
        mesh.to_bytes(exact).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    let sufficient = Limits {
        owned_bytes: low + tail.len(),
        ..exact
    };
    let loaded = Mesh::from_bytes(&bytes, Gothic1, false, sufficient).unwrap();
    assert_eq!(loaded, mesh);
    assert_eq!(loaded.to_bytes(sufficient).unwrap(), bytes);
    assert!(format!("{loaded:?}").len() < 300);
}
fn hierarchy() -> ModelHierarchy {
    ModelHierarchy::from_bytes(MDH1, Limits::default()).unwrap()
}
fn model() -> Model {
    Model::from_bytes(MDL1, Gothic1, Limits::default()).unwrap()
}
#[test]
fn independent_nonempty_fixtures_rewrite_exactly() {
    let l = Limits::default();
    for (g, msh, mrm, skin, mdm, mdl) in [
        (Gothic1, MSH1, MRM1, SKIN1, MDM1, MDL1),
        (Gothic2, MSH2, MRM2, SKIN2, MDM2, MDL2),
    ] {
        assert_eq!(
            Mesh::from_bytes(msh, g, false, l)
                .unwrap()
                .to_bytes(l)
                .unwrap(),
            msh
        );
        assert_eq!(
            MultiResolutionMesh::from_bytes(mrm, g, l)
                .unwrap()
                .to_bytes(l)
                .unwrap(),
            mrm
        );
        assert_eq!(
            SoftSkinMesh::from_bytes(skin, g, l)
                .unwrap()
                .to_bytes(l)
                .unwrap(),
            skin
        );
        assert_eq!(
            ModelMesh::from_bytes(mdm, g, l)
                .unwrap()
                .to_bytes(l)
                .unwrap(),
            mdm
        );
        assert_eq!(
            Model::from_bytes(mdl, g, l).unwrap().to_bytes(l).unwrap(),
            mdl
        );
    }
    assert_eq!(hierarchy().to_bytes(l).unwrap(), MDH1);
}
#[test]
fn known_geometry_and_relationships_are_exposed() {
    let m = mrm();
    assert_eq!(m.positions[0], Vec3::from([1.25, -2.5, 3.75]));
    assert_eq!(m.submeshes[0].triangles, [[0, 1, 2]]);
    assert_eq!(m.submeshes[0].wedge_map, [65535, 0, 1]);
    assert_eq!(m.submeshes[0].material.prefix, b"external\xfc");
    assert_eq!(m.obb.children.len(), 1);
    let m = model();
    assert_eq!(m.hierarchy.nodes[2].parent, 1);
    assert_eq!(m.mesh.attachments.len(), 2);
    assert_eq!(m.mesh.meshes[0].weights[0].len(), 2);
    assert_eq!(m.mesh.meshes[0].weights[0][1].node_index, 1);
}
#[test]
fn every_mrm_prefix_fails_transactionally() {
    for n in 0..MRM1.len() {
        let mut bytes = vec![0xaa; 7];
        bytes.extend(&MRM1[..n]);
        let mut r = Reader::new(&bytes);
        r.take_bytes(7, "prefix").unwrap();
        assert!(
            MultiResolutionMesh::read(&mut r, Gothic1, Limits::default()).is_err(),
            "prefix{n}"
        );
        assert_eq!(r.position(), 7);
    }
}
#[test]
fn embedded_reads_leave_suffix_and_whole_reads_reject_it() {
    let mut bytes = MSH1.to_vec();
    bytes.extend([0xaa, 0xbb]);
    let mut r = Reader::new(&bytes);
    Mesh::read(&mut r, Gothic1, false, Limits::default()).unwrap();
    assert_eq!(r.take_bytes(2, "suffix").unwrap(), [0xaa, 0xbb]);
    assert_eq!(
        Mesh::from_bytes(&bytes, Gothic1, false, Limits::default())
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
}
#[test]
fn input_byte_cap_precedes_nested_parsing() {
    let mut r = Reader::new(MDL1);
    let e = Model::read(
        &mut r,
        Gothic1,
        Limits {
            bytes: 20,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(e.kind, ErrorKind::LimitExceeded);
    assert_eq!(r.position(), 0);
}
#[test]
fn wrong_versions_and_games_are_rejected() {
    assert_eq!(
        MultiResolutionMesh::from_bytes(MRM1, Gothic2, Limits::default())
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
    let mut h = MDH1.to_vec();
    h[6] = 4;
    assert_eq!(
        ModelHierarchy::from_bytes(&h, Limits::default())
            .unwrap_err()
            .offset,
        Some(6)
    );
    let mut m = mrm();
    m.game = Gothic2;
    assert_eq!(
        m.to_bytes(Limits::default()).unwrap_err().kind,
        ErrorKind::Unsupported
    );
}
#[test]
fn unknown_and_extra_chunk_bytes_are_not_skipped() {
    let mut h = MDH1.to_vec();
    h[0] = 0xee;
    assert_eq!(
        ModelHierarchy::from_bytes(&h, Limits::default())
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
    let mut h = MDH1.to_vec();
    let n = u32::from_le_bytes(h[2..6].try_into().unwrap()) as usize;
    h.insert(6 + n, 0);
    h[2..6].copy_from_slice(&((n + 1) as u32).to_le_bytes());
    assert_eq!(
        ModelHierarchy::from_bytes(&h, Limits::default())
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
}
#[test]
fn zero_chunk_budget_rejects_all_writers() {
    let l = Limits {
        chunks: 0,
        ..Default::default()
    };
    assert_eq!(
        msh().to_bytes(l).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(
        mrm().to_bytes(l).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(
        hierarchy().to_bytes(l).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(
        model().to_bytes(l).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
}
#[test]
fn nested_chunk_budget_counts_external_skin_streams() {
    let m = model(); // hierarchy3 + MDM5/source + attachments2 + skin4 =14
    assert!(
        m.to_bytes(Limits {
            chunks: 14,
            ..Default::default()
        })
        .is_ok()
    );
    assert_eq!(
        m.to_bytes(Limits {
            chunks: 13,
            ..Default::default()
        })
        .unwrap_err()
        .kind,
        ErrorKind::LimitExceeded
    );
}
#[test]
fn opaque_updates_require_explicit_discard_before_geometry_edits() {
    let raw = include_bytes!("../tests/fixtures/synthetic-mesh-mrm-opaque-g1.mrm");
    let l = Limits::default();
    let mut m = MultiResolutionMesh::from_bytes(raw, Gothic1, l).unwrap();
    assert_eq!(m.to_bytes(l).unwrap(), raw);
    let original = m.original_content().unwrap().to_vec();
    assert!(m.vertex_updates().any(|(_, r)| r.count != 0));
    m.positions[0].x += 0.5;
    assert_eq!(m.to_bytes(l).unwrap_err().kind, ErrorKind::Unsupported);
    m.discard_vertex_updates();
    let out = m.to_bytes(l).unwrap();
    let loaded = MultiResolutionMesh::from_bytes(&out, Gothic1, l).unwrap();
    assert_eq!(loaded.positions[0], m.positions[0]);
    assert!(loaded.original_content().unwrap().starts_with(&original));
    assert!(loaded.vertex_updates().all(|(_, r)| r.count == 0));
}
#[test]
fn opaque_updates_allow_metadata_and_bounds_changes() {
    let raw = include_bytes!("../tests/fixtures/synthetic-mesh-mrm-opaque-g1.mrm");
    let l = Limits::default();
    let mut m = MultiResolutionMesh::from_bytes(raw, Gothic1, l).unwrap();
    m.bbox.min.x = -9.;
    m.submeshes[0].material.prefix = b"changed".to_vec();
    let loaded = MultiResolutionMesh::from_bytes(&m.to_bytes(l).unwrap(), Gothic1, l).unwrap();
    assert_eq!(loaded.original_content(), m.original_content());
    assert_eq!(loaded.bbox.min.x, -9.);
}
#[test]
fn content_edits_append_without_destroying_uninterpreted_bytes() {
    let mut m = mrm();
    let before = m.original_content().unwrap().to_vec();
    m.submeshes[0].wedges[0].reserved = 0xcafe;
    m.positions[0].x = f32::from_bits(0x7fc01234);
    let raw = m.to_bytes(Limits::default()).unwrap();
    let n = MultiResolutionMesh::from_bytes(&raw, Gothic1, Limits::default()).unwrap();
    assert!(n.original_content().unwrap().starts_with(&before));
    assert_eq!(n.positions[0].x.to_bits(), 0x7fc01234);
    assert_eq!(n.submeshes[0].wedges[0].reserved, 0xcafe);
}
#[test]
fn optional_arrays_and_unproven_indices_are_preserved() {
    let mut m = mrm();
    m.normals.clear();
    m.submeshes[0].colors.clear();
    m.submeshes[0].edges[0] = [65535, 65534];
    let l = Limits::default();
    let raw = m.to_bytes(l).unwrap();
    let n = MultiResolutionMesh::from_bytes(&raw, Gothic1, l).unwrap();
    assert_eq!(n.submeshes[0].edges[0], [65535, 65534]);
    assert!(n.normals.is_empty());
}
#[test]
fn invalid_known_indices_are_errors() {
    let mut m = mrm();
    m.submeshes[0].triangles[0][0] = 3;
    assert_eq!(
        m.validate(Limits::default()).unwrap_err().kind,
        ErrorKind::InvalidData
    );
    let mut m = msh();
    m.polygons[0].corners[0].feature = 4;
    assert_eq!(
        m.validate(Limits::default()).unwrap_err().kind,
        ErrorKind::InvalidData
    );
}
#[test]
fn shared_inline_unused_and_wide_lightmap_variants_round_trip() {
    let l = Limits::default();
    for (bytes, wide) in [
        (
            include_bytes!("../tests/fixtures/synthetic-mesh-msh-inline-g1.msh").as_slice(),
            false,
        ),
        (
            include_bytes!("../tests/fixtures/synthetic-mesh-msh-unused-g1.msh").as_slice(),
            false,
        ),
        (
            include_bytes!("../tests/fixtures/synthetic-mesh-msh-wide-g1.msh").as_slice(),
            true,
        ),
        (
            include_bytes!("../tests/fixtures/synthetic-mesh-msh-flags-g1.msh").as_slice(),
            false,
        ),
    ] {
        let m = Mesh::from_bytes(bytes, Gothic1, wide, l).unwrap();
        assert_eq!(m.to_bytes(l).unwrap(), bytes);
    }
    let m = msh();
    assert_eq!(m.lightmaps.as_ref().unwrap().textures.len(), 1);
    assert_eq!(m.lightmaps.as_ref().unwrap().maps[1].texture, 0);
}
#[test]
fn fan_triangulation_is_bounded_and_preserves_raw_polygons() {
    let mut m = msh();
    let t = m.triangulate_leaves(&[1, 0], 3).unwrap();
    assert_eq!(t.len(), 3);
    assert_eq!(t[1].corners.map(|c| c.vertex), [0, 2, 3]);
    assert_eq!(m.polygons[0].corners.len(), 4);
    assert_eq!(
        m.triangulate_leaves(&[0, 1], 2).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    assert!(m.triangulate_leaves(&[0, 0], 10).is_err());
    assert!(m.triangulate_leaves(&[2], 10).is_err());
    m.polygons[0].flags = PolygonFlags::Gothic1 {
        raw: [0x20, 0],
        sector_index: 0,
    };
    assert_eq!(m.triangulate_leaves(&[0, 1], 10).unwrap().len(), 1);
}
#[test]
fn hierarchy_forward_parents_and_multiple_roots_are_valid() {
    let mut h = hierarchy();
    h.nodes[0].parent = 2;
    h.nodes[1].parent = -1;
    h.nodes[2].parent = -1;
    assert!(h.to_bytes(Limits::default()).is_ok());
    h.nodes[2].parent = 0;
    assert_eq!(
        h.validate(Limits::default()).unwrap_err().kind,
        ErrorKind::InvalidData
    );
}
#[test]
fn composed_model_checks_cross_component_relationships() {
    let mut m = model();
    m.mesh.checksum = m.mesh.checksum.map(|v| v ^ 1);
    assert!(m.validate(Limits::default()).is_err());
    let mut m = model();
    m.mesh.attachments[0].name = b"missing".to_vec();
    assert!(m.validate(Limits::default()).is_err());
    let mut m = model();
    m.mesh.meshes[0].nodes[0] = 123;
    assert!(m.mesh.validate(Limits::default()).is_ok());
    assert!(m.validate(Limits::default()).is_err());
}
#[test]
fn skin_skeleton_indices_and_attachment_duplicates_are_rejected() {
    let mut m = model();
    m.mesh.meshes[0].weights[0][0].node_index = 3;
    assert!(m.validate(Limits::default()).is_err());
    let mut m = model();
    m.mesh.attachments[1].name = m.mesh.attachments[0].name.clone();
    assert!(m.validate(Limits::default()).is_err());
}
#[test]
fn raw_debug_is_compact_and_source_bytes_survive() {
    let mut h = hierarchy();
    h.source.as_mut().unwrap().date.reserved = 0xbeef;
    h.source.as_mut().unwrap().path = vec![0xff; 4096];
    let l = Limits::default();
    let n = ModelHierarchy::from_bytes(&h.to_bytes(l).unwrap(), l).unwrap();
    assert_eq!(h, n);
    assert!(format!("{n:?}").len() < 500);
}
#[test]
fn binary_material_scope_is_explicit() {
    let mut m = msh();
    m.materials.metadata.format = zenkit::archive::Format::Ascii;
    assert_eq!(
        m.to_bytes(Limits::default()).unwrap_err().kind,
        ErrorKind::Unsupported
    );
}
#[test]
fn fresh_empty_models_and_meshes_are_valid_values() {
    for g in [GameVersion::Gothic1, GameVersion::Gothic2] {
        let l = Limits::default();
        let m = MultiResolutionMesh::new(g);
        let raw = m.to_bytes(l).unwrap();
        assert!(MultiResolutionMesh::from_bytes(&raw, g, l).is_ok());
        let m = Mesh::new(g);
        let raw = m.to_bytes(l).unwrap();
        assert!(Mesh::from_bytes(&raw, g, false, l).is_ok());
    }
}

#[test]
fn absent_mdm_sections_and_attachment_end_markers_are_explicit() {
    let l = Limits::default();
    for raw in [
        include_bytes!("../tests/fixtures/synthetic-mesh-mdm-skins-g1.mdm").as_slice(),
        include_bytes!("../tests/fixtures/synthetic-mesh-mdm-attachments-g1.mdm").as_slice(),
    ] {
        let m = ModelMesh::from_bytes(raw, Gothic1, l).unwrap();
        assert_eq!(m.to_bytes(l).unwrap(), raw);
    }
    let mut m = ModelMesh::new(Gothic1);
    m.attachment_names_present = false;
    m.checksum = None;
    let raw = m.to_bytes(l).unwrap();
    let read = ModelMesh::from_bytes(&raw, Gothic1, l).unwrap();
    assert!(!read.attachment_names_present);
    assert_eq!(read.checksum, None);
    assert_eq!(read.to_bytes(l).unwrap(), raw);
    let mut m = model().mesh;
    m.meshes.clear();
    m.checksum = None;
    m.attachments[0].end_marker = true;
    m.attachments[1].end_marker = true;
    let raw = m.to_bytes(l).unwrap();
    let read = ModelMesh::from_bytes(&raw, Gothic1, l).unwrap();
    assert!(read.attachments.iter().all(|a| a.end_marker));
    assert_eq!(read.to_bytes(l).unwrap(), raw);
    m.attachment_names_present = false;
    assert_eq!(m.validate(l).unwrap_err().kind, ErrorKind::InvalidData);
    let mut m = model().mesh;
    m.checksum = None;
    assert_eq!(m.validate(l).unwrap_err().kind, ErrorKind::InvalidData);
}
