//! VOBs, visuals, AI and event-manager records.
//!
//! Editable records in [`crate::objects::ObjectArchive`]. World child lists are
//! stored separately in [`crate::world`].
//! Optional names and links retain packed absence versus present empty/null.
//! A visual's own decal name never aliases the containing VOB's `visual_name`.
//! [`VobKind`] determines the subtype's class and version. Subtype fields follow
//! the base record and its save suffix. Save state is required in saves and
//! forbidden in normal archives; callers must add or remove it when converting.
//!
//! [`LightPreset`] is shared by lights and standalone field streams. Raw range
//! and color animation text survives even when malformed; decoding is explicit
//! and validates the entire string before returning an allocation-free view.
//! [`ScreenState`] retains the opaque save fields and both FOV pairs, including
//! extra tagged bytes that cannot be emitted into unframed BINARY fields.
//! Buffers share the graph's copied-byte budget. Validate drafts before writing.
//! Container save items are ordered named links to Items; absent normal state
//! and an empty saved list remain distinct. Boolean wire values normalize to
//! the semantic Bool representation. Cutscene cameras retain two ordered
//! nullable Frame-only lists and their trailing G2 save state. Camera frames
//! preserve version metadata, raw matrix bits and tagged pose tails.
//! NPC and AI links share graph identity. Saved inventory depends on Item flags:
//! tagged archives resolve unfinished ancestors by exact field-name lookahead;
//! BINARY resolves that dependency using the known Item schema's bounded fixed
//! flags suffix, then validates the full parse.
//!
//! Packed G2 animation, bias and ambient use bits 8-9, 10-14 and 15 respectively.
//! Unknown unpacked enum integers and packed reserved bits survive;
//! unrepresentable conversions fail explicitly.
//!
//! ```
//! use zenkit::{GameVersion, archive::Format, objects::*, vob::*};
//! let mut graph = Graph::new()?;
//! let visual = graph.insert(Object::Visual(Visual::new(VisualKind::Mesh)))?;
//! let mut vob = Vob::new(GameVersion::Gothic2);
//! vob.base.visual_name = Some(b"ROCK.3DS".to_vec());
//! vob.base.visual = Some(Link::new(b"visual".to_vec(), Some(visual)));
//! let id = graph.insert(Object::Vob(vob))?;
//! graph.add_root(Link::new(b"root".to_vec(), Some(id)))?;
//! let archive = ObjectArchive { metadata: Metadata::new(Format::BinSafe),
//!     game: GameVersion::Gothic2, graph };
//! let bytes = archive.to_bytes(Limits::default())?;
//! assert_eq!(ObjectArchive::from_bytes(&bytes, archive.game, Limits::default())?
//!     .graph.len(), 2);
//! # Ok::<(), zenkit::Error>(())
//! ```
// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
mod camera;
mod camera_codec;
mod codec;
mod light;
mod mob;
mod mob_codec;
mod npc;
pub(crate) mod npc_codec;
mod special;
mod special_codec;
pub(crate) mod storage;
mod trigger;
mod trigger_codec;
use crate::{
    Error, ErrorKind, GameVersion, Result,
    material::AlphaFunction,
    objects::Link,
    values::{Aabb, Mat3, Vec2, Vec3},
};
pub use camera::*;
pub(crate) use codec::{check_object, read_prefix, read_suffix, write_prefix, write_suffix};
pub use light::*;
pub use mob::*;
pub use npc::*;
pub use special::*;
use std::fmt;
pub use trigger::*;
pub(crate) use trigger_codec::{Phase, ReadBudget, read_phase, write_phase};

fn bad(context: &'static str) -> Error {
    Error::operation(ErrorKind::InvalidData, context)
}
macro_rules! raw_enum {
    ($name:ident { $($constant:ident = $value:expr),+ $(,)? }) => {
        /// Raw archive enum integer; unknown values remain distinguishable.
        #[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
        pub struct $name(u32);
        impl $name {
            $(pub const $constant: Self = Self($value);)+
            pub const fn from_raw(value: u32) -> Self { Self(value) }
            pub const fn raw(self) -> u32 { self.0 }
        }
        impl From<u32> for $name { fn from(value: u32) -> Self { Self(value) } }
        impl From<$name> for u32 { fn from(value: $name) -> Self { value.0 } }
    }
}
raw_enum!(CameraAlignment { NONE=0, YAW=1, FULL=2 });
raw_enum!(Shadow { NONE=0, BLOB=1 });
raw_enum!(AnimationMode { NONE=0, WIND=1, WIND2=2 });
raw_enum!(MessageAction { NONE=0, TRIGGER=1, UNTRIGGER=2, ENABLE=3, DISABLE=4, TOGGLE=5 });
raw_enum!(MoverMessage { FIXED_DIRECT=0, FIXED_ORDER=1, NEXT=2, PREVIOUS=3 });
raw_enum!(TouchCollision { NONE=0, BOX=1, POINT=2 });
raw_enum!(SoundMode { LOOP=0, ONCE=1, RANDOM=2 });
raw_enum!(SoundVolume { SPHERICAL=0, ELLIPSOIDAL=1 });
raw_enum!(LightType { POINT=0, SPOT=1, RESERVED0=2, RESERVED1=3 });
raw_enum!(LightQuality { HIGH=0, MEDIUM=1, LOW=2 });
raw_enum!(MoverBehavior { TOGGLE=0, TRIGGER_CONTROL=1, OPEN_TIME=2, LOOP=3, SINGLE_KEYS=4 });
raw_enum!(MoverLerp { LINEAR=0, CURVE=1 });
raw_enum!(MoverSpeed { CONSTANT=0, SLOW_START_END=1, SLOW_START=2, SLOW_END=3, SEGMENT_SLOW_START_END=4, SEGMENT_SLOW_START=5, SEGMENT_SLOW_END=6 });
raw_enum!(TriggerBatch { ALL=0, NEXT=1, RANDOM=2 });
raw_enum!(SoundMaterial { WOOD=0, STONE=1, METAL=2, LEATHER=3, CLAY=4, GLASS=5 });
raw_enum!(CameraMotion { UNDEFINED=0, SMOOTH=1, LINEAR=2, STEP=3, SLOW=4, FAST=5, CUSTOM=6 });
raw_enum!(CameraCoordinate { WORLD=0, OBJECT=1 });
raw_enum!(CameraLoop { NONE=0, RESTART=1, PINGPONG=2 });
raw_enum!(CameraLerp { UNDEFINED=0, PATH=1, PATH_IGNORE_ROLL=2, PATH_ROTATION_SAMPLES=3 });

/// Typed VOB payload, determining the exact archive class and version.
/// Base VOB fields and identity remain shared; subtype fields follow the complete
/// base payload, including its save-state suffix. Unknown classes are rejected.
#[derive(Clone, Debug, Default, PartialEq)]
// Graph stores subtypes separately, so large NPCs do not enlarge other records.
#[allow(clippy::large_enum_variant)]
pub enum VobKind {
    #[default]
    Base,
    Spot,
    Stair,
    StartPoint,
    Level,
    Animate(Animate),
    Item(Item),
    LensFlare(LensFlare),
    PfxController(PfxController),
    MessageFilter(MessageFilter),
    MoverController(MoverController),
    TouchDamage(TouchDamage),
    Earthquake(Earthquake),
    ScreenEffect(ScreenEffect),
    ZoneMusic(ZoneMusic),
    ZoneMusicDefault(ZoneMusic),
    ZoneFarPlane(ZoneFarPlane),
    ZoneFarPlaneDefault(ZoneFarPlane),
    ZoneFog(ZoneFog),
    ZoneFogDefault(ZoneFog),
    Sound(Sound),
    SoundDaytime(SoundDaytime),
    Light(LightPreset),
    Trigger(Trigger),
    CutsceneTrigger(Trigger),
    Mover(Mover),
    TriggerList(TriggerList),
    TriggerScript(TriggerScript),
    TriggerChangeLevel(TriggerChangeLevel),
    TriggerWorldStart(TriggerWorldStart),
    TriggerUntouch(TriggerUntouch),
    CodeMaster(CodeMaster),
    Movable(Movable),
    Interactive(Interactive),
    Fire(Fire),
    Container(Container),
    Door(Door),
    Ladder(Interactive),
    Switch(Interactive),
    Wheel(Interactive),
    Bed(Interactive),
    Camera(Camera),
    CameraFrame(CameraFrame),
    Npc(NpcData),
}
impl VobKind {
    pub const fn class_name(&self) -> &'static [u8] {
        self.as_ref().class_name()
    }

    pub const fn version_identifier(&self, game: GameVersion) -> u16 {
        self.as_ref().version_identifier(game)
    }
}

/// Serialized layout. `marker` is the original nonzero `pack` discriminator.
/// `reserved` holds only bit 7 in both games, plus bit 5 in normal archives
/// (where it does not announce an event-manager record). Known bits are derived
/// from the other fields. Tagged `dataRaw` tails are retained verbatim; BINARY
/// cannot delimit them and serialization to BINARY rejects a nonempty tail.
#[derive(Clone, PartialEq, Eq)]
pub enum Packing {
    Unpacked,
    Packed {
        marker: i32,
        reserved: u16,
        tail: Vec<u8>,
    },
}
impl Default for Packing {
    fn default() -> Self {
        Self::Packed {
            marker: 1,
            reserved: 0,
            tail: Vec::new(),
        }
    }
}
impl fmt::Debug for Packing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unpacked => f.write_str("Unpacked"),
            Self::Packed {
                marker,
                reserved,
                tail,
            } => f
                .debug_struct("Packed")
                .field("marker", marker)
                .field("reserved", reserved)
                .field("tail_bytes", &tail.len())
                .finish(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gothic2Vob {
    pub animation: AnimationMode,
    pub strength: f32,
    pub far_clip: f32,
    pub bias: i32,
    pub ambient: bool,
}
impl Default for Gothic2Vob {
    fn default() -> Self {
        Self {
            animation: AnimationMode::NONE,
            strength: 0.,
            far_clip: 2.,
            bias: 0,
            ambient: false,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VobVersion {
    Gothic1,
    Gothic2(Gothic2Vob),
}
/// Fixed-size physics fields, also readable and writable inside an open archive.
/// The field layout is identical for Gothic 1/2 and normal/save archive headers.
/// A containing VOB decides whether these fields are present; standalone use
/// imposes no VOB physics-flag or save-mode requirement.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RigidBody {
    pub velocity: Vec3,
    pub mode: u8,
    pub gravity: bool,
    pub gravity_scale: f32,
    pub slide: Vec3,
}
impl RigidBody {
    /// Read the five inline fields at the current archive position. No object
    /// framing is consumed. Tagged field names and types must match the schema.
    /// Archive limits govern the reads; the returned value owns no allocation.
    /// On error the archive may be partially consumed: discard the reader.
    pub fn read(reader: &mut crate::archive::Reader<'_>) -> Result<Self> {
        let mut copied = 0;
        codec::read_rigid_body(&mut codec::Fields {
            r: reader,
            copied: &mut copied,
            limit: 0,
        })
    }

    /// Write the five inline fields at the current archive position. No object
    /// framing is emitted. Archive limits and float representation rules apply;
    /// decimal ASCII cannot retain arbitrary NaN payload bits.
    /// On error discard the partially written archive writer.
    ///
    /// ```
    /// use zenkit::{archive::{self, Format, Header, Value}, vob::RigidBody};
    /// let limits = archive::Limits::default();
    /// let mut writer = archive::Writer::new(Header::new(Format::BinSafe), limits)?;
    /// writer.write(b"before", Value::Int(17))?;
    /// let body = RigidBody { mode: 255, gravity: true, ..RigidBody::default() };
    /// body.write(&mut writer)?;
    /// writer.write(b"after", Value::Int(29))?;
    /// let bytes = writer.finish()?;
    /// let mut reader = archive::Reader::new(&bytes, limits)?;
    /// assert_eq!(reader.read_int()?, 17);
    /// assert_eq!(RigidBody::read(&mut reader)?, body);
    /// assert_eq!(reader.read_int()?, 29);
    /// reader.finish()?;
    /// # Ok::<(), zenkit::Error>(())
    /// ```
    pub fn write(&self, writer: &mut crate::archive::Writer) -> Result<()> {
        codec::write_rigid_body(writer, self)
    }
}
/// Present exactly in save archives. A body is present exactly when the VOB's
/// physics flag is true (including the unpacked layout's implicit true flag).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SaveState {
    pub sleep: u8,
    pub next_timer: f32,
    pub rigid_body: Option<RigidBody>,
}
#[derive(Clone, PartialEq)]
pub struct VobBase {
    pub version: VobVersion,
    pub packing: Packing,
    pub bbox: Aabb,
    pub position: Vec3,
    pub rotation: Mat3,
    pub show_visual: bool,
    pub camera_alignment: CameraAlignment,
    pub collision_static: bool,
    pub collision_dynamic: bool,
    pub static_vob: bool,
    pub shadow: Shadow,
    /// Explicit packed flag; implicitly true in unpacked records. Normal
    /// archives retain true even though they have no rigid-body save payload.
    pub physics_enabled: bool,
    pub preset: Option<Vec<u8>>,
    pub name: Option<Vec<u8>>,
    pub visual_name: Option<Vec<u8>>,
    pub visual: Option<Link>,
    pub ai: Option<Link>,
    pub event_manager: Option<Link>,
    /// Absent in normal archives; required in save archives.
    pub state: Option<SaveState>,
}

#[derive(Clone, PartialEq)]
pub struct Vob {
    pub base: VobBase,
    pub kind: VobKind,
}
impl Vob {
    /// Construct a packed normal-archive base VOB. Save mode requires explicitly
    /// adding `state`; unpacked mode requires all names and visual/AI occurrences.
    pub fn new(game: GameVersion) -> Self {
        Self {
            base: VobBase::new(game),
            kind: VobKind::Base,
        }
    }
    pub const fn game(&self) -> GameVersion {
        match self.base.version {
            VobVersion::Gothic1 => GameVersion::Gothic1,
            VobVersion::Gothic2(_) => GameVersion::Gothic2,
        }
    }
    pub const fn version_identifier(&self) -> u16 {
        self.kind.version_identifier(self.game())
    }

    /// The two packed flag words regenerated from the explicit representation.
    /// Returns `None` for unpacked layout. Checks this VOB's layout, state,
    /// reserved bits and representable ranges before shifting. Graph-wide
    /// handle kinds, archive game/mode and output budgets are checked separately.
    pub fn packed_flags(&self) -> Result<Option<(u8, u16)>> {
        self.as_ref().packed_flags()
    }
}
impl fmt::Debug for Vob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Vob")
            .field("kind", &self.kind)
            .field("version", &self.base.version)
            .field("packing", &self.base.packing)
            .field("preset_bytes", &self.base.preset.as_ref().map(Vec::len))
            .field("name_bytes", &self.base.name.as_ref().map(Vec::len))
            .field(
                "visual_name_bytes",
                &self.base.visual_name.as_ref().map(Vec::len),
            )
            .field("visual", &self.base.visual)
            .field("ai", &self.base.ai)
            .field("event_manager", &self.base.event_manager)
            .field("state", &self.base.state)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Gothic2Decal {
    pub alpha_weight: u8,
    pub ignore_daylight: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecalVersion {
    Gothic1,
    Gothic2(Gothic2Decal),
}
#[derive(Clone, PartialEq)]
pub struct Decal {
    pub name: Vec<u8>,
    pub dimension: Vec2,
    pub offset: Vec2,
    pub two_sided: bool,
    pub alpha: AlphaFunction,
    pub fps: f32,
    pub version: DecalVersion,
}
impl Decal {
    pub(crate) const fn version_supported(version: u16, game: GameVersion) -> bool {
        version == 0 || (matches!(game, GameVersion::Gothic2) && matches!(version, 53505 | 64513))
    }
    pub fn new(game: GameVersion) -> Self {
        Self {
            name: Vec::new(),
            dimension: Vec2::ZERO,
            offset: Vec2::ZERO,
            two_sided: false,
            alpha: AlphaFunction::DEFAULT,
            fps: 0.,
            version: match game {
                GameVersion::Gothic1 => DecalVersion::Gothic1,
                GameVersion::Gothic2 => DecalVersion::Gothic2(Gothic2Decal::default()),
            },
        }
    }
    pub const fn game(&self) -> GameVersion {
        match self.version {
            DecalVersion::Gothic1 => GameVersion::Gothic1,
            DecalVersion::Gothic2(_) => GameVersion::Gothic2,
        }
    }
}
impl fmt::Debug for Decal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Decal")
            .field("name_bytes", &self.name.len())
            .field("dimension", &self.dimension)
            .field("offset", &self.offset)
            .field("two_sided", &self.two_sided)
            .field("alpha", &self.alpha)
            .field("fps", &self.fps)
            .field("version", &self.version)
            .finish()
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum VisualKind {
    Decal(Decal),
    Mesh,
    ProgressiveMesh,
    ParticleFx,
    Camera,
    Model,
    MorphMesh,
}
impl VisualKind {
    pub const fn class_name(&self) -> &'static [u8] {
        match self {
            Self::Decal(_) => b"zCDecal",
            Self::Mesh => b"zCMesh",
            Self::ProgressiveMesh => b"zCProgMeshProto",
            Self::ParticleFx => b"zCParticleFX",
            Self::Camera => b"zCAICamera",
            Self::Model => b"zCModel",
            Self::MorphMesh => b"zCMorphMesh",
        }
    }
}
/// Empty visual schemas retain their otherwise uninterpreted version identifier.
/// Decals accept version 0 and the complete Gothic 2 corpus schemas 53505 (saved
/// worlds) and 64513 (compiled worlds), preserving the selected identifier.
/// Runtime-only name/type members are not serialized as wire payload.
#[derive(Clone, Debug, PartialEq)]
pub struct Visual {
    pub version: u16,
    pub kind: VisualKind,
}
impl Visual {
    pub const fn new(kind: VisualKind) -> Self {
        Self { version: 0, kind }
    }
}
/// `cutscene` is a nullable exact CutsceneContext occurrence. Its raw name and
/// shared/cyclic identity are retained by the common object graph.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventManager {
    pub cleared: bool,
    pub active: bool,
    pub cutscene: Link,
}
/// VOB links can form self/ancestor cycles. A nonnull owner targets an NPC.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AiMove {
    pub vob: Link,
    pub owner: Link,
}
/// Full AI-human scalar record with a nullable, exact NPC dependency.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AiHuman {
    pub water_level: i32,
    pub floor_y: f32,
    pub water_y: f32,
    pub ceil_y: f32,
    pub feet_y: f32,
    pub head_y: f32,
    pub fall_dist_y: f32,
    pub fall_start_y: f32,
    pub npc: Link,
    pub walk_mode: i32,
    pub weapon_mode: i32,
    pub weapon_mode_last: i32,
    pub weapon_mode_select: i32,
    pub change_weapon: bool,
    pub action_mode: i32,
}
pub(crate) const EVENT_MANAGER: &[u8] = b"zCEventManager";
pub(crate) const AI_MOVE: &[u8] = b"oCAIVobMove";
pub(crate) const AI_HUMAN: &[u8] = b"oCAIHuman:oCAniCtrl_Human:zCAIPlayer";

impl<'a> VobKindRef<'a> {
    pub const fn class_name(self) -> &'static [u8] {
        match self {
            Self::Base => b"zCVob",
            Self::Spot => b"zCVobSpot:zCVob",
            Self::Stair => b"zCVobStair:zCVob",
            Self::StartPoint => b"zCVobStartpoint:zCVob",
            Self::Level => b"zCVobLevelCompo:zCVob",
            Self::Animate(_) => b"zCVobAnimate:zCVob",
            Self::Item(_) => b"oCItem:zCVob",
            Self::LensFlare(_) => b"zCVobLensFlare:zCVob",
            Self::PfxController(_) => b"zCPFXControler:zCVob",
            Self::MessageFilter(_) => b"zCMessageFilter:zCVob",
            Self::MoverController(_) => b"zCMoverControler:zCVob",
            Self::TouchDamage(_) => b"oCTouchDamage:zCTouchDamage:zCVob",
            Self::Earthquake(_) => b"zCEarthquake:zCVob",
            Self::ScreenEffect(_) => b"zCVobScreenFX:zCVob",
            Self::ZoneMusic(_) => b"oCZoneMusic:zCVob",
            Self::ZoneMusicDefault(_) => b"oCZoneMusicDefault:oCZoneMusic:zCVob",
            Self::ZoneFarPlane(_) => b"zCZoneVobFarPlane:zCVob",
            Self::ZoneFarPlaneDefault(_) => b"zCZoneVobFarPlaneDefault:zCZoneVobFarPlane:zCVob",
            Self::ZoneFog(_) => b"zCZoneZFog:zCVob",
            Self::ZoneFogDefault(_) => b"zCZoneZFogDefault:zCZoneZFog:zCVob",
            Self::Sound(_) => b"zCVobSound:zCVob",
            Self::SoundDaytime(_) => b"zCVobSoundDaytime:zCVobSound:zCVob",
            Self::Light(_) => b"zCVobLight:zCVob",
            Self::Trigger(_) => b"zCTrigger:zCVob",
            Self::CutsceneTrigger(_) => b"oCCSTrigger:zCTrigger:zCVob",
            Self::Mover(_) => b"zCMover:zCTrigger:zCVob",
            Self::TriggerList(_) => b"zCTriggerList:zCTrigger:zCVob",
            Self::TriggerScript(_) => b"oCTriggerScript:zCTrigger:zCVob",
            Self::TriggerChangeLevel(_) => b"oCTriggerChangeLevel:zCTrigger:zCVob",
            Self::TriggerWorldStart(_) => b"zCTriggerWorldStart:zCVob",
            Self::TriggerUntouch(_) => b"zCTriggerUntouch:zCVob",
            Self::CodeMaster(_) => b"zCCodeMaster:zCVob",
            Self::Movable(_) => b"oCMOB:zCVob",
            Self::Interactive(_) => b"oCMobInter:oCMOB:zCVob",
            Self::Fire(_) => b"oCMobFire:oCMobInter:oCMOB:zCVob",
            Self::Container(_) => b"oCMobContainer:oCMobInter:oCMOB:zCVob",
            Self::Door(_) => b"oCMobDoor:oCMobInter:oCMOB:zCVob",
            Self::Ladder(_) => b"oCMobLadder:oCMobInter:oCMOB:zCVob",
            Self::Switch(_) => b"oCMobSwitch:oCMobInter:oCMOB:zCVob",
            Self::Wheel(_) => b"oCMobWheel:oCMobInter:oCMOB:zCVob",
            Self::Bed(_) => b"oCMobBed:oCMobInter:oCMOB:zCVob",
            Self::Camera(_) => b"zCCSCamera:zCVob",
            Self::CameraFrame(_) => b"zCCamTrj_KeyFrame:zCVob",
            Self::Npc(_) => b"oCNpc:zCVob",
        }
    }
}

impl<'a> VobKindRef<'a> {
    pub const fn version_identifier(self, game: GameVersion) -> u16 {
        match self {
            Self::Trigger(_) => 47105,
            Self::CutsceneTrigger(_) | Self::TriggerScript(_) | Self::TriggerChangeLevel(_) => {
                24577
            }
            Self::Mover(_) => 39936,
            Self::TriggerList(_) => {
                if matches!(game, GameVersion::Gothic1) {
                    59776
                } else {
                    5505
                }
            }
            Self::TriggerWorldStart(_) => 52224,
            Self::TriggerUntouch(_) => 193,
            Self::CodeMaster(_) => 0,
            Self::Movable(_) => 47105,
            Self::Interactive(_) | Self::Ladder(_) | Self::Switch(_) | Self::Wheel(_) => 35585,
            Self::Fire(_) => 18433,
            Self::Container(_) | Self::Door(_) => 64513,
            Self::Bed(_) => 61440,
            Self::Camera(_) => {
                if matches!(game, GameVersion::Gothic1) {
                    30720
                } else {
                    33793
                }
            }
            Self::CameraFrame(v) => v.version,
            Self::Npc(_) => 52224,
            Self::Item(_)
            | Self::MessageFilter(_)
            | Self::MoverController(_)
            | Self::ZoneMusic(_)
            | Self::ZoneMusicDefault(_) => 0,
            Self::TouchDamage(_) => 36865,
            Self::Earthquake(_) => 52224,
            Self::Sound(_) | Self::SoundDaytime(_) => 12289,
            Self::LensFlare(_) => match game {
                GameVersion::Gothic1 => 64704,
                GameVersion::Gothic2 => 193,
            },
            Self::ZoneFog(_) | Self::ZoneFogDefault(_) => match game {
                GameVersion::Gothic1 => 64704,
                GameVersion::Gothic2 => 5505,
            },
            Self::Light(_) => match game {
                GameVersion::Gothic1 => 46080,
                GameVersion::Gothic2 => 39168,
            },
            _ => match game {
                GameVersion::Gothic1 => 12289,
                GameVersion::Gothic2 => 52224,
            },
        }
    }
}

impl VobBase {
    pub fn new(game: GameVersion) -> Self {
        Self {
            version: match game {
                GameVersion::Gothic1 => VobVersion::Gothic1,
                GameVersion::Gothic2 => VobVersion::Gothic2(Gothic2Vob::default()),
            },
            packing: Packing::default(),
            bbox: Aabb::default(),
            position: Vec3::ZERO,
            rotation: Mat3::IDENTITY,
            show_visual: false,
            camera_alignment: CameraAlignment::NONE,
            collision_static: true,
            collision_dynamic: true,
            static_vob: false,
            shadow: Shadow::NONE,
            physics_enabled: true,
            preset: None,
            name: None,
            visual_name: None,
            visual: None,
            ai: None,
            event_manager: None,
            state: None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VobKindRef<'a> {
    Base,
    Spot,
    Stair,
    StartPoint,
    Level,
    Animate(&'a Animate),
    Item(&'a Item),
    LensFlare(&'a LensFlare),
    PfxController(&'a PfxController),
    MessageFilter(&'a MessageFilter),
    MoverController(&'a MoverController),
    TouchDamage(&'a TouchDamage),
    Earthquake(&'a Earthquake),
    ScreenEffect(&'a ScreenEffect),
    ZoneMusic(&'a ZoneMusic),
    ZoneMusicDefault(&'a ZoneMusic),
    ZoneFarPlane(&'a ZoneFarPlane),
    ZoneFarPlaneDefault(&'a ZoneFarPlane),
    ZoneFog(&'a ZoneFog),
    ZoneFogDefault(&'a ZoneFog),
    Sound(&'a Sound),
    SoundDaytime(&'a SoundDaytime),
    Light(&'a LightPreset),
    Trigger(&'a Trigger),
    CutsceneTrigger(&'a Trigger),
    Mover(&'a Mover),
    TriggerList(&'a TriggerList),
    TriggerScript(&'a TriggerScript),
    TriggerChangeLevel(&'a TriggerChangeLevel),
    TriggerWorldStart(&'a TriggerWorldStart),
    TriggerUntouch(&'a TriggerUntouch),
    CodeMaster(&'a CodeMaster),
    Movable(&'a Movable),
    Interactive(&'a Interactive),
    Fire(&'a Fire),
    Container(&'a Container),
    Door(&'a Door),
    Ladder(&'a Interactive),
    Switch(&'a Interactive),
    Wheel(&'a Interactive),
    Bed(&'a Interactive),
    Camera(&'a Camera),
    CameraFrame(&'a CameraFrame),
    Npc(&'a NpcData),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VobRef<'a> {
    pub(crate) base: &'a VobBase,
    pub(crate) kind: VobKindRef<'a>,
}
impl<'a> VobRef<'a> {
    /// Borrow the common fields.
    #[inline]
    pub fn base(self) -> &'a VobBase {
        self.base
    }
    /// Inspect the stored subtype and borrow its fields.
    #[inline]
    pub fn kind(self) -> VobKindRef<'a> {
        self.kind
    }
}

#[derive(Debug)]
pub enum VobKindMut<'a> {
    Base,
    Spot,
    Stair,
    StartPoint,
    Level,
    Animate(&'a mut Animate),
    Item(&'a mut Item),
    LensFlare(&'a mut LensFlare),
    PfxController(&'a mut PfxController),
    MessageFilter(&'a mut MessageFilter),
    MoverController(&'a mut MoverController),
    TouchDamage(&'a mut TouchDamage),
    Earthquake(&'a mut Earthquake),
    ScreenEffect(&'a mut ScreenEffect),
    ZoneMusic(&'a mut ZoneMusic),
    ZoneMusicDefault(&'a mut ZoneMusic),
    ZoneFarPlane(&'a mut ZoneFarPlane),
    ZoneFarPlaneDefault(&'a mut ZoneFarPlane),
    ZoneFog(&'a mut ZoneFog),
    ZoneFogDefault(&'a mut ZoneFog),
    Sound(&'a mut Sound),
    SoundDaytime(&'a mut SoundDaytime),
    Light(&'a mut LightPreset),
    Trigger(&'a mut Trigger),
    CutsceneTrigger(&'a mut Trigger),
    Mover(&'a mut Mover),
    TriggerList(&'a mut TriggerList),
    TriggerScript(&'a mut TriggerScript),
    TriggerChangeLevel(&'a mut TriggerChangeLevel),
    TriggerWorldStart(&'a mut TriggerWorldStart),
    TriggerUntouch(&'a mut TriggerUntouch),
    CodeMaster(&'a mut CodeMaster),
    Movable(&'a mut Movable),
    Interactive(&'a mut Interactive),
    Fire(&'a mut Fire),
    Container(&'a mut Container),
    Door(&'a mut Door),
    Ladder(&'a mut Interactive),
    Switch(&'a mut Interactive),
    Wheel(&'a mut Interactive),
    Bed(&'a mut Interactive),
    Camera(&'a mut Camera),
    CameraFrame(&'a mut CameraFrame),
    Npc(&'a mut NpcData),
}
/// Exclusive view of one VOB's separately stored common fields and subtype.
/// Use [`Self::base_mut`] for common fields, [`Self::kind_mut`] for subtype
/// fields, and [`Self::into_parts`] to retain both borrows simultaneously.
/// Structural replacement belongs to [`crate::objects::Graph::replace_vob_kind`].
///
/// The view's references and subtype discriminator cannot be reassigned as if
/// they were fields of an owned VOB:
///
/// ```compile_fail,E0616
/// use zenkit::{GameVersion, objects::{Graph, Object}, vob::{Vob, VobKindMut}};
/// let mut graph = Graph::new()?;
/// let id = graph.insert(Object::Vob(Vob::new(GameVersion::Gothic2)))?;
/// let mut view = graph.vob_mut(id)?;
/// view.kind = VobKindMut::Base;
/// # Ok::<(), zenkit::Error>(())
/// ```
///
/// ```compile_fail,E0616
/// use zenkit::{GameVersion, objects::{Graph, Object}, vob::Vob};
/// let mut graph = Graph::new()?;
/// let id = graph.insert(Object::Vob(Vob::new(GameVersion::Gothic2)))?;
/// let mut other = Vob::new(GameVersion::Gothic2);
/// let mut view = graph.vob_mut(id)?;
/// view.base = &mut other.base;
/// # Ok::<(), zenkit::Error>(())
/// ```
#[derive(Debug)]
pub struct VobMut<'a> {
    pub(crate) base: &'a mut VobBase,
    pub(crate) kind: VobKindMut<'a>,
}
impl<'a> VobMut<'a> {
    /// Borrow the common fields.
    #[inline]
    pub fn base(&self) -> &VobBase {
        self.base
    }
    /// Edit the common fields.
    #[inline]
    pub fn base_mut(&mut self) -> &mut VobBase {
        self.base
    }
    /// Inspect the stored subtype and borrow its fields.
    #[inline]
    pub fn kind(&self) -> VobKindRef<'_> {
        self.kind.as_ref()
    }
    /// Reborrow the subtype as an enum of mutable payload references.
    /// Use [`crate::objects::Graph::replace_vob_kind`] to change the subtype.
    #[inline]
    pub fn kind_mut(&mut self) -> VobKindMut<'_> {
        self.kind.reborrow()
    }
    /// Consume this view, retaining both borrows for the original borrow lifetime.
    ///
    /// ```
    /// use zenkit::{GameVersion, objects::{Graph, Object}, vob::*};
    /// let mut graph = Graph::new()?;
    /// let mut value = Vob::new(GameVersion::Gothic2);
    /// value.kind = VobKind::Npc(NpcData::new(GameVersion::Gothic2));
    /// let id = graph.insert(Object::Vob(value))?;
    /// let (base, kind) = graph.vob_mut(id)?.into_parts();
    /// if let VobKindMut::Npc(npc) = kind {
    ///     base.show_visual = true;
    ///     npc.guild = 42;
    /// }
    /// assert_eq!(graph.npc(id)?.guild, 42);
    /// # Ok::<(), zenkit::Error>(())
    /// ```
    #[inline]
    pub fn into_parts(self) -> (&'a mut VobBase, VobKindMut<'a>) {
        (self.base, self.kind)
    }
}
impl Vob {
    pub fn as_ref(&self) -> VobRef<'_> {
        VobRef {
            base: &self.base,
            kind: self.kind.as_ref(),
        }
    }
    pub fn as_mut(&mut self) -> VobMut<'_> {
        VobMut {
            base: &mut self.base,
            kind: self.kind.as_mut(),
        }
    }
}
impl VobMut<'_> {
    pub fn as_ref(&self) -> VobRef<'_> {
        VobRef {
            base: self.base,
            kind: self.kind.as_ref(),
        }
    }
    pub fn reborrow(&mut self) -> VobMut<'_> {
        VobMut {
            base: self.base,
            kind: self.kind.reborrow(),
        }
    }
    pub fn to_owned(&self) -> Vob {
        self.as_ref().to_owned()
    }
}
impl VobRef<'_> {
    /// Clone the payload. Its links still belong to the original graph.
    pub fn to_owned(self) -> Vob {
        Vob {
            base: self.base.clone(),
            kind: self.kind.to_owned(),
        }
    }
}
impl VobKindRef<'_> {
    /// Clone the selected subtype payload, without copying any graph ownership.
    pub fn to_owned(self) -> VobKind {
        match self {
            Self::Base => VobKind::Base,
            Self::Spot => VobKind::Spot,
            Self::Stair => VobKind::Stair,
            Self::StartPoint => VobKind::StartPoint,
            Self::Level => VobKind::Level,
            Self::Animate(v) => VobKind::Animate(*v),
            Self::Item(v) => VobKind::Item(v.clone()),
            Self::LensFlare(v) => VobKind::LensFlare(v.clone()),
            Self::PfxController(v) => VobKind::PfxController(v.clone()),
            Self::MessageFilter(v) => VobKind::MessageFilter(v.clone()),
            Self::MoverController(v) => VobKind::MoverController(v.clone()),
            Self::TouchDamage(v) => VobKind::TouchDamage(*v),
            Self::Earthquake(v) => VobKind::Earthquake(*v),
            Self::ScreenEffect(v) => VobKind::ScreenEffect(v.clone()),
            Self::ZoneMusic(v) => VobKind::ZoneMusic(*v),
            Self::ZoneMusicDefault(v) => VobKind::ZoneMusicDefault(*v),
            Self::ZoneFarPlane(v) => VobKind::ZoneFarPlane(*v),
            Self::ZoneFarPlaneDefault(v) => VobKind::ZoneFarPlaneDefault(*v),
            Self::ZoneFog(v) => VobKind::ZoneFog(*v),
            Self::ZoneFogDefault(v) => VobKind::ZoneFogDefault(*v),
            Self::Sound(v) => VobKind::Sound(v.clone()),
            Self::SoundDaytime(v) => VobKind::SoundDaytime(v.clone()),
            Self::Light(v) => VobKind::Light(v.clone()),
            Self::Trigger(v) => VobKind::Trigger(v.clone()),
            Self::CutsceneTrigger(v) => VobKind::CutsceneTrigger(v.clone()),
            Self::Mover(v) => VobKind::Mover(v.clone()),
            Self::TriggerList(v) => VobKind::TriggerList(v.clone()),
            Self::TriggerScript(v) => VobKind::TriggerScript(v.clone()),
            Self::TriggerChangeLevel(v) => VobKind::TriggerChangeLevel(v.clone()),
            Self::TriggerWorldStart(v) => VobKind::TriggerWorldStart(v.clone()),
            Self::TriggerUntouch(v) => VobKind::TriggerUntouch(v.clone()),
            Self::CodeMaster(v) => VobKind::CodeMaster(v.clone()),
            Self::Movable(v) => VobKind::Movable(v.clone()),
            Self::Interactive(v) => VobKind::Interactive(v.clone()),
            Self::Fire(v) => VobKind::Fire(v.clone()),
            Self::Container(v) => VobKind::Container(v.clone()),
            Self::Door(v) => VobKind::Door(v.clone()),
            Self::Ladder(v) => VobKind::Ladder(v.clone()),
            Self::Switch(v) => VobKind::Switch(v.clone()),
            Self::Wheel(v) => VobKind::Wheel(v.clone()),
            Self::Bed(v) => VobKind::Bed(v.clone()),
            Self::Camera(v) => VobKind::Camera(v.clone()),
            Self::CameraFrame(v) => VobKind::CameraFrame(v.clone()),
            Self::Npc(v) => VobKind::Npc(v.clone()),
        }
    }
}
impl VobKind {
    pub const fn as_ref(&self) -> VobKindRef<'_> {
        match self {
            Self::Base => VobKindRef::Base,
            Self::Spot => VobKindRef::Spot,
            Self::Stair => VobKindRef::Stair,
            Self::StartPoint => VobKindRef::StartPoint,
            Self::Level => VobKindRef::Level,
            Self::Animate(v) => VobKindRef::Animate(v),
            Self::Item(v) => VobKindRef::Item(v),
            Self::LensFlare(v) => VobKindRef::LensFlare(v),
            Self::PfxController(v) => VobKindRef::PfxController(v),
            Self::MessageFilter(v) => VobKindRef::MessageFilter(v),
            Self::MoverController(v) => VobKindRef::MoverController(v),
            Self::TouchDamage(v) => VobKindRef::TouchDamage(v),
            Self::Earthquake(v) => VobKindRef::Earthquake(v),
            Self::ScreenEffect(v) => VobKindRef::ScreenEffect(v),
            Self::ZoneMusic(v) => VobKindRef::ZoneMusic(v),
            Self::ZoneMusicDefault(v) => VobKindRef::ZoneMusicDefault(v),
            Self::ZoneFarPlane(v) => VobKindRef::ZoneFarPlane(v),
            Self::ZoneFarPlaneDefault(v) => VobKindRef::ZoneFarPlaneDefault(v),
            Self::ZoneFog(v) => VobKindRef::ZoneFog(v),
            Self::ZoneFogDefault(v) => VobKindRef::ZoneFogDefault(v),
            Self::Sound(v) => VobKindRef::Sound(v),
            Self::SoundDaytime(v) => VobKindRef::SoundDaytime(v),
            Self::Light(v) => VobKindRef::Light(v),
            Self::Trigger(v) => VobKindRef::Trigger(v),
            Self::CutsceneTrigger(v) => VobKindRef::CutsceneTrigger(v),
            Self::Mover(v) => VobKindRef::Mover(v),
            Self::TriggerList(v) => VobKindRef::TriggerList(v),
            Self::TriggerScript(v) => VobKindRef::TriggerScript(v),
            Self::TriggerChangeLevel(v) => VobKindRef::TriggerChangeLevel(v),
            Self::TriggerWorldStart(v) => VobKindRef::TriggerWorldStart(v),
            Self::TriggerUntouch(v) => VobKindRef::TriggerUntouch(v),
            Self::CodeMaster(v) => VobKindRef::CodeMaster(v),
            Self::Movable(v) => VobKindRef::Movable(v),
            Self::Interactive(v) => VobKindRef::Interactive(v),
            Self::Fire(v) => VobKindRef::Fire(v),
            Self::Container(v) => VobKindRef::Container(v),
            Self::Door(v) => VobKindRef::Door(v),
            Self::Ladder(v) => VobKindRef::Ladder(v),
            Self::Switch(v) => VobKindRef::Switch(v),
            Self::Wheel(v) => VobKindRef::Wheel(v),
            Self::Bed(v) => VobKindRef::Bed(v),
            Self::Camera(v) => VobKindRef::Camera(v),
            Self::CameraFrame(v) => VobKindRef::CameraFrame(v),
            Self::Npc(v) => VobKindRef::Npc(v),
        }
    }
}
impl VobKind {
    pub fn as_mut(&mut self) -> VobKindMut<'_> {
        match self {
            Self::Base => VobKindMut::Base,
            Self::Spot => VobKindMut::Spot,
            Self::Stair => VobKindMut::Stair,
            Self::StartPoint => VobKindMut::StartPoint,
            Self::Level => VobKindMut::Level,
            Self::Animate(v) => VobKindMut::Animate(v),
            Self::Item(v) => VobKindMut::Item(v),
            Self::LensFlare(v) => VobKindMut::LensFlare(v),
            Self::PfxController(v) => VobKindMut::PfxController(v),
            Self::MessageFilter(v) => VobKindMut::MessageFilter(v),
            Self::MoverController(v) => VobKindMut::MoverController(v),
            Self::TouchDamage(v) => VobKindMut::TouchDamage(v),
            Self::Earthquake(v) => VobKindMut::Earthquake(v),
            Self::ScreenEffect(v) => VobKindMut::ScreenEffect(v),
            Self::ZoneMusic(v) => VobKindMut::ZoneMusic(v),
            Self::ZoneMusicDefault(v) => VobKindMut::ZoneMusicDefault(v),
            Self::ZoneFarPlane(v) => VobKindMut::ZoneFarPlane(v),
            Self::ZoneFarPlaneDefault(v) => VobKindMut::ZoneFarPlaneDefault(v),
            Self::ZoneFog(v) => VobKindMut::ZoneFog(v),
            Self::ZoneFogDefault(v) => VobKindMut::ZoneFogDefault(v),
            Self::Sound(v) => VobKindMut::Sound(v),
            Self::SoundDaytime(v) => VobKindMut::SoundDaytime(v),
            Self::Light(v) => VobKindMut::Light(v),
            Self::Trigger(v) => VobKindMut::Trigger(v),
            Self::CutsceneTrigger(v) => VobKindMut::CutsceneTrigger(v),
            Self::Mover(v) => VobKindMut::Mover(v),
            Self::TriggerList(v) => VobKindMut::TriggerList(v),
            Self::TriggerScript(v) => VobKindMut::TriggerScript(v),
            Self::TriggerChangeLevel(v) => VobKindMut::TriggerChangeLevel(v),
            Self::TriggerWorldStart(v) => VobKindMut::TriggerWorldStart(v),
            Self::TriggerUntouch(v) => VobKindMut::TriggerUntouch(v),
            Self::CodeMaster(v) => VobKindMut::CodeMaster(v),
            Self::Movable(v) => VobKindMut::Movable(v),
            Self::Interactive(v) => VobKindMut::Interactive(v),
            Self::Fire(v) => VobKindMut::Fire(v),
            Self::Container(v) => VobKindMut::Container(v),
            Self::Door(v) => VobKindMut::Door(v),
            Self::Ladder(v) => VobKindMut::Ladder(v),
            Self::Switch(v) => VobKindMut::Switch(v),
            Self::Wheel(v) => VobKindMut::Wheel(v),
            Self::Bed(v) => VobKindMut::Bed(v),
            Self::Camera(v) => VobKindMut::Camera(v),
            Self::CameraFrame(v) => VobKindMut::CameraFrame(v),
            Self::Npc(v) => VobKindMut::Npc(v),
        }
    }
}
impl VobKindMut<'_> {
    pub fn movable(&self) -> Option<&Movable> {
        self.as_ref().movable()
    }
    pub const fn as_ref(&self) -> VobKindRef<'_> {
        match self {
            Self::Base => VobKindRef::Base,
            Self::Spot => VobKindRef::Spot,
            Self::Stair => VobKindRef::Stair,
            Self::StartPoint => VobKindRef::StartPoint,
            Self::Level => VobKindRef::Level,
            Self::Animate(v) => VobKindRef::Animate(v),
            Self::Item(v) => VobKindRef::Item(v),
            Self::LensFlare(v) => VobKindRef::LensFlare(v),
            Self::PfxController(v) => VobKindRef::PfxController(v),
            Self::MessageFilter(v) => VobKindRef::MessageFilter(v),
            Self::MoverController(v) => VobKindRef::MoverController(v),
            Self::TouchDamage(v) => VobKindRef::TouchDamage(v),
            Self::Earthquake(v) => VobKindRef::Earthquake(v),
            Self::ScreenEffect(v) => VobKindRef::ScreenEffect(v),
            Self::ZoneMusic(v) => VobKindRef::ZoneMusic(v),
            Self::ZoneMusicDefault(v) => VobKindRef::ZoneMusicDefault(v),
            Self::ZoneFarPlane(v) => VobKindRef::ZoneFarPlane(v),
            Self::ZoneFarPlaneDefault(v) => VobKindRef::ZoneFarPlaneDefault(v),
            Self::ZoneFog(v) => VobKindRef::ZoneFog(v),
            Self::ZoneFogDefault(v) => VobKindRef::ZoneFogDefault(v),
            Self::Sound(v) => VobKindRef::Sound(v),
            Self::SoundDaytime(v) => VobKindRef::SoundDaytime(v),
            Self::Light(v) => VobKindRef::Light(v),
            Self::Trigger(v) => VobKindRef::Trigger(v),
            Self::CutsceneTrigger(v) => VobKindRef::CutsceneTrigger(v),
            Self::Mover(v) => VobKindRef::Mover(v),
            Self::TriggerList(v) => VobKindRef::TriggerList(v),
            Self::TriggerScript(v) => VobKindRef::TriggerScript(v),
            Self::TriggerChangeLevel(v) => VobKindRef::TriggerChangeLevel(v),
            Self::TriggerWorldStart(v) => VobKindRef::TriggerWorldStart(v),
            Self::TriggerUntouch(v) => VobKindRef::TriggerUntouch(v),
            Self::CodeMaster(v) => VobKindRef::CodeMaster(v),
            Self::Movable(v) => VobKindRef::Movable(v),
            Self::Interactive(v) => VobKindRef::Interactive(v),
            Self::Fire(v) => VobKindRef::Fire(v),
            Self::Container(v) => VobKindRef::Container(v),
            Self::Door(v) => VobKindRef::Door(v),
            Self::Ladder(v) => VobKindRef::Ladder(v),
            Self::Switch(v) => VobKindRef::Switch(v),
            Self::Wheel(v) => VobKindRef::Wheel(v),
            Self::Bed(v) => VobKindRef::Bed(v),
            Self::Camera(v) => VobKindRef::Camera(v),
            Self::CameraFrame(v) => VobKindRef::CameraFrame(v),
            Self::Npc(v) => VobKindRef::Npc(v),
        }
    }
}
impl VobKindMut<'_> {
    pub fn reborrow(&mut self) -> VobKindMut<'_> {
        match self {
            Self::Base => VobKindMut::Base,
            Self::Spot => VobKindMut::Spot,
            Self::Stair => VobKindMut::Stair,
            Self::StartPoint => VobKindMut::StartPoint,
            Self::Level => VobKindMut::Level,
            Self::Animate(v) => VobKindMut::Animate(v),
            Self::Item(v) => VobKindMut::Item(v),
            Self::LensFlare(v) => VobKindMut::LensFlare(v),
            Self::PfxController(v) => VobKindMut::PfxController(v),
            Self::MessageFilter(v) => VobKindMut::MessageFilter(v),
            Self::MoverController(v) => VobKindMut::MoverController(v),
            Self::TouchDamage(v) => VobKindMut::TouchDamage(v),
            Self::Earthquake(v) => VobKindMut::Earthquake(v),
            Self::ScreenEffect(v) => VobKindMut::ScreenEffect(v),
            Self::ZoneMusic(v) => VobKindMut::ZoneMusic(v),
            Self::ZoneMusicDefault(v) => VobKindMut::ZoneMusicDefault(v),
            Self::ZoneFarPlane(v) => VobKindMut::ZoneFarPlane(v),
            Self::ZoneFarPlaneDefault(v) => VobKindMut::ZoneFarPlaneDefault(v),
            Self::ZoneFog(v) => VobKindMut::ZoneFog(v),
            Self::ZoneFogDefault(v) => VobKindMut::ZoneFogDefault(v),
            Self::Sound(v) => VobKindMut::Sound(v),
            Self::SoundDaytime(v) => VobKindMut::SoundDaytime(v),
            Self::Light(v) => VobKindMut::Light(v),
            Self::Trigger(v) => VobKindMut::Trigger(v),
            Self::CutsceneTrigger(v) => VobKindMut::CutsceneTrigger(v),
            Self::Mover(v) => VobKindMut::Mover(v),
            Self::TriggerList(v) => VobKindMut::TriggerList(v),
            Self::TriggerScript(v) => VobKindMut::TriggerScript(v),
            Self::TriggerChangeLevel(v) => VobKindMut::TriggerChangeLevel(v),
            Self::TriggerWorldStart(v) => VobKindMut::TriggerWorldStart(v),
            Self::TriggerUntouch(v) => VobKindMut::TriggerUntouch(v),
            Self::CodeMaster(v) => VobKindMut::CodeMaster(v),
            Self::Movable(v) => VobKindMut::Movable(v),
            Self::Interactive(v) => VobKindMut::Interactive(v),
            Self::Fire(v) => VobKindMut::Fire(v),
            Self::Container(v) => VobKindMut::Container(v),
            Self::Door(v) => VobKindMut::Door(v),
            Self::Ladder(v) => VobKindMut::Ladder(v),
            Self::Switch(v) => VobKindMut::Switch(v),
            Self::Wheel(v) => VobKindMut::Wheel(v),
            Self::Bed(v) => VobKindMut::Bed(v),
            Self::Camera(v) => VobKindMut::Camera(v),
            Self::CameraFrame(v) => VobKindMut::CameraFrame(v),
            Self::Npc(v) => VobKindMut::Npc(v),
        }
    }
}

impl<'a> VobRef<'a> {
    pub fn game(self) -> GameVersion {
        match self.base.version {
            VobVersion::Gothic1 => GameVersion::Gothic1,
            VobVersion::Gothic2(_) => GameVersion::Gothic2,
        }
    }
}

impl<'a> VobRef<'a> {
    pub fn version_identifier(self) -> u16 {
        self.kind.version_identifier(self.game())
    }
}

impl<'a> VobRef<'a> {
    pub(crate) fn buffers(self) -> impl Iterator<Item = &'a [u8]> {
        [
            self.base.preset.as_deref(),
            self.base.name.as_deref(),
            self.base.visual_name.as_deref(),
            match &self.base.packing {
                Packing::Unpacked => None,
                Packing::Packed { tail, .. } => Some(tail.as_slice()),
            },
        ]
        .into_iter()
        .flatten()
        .chain(self.kind.extra_buffers())
        .chain(self.kind.trigger_buffers())
        .chain(self.kind.mob_buffers())
        .chain(self.kind.camera_buffers())
        .chain(self.kind.npc().into_iter().flat_map(NpcData::buffers))
    }
}

impl<'a> VobRef<'a> {
    pub(crate) fn base_child_count(self) -> usize {
        usize::from(self.base.visual.is_some())
            + usize::from(self.base.ai.is_some())
            + usize::from(self.base.event_manager.is_some())
    }
}

impl<'a> VobRef<'a> {
    pub(crate) fn links(self) -> impl Iterator<Item = &'a Link> {
        [
            self.base.visual.as_ref(),
            self.base.ai.as_ref(),
            self.base.event_manager.as_ref(),
        ]
        .into_iter()
        .flatten()
        .chain(self.kind.late_links())
        .chain(self.kind.camera_links())
        .chain(self.kind.npc().into_iter().flat_map(NpcData::links))
    }
}

impl<'a> VobRef<'a> {
    pub(crate) fn check(self, save: bool) -> Result<()> {
        self.kind.check(self.game(), save)?;
        self.kind.check_trigger(self.game(), save)?;
        self.kind.check_mob(save)?;
        self.kind.check_camera(self.game(), save)?;
        if let Some(n) = self.kind.npc() {
            n.check(self.game())?;
        }
        if self.base.state.is_some() != save {
            return Err(bad("VOB state must match archive save mode"));
        }
        if self
            .base
            .state
            .is_some_and(|s| s.rigid_body.is_some() != self.base.physics_enabled)
        {
            return Err(bad("VOB rigid body must match physics flag"));
        }
        match &self.base.packing {
            Packing::Unpacked => {
                if self.base.preset.is_none()
                    || self.base.name.is_none()
                    || self.base.visual_name.is_none()
                    || self.base.visual.is_none()
                    || self.base.ai.is_none()
                    || self.base.event_manager.is_some()
                    || !self.base.physics_enabled
                {
                    return Err(bad(
                        "unpacked VOB requires names, visual/AI records and implicit physics",
                    ));
                }
            }
            Packing::Packed {
                marker, reserved, ..
            } => {
                let mask = 0x80 | if save { 0 } else { 0x20 };
                if *marker == 0 || reserved & !mask != 0 {
                    return Err(bad("packed VOB marker/reserved bits"));
                }
                if !save && self.base.event_manager.is_some() {
                    return Err(bad("event-manager record requires packed save VOB"));
                }
                if self.base.camera_alignment.raw() > 3 || self.base.shadow.raw() > 3 {
                    return Err(Error::operation(
                        ErrorKind::Unsupported,
                        "packed VOB enum range",
                    ));
                }
                if let VobVersion::Gothic2(g) = self.base.version
                    && (g.animation.raw() > 3 || !(0..=31).contains(&g.bias))
                {
                    return Err(Error::operation(
                        ErrorKind::Unsupported,
                        "packed G2 VOB animation/bias range",
                    ));
                }
            }
        }
        Ok(())
    }
}

impl<'a> VobRef<'a> {
    pub fn packed_flags(self) -> Result<Option<(u8, u16)>> {
        self.check(self.base.state.is_some())?;
        let Packing::Packed { reserved, .. } = self.base.packing else {
            return Ok(None);
        };
        let a = u32::from(self.base.show_visual)
            | (self.base.camera_alignment.raw() << 1)
            | (u32::from(self.base.collision_static) << 3)
            | (u32::from(self.base.collision_dynamic) << 4)
            | (u32::from(self.base.static_vob) << 5)
            | (self.base.shadow.raw() << 6);
        let mut b = reserved
            | u16::from(self.base.preset.is_some())
            | (u16::from(self.base.name.is_some()) << 1)
            | (u16::from(self.base.visual_name.is_some()) << 2)
            | (u16::from(self.base.visual.is_some()) << 3)
            | (u16::from(self.base.ai.is_some()) << 4)
            | (u16::from(self.base.event_manager.is_some()) << 5)
            | (u16::from(self.base.physics_enabled) << 6);
        if let VobVersion::Gothic2(g) = self.base.version {
            b |= (g.animation.raw() as u16) << 8;
            b |= (g.bias as u16) << 10;
            b |= u16::from(g.ambient) << 15;
        }
        Ok(Some((a as u8, b)))
    }
}

impl VobMut<'_> {
    pub(crate) fn links_mut(&mut self) -> impl Iterator<Item = &mut Link> {
        let base = &mut self.base;
        [
            base.visual.as_mut(),
            base.ai.as_mut(),
            base.event_manager.as_mut(),
        ]
        .into_iter()
        .flatten()
        .chain(self.kind.late_links_mut())
    }
}

impl VobMut<'_> {
    pub fn game(&self) -> GameVersion {
        self.as_ref().game()
    }
    pub fn version_identifier(&self) -> u16 {
        self.as_ref().version_identifier()
    }
    pub(crate) fn base_child_count(&self) -> usize {
        self.as_ref().base_child_count()
    }
    pub fn packed_flags(&self) -> Result<Option<(u8, u16)>> {
        self.as_ref().packed_flags()
    }
}
impl fmt::Debug for VobBase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VobBase")
            .field("version", &self.version)
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}
