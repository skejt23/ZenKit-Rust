// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2022-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT

use super::*;

macro_rules! raw_word {
    ($name:ident { $($constant:ident = $value:expr,)* }) => {
        /// A raw script enum word. Unknown values are preserved.
        #[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
        pub struct $name(u32);
        impl $name {
            $(pub const $constant: Self = Self($value);)*
            pub const fn from_raw(value: u32) -> Self { Self(value) }
            pub const fn raw(self) -> u32 { self.0 }
        }
        impl Cell for $name {
            fn read(&self) -> MemberValue<'_> { MemberValue::Int(self.0 as i32) }
            fn write(&mut self, value: MemberValue<'_>, _: usize) -> Result<()> {
                let MemberValue::Int(value) = value else { return Err(error(ErrorKind::WrongObjectType)); };
                self.0 = value as u32;
                Ok(())
            }
        }
        scalar_field!($name);
    };
}
macro_rules! flags {
    ($name:ident) => {
        impl $name {
            /// Whether any bits overlap.
            pub const fn intersects(self, other: Self) -> bool {
                self.0 & other.0 != 0
            }
            pub const fn contains(self, other: Self) -> bool {
                self.0 & other.0 == other.0
            }
        }
        impl std::ops::BitOr for $name {
            type Output = Self;
            fn bitor(self, rhs: Self) -> Self {
                Self(self.0 | rhs.0)
            }
        }
        impl std::ops::BitAnd for $name {
            type Output = Self;
            fn bitand(self, rhs: Self) -> Self {
                Self(self.0 & rhs.0)
            }
        }
    };
}

raw_word!(NpcType {
    G1_AMBIENT = 0,
    G1_MAIN = 1,
    G1_GUARD = 2,
    G1_FRIEND = 3,
    G1_MINE_AMBIENT = 4,
    G1_MINE_GUARD = 5,
    G1_OW_AMBIENT = 6,
    G1_OW_GUARD = 7,
    G1_ROGUE = 8,
    G2_AMBIENT = 0,
    G2_MAIN = 1,
    G2_FRIEND = 2,
    G2_OC_AMBIENT = 3,
    G2_OC_MAIN = 4,
    G2_BL_AMBIENT = 5,
    G2_TAL_AMBIENT = 6,
    G2_BL_MAIN = 7,
    AMBIENT = Self::G2_AMBIENT.0,
    MAIN = Self::G2_MAIN.0,
});

raw_word!(NpcFlag {
    NONE = 0,
    FRIENDS = 1 << 0,
    IMMORTAL = 1 << 1,
    GHOST = 1 << 2,
    PROTECTED = 1 << 3,
});

raw_word!(ItemFlag {
    DAGGER = 1 << 13,
    SWORD = 1 << 14,
    AXE = 1 << 15,
    TWO_HANDED_SWORD = 1 << 16,
    TWO_HANDED_AXE = 1 << 17,
    BOW = 1 << 19,
    CROSSBOW = 1 << 20,
    AMULET = 1 << 22,
    RING = 1 << 11,
    BELT = 1 << 24,
    MISSION = 1 << 12,
});

raw_word!(MenuFlag {
    OVERTOP = 1 << 0,
    EXCLUSIVE = 1 << 1,
    NO_ANIMATION = 1 << 2,
    DONT_SCALE_DIMENSION = 1 << 3,
    DONT_SCALE_POSITION = 1 << 4,
    ALIGN_CENTER = 1 << 5,
    SHOW_INFO = 1 << 6,
});

raw_word!(MenuItemFlag {
    CHROMAKEYED = 1 << 0,
    TRANSPARENT = 1 << 1,
    SELECTABLE = 1 << 2,
    MOVABLE = 1 << 3,
    CENTERED = 1 << 4,
    DISABLED = 1 << 5,
    FADE = 1 << 6,
    EFFECTS = 1 << 7,
    ONLY_OUTGAME = 1 << 8,
    ONLY_INGAME = 1 << 9,
    PERF_OPTION = 1 << 10,
    MULTILINE = 1 << 11,
    NEEDS_APPLY = 1 << 12,
    NEEDS_RESTART = 1 << 13,
    EXTENDED_MENU = 1 << 14,
    HOR_SELECTABLE = 1 << 15,
});

raw_word!(MenuItemType {
    UNKNOWN = 0,
    TEXT = 1,
    SLIDER = 2,
    INPUT = 3,
    CURSOR = 4,
    CHOICEBOX = 5,
    BUTTON = 6,
    LISTBOX = 7,
});

raw_word!(MenuItemSelectEvent {
    EXECUTE = 1,
    CHANGED = 2,
    LEAVE = 3,
    TIMER = 4,
    CLOSE = 5,
    INIT = 6,
    SELECT_PREVIOUS = 7,
    SELECT_NEXT = 8,
});

raw_word!(MenuItemSelectAction {
    UNKNOWN = 0,
    BACK = 1,
    START_MENU = 2,
    START_ITEM = 3,
    CLOSE = 4,
    CON_COMMANDS = 5,
    PLAY_SOUND = 6,
    EXECUTE_COMMANDS = 7,
});

raw_word!(MusicTransitionEffect {
    UNKNOWN = 0,
    NONE = 1,
    GROOVE = 2,
    FILL = 3,
    BREAK = 4,
    INTRO = 5,
    END = 6,
    END_AND_INTO = 7,
});

raw_word!(MusicTransitionType {
    UNKNOWN = 0,
    IMMEDIATE = 1,
    BEAT = 2,
    MEASURE = 3,
});

raw_word!(FightAiMove {
    NOP = 0,
    RUN = 1,
    RUN_BACK = 2,
    JUMP_BACK = 3,
    TURN = 4,
    STRAFE = 5,
    ATTACK = 6,
    ATTACK_SIDE = 7,
    ATTACK_FRONT = 8,
    ATTACK_TRIPLE = 9,
    ATTACK_WHIRL = 10,
    ATTACK_MASTER = 11,
    TURN_TO_HIT = 15,
    PARRY = 17,
    STAND_UP = 18,
    WAIT = 19,
    WAIT_LONGER = 23,
    WAIT_EXT = 24,
});

flags!(NpcFlag);
flags!(ItemFlag);
flags!(MenuFlag);
flags!(MenuItemFlag);

pub mod damage_type {
    pub const BARRIER: usize = 0;
    pub const BLUNT: usize = 1;
    pub const EDGE: usize = 2;
    pub const FIRE: usize = 3;
    pub const FLY: usize = 4;
    pub const MAGIC: usize = 5;
    pub const POINT: usize = 6;
    pub const FALL: usize = 7;
    pub const NUM: usize = 8;
}

pub mod npc_attribute {
    pub const HITPOINTS: usize = 0;
    pub const HITPOINTS_MAX: usize = 1;
    pub const MANA: usize = 2;
    pub const MANA_MAX: usize = 3;
    pub const STRENGTH: usize = 4;
    pub const DEXTERITY: usize = 5;
    pub const REGENERATE_HP: usize = 6;
    pub const REGENERATE_MANA: usize = 7;
    pub const NUM: usize = 8;
}
