use zenkit::{
    ErrorKind, GameVersion,
    io::Reader,
    world::{BspChunkKind, BspLimits, BspNodeKind, BspTree},
};

fn chunk(out: &mut Vec<u8>, id: u16, bytes: &[u8]) {
    out.extend(id.to_le_bytes());
    out.extend((bytes.len() as u32).to_le_bytes());
    out.extend(bytes);
}
fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn floats(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|v| v.to_bits().to_le_bytes())
        .collect()
}
fn node(start: u32, count: u32) -> Vec<u8> {
    let mut result = floats(&[-1., -2., -3., 4., 5., 6.]);
    result.extend(words(&[start, count]));
    result
}
// Hand-authored records with repeated leaf polygons and two distinct lights.
fn fixture(game: GameVersion) -> Vec<u8> {
    let mut out = Vec::new();
    let mut header = 0xa2b3u16.to_le_bytes().to_vec();
    header.extend(17u32.to_le_bytes());
    chunk(&mut out, 0xc000, &header);
    chunk(&mut out, 0xc010, &words(&[3, 5, 3, 9]));
    let mut tree = words(&[3, 2]);
    tree.extend(node(0, 0));
    tree.push(0xaf); // Two leaf children; high bits must survive.
    tree.extend(floats(&[-12.5, 1.25, -2.5, 3.75]));
    if game == GameVersion::Gothic1 {
        tree.push(0xfe);
    }
    tree.extend(node(0, 2));
    tree.extend(node(1, 2));
    chunk(&mut out, 0xc040, &tree);
    chunk(&mut out, 0xc045, &floats(&[7., 8., 9., -4., -5., -6.]));
    let mut outdoors = words(&[1]);
    outdoors.extend(b"ROOM\xff \t\n");
    outdoors.extend(words(&[2, 1, 2, 0, 9, 1, 5]));
    chunk(&mut out, 0xc050, &outdoors);
    chunk(&mut out, 0xc0ff, &[0xe1]);
    out
}
fn tree_body(bytes: &[u8]) -> usize {
    let mut p = 0;
    loop {
        let id = u16::from_le_bytes(bytes[p..p + 2].try_into().unwrap());
        if id == 0xc040 {
            return p + 6;
        }
        p += 6 + u32::from_le_bytes(bytes[p + 2..p + 6].try_into().unwrap()) as usize;
    }
}

#[test]
fn independent_bsp_fields_topology_and_exact_bytes_in_both_games() {
    for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
        let input = fixture(game);
        let tree = BspTree::from_bytes(&input, game, BspLimits::default()).unwrap();
        assert_eq!(
            (tree.header_word, tree.mode, tree.end_byte),
            (0xa2b3, 17, 0xe1)
        );
        assert_eq!(tree.polygon_indices, [5, 3, 9]);
        assert_eq!(tree.preorder(2).unwrap(), [0, 1, 2]);
        assert_eq!(tree.parent_indices(2).unwrap(), [None, Some(0), Some(0)]);
        assert_eq!(tree.leaf_node_indices(2).unwrap(), [1, 2]);
        assert_eq!(tree.leaf_polygons(2, 4).unwrap(), [3, 3, 5, 9]);
        let BspNodeKind::Branch {
            plane,
            front,
            back,
            lod,
            extra_flags,
        } = tree.nodes[0].kind
        else {
            panic!()
        };
        assert_eq!(
            (
                plane.distance,
                plane.normal.x,
                plane.normal.y,
                plane.normal.z
            ),
            (-12.5, 1.25, -2.5, 3.75)
        );
        assert_eq!((front, back, extra_flags), (Some(1), Some(2), 0xa0));
        assert_eq!(lod, (game == GameVersion::Gothic1).then_some(0xfe));
        assert_eq!(tree.light_points.as_ref().unwrap()[0].x, 7.);
        assert_eq!(tree.light_points.as_ref().unwrap()[1].x, -4.);
        let outdoors = tree.outdoors.as_ref().unwrap();
        assert_eq!(outdoors.sectors[0].name, b"ROOM\xff \t");
        assert_eq!(outdoors.sectors[0].node_indices, [2, 0]);
        assert_eq!(outdoors.sectors[0].portal_polygon_indices, [9]);
        assert_eq!(outdoors.portal_polygon_indices, [5]);
        assert_eq!(tree.to_bytes(BspLimits::default()).unwrap(), input);
        assert_eq!(
            tree.leaf_polygons(2, 3).unwrap_err().kind,
            ErrorKind::LimitExceeded
        );
        assert_eq!(
            tree.validate_mesh_polygons(9).unwrap_err().kind,
            ErrorKind::InvalidData
        );
        tree.validate_mesh_polygons(10).unwrap();
    }
}

#[test]
fn reordered_node_storage_remaps_sectors_without_reassigning_lights() {
    let input = fixture(GameVersion::Gothic2);
    let mut tree = BspTree::from_bytes(&input, GameVersion::Gothic2, BspLimits::default()).unwrap();
    tree.nodes.swap(1, 2);
    let BspNodeKind::Branch { front, back, .. } = &mut tree.nodes[0].kind else {
        panic!()
    };
    *front = Some(2);
    *back = Some(1);
    tree.outdoors.as_mut().unwrap().sectors[0].node_indices[0] = 1;
    assert_eq!(tree.leaf_node_indices(2).unwrap(), [2, 1]);
    assert_eq!(tree.to_bytes(BspLimits::default()).unwrap(), input);
}

#[test]
fn branch_without_children_and_absent_child_leaf_bits_are_preserved() {
    let game = GameVersion::Gothic1;
    let mut tree = BspTree::new(game).unwrap();
    tree.nodes.push(tree.nodes[0]);
    tree.nodes[0].kind = BspNodeKind::Branch {
        plane: zenkit::mesh::Plane {
            distance: -0.,
            normal: Default::default(),
        },
        front: Some(1),
        back: None,
        lod: Some(255),
        extra_flags: 0x88,
    };
    tree.nodes[1].kind = BspNodeKind::Branch {
        plane: zenkit::mesh::Plane {
            distance: 123.,
            normal: Default::default(),
        },
        front: None,
        back: None,
        lod: Some(17),
        extra_flags: 0x4c,
    };
    tree.light_points.as_mut().unwrap().clear();
    let bytes = tree.to_bytes(BspLimits::default()).unwrap();
    let restored = BspTree::from_bytes(&bytes, game, BspLimits::default()).unwrap();
    assert!(!restored.nodes[1].is_leaf());
    assert!(restored.leaf_node_indices(2).unwrap().is_empty());
    assert_eq!(restored.to_bytes(BspLimits::default()).unwrap(), bytes);
    tree.nodes.pop();
    tree.nodes[0].kind = restored.nodes[1].kind;
    assert_eq!(
        tree.to_bytes(BspLimits::default()).unwrap_err().kind,
        ErrorKind::InvalidData
    );
}

#[test]
fn prefix_read_rolls_back_and_chunk_presence_order_are_explicit() {
    let game = GameVersion::Gothic2;
    let bytes = fixture(game);
    let mut input = bytes.clone();
    input.extend([0xaa, 0xbb]);
    let mut r = Reader::new(&input);
    BspTree::read(&mut r, game, BspLimits::default()).unwrap();
    assert_eq!(r.remaining(), 2);
    assert_eq!(
        BspTree::from_bytes(&input, game, BspLimits::default())
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
    for cut in 0..bytes.len() {
        let mut r = Reader::new(&bytes[..cut]);
        assert!(
            BspTree::read(&mut r, game, BspLimits::default()).is_err(),
            "cut {cut}"
        );
        assert_eq!(r.position(), 0);
    }
    let mut tree = BspTree::from_bytes(&bytes, game, BspLimits::default()).unwrap();
    tree.chunk_order.swap(1, 3); // LIGHT can precede TREE; its count is checked at end.
    let swapped = tree.to_bytes(BspLimits::default()).unwrap();
    let restored = BspTree::from_bytes(&swapped, game, BspLimits::default()).unwrap();
    assert_eq!(restored.chunk_order, tree.chunk_order);
    tree.light_points = None;
    tree.outdoors = None;
    tree.chunk_order
        .retain(|k| !matches!(k, BspChunkKind::Light | BspChunkKind::Outdoors));
    let minimal = tree.to_bytes(BspLimits::default()).unwrap();
    let restored = BspTree::from_bytes(&minimal, game, BspLimits::default()).unwrap();
    assert!(restored.light_points.is_none() && restored.outdoors.is_none());
    tree.chunk_order.push(BspChunkKind::Header);
    assert_eq!(
        tree.validate(BspLimits::default()).unwrap_err().kind,
        ErrorKind::InvalidData
    );
}

#[test]
fn counts_depth_ranges_kinds_and_draft_topology_fail_with_bounds() {
    let game = GameVersion::Gothic2;
    let bytes = fixture(game);
    let position = tree_body(&bytes);
    for count in [0, 2, 4, u32::MAX] {
        let mut raw = bytes.clone();
        raw[position..position + 4].copy_from_slice(&count.to_le_bytes());
        assert!(BspTree::from_bytes(&raw, game, BspLimits::default()).is_err());
    }
    for leaves in [0, 1, 3, u32::MAX] {
        let mut raw = bytes.clone();
        raw[position + 4..position + 8].copy_from_slice(&leaves.to_le_bytes());
        assert!(BspTree::from_bytes(&raw, game, BspLimits::default()).is_err());
    }
    let limits = BspLimits {
        depth: 1,
        ..Default::default()
    };
    assert_eq!(
        BspTree::from_bytes(&bytes, game, limits).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    let mut limits = BspLimits::default();
    limits.mesh.chunks = 5;
    assert_eq!(
        BspTree::from_bytes(&bytes, game, limits).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    let mut limits = BspLimits::default();
    limits.mesh.bytes = bytes.len() - 1;
    assert_eq!(
        BspTree::from_bytes(&bytes, game, limits).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    let original = BspTree::from_bytes(&bytes, game, BspLimits::default()).unwrap();
    for (front_value, back_value) in [
        (Some(0), Some(2)),
        (Some(1), Some(1)),
        (None, Some(2)),
        (Some(7), Some(2)),
    ] {
        let mut draft = original.clone();
        let BspNodeKind::Branch { front, back, .. } = &mut draft.nodes[0].kind else {
            panic!()
        };
        *front = front_value;
        *back = back_value;
        assert_eq!(
            draft.validate(BspLimits::default()).unwrap_err().kind,
            ErrorKind::InvalidData
        );
    }
    let mut draft = original.clone();
    draft.nodes[1].polygon_count = u32::MAX;
    assert_eq!(
        draft.validate(BspLimits::default()).unwrap_err().kind,
        ErrorKind::InvalidData
    );
    let mut draft = original.clone();
    draft.light_points.as_mut().unwrap().pop();
    assert_eq!(
        draft.validate(BspLimits::default()).unwrap_err().kind,
        ErrorKind::InvalidData
    );
    for delimiter in [0, b'\r', b'\n'] {
        let mut draft = original.clone();
        draft.outdoors.as_mut().unwrap().sectors[0]
            .name
            .push(delimiter);
        assert_eq!(
            draft.validate(BspLimits::default()).unwrap_err().kind,
            ErrorKind::InvalidData
        );
    }
    let mut raw = bytes;
    raw[0..2].copy_from_slice(&0xc099u16.to_le_bytes());
    assert_eq!(
        BspTree::from_bytes(&raw, game, BspLimits::default())
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
}

#[test]
fn bsp_unknown_chunks_and_surplus_header_payload_are_rejected() {
    for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
        let source = fixture(game);
        let mut unknown = source.clone();
        unknown[..2].copy_from_slice(&0x1234u16.to_le_bytes());
        assert_eq!(
            BspTree::from_bytes(&unknown, game, BspLimits::default())
                .unwrap_err()
                .kind,
            ErrorKind::Unsupported
        );
        let length = u32::from_le_bytes(source[2..6].try_into().unwrap());
        let mut padded = source;
        padded.insert(6 + length as usize, 0xab);
        padded[2..6].copy_from_slice(&(length + 1).to_le_bytes());
        assert_eq!(
            BspTree::from_bytes(&padded, game, BspLimits::default())
                .unwrap_err()
                .kind,
            ErrorKind::Unsupported
        );
    }
}
