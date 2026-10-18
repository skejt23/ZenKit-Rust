use zenkit::{
    GameVersion,
    material::{Material, MaterialRecord},
    mesh::{Limits, MultiResolutionMesh, SubMesh, Wedge},
    values::Vec3,
};
fn main() -> zenkit::Result<()> {
    let game = GameVersion::Gothic2;
    let limits = Limits::default();
    let mut mesh = MultiResolutionMesh::new(game);
    mesh.positions = vec![
        [0., 0., 0.].into(),
        [1., 0., 0.].into(),
        [0., 1., 0.].into(),
    ];
    let mut submesh = SubMesh::new(MaterialRecord {
        prefix: b"STONE".to_vec(),
        prefix_field_name: None,
        object_name: b"material".to_vec(),
        material: Material::new(game),
    });
    submesh.triangles.push([0, 1, 2]);
    submesh.wedges = (0..3)
        .map(|index| Wedge {
            normal: Vec3::from([0., 0., 1.]),
            index,
            ..Default::default()
        })
        .collect();
    mesh.submeshes.push(submesh);
    let bytes = mesh.to_bytes(limits)?;
    let mut loaded = MultiResolutionMesh::from_bytes(&bytes, game, limits)?;
    assert_eq!(loaded.submeshes[0].triangles, [[0, 1, 2]]);
    // This constructed file has no opaque updates. Files which have them require
    // an explicit discard_vertex_updates() before altering content arrays.
    loaded.positions[1].x = 2.;
    let edited = loaded.to_bytes(limits)?;
    let reopened = MultiResolutionMesh::from_bytes(&edited, game, limits)?;
    assert_eq!(reopened.positions[1].x, 2.);
    println!("MRM create/read/edit/write workflow passed");
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
