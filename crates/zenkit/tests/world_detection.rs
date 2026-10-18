use zenkit::{
    ErrorKind, GameVersion,
    objects::{Limits, ObjectArchive},
    world,
};

// Independent ASCII records: no production writer supplies the game evidence.
fn world_bytes(game: Option<GameVersion>, save: bool) -> Vec<u8> {
    let mut body = String::new();
    if let Some(game) = game {
        body.push_str("[SkyCtrl % 0 0]\n[sky zCSkyControler_Outdoor 0 1]\n");
        for name in [
            "masterTime",
            "rainWeight",
            "rainStart",
            "rainStop",
            "rainSctTimer",
            "rainSndVol",
            "dayCtr",
        ] {
            body.push_str(&format!("{name}=float:1.25\n"));
        }
        if game == GameVersion::Gothic2 {
            body.push_str(
                "fadeScale=float:0.5\nrenderLightning=bool:1\nisRaining=bool:0\nrainCtr=int:-7\n",
            );
        }
        body.push_str("[]\n[]\n");
    }
    body.push_str("[EndMarker % 0 0]\n[]\n");
    if save {
        body.push_str("npcCount=int:0\nNoOfEntries=int:0\nspawningEnabled=bool:0\n");
        if game == Some(GameVersion::Gothic2) {
            body.push_str("spawnFlags=int:0\n");
        }
    }
    format!(
        "ZenGin Archive\nver 1\nzCArchiverGeneric\nASCII\nsaveGame {}\nEND\nobjects {}\nEND\n[world oCWorld:zCWorld 64513 0]\n{body}[]\n",
        usize::from(save), if game.is_some() { 2 } else { 1 },
    ).into_bytes()
}

#[test]
fn independent_world_layout_selects_unique_game_and_returns_shared_archive() {
    for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
        let bytes = world_bytes(Some(game), false);
        assert_eq!(world::detect_game(&bytes, Limits::default()).unwrap(), game);
        let document = world::from_bytes(&bytes, Limits::default()).unwrap();
        assert_eq!(document.game, game);
        assert_eq!(document.graph.roots().len(), 1);
    }
}

#[test]
fn ambiguous_prefabs_saves_wrong_roots_and_resource_exhaustion_do_not_guess() {
    let prefab = world_bytes(None, false);
    for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
        ObjectArchive::from_bytes(&prefab, game, Limits::default()).unwrap();
        let saved = world_bytes(Some(game), true);
        ObjectArchive::from_bytes(&saved, game, Limits::default()).unwrap();
        assert_eq!(
            world::detect_game(&saved, Limits::default())
                .unwrap_err()
                .kind,
            ErrorKind::Unsupported
        );
    }
    let ambiguous = world::detect_game(&prefab, Limits::default()).unwrap_err();
    assert_eq!(ambiguous.kind, ErrorKind::Unsupported);
    assert_eq!(ambiguous.context, "ambiguous world game");
    let bytes = world_bytes(Some(GameVersion::Gothic2), false);
    let limits = Limits {
        nodes: 1,
        ..Default::default()
    };
    assert_eq!(
        world::detect_game(&bytes, limits).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    let wrong = String::from_utf8(prefab)
        .unwrap()
        .replace("oCWorld:zCWorld", "zCWayNet");
    assert_eq!(
        world::detect_game(wrong.as_bytes(), Limits::default())
            .unwrap_err()
            .kind,
        ErrorKind::WrongObjectType
    );
    for cut in 0..bytes.len() - 1 {
        assert!(
            world::detect_game(&bytes[..cut], Limits::default()).is_err(),
            "prefix {cut}"
        );
    }
}
