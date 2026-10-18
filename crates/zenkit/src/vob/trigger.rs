// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;
use crate::animation::Sample;

macro_rules! flags {
    ($name:ident, $(($get:ident,$set:ident,$mask:expr)),+ $(,)?) => {
        /// Authoritative wire bits. Named setters preserve all other/reserved bits.
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub struct $name(u8);
        impl $name {
            pub const fn from_raw(value:u8)->Self { Self(value) }
            pub const fn raw(self)->u8 {self.0}
            $(pub const fn $get(self)->bool {self.0 & $mask != 0}
            pub fn $set(&mut self,value:bool) {self.0 = (self.0 & !$mask) | if value {$mask} else {0};})+
        }
        impl From<u8> for $name { fn from(value:u8)->Self {Self(value)} }
        impl From<$name> for u8 { fn from(value:$name)->Self {value.0} }
    }
}
flags!(
    TriggerFlags,
    (start_enabled, set_start_enabled, 1),
    (send_untrigger, set_send_untrigger, 4)
);
flags!(
    TriggerFilter,
    (on_trigger, set_on_trigger, 1),
    (on_touch, set_on_touch, 2),
    (on_damage, set_on_damage, 4),
    (object, set_object, 8),
    (player, set_player, 16),
    (npc, set_npc, 32)
);

#[derive(Clone, Debug, PartialEq)]
/// Present exactly in save archives. `other` is a nullable VOB occurrence;
/// aliases/back references retain graph identity. Counters remain uninterpreted,
/// including negative sentinels. G1 has no serialized `enabled` field.
pub struct TriggerState {
    pub next_time: f32,
    pub other: Link,
    pub activation_count: i32,
    /// Present only in G2 save records.
    pub enabled: Option<bool>,
}
impl TriggerState {
    pub fn new(game: GameVersion) -> Self {
        Self {
            next_time: 0.,
            other: Link::new(b"savedOtherVob".to_vec(), None),
            activation_count: 0,
            enabled: (game == GameVersion::Gothic2).then_some(true),
        }
    }
}
#[derive(Clone, Default, PartialEq)]
/// Common trigger fields. Flags are authoritative raw bits; named setters retain
/// reserved bits. Tagged flag-record tails survive unchanged; conversion to
/// unframed BINARY rejects them rather than silently truncating them.
pub struct Trigger {
    pub target: Vec<u8>,
    pub flags: TriggerFlags,
    pub flags_tail: Vec<u8>,
    pub filter: TriggerFilter,
    pub filter_tail: Vec<u8>,
    pub vob_target: Vec<u8>,
    pub max_activation_count: i32,
    pub retrigger_delay: f32,
    pub damage_threshold: f32,
    pub fire_delay: f32,
    pub state: Option<TriggerState>,
}
impl fmt::Debug for Trigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Trigger")
            .field("target_bytes", &self.target.len())
            .field("flags", &self.flags)
            .field("flags_tail_bytes", &self.flags_tail.len())
            .field("filter", &self.filter)
            .field("filter_tail_bytes", &self.filter_tail.len())
            .field("vob_target_bytes", &self.vob_target.len())
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}
/// Present exactly when at least one keyframe is stored. All sample float bits
/// survive; rotations use the shared WXYZ value API and XYZW wire ordering.
#[derive(Clone, PartialEq)]
pub struct MoverMotion {
    pub speed: f32,
    pub lerp: MoverLerp,
    pub speed_mode: MoverSpeed,
    pub samples: Vec<Sample>,
    /// Tagged bytes beyond the declared sample array; unsupported in BINARY output.
    pub tail: Vec<u8>,
}
impl fmt::Debug for MoverMotion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MoverMotion")
            .field("speed", &self.speed)
            .field("lerp", &self.lerp)
            .field("speed_mode", &self.speed_mode)
            .field("samples", &self.samples.len())
            .field("tail_bytes", &self.tail.len())
            .finish()
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MoverState {
    pub key_position_delta: Vec3,
    pub keyframe_fraction: f32,
    pub keyframe: i32,
    pub next_keyframe: i32,
    pub speed_unit: f32,
    pub advance_direction: f32,
    pub state: u32,
    pub trigger_events: i32,
    pub stay_open_until: f32,
}
#[derive(Clone, Default, PartialEq, Eq)]
pub struct MoverSounds {
    pub open_start: Vec<u8>,
    pub open_end: Vec<u8>,
    pub moving: Vec<u8>,
    pub close_start: Vec<u8>,
    pub close_end: Vec<u8>,
    pub lock: Vec<u8>,
    pub unlock: Vec<u8>,
    pub use_locked: Vec<u8>,
}
impl MoverSounds {
    pub(crate) fn buffers(&self) -> [&[u8]; 8] {
        [
            &self.open_start,
            &self.open_end,
            &self.moving,
            &self.close_start,
            &self.close_end,
            &self.lock,
            &self.unlock,
            &self.use_locked,
        ]
    }
}
impl fmt::Debug for MoverSounds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MoverSounds")
            .field("byte_lengths", &self.buffers().map(<[u8]>::len))
            .finish()
    }
}
#[derive(Clone, Debug, PartialEq)]
/// Complete mover payload. `motion=None` means a zero wire frame count and no
/// speed/interpolation fields; `Some` requires 1..65535 samples. Dynamic array
/// entries share `objects::Limits::sequence_items`. G2 alone has `auto_rotate`.
pub struct Mover {
    pub trigger: Trigger,
    pub behavior: MoverBehavior,
    pub touch_damage: f32,
    pub stay_open: f32,
    pub locked: bool,
    pub auto_link: bool,
    pub auto_rotate: Option<bool>,
    pub motion: Option<MoverMotion>,
    pub state: Option<MoverState>,
    pub sounds: MoverSounds,
}
impl Mover {
    pub fn new(game: GameVersion) -> Self {
        Self {
            trigger: Trigger::default(),
            behavior: MoverBehavior::TOGGLE,
            touch_damage: 0.,
            stay_open: 0.,
            locked: true,
            auto_link: false,
            auto_rotate: (game == GameVersion::Gothic2).then_some(false),
            motion: None,
            state: None,
            sounds: MoverSounds::default(),
        }
    }
}
#[derive(Clone, Default, PartialEq)]
pub struct TriggerTarget {
    pub name: Vec<u8>,
    pub delay: f32,
}
impl fmt::Debug for TriggerTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TriggerTarget")
            .field("name_bytes", &self.name.len())
            .field("delay", &self.delay)
            .finish()
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TriggerListState {
    pub active_target: u8,
    pub send_on_trigger: bool,
}
#[derive(Clone, Default, PartialEq)]
/// Ordered targets, with at most 255 entries. Saved active-target values remain
/// raw and need not be valid current indices. Canonical output uses `numTarget`;
/// `numTargets` is also accepted on input.
pub struct TriggerList {
    pub trigger: Trigger,
    pub mode: TriggerBatch,
    pub targets: Vec<TriggerTarget>,
    pub state: Option<TriggerListState>,
}
impl fmt::Debug for TriggerList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TriggerList")
            .field("trigger", &self.trigger)
            .field("mode", &self.mode)
            .field("targets", &self.targets.len())
            .field("state", &self.state)
            .finish()
    }
}
#[derive(Clone, Default, PartialEq)]
pub struct TriggerScript {
    pub trigger: Trigger,
    pub function: Vec<u8>,
}
#[derive(Clone, Default, PartialEq)]
pub struct TriggerChangeLevel {
    pub trigger: Trigger,
    pub level: Vec<u8>,
    pub start_vob: Vec<u8>,
}
#[derive(Clone, Default, PartialEq, Eq)]
pub struct TriggerWorldStart {
    pub target: Vec<u8>,
    pub fire_once: bool,
    pub has_fired: Option<bool>,
}
#[derive(Clone, Default, PartialEq, Eq)]
pub struct TriggerUntouch {
    pub target: Vec<u8>,
}
macro_rules! small_debug {($ty:ident,$($field:ident),+)=>{impl fmt::Debug for $ty {
    fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result {f.debug_struct(stringify!($ty))
        $(.field(concat!(stringify!($field),"_bytes"),&self.$field.len()))+.finish_non_exhaustive()}
}}}
small_debug!(TriggerScript, function);
small_debug!(TriggerChangeLevel, level, start_vob);
small_debug!(TriggerWorldStart, target);
small_debug!(TriggerUntouch, target);
/// G2 save storage. The counter is independent of the number of occurrences.
/// Each slave name has one occurrence with an uninterpreted target meaning.
/// Any supported object kind is retained.
#[derive(Clone, Default, PartialEq)]
pub struct CodeMasterState {
    pub triggered_count: u8,
    pub triggered: Vec<Link>,
}
impl fmt::Debug for CodeMasterState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodeMasterState")
            .field("triggered_count", &self.triggered_count)
            .field("occurrences", &self.triggered.len())
            .finish()
    }
}
#[derive(Clone, Default, PartialEq)]
/// At most 255 slave names, including empty names. State is present exactly in
/// G2 saves, with one named occurrence per slave, preserved without guessing
/// activation semantics or restricting targets to a fabricated class category.
pub struct CodeMaster {
    pub target: Vec<u8>,
    pub ordered: bool,
    pub first_false_is_failure: bool,
    pub failure_target: Vec<u8>,
    pub untrigger_cancels: bool,
    pub slaves: Vec<Vec<u8>>,
    pub state: Option<CodeMasterState>,
}
impl fmt::Debug for CodeMaster {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodeMaster")
            .field("target_bytes", &self.target.len())
            .field("failure_target_bytes", &self.failure_target.len())
            .field("slaves", &self.slaves.len())
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl VobKind {
    pub fn trigger(&self) -> Option<&Trigger> {
        self.as_ref().trigger()
    }
    pub fn trigger_mut(&mut self) -> Option<&mut Trigger> {
        match self {
            Self::Trigger(v) | Self::CutsceneTrigger(v) => Some(v),
            Self::Mover(v) => Some(&mut v.trigger),
            Self::TriggerList(v) => Some(&mut v.trigger),
            Self::TriggerScript(v) => Some(&mut v.trigger),
            Self::TriggerChangeLevel(v) => Some(&mut v.trigger),
            _ => None,
        }
    }
}

impl<'a> VobKindRef<'a> {
    pub fn trigger(self) -> Option<&'a Trigger> {
        match self {
            Self::Trigger(v) | Self::CutsceneTrigger(v) => Some(v),
            Self::Mover(v) => Some(&v.trigger),
            Self::TriggerList(v) => Some(&v.trigger),
            Self::TriggerScript(v) => Some(&v.trigger),
            Self::TriggerChangeLevel(v) => Some(&v.trigger),
            _ => None,
        }
    }
}

impl VobKindMut<'_> {
    pub fn trigger_mut(&mut self) -> Option<&mut Trigger> {
        match self {
            Self::Trigger(v) | Self::CutsceneTrigger(v) => Some(v),
            Self::Mover(v) => Some(&mut v.trigger),
            Self::TriggerList(v) => Some(&mut v.trigger),
            Self::TriggerScript(v) => Some(&mut v.trigger),
            Self::TriggerChangeLevel(v) => Some(&mut v.trigger),
            _ => None,
        }
    }
}

impl<'a> VobKindRef<'a> {
    pub(crate) fn sequence_items(self) -> usize {
        match self {
            Self::Mover(v) => v.motion.as_ref().map_or(0, |m| m.samples.len()),
            Self::TriggerList(v) => v.targets.len(),
            Self::CodeMaster(v) => v.slaves.len(),
            Self::Npc(n) => n
                .overlays
                .len()
                .saturating_add(n.news.len())
                .saturating_add(n.slots.len()),
            _ => 0,
        }
    }
}

impl<'a> VobKindRef<'a> {
    pub(crate) fn late_links(self) -> impl Iterator<Item = &'a Link> {
        let other = self
            .trigger()
            .and_then(|v| v.state.as_ref())
            .map(|s| &s.other);
        let slaves = match self {
            Self::CodeMaster(v) => v.state.as_ref().map_or(&[][..], |s| s.triggered.as_slice()),
            Self::Container(v) => v.items.as_deref().unwrap_or(&[]),
            _ => &[],
        };
        other.into_iter().chain(slaves)
    }
}

impl VobKindMut<'_> {
    pub(crate) fn late_links_mut(&mut self) -> impl Iterator<Item = &mut Link> {
        let (other, slaves, targets, npc) = match self {
            Self::Npc(n) => (None, &mut [][..], &mut [][..], Some(&mut **n)),
            Self::Camera(v) => (
                None,
                v.positions.as_mut_slice(),
                v.targets.as_mut_slice(),
                None,
            ),
            Self::Container(v) => (
                None,
                v.items.as_deref_mut().unwrap_or(&mut []),
                &mut [][..],
                None,
            ),
            Self::CodeMaster(v) => (
                None,
                v.state
                    .as_mut()
                    .map_or(&mut [][..], |s| s.triggered.as_mut_slice()),
                &mut [][..],
                None,
            ),
            v => (
                v.trigger_mut()
                    .and_then(|v| v.state.as_mut())
                    .map(|s| &mut s.other),
                &mut [][..],
                &mut [][..],
                None,
            ),
        };
        other
            .into_iter()
            .chain(slaves)
            .chain(targets)
            .chain(npc.into_iter().flat_map(NpcData::links_mut))
    }
}

impl<'a> VobKindRef<'a> {
    pub(crate) fn trigger_buffers(self) -> impl Iterator<Item = &'a [u8]> {
        let mut a = [None; 16];
        if let Some(t) = self.trigger() {
            a[..4].copy_from_slice(&[
                Some(t.target.as_slice()),
                Some(&t.flags_tail),
                Some(&t.filter_tail),
                Some(&t.vob_target),
            ]);
        }
        match self {
            Self::Mover(v) => {
                for (slot, sound) in a[4..12].iter_mut().zip(v.sounds.buffers()) {
                    *slot = Some(sound);
                }
                a[12] = v.motion.as_ref().map(|m| m.tail.as_slice());
            }
            Self::TriggerScript(v) => a[4] = Some(&v.function),
            Self::TriggerChangeLevel(v) => {
                a[4] = Some(&v.level);
                a[5] = Some(&v.start_vob);
            }
            Self::TriggerWorldStart(v) => a[0] = Some(&v.target),
            Self::TriggerUntouch(v) => a[0] = Some(&v.target),
            Self::CodeMaster(v) => {
                a[0] = Some(&v.target);
                a[1] = Some(&v.failure_target);
            }
            _ => {}
        }
        let targets = match self {
            Self::TriggerList(v) => v.targets.as_slice(),
            _ => &[],
        };
        let slaves = match self {
            Self::CodeMaster(v) => v.slaves.as_slice(),
            _ => &[],
        };
        a.into_iter()
            .flatten()
            .chain(targets.iter().map(|v| v.name.as_slice()))
            .chain(slaves.iter().map(Vec::as_slice))
    }
}

impl<'a> VobKindRef<'a> {
    pub(crate) fn check_trigger(self, game: GameVersion, save: bool) -> Result<()> {
        if let Some(t) = self.trigger() {
            if t.state.is_some() != save {
                return Err(bad("trigger state must match save mode"));
            }
            if t.state
                .as_ref()
                .is_some_and(|s| s.enabled.is_some() != (game == GameVersion::Gothic2))
            {
                return Err(bad("trigger enabled game mismatch"));
            }
        }
        match self {
            Self::Mover(v) => {
                if v.auto_rotate.is_some() != (game == GameVersion::Gothic2)
                    || v.state.is_some() != save
                {
                    return Err(bad("mover game/save state"));
                }
                if v.motion
                    .as_ref()
                    .is_some_and(|m| m.samples.is_empty() || m.samples.len() > u16::MAX as usize)
                {
                    return Err(bad("mover motion requires 1..65535 frames"));
                }
            }
            Self::TriggerList(v) => {
                if v.targets.len() > 255 || v.state.is_some() != save {
                    return Err(bad("trigger-list count/save state"));
                }
            }
            Self::TriggerWorldStart(v) => {
                if v.has_fired.is_some() != (save && game == GameVersion::Gothic2) {
                    return Err(bad("world-start game/save state"));
                }
            }
            Self::CodeMaster(v) => {
                if v.slaves.len() > 255
                    || v.state.is_some() != (save && game == GameVersion::Gothic2)
                {
                    return Err(bad("code-master count/save state"));
                }
                if v.state
                    .as_ref()
                    .is_some_and(|s| s.triggered.len() != v.slaves.len())
                {
                    return Err(bad("code-master occurrences must match slave names"));
                }
            }
            _ => {}
        }
        Ok(())
    }
}
