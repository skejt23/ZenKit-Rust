use zenkit::{
    ErrorKind,
    GameVersion::{Gothic1, Gothic2},
    io::Reader,
    material::{AlphaFunction, MaterialVersion},
    mesh::{Limits, Model, ModelMesh, ModelMeshVersion, MrmVersion, MultiResolutionMesh},
};

// Independently authored by tools/fixtures/mesh_fixtures.py; no game bytes.
const EARLY_MRM: &[u8] = include_bytes!("fixtures/synthetic-mesh-mrm-early-g2.mrm");
const SPARSE_G1: &[u8] = include_bytes!("fixtures/synthetic-mesh-mdm-sparse-g1.mdm");
const EARLY_MDM: &[u8] = include_bytes!("fixtures/synthetic-mesh-mdm-early-g2.mdm");
const MODERN_MDM: &[u8] = include_bytes!("fixtures/synthetic-mesh-mdm-modern-g2.mdm");
const EARLY_MDL: &[u8] = include_bytes!("fixtures/synthetic-mesh-mdl-early-g2.mdl");
const MODERN_MDL: &[u8] = include_bytes!("fixtures/synthetic-mesh-mdl-modern-g2.mdl");

#[test]
fn early_mrm_preserves_alpha_bounds_materials_and_opaque_content() {
    let l = Limits::default();
    let m = MultiResolutionMesh::from_bytes(EARLY_MRM, Gothic2, l).unwrap();
    assert_eq!(m.version, MrmVersion::Gothic2Early);
    assert_eq!(m.version.raw(), 0x0805);
    assert_eq!(m.alpha_test, Some(7));
    assert_eq!(m.bbox.min.x, -1.0);
    assert_eq!(m.obb.children.len(), 1);
    assert_eq!(m.submeshes[0].material.prefix, b"external\xfc");
    assert_eq!(m.submeshes[0].wedges[2].reserved, 0x1202);
    assert_eq!(m.trailer, std::array::from_fn(|i| i as u8));
    assert_eq!(m.vertex_updates().next().unwrap().1.count, 2);
    assert!(m.original_content().unwrap().ends_with(&[0x19, 0x82, 0x73]));
    assert_eq!(m.to_bytes(l).unwrap(), EARLY_MRM);
    assert_eq!(
        MultiResolutionMesh::from_bytes(&m.to_bytes(l).unwrap(), Gothic2, l).unwrap(),
        m
    );
}

#[test]
fn model_versions_and_sparse_skeleton_ids_survive_independent_fixtures() {
    let l = Limits::default();
    for (bytes, g, version, mrm_version) in [
        (
            SPARSE_G1,
            Gothic1,
            ModelMeshVersion::Legacy,
            MrmVersion::Gothic1,
        ),
        (
            EARLY_MDM,
            Gothic2,
            ModelMeshVersion::Gothic2Early,
            MrmVersion::Gothic2Early,
        ),
        (
            MODERN_MDM,
            Gothic2,
            ModelMeshVersion::Gothic2,
            MrmVersion::Gothic2,
        ),
    ] {
        let m = ModelMesh::from_bytes(bytes, g, l).unwrap();
        assert_eq!(m.version, version);
        assert_eq!(m.meshes[0].mesh.version, mrm_version);
        assert_eq!(m.attachments[1].mesh.version, mrm_version);
        assert_eq!(m.meshes[0].nodes, [17, 32]);
        assert_eq!(m.meshes[0].weights[0][1].node_index, 32);
        assert_eq!(m.to_bytes(l).unwrap(), bytes);
    }
    for (bytes, version) in [
        (EARLY_MDL, ModelMeshVersion::Gothic2Early),
        (MODERN_MDL, ModelMeshVersion::Gothic2),
    ] {
        let m = Model::from_bytes(bytes, Gothic2, l).unwrap();
        assert_eq!(m.mesh.version, version);
        assert_eq!(m.hierarchy.nodes.len(), 33);
        assert_eq!(m.mesh.meshes[0].weights[0][1].node_index, 32);
        assert_eq!(m.to_bytes(l).unwrap(), bytes);
    }
}

#[test]
fn early_and_modern_layouts_have_identical_decoded_fields_after_explicit_conversion() {
    let l = Limits::default();
    let mut early = Model::from_bytes(EARLY_MDL, Gothic2, l).unwrap();
    let modern = Model::from_bytes(MODERN_MDL, Gothic2, l).unwrap();
    early.mesh.version = ModelMeshVersion::Gothic2;
    for a in &mut early.mesh.attachments {
        a.mesh.version = MrmVersion::Gothic2;
        for s in &mut a.mesh.submeshes {
            let MaterialVersion::Gothic2Early(v) = &s.material.material.version else {
                panic!()
            };
            s.material.material.version =
                MaterialVersion::Gothic2(v.clone().with_alpha(AlphaFunction::from_raw(3)));
        }
    }
    for s in &mut early.mesh.meshes {
        s.mesh.version = MrmVersion::Gothic2;
        for sub in &mut s.mesh.submeshes {
            let MaterialVersion::Gothic2Early(v) = &sub.material.material.version else {
                panic!()
            };
            sub.material.material.version =
                MaterialVersion::Gothic2(v.clone().with_alpha(AlphaFunction::from_raw(3)));
        }
    }
    assert_eq!(early, modern);
    assert_eq!(early.to_bytes(l).unwrap(), MODERN_MDL);
}

#[test]
fn weight_bounds_require_the_combined_hierarchy() {
    let l = Limits::default();
    let mut m = Model::from_bytes(EARLY_MDL, Gothic2, l).unwrap();
    // 33 is representable in standalone storage, but outside this hierarchy.
    m.mesh.meshes[0].weights[0][1].node_index = 33;
    let bytes = m.mesh.to_bytes(l).unwrap();
    assert_eq!(
        ModelMesh::from_bytes(&bytes, Gothic2, l).unwrap().meshes[0].weights[0][1].node_index,
        33
    );
    assert_eq!(m.validate(l).unwrap_err().kind, ErrorKind::InvalidData);
    assert_eq!(m.to_bytes(l).unwrap_err().kind, ErrorKind::InvalidData);
    // A valid skeleton ID need not be a local list position or list member.
    m.mesh.meshes[0].weights[0][1].node_index = 31;
    assert!(m.validate(l).is_ok());
}

#[test]
fn version_checks_reject_unknown_values_and_game_mismatches() {
    let l = Limits::default();
    assert_eq!(MrmVersion::from_raw(0x0605), None);
    assert_eq!(ModelMeshVersion::from_raw(0x04060506), None);
    for bytes in [EARLY_MDM, MODERN_MDM] {
        assert_eq!(
            ModelMesh::from_bytes(bytes, Gothic1, l).unwrap_err().kind,
            ErrorKind::Unsupported
        );
    }
    let mut r = Reader::new(EARLY_MRM);
    assert_eq!(
        MultiResolutionMesh::read(&mut r, Gothic1, l)
            .unwrap_err()
            .offset,
        Some(6)
    );
    assert_eq!(r.position(), 0);
    let mut m = MultiResolutionMesh::from_bytes(EARLY_MRM, Gothic2, l).unwrap();
    m.version = MrmVersion::Gothic1;
    assert_eq!(m.to_bytes(l).unwrap_err().kind, ErrorKind::Unsupported);
    let mut m = ModelMesh::new(Gothic1);
    m.version = ModelMeshVersion::Gothic2;
    assert_eq!(m.to_bytes(l).unwrap_err().kind, ErrorKind::Unsupported);
}

#[test]
fn early_model_read_preserves_cursor_and_enforces_input_budget() {
    let l = Limits::default();
    let mut bytes = EARLY_MDL.to_vec();
    bytes.extend_from_slice(b"suffix");
    let mut r = Reader::new(&bytes);
    let m = Model::read(&mut r, Gothic2, l).unwrap();
    assert_eq!(m.mesh.version, ModelMeshVersion::Gothic2Early);
    assert_eq!(r.position(), EARLY_MDL.len());
    let mut r = Reader::new(&bytes);
    let e = Model::read(
        &mut r,
        Gothic2,
        Limits {
            bytes: EARLY_MDL.len() - 1,
            ..l
        },
    )
    .unwrap_err();
    assert_eq!(e.kind, ErrorKind::LimitExceeded);
    assert_eq!(r.position(), 0);
}
