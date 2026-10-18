// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// SPDX-License-Identifier: MIT
use crate::{
    Error, ErrorKind, GameVersion, Result, archive,
    objects::{Limits, ObjectArchive, ObjectRef},
};

fn interpretation(bytes: &[u8], game: GameVersion, limits: Limits) -> Result<()> {
    let document = ObjectArchive::from_bytes(bytes, game, limits)?;
    let [root] = document.graph.roots() else {
        return Err(Error::operation(ErrorKind::InvalidData, "world root count"));
    };
    let Some(id) = root.target else {
        return Err(Error::operation(ErrorKind::WrongObjectType, "world root"));
    };
    if !matches!(document.graph.get(id)?, ObjectRef::World(_)) {
        return Err(Error::operation(ErrorKind::WrongObjectType, "world root"));
    }
    Ok(())
}

/// Determine the unique valid game interpretation of one complete normal world.
///
/// This convenience performs up to two sequential, fully bounded parses. Each
/// temporary graph is dropped before the next attempt. It never scans for byte
/// signatures or guesses from an archive header. Limits apply to each attempt;
/// total work can therefore be twice that of an explicit-game parse.
///
/// Save archives require an explicit game.
/// A valid prefab whose fields fit both games is ambiguous and returns
/// [`ErrorKind::Unsupported`]. The same kind with a different context reports no
/// valid interpretation; use [`ObjectArchive::from_bytes`] with an explicit game
/// for its precise parse error. Any [`ErrorKind::LimitExceeded`] is propagated:
/// an exhausted attempt cannot establish that the other game is correct.
pub fn detect_game(bytes: &[u8], limits: Limits) -> Result<GameVersion> {
    let mut reader = archive::Reader::new(bytes, limits.archive)?;
    if reader.header().save_game {
        return Err(Error::operation(
            ErrorKind::Unsupported,
            "world save requires explicit game",
        ));
    }
    let root_offset = reader.position();
    if !reader
        .begin_object()?
        .is_some_and(|h| h.class_name == b"oCWorld:zCWorld")
    {
        return Err(Error::new(
            ErrorKind::WrongObjectType,
            root_offset,
            "world root",
        ));
    }
    drop(reader);
    let mut selected = None;
    for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
        match interpretation(bytes, game, limits) {
            Ok(()) if selected.is_some() => {
                return Err(Error::operation(
                    ErrorKind::Unsupported,
                    "ambiguous world game",
                ));
            }
            Ok(()) => selected = Some(game),
            Err(error) if error.kind == ErrorKind::LimitExceeded => return Err(error),
            Err(_) => {}
        }
    }
    selected.ok_or_else(|| {
        Error::operation(ErrorKind::Unsupported, "no valid world game interpretation")
    })
}

/// Load one normal world after [`detect_game`] establishes an unambiguous game.
///
/// Returns the shared object archive so all aliases, cycles, metadata and section
/// occurrences remain available. This performs at most three bounded parses,
/// retaining at most one graph at a time. Prefer [`ObjectArchive::from_bytes`]
/// when the game is already known or when loading a save.
pub fn from_bytes(bytes: &[u8], limits: Limits) -> Result<ObjectArchive> {
    ObjectArchive::from_bytes(bytes, detect_game(bytes, limits)?, limits)
}
