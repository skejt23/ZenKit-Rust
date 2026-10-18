// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;
use crate::values::Color;

macro_rules! bytes_debug {
    ($name:ident,$($field:ident),+)=>{impl fmt::Debug for $name {fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result {f.debug_struct(stringify!($name))$(.field(concat!(stringify!($field),"_bytes"),&self.$field.len()))+.finish_non_exhaustive()}}};
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Animate {
    pub start_on: bool,
    pub running: Option<bool>,
}
impl Animate {
    /// Effective running state; normal archives derive it from `start_on`.
    pub fn is_running(&self) -> bool {
        self.running.unwrap_or(self.start_on)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ItemState {
    pub amount: i32,
    pub flags: i32,
}
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Item {
    pub instance: Vec<u8>,
    pub state: Option<ItemState>,
}
bytes_debug!(Item, instance);
#[derive(Clone, Default, PartialEq, Eq)]
pub struct LensFlare {
    pub effect: Vec<u8>,
}
bytes_debug!(LensFlare, effect);
#[derive(Clone, Default, PartialEq, Eq)]
pub struct PfxController {
    pub name: Vec<u8>,
    pub kill_when_done: bool,
    pub initially_running: bool,
}
bytes_debug!(PfxController, name);
#[derive(Clone, Default, PartialEq, Eq)]
pub struct MessageFilter {
    pub target: Vec<u8>,
    pub on_trigger: MessageAction,
    pub on_untrigger: MessageAction,
}
bytes_debug!(MessageFilter, target);
#[derive(Clone, Default, PartialEq, Eq)]
pub struct MoverController {
    pub target: Vec<u8>,
    pub message: MoverMessage,
    pub key: i32,
}
bytes_debug!(MoverController, target);
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TouchDamage {
    pub damage: f32,
    pub barrier: bool,
    pub blunt: bool,
    pub edge: bool,
    pub fire: bool,
    pub fly: bool,
    pub magic: bool,
    pub point: bool,
    pub fall: bool,
    pub repeat_delay: f32,
    pub volume_scale: f32,
    pub collision: TouchCollision,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Earthquake {
    pub radius: f32,
    pub duration: f32,
    pub amplitude: Vec3,
}
/// Complete opaque save fields. Each raw field is at least 20 bytes in G1 or 48
/// in G2. Tagged tails survive; BINARY output requires the exact game length.
#[derive(Clone, PartialEq)]
pub struct ScreenState {
    pub game: GameVersion,
    pub blend: Vec<u8>,
    pub cinema: Vec<u8>,
    pub fov_morph: Vec<u8>,
    pub fov_saved: Vec2,
    pub fov_saved_first: Vec2,
}
impl ScreenState {
    pub fn new(game: GameVersion) -> Self {
        let n = if game == GameVersion::Gothic1 { 20 } else { 48 };
        Self {
            game,
            blend: vec![0; n],
            cinema: vec![0; n],
            fov_morph: vec![0; n],
            fov_saved: Vec2::ZERO,
            fov_saved_first: Vec2::ZERO,
        }
    }
}
bytes_debug!(ScreenState, blend, cinema, fov_morph);
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScreenEffect {
    pub state: Option<ScreenState>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MusicState {
    pub local_enabled: bool,
    pub day_entrance_done: bool,
    pub night_entrance_done: bool,
}
impl Default for MusicState {
    fn default() -> Self {
        Self {
            local_enabled: true,
            day_entrance_done: false,
            night_entrance_done: false,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ZoneMusic {
    pub enabled: bool,
    pub priority: i32,
    pub ellipsoid: bool,
    pub reverb: f32,
    pub volume: f32,
    pub looped: bool,
    pub state: Option<MusicState>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ZoneFarPlane {
    pub far_plane: f32,
    pub inner_range: f32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Gothic2Fog {
    pub fade_out_sky: bool,
    pub override_color: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FogVersion {
    Gothic1,
    Gothic2(Gothic2Fog),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoneFog {
    pub range_center: f32,
    pub inner_range: f32,
    pub color: Color,
    pub version: FogVersion,
}
impl ZoneFog {
    pub fn new(game: GameVersion) -> Self {
        Self {
            range_center: 0.,
            inner_range: 0.,
            color: Color::default(),
            version: match game {
                GameVersion::Gothic1 => FogVersion::Gothic1,
                GameVersion::Gothic2 => FogVersion::Gothic2(Gothic2Fog::default()),
            },
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SoundState {
    pub running: bool,
    pub allowed_to_run: bool,
}
#[derive(Clone, PartialEq)]
pub struct Sound {
    pub volume: f32,
    pub mode: SoundMode,
    pub random_delay: f32,
    pub random_delay_var: f32,
    pub initially_playing: bool,
    pub ambient_3d: bool,
    pub obstruction: bool,
    pub cone_angle: f32,
    pub volume_type: SoundVolume,
    pub radius: f32,
    pub name: Vec<u8>,
    pub state: Option<SoundState>,
}
bytes_debug!(Sound, name);
impl Default for Sound {
    fn default() -> Self {
        Self {
            volume: 0.,
            mode: SoundMode::ONCE,
            random_delay: 0.,
            random_delay_var: 0.,
            initially_playing: false,
            ambient_3d: false,
            obstruction: true,
            cone_angle: 0.,
            volume_type: SoundVolume::SPHERICAL,
            radius: 0.,
            name: Vec::new(),
            state: None,
        }
    }
}
#[derive(Clone, Default, PartialEq)]
pub struct SoundDaytime {
    pub sound: Sound,
    pub start_time: f32,
    pub end_time: f32,
    pub second_name: Vec<u8>,
}
bytes_debug!(SoundDaytime, second_name);

impl VobKind {}

impl<'a> VobKindRef<'a> {
    pub(crate) fn extra_buffers(self) -> impl Iterator<Item = &'a [u8]> {
        let a: [Option<&[u8]>; 4] = match self {
            Self::Item(v) => [Some(&v.instance), None, None, None],
            Self::LensFlare(v) => [Some(&v.effect), None, None, None],
            Self::PfxController(v) => [Some(&v.name), None, None, None],
            Self::MessageFilter(v) => [Some(&v.target), None, None, None],
            Self::MoverController(v) => [Some(&v.target), None, None, None],
            Self::Sound(v) => [Some(&v.name), None, None, None],
            Self::SoundDaytime(v) => [Some(&v.sound.name), Some(&v.second_name), None, None],
            Self::ScreenEffect(ScreenEffect { state: Some(v) }) => {
                [Some(&v.blend), Some(&v.cinema), Some(&v.fov_morph), None]
            }
            Self::Light(v) => [
                Some(&v.preset),
                Some(&v.lensflare),
                v.animation.as_ref().map(|a| a.range_scale.as_bytes()),
                v.animation.as_ref().map(|a| a.colors.as_bytes()),
            ],
            _ => [None; 4],
        };
        a.into_iter().flatten()
    }
}

impl<'a> VobKindRef<'a> {
    pub(crate) fn check(self, game: GameVersion, save: bool) -> Result<()> {
        let state = match self {
            Self::Animate(v) => Some(v.running.is_some()),
            Self::Item(v) => Some(v.state.is_some()),
            Self::ScreenEffect(v) => Some(v.state.is_some()),
            Self::ZoneMusic(v) | Self::ZoneMusicDefault(v) => Some(v.state.is_some()),
            Self::Sound(v) => Some(v.state.is_some()),
            Self::SoundDaytime(v) => Some(v.sound.state.is_some()),
            _ => None,
        };
        if state.is_some_and(|present| present != save) {
            return Err(bad("VOB subtype state must match save mode"));
        }
        match self {
            Self::ScreenEffect(ScreenEffect { state: Some(s) }) => {
                if s.game != game {
                    return Err(bad("screen effect game mismatch"));
                }
                let n = if game == GameVersion::Gothic1 { 20 } else { 48 };
                if [&s.blend, &s.cinema, &s.fov_morph]
                    .iter()
                    .any(|v| v.len() < n)
                {
                    return Err(bad("screen effect raw field minimum"));
                }
            }
            Self::ZoneFog(v) | Self::ZoneFogDefault(v) => {
                if matches!(v.version, FogVersion::Gothic1) != (game == GameVersion::Gothic1) {
                    return Err(bad("fog game mismatch"));
                }
            }
            Self::Light(v) => v.check_game(game)?,
            _ => {}
        }
        Ok(())
    }
}
