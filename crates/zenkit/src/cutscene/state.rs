// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;
use crate::objects::CutsceneBlock;

/// Complete version-zero `zCCSProps` payload. All fields are present in both
/// games and normal/save archives. Unknown behavior integers are retained.
#[derive(Clone, Default, PartialEq)]
pub struct Props {
    pub name: Vec<u8>,
    pub global: bool,
    pub looped: bool,
    pub has_to_be_triggered: bool,
    pub distance: f32,
    pub range: f32,
    pub locked_block_count: i32,
    pub run_behavior: u32,
    pub run_behavior_value: i32,
    pub stage_name: Vec<u8>,
    pub script_function_on_stop: Vec<u8>,
}
impl fmt::Debug for Props {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Props")
            .field("name_bytes", &self.name.len())
            .field("global", &self.global)
            .field("looped", &self.looped)
            .field("has_to_be_triggered", &self.has_to_be_triggered)
            .field("distance", &self.distance)
            .field("range", &self.range)
            .field("locked_block_count", &self.locked_block_count)
            .field("run_behavior", &self.run_behavior)
            .field("run_behavior_value", &self.run_behavior_value)
            .field("stage_name_bytes", &self.stage_name.len())
            .field("stop_function_bytes", &self.script_function_on_stop.len())
            .finish()
    }
}
/// Cutscene context with its complete inherited Block and unconditional suffix.
/// Role counts are opaque signed scalar values, not lengths of inferred arrays.
/// `props` is a nullable Props occurrence; `npc` and `main_role` are nullable
/// exact NPC occurrences. Stage links may target any supported object, including
/// this context. The block child still requires an exact Atomic/Block node.
#[derive(Clone, Debug, PartialEq)]
pub struct Context {
    pub block: CutsceneBlock,
    pub props: Link,
    pub role_count: i32,
    pub role_vob_count: i32,
    pub npc: Link,
    pub main_role: Link,
    pub is_cutscene: bool,
    pub reference: i32,
    pub actual_block: i32,
    pub alt_stage: Link,
    pub stage: Link,
    pub was_triggered: bool,
}
impl Default for Context {
    /// Empty editable draft with allocation-free empty occurrence names. Its
    /// required Block child must be supplied before validation/serialization.
    fn default() -> Self {
        let link = || Link::new(Vec::new(), None);
        Self {
            block: CutsceneBlock {
                name: Vec::new(),
                sub_block0: 0.,
                child: link(),
            },
            props: link(),
            role_count: 0,
            role_vob_count: 0,
            npc: link(),
            main_role: link(),
            is_cutscene: false,
            reference: 0,
            actual_block: 0,
            alt_stage: link(),
            stage: link(),
            was_triggered: false,
        }
    }
}
impl Context {
    pub(crate) fn links(&self) -> [&Link; 6] {
        [
            &self.block.child,
            &self.props,
            &self.npc,
            &self.main_role,
            &self.alt_stage,
            &self.stage,
        ]
    }
    pub(crate) fn links_mut(&mut self) -> [&mut Link; 6] {
        [
            &mut self.block.child,
            &mut self.props,
            &mut self.npc,
            &mut self.main_role,
            &mut self.alt_stage,
            &mut self.stage,
        ]
    }
}
/// Ordered nullable exact Context occurrences. Count derives from `playlists`,
/// bounded by i32 and cumulative graph links; no separate sequence-item charge.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Player {
    pub last_process_day: i32,
    pub last_process_hour: i32,
    pub playlists: Vec<Link>,
}
impl fmt::Debug for Player {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Player")
            .field("last_process_day", &self.last_process_day)
            .field("last_process_hour", &self.last_process_hour)
            .field("playlists", &self.playlists.len())
            .finish()
    }
}
/// Complete pool state. `deactivated`, flags and counters remain signed integers,
/// rather than being narrowed to booleans or constrained to positive values.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct PoolItem {
    pub name: Vec<u8>,
    pub run_behavior: u32,
    pub run_behavior_value: i32,
    pub times_played: i32,
    pub deactivated: i32,
    pub flags: i32,
}
impl PoolItem {
    pub const ALWAYS: u32 = 0;
    pub const TIMES: u32 = 1;
    pub const PER_HOUR: u32 = 2;
}
impl fmt::Debug for PoolItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PoolItem")
            .field("name_bytes", &self.name.len())
            .field("run_behavior", &self.run_behavior)
            .field("run_behavior_value", &self.run_behavior_value)
            .field("times_played", &self.times_played)
            .field("deactivated", &self.deactivated)
            .field("flags", &self.flags)
            .finish()
    }
}
/// Ordered nullable exact PoolItem occurrences, with names and aliases retained.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Manager {
    pub pool_items: Vec<Link>,
}
impl fmt::Debug for Manager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Manager")
            .field("pool_items", &self.pool_items.len())
            .finish()
    }
}
