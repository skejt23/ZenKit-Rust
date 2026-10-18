use zenkit::{ErrorKind, GameVersion, archive::Format, objects::*, vob::*};
fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/synthetic-{name}.zen")),
    )
    .unwrap()
}
fn parse(name: &str, game: GameVersion) -> ObjectArchive {
    ObjectArchive::from_bytes(&bytes(name), game, Limits::default()).unwrap()
}
fn npc(d: &ObjectArchive) -> &NpcData {
    d.graph
        .objects()
        .find_map(|(_, o)| match o {
            ObjectRef::Vob(v) => match v.kind() {
                VobKindRef::Npc(n) => Some(n),
                _ => None,
            },
            _ => None,
        })
        .unwrap()
}
fn npc_id(d: &ObjectArchive) -> ObjectId {
    d.graph
        .objects()
        .find(|(_, o)| {
            matches!(
                o,
                ObjectRef::Vob(v) if matches!(v.kind(), VobKindRef::Npc(_))
            )
        })
        .unwrap()
        .0
}
fn npc_mut(d: &mut ObjectArchive) -> &mut NpcData {
    let id = npc_id(d);
    d.graph.npc_mut(id).unwrap()
}
fn reopen(d: &ObjectArchive) -> ObjectArchive {
    ObjectArchive::from_bytes(
        &d.to_bytes(Limits::default()).unwrap(),
        d.game,
        Limits::default(),
    )
    .unwrap()
}
fn canonical() -> ObjectArchive {
    parse(
        "vob-npc-bin_safe-g2-save-packed-canonical",
        GameVersion::Gothic2,
    )
}
#[test]
fn complete_phases_and_identity_in_all_twenty_four_modes() {
    for enc in ["ascii", "binary", "bin_safe"] {
        for (g, game) in [(1, GameVersion::Gothic1), (2, GameVersion::Gothic2)] {
            for save in [false, true] {
                for packed in [false, true] {
                    let d = parse(
                        &format!(
                            "vob-npc-{enc}-g{g}-{}-{}-canonical",
                            if save { "save" } else { "normal" },
                            if packed { "packed" } else { "unpacked" }
                        ),
                        game,
                    );
                    assert_eq!(d.graph.len(), 9);
                    let n = npc(&d);
                    assert_eq!(n.instance, b"NPC_INSTANCE\xff");
                    assert_eq!(
                        n.overlays,
                        [b"OVERLAY_A\xfe".to_vec(), b"OVERLAY_B\xfd".to_vec()]
                    );
                    assert_eq!(
                        (
                            n.flags,
                            n.guild,
                            n.guild_true,
                            n.level,
                            n.xp,
                            n.xp_next,
                            n.lp
                        ),
                        (0x12345, 7, 9, 23, 123456, 234567, -4)
                    );
                    assert_eq!(n.talents.len(), 4);
                    assert_eq!(n.talents[0].target, n.talents[3].target);
                    assert!(n.talents[1].target.is_none());
                    assert_eq!(n.talents[3].name, b"talentAlias");
                    assert_eq!((n.fight_tactic, n.fight_mode, n.mad_time), (2, 3, -17));
                    assert!(n.wounded && !n.mad && n.player);
                    assert_eq!(n.attributes, std::array::from_fn(|i| 10 + 3 * i as i32));
                    assert_eq!(n.missions, [-10, -9, -8, -7, -6]);
                    assert_eq!(n.version.script_vars().len(), if g == 1 { 50 } else { 100 });
                    assert_eq!(n.version.script_vars()[0], -400);
                    assert_eq!(
                        *n.version.script_vars().last().unwrap(),
                        if g == 1 { 433 } else { 1283 }
                    );
                    assert_eq!(n.spells, [18, 128, 0, 255]);
                    assert_eq!(n.news.len(), 2);
                    assert_eq!(n.news[0].witness_name, b"WITNESS\xfa");
                    assert_eq!(n.news[1].spread_time, -0.25);
                    assert_eq!(n.carry.target, n.items[0].item.target);
                    assert_eq!(n.enemy.target, Some(npc_id(&d)));
                    assert!(n.move_lock);
                    assert_eq!(n.items[0].item.target, n.items[2].item.target);
                    assert_eq!(n.items[0].short_key, save.then_some(7));
                    assert_eq!(n.items[2].short_key, save.then_some(-3));
                    assert!(n.slots[0].entry.is_none());
                    assert_eq!(
                        n.slots[1].entry.as_ref().unwrap().item.target,
                        n.items[1].item.target
                    );
                    assert!(n.slots[1].entry.as_ref().unwrap().in_inventory);
                    assert!(n.slots[2].entry.as_ref().unwrap().item.target.is_none());
                    assert_eq!(n.current_state.name, b"ZS_CURRENT\xf4");
                    assert_eq!(n.next_state.index, 19);
                    assert_eq!(n.current_routine, b"ROUTINE\xf2");
                    assert_eq!(n.respawn_time, 7654);
                    assert_eq!(n.protection, [-30, -19, -8, 3, 14, 25, 36, 47]);
                    if let NpcVersion::Gothic2 {
                        hit_chance,
                        packed,
                        interruptable_override,
                        npc_type,
                        spell_mana,
                        ..
                    } = &n.version
                    {
                        assert_eq!(*hit_chance, [11, 22, 33, 44]);
                        assert_eq!(packed.fields().unwrap()[8], b"PACK8");
                        assert_eq!(
                            (*interruptable_override, *npc_type, *spell_mana),
                            (-2, 5, 31)
                        );
                    }
                    let wire = d.to_bytes(Limits::default()).unwrap();
                    let r = reopen(&d);
                    assert_eq!(wire, r.to_bytes(Limits::default()).unwrap());
                }
            }
        }
    }
}
#[test]
fn npc_identity_survives_typed_pool_growth() {
    assert!(std::mem::size_of::<ObjectRef<'_>>() <= 64);
    let mut data = NpcData::new(GameVersion::Gothic2);
    data.guild = -17;
    let mut graph = Graph::new().unwrap();
    let mut vob = Vob::new(GameVersion::Gothic2);
    vob.kind = VobKind::Npc(data);
    let id = graph.insert(Object::Vob(vob)).unwrap();
    for _ in 0..100 {
        let mut value = Vob::new(GameVersion::Gothic2);
        value.kind = VobKind::Npc(NpcData::new(GameVersion::Gothic2));
        graph.insert(Object::Vob(value)).unwrap();
    }
    let n = graph.npc(id).unwrap();
    assert_eq!(n.guild, -17);
}
#[test]
fn packed_text_preserves_malformed_bytes_and_has_checked_edit_path() {
    for raw in [b"".as_slice(), b"ONE;TWO", b";;;;;;;;;"] {
        let value = NpcPacked::from_bytes(raw.to_vec());
        assert_eq!(value.as_bytes(), raw);
        assert!(value.fields().is_err());
    }
    let fields = [b"".as_slice(); 9];
    assert_eq!(
        NpcPacked::from_fields(fields, 7).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    let n = NpcPacked::from_fields(fields, 8).unwrap();
    assert_eq!(n.as_bytes(), b";;;;;;;;");
    assert_eq!(n.fields().unwrap(), fields);
    let mut fields = fields;
    fields[4] = b"a;b";
    assert_eq!(
        NpcPacked::from_fields(fields, 100).unwrap_err().kind,
        ErrorKind::InvalidData
    );
    let mut d = canonical();
    let NpcVersion::Gothic2 { packed, .. } = &mut npc_mut(&mut d).version else {
        panic!()
    };
    *packed = NpcPacked::from_bytes(b"malformed;\xff".to_vec());
    let r = reopen(&d);
    let NpcVersion::Gothic2 { packed, .. } = &npc(&r).version else {
        panic!()
    };
    assert_eq!(packed.as_bytes(), b"malformed;\xff");
    assert!(packed.fields().is_err());
}
#[test]
fn ancestor_item_dependencies_roundtrip_without_reordering_in_all_encodings() {
    for enc in ["ascii", "bin_safe"] {
        for (g, game) in [(1, GameVersion::Gothic1), (2, GameVersion::Gothic2)] {
            for key in ["ancestor-key", "ancestor-no-key"] {
                let mut d = parse(&format!("vob-npc-{enc}-g{g}-{key}"), game);
                let r = reopen(&d);
                assert_eq!(
                    npc(&r).items[0].short_key,
                    if key == "ancestor-key" { Some(7) } else { None }
                );
                d.metadata.format = Format::Binary;
                let r = reopen(&d);
                assert_eq!(npc(&r).items[0].short_key, npc(&d).items[0].short_key);
                assert_eq!(r.graph.roots()[0].name, b"ancestorItem");
                assert_eq!(npc(&r).items[0].item.target, r.graph.roots()[0].target);
            }
        }
    }
    for game in [1, 2] {
        for name in ["ancestor-key", "ancestor-no-key"] {
            let b = bytes(&format!("vob-npc-unsupported-binary-g{game}-{name}"));
            let d = ObjectArchive::from_bytes(
                &b,
                if game == 1 {
                    GameVersion::Gothic1
                } else {
                    GameVersion::Gothic2
                },
                Limits::default(),
            )
            .unwrap();
            let r = reopen(&d);
            assert_eq!(
                npc(&r).items[0].short_key,
                if name == "ancestor-key" {
                    Some(7)
                } else {
                    None
                }
            );
            assert_eq!(npc(&r).items[0].item.target, r.graph.roots()[0].target);
        }
    }
}
#[test]
fn nested_binary_items_keep_definition_flags_and_occurrence_keys_separate() {
    for (g, game) in [(1, GameVersion::Gothic1), (2, GameVersion::Gothic2)] {
        let d = parse(&format!("vob-npc-binary-g{g}-ancestor-nested"), game);
        let wire = d.to_bytes(Limits::default()).unwrap();
        let r = reopen(&d);
        assert_eq!(wire, r.to_bytes(Limits::default()).unwrap());
        let nested = r
            .graph
            .objects()
            .find_map(|(_, object)| match object {
                ObjectRef::Vob(v) => match v.kind() {
                    VobKindRef::Npc(n) if n.instance == b"NESTED_NPC" => Some(n),
                    _ => None,
                },
                _ => None,
            })
            .unwrap();
        assert_eq!(
            nested.items.iter().map(|v| v.short_key).collect::<Vec<_>>(),
            [Some(-17), None, Some(44), None]
        );
        assert_eq!(nested.items[0].item.target, r.graph.roots()[0].target);
        assert_eq!(nested.items[0].item.target, nested.items[2].item.target);
        assert_eq!(nested.items[1].item.target, nested.items[3].item.target);
        assert_ne!(nested.items[0].item.target, nested.items[1].item.target);
        for (index, expected) in [(0, 0x200), (1, 0x402)] {
            let item = r
                .graph
                .item(nested.items[index].item.target.unwrap())
                .unwrap();
            assert_eq!(item.state.unwrap().flags, expected);
        }
    }
}

#[test]
fn binary_item_suffix_lookahead_does_not_accept_extra_or_truncated_payload() {
    for (g, game) in [(1, GameVersion::Gothic1), (2, GameVersion::Gothic2)] {
        let wire = bytes(&format!("vob-npc-unsupported-binary-g{g}-ancestor-key"));
        let mut framing = zenkit::archive::Reader::new(&wire, Default::default()).unwrap();
        let start = framing.position();
        let header = framing.begin_object().unwrap().unwrap();
        assert_eq!(header.class_name, b"oCItem:zCVob");
        let body = framing.position();
        let length = u32::from_le_bytes(wire[start..start + 4].try_into().unwrap()) as usize;
        let end = start + length;
        let flags = &wire[end - 4..end];
        assert_eq!(i32::from_le_bytes(flags.try_into().unwrap()), 0x200);
        for cut in [body + 7, end - 1] {
            assert!(ObjectArchive::from_bytes(&wire[..cut], game, Limits::default()).is_err());
        }
        for declared in [body - start + 7, length - 1, length + 1] {
            let mut changed = wire.clone();
            changed[start..start + 4].copy_from_slice(&(declared as u32).to_le_bytes());
            assert!(ObjectArchive::from_bytes(&changed, game, Limits::default()).is_err());
        }
        let mut extra = wire.clone();
        extra.splice(end..end, flags.iter().copied());
        extra[start..start + 4].copy_from_slice(&((length + 4) as u32).to_le_bytes());
        let error = ObjectArchive::from_bytes(&extra, game, Limits::default()).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unsupported);
        assert_eq!(error.context, "extra object payload");
    }
}

#[test]
fn malformed_counts_fields_types_and_deferred_keys_fail() {
    for file in
        std::fs::read_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
            .unwrap()
    {
        let file = file.unwrap();
        let name = file.file_name().to_string_lossy().into_owned();
        if name.starts_with("synthetic-vob-npc-invalid-") {
            let b = std::fs::read(file.path()).unwrap();
            let e =
                ObjectArchive::from_bytes(&b, GameVersion::Gothic2, Limits::default()).unwrap_err();
            assert!(e.offset.is_some(), "{name}: {e:?}");
        }
    }
}
#[test]
fn raw_field_tails_survive_tagged_transport_and_refuse_binary() {
    for enc in ["ascii", "bin_safe"] {
        let mut d = parse(&format!("vob-npc-{enc}-raw"), GameVersion::Gothic2);
        let n = npc(&d);
        assert_eq!(n.script_vars_tail, [0x91, 0x82, 0x73]);
        assert_eq!(n.spells_tail, [0xaa, 0, 0xbb]);
        assert_eq!(n.protection_tail, [0xcc, 0xdd]);
        let r = reopen(&d);
        assert_eq!(npc(&r).protection_tail, n.protection_tail);
        d.metadata.format = Format::Binary;
        assert_eq!(
            d.to_bytes(Limits::default()).unwrap_err().kind,
            ErrorKind::Unsupported
        );
    }
}
#[test]
fn exact_sequence_budget_includes_orphan_descriptors_but_not_links() {
    let mut d = canonical();
    let limits = Limits {
        sequence_items: 7,
        ..Limits::default()
    };
    d.validate(limits).unwrap();
    let wire = d.to_bytes(limits).unwrap();
    ObjectArchive::from_bytes(&wire, d.game, limits).unwrap();
    let low = Limits {
        sequence_items: 6,
        ..limits
    };
    assert_eq!(d.validate(low).unwrap_err().kind, ErrorKind::LimitExceeded);
    assert_eq!(
        ObjectArchive::from_bytes(&wire, d.game, low)
            .unwrap_err()
            .kind,
        ErrorKind::LimitExceeded
    );
    let mut orphan = Vob::new(d.game);
    let mut n = NpcData::new(d.game);
    n.slots.push(NpcSlot::default());
    orphan.kind = VobKind::Npc(n);
    orphan.base.physics_enabled = false;
    orphan.base.state = Some(SaveState::default());
    d.graph.insert(Object::Vob(orphan)).unwrap();
    assert_eq!(
        d.validate(limits).unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
}
fn first_cap(mut check: impl FnMut(usize) -> bool) -> usize {
    let (mut low, mut high) = (0, 1);
    while !check(high) {
        high *= 2;
        assert!(high < 1_000_000);
    }
    while low < high {
        let mid = low + (high - low) / 2;
        if check(mid) {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    low
}
#[test]
fn copied_and_link_budgets_match_between_read_validate_and_write() {
    let d = canonical();
    let wire = d.to_bytes(Limits::default()).unwrap();
    let copy_cap = first_cap(|copied_bytes| {
        d.validate(Limits {
            copied_bytes,
            ..Limits::default()
        })
        .is_ok()
    });
    let read_cap = first_cap(|copied_bytes| {
        ObjectArchive::from_bytes(
            &wire,
            d.game,
            Limits {
                copied_bytes,
                ..Limits::default()
            },
        )
        .is_ok()
    });
    assert_eq!(copy_cap, read_cap);
    d.to_bytes(Limits {
        copied_bytes: copy_cap,
        ..Limits::default()
    })
    .unwrap();
    let link_cap = first_cap(|links| {
        d.validate(Limits {
            links,
            ..Limits::default()
        })
        .is_ok()
    });
    let read_cap = first_cap(|links| {
        ObjectArchive::from_bytes(
            &wire,
            d.game,
            Limits {
                links,
                ..Limits::default()
            },
        )
        .is_ok()
    });
    assert_eq!(link_cap, read_cap);
    d.to_bytes(Limits {
        links: link_cap,
        ..Limits::default()
    })
    .unwrap();
}
#[test]
fn item_shortcut_validation_is_per_occurrence_and_covers_orphans() {
    let mut d = canonical();
    npc_mut(&mut d).items[0].short_key = None;
    assert_eq!(
        d.validate(Limits::default()).unwrap_err().kind,
        ErrorKind::InvalidData
    );
    npc_mut(&mut d).items[0].short_key = Some(-999);
    d.validate(Limits::default()).unwrap();
    assert_eq!(npc(&reopen(&d)).items[0].short_key, Some(-999));
    let mut foreign = Graph::new().unwrap();
    let id = foreign
        .insert(Object::NpcTalent(NpcTalent::default()))
        .unwrap();
    npc_mut(&mut d).items[0].item.target = Some(id);
    assert_eq!(
        d.validate(Limits::default()).unwrap_err().kind,
        ErrorKind::InvalidHandle
    );
    let root = npc_id(&d);
    npc_mut(&mut d).items[0].item.target = Some(root);
    assert_eq!(
        d.validate(Limits::default()).unwrap_err().kind,
        ErrorKind::WrongObjectType
    );
}
#[test]
fn null_inventory_and_used_null_slot_remain_distinct() {
    let mut d = canonical();
    npc_mut(&mut d).items.insert(
        1,
        NpcItem {
            item: Link::new(b"null".to_vec(), None),
            short_key: None,
        },
    );
    let r = reopen(&d);
    assert_eq!(npc(&r).items.len(), 4);
    assert!(npc(&r).items[1].item.target.is_none());
    assert!(npc(&r).slots[0].entry.is_none());
    assert!(
        npc(&r).slots[2]
            .entry
            .as_ref()
            .unwrap()
            .item
            .target
            .is_none()
    );
    npc_mut(&mut d).items[1].short_key = Some(0);
    assert!(d.validate(Limits::default()).is_err());
}
#[test]
fn fixed_talent_is_dispatchable_without_npc_and_wrong_game_is_rejected() {
    let mut d = canonical();
    let talent = d
        .graph
        .insert(Object::NpcTalent(NpcTalent {
            talent: -1,
            value: i32::MIN,
            skill: i32::MAX,
        }))
        .unwrap();
    d.graph
        .add_root(Link::new(b"rawTalent".to_vec(), Some(talent)))
        .unwrap();
    let r = reopen(&d);
    let ObjectRef::NpcTalent(t) = r
        .graph
        .get(r.graph.roots().last().unwrap().target.unwrap())
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        *t,
        NpcTalent {
            talent: -1,
            value: i32::MIN,
            skill: i32::MAX
        }
    );
    npc_mut(&mut d).version = NpcVersion::new(GameVersion::Gothic1);
    assert_eq!(
        d.validate(Limits::default()).unwrap_err().kind,
        ErrorKind::InvalidData
    );
}
#[test]
fn debug_output_does_not_expand_arbitrary_owned_payloads() {
    let mut n = NpcData::new(GameVersion::Gothic2);
    n.instance = vec![255; 10000];
    n.news.push(NpcNews {
        witness_name: vec![255; 10000],
        ..Default::default()
    });
    assert!(format!("{n:?}").len() < 500);
    assert!(format!("{:?}", NpcPacked::from_bytes(vec![255; 10000])).len() < 80);
}
