use zenkit::objects::ObjectRef;
use zenkit::{
    ErrorKind,
    GameVersion::{Gothic1, Gothic2},
    archive::{self, Value},
    material::{AlphaFunction, MaterialRecord, MaterialVersion},
    objects::ObjectArchive,
};

const FIXTURES: [(&[u8], &[u8]); 3] = [
    (
        include_bytes!("fixtures/synthetic-material-early-ascii-g2.zen"),
        include_bytes!("fixtures/synthetic-material-record-early-ascii-g2.mat"),
    ),
    (
        include_bytes!("fixtures/synthetic-material-early-binary-g2.zen"),
        include_bytes!("fixtures/synthetic-material-record-early-binary-g2.mat"),
    ),
    (
        include_bytes!("fixtures/synthetic-material-early-bin_safe-g2.zen"),
        include_bytes!("fixtures/synthetic-material-record-early-bin_safe-g2.mat"),
    ),
];

#[test]
fn early_material_records_preserve_version_fields_and_auto_selection() {
    for (_, bytes) in FIXTURES {
        let mut reader = archive::Reader::new(bytes, Default::default()).unwrap();
        let header = reader.header();
        assert_eq!(reader.read_int().unwrap(), 1);
        let record = MaterialRecord::read_auto(&mut reader, 10000).unwrap();
        reader.finish().unwrap();
        assert_eq!(record.material.version.game(), Gothic2);
        assert_eq!(record.material.version_identifier(), 0x6002);
        let MaterialVersion::Gothic2Early(fields) = &record.material.version else {
            panic!()
        };
        assert_eq!(fields.wave_max_amplitude, 30.25);
        assert_eq!(fields.wave_grid_size, 100.5);
        assert!(fields.ignore_sun);
        assert_eq!(
            record.material.default_mapping.to_array(),
            [2.34375, -1.125]
        );
        assert_eq!(
            record.material.texture_scale.as_bytes(),
            b" 512.125  -64.5 "
        );
        let mut writer = archive::Writer::new(header, Default::default()).unwrap();
        writer.write(b"materialCount", Value::Int(1)).unwrap();
        record.write(&mut writer, Gothic2, 10000).unwrap();
        let out = writer.finish().unwrap();
        let mut reader = archive::Reader::new(&out, Default::default()).unwrap();
        reader.read_int().unwrap();
        assert_eq!(
            MaterialRecord::read(&mut reader, Gothic2, 10000).unwrap(),
            record
        );
        reader.finish().unwrap();
        let mut reader = archive::Reader::new(bytes, Default::default()).unwrap();
        reader.read_int().unwrap();
        assert_eq!(
            MaterialRecord::read(&mut reader, Gothic1, 10000)
                .unwrap_err()
                .kind,
            ErrorKind::Unsupported
        );
    }
}

#[test]
fn early_material_object_graphs_retain_layout_and_alias_identity() {
    for (bytes, _) in FIXTURES {
        let doc = ObjectArchive::from_bytes(bytes, Gothic2, Default::default()).unwrap();
        assert_eq!(doc.graph.roots()[0].target, doc.graph.roots()[1].target);
        let ObjectRef::Material(m) = doc.graph.get(doc.graph.roots()[0].target.unwrap()).unwrap()
        else {
            panic!()
        };
        assert_eq!(m.version_identifier(), 0x6002);
        let out = doc.to_bytes(Default::default()).unwrap();
        let loaded = ObjectArchive::from_bytes(&out, Gothic2, Default::default()).unwrap();
        assert_eq!(loaded.metadata, doc.metadata);
        assert_eq!(
            loaded.graph.roots()[0].target,
            loaded.graph.roots()[1].target
        );
        let ObjectRef::Material(restored) = loaded
            .graph
            .get(loaded.graph.roots()[0].target.unwrap())
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(restored, m);
        assert_eq!(loaded.to_bytes(Default::default()).unwrap(), out);
    }
}

#[test]
fn material_modernization_requires_the_caller_to_supply_alpha() {
    let doc = ObjectArchive::from_bytes(FIXTURES[0].0, Gothic2, Default::default()).unwrap();
    let ObjectRef::Material(m) = doc.graph.get(doc.graph.roots()[0].target.unwrap()).unwrap()
    else {
        panic!()
    };
    let MaterialVersion::Gothic2Early(fields) = &m.version else {
        panic!()
    };
    let modern = fields.clone().with_alpha(AlphaFunction::BLEND);
    assert_eq!(modern.alpha_func, AlphaFunction::BLEND);
    assert_eq!(modern.without_alpha(), *fields);
}
