//! VOB common fields and subtype payloads have independent typed ownership.
use super::*;
use crate::objects::arena::{Arena, Prepared, Replacement, Stored};

macro_rules! vob_pools {
    (empty { $($empty:ident),* $(,)? }
     pools { $($pool:ident: $ty:ty => [$($tag:ident),+]),* $(,)? }) => {
        #[derive(Clone, Copy)]
        enum Location { $($empty,)* $($($tag(u32),)+)* }
        #[derive(Clone, Copy)]
        pub(crate) enum Schema { $($empty,)* $($($tag,)+)* }
        #[derive(Clone, Copy, PartialEq)]
        enum PoolKind { Empty, $($pool,)* }
        struct Record { base: VobBase, kind: Location }
        #[derive(Default)]
        #[allow(non_snake_case)]
        pub(crate) struct VobPools { records: Arena<Stored<Record>>, $($pool: Arena<Stored<$ty>>,)* }
        pub(crate) struct PreparedVob { record: Prepared<Stored<Record>>, kind: PreparedKind }
        enum PreparedKind { Empty, $($pool(Prepared<Stored<$ty>>),)* }
        trait Restore { fn restore(self, location: Location) -> VobKind; }
        $(impl Restore for $ty {
            fn restore(self, location: Location) -> VobKind {
                match location { $(Location::$tag(_) => VobKind::$tag(self),)+ _ => unreachable!("payload class tag") }
            }
        })*
        impl Location {
            fn pool(self) -> PoolKind { match self { $(Self::$empty => PoolKind::Empty,)* $($(Self::$tag(_) => PoolKind::$pool,)+)* } }
            fn index(self) -> u32 { match self { $(Self::$empty => unreachable!("empty subtype index"),)* $($(Self::$tag(i) => i,)+)* } }
            fn set_index(&mut self, index: u32) { match self { $(Self::$empty => unreachable!("empty subtype moved"),)* $($(Self::$tag(i) => *i = index,)+)* } }
            fn empty_kind(self) -> VobKind { match self { $(Self::$empty => VobKind::$empty,)* _ => unreachable!("nonempty subtype") } }
        }
        impl VobPools {
            pub(crate) fn register(&mut self, schema: Schema, game: GameVersion, version: u16, owner: usize) -> Result<usize> {
                u32::try_from(self.records.len()).map_err(|_| Error::operation(ErrorKind::LimitExceeded, "VOB record index"))?;
                let record = self.records.prepare()?;
                let index = self.records.len();
                let kind = match schema {
                    $(Schema::$empty => Location::$empty,)*
                    $($(Schema::$tag => {
                        u32::try_from(self.$pool.len()).map_err(|_| Error::operation(ErrorKind::LimitExceeded, "VOB payload index"))?;
                        let prepared = self.$pool.prepare()?;
                        let value = initial_payload!($pool, game, version);
                        Location::$tag(self.$pool.commit(prepared, Stored { owner: index, value }) as u32)
                    },)+)*
                };
                Ok(self.records.commit(record, Stored { owner, value: Record { base: VobBase::new(game), kind } }))
            }
            fn prepare_kind(&mut self, kind: &VobKind) -> Result<PreparedKind> {
                Ok(match kind {
                    $(VobKind::$empty => PreparedKind::Empty,)*
                    $($(VobKind::$tag(_) => {
                        u32::try_from(self.$pool.len()).map_err(|_| Error::operation(ErrorKind::LimitExceeded, "VOB payload index"))?;
                        PreparedKind::$pool(self.$pool.prepare()?)
                    },)+)*
                })
            }
            pub(crate) fn prepare(&mut self, value: &Vob) -> Result<PreparedVob> {
                u32::try_from(self.records.len()).map_err(|_| Error::operation(ErrorKind::LimitExceeded, "VOB record index"))?;
                let record = self.records.prepare()?;
                let kind = self.prepare_kind(&value.kind)?;
                Ok(PreparedVob { record, kind })
            }
            fn commit_kind(&mut self, prepared: PreparedKind, value: VobKind, owner: usize) -> Location {
                match (prepared, value) {
                    $((PreparedKind::Empty, VobKind::$empty) => Location::$empty,)*
                    $($((PreparedKind::$pool(p), VobKind::$tag(value)) => Location::$tag(self.$pool.commit(p, Stored { owner, value }) as u32),)+)*
                    _ => unreachable!("prepared VOB subtype"),
                }
            }
            pub(crate) fn commit(&mut self, prepared: PreparedVob, value: Vob, owner: usize) -> usize {
                let index = self.records.len();
                let kind = self.commit_kind(prepared.kind, value.kind, index);
                self.records.commit(prepared.record, Stored { owner, value: Record { base: value.base, kind } })
            }
            pub(crate) fn get(&self, index: usize) -> VobRef<'_> {
                let r = &self.records.get(index).value;
                let kind = match r.kind {
                    $(Location::$empty => VobKindRef::$empty,)*
                    $($(Location::$tag(i) => VobKindRef::$tag(&self.$pool.get(i as usize).value),)+)*
                };
                VobRef { base: &r.base, kind }
            }
            pub(crate) fn get_mut(&mut self, index: usize) -> VobMut<'_> {
                let r = &mut self.records.get_mut(index).value;
                let kind = match r.kind {
                    $(Location::$empty => VobKindMut::$empty,)*
                    $($(Location::$tag(i) => VobKindMut::$tag(&mut self.$pool.get_mut(i as usize).value),)+)*
                };
                VobMut { base: &mut r.base, kind }
            }
            fn remove_kind(&mut self, location: Location) -> VobKind {
                match location {
                    $(Location::$empty => VobKind::$empty,)*
                    $($(Location::$tag(i) => {
                        let old = self.$pool.swap_remove(i as usize).value;
                        if (i as usize) < self.$pool.len() {
                            let owner = self.$pool.get(i as usize).owner;
                            self.records.get_mut(owner).value.kind.set_index(i);
                        }
                        VobKind::$tag(old)
                    },)+)*
                }
            }
            fn set_owner(&mut self, location: Location, owner: usize) {
                match location { $(Location::$empty => {},)* $($(Location::$tag(i) => self.$pool.get_mut(i as usize).owner = owner,)+)* }
            }
            pub(crate) fn remove(&mut self, index: usize) -> (Vob, Option<usize>) {
                let kind = self.remove_kind(self.records.get(index).value.kind);
                let old = self.records.swap_remove(index).value;
                let moved = if index < self.records.len() {
                    let moved = self.records.get(index);
                    let owner = moved.owner; let location = moved.value.kind;
                    self.set_owner(location, index); Some(owner)
                } else { None };
                (Vob { base: old.base, kind }, moved)
            }
            fn replace_kind_in_place(&mut self, location: Location, value: VobKind)
                -> Replacement<VobKind, Location>
            {
                match value {
                    $(VobKind::$empty => if location.pool() == PoolKind::Empty {
                        Replacement::Replaced(location.empty_kind(), Location::$empty)
                    } else { Replacement::Different(VobKind::$empty) },)*
                    $($(VobKind::$tag(value) => if location.pool() == PoolKind::$pool {
                        let i = location.index();
                        let old = std::mem::replace(&mut self.$pool.get_mut(i as usize).value, value);
                        Replacement::Replaced(old.restore(location), Location::$tag(i))
                    } else { Replacement::Different(VobKind::$tag(value)) },)+)*
                }
            }
            pub(crate) fn replace_kind(&mut self, index: usize, value: VobKind) -> Result<VobKind> {
                let location = self.records.get(index).value.kind;
                let (new_kind, old_kind) = match self.replace_kind_in_place(location, value) {
                    Replacement::Replaced(old, location) => (location, old),
                    Replacement::Different(kind) => {
                        // Source/destination pools differ. The unpublished destination
                        // page cannot be invalidated by retiring a source tail.
                        let prepared = self.prepare_kind(&kind)?;
                        let old = self.remove_kind(location);
                        (self.commit_kind(prepared, kind, index), old)
                    }
                };
                self.records.get_mut(index).value.kind = new_kind;
                Ok(old_kind)
            }
            pub(crate) fn replace(&mut self, index: usize, value: Vob) -> Result<Vob> {
                let kind = self.replace_kind(index, value.kind)?;
                let base = std::mem::replace(&mut self.records.get_mut(index).value.base, value.base);
                Ok(Vob { base, kind })
            }
        }
    }
}
macro_rules! initial_payload {
    (ZoneFog, $game:ident, $version:ident) => {
        ZoneFog::new($game)
    };
    (LightPreset, $game:ident, $version:ident) => {
        LightPreset::new($game)
    };
    (Mover, $game:ident, $version:ident) => {
        Mover::new($game)
    };
    (CameraFrame, $game:ident, $version:ident) => {{
        let mut value = CameraFrame::new($game);
        value.version = $version;
        value
    }};
    (NpcData, $game:ident, $version:ident) => {
        NpcData::new($game)
    };
    ($ty:ident, $game:ident, $version:ident) => {
        $ty::default()
    };
}
vob_pools! {
    empty { Base, Spot, Stair, StartPoint, Level }
    pools {
        Animate: Animate => [Animate],
        Item: Item => [Item],
        LensFlare: LensFlare => [LensFlare],
        PfxController: PfxController => [PfxController],
        MessageFilter: MessageFilter => [MessageFilter],
        MoverController: MoverController => [MoverController],
        TouchDamage: TouchDamage => [TouchDamage],
        Earthquake: Earthquake => [Earthquake],
        ScreenEffect: ScreenEffect => [ScreenEffect],
        ZoneMusic: ZoneMusic => [ZoneMusic, ZoneMusicDefault],
        ZoneFarPlane: ZoneFarPlane => [ZoneFarPlane, ZoneFarPlaneDefault],
        ZoneFog: ZoneFog => [ZoneFog, ZoneFogDefault],
        Sound: Sound => [Sound],
        SoundDaytime: SoundDaytime => [SoundDaytime],
        LightPreset: LightPreset => [Light],
        Trigger: Trigger => [Trigger, CutsceneTrigger],
        Mover: Mover => [Mover],
        TriggerList: TriggerList => [TriggerList],
        TriggerScript: TriggerScript => [TriggerScript],
        TriggerChangeLevel: TriggerChangeLevel => [TriggerChangeLevel],
        TriggerWorldStart: TriggerWorldStart => [TriggerWorldStart],
        TriggerUntouch: TriggerUntouch => [TriggerUntouch],
        CodeMaster: CodeMaster => [CodeMaster],
        Movable: Movable => [Movable],
        Interactive: Interactive => [Interactive, Ladder, Switch, Wheel, Bed],
        Fire: Fire => [Fire],
        Container: Container => [Container],
        Door: Door => [Door],
        Camera: Camera => [Camera],
        CameraFrame: CameraFrame => [CameraFrame],
        NpcData: NpcData => [Npc]
    }
}

impl Schema {
    pub(crate) fn from_class(class: &[u8]) -> Option<Self> {
        Some(match class {
            b"zCVob" => Self::Base,
            b"zCVobSpot:zCVob" => Self::Spot,
            b"zCVobStair:zCVob" => Self::Stair,
            b"zCVobStartpoint:zCVob" => Self::StartPoint,
            b"zCVobLevelCompo:zCVob" => Self::Level,
            b"zCVobAnimate:zCVob" => Self::Animate,
            b"oCItem:zCVob" => Self::Item,
            b"zCVobLensFlare:zCVob" => Self::LensFlare,
            b"zCPFXControler:zCVob" => Self::PfxController,
            b"zCMessageFilter:zCVob" => Self::MessageFilter,
            b"zCMoverControler:zCVob" => Self::MoverController,
            b"oCTouchDamage:zCTouchDamage:zCVob" => Self::TouchDamage,
            b"zCEarthquake:zCVob" => Self::Earthquake,
            b"zCVobScreenFX:zCVob" => Self::ScreenEffect,
            b"oCZoneMusic:zCVob" => Self::ZoneMusic,
            b"oCZoneMusicDefault:oCZoneMusic:zCVob" => Self::ZoneMusicDefault,
            b"zCZoneVobFarPlane:zCVob" => Self::ZoneFarPlane,
            b"zCZoneVobFarPlaneDefault:zCZoneVobFarPlane:zCVob" => Self::ZoneFarPlaneDefault,
            b"zCZoneZFog:zCVob" => Self::ZoneFog,
            b"zCZoneZFogDefault:zCZoneZFog:zCVob" => Self::ZoneFogDefault,
            b"zCVobSound:zCVob" => Self::Sound,
            b"zCVobSoundDaytime:zCVobSound:zCVob" => Self::SoundDaytime,
            b"zCVobLight:zCVob" => Self::Light,
            b"zCTrigger:zCVob" => Self::Trigger,
            b"oCCSTrigger:zCTrigger:zCVob" => Self::CutsceneTrigger,
            b"zCMover:zCTrigger:zCVob" => Self::Mover,
            b"zCTriggerList:zCTrigger:zCVob" => Self::TriggerList,
            b"oCTriggerScript:zCTrigger:zCVob" => Self::TriggerScript,
            b"oCTriggerChangeLevel:zCTrigger:zCVob" => Self::TriggerChangeLevel,
            b"zCTriggerWorldStart:zCVob" => Self::TriggerWorldStart,
            b"zCTriggerUntouch:zCVob" => Self::TriggerUntouch,
            b"zCCodeMaster:zCVob" => Self::CodeMaster,
            b"oCMOB:zCVob" => Self::Movable,
            b"oCMobInter:oCMOB:zCVob" => Self::Interactive,
            b"oCMobFire:oCMobInter:oCMOB:zCVob" => Self::Fire,
            b"oCMobContainer:oCMobInter:oCMOB:zCVob" => Self::Container,
            b"oCMobDoor:oCMobInter:oCMOB:zCVob" => Self::Door,
            b"oCMobLadder:oCMobInter:oCMOB:zCVob" => Self::Ladder,
            b"oCMobSwitch:oCMobInter:oCMOB:zCVob" => Self::Switch,
            b"oCMobWheel:oCMobInter:oCMOB:zCVob" => Self::Wheel,
            b"oCMobBed:oCMobInter:oCMOB:zCVob" => Self::Bed,
            b"zCCSCamera:zCVob" => Self::Camera,
            b"zCCamTrj_KeyFrame:zCVob" => Self::CameraFrame,

            b"oCNpc:zCVob" => Self::Npc,
            _ => return None,
        })
    }
    pub const fn version_identifier(self, game: GameVersion, record_version: u16) -> u16 {
        match self {
            Self::Trigger => 47105,
            Self::CutsceneTrigger | Self::TriggerScript | Self::TriggerChangeLevel => 24577,
            Self::Mover => 39936,
            Self::TriggerList => {
                if matches!(game, GameVersion::Gothic1) {
                    59776
                } else {
                    5505
                }
            }
            Self::TriggerWorldStart => 52224,
            Self::TriggerUntouch => 193,
            Self::CodeMaster => 0,
            Self::Movable => 47105,
            Self::Interactive | Self::Ladder | Self::Switch | Self::Wheel => 35585,
            Self::Fire => 18433,
            Self::Container | Self::Door => 64513,
            Self::Bed => 61440,
            Self::Camera => {
                if matches!(game, GameVersion::Gothic1) {
                    30720
                } else {
                    33793
                }
            }
            Self::CameraFrame => record_version,
            Self::Npc => 52224,
            Self::Item
            | Self::MessageFilter
            | Self::MoverController
            | Self::ZoneMusic
            | Self::ZoneMusicDefault => 0,
            Self::TouchDamage => 36865,
            Self::Earthquake => 52224,
            Self::Sound | Self::SoundDaytime => 12289,
            Self::LensFlare => match game {
                GameVersion::Gothic1 => 64704,
                GameVersion::Gothic2 => 193,
            },
            Self::ZoneFog | Self::ZoneFogDefault => match game {
                GameVersion::Gothic1 => 64704,
                GameVersion::Gothic2 => 5505,
            },
            Self::Light => match game {
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
