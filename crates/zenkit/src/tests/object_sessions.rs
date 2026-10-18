use crate as zenkit;
use std::panic::{AssertUnwindSafe, catch_unwind};
use zenkit::objects::{ObjectMut, ObjectRef};
use zenkit::{
    Error, ErrorKind, GameVersion as Game,
    archive::{
        self, AddressSpace, AsciiSeparator, Format, Header, Reader, TableBudget, Value, Writer,
    },
    objects::{Graph, Limits, Link, Metadata, Object, ReadSession, WriteSession},
    vob::{AiMove, SaveState, Visual, VisualKind, Vob},
    world::{Hierarchy, HierarchyEntry},
};

const FORMATS: [Format; 3] = [Format::Ascii, Format::Binary, Format::BinSafe];
const GAME: Game = Game::Gothic2;
fn entry(link: Link, parent: Option<usize>) -> HierarchyEntry {
    HierarchyEntry {
        object: link,
        parent,
        children_name: None,
    }
}
fn null_tree() -> Hierarchy {
    Hierarchy {
        root_count_name: None,
        entries: vec![entry(Link::new(b"n".to_vec(), None), None)],
    }
}
fn fixture(format: Format, game: Game, save: bool) -> Vec<u8> {
    let encoding = match format {
        Format::Ascii => "ascii",
        Format::Binary => "binary",
        Format::BinSafe => "bin_safe",
    };
    let game = if game == Game::Gothic1 { 1 } else { 2 };
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "tests/fixtures/synthetic-vob-tree-{encoding}-g{game}-{}.zen",
            if save { "save" } else { "normal" }
        )),
    )
    .unwrap()
}
fn null_bytes(format: Format, counts: &[i32], count_name: &[u8]) -> Vec<u8> {
    let mut w = Writer::new(Header::new(format), archive::Limits::default()).unwrap();
    for count in counts {
        w.begin_object(b"n", b"%", 0).unwrap();
        w.end_object().unwrap();
        w.write(count_name, Value::Int(*count)).unwrap();
    }
    w.finish().unwrap()
}

#[test]
fn independent_trees_share_definitions_with_objects_and_keep_custom_fields() {
    for format in FORMATS {
        for game in [Game::Gothic1, GAME] {
            for save in [false, true] {
                let bytes = fixture(format, game, save);
                let mut s = ReadSession::new(&bytes, game, Limits::default()).unwrap();
                assert_eq!(s.metadata().save_game, save);
                assert_eq!(s.with_fields(|r| r.read_int()).unwrap(), -701);
                let seed = s.read_object().unwrap().unwrap();
                let first = s.read_vob_tree().unwrap();
                assert_eq!(s.with_fields(|r| r.read_int()).unwrap(), 812);
                let second = s.read_vob_tree().unwrap();
                let alias = s.read_object().unwrap().unwrap();
                assert_eq!(s.with_fields(|r| r.read_int()).unwrap(), -913);
                assert_eq!(
                    first.entries.iter().map(|e| e.parent).collect::<Vec<_>>(),
                    [None, Some(0), Some(0), Some(2)]
                );
                assert_eq!(
                    second.entries.iter().map(|e| e.parent).collect::<Vec<_>>(),
                    [None, Some(0)]
                );
                for e in [&first.entries[1], &first.entries[3], &second.entries[1]] {
                    assert_eq!(e.object.target, seed.target);
                }
                assert_eq!(alias.target, seed.target);
                for tree in [&first, &second] {
                    assert!(tree.root_count_name.is_none());
                    for (i, e) in tree.entries.iter().enumerate() {
                        assert_eq!(
                            e.children_name,
                            (format != Format::Binary).then(|| format!("childs{i}").into_bytes())
                        );
                    }
                }
                let graph = s.finish().unwrap();
                assert!(graph.roots().is_empty());
                assert_eq!(graph.len(), 5);
                let ObjectRef::Vob(leaf) = graph.get(seed.target.unwrap()).unwrap() else {
                    panic!()
                };
                assert_eq!(leaf.base().position.to_array(), [7.25, -8.5, 9.75]);
                assert_eq!(leaf.base().state.is_some(), save);
            }
        }
    }
}

#[test]
fn occurrence_topology_retains_null_subtrees_and_aliases_with_different_children() {
    for format in FORMATS {
        let mut graph = Graph::new().unwrap();
        let id = graph.insert(Object::Vob(Vob::new(GAME))).unwrap();
        let link = Link::new(b"v".to_vec(), Some(id));
        let mut tree = null_tree();
        tree.entries.extend([
            entry(link.clone(), Some(0)),
            entry(link.clone(), Some(1)),
            entry(link.clone(), Some(0)),
        ]);
        tree.entries[0].children_name = Some(b"childs0007".to_vec());
        let mut s =
            WriteSession::new(&Metadata::new(format), GAME, &graph, Limits::default()).unwrap();
        s.write_vob_tree(&tree).unwrap();
        s.write_object(&Link::new(b"temporary".to_vec(), Some(id)))
            .unwrap();
        let bytes = s.finish().unwrap();
        let mut s = ReadSession::new(&bytes, GAME, Limits::default()).unwrap();
        let got = s.read_vob_tree().unwrap();
        let after = s.read_object().unwrap().unwrap();
        assert!(got.entries[0].object.target.is_none());
        assert_eq!(
            got.entries.iter().map(|e| e.parent).collect::<Vec<_>>(),
            [None, Some(0), Some(1), Some(0)]
        );
        for e in &got.entries[1..] {
            assert_eq!(e.object.target, after.target);
        }
        assert_eq!(
            got.entries[0].children_name,
            (format != Format::Binary).then(|| b"childs0007".to_vec())
        );
        assert_eq!(s.finish().unwrap().len(), 1);
    }
}

#[test]
fn graph_cycles_remain_separate_from_the_finite_tree() {
    let mut graph = Graph::new().unwrap();
    let id = graph.insert(Object::Vob(Vob::new(GAME))).unwrap();
    let ai = graph
        .insert(Object::AiMove(AiMove {
            vob: Link::new(b"vob".to_vec(), Some(id)),
            owner: Link::new(b"owner".to_vec(), None),
        }))
        .unwrap();
    let ObjectMut::Vob(mut v) = graph.get_mut(id).unwrap() else {
        panic!()
    };
    v.base_mut().ai = Some(Link::new(b"ai".to_vec(), Some(ai)));
    let tree = Hierarchy {
        root_count_name: None,
        entries: vec![entry(Link::new(b"v".to_vec(), Some(id)), None)],
    };
    for format in FORMATS {
        let mut s =
            WriteSession::new(&Metadata::new(format), GAME, &graph, Limits::default()).unwrap();
        s.write_vob_tree(&tree).unwrap();
        s.write_vob_tree(&tree).unwrap();
        let bytes = s.finish().unwrap();
        let mut s = ReadSession::new(&bytes, GAME, Limits::default()).unwrap();
        let a = s.read_vob_tree().unwrap();
        let b = s.read_vob_tree().unwrap();
        assert_eq!(a.entries[0].object.target, b.entries[0].object.target);
        let graph = s.finish().unwrap();
        let ObjectRef::Vob(v) = graph.get(a.entries[0].object.target.unwrap()).unwrap() else {
            panic!()
        };
        let ObjectRef::AiMove(ai) = graph
            .get(v.base().ai.as_ref().unwrap().target.unwrap())
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(ai.vob.target, a.entries[0].object.target);
    }
}

#[test]
fn already_open_archives_return_at_the_same_frame_for_a_suffix() {
    let graph = Graph::new().unwrap();
    for format in FORMATS {
        let limits = Limits::default();
        let mut w = Writer::new(Header::new(format), limits.archive).unwrap();
        w.begin_object(b"outer", b"Application", 17).unwrap();
        w.write(b"before", Value::Int(11)).unwrap();
        let mut s = WriteSession::from_writer(w, GAME, &graph, limits).unwrap();
        s.write_vob_tree(&null_tree()).unwrap();
        let mut w = s.into_writer().unwrap();
        assert_eq!(w.depth(), 1);
        w.write(b"after", Value::Int(23)).unwrap();
        w.end_object().unwrap();
        let bytes = w.finish().unwrap();
        let mut r = Reader::new(&bytes, limits.archive).unwrap();
        assert_eq!(r.begin_object().unwrap().unwrap().version, 17);
        assert_eq!(r.read_int().unwrap(), 11);
        let mut s = ReadSession::from_reader(r, GAME, limits).unwrap();
        assert!(
            s.read_vob_tree().unwrap().entries[0]
                .object
                .target
                .is_none()
        );
        let (mut r, graph) = s.into_parts().unwrap();
        assert!(graph.is_empty());
        assert_eq!(r.depth(), 1);
        assert_eq!(r.read_int().unwrap(), 23);
        assert!(r.end_object().unwrap());
        r.finish().unwrap();
    }
}

#[test]
fn embedded_archives_keep_nonzero_physical_offsets_and_remote_tables() {
    for format in FORMATS {
        let origin = 8192usize;
        let prefix = 19usize;
        let mut bytes = null_bytes(format, &[0], b"childs0");
        let table = if format == Format::BinSafe {
            let at = bytes.windows(5).position(|v| v == b"\nEND\n").unwrap() + 5 + 8;
            let table = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
            bytes[at..at + 4].copy_from_slice(&((table + origin + prefix) as u32).to_le_bytes());
            Some(table)
        } else {
            None
        };
        let end = origin + prefix + table.unwrap_or(bytes.len());
        let mut file = vec![0x77; prefix];
        file.extend(bytes);
        file.extend(b"outside suffix");
        let space = AddressSpace::new(&file, origin).unwrap();
        let reader = || {
            Reader::embedded(
                space,
                origin + prefix..end,
                AsciiSeparator::Optional,
                archive::Limits::default(),
                &mut TableBudget::new(100, 10000),
            )
            .unwrap()
        };
        let mut s = ReadSession::from_reader(reader(), GAME, Limits::default()).unwrap();
        s.read_vob_tree().unwrap();
        let (r, _) = s.into_parts().unwrap();
        assert_eq!(r.position(), end);
        r.finish().unwrap();
        let e = ReadSession::from_reader(
            reader(),
            GAME,
            Limits {
                copied_bytes: 0,
                ..Limits::default()
            },
        )
        .unwrap_err();
        assert_eq!(e.kind, ErrorKind::LimitExceeded);
        let archiver = Header::new(format).archiver;
        let p = file
            .windows(archiver.len())
            .position(|s| s == archiver)
            .unwrap();
        assert_eq!(e.offset, Some(origin + p));
    }
}

#[test]
fn root_link_and_copied_budgets_accumulate_across_calls() {
    let graph = Graph::new().unwrap();
    for format in FORMATS {
        let bytes = null_bytes(format, &[0, 0], b"childs0");
        let metadata = Metadata::new(format);
        for limits in [
            Limits {
                roots: 1,
                ..Limits::default()
            },
            Limits {
                links: 1,
                ..Limits::default()
            },
            Limits {
                copied_bytes: metadata.archiver.len()
                    + 1
                    + if format == Format::Binary { 0 } else { 7 },
                ..Limits::default()
            },
        ] {
            let mut s = ReadSession::new(&bytes, GAME, limits).unwrap();
            s.read_vob_tree().unwrap();
            assert_eq!(
                s.read_vob_tree().unwrap_err().kind,
                ErrorKind::LimitExceeded
            );
            assert!(s.graph().is_err());
            assert!(s.into_parts().is_err());
        }
        for limits in [
            Limits {
                roots: 1,
                ..Limits::default()
            },
            Limits {
                links: 1,
                ..Limits::default()
            },
            Limits {
                copied_bytes: metadata.archiver.len() + 1,
                ..Limits::default()
            },
        ] {
            let mut s = WriteSession::new(&metadata, GAME, &graph, limits).unwrap();
            s.write_vob_tree(&null_tree()).unwrap();
            assert_eq!(
                s.write_vob_tree(&null_tree()).unwrap_err().kind,
                ErrorKind::LimitExceeded
            );
            assert!(s.into_writer().is_err());
        }
    }
}

#[test]
fn pending_siblings_are_charged_before_another_tree_occurrence_is_read() {
    for format in FORMATS {
        // At entry 1, its two children plus its pending sibling need three slots;
        // only two remain. There are deliberately no child records to consume.
        let bytes = null_bytes(format, &[2, 2], b"childs0");
        let mut s = ReadSession::new(
            &bytes,
            GAME,
            Limits {
                links: 4,
                ..Limits::default()
            },
        )
        .unwrap();
        assert_eq!(
            s.read_vob_tree().unwrap_err().kind,
            ErrorKind::LimitExceeded
        );
        assert!(s.finish().is_err());
    }
}

#[test]
fn archive_counters_survive_transfer_and_limit_configurations_must_match() {
    let graph = Graph::new().unwrap();
    for format in FORMATS {
        let limits = Limits {
            archive: archive::Limits {
                objects: 1,
                ..archive::Limits::default()
            },
            ..Limits::default()
        };
        let bytes = null_bytes(format, &[0, 0], b"childs0");
        let mut r = Reader::new(&bytes, limits.archive).unwrap();
        r.begin_object().unwrap().unwrap();
        assert!(r.end_object().unwrap());
        r.read_int().unwrap();
        let mut s = ReadSession::from_reader(r, GAME, limits).unwrap();
        assert_eq!(
            s.read_vob_tree().unwrap_err().kind,
            ErrorKind::LimitExceeded
        );
        let mut w = Writer::new(Header::new(format), limits.archive).unwrap();
        w.begin_object(b"n", b"%", 0).unwrap();
        w.end_object().unwrap();
        let mut s = WriteSession::from_writer(w, GAME, &graph, limits).unwrap();
        assert_eq!(
            s.write_vob_tree(&null_tree()).unwrap_err().kind,
            ErrorKind::LimitExceeded
        );
        let r = Reader::new(&bytes, limits.archive).unwrap();
        assert!(ReadSession::from_reader(r, GAME, Limits::default()).is_err());
        let w = Writer::new(Header::new(format), limits.archive).unwrap();
        assert!(WriteSession::from_writer(w, GAME, &graph, Limits::default()).is_err());
    }
}

#[test]
fn errors_and_callback_unwind_poison_all_further_use() {
    let graph = Graph::new().unwrap();
    for panic in [false, true] {
        for extract in [false, true] {
            let bytes = null_bytes(Format::Ascii, &[0], b"childs0");
            let mut s = ReadSession::new(&bytes, GAME, Limits::default()).unwrap();
            let _ = catch_unwind(AssertUnwindSafe(|| {
                s.with_fields::<()>(|_| {
                    if panic {
                        panic!("application callback")
                    }
                    Err(Error::operation(
                        ErrorKind::InvalidData,
                        "application callback",
                    ))
                })
            }));
            assert!(s.read_object().is_err());
            assert!(s.read_vob_tree().is_err());
            assert!(s.graph().is_err());
            assert!(s.with_fields(|_| Ok(())).is_err());
            if extract {
                assert!(s.into_parts().is_err());
            } else {
                assert!(s.finish().is_err());
            }
            let mut s = WriteSession::new(
                &Metadata::new(Format::Ascii),
                GAME,
                &graph,
                Limits::default(),
            )
            .unwrap();
            let _ = catch_unwind(AssertUnwindSafe(|| {
                s.with_fields::<()>(|_| {
                    if panic {
                        panic!("application callback")
                    }
                    Err(Error::operation(
                        ErrorKind::InvalidData,
                        "application callback",
                    ))
                })
            }));
            assert!(s.write_object(&Link::new(vec![], None)).is_err());
            assert!(s.write_vob_tree(&null_tree()).is_err());
            assert!(s.with_fields(|_| Ok(())).is_err());
            if extract {
                assert!(s.into_writer().is_err());
            } else {
                assert!(s.finish().is_err());
            }
        }
    }
}

#[test]
fn malformed_tree_counts_and_missing_children_never_escape_a_session() {
    for format in FORMATS {
        for (counts, kind) in [
            (vec![-1], ErrorKind::InvalidData),
            (vec![i32::MAX], ErrorKind::LimitExceeded),
            (vec![1], ErrorKind::InvalidData),
        ] {
            let bytes = null_bytes(format, &counts, b"childs0");
            let mut s = ReadSession::new(&bytes, GAME, Limits::default()).unwrap();
            assert_eq!(s.read_vob_tree().unwrap_err().kind, kind);
            assert!(s.into_parts().is_err());
        }
        if format != Format::Binary {
            let bytes = null_bytes(format, &[0], b"notChildren");
            let mut s = ReadSession::new(&bytes, GAME, Limits::default()).unwrap();
            assert_eq!(s.read_vob_tree().unwrap_err().kind, ErrorKind::InvalidData);
        }
    }
}

#[test]
fn writer_rejects_non_trees_foreign_handles_and_non_vob_targets() {
    let mut graph = Graph::new().unwrap();
    let visual = graph
        .insert(Object::Visual(Visual::new(VisualKind::Mesh)))
        .unwrap();
    let mut other = Graph::new().unwrap();
    let foreign = other.insert(Object::Vob(Vob::new(GAME))).unwrap();
    let base = null_tree();
    let mut cases = vec![Hierarchy::default()];
    let mut t = base.clone();
    t.root_count_name = Some(b"childs0".to_vec());
    cases.push(t);
    let mut t = base.clone();
    t.entries.push(t.entries[0].clone());
    cases.push(t);
    let mut t = base.clone();
    t.entries[0].parent = Some(0);
    cases.push(t);
    let mut t = base.clone();
    t.entries.extend([
        entry(Link::new(vec![], None), Some(0)),
        entry(Link::new(vec![], None), Some(0)),
        entry(Link::new(vec![], None), Some(1)),
    ]);
    cases.push(t);
    let mut t = base.clone();
    t.entries[0].children_name = Some(b"childs-1".to_vec());
    cases.push(t);
    for id in [visual, foreign] {
        let mut t = base.clone();
        t.entries[0].object.target = Some(id);
        cases.push(t);
    }
    for tree in cases {
        let mut s = WriteSession::new(
            &Metadata::new(Format::Binary),
            GAME,
            &graph,
            Limits::default(),
        )
        .unwrap();
        assert!(s.write_vob_tree(&tree).is_err());
        assert!(s.finish().is_err());
    }
    for format in FORMATS {
        let mut s =
            WriteSession::new(&Metadata::new(format), GAME, &graph, Limits::default()).unwrap();
        s.write_object(&Link::new(b"bad".to_vec(), Some(visual)))
            .unwrap();
        s.with_fields(|w| w.write(b"childs0", Value::Int(0)))
            .unwrap();
        let bytes = s.finish().unwrap();
        let mut s = ReadSession::new(&bytes, GAME, Limits::default()).unwrap();
        assert_eq!(
            s.read_vob_tree().unwrap_err().kind,
            ErrorKind::WrongObjectType
        );
        assert!(s.into_parts().is_err());
    }
}

#[test]
fn source_graph_orphans_and_actual_save_header_are_validated() {
    let mut graph = Graph::new().unwrap();
    graph.insert(Object::Vob(Vob::new(Game::Gothic1))).unwrap();
    let mut h = Header::new(Format::Ascii);
    assert!(
        WriteSession::from_writer(
            Writer::new(h, archive::Limits::default()).unwrap(),
            GAME,
            &graph,
            Limits::default()
        )
        .is_err()
    );
    h.save_game = true;
    assert!(
        WriteSession::from_writer(
            Writer::new(h, archive::Limits::default()).unwrap(),
            Game::Gothic1,
            &graph,
            Limits::default()
        )
        .is_err()
    );
    let mut graph = Graph::new().unwrap();
    let mut vob = Vob::new(GAME);
    vob.base.physics_enabled = false;
    vob.base.state = Some(SaveState::default());
    let id = graph.insert(Object::Vob(vob)).unwrap();
    // Source roots are validated but not emitted, and their links count in addition
    // to explicit session occurrences.
    graph
        .add_root(Link::new(b"source".to_vec(), Some(id)))
        .unwrap();
    let limits = Limits {
        links: 1,
        ..Limits::default()
    };
    let mut s = WriteSession::from_writer(
        Writer::new(h, limits.archive).unwrap(),
        GAME,
        &graph,
        limits,
    )
    .unwrap();
    assert_eq!(
        s.write_object(&Link::new(vec![], Some(id)))
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    let s = WriteSession::from_writer(
        Writer::new(h, limits.archive).unwrap(),
        GAME,
        &graph,
        limits,
    )
    .unwrap();
    let bytes = s.finish().unwrap();
    let s = ReadSession::new(&bytes, GAME, Limits::default()).unwrap();
    assert!(s.finish().unwrap().is_empty());
}

#[test]
fn deep_tree_traversal_is_iterative_and_has_its_own_depth_limit() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let graph = Graph::new().unwrap();
            let count = 4096usize;
            let tree = Hierarchy {
                root_count_name: None,
                entries: (0..count)
                    .map(|i| entry(Link::new(b"n".to_vec(), None), i.checked_sub(1)))
                    .collect(),
            };
            let mut limits = Limits {
                links: count,
                ..Limits::default()
            };
            limits.archive.depth = 1;
            limits.world.hierarchy_depth = count;
            let mut s =
                WriteSession::new(&Metadata::new(Format::Binary), GAME, &graph, limits).unwrap();
            s.write_vob_tree(&tree).unwrap();
            let bytes = s.finish().unwrap();
            let mut s = ReadSession::new(&bytes, GAME, limits).unwrap();
            assert_eq!(s.read_vob_tree().unwrap().entries.len(), count);
            s.finish().unwrap();
            limits.world.hierarchy_depth -= 1;
            let mut s = ReadSession::new(&bytes, GAME, limits).unwrap();
            assert_eq!(
                s.read_vob_tree().unwrap_err().kind,
                ErrorKind::LimitExceeded
            );
            let mut s =
                WriteSession::new(&Metadata::new(Format::Binary), GAME, &graph, limits).unwrap();
            assert_eq!(
                s.write_vob_tree(&tree).unwrap_err().kind,
                ErrorKind::LimitExceeded
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn extraction_rejects_unbalanced_frames_and_tagged_peeking_does_not_charge_roots() {
    let graph = Graph::new().unwrap();
    for format in FORMATS {
        let mut s =
            WriteSession::new(&Metadata::new(format), GAME, &graph, Limits::default()).unwrap();
        s.with_fields(|w| w.begin_object(b"extra", b"Application", 0))
            .unwrap();
        assert!(s.into_writer().is_err());
        let mut w = Writer::new(Header::new(format), archive::Limits::default()).unwrap();
        w.begin_object(b"extra", b"Application", 0).unwrap();
        w.end_object().unwrap();
        let bytes = w.finish().unwrap();
        let mut s = ReadSession::new(&bytes, GAME, Limits::default()).unwrap();
        s.with_fields(|r| r.begin_object()).unwrap();
        assert!(s.into_parts().is_err());
    }
    for format in [Format::Ascii, Format::BinSafe] {
        let mut w = Writer::new(Header::new(format), archive::Limits::default()).unwrap();
        w.write(b"int", Value::Int(42)).unwrap();
        let bytes = w.finish().unwrap();
        let mut s = ReadSession::new(
            &bytes,
            GAME,
            Limits {
                roots: 0,
                ..Limits::default()
            },
        )
        .unwrap();
        assert!(s.read_object().unwrap().is_none());
        assert_eq!(s.with_fields(|r| r.read_int()).unwrap(), 42);
        s.finish().unwrap();
    }
}
