use zenkit::{
    GameVersion,
    animation::{Limits, ModelAnimation},
    model_script::{Event, ModelScript, SyntaxMode, TagPayload},
    morph::MorphMesh,
};
fn main() -> zenkit::Result<()> {
    let limits = Limits::default();
    let man = ModelAnimation::from_bytes(
        include_bytes!("../fixtures/synthetic-animation-man-g1.man"),
        limits,
    )?;
    // Channel 1 refers to node_indices[1], which is global skeleton node 4.
    let sample = man.sample(1, 1).expect("known fixture channel");
    assert_eq!(man.node_indices[1], 4);
    assert_eq!(man.samples().len(), 6);
    let morph = MorphMesh::from_bytes(
        include_bytes!("../fixtures/synthetic-animation-mmb-g1.mmb"),
        GameVersion::Gothic1,
        limits,
    )?;
    assert!(morph.animations[0].sample(1, 1).is_some());
    let script = ModelScript::from_mds(
        include_bytes!("../fixtures/synthetic-animation-mds-g1.mds"),
        SyntaxMode::Strict,
        limits,
    )?;
    let mut frame_arguments = 0;
    for event in &script.animations[0].events {
        if let Event::Tag(tag) = event
            && let TagPayload::Frames(frames) = tag.payload()?
        {
            frame_arguments += frames.iter().count();
        }
    }
    assert_eq!(frame_arguments, 20);
    println!(
        "MAN node {} position {:?}; {} morph animations; {} ordered script events",
        man.node_indices[1],
        sample.position,
        morph.animations.len(),
        script.animations[0].events.len()
    );
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
