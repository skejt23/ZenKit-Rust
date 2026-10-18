use crate as zenkit;
use zenkit::objects::ObjectMut;
use zenkit::{
    ErrorKind, GameVersion as Game,
    archive::{self, AddressSpace, AsciiSeparator, Format, TableBudget},
    io, mesh,
};

// Independent primitive wire authoring: no production archive/material writer.
struct Wire {
    format: Format,
    body: Vec<u8>,
    keys: Vec<Vec<u8>>,
    frames: Vec<usize>,
}
impl Wire {
    fn new(format: Format) -> Self {
        Self {
            format,
            body: vec![],
            keys: vec![],
            frames: vec![],
        }
    }
    fn header(&self) -> Vec<u8> {
        let f = match self.format {
            Format::Ascii => "ASCII",
            Format::Binary => "BINARY",
            Format::BinSafe => "BIN_SAFE",
        };
        let mut h = format!(
            "ZenGin Archive\nver 1\nEmbeddedTest\n{f}\nsaveGame 0\ndate RAW_DATE\nuser OWNER\nEND\n"
        )
        .into_bytes();
        if self.format == Format::BinSafe {
            h.extend(2u32.to_le_bytes());
            h.extend(100u32.to_le_bytes());
            h.extend(0u32.to_le_bytes());
        } else {
            h.extend(b"objects 100\nEND\n\n");
        }
        h
    }
    fn record(&mut self, line: &[u8]) {
        if self.format == Format::Ascii {
            self.body.extend(line);
            self.body.push(b'\n');
        } else {
            self.body.push(1);
            self.body.extend((line.len() as u16).to_le_bytes());
            self.body.extend(line);
        }
    }
    fn begin(&mut self, name: &str, class: &str, version: u16, index: u32) {
        if self.format == Format::Binary {
            self.frames.push(self.body.len());
            self.body.extend(0u32.to_le_bytes());
            self.body.extend(version.to_le_bytes());
            self.body.extend(index.to_le_bytes());
            self.body.extend(name.as_bytes());
            self.body.push(0);
            self.body.extend(class.as_bytes());
            self.body.push(0);
        } else {
            self.record(format!("[{name} {class} {version} {index}]").as_bytes());
        }
    }
    fn end(&mut self) {
        if self.format == Format::Binary {
            let p = self.frames.pop().unwrap();
            let n = (self.body.len() - p) as u32;
            self.body[p..p + 4].copy_from_slice(&n.to_le_bytes());
        } else {
            self.record(b"[]");
        }
    }
    fn field(&mut self, name: &str, tag: u8, data: &[u8], text: &str) {
        if self.format == Format::Ascii {
            let ty = match tag {
                1 => "string",
                2 => "int",
                3 => "float",
                6 => "bool",
                8 => "color",
                16 => "rawFloat",
                17 => "enum",
                _ => panic!(),
            };
            self.body.extend(format!("{name}={ty}:{text}\n").as_bytes());
        } else if self.format == Format::BinSafe {
            let key = self
                .keys
                .iter()
                .position(|k| k == name.as_bytes())
                .unwrap_or_else(|| {
                    self.keys.push(name.as_bytes().to_vec());
                    self.keys.len() - 1
                });
            self.body.push(18);
            self.body.extend((key as u32).to_le_bytes());
            self.body.push(tag);
            if matches!(tag, 1 | 9 | 16) {
                self.body.extend((data.len() as u16).to_le_bytes());
            }
            self.body.extend(data);
        } else {
            if matches!(tag, 6 | 17) {
                self.body.push(data[0]);
            } else {
                self.body.extend(data);
                if tag == 1 {
                    self.body.push(0);
                }
            }
        }
    }
    fn string(&mut self, name: &str, value: &str) {
        self.field(name, 1, value.as_bytes(), value);
    }
    fn word(&mut self, name: &str, tag: u8, value: u32) {
        self.field(name, tag, &value.to_le_bytes(), &value.to_string());
    }
    fn float(&mut self, name: &str, value: f32) {
        self.field(name, 3, &value.to_bits().to_le_bytes(), &value.to_string());
    }
    fn table(&self) -> Vec<u8> {
        let mut v = (self.keys.len() as u32).to_le_bytes().to_vec();
        for (i, key) in self.keys.iter().enumerate().rev() {
            v.extend((key.len() as u16).to_le_bytes());
            v.extend((i as u16).to_le_bytes());
            v.extend(123u32.to_le_bytes());
            v.extend(key);
        }
        v
    }
    fn finish(&self, origin: usize, remote: Option<usize>) -> Vec<u8> {
        let mut h = self.header();
        if self.format == Format::BinSafe {
            let at = h.len() - 4;
            let target = remote.unwrap_or(origin + h.len() + self.body.len());
            h[at..].copy_from_slice(&(target as u32).to_le_bytes());
        }
        h.extend(&self.body);
        if self.format == Format::BinSafe && remote.is_none() {
            h.extend(self.table());
        }
        h
    }
}
fn material(w: &mut Wire, game: Game, index: u32) {
    w.string("wrapper", "PREFIX");
    w.begin(
        "material_obj",
        "zCMaterial",
        if game == Game::Gothic1 { 17408 } else { 0x9c03 },
        index,
    );
    w.string("name", "SURFACE");
    w.word("matGroup", 17, 3);
    w.field("color", 8, &[30, 20, 10, 40], "10 20 30 40");
    w.float("smoothAngle", 17.25);
    w.string("texture", "TEX.TGA");
    w.string("texScale", "1.250 -2.5");
    w.float("texAniFPS", 3.5);
    w.word("texAniMapMode", 17, 1);
    w.string("texAniMapDir", "-3 4.00");
    w.word("noCollDet", 6, 1);
    w.word("noLightmap", 6, 0);
    w.word("lodDontCollapse", 6, 1);
    w.string("detailObject", "DETAIL");
    if game == Game::Gothic2 {
        w.float("detailObjectScale", 2.5);
        w.word("forceOccluder", 6, 1);
        w.word("environmentalMapping", 6, 1);
        w.float("environmentalMappingStrength", 0.75);
        w.word("waveMode", 17, 2);
        w.word("waveSpeed", 17, 1);
        w.float("waveMaxAmplitude", 1.5);
        w.float("waveGridSize", 4.5);
        w.word("ignoreSunLight", 6, 1);
        w.word("alphaFunc", 17, 3);
    }
    let mut mapping = 1.5f32.to_le_bytes().to_vec();
    mapping.extend((-2.25f32).to_le_bytes());
    w.field("defaultMapping", 16, &mapping, "1.5 -2.25");
    w.end();
}
fn chunk(id: u16, body: &[u8]) -> Vec<u8> {
    let mut v = id.to_le_bytes().to_vec();
    v.extend((body.len() as u32).to_le_bytes());
    v.extend(body);
    v
}
fn msh(format: Format, game: Game, origin: usize, count: u32, separator: bool) -> Vec<u8> {
    msh_with_alpha(
        format,
        game,
        origin,
        count,
        separator,
        (game == Game::Gothic2).then_some(7),
    )
}
fn msh_with_alpha(
    format: Format,
    game: Game,
    origin: usize,
    count: u32,
    separator: bool,
    alpha: Option<u8>,
) -> Vec<u8> {
    let mut marker = (if game == Game::Gothic1 { 9u16 } else { 265 })
        .to_le_bytes()
        .to_vec();
    marker.extend([0; 16]);
    marker.extend(b"fixture\n");
    let mut v = chunk(0xb000, &marker);
    v.extend(chunk(0xb010, &[0; 86]));
    let mut w = Wire::new(format);
    w.body.extend(count.to_le_bytes());
    for i in 0..count {
        material(&mut w, game, i);
    }
    if let Some(alpha) = alpha {
        // The alpha byte precedes any BIN_SAFE key table retained in the suffix.
        w.body.push(alpha);
    }
    let mut archive = w.finish(origin + v.len() + 6, None);
    if !separator && format == Format::Ascii {
        let at = w.header().len() - 1;
        archive.remove(at);
    }
    archive.extend([0xde, 0xad]);
    v.extend(chunk(0xb020, &archive));
    for id in [0xb030, 0xb040, 0xb050] {
        v.extend(chunk(id, &0u32.to_le_bytes()));
    }
    v.extend(chunk(0xb060, &[]));
    v
}

#[test]
fn missing_g2_alpha_does_not_consume_an_adjacent_binsafe_table() {
    let limits = mesh::Limits::default();
    for count in [0, 2] {
        let mut wire = Wire::new(Format::BinSafe);
        for index in 0..count {
            material(&mut wire, Game::Gothic2, index);
        }
        let mut expected_tail = wire.table();
        expected_tail.extend(b"\xde\xad");
        for origin in [0, 73] {
            for alpha in [None, Some(0), Some(1), Some(7), Some(255)] {
                let bytes =
                    msh_with_alpha(Format::BinSafe, Game::Gothic2, origin, count, true, alpha);
                let mut file = vec![0x77; origin];
                file.extend(&bytes);
                file.extend(b"suffix");
                let mut reader = io::Reader::new(&file);
                reader.take_bytes(origin, "prefix").unwrap();
                let mut value =
                    mesh::Mesh::read(&mut reader, Game::Gothic2, false, limits).unwrap();
                assert_eq!(value.alpha_test, alpha, "origin {origin}, count {count}");
                assert_eq!(value.alpha_test_enabled(), alpha != Some(0));
                assert_eq!(value.material_tail, expected_tail);
                assert_eq!(reader.position(), origin + bytes.len());
                value.validate(limits).unwrap();
                assert_eq!(
                    value.to_bytes(limits).unwrap_err().kind,
                    ErrorKind::Unsupported
                );

                value.materials.metadata.format = Format::Binary;
                if alpha.is_none() {
                    // Binary has no table pointer to distinguish a missing flag
                    // from the retained table. Choose the effective default.
                    assert_eq!(
                        value.to_bytes(limits).unwrap_err().kind,
                        ErrorKind::InvalidData
                    );
                    value.alpha_test = Some(1);
                }
                let output = value.to_bytes(limits).unwrap();
                let back = mesh::Mesh::from_bytes(&output, Game::Gothic2, false, limits).unwrap();
                assert_eq!(back.alpha_test, Some(alpha.unwrap_or(1)));
                assert_eq!(back.alpha_test_enabled(), alpha != Some(0));
                assert_eq!(back.material_tail, expected_tail);
            }
        }
    }
}

fn mrm_section(format: Format, game: Game, origin: usize) -> Vec<u8> {
    let mut w = Wire::new(format);
    material(&mut w, game, 0);
    let content = if format == Format::BinSafe {
        w.table()
    } else {
        vec![0x39, 0x27]
    };
    let mut v = (if game == Game::Gothic1 {
        0x305u16
    } else {
        0x905
    })
    .to_le_bytes()
    .to_vec();
    v.extend((content.len() as u32).to_le_bytes());
    v.extend(&content);
    v.push(1);
    v.extend([0; 96]);
    let at = origin + v.len();
    v.extend(w.finish(at, (format == Format::BinSafe).then_some(origin + 6)));
    if game == Game::Gothic2 {
        v.push(7);
    }
    v.extend([0; 86]);
    v.extend([0x5c; 16]);
    v
}
fn assert_material(m: &zenkit::material::MaterialRecord, format: Format) {
    assert_eq!(m.prefix, b"PREFIX");
    assert_eq!(m.object_name, b"material_obj");
    assert_eq!(m.material.name, b"SURFACE");
    assert_eq!(
        m.prefix_field_name.as_deref(),
        if format == Format::Binary {
            None
        } else {
            Some(b"wrapper".as_slice())
        }
    );
    assert_eq!(m.material.smooth_angle.to_bits(), 17.25f32.to_bits());
    assert_eq!(m.material.default_mapping.to_array(), [1.5, -2.25]);
}

#[test]
fn all_embedded_material_encodings_normalize_explicitly() {
    for game in [Game::Gothic1, Game::Gothic2] {
        for format in [Format::Ascii, Format::Binary, Format::BinSafe] {
            let bytes = msh(format, game, 0, 2, true);
            let mut m =
                mesh::Mesh::from_bytes(&bytes, game, false, mesh::Limits::default()).unwrap();
            assert_eq!(m.materials.metadata.format, format);
            assert_eq!(
                m.materials.metadata.user.as_deref(),
                Some(b"OWNER".as_slice())
            );
            for r in &m.materials.records {
                assert_material(r, format);
            }
            assert_eq!(m.alpha_test, (game == Game::Gothic2).then_some(7));
            assert!(m.material_tail.ends_with(b"\xde\xad"));
            if format != Format::BinSafe {
                assert_eq!(m.material_tail, b"\xde\xad");
            }
            m.validate(mesh::Limits::default()).unwrap();
            if format != Format::Binary {
                assert_eq!(
                    m.to_bytes(mesh::Limits::default()).unwrap_err().kind,
                    ErrorKind::Unsupported
                );
            }
            m.materials.metadata.format = Format::Binary;
            let encoded = m.to_bytes(mesh::Limits::default()).unwrap();
            let back =
                mesh::Mesh::from_bytes(&encoded, game, false, mesh::Limits::default()).unwrap();
            assert_eq!(
                back.materials.records[0].material,
                m.materials.records[0].material
            );
            assert_eq!(back.alpha_test, m.alpha_test);
            assert_eq!(back.material_tail, m.material_tail);
            let section = mrm_section(format, game, 6);
            let mut bytes = chunk(0xb100, &section);
            bytes.extend(chunk(0xb1ff, &[]));
            let mut m =
                mesh::MultiResolutionMesh::from_bytes(&bytes, game, mesh::Limits::default())
                    .unwrap();
            assert_material(&m.submeshes[0].material, format);
            assert_eq!(m.trailer, [0x5c; 16]);
            assert_eq!(
                m.alpha_test,
                if game == Game::Gothic2 { Some(7) } else { None }
            );
            m.validate(mesh::Limits::default()).unwrap();
            if format != Format::Binary {
                assert_eq!(
                    m.to_bytes(mesh::Limits::default()).unwrap_err().kind,
                    ErrorKind::Unsupported
                );
            }
            m.material_metadata.format = Format::Binary;
            let out = m.to_bytes(mesh::Limits::default()).unwrap();
            let back =
                mesh::MultiResolutionMesh::from_bytes(&out, game, mesh::Limits::default()).unwrap();
            assert_eq!(back.trailer, m.trailer);
        }
    }
}

#[test]
fn raw_count_ten_requires_the_canonical_ascii_separator() {
    let bytes = msh(Format::Ascii, Game::Gothic1, 0, 10, true);
    assert_eq!(
        mesh::Mesh::from_bytes(&bytes, Game::Gothic1, false, mesh::Limits::default())
            .unwrap()
            .materials
            .records
            .len(),
        10
    );
    let bytes = msh(Format::Ascii, Game::Gothic1, 0, 10, false);
    assert!(mesh::Mesh::from_bytes(&bytes, Game::Gothic1, false, mesh::Limits::default()).is_err());
}

#[test]
fn nested_skin_and_morph_keep_enclosing_table_origin() {
    for game in [Game::Gothic1, Game::Gothic2] {
        for format in [Format::Ascii, Format::BinSafe] {
            let mut bytes = chunk(0xe100, &4u32.to_le_bytes());
            let at = bytes.len() + 6;
            bytes.extend(chunk(0xb100, &mrm_section(format, game, at)));
            bytes.extend(chunk(0xb1ff, &[0; 10]));
            bytes.extend(chunk(0xe110, &[]));
            let mut skin =
                mesh::SoftSkinMesh::from_bytes(&bytes, game, mesh::Limits::default()).unwrap();
            assert_material(&skin.mesh.submeshes[0].material, format);
            skin.mesh.material_metadata.format = Format::Binary;
            skin.to_bytes(mesh::Limits::default()).unwrap();
            let mut marker = 3u32.to_le_bytes().to_vec();
            marker.extend(b"morph\n");
            let mut bytes = chunk(0xe020, &marker);
            let at = bytes.len() + 6;
            bytes.extend(chunk(0xb100, &mrm_section(format, game, at)));
            bytes.extend(chunk(0xb1ff, &[]));
            bytes.extend(chunk(0xe030, &0u16.to_le_bytes()));
            let morph = zenkit::morph::MorphMesh::from_bytes(
                &bytes,
                game,
                zenkit::animation::Limits::default(),
            )
            .unwrap();
            assert_material(&morph.mesh.submeshes[0].material, format);
        }
    }
}

#[test]
fn explicit_resolver_checks_storage_identity_and_absolute_positions() {
    let prefix = 73;
    let bytes = msh(Format::BinSafe, Game::Gothic1, prefix, 1, true);
    let mut file = vec![0x77; prefix];
    file.extend(&bytes);
    file.extend(b"suffix");
    let mut parent = io::Reader::new(&file);
    parent.take_bytes(prefix, "prefix").unwrap();
    let mut child = parent.substream(bytes.len(), "mesh").unwrap();
    let space = AddressSpace::new(&file, 0).unwrap();
    let m = mesh::Mesh::read_with_address_space(
        &mut child,
        space,
        Game::Gothic1,
        false,
        mesh::Limits::default(),
    )
    .unwrap();
    assert_material(&m.materials.records[0], Format::BinSafe);
    assert_eq!(child.remaining(), 0);
    let clone = file.clone();
    let mut parent = io::Reader::new(&file);
    parent.take_bytes(prefix, "prefix").unwrap();
    let mut child = parent.substream(bytes.len(), "mesh").unwrap();
    assert_eq!(
        mesh::Mesh::read_with_address_space(
            &mut child,
            AddressSpace::new(&clone, 0).unwrap(),
            Game::Gothic1,
            false,
            mesh::Limits::default()
        )
        .unwrap_err()
        .kind,
        ErrorKind::InvalidData
    );
    assert_eq!(child.position(), prefix);
    assert!(
        mesh::Mesh::read_with_address_space(
            &mut child,
            AddressSpace::new(&file, 1).unwrap(),
            Game::Gothic1,
            false,
            mesh::Limits::default()
        )
        .is_err()
    );
}

#[test]
fn remote_preceding_tables_and_work_budgets_are_independent_of_body_consumption() {
    let mut w = Wire::new(Format::BinSafe);
    w.word("answer", 2, 42);
    let table = w.table();
    let origin = 4096;
    let table_at = origin;
    let archive_at = origin + table.len() + 19;
    let mut bytes = table.clone();
    bytes.extend([0x77; 19]);
    bytes.extend(w.finish(archive_at, Some(table_at)));
    bytes.extend(b"parent suffix");
    let body_end = archive_at + w.header().len() + w.body.len();
    let space = AddressSpace::new(&bytes, origin).unwrap();
    let mut budget = TableBudget::new(2, table.len() * 2);
    for _ in 0..2 {
        let mut r = archive::Reader::embedded(
            space,
            archive_at..body_end,
            AsciiSeparator::Required,
            archive::Limits::default(),
            &mut budget,
        )
        .unwrap();
        assert_eq!(r.table_range(), Some(table_at..table_at + table.len()));
        assert_eq!(r.read_int().unwrap(), 42);
        assert_eq!(r.position(), body_end);
        r.finish().unwrap();
    }
    assert_eq!(budget.entries(), 2);
    let before = (budget.entries(), budget.bytes());
    assert_eq!(
        archive::Reader::embedded(
            space,
            archive_at..body_end,
            AsciiSeparator::Required,
            archive::Limits::default(),
            &mut budget
        )
        .unwrap_err()
        .kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!((budget.entries(), budget.bytes()), before);
    let mut invalid = bytes.clone();
    let word = archive_at - origin + w.header().len() - 4;
    invalid[word..word + 4].copy_from_slice(&(origin as u32 - 1).to_le_bytes());
    let e = archive::Reader::embedded(
        AddressSpace::new(&invalid, origin).unwrap(),
        archive_at..body_end,
        AsciiSeparator::Required,
        archive::Limits::default(),
        &mut TableBudget::new(99, 9999),
    )
    .unwrap_err();
    assert_eq!(e.offset, Some(origin + word));
    assert!(AddressSpace::new(&bytes, usize::MAX).is_err());
}

#[test]
fn full_world_separates_outer_table_and_embedded_material_origin() {
    use zenkit::{
        objects::{Limits, ObjectArchive},
        world::SectionData,
    };
    for game in [Game::Gothic1, Game::Gothic2] {
        for outer in [Format::Ascii, Format::Binary, Format::BinSafe] {
            for inner in [Format::Ascii, Format::BinSafe] {
                let mut w = Wire::new(outer);
                w.begin("world", "oCWorld:zCWorld", 64513, 0);
                w.begin("MeshAndBsp", "%", 0, 0);
                let mesh_origin = w.header().len() + w.body.len() + 8;
                let mut geometry = msh(inner, game, mesh_origin, 2, true);
                geometry.extend(chunk(0xc000, &[0, 0, 0, 0, 0, 0]));
                geometry.extend(chunk(0xc010, &[0; 4]));
                let mut tree = 1u32.to_le_bytes().to_vec();
                tree.extend(1u32.to_le_bytes());
                tree.extend([0; 32]);
                geometry.extend(chunk(0xc040, &tree));
                geometry.extend(chunk(0xc0ff, &[7]));
                let version: u32 = if game == Game::Gothic1 {
                    0x2090000
                } else {
                    0x4090000
                };
                w.body.extend(version.to_le_bytes());
                w.body.extend((geometry.len() as u32).to_le_bytes());
                w.body.extend(geometry);
                w.end();
                w.begin("EndMarker", "%", 0, 0);
                w.end();
                w.end();
                let bytes = w.finish(0, None);
                let mut doc = ObjectArchive::from_bytes(&bytes, game, Limits::default()).unwrap();
                let root = doc.graph.roots()[0].target.unwrap();
                let ObjectMut::World(world) = doc.graph.get_mut(root).unwrap() else {
                    panic!()
                };
                let SectionData::MeshAndBsp(g) = &mut world.sections[0].data else {
                    panic!()
                };
                assert_material(&g.mesh.materials.records[0], inner);
                assert_eq!(g.bsp.end_byte, 7);
                g.mesh.materials.metadata.format = Format::Binary;
                let encoded = doc.to_bytes(Limits::default()).unwrap();
                ObjectArchive::from_bytes(&encoded, game, Limits::default()).unwrap();
            }
        }
    }
}

#[test]
fn late_material_failures_have_one_absolute_offset_and_rollback() {
    let prefix = 31;
    let bytes = msh(Format::BinSafe, Game::Gothic2, prefix, 2, true);
    let mut complete = vec![0x42; prefix];
    complete.extend(&bytes);
    for length in 0..bytes.len() {
        let mut input = io::Reader::new(&complete[..prefix + length]);
        input.take_bytes(prefix, "prefix").unwrap();
        assert!(
            mesh::Mesh::read(&mut input, Game::Gothic2, false, mesh::Limits::default()).is_err()
        );
        assert_eq!(input.position(), prefix);
    }
    let needle = b"[material_obj zCMaterial";
    let p = complete
        .windows(needle.len())
        .position(|v| v == needle)
        .unwrap();
    complete[p + 14] = b'X';
    let mut input = io::Reader::new(&complete);
    input.take_bytes(prefix, "prefix").unwrap();
    let error =
        mesh::Mesh::read(&mut input, Game::Gothic2, false, mesh::Limits::default()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::WrongObjectType);
    assert_eq!(error.offset, Some(p - 3));
    assert_eq!(input.position(), prefix);
}

#[test]
fn per_table_and_aggregate_limits_precede_material_allocation() {
    let bytes = msh(Format::BinSafe, Game::Gothic1, 0, 2, true);
    let mut wire = Wire::new(Format::BinSafe);
    material(&mut wire, Game::Gothic1, 0);
    let keys = wire.keys.len();
    let table_bytes = wire.table().len();
    for limits in [
        mesh::Limits {
            material_table_keys: keys - 1,
            ..Default::default()
        },
        mesh::Limits {
            material_table_entries: keys - 1,
            ..Default::default()
        },
        mesh::Limits {
            material_table_bytes: table_bytes - 1,
            ..Default::default()
        },
    ] {
        assert_eq!(
            mesh::Mesh::from_bytes(&bytes, Game::Gothic1, false, limits)
                .unwrap_err()
                .kind,
            ErrorKind::LimitExceeded
        );
    }
    mesh::Mesh::from_bytes(
        &bytes,
        Game::Gothic1,
        false,
        mesh::Limits {
            material_table_keys: keys,
            material_table_entries: keys,
            material_table_bytes: table_bytes,
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn remote_table_after_parent_suffix_does_not_advance_body_and_overlap_rejects() {
    let mut wire = Wire::new(Format::BinSafe);
    wire.word("answer", 2, 42);
    let prefix = 17;
    let body_end = prefix + wire.header().len() + wire.body.len();
    let table_at = body_end + 23;
    let mut bytes = vec![0x66; prefix];
    bytes.extend(wire.finish(prefix, Some(table_at)));
    bytes.extend([0xa5; 23]);
    bytes.extend(wire.table());
    bytes.extend(b"file suffix");
    let mut r = archive::Reader::embedded(
        AddressSpace::new(&bytes, 0).unwrap(),
        prefix..body_end,
        AsciiSeparator::Required,
        Default::default(),
        &mut TableBudget::new(1, wire.table().len()),
    )
    .unwrap();
    assert_eq!(r.read_int().unwrap(), 42);
    assert_eq!(r.position(), body_end);
    r.finish().unwrap();
    assert_eq!(&bytes[body_end..table_at], &[0xa5; 23]);
    // The standalone contract still rejects a table followed by unrelated bytes.
    let mut standalone = wire.finish(0, None);
    standalone.extend(b"extra");
    assert!(archive::Reader::new(&standalone, Default::default()).is_err());
    // A valid empty table embedded in the header is not allowed to overlap it.
    let mut invalid = bytes.clone();
    let pointer = prefix + wire.header().len() - 4;
    invalid[pointer..pointer + 4].copy_from_slice(&(pointer as u32 - 4).to_le_bytes());
    invalid[pointer - 4..pointer].fill(0);
    let error = archive::Reader::embedded(
        AddressSpace::new(&invalid, 0).unwrap(),
        prefix..body_end,
        AsciiSeparator::Required,
        Default::default(),
        &mut TableBudget::new(10, 1000),
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidData);
    assert_eq!(error.offset, Some(pointer));
    // Truncated remote table errors point into that table, not at the body origin.
    for end in table_at..table_at + wire.table().len() {
        let mut work = TableBudget::new(10, 1000);
        let error = archive::Reader::embedded(
            AddressSpace::new(&bytes[..end], 0).unwrap(),
            prefix..body_end,
            AsciiSeparator::Required,
            Default::default(),
            &mut work,
        )
        .unwrap_err();
        assert!(error.offset.unwrap() >= table_at);
        assert_eq!((work.entries(), work.bytes()), (0, 0));
    }
}
