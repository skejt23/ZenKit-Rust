// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2022-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT

use super::*;

define_class!(GuildValues, b"C_GILVALUES", {
    0 => water_depth_knee: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.WATER_DEPTH_KNEE", Int, 66, None;
    1 => water_depth_chest: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.WATER_DEPTH_CHEST", Int, 66, None;
    2 => jumpup_height: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.JUMPUP_HEIGHT", Int, 66, None;
    3 => swim_time: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.SWIM_TIME", Int, 66, None;
    4 => dive_time: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.DIVE_TIME", Int, 66, None;
    5 => step_height: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.STEP_HEIGHT", Int, 66, None;
    6 => jumplow_height: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.JUMPLOW_HEIGHT", Int, 66, None;
    7 => jumpmid_height: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.JUMPMID_HEIGHT", Int, 66, None;
    8 => slide_angle: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.SLIDE_ANGLE", Int, 66, None;
    9 => slide_angle2: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.SLIDE_ANGLE2", Int, 66, None;
    10 => disable_autoroll: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.DISABLE_AUTOROLL", Int, 66, None;
    11 => surface_align: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.SURFACE_ALIGN", Int, 66, None;
    12 => climb_heading_angle: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.CLIMB_HEADING_ANGLE", Int, 66, None;
    13 => climb_horiz_angle: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.CLIMB_HORIZ_ANGLE", Int, 66, None;
    14 => climb_ground_angle: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.CLIMB_GROUND_ANGLE", Int, 66, None;
    15 => fight_range_base: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.FIGHT_RANGE_BASE", Int, 66, None;
    16 => fight_range_fist: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.FIGHT_RANGE_FIST", Int, 66, None;
    17 => fight_range_1hs: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.FIGHT_RANGE_1HS", Int, 66, None;
    18 => fight_range_1ha: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.FIGHT_RANGE_1HA", Int, 66, None;
    19 => fight_range_2hs: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.FIGHT_RANGE_2HS", Int, 66, None;
    20 => fight_range_2ha: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.FIGHT_RANGE_2HA", Int, 66, None;
    21 => falldown_height: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.FALLDOWN_HEIGHT", Int, 66, None;
    22 => falldown_damage: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.FALLDOWN_DAMAGE", Int, 66, None;
    23 => blood_disabled: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.BLOOD_DISABLED", Int, 66, None;
    24 => blood_max_distance: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.BLOOD_MAX_DISTANCE", Int, 66, None;
    25 => blood_amount: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.BLOOD_AMOUNT", Int, 66, None;
    26 => blood_flow: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.BLOOD_FLOW", Int, 66, None;
    27 => blood_emitter: [Vec<u8>; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.BLOOD_EMITTER", String, 66, None;
    28 => blood_texture: [Vec<u8>; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.BLOOD_TEXTURE", String, 66, None;
    29 => turn_speed: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.TURN_SPEED", Int, 66, None;
    30 => fight_range_g: [i32; 66] = std::array::from_fn(|_| Default::default()), b"C_GILVALUES.FIGHT_RANGE_G", Int, 66, Some(b"C_GILVALUES.FIGHT_RANGE_G");
}, {});

define_class!(Npc, b"C_NPC", {
    0 => id: i32 = Default::default(), b"C_NPC.ID", Int, 1, None;
    1 => name: [Vec<u8>; 5] = std::array::from_fn(|_| Default::default()), b"C_NPC.NAME", String, 5, None;
    2 => slot: Vec<u8> = Default::default(), b"C_NPC.SLOT", String, 1, None;
    3 => r#type: NpcType = Default::default(), b"C_NPC.NPCTYPE", Int, 1, None;
    4 => flags: NpcFlag = Default::default(), b"C_NPC.FLAGS", Int, 1, None;
    5 => attribute: [i32; 8] = std::array::from_fn(|_| Default::default()), b"C_NPC.ATTRIBUTE", Int, 8, None;
    6 => protection: [i32; 8] = std::array::from_fn(|_| Default::default()), b"C_NPC.PROTECTION", Int, 8, None;
    7 => damage: [i32; 8] = std::array::from_fn(|_| Default::default()), b"C_NPC.DAMAGE", Int, 8, None;
    8 => damage_type: i32 = Default::default(), b"C_NPC.DAMAGETYPE", Int, 1, None;
    9 => guild: i32 = Default::default(), b"C_NPC.GUILD", Int, 1, None;
    10 => level: i32 = Default::default(), b"C_NPC.LEVEL", Int, 1, None;
    11 => mission: [i32; 5] = std::array::from_fn(|_| Default::default()), b"C_NPC.MISSION", Int, 5, None;
    12 => fight_tactic: i32 = Default::default(), b"C_NPC.FIGHT_TACTIC", Int, 1, None;
    13 => weapon: i32 = Default::default(), b"C_NPC.WEAPON", Int, 1, None;
    14 => voice: i32 = Default::default(), b"C_NPC.VOICE", Int, 1, None;
    15 => voice_pitch: i32 = Default::default(), b"C_NPC.VOICEPITCH", Int, 1, None;
    16 => body_mass: i32 = Default::default(), b"C_NPC.BODYMASS", Int, 1, None;
    17 => daily_routine: i32 = Default::default(), b"C_NPC.DAILY_ROUTINE", Int, 1, None;
    18 => start_aistate: i32 = Default::default(), b"C_NPC.START_AISTATE", Int, 1, None;
    19 => spawnpoint: Vec<u8> = Default::default(), b"C_NPC.SPAWNPOINT", String, 1, None;
    20 => spawn_delay: i32 = Default::default(), b"C_NPC.SPAWNDELAY", Int, 1, None;
    21 => senses: i32 = Default::default(), b"C_NPC.SENSES", Int, 1, None;
    22 => senses_range: i32 = Default::default(), b"C_NPC.SENSES_RANGE", Int, 1, None;
    23 => aivar: [i32; 100] = std::array::from_fn(|_| Default::default()), b"C_NPC.AIVAR", Int, 100, None;
    24 => wp: Vec<u8> = Default::default(), b"C_NPC.WP", String, 1, None;
    25 => exp: i32 = Default::default(), b"C_NPC.EXP", Int, 1, None;
    26 => exp_next: i32 = Default::default(), b"C_NPC.EXP_NEXT", Int, 1, None;
    27 => lp: i32 = Default::default(), b"C_NPC.LP", Int, 1, None;
    28 => hitchance: [i32; 5] = std::array::from_fn(|_| Default::default()), b"C_NPC.HITCHANCE", Int, 5, Some(b"C_NPC.HITCHANCE");
    29 => bodystate_interruptable_override: i32 = Default::default(), b"C_NPC.BODYSTATEINTERRUPTABLEOVERRIDE", Int, 1, Some(b"C_NPC.HITCHANCE");
    30 => no_focus: i32 = Default::default(), b"C_NPC.NOFOCUS", Int, 1, Some(b"C_NPC.HITCHANCE");
    31 => effect: Vec<u8> = Default::default(), b"C_NPC.EFFECT", String, 1, Some(b"C_NPC.EFFECT");
}, {});

define_class!(Mission, b"C_MISSION", {
    0 => name: Vec<u8> = Default::default(), b"C_MISSION.NAME", String, 1, None;
    1 => description: Vec<u8> = Default::default(), b"C_MISSION.DESCRIPTION", String, 1, None;
    2 => duration: i32 = Default::default(), b"C_MISSION.DURATION", Int, 1, None;
    3 => important: i32 = Default::default(), b"C_MISSION.IMPORTANT", Int, 1, None;
    4 => offer_conditions: i32 = Default::default(), b"C_MISSION.OFFERCONDITIONS", Int, 1, None;
    5 => offer: i32 = Default::default(), b"C_MISSION.OFFER", Int, 1, None;
    6 => success_conditions: i32 = Default::default(), b"C_MISSION.SUCCESSCONDITIONS", Int, 1, None;
    7 => success: i32 = Default::default(), b"C_MISSION.SUCCESS", Int, 1, None;
    8 => failure_conditions: i32 = Default::default(), b"C_MISSION.FAILURECONDITIONS", Int, 1, None;
    9 => failure: i32 = Default::default(), b"C_MISSION.FAILURE", Int, 1, None;
    10 => obsolete_conditions: i32 = Default::default(), b"C_MISSION.OBSOLETECONDITIONS", Int, 1, None;
    11 => obsolete: i32 = Default::default(), b"C_MISSION.OBSOLETE", Int, 1, None;
    12 => running: i32 = Default::default(), b"C_MISSION.RUNNING", Int, 1, None;
}, {});

define_class!(Item, b"C_ITEM", {
    0 => id: i32 = Default::default(), b"C_ITEM.ID", Int, 1, None;
    1 => name: Vec<u8> = Default::default(), b"C_ITEM.NAME", String, 1, None;
    2 => name_id: Vec<u8> = Default::default(), b"C_ITEM.NAMEID", String, 1, None;
    3 => hp: i32 = Default::default(), b"C_ITEM.HP", Int, 1, None;
    4 => hp_max: i32 = Default::default(), b"C_ITEM.HP_MAX", Int, 1, None;
    5 => main_flag: i32 = Default::default(), b"C_ITEM.MAINFLAG", Int, 1, None;
    6 => flags: ItemFlag = Default::default(), b"C_ITEM.FLAGS", Int, 1, None;
    7 => weight: i32 = Default::default(), b"C_ITEM.WEIGHT", Int, 1, None;
    8 => value: i32 = Default::default(), b"C_ITEM.VALUE", Int, 1, None;
    9 => damage_type: i32 = Default::default(), b"C_ITEM.DAMAGETYPE", Int, 1, None;
    10 => damage_total: i32 = Default::default(), b"C_ITEM.DAMAGETOTAL", Int, 1, None;
    11 => damage: [i32; 8] = std::array::from_fn(|_| Default::default()), b"C_ITEM.DAMAGE", Int, 8, None;
    12 => wear: i32 = Default::default(), b"C_ITEM.WEAR", Int, 1, None;
    13 => protection: [i32; 8] = std::array::from_fn(|_| Default::default()), b"C_ITEM.PROTECTION", Int, 8, None;
    14 => nutrition: i32 = Default::default(), b"C_ITEM.NUTRITION", Int, 1, None;
    15 => cond_atr: [i32; 3] = std::array::from_fn(|_| Default::default()), b"C_ITEM.COND_ATR", Int, 3, None;
    16 => cond_value: [i32; 3] = std::array::from_fn(|_| Default::default()), b"C_ITEM.COND_VALUE", Int, 3, None;
    17 => change_atr: [i32; 3] = std::array::from_fn(|_| Default::default()), b"C_ITEM.CHANGE_ATR", Int, 3, None;
    18 => change_value: [i32; 3] = std::array::from_fn(|_| Default::default()), b"C_ITEM.CHANGE_VALUE", Int, 3, None;
    19 => magic: i32 = Default::default(), b"C_ITEM.MAGIC", Int, 1, None;
    20 => on_equip: i32 = Default::default(), b"C_ITEM.ON_EQUIP", Int, 1, None;
    21 => on_unequip: i32 = Default::default(), b"C_ITEM.ON_UNEQUIP", Int, 1, None;
    22 => on_state: [i32; 4] = std::array::from_fn(|_| Default::default()), b"C_ITEM.ON_STATE", Int, 4, None;
    23 => owner: i32 = Default::default(), b"C_ITEM.OWNER", Int, 1, None;
    24 => owner_guild: i32 = Default::default(), b"C_ITEM.OWNERGUILD", Int, 1, None;
    25 => disguise_guild: i32 = Default::default(), b"C_ITEM.DISGUISEGUILD", Int, 1, None;
    26 => visual: Vec<u8> = Default::default(), b"C_ITEM.VISUAL", String, 1, None;
    27 => visual_change: Vec<u8> = Default::default(), b"C_ITEM.VISUAL_CHANGE", String, 1, None;
    28 => visual_skin: i32 = Default::default(), b"C_ITEM.VISUAL_SKIN", Int, 1, None;
    29 => scheme_name: Vec<u8> = Default::default(), b"C_ITEM.SCEMENAME", String, 1, None;
    30 => material: i32 = Default::default(), b"C_ITEM.MATERIAL", Int, 1, None;
    31 => munition: i32 = Default::default(), b"C_ITEM.MUNITION", Int, 1, None;
    32 => spell: i32 = Default::default(), b"C_ITEM.SPELL", Int, 1, None;
    33 => range: i32 = Default::default(), b"C_ITEM.RANGE", Int, 1, None;
    34 => mag_circle: i32 = Default::default(), b"C_ITEM.MAG_CIRCLE", Int, 1, None;
    35 => description: Vec<u8> = Default::default(), b"C_ITEM.DESCRIPTION", String, 1, None;
    36 => text: [Vec<u8>; 6] = std::array::from_fn(|_| Default::default()), b"C_ITEM.TEXT", String, 6, None;
    37 => count: [i32; 6] = std::array::from_fn(|_| Default::default()), b"C_ITEM.COUNT", Int, 6, None;
    38 => inv_zbias: i32 = Default::default(), b"C_ITEM.INV_ZBIAS", Int, 1, Some(b"C_ITEM.INV_ZBIAS");
    39 => inv_rot_x: i32 = Default::default(), b"C_ITEM.INV_ROTX", Int, 1, Some(b"C_ITEM.INV_ZBIAS");
    40 => inv_rot_y: i32 = Default::default(), b"C_ITEM.INV_ROTY", Int, 1, Some(b"C_ITEM.INV_ZBIAS");
    41 => inv_rot_z: i32 = Default::default(), b"C_ITEM.INV_ROTZ", Int, 1, Some(b"C_ITEM.INV_ZBIAS");
    42 => inv_animate: i32 = Default::default(), b"C_ITEM.INV_ANIMATE", Int, 1, Some(b"C_ITEM.INV_ZBIAS");
    43 => effect: Vec<u8> = Default::default(), b"C_ITEM.EFFECT", String, 1, Some(b"C_ITEM.EFFECT");
}, {});

define_class!(Focus, b"C_FOCUS", {
    0 => npc_longrange: f32 = Default::default(), b"C_FOCUS.NPC_LONGRANGE", Float, 1, None;
    1 => npc_range1: f32 = Default::default(), b"C_FOCUS.NPC_RANGE1", Float, 1, None;
    2 => npc_range2: f32 = Default::default(), b"C_FOCUS.NPC_RANGE2", Float, 1, None;
    3 => npc_azi: f32 = Default::default(), b"C_FOCUS.NPC_AZI", Float, 1, None;
    4 => npc_elevdo: f32 = Default::default(), b"C_FOCUS.NPC_ELEVDO", Float, 1, None;
    5 => npc_elevup: f32 = Default::default(), b"C_FOCUS.NPC_ELEVUP", Float, 1, None;
    6 => npc_prio: i32 = Default::default(), b"C_FOCUS.NPC_PRIO", Int, 1, None;
    7 => item_range1: f32 = Default::default(), b"C_FOCUS.ITEM_RANGE1", Float, 1, None;
    8 => item_range2: f32 = Default::default(), b"C_FOCUS.ITEM_RANGE2", Float, 1, None;
    9 => item_azi: f32 = Default::default(), b"C_FOCUS.ITEM_AZI", Float, 1, None;
    10 => item_elevdo: f32 = Default::default(), b"C_FOCUS.ITEM_ELEVDO", Float, 1, None;
    11 => item_elevup: f32 = Default::default(), b"C_FOCUS.ITEM_ELEVUP", Float, 1, None;
    12 => item_prio: i32 = Default::default(), b"C_FOCUS.ITEM_PRIO", Int, 1, None;
    13 => mob_range1: f32 = Default::default(), b"C_FOCUS.MOB_RANGE1", Float, 1, None;
    14 => mob_range2: f32 = Default::default(), b"C_FOCUS.MOB_RANGE2", Float, 1, None;
    15 => mob_azi: f32 = Default::default(), b"C_FOCUS.MOB_AZI", Float, 1, None;
    16 => mob_elevdo: f32 = Default::default(), b"C_FOCUS.MOB_ELEVDO", Float, 1, None;
    17 => mob_elevup: f32 = Default::default(), b"C_FOCUS.MOB_ELEVUP", Float, 1, None;
    18 => mob_prio: i32 = Default::default(), b"C_FOCUS.MOB_PRIO", Int, 1, None;
}, {});

define_class!(Info, b"C_INFO", {
    0 => npc: i32 = Default::default(), b"C_INFO.NPC", Int, 1, None;
    1 => nr: i32 = Default::default(), b"C_INFO.NR", Int, 1, None;
    2 => important: i32 = Default::default(), b"C_INFO.IMPORTANT", Int, 1, None;
    3 => condition: i32 = Default::default(), b"C_INFO.CONDITION", Int, 1, None;
    4 => information: i32 = Default::default(), b"C_INFO.INFORMATION", Int, 1, None;
    5 => description: Vec<u8> = Default::default(), b"C_INFO.DESCRIPTION", String, 1, None;
    6 => trade: i32 = Default::default(), b"C_INFO.TRADE", Int, 1, None;
    7 => permanent: i32 = Default::default(), b"C_INFO.PERMANENT", Int, 1, None;
}, {choices: Vec<InfoChoice> = Vec::new();});

define_class!(ItemReact, b"C_ITEMREACT", {
    0 => npc: i32 = Default::default(), b"C_ITEMREACT.NPC", Int, 1, None;
    1 => trade_item: i32 = Default::default(), b"C_ITEMREACT.TRADE_ITEM", Int, 1, None;
    2 => trade_amount: i32 = Default::default(), b"C_ITEMREACT.TRADE_AMOUNT", Int, 1, None;
    3 => requested_cat: i32 = Default::default(), b"C_ITEMREACT.REQUESTED_CAT", Int, 1, None;
    4 => requested_item: i32 = Default::default(), b"C_ITEMREACT.REQUESTED_ITEM", Int, 1, None;
    5 => requested_amount: i32 = Default::default(), b"C_ITEMREACT.REQUESTED_AMOUNT", Int, 1, None;
    6 => reaction: i32 = Default::default(), b"C_ITEMREACT.REACTION", Int, 1, None;
}, {});

define_class!(Spell, b"C_SPELL", {
    0 => time_per_mana: f32 = Default::default(), b"C_SPELL.TIME_PER_MANA", Float, 1, None;
    1 => damage_per_level: i32 = Default::default(), b"C_SPELL.DAMAGE_PER_LEVEL", Int, 1, None;
    2 => damage_type: i32 = Default::default(), b"C_SPELL.DAMAGETYPE", Int, 1, None;
    3 => spell_type: i32 = Default::default(), b"C_SPELL.SPELLTYPE", Int, 1, None;
    4 => can_turn_during_invest: i32 = Default::default(), b"C_SPELL.CANTURNDURINGINVEST", Int, 1, None;
    5 => can_change_target_during_invest: i32 = Default::default(), b"C_SPELL.CANCHANGETARGETDURINGINVEST", Int, 1, None;
    6 => is_multi_effect: i32 = Default::default(), b"C_SPELL.ISMULTIEFFECT", Int, 1, None;
    7 => target_collect_algo: i32 = Default::default(), b"C_SPELL.TARGETCOLLECTALGO", Int, 1, None;
    8 => target_collect_type: i32 = Default::default(), b"C_SPELL.TARGETCOLLECTTYPE", Int, 1, None;
    9 => target_collect_range: i32 = Default::default(), b"C_SPELL.TARGETCOLLECTRANGE", Int, 1, None;
    10 => target_collect_azi: i32 = Default::default(), b"C_SPELL.TARGETCOLLECTAZI", Int, 1, None;
    11 => target_collect_elev: i32 = Default::default(), b"C_SPELL.TARGETCOLLECTELEV", Int, 1, None;
}, {});

define_class!(Svm, b"C_SVM", {
    0 => mil_greetings: Vec<u8> = Default::default(), b"C_SVM.MILGREETINGS", String, 1, Some(b"C_SVM.MILGREETINGS");
    1 => pal_greetings: Vec<u8> = Default::default(), b"C_SVM.PALGREETINGS", String, 1, Some(b"C_SVM.PALGREETINGS");
    2 => weather: Vec<u8> = Default::default(), b"C_SVM.WEATHER", String, 1, Some(b"C_SVM.WEATHER");
    3 => iget_you_still: Vec<u8> = Default::default(), b"C_SVM.IGETYOUSTILL", String, 1, Some(b"C_SVM.IGETYOUSTILL");
    4 => die_enemy: Vec<u8> = Default::default(), b"C_SVM.DIEENEMY", String, 1, Some(b"C_SVM.DIEENEMY");
    5 => die_monster: Vec<u8> = Default::default(), b"C_SVM.DIEMONSTER", String, 1, Some(b"C_SVM.DIEMONSTER");
    6 => addon_die_monster: Vec<u8> = Default::default(), b"C_SVM.ADDON_DIEMONSTER", String, 1, Some(b"C_SVM.ADDON_DIEMONSTER");
    7 => addon_die_monster_2: Vec<u8> = Default::default(), b"C_SVM.ADDON_DIEMONSTER2", String, 1, Some(b"C_SVM.ADDON_DIEMONSTER2");
    8 => dirty_thief: Vec<u8> = Default::default(), b"C_SVM.DIRTYTHIEF", String, 1, Some(b"C_SVM.DIRTYTHIEF");
    9 => hands_off: Vec<u8> = Default::default(), b"C_SVM.HANDSOFF", String, 1, Some(b"C_SVM.HANDSOFF");
    10 => sheep_killer: Vec<u8> = Default::default(), b"C_SVM.SHEEPKILLER", String, 1, Some(b"C_SVM.SHEEPKILLER");
    11 => sheep_killer_monster: Vec<u8> = Default::default(), b"C_SVM.SHEEPKILLERMONSTER", String, 1, Some(b"C_SVM.SHEEPKILLERMONSTER");
    12 => you_murderer: Vec<u8> = Default::default(), b"C_SVM.YOUMURDERER", String, 1, Some(b"C_SVM.YOUMURDERER");
    13 => die_stupid_beast: Vec<u8> = Default::default(), b"C_SVM.DIESTUPIDBEAST", String, 1, Some(b"C_SVM.DIESTUPIDBEAST");
    14 => you_dare_hit_me: Vec<u8> = Default::default(), b"C_SVM.YOUDAREHITME", String, 1, Some(b"C_SVM.YOUDAREHITME");
    15 => you_asked_for_it: Vec<u8> = Default::default(), b"C_SVM.YOUASKEDFORIT", String, 1, Some(b"C_SVM.YOUASKEDFORIT");
    16 => then_ibeat_you_out_of_here: Vec<u8> = Default::default(), b"C_SVM.THENIBEATYOUOUTOFHERE", String, 1, Some(b"C_SVM.THENIBEATYOUOUTOFHERE");
    17 => what_did_you_do_in_there: Vec<u8> = Default::default(), b"C_SVM.WHATDIDYOUDOINTHERE", String, 1, Some(b"C_SVM.WHATDIDYOUDOINTHERE");
    18 => will_you_stop_fighting: Vec<u8> = Default::default(), b"C_SVM.WILLYOUSTOPFIGHTING", String, 1, Some(b"C_SVM.WILLYOUSTOPFIGHTING");
    19 => kill_enemy: Vec<u8> = Default::default(), b"C_SVM.KILLENEMY", String, 1, Some(b"C_SVM.KILLENEMY");
    20 => enemy_killed: Vec<u8> = Default::default(), b"C_SVM.ENEMYKILLED", String, 1, Some(b"C_SVM.ENEMYKILLED");
    21 => monster_killed: Vec<u8> = Default::default(), b"C_SVM.MONSTERKILLED", String, 1, Some(b"C_SVM.MONSTERKILLED");
    22 => addon_monster_killed: Vec<u8> = Default::default(), b"C_SVM.ADDON_MONSTERKILLED", String, 1, Some(b"C_SVM.ADDON_MONSTERKILLED");
    23 => addon_monster_killed_2: Vec<u8> = Default::default(), b"C_SVM.ADDON_MONSTERKILLED2", String, 1, Some(b"C_SVM.ADDON_MONSTERKILLED2");
    24 => thief_down: Vec<u8> = Default::default(), b"C_SVM.THIEFDOWN", String, 1, Some(b"C_SVM.THIEFDOWN");
    25 => rumfummler_down: Vec<u8> = Default::default(), b"C_SVM.RUMFUMMLERDOWN", String, 1, Some(b"C_SVM.RUMFUMMLERDOWN");
    26 => sheep_attacker_down: Vec<u8> = Default::default(), b"C_SVM.SHEEPATTACKERDOWN", String, 1, Some(b"C_SVM.SHEEPATTACKERDOWN");
    27 => kill_murderer: Vec<u8> = Default::default(), b"C_SVM.KILLMURDERER", String, 1, Some(b"C_SVM.KILLMURDERER");
    28 => stupid_beast_killed: Vec<u8> = Default::default(), b"C_SVM.STUPIDBEASTKILLED", String, 1, Some(b"C_SVM.STUPIDBEASTKILLED");
    29 => never_hit_me_again: Vec<u8> = Default::default(), b"C_SVM.NEVERHITMEAGAIN", String, 1, Some(b"C_SVM.NEVERHITMEAGAIN");
    30 => you_better_should_have_listened: Vec<u8> = Default::default(), b"C_SVM.YOUBETTERSHOULDHAVELISTENED", String, 1, Some(b"C_SVM.YOUBETTERSHOULDHAVELISTENED");
    31 => get_up_and_begone: Vec<u8> = Default::default(), b"C_SVM.GETUPANDBEGONE", String, 1, Some(b"C_SVM.GETUPANDBEGONE");
    32 => never_enter_room_again: Vec<u8> = Default::default(), b"C_SVM.NEVERENTERROOMAGAIN", String, 1, Some(b"C_SVM.NEVERENTERROOMAGAIN");
    33 => there_is_no_fighting_here: Vec<u8> = Default::default(), b"C_SVM.THEREISNOFIGHTINGHERE", String, 1, Some(b"C_SVM.THEREISNOFIGHTINGHERE");
    34 => spare_me: Vec<u8> = Default::default(), b"C_SVM.SPAREME", String, 1, Some(b"C_SVM.SPAREME");
    35 => run_away: Vec<u8> = Default::default(), b"C_SVM.RUNAWAY", String, 1, Some(b"C_SVM.RUNAWAY");
    36 => alarm: Vec<u8> = Default::default(), b"C_SVM.ALARM", String, 1, Some(b"C_SVM.ALARM");
    37 => guards: Vec<u8> = Default::default(), b"C_SVM.GUARDS", String, 1, Some(b"C_SVM.GUARDS");
    38 => help: Vec<u8> = Default::default(), b"C_SVM.HELP", String, 1, Some(b"C_SVM.HELP");
    39 => good_monster_kill: Vec<u8> = Default::default(), b"C_SVM.GOODMONSTERKILL", String, 1, Some(b"C_SVM.GOODMONSTERKILL");
    40 => good_kill: Vec<u8> = Default::default(), b"C_SVM.GOODKILL", String, 1, Some(b"C_SVM.GOODKILL");
    41 => not_now: Vec<u8> = Default::default(), b"C_SVM.NOTNOW", String, 1, Some(b"C_SVM.NOTNOW");
    42 => run_coward: Vec<u8> = Default::default(), b"C_SVM.RUNCOWARD", String, 1, Some(b"C_SVM.RUNCOWARD");
    43 => get_out_of_here: Vec<u8> = Default::default(), b"C_SVM.GETOUTOFHERE", String, 1, Some(b"C_SVM.GETOUTOFHERE");
    44 => why_are_you_in_here: Vec<u8> = Default::default(), b"C_SVM.WHYAREYOUINHERE", String, 1, Some(b"C_SVM.WHYAREYOUINHERE");
    45 => yes_go_out_of_here: Vec<u8> = Default::default(), b"C_SVM.YESGOOUTOFHERE", String, 1, Some(b"C_SVM.YESGOOUTOFHERE");
    46 => whats_this_supposed_to_be: Vec<u8> = Default::default(), b"C_SVM.WHATSTHISSUPPOSEDTOBE", String, 1, Some(b"C_SVM.WHATSTHISSUPPOSEDTOBE");
    47 => you_disturbed_my_slumber: Vec<u8> = Default::default(), b"C_SVM.YOUDISTURBEDMYSLUMBER", String, 1, Some(b"C_SVM.YOUDISTURBEDMYSLUMBER");
    48 => itook_your_gold: Vec<u8> = Default::default(), b"C_SVM.ITOOKYOURGOLD", String, 1, Some(b"C_SVM.ITOOKYOURGOLD");
    49 => shit_no_gold: Vec<u8> = Default::default(), b"C_SVM.SHITNOGOLD", String, 1, Some(b"C_SVM.SHITNOGOLD");
    50 => itake_your_weapon: Vec<u8> = Default::default(), b"C_SVM.ITAKEYOURWEAPON", String, 1, Some(b"C_SVM.ITAKEYOURWEAPON");
    51 => what_are_you_doing: Vec<u8> = Default::default(), b"C_SVM.WHATAREYOUDOING", String, 1, Some(b"C_SVM.WHATAREYOUDOING");
    52 => looking_for_trouble_again: Vec<u8> = Default::default(), b"C_SVM.LOOKINGFORTROUBLEAGAIN", String, 1, Some(b"C_SVM.LOOKINGFORTROUBLEAGAIN");
    53 => stop_magic: Vec<u8> = Default::default(), b"C_SVM.STOPMAGIC", String, 1, Some(b"C_SVM.STOPMAGIC");
    54 => isaid_stop_magic: Vec<u8> = Default::default(), b"C_SVM.ISAIDSTOPMAGIC", String, 1, Some(b"C_SVM.ISAIDSTOPMAGIC");
    55 => weapon_down: Vec<u8> = Default::default(), b"C_SVM.WEAPONDOWN", String, 1, Some(b"C_SVM.WEAPONDOWN");
    56 => isaid_weapon_down: Vec<u8> = Default::default(), b"C_SVM.ISAIDWEAPONDOWN", String, 1, Some(b"C_SVM.ISAIDWEAPONDOWN");
    57 => wise_move: Vec<u8> = Default::default(), b"C_SVM.WISEMOVE", String, 1, Some(b"C_SVM.WISEMOVE");
    58 => next_time_youre_in_for_it: Vec<u8> = Default::default(), b"C_SVM.NEXTTIMEYOUREINFORIT", String, 1, Some(b"C_SVM.NEXTTIMEYOUREINFORIT");
    59 => oh_my_head: Vec<u8> = Default::default(), b"C_SVM.OHMYHEAD", String, 1, Some(b"C_SVM.OHMYHEAD");
    60 => theres_afight: Vec<u8> = Default::default(), b"C_SVM.THERESAFIGHT", String, 1, Some(b"C_SVM.THERESAFIGHT");
    61 => oh_my_god_its_afight: Vec<u8> = Default::default(), b"C_SVM.OHMYGODITSAFIGHT", String, 1, Some(b"C_SVM.OHMYGODITSAFIGHT");
    62 => good_victory: Vec<u8> = Default::default(), b"C_SVM.GOODVICTORY", String, 1, Some(b"C_SVM.GOODVICTORY");
    63 => not_bad: Vec<u8> = Default::default(), b"C_SVM.NOTBAD", String, 1, Some(b"C_SVM.NOTBAD");
    64 => oh_my_god_hes_down: Vec<u8> = Default::default(), b"C_SVM.OHMYGODHESDOWN", String, 1, Some(b"C_SVM.OHMYGODHESDOWN");
    65 => cheer_friend_01: Vec<u8> = Default::default(), b"C_SVM.CHEERFRIEND01", String, 1, Some(b"C_SVM.CHEERFRIEND01");
    66 => cheer_friend_02: Vec<u8> = Default::default(), b"C_SVM.CHEERFRIEND02", String, 1, Some(b"C_SVM.CHEERFRIEND02");
    67 => cheer_friend_03: Vec<u8> = Default::default(), b"C_SVM.CHEERFRIEND03", String, 1, Some(b"C_SVM.CHEERFRIEND03");
    68 => ooh_01: Vec<u8> = Default::default(), b"C_SVM.OOH01", String, 1, Some(b"C_SVM.OOH01");
    69 => ooh_02: Vec<u8> = Default::default(), b"C_SVM.OOH02", String, 1, Some(b"C_SVM.OOH02");
    70 => ooh_03: Vec<u8> = Default::default(), b"C_SVM.OOH03", String, 1, Some(b"C_SVM.OOH03");
    71 => what_was_that: Vec<u8> = Default::default(), b"C_SVM.WHATWASTHAT", String, 1, Some(b"C_SVM.WHATWASTHAT");
    72 => get_out_of_my_bed: Vec<u8> = Default::default(), b"C_SVM.GETOUTOFMYBED", String, 1, Some(b"C_SVM.GETOUTOFMYBED");
    73 => awake: Vec<u8> = Default::default(), b"C_SVM.AWAKE", String, 1, Some(b"C_SVM.AWAKE");
    74 => abs_commander: Vec<u8> = Default::default(), b"C_SVM.ABS_COMMANDER", String, 1, Some(b"C_SVM.ABS_COMMANDER");
    75 => abs_monastery: Vec<u8> = Default::default(), b"C_SVM.ABS_MONASTERY", String, 1, Some(b"C_SVM.ABS_MONASTERY");
    76 => abs_farm: Vec<u8> = Default::default(), b"C_SVM.ABS_FARM", String, 1, Some(b"C_SVM.ABS_FARM");
    77 => abs_good: Vec<u8> = Default::default(), b"C_SVM.ABS_GOOD", String, 1, Some(b"C_SVM.ABS_GOOD");
    78 => sheep_killer_crime: Vec<u8> = Default::default(), b"C_SVM.SHEEPKILLER_CRIME", String, 1, Some(b"C_SVM.SHEEPKILLER_CRIME");
    79 => attack_crime: Vec<u8> = Default::default(), b"C_SVM.ATTACK_CRIME", String, 1, Some(b"C_SVM.ATTACK_CRIME");
    80 => theft_crime: Vec<u8> = Default::default(), b"C_SVM.THEFT_CRIME", String, 1, Some(b"C_SVM.THEFT_CRIME");
    81 => murder_crime: Vec<u8> = Default::default(), b"C_SVM.MURDER_CRIME", String, 1, Some(b"C_SVM.MURDER_CRIME");
    82 => pal_city_crime: Vec<u8> = Default::default(), b"C_SVM.PAL_CITY_CRIME", String, 1, Some(b"C_SVM.PAL_CITY_CRIME");
    83 => mil_city_crime: Vec<u8> = Default::default(), b"C_SVM.MIL_CITY_CRIME", String, 1, Some(b"C_SVM.MIL_CITY_CRIME");
    84 => city_crime: Vec<u8> = Default::default(), b"C_SVM.CITY_CRIME", String, 1, Some(b"C_SVM.CITY_CRIME");
    85 => mona_crime: Vec<u8> = Default::default(), b"C_SVM.MONA_CRIME", String, 1, Some(b"C_SVM.MONA_CRIME");
    86 => farm_crime: Vec<u8> = Default::default(), b"C_SVM.FARM_CRIME", String, 1, Some(b"C_SVM.FARM_CRIME");
    87 => oc_crime: Vec<u8> = Default::default(), b"C_SVM.OC_CRIME", String, 1, Some(b"C_SVM.OC_CRIME");
    88 => toughguy_attack_lost: Vec<u8> = Default::default(), b"C_SVM.TOUGHGUY_ATTACKLOST", String, 1, Some(b"C_SVM.TOUGHGUY_ATTACKLOST");
    89 => toughguy_attack_won: Vec<u8> = Default::default(), b"C_SVM.TOUGHGUY_ATTACKWON", String, 1, Some(b"C_SVM.TOUGHGUY_ATTACKWON");
    90 => toughguy_player_attack: Vec<u8> = Default::default(), b"C_SVM.TOUGHGUY_PLAYERATTACK", String, 1, Some(b"C_SVM.TOUGHGUY_PLAYERATTACK");
    91 => gold_1000: Vec<u8> = Default::default(), b"C_SVM.GOLD_1000", String, 1, Some(b"C_SVM.GOLD_1000");
    92 => gold_950: Vec<u8> = Default::default(), b"C_SVM.GOLD_950", String, 1, Some(b"C_SVM.GOLD_950");
    93 => gold_900: Vec<u8> = Default::default(), b"C_SVM.GOLD_900", String, 1, Some(b"C_SVM.GOLD_900");
    94 => gold_850: Vec<u8> = Default::default(), b"C_SVM.GOLD_850", String, 1, Some(b"C_SVM.GOLD_850");
    95 => gold_800: Vec<u8> = Default::default(), b"C_SVM.GOLD_800", String, 1, Some(b"C_SVM.GOLD_800");
    96 => gold_750: Vec<u8> = Default::default(), b"C_SVM.GOLD_750", String, 1, Some(b"C_SVM.GOLD_750");
    97 => gold_700: Vec<u8> = Default::default(), b"C_SVM.GOLD_700", String, 1, Some(b"C_SVM.GOLD_700");
    98 => gold_650: Vec<u8> = Default::default(), b"C_SVM.GOLD_650", String, 1, Some(b"C_SVM.GOLD_650");
    99 => gold_600: Vec<u8> = Default::default(), b"C_SVM.GOLD_600", String, 1, Some(b"C_SVM.GOLD_600");
    100 => gold_550: Vec<u8> = Default::default(), b"C_SVM.GOLD_550", String, 1, Some(b"C_SVM.GOLD_550");
    101 => gold_500: Vec<u8> = Default::default(), b"C_SVM.GOLD_500", String, 1, Some(b"C_SVM.GOLD_500");
    102 => gold_450: Vec<u8> = Default::default(), b"C_SVM.GOLD_450", String, 1, Some(b"C_SVM.GOLD_450");
    103 => gold_400: Vec<u8> = Default::default(), b"C_SVM.GOLD_400", String, 1, Some(b"C_SVM.GOLD_400");
    104 => gold_350: Vec<u8> = Default::default(), b"C_SVM.GOLD_350", String, 1, Some(b"C_SVM.GOLD_350");
    105 => gold_300: Vec<u8> = Default::default(), b"C_SVM.GOLD_300", String, 1, Some(b"C_SVM.GOLD_300");
    106 => gold_250: Vec<u8> = Default::default(), b"C_SVM.GOLD_250", String, 1, Some(b"C_SVM.GOLD_250");
    107 => gold_200: Vec<u8> = Default::default(), b"C_SVM.GOLD_200", String, 1, Some(b"C_SVM.GOLD_200");
    108 => gold_150: Vec<u8> = Default::default(), b"C_SVM.GOLD_150", String, 1, Some(b"C_SVM.GOLD_150");
    109 => gold_100: Vec<u8> = Default::default(), b"C_SVM.GOLD_100", String, 1, Some(b"C_SVM.GOLD_100");
    110 => gold_90: Vec<u8> = Default::default(), b"C_SVM.GOLD_90", String, 1, Some(b"C_SVM.GOLD_90");
    111 => gold_80: Vec<u8> = Default::default(), b"C_SVM.GOLD_80", String, 1, Some(b"C_SVM.GOLD_80");
    112 => gold_70: Vec<u8> = Default::default(), b"C_SVM.GOLD_70", String, 1, Some(b"C_SVM.GOLD_70");
    113 => gold_60: Vec<u8> = Default::default(), b"C_SVM.GOLD_60", String, 1, Some(b"C_SVM.GOLD_60");
    114 => gold_50: Vec<u8> = Default::default(), b"C_SVM.GOLD_50", String, 1, Some(b"C_SVM.GOLD_50");
    115 => gold_40: Vec<u8> = Default::default(), b"C_SVM.GOLD_40", String, 1, Some(b"C_SVM.GOLD_40");
    116 => gold_30: Vec<u8> = Default::default(), b"C_SVM.GOLD_30", String, 1, Some(b"C_SVM.GOLD_30");
    117 => gold_20: Vec<u8> = Default::default(), b"C_SVM.GOLD_20", String, 1, Some(b"C_SVM.GOLD_20");
    118 => gold_10: Vec<u8> = Default::default(), b"C_SVM.GOLD_10", String, 1, Some(b"C_SVM.GOLD_10");
    119 => smalltalk_01: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK01", String, 1, Some(b"C_SVM.SMALLTALK01");
    120 => smalltalk_02: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK02", String, 1, Some(b"C_SVM.SMALLTALK02");
    121 => smalltalk_03: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK03", String, 1, Some(b"C_SVM.SMALLTALK03");
    122 => smalltalk_04: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK04", String, 1, Some(b"C_SVM.SMALLTALK04");
    123 => smalltalk_05: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK05", String, 1, Some(b"C_SVM.SMALLTALK05");
    124 => smalltalk_06: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK06", String, 1, Some(b"C_SVM.SMALLTALK06");
    125 => smalltalk_07: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK07", String, 1, Some(b"C_SVM.SMALLTALK07");
    126 => smalltalk_08: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK08", String, 1, Some(b"C_SVM.SMALLTALK08");
    127 => smalltalk_09: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK09", String, 1, Some(b"C_SVM.SMALLTALK09");
    128 => smalltalk_10: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK10", String, 1, Some(b"C_SVM.SMALLTALK10");
    129 => smalltalk_11: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK11", String, 1, Some(b"C_SVM.SMALLTALK11");
    130 => smalltalk_12: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK12", String, 1, Some(b"C_SVM.SMALLTALK12");
    131 => smalltalk_13: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK13", String, 1, Some(b"C_SVM.SMALLTALK13");
    132 => smalltalk_14: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK14", String, 1, Some(b"C_SVM.SMALLTALK14");
    133 => smalltalk_15: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK15", String, 1, Some(b"C_SVM.SMALLTALK15");
    134 => smalltalk_16: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK16", String, 1, Some(b"C_SVM.SMALLTALK16");
    135 => smalltalk_17: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK17", String, 1, Some(b"C_SVM.SMALLTALK17");
    136 => smalltalk_18: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK18", String, 1, Some(b"C_SVM.SMALLTALK18");
    137 => smalltalk_19: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK19", String, 1, Some(b"C_SVM.SMALLTALK19");
    138 => smalltalk_20: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK20", String, 1, Some(b"C_SVM.SMALLTALK20");
    139 => smalltalk_21: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK21", String, 1, Some(b"C_SVM.SMALLTALK21");
    140 => smalltalk_22: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK22", String, 1, Some(b"C_SVM.SMALLTALK22");
    141 => smalltalk_23: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK23", String, 1, Some(b"C_SVM.SMALLTALK23");
    142 => smalltalk_24: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK24", String, 1, Some(b"C_SVM.SMALLTALK24");
    143 => smalltalk_25: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK25", String, 1, Some(b"C_SVM.SMALLTALK25");
    144 => smalltalk_26: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK26", String, 1, Some(b"C_SVM.SMALLTALK26");
    145 => smalltalk_27: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK27", String, 1, Some(b"C_SVM.SMALLTALK27");
    146 => smalltalk_28: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK28", String, 1, Some(b"C_SVM.SMALLTALK28");
    147 => smalltalk_29: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK29", String, 1, Some(b"C_SVM.SMALLTALK29");
    148 => smalltalk_30: Vec<u8> = Default::default(), b"C_SVM.SMALLTALK30", String, 1, Some(b"C_SVM.SMALLTALK30");
    149 => no_learn_no_points: Vec<u8> = Default::default(), b"C_SVM.NOLEARNNOPOINTS", String, 1, Some(b"C_SVM.NOLEARNNOPOINTS");
    150 => no_learn_over_personal_max: Vec<u8> = Default::default(), b"C_SVM.NOLEARNOVERPERSONALMAX", String, 1, Some(b"C_SVM.NOLEARNOVERPERSONALMAX");
    151 => no_learn_youre_better: Vec<u8> = Default::default(), b"C_SVM.NOLEARNYOUREBETTER", String, 1, Some(b"C_SVM.NOLEARNYOUREBETTER");
    152 => you_learned_something: Vec<u8> = Default::default(), b"C_SVM.YOULEARNEDSOMETHING", String, 1, Some(b"C_SVM.YOULEARNEDSOMETHING");
    153 => unterstadt: Vec<u8> = Default::default(), b"C_SVM.UNTERSTADT", String, 1, Some(b"C_SVM.UNTERSTADT");
    154 => oberstadt: Vec<u8> = Default::default(), b"C_SVM.OBERSTADT", String, 1, Some(b"C_SVM.OBERSTADT");
    155 => tempel: Vec<u8> = Default::default(), b"C_SVM.TEMPEL", String, 1, Some(b"C_SVM.TEMPEL");
    156 => markt: Vec<u8> = Default::default(), b"C_SVM.MARKT", String, 1, Some(b"C_SVM.MARKT");
    157 => galgen: Vec<u8> = Default::default(), b"C_SVM.GALGEN", String, 1, Some(b"C_SVM.GALGEN");
    158 => kaserne: Vec<u8> = Default::default(), b"C_SVM.KASERNE", String, 1, Some(b"C_SVM.KASERNE");
    159 => hafen: Vec<u8> = Default::default(), b"C_SVM.HAFEN", String, 1, Some(b"C_SVM.HAFEN");
    160 => whereto: Vec<u8> = Default::default(), b"C_SVM.WHERETO", String, 1, Some(b"C_SVM.WHERETO");
    161 => oberstadt_2_unterstadt: Vec<u8> = Default::default(), b"C_SVM.OBERSTADT_2_UNTERSTADT", String, 1, Some(b"C_SVM.OBERSTADT_2_UNTERSTADT");
    162 => unterstadt_2_oberstadt: Vec<u8> = Default::default(), b"C_SVM.UNTERSTADT_2_OBERSTADT", String, 1, Some(b"C_SVM.UNTERSTADT_2_OBERSTADT");
    163 => unterstadt_2_tempel: Vec<u8> = Default::default(), b"C_SVM.UNTERSTADT_2_TEMPEL", String, 1, Some(b"C_SVM.UNTERSTADT_2_TEMPEL");
    164 => unterstadt_2_hafen: Vec<u8> = Default::default(), b"C_SVM.UNTERSTADT_2_HAFEN", String, 1, Some(b"C_SVM.UNTERSTADT_2_HAFEN");
    165 => tempel_2_unterstadt: Vec<u8> = Default::default(), b"C_SVM.TEMPEL_2_UNTERSTADT", String, 1, Some(b"C_SVM.TEMPEL_2_UNTERSTADT");
    166 => tempel_2_markt: Vec<u8> = Default::default(), b"C_SVM.TEMPEL_2_MARKT", String, 1, Some(b"C_SVM.TEMPEL_2_MARKT");
    167 => tempel_2_galgen: Vec<u8> = Default::default(), b"C_SVM.TEMPEL_2_GALGEN", String, 1, Some(b"C_SVM.TEMPEL_2_GALGEN");
    168 => markt_2_tempel: Vec<u8> = Default::default(), b"C_SVM.MARKT_2_TEMPEL", String, 1, Some(b"C_SVM.MARKT_2_TEMPEL");
    169 => markt_2_kaserne: Vec<u8> = Default::default(), b"C_SVM.MARKT_2_KASERNE", String, 1, Some(b"C_SVM.MARKT_2_KASERNE");
    170 => markt_2_galgen: Vec<u8> = Default::default(), b"C_SVM.MARKT_2_GALGEN", String, 1, Some(b"C_SVM.MARKT_2_GALGEN");
    171 => galgen_2_tempel: Vec<u8> = Default::default(), b"C_SVM.GALGEN_2_TEMPEL", String, 1, Some(b"C_SVM.GALGEN_2_TEMPEL");
    172 => galgen_2_markt: Vec<u8> = Default::default(), b"C_SVM.GALGEN_2_MARKT", String, 1, Some(b"C_SVM.GALGEN_2_MARKT");
    173 => galgen_2_kaserne: Vec<u8> = Default::default(), b"C_SVM.GALGEN_2_KASERNE", String, 1, Some(b"C_SVM.GALGEN_2_KASERNE");
    174 => kaserne_2_markt: Vec<u8> = Default::default(), b"C_SVM.KASERNE_2_MARKT", String, 1, Some(b"C_SVM.KASERNE_2_MARKT");
    175 => kaserne_2_galgen: Vec<u8> = Default::default(), b"C_SVM.KASERNE_2_GALGEN", String, 1, Some(b"C_SVM.KASERNE_2_GALGEN");
    176 => hafen_2_unterstadt: Vec<u8> = Default::default(), b"C_SVM.HAFEN_2_UNTERSTADT", String, 1, Some(b"C_SVM.HAFEN_2_UNTERSTADT");
    177 => dead: Vec<u8> = Default::default(), b"C_SVM.DEAD", String, 1, Some(b"C_SVM.DEAD");
    178 => aargh_1: Vec<u8> = Default::default(), b"C_SVM.AARGH_1", String, 1, Some(b"C_SVM.AARGH_1");
    179 => aargh_2: Vec<u8> = Default::default(), b"C_SVM.AARGH_2", String, 1, Some(b"C_SVM.AARGH_2");
    180 => aargh_3: Vec<u8> = Default::default(), b"C_SVM.AARGH_3", String, 1, Some(b"C_SVM.AARGH_3");
    181 => addon_wrong_armor: Vec<u8> = Default::default(), b"C_SVM.ADDON_WRONGARMOR", String, 1, Some(b"C_SVM.ADDON_WRONGARMOR");
    182 => addon_wrong_armor_sld: Vec<u8> = Default::default(), b"C_SVM.ADDON_WRONGARMOR_SLD", String, 1, Some(b"C_SVM.ADDON_WRONGARMOR_SLD");
    183 => addon_wrong_armor_mil: Vec<u8> = Default::default(), b"C_SVM.ADDON_WRONGARMOR_MIL", String, 1, Some(b"C_SVM.ADDON_WRONGARMOR_MIL");
    184 => addon_wrong_armor_kdf: Vec<u8> = Default::default(), b"C_SVM.ADDON_WRONGARMOR_KDF", String, 1, Some(b"C_SVM.ADDON_WRONGARMOR_KDF");
    185 => addon_no_armor_bdt: Vec<u8> = Default::default(), b"C_SVM.ADDON_NOARMOR_BDT", String, 1, Some(b"C_SVM.ADDON_NOARMOR_BDT");
    186 => addon_die_bandit: Vec<u8> = Default::default(), b"C_SVM.ADDON_DIEBANDIT", String, 1, Some(b"C_SVM.ADDON_DIEBANDIT");
    187 => addon_dirty_pirate: Vec<u8> = Default::default(), b"C_SVM.ADDON_DIRTYPIRATE", String, 1, Some(b"C_SVM.ADDON_DIRTYPIRATE");
    188 => sc_hey_turn_around: Vec<u8> = Default::default(), b"C_SVM.SC_HEYTURNAROUND", String, 1, Some(b"C_SVM.SC_HEYTURNAROUND");
    189 => sc_hey_turn_around_02: Vec<u8> = Default::default(), b"C_SVM.SC_HEYTURNAROUND02", String, 1, Some(b"C_SVM.SC_HEYTURNAROUND02");
    190 => sc_hey_turn_around_03: Vec<u8> = Default::default(), b"C_SVM.SC_HEYTURNAROUND03", String, 1, Some(b"C_SVM.SC_HEYTURNAROUND03");
    191 => sc_hey_turn_around_04: Vec<u8> = Default::default(), b"C_SVM.SC_HEYTURNAROUND04", String, 1, Some(b"C_SVM.SC_HEYTURNAROUND04");
    192 => sc_hey_wait_asecond: Vec<u8> = Default::default(), b"C_SVM.SC_HEYWAITASECOND", String, 1, Some(b"C_SVM.SC_HEYWAITASECOND");
    193 => doesnt_mork: Vec<u8> = Default::default(), b"C_SVM.DOESNTWORK", String, 1, Some(b"C_SVM.DOESNTWORK");
    194 => pick_broke: Vec<u8> = Default::default(), b"C_SVM.PICKBROKE", String, 1, Some(b"C_SVM.PICKBROKE");
    195 => need_key: Vec<u8> = Default::default(), b"C_SVM.NEEDKEY", String, 1, Some(b"C_SVM.NEEDKEY");
    196 => no_more_picks: Vec<u8> = Default::default(), b"C_SVM.NOMOREPICKS", String, 1, Some(b"C_SVM.NOMOREPICKS");
    197 => no_pick_lock_talent: Vec<u8> = Default::default(), b"C_SVM.NOPICKLOCKTALENT", String, 1, Some(b"C_SVM.NOPICKLOCKTALENT");
    198 => no_sweeping: Vec<u8> = Default::default(), b"C_SVM.NOSWEEPING", String, 1, Some(b"C_SVM.NOSWEEPING");
    199 => pick_lock_or_key_missing: Vec<u8> = Default::default(), b"C_SVM.PICKLOCKORKEYMISSING", String, 1, Some(b"C_SVM.PICKLOCKORKEYMISSING");
    200 => key_missing: Vec<u8> = Default::default(), b"C_SVM.KEYMISSING", String, 1, Some(b"C_SVM.KEYMISSING");
    201 => pick_lock_missing: Vec<u8> = Default::default(), b"C_SVM.PICKLOCKMISSING", String, 1, Some(b"C_SVM.PICKLOCKMISSING");
    202 => never_open: Vec<u8> = Default::default(), b"C_SVM.NEVEROPEN", String, 1, Some(b"C_SVM.NEVEROPEN");
    203 => missing_item: Vec<u8> = Default::default(), b"C_SVM.MISSINGITEM", String, 1, Some(b"C_SVM.MISSINGITEM");
    204 => dont_know: Vec<u8> = Default::default(), b"C_SVM.DONTKNOW", String, 1, Some(b"C_SVM.DONTKNOW");
    205 => nothing_to_get: Vec<u8> = Default::default(), b"C_SVM.NOTHINGTOGET", String, 1, Some(b"C_SVM.NOTHINGTOGET");
    206 => nothing_to_get_02: Vec<u8> = Default::default(), b"C_SVM.NOTHINGTOGET02", String, 1, Some(b"C_SVM.NOTHINGTOGET02");
    207 => nothing_to_get_03: Vec<u8> = Default::default(), b"C_SVM.NOTHINGTOGET03", String, 1, Some(b"C_SVM.NOTHINGTOGET03");
    208 => heal_shrine: Vec<u8> = Default::default(), b"C_SVM.HEALSHRINE", String, 1, Some(b"C_SVM.HEALSHRINE");
    209 => heal_last_shrine: Vec<u8> = Default::default(), b"C_SVM.HEALLASTSHRINE", String, 1, Some(b"C_SVM.HEALLASTSHRINE");
    210 => irdorath_there_you_are: Vec<u8> = Default::default(), b"C_SVM.IRDORATHTHEREYOUARE", String, 1, Some(b"C_SVM.IRDORATHTHEREYOUARE");
    211 => sc_opens_irdorath_book: Vec<u8> = Default::default(), b"C_SVM.SCOPENSIRDORATHBOOK", String, 1, Some(b"C_SVM.SCOPENSIRDORATHBOOK");
    212 => sc_opens_last_door: Vec<u8> = Default::default(), b"C_SVM.SCOPENSLASTDOOR", String, 1, Some(b"C_SVM.SCOPENSLASTDOOR");
    213 => trade_1: Vec<u8> = Default::default(), b"C_SVM.TRADE_1", String, 1, Some(b"C_SVM.TRADE_1");
    214 => trade_2: Vec<u8> = Default::default(), b"C_SVM.TRADE_2", String, 1, Some(b"C_SVM.TRADE_2");
    215 => trade_3: Vec<u8> = Default::default(), b"C_SVM.TRADE_3", String, 1, Some(b"C_SVM.TRADE_3");
    216 => verstehe: Vec<u8> = Default::default(), b"C_SVM.VERSTEHE", String, 1, Some(b"C_SVM.VERSTEHE");
    217 => found_treasure: Vec<u8> = Default::default(), b"C_SVM.FOUNDTREASURE", String, 1, Some(b"C_SVM.FOUNDTREASURE");
    218 => cant_understand_this: Vec<u8> = Default::default(), b"C_SVM.CANTUNDERSTANDTHIS", String, 1, Some(b"C_SVM.CANTUNDERSTANDTHIS");
    219 => cant_read_this: Vec<u8> = Default::default(), b"C_SVM.CANTREADTHIS", String, 1, Some(b"C_SVM.CANTREADTHIS");
    220 => stoneplate_1: Vec<u8> = Default::default(), b"C_SVM.STONEPLATE_1", String, 1, Some(b"C_SVM.STONEPLATE_1");
    221 => stoneplate_2: Vec<u8> = Default::default(), b"C_SVM.STONEPLATE_2", String, 1, Some(b"C_SVM.STONEPLATE_2");
    222 => stoneplate_3: Vec<u8> = Default::default(), b"C_SVM.STONEPLATE_3", String, 1, Some(b"C_SVM.STONEPLATE_3");
    223 => cough: Vec<u8> = Default::default(), b"C_SVM.COUGH", String, 1, Some(b"C_SVM.COUGH");
    224 => hui: Vec<u8> = Default::default(), b"C_SVM.HUI", String, 1, Some(b"C_SVM.HUI");
    225 => addon_this_little_bastard: Vec<u8> = Default::default(), b"C_SVM.ADDON_THISLITTLEBASTARD", String, 1, Some(b"C_SVM.ADDON_THISLITTLEBASTARD");
    226 => addon_open_adanos_temple: Vec<u8> = Default::default(), b"C_SVM.ADDON_OPENADANOSTEMPLE", String, 1, Some(b"C_SVM.ADDON_OPENADANOSTEMPLE");
    227 => attentat_addon_description: Vec<u8> = Default::default(), b"C_SVM.ATTENTAT_ADDON_DESCRIPTION", String, 1, Some(b"C_SVM.ATTENTAT_ADDON_DESCRIPTION");
    228 => attentat_addon_description_2: Vec<u8> = Default::default(), b"C_SVM.ATTENTAT_ADDON_DESCRIPTION2", String, 1, Some(b"C_SVM.ATTENTAT_ADDON_DESCRIPTION2");
    229 => attentat_addon_pro: Vec<u8> = Default::default(), b"C_SVM.ATTENTAT_ADDON_PRO", String, 1, Some(b"C_SVM.ATTENTAT_ADDON_PRO");
    230 => attentat_addon_contra: Vec<u8> = Default::default(), b"C_SVM.ATTENTAT_ADDON_CONTRA", String, 1, Some(b"C_SVM.ATTENTAT_ADDON_CONTRA");
    231 => mine_addon_description: Vec<u8> = Default::default(), b"C_SVM.MINE_ADDON_DESCRIPTION", String, 1, Some(b"C_SVM.MINE_ADDON_DESCRIPTION");
    232 => addon_summon_ancient_ghost: Vec<u8> = Default::default(), b"C_SVM.ADDON_SUMMONANCIENTGHOST", String, 1, Some(b"C_SVM.ADDON_SUMMONANCIENTGHOST");
    233 => addon_ancient_ghost_not_near: Vec<u8> = Default::default(), b"C_SVM.ADDON_ANCIENTGHOST_NOTNEAR", String, 1, Some(b"C_SVM.ADDON_ANCIENTGHOST_NOTNEAR");
    234 => addon_gold_description: Vec<u8> = Default::default(), b"C_SVM.ADDON_GOLD_DESCRIPTION", String, 1, Some(b"C_SVM.ADDON_GOLD_DESCRIPTION");
    235 => watch_your_aim: Vec<u8> = Default::default(), b"C_SVM.WATCHYOURAIM", String, 1, Some(b"C_SVM.WATCHYOURAIM");
    236 => watch_your_aim_angry: Vec<u8> = Default::default(), b"C_SVM.watchyouraimangry", String, 1, Some(b"C_SVM.watchyouraimangry");
    237 => lets_forget_our_little_fight: Vec<u8> = Default::default(), b"C_SVM.letsforgetourlittlefight", String, 1, Some(b"C_SVM.letsforgetourlittlefight");
    238 => strange: Vec<u8> = Default::default(), b"C_SVM.strange", String, 1, Some(b"C_SVM.strange");
    239 => die_mortal_enemy: Vec<u8> = Default::default(), b"C_SVM.diemortalenemy", String, 1, Some(b"C_SVM.diemortalenemy");
    240 => now_wait: Vec<u8> = Default::default(), b"C_SVM.nowwait", String, 1, Some(b"C_SVM.nowwait");
    241 => now_wait_intruder: Vec<u8> = Default::default(), b"C_SVM.nowwaitintruder", String, 1, Some(b"C_SVM.nowwaitintruder");
    242 => you_still_not_have_enough: Vec<u8> = Default::default(), b"C_SVM.youstillnothaveenough", String, 1, Some(b"C_SVM.youstillnothaveenough");
    243 => you_attacked_my_charge: Vec<u8> = Default::default(), b"C_SVM.youattackedmycharge", String, 1, Some(b"C_SVM.youattackedmycharge");
    244 => iwill_teach_you_respect_for_foreign_property: Vec<u8> = Default::default(), b"C_SVM.iwillteachyourespectforforeignproperty", String, 1, Some(b"C_SVM.iwillteachyourespectforforeignproperty");
    245 => you_killed_one_of_us: Vec<u8> = Default::default(), b"C_SVM.youkilledoneofus", String, 1, Some(b"C_SVM.youkilledoneofus");
    246 => berzerk: Vec<u8> = Default::default(), b"C_SVM.berzerk", String, 1, Some(b"C_SVM.berzerk");
    247 => youll_be_sorry_for_this: Vec<u8> = Default::default(), b"C_SVM.youllbesorryforthis", String, 1, Some(b"C_SVM.youllbesorryforthis");
    248 => yes_yes: Vec<u8> = Default::default(), b"C_SVM.yesyes", String, 1, Some(b"C_SVM.yesyes");
    249 => shit_what_amonster: Vec<u8> = Default::default(), b"C_SVM.shitwhatamonster", String, 1, Some(b"C_SVM.shitwhatamonster");
    250 => we_will_meet_again: Vec<u8> = Default::default(), b"C_SVM.wewillmeetagain", String, 1, Some(b"C_SVM.wewillmeetagain");
    251 => never_try_that_again: Vec<u8> = Default::default(), b"C_SVM.nevertrythatagain", String, 1, Some(b"C_SVM.nevertrythatagain");
    252 => itook_your_ore: Vec<u8> = Default::default(), b"C_SVM.itookyourore", String, 1, Some(b"C_SVM.itookyourore");
    253 => shit_no_ore: Vec<u8> = Default::default(), b"C_SVM.shitnoore", String, 1, Some(b"C_SVM.shitnoore");
    254 => you_violated_forbidden_territory: Vec<u8> = Default::default(), b"C_SVM.youviolatedforbiddenterritory", String, 1, Some(b"C_SVM.youviolatedforbiddenterritory");
    255 => you_wanna_fool_me: Vec<u8> = Default::default(), b"C_SVM.youwannafoolme", String, 1, Some(b"C_SVM.youwannafoolme");
    256 => what_did_you_in_there: Vec<u8> = Default::default(), b"C_SVM.whatdidyouinthere", String, 1, Some(b"C_SVM.whatdidyouinthere");
    257 => intruder_alert: Vec<u8> = Default::default(), b"C_SVM.intruderalert", String, 1, Some(b"C_SVM.intruderalert");
    258 => behind_you: Vec<u8> = Default::default(), b"C_SVM.behindyou", String, 1, Some(b"C_SVM.behindyou");
    259 => hey_hey_hey: Vec<u8> = Default::default(), b"C_SVM.heyheyhey", String, 1, Some(b"C_SVM.heyheyhey");
    260 => cheer_fight: Vec<u8> = Default::default(), b"C_SVM.cheerfight", String, 1, Some(b"C_SVM.cheerfight");
    261 => cheer_friend: Vec<u8> = Default::default(), b"C_SVM.cheerfriend", String, 1, Some(b"C_SVM.cheerfriend");
    262 => ooh: Vec<u8> = Default::default(), b"C_SVM.ooh", String, 1, Some(b"C_SVM.ooh");
    263 => yeah_well_done: Vec<u8> = Default::default(), b"C_SVM.yeahwelldone", String, 1, Some(b"C_SVM.yeahwelldone");
    264 => he_defeatedhim: Vec<u8> = Default::default(), b"C_SVM.hedefeatedhim", String, 1, Some(b"C_SVM.hedefeatedhim");
    265 => he_deserv_edit: Vec<u8> = Default::default(), b"C_SVM.hedeservedit", String, 1, Some(b"C_SVM.hedeservedit");
    266 => he_killed_him: Vec<u8> = Default::default(), b"C_SVM.hekilledhim", String, 1, Some(b"C_SVM.hekilledhim");
    267 => it_was_agood_fight: Vec<u8> = Default::default(), b"C_SVM.itwasagoodfight", String, 1, Some(b"C_SVM.itwasagoodfight");
    268 => friendly_greetings: Vec<u8> = Default::default(), b"C_SVM.friendlygreetings", String, 1, Some(b"C_SVM.friendlygreetings");
    269 => al_greetings: Vec<u8> = Default::default(), b"C_SVM.algreetings", String, 1, Some(b"C_SVM.algreetings");
    270 => mage_greetings: Vec<u8> = Default::default(), b"C_SVM.magegreetings", String, 1, Some(b"C_SVM.magegreetings");
    271 => sect_greetings: Vec<u8> = Default::default(), b"C_SVM.sectgreetings", String, 1, Some(b"C_SVM.sectgreetings");
    272 => there_he_is: Vec<u8> = Default::default(), b"C_SVM.thereheis", String, 1, Some(b"C_SVM.thereheis");
    273 => no_learn_over_max: Vec<u8> = Default::default(), b"C_SVM.nolearnovermax", String, 1, Some(b"C_SVM.nolearnovermax");
    274 => no_learn_you_already_know: Vec<u8> = Default::default(), b"C_SVM.nolearnyoualreadyknow", String, 1, Some(b"C_SVM.nolearnyoualreadyknow");
    275 => hey_you: Vec<u8> = Default::default(), b"C_SVM.heyyou", String, 1, Some(b"C_SVM.heyyou");
    276 => what_do_you_want: Vec<u8> = Default::default(), b"C_SVM.whatdoyouwant", String, 1, Some(b"C_SVM.whatdoyouwant");
    277 => isaid_what_do_you_want: Vec<u8> = Default::default(), b"C_SVM.isaidwhatdoyouwant", String, 1, Some(b"C_SVM.isaidwhatdoyouwant");
    278 => make_way: Vec<u8> = Default::default(), b"C_SVM.makeway", String, 1, Some(b"C_SVM.makeway");
    279 => out_of_my_way: Vec<u8> = Default::default(), b"C_SVM.outofmyway", String, 1, Some(b"C_SVM.outofmyway");
    280 => you_deaf_or_what: Vec<u8> = Default::default(), b"C_SVM.youdeaforwhat", String, 1, Some(b"C_SVM.youdeaforwhat");
    281 => look_away: Vec<u8> = Default::default(), b"C_SVM.lookaway", String, 1, Some(b"C_SVM.lookaway");
    282 => okay_keep_it: Vec<u8> = Default::default(), b"C_SVM.okaykeepit", String, 1, Some(b"C_SVM.okaykeepit");
    283 => whats_that: Vec<u8> = Default::default(), b"C_SVM.whatsthat", String, 1, Some(b"C_SVM.whatsthat");
    284 => thats_my_weapon: Vec<u8> = Default::default(), b"C_SVM.thatsmyweapon", String, 1, Some(b"C_SVM.thatsmyweapon");
    285 => give_it_tome: Vec<u8> = Default::default(), b"C_SVM.giveittome", String, 1, Some(b"C_SVM.giveittome");
    286 => you_can_keep_the_crap: Vec<u8> = Default::default(), b"C_SVM.youcankeepthecrap", String, 1, Some(b"C_SVM.youcankeepthecrap");
    287 => they_killed_my_friend: Vec<u8> = Default::default(), b"C_SVM.theykilledmyfriend", String, 1, Some(b"C_SVM.theykilledmyfriend");
    288 => sucker_got_some: Vec<u8> = Default::default(), b"C_SVM.suckergotsome", String, 1, Some(b"C_SVM.suckergotsome");
    289 => sucker_defeated_ebr: Vec<u8> = Default::default(), b"C_SVM.suckerdefeatedebr", String, 1, Some(b"C_SVM.suckerdefeatedebr");
    290 => sucker_defeated_gur: Vec<u8> = Default::default(), b"C_SVM.suckerdefeatedgur", String, 1, Some(b"C_SVM.suckerdefeatedgur");
    291 => sucker_defeated_mage: Vec<u8> = Default::default(), b"C_SVM.suckerdefeatedmage", String, 1, Some(b"C_SVM.suckerdefeatedmage");
    292 => sucker_defeated_nov_guard: Vec<u8> = Default::default(), b"C_SVM.suckerdefeatednov_guard", String, 1, Some(b"C_SVM.suckerdefeatednov_guard");
    293 => sucker_defeated_vlk_guard: Vec<u8> = Default::default(), b"C_SVM.suckerdefeatedvlk_guard", String, 1, Some(b"C_SVM.suckerdefeatedvlk_guard");
    294 => you_defeated_my_comrade: Vec<u8> = Default::default(), b"C_SVM.youdefeatedmycomrade", String, 1, Some(b"C_SVM.youdefeatedmycomrade");
    295 => you_defeated_nov_guard: Vec<u8> = Default::default(), b"C_SVM.youdefeatednov_guard", String, 1, Some(b"C_SVM.youdefeatednov_guard");
    296 => you_defeated_vlk_guard: Vec<u8> = Default::default(), b"C_SVM.youdefeatedvlk_guard", String, 1, Some(b"C_SVM.youdefeatedvlk_guard");
    297 => you_stole_from_me: Vec<u8> = Default::default(), b"C_SVM.youstolefromme", String, 1, Some(b"C_SVM.youstolefromme");
    298 => you_stole_from_us: Vec<u8> = Default::default(), b"C_SVM.youstolefromus", String, 1, Some(b"C_SVM.youstolefromus");
    299 => you_stole_from_ebr: Vec<u8> = Default::default(), b"C_SVM.youstolefromebr", String, 1, Some(b"C_SVM.youstolefromebr");
    300 => you_stole_from_gur: Vec<u8> = Default::default(), b"C_SVM.youstolefromgur", String, 1, Some(b"C_SVM.youstolefromgur");
    301 => stole_urom_mage: Vec<u8> = Default::default(), b"C_SVM.stolefrommage", String, 1, Some(b"C_SVM.stolefrommage");
    302 => you_killedmyfriend: Vec<u8> = Default::default(), b"C_SVM.youkilledmyfriend", String, 1, Some(b"C_SVM.youkilledmyfriend");
    303 => you_killed_ebr: Vec<u8> = Default::default(), b"C_SVM.youkilledebr", String, 1, Some(b"C_SVM.youkilledebr");
    304 => you_killed_gur: Vec<u8> = Default::default(), b"C_SVM.youkilledgur", String, 1, Some(b"C_SVM.youkilledgur");
    305 => you_killed_mage: Vec<u8> = Default::default(), b"C_SVM.youkilledmage", String, 1, Some(b"C_SVM.youkilledmage");
    306 => you_killed_oc_folk: Vec<u8> = Default::default(), b"C_SVM.youkilledocfolk", String, 1, Some(b"C_SVM.youkilledocfolk");
    307 => you_killed_nc_folk: Vec<u8> = Default::default(), b"C_SVM.youkilledncfolk", String, 1, Some(b"C_SVM.youkilledncfolk");
    308 => you_killed_psi_folk: Vec<u8> = Default::default(), b"C_SVM.youkilledpsifolk", String, 1, Some(b"C_SVM.youkilledpsifolk");
    309 => get_things_right: Vec<u8> = Default::default(), b"C_SVM.getthingsright", String, 1, Some(b"C_SVM.getthingsright");
    310 => you_defeated_me_well: Vec<u8> = Default::default(), b"C_SVM.youdefeatedmewell", String, 1, Some(b"C_SVM.youdefeatedmewell");
    311 => om: Vec<u8> = Default::default(), b"C_SVM.om", String, 1, Some(b"C_SVM.om");
}, {});

define_class!(Menu, b"C_MENU", {
    0 => back_pic: Vec<u8> = Default::default(), b"C_MENU.BACKPIC", String, 1, None;
    1 => back_world: Vec<u8> = Default::default(), b"C_MENU.BACKWORLD", String, 1, None;
    2 => pos_x: i32 = Default::default(), b"C_MENU.POSX", Int, 1, None;
    3 => pos_y: i32 = Default::default(), b"C_MENU.POSY", Int, 1, None;
    4 => dim_x: i32 = Default::default(), b"C_MENU.DIMX", Int, 1, None;
    5 => dim_y: i32 = Default::default(), b"C_MENU.DIMY", Int, 1, None;
    6 => alpha: i32 = Default::default(), b"C_MENU.ALPHA", Int, 1, None;
    7 => music_theme: Vec<u8> = Default::default(), b"C_MENU.MUSICTHEME", String, 1, None;
    8 => event_timer_msec: i32 = Default::default(), b"C_MENU.EVENTTIMERMSEC", Int, 1, None;
    9 => items: [Vec<u8>; 150] = std::array::from_fn(|_| Default::default()), b"C_MENU.ITEMS", String, 150, None;
    10 => flags: MenuFlag = Default::default(), b"C_MENU.FLAGS", Int, 1, None;
    11 => default_outgame: i32 = Default::default(), b"C_MENU.DEFAULTOUTGAME", Int, 1, None;
    12 => default_ingame: i32 = Default::default(), b"C_MENU.DEFAULTINGAME", Int, 1, None;
}, {});

define_class!(MenuItem, b"C_MENU_ITEM", {
    0 => fontname: Vec<u8> = Default::default(), b"C_MENU_ITEM.FONTNAME", String, 1, None;
    1 => text: [Vec<u8>; 10] = std::array::from_fn(|_| Default::default()), b"C_MENU_ITEM.TEXT", String, 10, None;
    2 => backpic: Vec<u8> = Default::default(), b"C_MENU_ITEM.BACKPIC", String, 1, None;
    3 => alphamode: Vec<u8> = Default::default(), b"C_MENU_ITEM.ALPHAMODE", String, 1, None;
    4 => alpha: i32 = Default::default(), b"C_MENU_ITEM.ALPHA", Int, 1, None;
    5 => r#type: MenuItemType = Default::default(), b"C_MENU_ITEM.TYPE", Int, 1, None;
    6 => on_sel_action: [i32; 5] = std::array::from_fn(|_| Default::default()), b"C_MENU_ITEM.ONSELACTION", Int, 5, None;
    7 => on_sel_action_s: [Vec<u8>; 5] = std::array::from_fn(|_| Default::default()), b"C_MENU_ITEM.ONSELACTION_S", String, 5, None;
    8 => on_chg_set_option: Vec<u8> = Default::default(), b"C_MENU_ITEM.ONCHGSETOPTION", String, 1, None;
    9 => on_chg_set_option_section: Vec<u8> = Default::default(), b"C_MENU_ITEM.ONCHGSETOPTIONSECTION", String, 1, None;
    10 => on_event_action: [i32; 10] = std::array::from_fn(|_| Default::default()), b"C_MENU_ITEM.ONEVENTACTION", Int, 10, None;
    11 => pos_x: i32 = Default::default(), b"C_MENU_ITEM.POSX", Int, 1, None;
    12 => pos_y: i32 = Default::default(), b"C_MENU_ITEM.POSY", Int, 1, None;
    13 => dim_x: i32 = Default::default(), b"C_MENU_ITEM.DIMX", Int, 1, None;
    14 => dim_y: i32 = Default::default(), b"C_MENU_ITEM.DIMY", Int, 1, None;
    15 => size_start_scale: f32 = Default::default(), b"C_MENU_ITEM.SIZESTARTSCALE", Float, 1, None;
    16 => flags: MenuItemFlag = Default::default(), b"C_MENU_ITEM.FLAGS", Int, 1, None;
    17 => open_delay_time: f32 = Default::default(), b"C_MENU_ITEM.OPENDELAYTIME", Float, 1, None;
    18 => open_duration: f32 = Default::default(), b"C_MENU_ITEM.OPENDURATION", Float, 1, None;
    19 => user_float: [f32; 4] = std::array::from_fn(|_| Default::default()), b"C_MENU_ITEM.USERFLOAT", Float, 4, None;
    20 => user_string: [Vec<u8>; 4] = std::array::from_fn(|_| Default::default()), b"C_MENU_ITEM.USERSTRING", String, 4, None;
    21 => frame_sizex: i32 = Default::default(), b"C_MENU_ITEM.FRAMESIZEX", Int, 1, None;
    22 => frame_sizey: i32 = Default::default(), b"C_MENU_ITEM.FRAMESIZEY", Int, 1, None;
    23 => frame_posx: i32 = Default::default(), b"C_MENU_ITEM.FRAMEPOSX", Int, 1, Some(b"C_MENU_ITEM.FRAMEPOSX");
    24 => frame_posy: i32 = Default::default(), b"C_MENU_ITEM.FRAMEPOSY", Int, 1, Some(b"C_MENU_ITEM.FRAMEPOSX");
    25 => hide_if_option_section_set: Vec<u8> = Default::default(), b"C_MENU_ITEM.HIDEIFOPTIONSECTIONSET", String, 1, Some(b"C_MENU_ITEM.HIDEIFOPTIONSECTIONSET");
    26 => hide_if_option_set: Vec<u8> = Default::default(), b"C_MENU_ITEM.HIDEIFOPTIONSET", String, 1, Some(b"C_MENU_ITEM.HIDEIFOPTIONSECTIONSET");
    27 => hide_on_value: i32 = Default::default(), b"C_MENU_ITEM.HIDEONVALUE", Int, 1, Some(b"C_MENU_ITEM.HIDEIFOPTIONSECTIONSET");
}, {});

define_class!(Camera, b"CCAMSYS", {
    0 => best_range: f32 = Default::default(), b"CCAMSYS.BESTRANGE", Float, 1, None;
    1 => min_range: f32 = Default::default(), b"CCAMSYS.MINRANGE", Float, 1, None;
    2 => max_range: f32 = Default::default(), b"CCAMSYS.MAXRANGE", Float, 1, None;
    3 => best_elevation: f32 = Default::default(), b"CCAMSYS.BESTELEVATION", Float, 1, None;
    4 => min_elevation: f32 = Default::default(), b"CCAMSYS.MINELEVATION", Float, 1, None;
    5 => max_elevation: f32 = Default::default(), b"CCAMSYS.MAXELEVATION", Float, 1, None;
    6 => best_azimuth: f32 = Default::default(), b"CCAMSYS.BESTAZIMUTH", Float, 1, None;
    7 => min_azimuth: f32 = Default::default(), b"CCAMSYS.MINAZIMUTH", Float, 1, None;
    8 => max_azimuth: f32 = Default::default(), b"CCAMSYS.MAXAZIMUTH", Float, 1, None;
    9 => best_rot_z: f32 = Default::default(), b"CCAMSYS.BESTROTZ", Float, 1, None;
    10 => min_rot_z: f32 = Default::default(), b"CCAMSYS.MINROTZ", Float, 1, None;
    11 => max_rot_z: f32 = Default::default(), b"CCAMSYS.MAXROTZ", Float, 1, None;
    12 => rot_offset_x: f32 = Default::default(), b"CCAMSYS.ROTOFFSETX", Float, 1, None;
    13 => rot_offset_y: f32 = Default::default(), b"CCAMSYS.ROTOFFSETY", Float, 1, None;
    14 => rot_offset_z: f32 = Default::default(), b"CCAMSYS.ROTOFFSETZ", Float, 1, None;
    15 => target_offset_x: f32 = Default::default(), b"CCAMSYS.TARGETOFFSETX", Float, 1, None;
    16 => target_offset_y: f32 = Default::default(), b"CCAMSYS.TARGETOFFSETY", Float, 1, None;
    17 => target_offset_z: f32 = Default::default(), b"CCAMSYS.TARGETOFFSETZ", Float, 1, None;
    18 => velo_trans: f32 = Default::default(), b"CCAMSYS.VELOTRANS", Float, 1, None;
    19 => velo_rot: f32 = Default::default(), b"CCAMSYS.VELOROT", Float, 1, None;
    20 => translate: i32 = Default::default(), b"CCAMSYS.TRANSLATE", Int, 1, None;
    21 => rotate: i32 = Default::default(), b"CCAMSYS.ROTATE", Int, 1, None;
    22 => collision: i32 = Default::default(), b"CCAMSYS.COLLISION", Int, 1, None;
}, {});

define_class!(MusicSystem, b"C_MUSICSYS_CFG", {
    0 => volume: f32 = Default::default(), b"C_MUSICSYS_CFG.VOLUME", Float, 1, None;
    1 => bit_resolution: i32 = Default::default(), b"C_MUSICSYS_CFG.BITRESOLUTION", Int, 1, None;
    2 => global_reverb_enabled: i32 = Default::default(), b"C_MUSICSYS_CFG.GLOBALREVERBENABLED", Int, 1, None;
    3 => sample_rate: i32 = Default::default(), b"C_MUSICSYS_CFG.SAMPLERATE", Int, 1, None;
    4 => num_channels: i32 = Default::default(), b"C_MUSICSYS_CFG.NUMCHANNELS", Int, 1, None;
    5 => reverb_buffer_size: i32 = Default::default(), b"C_MUSICSYS_CFG.REVERBBUFFERSIZE", Int, 1, None;
}, {});

define_class!(MusicTheme, b"C_MUSICTHEME", {
    0 => file: Vec<u8> = Default::default(), b"C_MUSICTHEME.FILE", String, 1, None;
    1 => vol: f32 = Default::default(), b"C_MUSICTHEME.VOL", Float, 1, None;
    2 => r#loop: i32 = Default::default(), b"C_MUSICTHEME.LOOP", Int, 1, None;
    3 => reverbmix: f32 = Default::default(), b"C_MUSICTHEME.REVERBMIX", Float, 1, None;
    4 => reverbtime: f32 = Default::default(), b"C_MUSICTHEME.REVERBTIME", Float, 1, None;
    5 => transtype: MusicTransitionEffect = Default::default(), b"C_MUSICTHEME.TRANSTYPE", Int, 1, None;
    6 => transsubtype: MusicTransitionType = Default::default(), b"C_MUSICTHEME.TRANSSUBTYPE", Int, 1, None;
}, {});

define_class!(MusicJingle, b"C_MUSICJINGLE", {
    0 => name: Vec<u8> = Default::default(), b"C_MUSICJINGLE.NAME", String, 1, None;
    1 => r#loop: i32 = Default::default(), b"C_MUSICJINGLE.LOOP", Int, 1, None;
    2 => vol: f32 = Default::default(), b"C_MUSICJINGLE.VOL", Float, 1, None;
    3 => transsubtype: i32 = Default::default(), b"C_MUSICJINGLE.TRANSSUBTYPE", Int, 1, None;
}, {});

define_class!(ParticleEffect, b"C_PARTICLEFX", {
    0 => pps_value: f32 = Default::default(), b"C_PARTICLEFX.PPSVALUE", Float, 1, None;
    1 => pps_scale_keys_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.PPSSCALEKEYS_S", String, 1, None;
    2 => pps_is_looping: i32 = Default::default(), b"C_PARTICLEFX.PPSISLOOPING", Int, 1, None;
    3 => pps_is_smooth: i32 = Default::default(), b"C_PARTICLEFX.PPSISSMOOTH", Int, 1, None;
    4 => pps_fps: f32 = Default::default(), b"C_PARTICLEFX.PPSFPS", Float, 1, None;
    5 => pps_create_em_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.PPSCREATEEM_S", String, 1, None;
    6 => pps_create_em_delay: f32 = Default::default(), b"C_PARTICLEFX.PPSCREATEEMDELAY", Float, 1, None;
    7 => shp_type_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.SHPTYPE_S", String, 1, None;
    8 => shp_for_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.SHPFOR_S", String, 1, None;
    9 => shp_offset_vec_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.SHPOFFSETVEC_S", String, 1, None;
    10 => shp_distrib_type_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.SHPDISTRIBTYPE_S", String, 1, None;
    11 => shp_distrib_walk_speed: f32 = Default::default(), b"C_PARTICLEFX.SHPDISTRIBWALKSPEED", Float, 1, None;
    12 => shp_is_volume: i32 = Default::default(), b"C_PARTICLEFX.SHPISVOLUME", Int, 1, None;
    13 => shp_dim_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.SHPDIM_S", String, 1, None;
    14 => shp_mesh_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.SHPMESH_S", String, 1, None;
    15 => shp_mesh_render_b: i32 = Default::default(), b"C_PARTICLEFX.SHPMESHRENDER_B", Int, 1, None;
    16 => shp_scale_keys_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.SHPSCALEKEYS_S", String, 1, None;
    17 => shp_scale_is_looping: i32 = Default::default(), b"C_PARTICLEFX.SHPSCALEISLOOPING", Int, 1, None;
    18 => shp_scale_is_smooth: i32 = Default::default(), b"C_PARTICLEFX.SHPSCALEISSMOOTH", Int, 1, None;
    19 => shp_scale_fps: f32 = Default::default(), b"C_PARTICLEFX.SHPSCALEFPS", Float, 1, None;
    20 => dir_mode_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.DIRMODE_S", String, 1, None;
    21 => dir_for_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.DIRFOR_S", String, 1, None;
    22 => dir_mode_target_for_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.DIRMODETARGETFOR_S", String, 1, None;
    23 => dir_mode_target_pos_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.DIRMODETARGETPOS_S", String, 1, None;
    24 => dir_angle_head: f32 = Default::default(), b"C_PARTICLEFX.DIRANGLEHEAD", Float, 1, None;
    25 => dir_angle_head_var: f32 = Default::default(), b"C_PARTICLEFX.DIRANGLEHEADVAR", Float, 1, None;
    26 => dir_angle_elev: f32 = Default::default(), b"C_PARTICLEFX.DIRANGLEELEV", Float, 1, None;
    27 => dir_angle_elev_var: f32 = Default::default(), b"C_PARTICLEFX.DIRANGLEELEVVAR", Float, 1, None;
    28 => vel_avg: f32 = Default::default(), b"C_PARTICLEFX.VELAVG", Float, 1, None;
    29 => vel_var: f32 = Default::default(), b"C_PARTICLEFX.VELVAR", Float, 1, None;
    30 => lsp_part_avg: f32 = Default::default(), b"C_PARTICLEFX.LSPPARTAVG", Float, 1, None;
    31 => lsp_part_var: f32 = Default::default(), b"C_PARTICLEFX.LSPPARTVAR", Float, 1, None;
    32 => fly_gravity_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.FLYGRAVITY_S", String, 1, None;
    33 => fly_colldet_b: i32 = Default::default(), b"C_PARTICLEFX.FLYCOLLDET_B", Int, 1, None;
    34 => vis_name_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.VISNAME_S", String, 1, None;
    35 => vis_orientation_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.VISORIENTATION_S", String, 1, None;
    36 => vis_tex_is_quadpoly: i32 = Default::default(), b"C_PARTICLEFX.VISTEXISQUADPOLY", Int, 1, None;
    37 => vis_tex_ani_fps: f32 = Default::default(), b"C_PARTICLEFX.VISTEXANIFPS", Float, 1, None;
    38 => vis_tex_ani_is_looping: i32 = Default::default(), b"C_PARTICLEFX.VISTEXANIISLOOPING", Int, 1, None;
    39 => vis_tex_color_start_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.VISTEXCOLORSTART_S", String, 1, None;
    40 => vis_tex_color_end_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.VISTEXCOLOREND_S", String, 1, None;
    41 => vis_size_start_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.VISSIZESTART_S", String, 1, None;
    42 => vis_size_end_scale: f32 = Default::default(), b"C_PARTICLEFX.VISSIZEENDSCALE", Float, 1, None;
    43 => vis_alpha_func_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.VISALPHAFUNC_S", String, 1, None;
    44 => vis_alpha_start: f32 = Default::default(), b"C_PARTICLEFX.VISALPHASTART", Float, 1, None;
    45 => vis_alpha_end: f32 = Default::default(), b"C_PARTICLEFX.VISALPHAEND", Float, 1, None;
    46 => trl_fade_speed: f32 = Default::default(), b"C_PARTICLEFX.TRLFADESPEED", Float, 1, None;
    47 => trl_texture_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.TRLTEXTURE_S", String, 1, None;
    48 => trl_width: f32 = Default::default(), b"C_PARTICLEFX.TRLWIDTH", Float, 1, None;
    49 => mrk_fades_peed: f32 = Default::default(), b"C_PARTICLEFX.MRKFADESPEED", Float, 1, None;
    50 => mrkt_exture_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.MRKTEXTURE_S", String, 1, None;
    51 => mrk_size: f32 = Default::default(), b"C_PARTICLEFX.MRKSIZE", Float, 1, None;
    52 => flock_mode: Vec<u8> = Default::default(), b"C_PARTICLEFX.FLOCKMODE", String, 1, Some(b"C_PARTICLEFX.FLOCKMODE");
    53 => flock_strength: f32 = Default::default(), b"C_PARTICLEFX.FLOCKSTRENGTH", Float, 1, Some(b"C_PARTICLEFX.FLOCKMODE");
    54 => use_emitters_for: i32 = Default::default(), b"C_PARTICLEFX.USEEMITTERSFOR", Int, 1, Some(b"C_PARTICLEFX.FLOCKMODE");
    55 => time_start_end_s: Vec<u8> = Default::default(), b"C_PARTICLEFX.TIMESTARTEND_S", String, 1, Some(b"C_PARTICLEFX.FLOCKMODE");
    56 => m_bis_ambient_pfx: i32 = Default::default(), b"C_PARTICLEFX.M_BISAMBIENTPFX", Int, 1, Some(b"C_PARTICLEFX.FLOCKMODE");
}, {});

define_class!(EffectBase, b"CFX_BASE", {
    0 => vis_name_s: Vec<u8> = Default::default(), b"CFX_BASE.VISNAME_S", String, 1, None;
    1 => vis_size_s: Vec<u8> = Default::default(), b"CFX_BASE.VISSIZE_S", String, 1, None;
    2 => vis_alpha: f32 = Default::default(), b"CFX_BASE.VISALPHA", Float, 1, None;
    3 => vis_alpha_blend_func_s: Vec<u8> = Default::default(), b"CFX_BASE.VISALPHABLENDFUNC_S", String, 1, None;
    4 => vis_tex_ani_fps: f32 = Default::default(), b"CFX_BASE.VISTEXANIFPS", Float, 1, None;
    5 => vis_tex_ani_is_looping: i32 = Default::default(), b"CFX_BASE.VISTEXANIISLOOPING", Int, 1, None;
    6 => em_trj_mode_s: Vec<u8> = Default::default(), b"CFX_BASE.EMTRJMODE_S", String, 1, None;
    7 => em_trj_origin_node: Vec<u8> = Default::default(), b"CFX_BASE.EMTRJORIGINNODE", String, 1, None;
    8 => em_trj_target_node: Vec<u8> = Default::default(), b"CFX_BASE.EMTRJTARGETNODE", String, 1, None;
    9 => em_trj_target_range: f32 = Default::default(), b"CFX_BASE.EMTRJTARGETRANGE", Float, 1, None;
    10 => em_trj_target_azi: f32 = Default::default(), b"CFX_BASE.EMTRJTARGETAZI", Float, 1, None;
    11 => em_trj_target_elev: f32 = Default::default(), b"CFX_BASE.EMTRJTARGETELEV", Float, 1, None;
    12 => em_trj_num_keys: i32 = Default::default(), b"CFX_BASE.EMTRJNUMKEYS", Int, 1, None;
    13 => em_trj_num_keys_var: i32 = Default::default(), b"CFX_BASE.EMTRJNUMKEYSVAR", Int, 1, None;
    14 => em_trj_angle_elev_var: f32 = Default::default(), b"CFX_BASE.EMTRJANGLEELEVVAR", Float, 1, None;
    15 => em_trj_angle_head_var: f32 = Default::default(), b"CFX_BASE.EMTRJANGLEHEADVAR", Float, 1, None;
    16 => em_trj_key_dist_var: f32 = Default::default(), b"CFX_BASE.EMTRJKEYDISTVAR", Float, 1, None;
    17 => em_trj_loop_mode_s: Vec<u8> = Default::default(), b"CFX_BASE.EMTRJLOOPMODE_S", String, 1, None;
    18 => em_trj_ease_func_s: Vec<u8> = Default::default(), b"CFX_BASE.EMTRJEASEFUNC_S", String, 1, None;
    19 => em_trj_ease_vel: f32 = Default::default(), b"CFX_BASE.EMTRJEASEVEL", Float, 1, None;
    20 => em_trj_dyn_update_delay: f32 = Default::default(), b"CFX_BASE.EMTRJDYNUPDATEDELAY", Float, 1, None;
    21 => em_trj_dyn_update_target_only: i32 = Default::default(), b"CFX_BASE.EMTRJDYNUPDATETARGETONLY", Int, 1, None;
    22 => em_fx_create_s: Vec<u8> = Default::default(), b"CFX_BASE.EMFXCREATE_S", String, 1, None;
    23 => em_fx_invest_origin_s: Vec<u8> = Default::default(), b"CFX_BASE.EMFXINVESTORIGIN_S", String, 1, None;
    24 => em_fx_invest_target_s: Vec<u8> = Default::default(), b"CFX_BASE.EMFXINVESTTARGET_S", String, 1, None;
    25 => em_fx_trigger_delay: f32 = Default::default(), b"CFX_BASE.EMFXTRIGGERDELAY", Float, 1, None;
    26 => em_fx_create_down_trj: i32 = Default::default(), b"CFX_BASE.EMFXCREATEDOWNTRJ", Int, 1, None;
    27 => em_action_coll_dyn_s: Vec<u8> = Default::default(), b"CFX_BASE.EMACTIONCOLLDYN_S", String, 1, None;
    28 => em_action_coll_stat_s: Vec<u8> = Default::default(), b"CFX_BASE.EMACTIONCOLLSTAT_S", String, 1, None;
    29 => em_fx_coll_stat_s: Vec<u8> = Default::default(), b"CFX_BASE.EMFXCOLLSTAT_S", String, 1, None;
    30 => em_fx_coll_dyn_s: Vec<u8> = Default::default(), b"CFX_BASE.EMFXCOLLDYN_S", String, 1, None;
    31 => em_fx_coll_stat_align_s: Vec<u8> = Default::default(), b"CFX_BASE.EMFXCOLLSTATALIGN_S", String, 1, None;
    32 => em_fx_coll_dyn_align_s: Vec<u8> = Default::default(), b"CFX_BASE.EMFXCOLLDYNALIGN_S", String, 1, None;
    33 => em_fx_lifespan: f32 = Default::default(), b"CFX_BASE.EMFXLIFESPAN", Float, 1, None;
    34 => em_check_collision: i32 = Default::default(), b"CFX_BASE.EMCHECKCOLLISION", Int, 1, None;
    35 => em_adjust_shp_to_origin: i32 = Default::default(), b"CFX_BASE.EMADJUSTSHPTOORIGIN", Int, 1, None;
    36 => em_invest_next_key_duration: f32 = Default::default(), b"CFX_BASE.EMINVESTNEXTKEYDURATION", Float, 1, None;
    37 => em_fly_gravity: f32 = Default::default(), b"CFX_BASE.EMFLYGRAVITY", Float, 1, None;
    38 => em_self_rot_vel_s: Vec<u8> = Default::default(), b"CFX_BASE.EMSELFROTVEL_S", String, 1, None;
    39 => user_string: [Vec<u8>; 5] = std::array::from_fn(|_| Default::default()), b"CFX_BASE.USERSTRING", String, 5, None;
    40 => light_preset_name: Vec<u8> = Default::default(), b"CFX_BASE.LIGHTPRESETNAME", String, 1, None;
    41 => sfx_id: Vec<u8> = Default::default(), b"CFX_BASE.SFXID", String, 1, None;
    42 => sfx_is_ambient: i32 = Default::default(), b"CFX_BASE.SFXISAMBIENT", Int, 1, None;
    43 => send_assess_magic: i32 = Default::default(), b"CFX_BASE.SENDASSESSMAGIC", Int, 1, None;
    44 => secs_per_damage: f32 = Default::default(), b"CFX_BASE.SECSPERDAMAGE", Float, 1, None;
    45 => em_fx_coll_dyn_perc_s: Vec<u8> = Default::default(), b"CFX_BASE.EMFXCOLLDYNPERC_S", String, 1, Some(b"CFX_BASE.EMFXCOLLDYNPERC_S");
}, {});

define_class!(ParticleEffectEmitKey, b"C_PARTICLEFXEMITKEY", {
    0 => vis_name_s: Vec<u8> = Default::default(), b"C_PARTICLEFXEMITKEY.VISNAME_S", String, 1, None;
    1 => vis_size_scale: f32 = Default::default(), b"C_PARTICLEFXEMITKEY.VISSIZESCALE", Float, 1, None;
    2 => scale_duration: f32 = Default::default(), b"C_PARTICLEFXEMITKEY.SCALEDURATION", Float, 1, None;
    3 => pfx_pps_value: f32 = Default::default(), b"C_PARTICLEFXEMITKEY.PFX_PPSVALUE", Float, 1, None;
    4 => pfx_pps_is_smooth_chg: i32 = Default::default(), b"C_PARTICLEFXEMITKEY.PFX_PPSISSMOOTHCHG", Int, 1, None;
    5 => pfx_pps_is_looping_chg: i32 = Default::default(), b"C_PARTICLEFXEMITKEY.PFX_PPSISLOOPINGCHG", Int, 1, None;
    6 => pfx_sc_time: f32 = Default::default(), b"C_PARTICLEFXEMITKEY.PFX_SCTIME", Float, 1, None;
    7 => pfx_fly_gravity_s: Vec<u8> = Default::default(), b"C_PARTICLEFXEMITKEY.PFX_FLYGRAVITY_S", String, 1, None;
    8 => pfx_shp_dim_s: Vec<u8> = Default::default(), b"C_PARTICLEFXEMITKEY.PFX_SHPDIM_S", String, 1, None;
    9 => pfx_shp_is_volume_chg: i32 = Default::default(), b"C_PARTICLEFXEMITKEY.PFX_SHPISVOLUMECHG", Int, 1, None;
    10 => pfx_shp_scale_fps: f32 = Default::default(), b"C_PARTICLEFXEMITKEY.PFX_SHPSCALEFPS", Float, 1, None;
    11 => pfx_shp_distrib_walks_peed: f32 = Default::default(), b"C_PARTICLEFXEMITKEY.PFX_SHPDISTRIBWALKSPEED", Float, 1, None;
    12 => pfx_shp_offset_vec_s: Vec<u8> = Default::default(), b"C_PARTICLEFXEMITKEY.PFX_SHPOFFSETVEC_S", String, 1, None;
    13 => pfx_shp_distrib_type_s: Vec<u8> = Default::default(), b"C_PARTICLEFXEMITKEY.PFX_SHPDISTRIBTYPE_S", String, 1, None;
    14 => pfx_dir_mode_s: Vec<u8> = Default::default(), b"C_PARTICLEFXEMITKEY.PFX_DIRMODE_S", String, 1, None;
    15 => pfx_dir_for_s: Vec<u8> = Default::default(), b"C_PARTICLEFXEMITKEY.PFX_DIRFOR_S", String, 1, None;
    16 => pfx_dir_mode_target_for_s: Vec<u8> = Default::default(), b"C_PARTICLEFXEMITKEY.PFX_DIRMODETARGETFOR_S", String, 1, None;
    17 => pfx_dir_mode_target_pos_s: Vec<u8> = Default::default(), b"C_PARTICLEFXEMITKEY.PFX_DIRMODETARGETPOS_S", String, 1, None;
    18 => pfx_vel_avg: f32 = Default::default(), b"C_PARTICLEFXEMITKEY.PFX_VELAVG", Float, 1, None;
    19 => pfx_lsp_part_avg: f32 = Default::default(), b"C_PARTICLEFXEMITKEY.PFX_LSPPARTAVG", Float, 1, None;
    20 => pfx_vis_alpha_start: f32 = Default::default(), b"C_PARTICLEFXEMITKEY.PFX_VISALPHASTART", Float, 1, None;
    21 => light_preset_name: Vec<u8> = Default::default(), b"C_PARTICLEFXEMITKEY.LIGHTPRESETNAME", String, 1, None;
    22 => light_range: f32 = Default::default(), b"C_PARTICLEFXEMITKEY.LIGHTRANGE", Float, 1, None;
    23 => sfx_id: Vec<u8> = Default::default(), b"C_PARTICLEFXEMITKEY.SFXID", String, 1, None;
    24 => sfx_is_ambient: i32 = Default::default(), b"C_PARTICLEFXEMITKEY.SFXISAMBIENT", Int, 1, None;
    25 => em_create_fx_id: Vec<u8> = Default::default(), b"C_PARTICLEFXEMITKEY.EMCREATEFXID", String, 1, None;
    26 => em_fly_gravity: f32 = Default::default(), b"C_PARTICLEFXEMITKEY.EMFLYGRAVITY", Float, 1, None;
    27 => em_self_rot_vel_s: Vec<u8> = Default::default(), b"C_PARTICLEFXEMITKEY.EMSELFROTVEL_S", String, 1, None;
    28 => em_trj_mode_s: Vec<u8> = Default::default(), b"C_PARTICLEFXEMITKEY.EMTRJMODE_S", String, 1, None;
    29 => em_trj_ease_vel: f32 = Default::default(), b"C_PARTICLEFXEMITKEY.EMTRJEASEVEL", Float, 1, None;
    30 => em_check_collision: i32 = Default::default(), b"C_PARTICLEFXEMITKEY.EMCHECKCOLLISION", Int, 1, None;
    31 => em_fx_lifespan: f32 = Default::default(), b"C_PARTICLEFXEMITKEY.EMFXLIFESPAN", Float, 1, None;
}, {});

define_class!(FightAi, b"C_FIGHTAI", {
    0 => r#move: [FightAiMove; 6] = std::array::from_fn(|_| Default::default()), b"C_FIGHTAI.MOVE", Int, 6, None;
}, {});

define_class!(SoundEffect, b"C_SFX", {
    0 => file: Vec<u8> = Default::default(), b"C_SFX.FILE", String, 1, None;
    1 => pitch_off: i32 = Default::default(), b"C_SFX.PITCHOFF", Int, 1, None;
    2 => pitch_var: i32 = Default::default(), b"C_SFX.PITCHVAR", Int, 1, None;
    3 => vol: i32 = Default::default(), b"C_SFX.VOL", Int, 1, None;
    4 => r#loop: i32 = Default::default(), b"C_SFX.LOOP", Int, 1, None;
    5 => loop_start_offset: i32 = Default::default(), b"C_SFX.LOOPSTARTOFFSET", Int, 1, None;
    6 => loop_end_offset: i32 = Default::default(), b"C_SFX.LOOPENDOFFSET", Int, 1, None;
    7 => reverb_level: f32 = Default::default(), b"C_SFX.REVERBLEVEL", Float, 1, None;
    8 => pfx_name: Vec<u8> = Default::default(), b"C_SFX.PFXNAME", String, 1, None;
}, {});

define_class!(SoundSystem, b"C_SNDSYS_CFG", {
    0 => volume: f32 = Default::default(), b"C_SNDSYS_CFG.VOLUME", Float, 1, None;
    1 => bit_resolution: i32 = Default::default(), b"C_SNDSYS_CFG.BITRESOLUTION", Int, 1, None;
    2 => sample_rate: i32 = Default::default(), b"C_SNDSYS_CFG.SAMPLERATE", Int, 1, None;
    3 => use_stereo: i32 = Default::default(), b"C_SNDSYS_CFG.USESTEREO", Int, 1, None;
    4 => num_sfx_channels: i32 = Default::default(), b"C_SNDSYS_CFG.NUMSFXCHANNELS", Int, 1, None;
    5 => used_3d_provider_name: Vec<u8> = Default::default(), b"C_SNDSYS_CFG.USED3DPROVIDERNAME", String, 1, None;
}, {});

pub const CLASSES: &[&ClassSchema] = &[
    GuildValues::SCHEMA,
    Npc::SCHEMA,
    Mission::SCHEMA,
    Item::SCHEMA,
    Focus::SCHEMA,
    Info::SCHEMA,
    ItemReact::SCHEMA,
    Spell::SCHEMA,
    Svm::SCHEMA,
    Menu::SCHEMA,
    MenuItem::SCHEMA,
    Camera::SCHEMA,
    MusicSystem::SCHEMA,
    MusicTheme::SCHEMA,
    MusicJingle::SCHEMA,
    ParticleEffect::SCHEMA,
    EffectBase::SCHEMA,
    ParticleEffectEmitKey::SCHEMA,
    FightAi::SCHEMA,
    SoundEffect::SCHEMA,
    SoundSystem::SCHEMA,
];

/// Validate every present built-in class. Absent classes are skipped; a present
/// class with missing required members fails the entire returned collection.
pub fn bind_all_present(script: &Script, max_members: usize) -> Result<Vec<ClassBinding>> {
    bind_all_present_detailed(script, max_members).map_err(BindingError::into_error)
}
/// The same atomic validation with typed binding failure context.
pub fn bind_all_present_detailed(
    script: &Script,
    max_members: usize,
) -> BindingResult<Vec<ClassBinding>> {
    type Binder = fn(&Script, usize) -> BindingResult<Option<ClassBinding>>;
    let binders: &[Binder] = &[
        ClassBinding::bind_present_detailed::<GuildValues>,
        ClassBinding::bind_present_detailed::<Npc>,
        ClassBinding::bind_present_detailed::<Mission>,
        ClassBinding::bind_present_detailed::<Item>,
        ClassBinding::bind_present_detailed::<Focus>,
        ClassBinding::bind_present_detailed::<Info>,
        ClassBinding::bind_present_detailed::<ItemReact>,
        ClassBinding::bind_present_detailed::<Spell>,
        ClassBinding::bind_present_detailed::<Svm>,
        ClassBinding::bind_present_detailed::<Menu>,
        ClassBinding::bind_present_detailed::<MenuItem>,
        ClassBinding::bind_present_detailed::<Camera>,
        ClassBinding::bind_present_detailed::<MusicSystem>,
        ClassBinding::bind_present_detailed::<MusicTheme>,
        ClassBinding::bind_present_detailed::<MusicJingle>,
        ClassBinding::bind_present_detailed::<ParticleEffect>,
        ClassBinding::bind_present_detailed::<EffectBase>,
        ClassBinding::bind_present_detailed::<ParticleEffectEmitKey>,
        ClassBinding::bind_present_detailed::<FightAi>,
        ClassBinding::bind_present_detailed::<SoundEffect>,
        ClassBinding::bind_present_detailed::<SoundSystem>,
    ];
    let mut result = Vec::new();
    result.try_reserve_exact(binders.len()).map_err(|_| {
        BindingError::new(
            error(ErrorKind::LimitExceeded),
            BindingDiagnostic::ClassAllocation {
                classes: binders.len(),
            },
        )
    })?;
    let mut remaining = max_members;
    for bind in binders {
        if let Some(binding) = bind(script, remaining)? {
            remaining -= binding.members().len();
            result.push(binding);
        }
    }
    Ok(result)
}
