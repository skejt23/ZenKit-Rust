//! Bounded MDS source and MSB compiled model-script readers.
//!
//! Source syntax mode is explicit. Legacy accepts documented recoveries and
//! retains diagnostics. Events retain wire/source order and raw tag arguments.
//! Reading only; no compiler or serializer is provided.
//!
//! Strict source syntax checks delimiters and accepts finite decimal/exponent f32
//! literals, signed frame integers and quoted `"ATTACH"`. Legacy additionally
//! accepts missing quotes/braces, optional parentheses, unbraced events, single
//! slash comments and `EMTPY_SLOT`; corrections have absolute-offset diagnostics.
//! Comments/semicolon separators are not tokens; ignored parentheses are charged.
//! Optional legacy blend flags use conventional `MREFIP.` spelling and cannot
//! cross an explicit closing parenthesis. Required flags retain unknown bytes.
//!
//! Binary strings use exact LF or CRLF delimiters with no leading-space trimming.
//! Unknown chunks/instructions fail explicitly. MSB MAX_FPS (F510), animation
//! sync/batch chunks (F550/F560) and PFX-ground/set-mesh/swap-mesh event chunks
//! (F5A6/F5A7/F5A8) return Unsupported. Event groups own ordered events on the most
//! recent animation, blend or combination; other declarations/scopes clear ownership.
//!
//! ```
//! use zenkit::{animation::Limits, model_script::{ModelScript, SyntaxMode}};
//! let script = ModelScript::from_mds(b"Model(\"M\"){aniEnum{}}", SyntaxMode::Strict, Limits::default())?;
//! assert_eq!(script.model_name.as_deref(), Some(b"M".as_slice()));
//! # Ok::<(), zenkit::Error>(())
//! ```
// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2022-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
mod binary;
mod source;
use crate::{
    ErrorKind, Result,
    animation::{self, CTX, Limits, at, bad, chunk, finish, line, push},
    io::Reader,
    mesh::Budget,
    values::Date,
};
use std::fmt;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxMode {
    /// Requires balanced delimiters and quoted strings. Quoted `"ATTACH"` is an
    /// accepted spelling, recorded by `QuotedAttachment` diagnostics.
    Strict,
    /// Accepts missing closing quotes/braces, optional parentheses, unbraced
    /// events, single-slash comments and `EMTPY_SLOT`, with recovery diagnostics.
    /// Parentheses are grammar separators; optional blend flags accept only
    /// established `MREFIP.` spellings to avoid consuming unknown instructions.
    Legacy,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recovery {
    MissingQuote,
    ClosingParenthesisInUnclosedString,
    SingleSlashComment,
    MissingBrace,
    UnbracedEvents,
    MisspelledEmptySlot,
    QuotedAttachment,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub offset: usize,
    pub recovery: Recovery,
}
/// Retains all MSB boolean bits; source syntax constructs canonical zero/one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bool32(pub u32);
impl Bool32 {
    pub const fn get(self) -> bool {
        self.0 != 0
    }
}
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Flags(pub Vec<u8>);
impl Flags {
    /// Bits for recognized uppercase M/R/E/F/I/P; unknown bytes stay in the raw tuple field.
    pub fn known_bits(&self) -> u8 {
        self.0.iter().fold(0, |bits, c| {
            bits | match c {
                b'M' => 1,
                b'R' => 2,
                b'E' => 4,
                b'F' => 8,
                b'I' => 16,
                b'P' => 32,
                _ => 0,
            }
        })
    }
}
impl fmt::Debug for Flags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Flags")
            .field("known_bits", &self.known_bits())
            .field("raw_bytes", &self.0.len())
            .finish()
    }
}
#[derive(Clone, PartialEq, Eq)]
pub struct Direction(pub Vec<u8>);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Playback {
    Forward,
    Backward,
}
impl Direction {
    pub fn playback(&self) -> Option<Playback> {
        if self.0.eq_ignore_ascii_case(b"F") {
            Some(Playback::Forward)
        } else if self.0.eq_ignore_ascii_case(b"R") {
            Some(Playback::Backward)
        } else {
            None
        }
    }
}
impl fmt::Debug for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Direction")
            .field("playback", &self.playback())
            .field("raw_bytes", &self.0.len())
            .finish()
    }
}
#[derive(Clone, Default, PartialEq)]
pub struct Skeleton {
    pub name: Vec<u8>,
    pub disable_mesh: Bool32,
}
#[derive(Clone, PartialEq)]
pub struct Common {
    pub name: Vec<u8>,
    pub layer: u32,
    pub next: Vec<u8>,
    pub blend_in: f32,
    pub blend_out: f32,
    pub flags: Flags,
}
#[derive(Clone, PartialEq)]
pub struct Animation {
    pub common: Common,
    pub model: Vec<u8>,
    pub direction: Direction,
    pub first_frame: i32,
    pub last_frame: i32,
    pub fps: f32,
    pub speed: f32,
    pub collision_volume_scale: f32,
    pub events: Vec<Event>,
}
#[derive(Clone, PartialEq)]
pub struct Alias {
    pub common: Common,
    pub alias: Vec<u8>,
    pub direction: Direction,
}
#[derive(Clone, PartialEq)]
pub struct Blend {
    pub name: Vec<u8>,
    pub next: Vec<u8>,
    pub blend_in: f32,
    pub blend_out: f32,
    pub layer: Option<i32>,
    pub flags: Option<Flags>,
    pub events: Vec<Event>,
}
#[derive(Clone, PartialEq)]
pub struct Combination {
    pub common: Common,
    pub model: Vec<u8>,
    pub last_frame: i32,
    pub events: Vec<Event>,
}
#[derive(Clone, PartialEq)]
pub struct ModelTag {
    pub frame: Option<i32>,
    pub name: Vec<u8>,
    pub bone: Vec<u8>,
}
#[derive(Clone, PartialEq)]
pub struct Sound {
    pub frame: i32,
    pub name: Vec<u8>,
    pub range: f32,
    pub empty_slot: Bool32,
}
#[derive(Clone, PartialEq)]
pub struct Particle {
    pub frame: i32,
    pub index: i32,
    pub name: Vec<u8>,
    pub position: Vec<u8>,
    pub attached: Bool32,
    pub attachment_literal: Option<Vec<u8>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParticleStop {
    pub frame: i32,
    pub index: i32,
}
#[derive(Clone, PartialEq)]
pub struct Morph {
    pub frame: i32,
    pub animation: Vec<u8>,
    pub node: Vec<u8>,
    pub compiled_values: Option<[f32; 2]>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CameraTremor {
    pub frame: i32,
    pub fields: [i32; 4],
}
#[derive(Clone, PartialEq)]
pub enum Event {
    Tag(Tag),
    Sound(Sound),
    GroundSound(Sound),
    Particle(Particle),
    ParticleStop(ParticleStop),
    Morph(Morph),
    CameraTremor(CameraTremor),
}
impl fmt::Debug for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tag(v) => v.fmt(f),
            Self::Sound(v) | Self::GroundSound(v) => v.fmt(f),
            Self::Particle(v) => v.fmt(f),
            Self::ParticleStop(v) => v.fmt(f),
            Self::Morph(v) => v.fmt(f),
            Self::CameraTremor(v) => v.fmt(f),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TagKind {
    Unknown = 0,
    ItemCreate = 1,
    ItemInsert = 2,
    ItemRemove = 3,
    ItemDestroy = 4,
    ItemPlace = 5,
    ItemExchange = 6,
    SetFightMode = 7,
    MunitionPlace = 8,
    MunitionRemove = 9,
    SoundDraw = 10,
    SoundUndraw = 11,
    MeshSwap = 12,
    TorchDraw = 13,
    TorchInventory = 14,
    TorchDrop = 15,
    HitLimb = 16,
    HitDirection = 17,
    DamageMultiplier = 18,
    ParryFrame = 19,
    OptimalFrame = 20,
    HitEnd = 21,
    ComboWindow = 22,
}
#[derive(Clone, PartialEq)]
pub struct Tag {
    pub frame: i32,
    pub name: Vec<u8>,
    pub arguments: Vec<Vec<u8>>,
    pub attached: bool,
}
impl Tag {
    pub fn kind(&self) -> TagKind {
        match self.name.as_slice() {
            b"DEF_CREATE_ITEM" => TagKind::ItemCreate,
            b"DEF_INSERT_ITEM" => TagKind::ItemInsert,
            b"DEF_REMOVE_ITEM" => TagKind::ItemRemove,
            b"DEF_DESTROY_ITEM" => TagKind::ItemDestroy,
            b"DEF_PLACE_ITEM" => TagKind::ItemPlace,
            b"DEF_EXCHANGE_ITEM" => TagKind::ItemExchange,
            b"DEF_FIGHTMODE" => TagKind::SetFightMode,
            b"DEF_PLACE_MUNITION" => TagKind::MunitionPlace,
            b"DEF_REMOVE_MUNITION" => TagKind::MunitionRemove,
            b"DEF_DRAWSOUND" => TagKind::SoundDraw,
            b"DEF_UNDRAWSOUND" => TagKind::SoundUndraw,
            b"DEF_SWAPMESH" => TagKind::MeshSwap,
            b"DEF_DRAWTORCH" => TagKind::TorchDraw,
            b"DEF_INV_TORCH" => TagKind::TorchInventory,
            b"DEF_DROP_TORCH" => TagKind::TorchDrop,
            b"DEF_HIT_LIMB" | b"HIT_LIMB" => TagKind::HitLimb,
            b"DEF_HIT_DIR" | b"DEF_DIR" => TagKind::HitDirection,
            b"DEF_DAM_MULTIPLY" => TagKind::DamageMultiplier,
            b"DEF_PAR_FRAME" => TagKind::ParryFrame,
            b"DEF_OPT_FRAME" => TagKind::OptimalFrame,
            b"DEF_HIT_END" => TagKind::HitEnd,
            b"DEF_WINDOW" => TagKind::ComboWindow,
            _ => TagKind::Unknown,
        }
    }
    /// Interpret recognized arguments without discarding their raw spelling.
    /// Unknown fight modes remain distinguishable from the explicit NONE mode.
    /// Numeric lists require complete i32 tokens; an empty list is valid.
    pub fn payload(&self) -> Result<TagPayload<'_>> {
        let a = self.arguments.first().map_or(&[][..], Vec::as_slice);
        let b = self.arguments.get(1).map_or(&[][..], Vec::as_slice);
        Ok(match self.kind() {
            TagKind::ItemCreate | TagKind::ItemExchange => TagPayload::Item { slot: a, item: b },
            TagKind::MeshSwap => TagPayload::MeshSwap {
                slot: a,
                replacement: b,
            },
            TagKind::ItemInsert
            | TagKind::ItemRemove
            | TagKind::ItemDestroy
            | TagKind::MunitionPlace
            | TagKind::MunitionRemove
            | TagKind::SoundDraw
            | TagKind::SoundUndraw
            | TagKind::TorchInventory
            | TagKind::HitLimb
            | TagKind::HitDirection => TagPayload::Slot(a),
            TagKind::SetFightMode => TagPayload::FightMode(match a {
                b"FIST" => FightMode::Fist,
                b"1H" | b"1h" => FightMode::SingleHanded,
                b"2H" | b"2h" => FightMode::DualHanded,
                b"BOW" => FightMode::Bow,
                b"CBOW" => FightMode::Crossbow,
                b"MAG" => FightMode::Magic,
                b"NONE" | b"" => FightMode::None,
                _ => FightMode::Unknown(a),
            }),
            TagKind::DamageMultiplier
            | TagKind::ParryFrame
            | TagKind::OptimalFrame
            | TagKind::HitEnd
            | TagKind::ComboWindow => TagPayload::Frames(FrameList::new(a)?),
            TagKind::Unknown => TagPayload::Unknown,
            _ => TagPayload::None,
        })
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FightMode<'a> {
    Fist,
    SingleHanded,
    DualHanded,
    Bow,
    Crossbow,
    Magic,
    None,
    Unknown(&'a [u8]),
}
pub enum TagPayload<'a> {
    None,
    Item {
        slot: &'a [u8],
        item: &'a [u8],
    },
    Slot(&'a [u8]),
    MeshSwap {
        slot: &'a [u8],
        replacement: &'a [u8],
    },
    FightMode(FightMode<'a>),
    Frames(FrameList<'a>),
    Unknown,
}
impl fmt::Debug for FightMode<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(v) => f.debug_struct("Unknown").field("bytes", &v.len()).finish(),
            Self::Fist => f.write_str("Fist"),
            Self::SingleHanded => f.write_str("SingleHanded"),
            Self::DualHanded => f.write_str("DualHanded"),
            Self::Bow => f.write_str("Bow"),
            Self::Crossbow => f.write_str("Crossbow"),
            Self::Magic => f.write_str("Magic"),
            Self::None => f.write_str("None"),
        }
    }
}
impl fmt::Debug for TagPayload<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Unknown => f.write_str("Unknown"),
            Self::Slot(v) => f.debug_struct("Slot").field("bytes", &v.len()).finish(),
            Self::Item { slot, item } => f
                .debug_struct("Item")
                .field("slot_bytes", &slot.len())
                .field("item_bytes", &item.len())
                .finish(),
            Self::MeshSwap { slot, replacement } => f
                .debug_struct("MeshSwap")
                .field("slot_bytes", &slot.len())
                .field("replacement_bytes", &replacement.len())
                .finish(),
            Self::FightMode(v) => f.debug_tuple("FightMode").field(v).finish(),
            Self::Frames(v) => f.debug_tuple("Frames").field(v).finish(),
        }
    }
}
pub(crate) fn space(c: u8) -> bool {
    c.is_ascii_whitespace() || c == 0x0b
}
fn parse_int(v: &[u8]) -> Result<i32> {
    std::str::from_utf8(v)
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| bad(ErrorKind::InvalidData))
}
#[derive(Clone, Copy)]
pub struct FrameList<'a> {
    raw: &'a [u8],
    count: usize,
}
impl<'a> FrameList<'a> {
    pub fn new(raw: &'a [u8]) -> Result<Self> {
        let mut count = 0;
        for v in raw.split(|&c| space(c)).filter(|v| !v.is_empty()) {
            parse_int(v)?;
            count += 1;
        }
        Ok(Self { raw, count })
    }
    pub fn len(self) -> usize {
        self.count
    }
    pub fn is_empty(self) -> bool {
        self.count == 0
    }
    pub fn iter(self) -> impl DoubleEndedIterator<Item = i32> + 'a {
        self.raw
            .split(|&c| space(c))
            .filter(|v| !v.is_empty())
            .map(|v| parse_int(v).expect("validated immutable frame list"))
    }
}
impl fmt::Debug for FrameList<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FrameList")
            .field("bytes", &self.raw.len())
            .field("count", &self.count)
            .finish()
    }
}
pub(crate) fn validate_tag(t: &Tag, argument_offset: usize) -> Result<()> {
    if matches!(
        t.kind(),
        TagKind::DamageMultiplier
            | TagKind::ParryFrame
            | TagKind::OptimalFrame
            | TagKind::HitEnd
            | TagKind::ComboWindow
    ) {
        let raw = t.arguments.first().map_or(&[][..], Vec::as_slice);
        let mut i = 0;
        while i < raw.len() {
            if space(raw[i]) {
                i += 1;
                continue;
            }
            let start = i;
            while i < raw.len() && !space(raw[i]) {
                i += 1;
            }
            parse_int(&raw[start..i])
                .map_err(|_| at(ErrorKind::InvalidData, argument_offset + start))?;
        }
    }
    Ok(())
}
#[derive(Clone, PartialEq)]
pub struct Root {
    pub value: u32,
    pub path: Vec<u8>,
}
#[derive(Clone, PartialEq)]
pub struct Source {
    pub date: Date,
    pub path: Vec<u8>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Marker {
    pub id: u16,
    pub offset: usize,
    pub value: Option<u32>,
}
#[derive(Clone, Default, PartialEq)]
pub struct ModelScript {
    pub model_name: Option<Vec<u8>>,
    pub root: Option<Root>,
    pub source: Option<Source>,
    pub markers: Vec<Marker>,
    pub skeleton: Option<Skeleton>,
    pub meshes: Vec<Vec<u8>>,
    pub disabled_animations: Vec<Vec<u8>>,
    pub combinations: Vec<Combination>,
    pub blends: Vec<Blend>,
    pub aliases: Vec<Alias>,
    pub model_tags: Vec<ModelTag>,
    pub animations: Vec<Animation>,
    pub diagnostics: Vec<Diagnostic>,
}
impl ModelScript {
    /// Read a complete script, selecting its format from the first word.
    /// A little-endian u16 of `0xd000` or at least `0xf000` selects compiled MSB;
    /// other values select source MDS. At least two input bytes are required.
    /// This is a compatibility heuristic, not universal format identification.
    /// `source_mode` applies only to MDS. Errors never trigger another codec.
    /// Use [`Self::from_msb`] or [`Self::from_mds`] to choose explicitly.
    pub fn from_bytes(bytes: &[u8], source_mode: SyntaxMode, limits: Limits) -> Result<Self> {
        Self::read(&mut Reader::new(bytes), bytes.len(), source_mode, limits)
    }
    /// Select and parse exactly one caller-sized script span. Selection stays
    /// inside the bounded window; following bytes cannot affect it. On error the
    /// parent cursor is unchanged. Errors/diagnostics retain physical offsets.
    /// See [`Self::from_bytes`] for the heuristic and explicit-codec alternatives.
    pub fn read(
        r: &mut Reader<'_>,
        length: usize,
        source_mode: SyntaxMode,
        limits: Limits,
    ) -> Result<Self> {
        animation::window(r, length, limits, |r, b| {
            let first = r.clone().u16(CTX)?;
            if first >= 0xf000 || first == 0xd000 {
                binary::read(r, b)
            } else {
                Self::parse_source(r, b, source_mode, limits)
            }
        })
    }
    pub fn from_msb(bytes: &[u8], limits: Limits) -> Result<Self> {
        Self::read_msb(&mut Reader::new(bytes), bytes.len(), limits)
    }
    pub fn from_mds(bytes: &[u8], mode: SyntaxMode, limits: Limits) -> Result<Self> {
        Self::read_mds(&mut Reader::new(bytes), bytes.len(), mode, limits)
    }
    /// Compiled scripts are bounded by an explicit enclosing byte span.
    pub fn read_msb(r: &mut Reader<'_>, length: usize, limits: Limits) -> Result<Self> {
        animation::window(r, length, limits, binary::read)
    }
    /// Parse a complete source span, consuming it only after success. Mode is
    /// deliberately explicit; Legacy recovery diagnostics remain on the result.
    pub fn read_mds(
        r: &mut Reader<'_>,
        length: usize,
        mode: SyntaxMode,
        limits: Limits,
    ) -> Result<Self> {
        animation::window(r, length, limits, |r, b| {
            Self::parse_source(r, b, mode, limits)
        })
    }
    fn parse_source(
        r: &mut Reader<'_>,
        budget: &mut Budget,
        mode: SyntaxMode,
        limits: Limits,
    ) -> Result<Self> {
        let p = r.position();
        let bytes = r.take_bytes(r.remaining(), CTX)?;
        source::read(bytes, p, mode, limits, budget)
    }
}
macro_rules! compact{($ty:ty,$label:literal,$($name:literal=>$value:ident),*)=>{impl fmt::Debug for $ty{fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{f.debug_struct($label)$(.field($name,&self.$value.len()))*.finish_non_exhaustive()}}};}
compact!(Skeleton,"Skeleton","name_bytes"=>name);
compact!(Common,"AnimationCommon","name_bytes"=>name,"next_bytes"=>next);
compact!(Animation,"ScriptAnimation","model_bytes"=>model,"events"=>events);
compact!(Alias,"AnimationAlias","alias_bytes"=>alias);
compact!(Blend,"AnimationBlend","name_bytes"=>name,"events"=>events);
compact!(Combination,"AnimationCombination","model_bytes"=>model,"events"=>events);
compact!(ModelTag,"ModelTag","name_bytes"=>name,"bone_bytes"=>bone);
compact!(Sound,"Sound","name_bytes"=>name);
compact!(Particle,"Particle","name_bytes"=>name,"position_bytes"=>position);
compact!(Morph,"Morph","animation_bytes"=>animation,"node_bytes"=>node);
compact!(Tag,"Tag","name_bytes"=>name,"arguments"=>arguments);
compact!(Root,"ScriptRoot","path_bytes"=>path);
compact!(Source,"ScriptSource","path_bytes"=>path);
compact!(ModelScript,"ModelScript","animations"=>animations,"aliases"=>aliases,"blends"=>blends,"combinations"=>combinations,"diagnostics"=>diagnostics);
