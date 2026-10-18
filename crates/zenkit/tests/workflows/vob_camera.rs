use zenkit::{
    GameVersion,
    archive::Format,
    objects::*,
    values::{Mat4, Vec4},
    vob::*,
};
fn main() -> zenkit::Result<()> {
    let game = GameVersion::Gothic2;
    let mut graph = Graph::new()?;
    let mut key = Vob::new(game);
    key.base.physics_enabled = false;
    key.base.state = Some(SaveState::default());
    key.kind = VobKind::CameraFrame(CameraFrame {
        time: 2.5,
        original_pose: Mat4::from_rows([
            Vec4::from([1., 2., 3., 4.]),
            Vec4::from([5., 6., 7., 8.]),
            Vec4::from([9., 10., 11., 12.]),
            Vec4::from([13., 14., 15., 16.]),
        ]),
        ..CameraFrame::new(game)
    });
    let key = graph.insert(Object::Vob(key))?;
    let mut camera = Vob::new(game);
    camera.base.physics_enabled = false;
    camera.base.state = Some(SaveState::default());
    camera.kind = VobKind::Camera(Camera {
        auto_focus_vob: b"PLAYER".to_vec(),
        positions: vec![Link::new(b"position".to_vec(), Some(key))],
        targets: vec![
            Link::new(b"shared_target".to_vec(), Some(key)),
            Link::new(b"empty_target".to_vec(), None),
        ],
        state: Some(CameraState {
            started: true,
            cs_time: 1.25,
            ..Default::default()
        }),
        ..Default::default()
    });
    let camera = graph.insert(Object::Vob(camera))?;
    graph.add_root(Link::new(b"camera".to_vec(), Some(camera)))?;
    let mut metadata = Metadata::new(Format::BinSafe);
    metadata.save_game = true;
    let document = ObjectArchive {
        metadata,
        game,
        graph,
    };
    let bytes = document.to_bytes(Limits::default())?;
    let restored = ObjectArchive::from_bytes(&bytes, game, Limits::default())?;
    let ObjectRef::Vob(v) = restored
        .graph
        .get(restored.graph.roots()[0].target.unwrap())?
    else {
        unreachable!()
    };
    let VobKindRef::Camera(c) = &v.kind() else {
        unreachable!()
    };
    assert_eq!(c.positions[0].target, c.targets[0].target);
    assert!(c.targets[1].target.is_none());
    assert_eq!(c.state.unwrap().cs_time, 1.25);
    let ObjectRef::Vob(v) = restored.graph.get(c.positions[0].target.unwrap())? else {
        unreachable!()
    };
    let VobKindRef::CameraFrame(f) = &v.kind() else {
        unreachable!()
    };
    assert_eq!(f.original_pose[1][0], 2.);
    assert_eq!(f.original_pose[0][1], 5.);
    println!("Restored shared camera keyframes, row-major pose and trailing save state");
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
