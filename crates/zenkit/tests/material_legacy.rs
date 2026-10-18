use zenkit::objects::ObjectRef;
use zenkit::{
    ErrorKind,
    GameVersion::{Gothic1, Gothic2},
    archive::{self, Format, Header, Value},
    io::Reader,
    material::{MaterialRecord, MaterialVersion},
    mesh::{Limits, Model, ModelMesh, ModelMeshVersion, MrmVersion, MultiResolutionMesh},
    objects::ObjectArchive,
};

const RECORD: &[u8] = include_bytes!("fixtures/synthetic-material-record-legacy-binary-g2.mat");
const GRAPH: &[u8] = include_bytes!("fixtures/synthetic-material-legacy-binary-g2.zen");
const MRM: &[u8] = include_bytes!("fixtures/synthetic-mesh-mrm-legacy-g2.mrm");
const MDM: &[u8] = include_bytes!("fixtures/synthetic-mesh-mdm-legacy-g2.mdm");
const MDL: &[u8] = include_bytes!("fixtures/synthetic-mesh-mdl-legacy-g2.mdl");

fn record() -> MaterialRecord {
    let mut reader = archive::Reader::new(RECORD, Default::default()).unwrap();
    assert_eq!(reader.read_int().unwrap(), 1);
    let value = MaterialRecord::read_auto(&mut reader, 10000).unwrap();
    reader.finish().unwrap();
    value
}

#[test]
fn legacy_parameters_preserve_all_bits_and_leave_default_mapping_aligned() {
    let r = record();
    assert_eq!(r.material.version_identifier(), 0x7401);
    let MaterialVersion::Gothic2Legacy(fields) = &r.material.version else {
        panic!()
    };
    assert_eq!(fields.detail_object_scale, 1.75);
    assert_eq!(fields.wave_mode.raw(), 7);
    assert_eq!(fields.wave_speed.raw(), 3);
    assert_eq!(fields.parameters, [0, 0xff, 0x7d, 0x80, 0x3a]);
    assert_eq!(r.material.default_mapping.to_array(), [2.34375, -1.125]);
    let reader = archive::Reader::new(RECORD, Default::default()).unwrap();
    let mut writer = archive::Writer::new(reader.header(), Default::default()).unwrap();
    writer.write(b"materialCount", Value::Int(1)).unwrap();
    r.write(&mut writer, Gothic2, 10000).unwrap();
    assert_eq!(writer.finish().unwrap(), RECORD);
}

#[test]
fn legacy_material_graphs_keep_versions_and_shared_identity() {
    let doc = ObjectArchive::from_bytes(GRAPH, Gothic2, Default::default()).unwrap();
    let out = doc.to_bytes(Default::default()).unwrap();
    let loaded = ObjectArchive::from_bytes(&out, Gothic2, Default::default()).unwrap();
    assert_eq!(
        loaded.graph.roots()[0].target,
        loaded.graph.roots()[1].target
    );
    let ObjectRef::Material(m) = loaded
        .graph
        .get(loaded.graph.roots()[0].target.unwrap())
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(m, &record().material);
    assert_eq!(loaded.to_bytes(Default::default()).unwrap(), out);
}

#[test]
fn legacy_models_preserve_independent_layouts_and_sparse_ids() {
    let l = Limits::default();
    let mrm = MultiResolutionMesh::from_bytes(MRM, Gothic2, l).unwrap();
    assert_eq!(mrm.version, MrmVersion::Gothic2Legacy);
    assert_eq!(mrm.alpha_test, Some(7));
    assert_eq!(mrm.to_bytes(l).unwrap(), MRM);
    let mdm = ModelMesh::from_bytes(MDM, Gothic2, l).unwrap();
    assert_eq!(mdm.version, ModelMeshVersion::Gothic2Legacy);
    assert_eq!(mdm.meshes[0].nodes, [17, 32]);
    assert_eq!(mdm.to_bytes(l).unwrap(), MDM);
    let mdl = Model::from_bytes(MDL, Gothic2, l).unwrap();
    assert_eq!(mdl.hierarchy.nodes.len(), 33);
    assert_eq!(mdl.to_bytes(l).unwrap(), MDL);
    let mut reader = Reader::new(MDL);
    assert_eq!(
        Model::read(&mut reader, Gothic1, l).unwrap_err().kind,
        ErrorKind::Unsupported
    );
    assert_eq!(reader.position(), 0);
    let mut reader = Reader::new(MDL);
    assert_eq!(
        Model::read(
            &mut reader,
            Gothic2,
            Limits {
                bytes: MDL.len() - 1,
                ..l
            }
        )
        .unwrap_err()
        .kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(reader.position(), 0);
}

#[test]
fn legacy_tagged_encoding_is_explicitly_unsupported() {
    let r = record();
    for format in [Format::Ascii, Format::BinSafe] {
        let mut w = archive::Writer::new(Header::new(format), Default::default()).unwrap();
        assert_eq!(
            r.write(&mut w, Gothic2, 10000).unwrap_err().kind,
            ErrorKind::Unsupported
        );
        let mut doc = ObjectArchive::from_bytes(GRAPH, Gothic2, Default::default()).unwrap();
        doc.metadata.format = format;
        assert_eq!(
            doc.to_bytes(Default::default()).unwrap_err().kind,
            ErrorKind::Unsupported
        );
    }
    // A tagged header alone cannot authorize interpreting the five raw bytes.
    let mut bytes =
        include_bytes!("fixtures/synthetic-material-record-early-ascii-g2.mat").to_vec();
    let at = bytes.windows(7).position(|v| v == b" 24578 ").unwrap();
    bytes[at + 1..at + 6].copy_from_slice(b"29697");
    let mut reader = archive::Reader::new(&bytes, Default::default()).unwrap();
    reader.read_int().unwrap();
    assert_eq!(
        MaterialRecord::read_auto(&mut reader, 10000)
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
}
