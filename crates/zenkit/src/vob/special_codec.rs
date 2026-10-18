// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::codec::Fields;
use super::*;
use crate::archive::{self, Value as V};

pub(super) fn read(
    f: &mut Fields<'_, '_, '_>,
    kind: &mut VobKindMut<'_>,
    game: GameVersion,
    save: bool,
) -> Result<()> {
    match kind {
        VobKindMut::Animate(v) => {
            v.start_on = f.boolean(b"startOn")?;
            v.running = if save {
                Some(f.boolean(b"isRunning")?)
            } else {
                None
            };
        }
        VobKindMut::Item(v) => {
            v.instance = f.string(b"itemInstance")?;
            v.state = if save {
                Some(ItemState {
                    amount: f.int(b"amount")?,
                    flags: f.int(b"flags")?,
                })
            } else {
                None
            };
        }
        VobKindMut::LensFlare(v) => {
            v.effect = f.string(b"lensflareFX")?;
        }
        VobKindMut::PfxController(v) => {
            v.name = f.string(b"pfxName")?;
            v.kill_when_done = f.boolean(b"killVobWhenDone")?;
            v.initially_running = f.boolean(b"pfxStartOn")?;
        }
        VobKindMut::MessageFilter(v) => {
            v.target = f.string(b"triggerTarget")?;
            v.on_trigger = MessageAction::from_raw(f.enumeration(b"onTrigger")?);
            v.on_untrigger = MessageAction::from_raw(f.enumeration(b"onUntrigger")?);
        }
        VobKindMut::MoverController(v) => {
            v.target = f.string(b"triggerTarget")?;
            v.message = MoverMessage::from_raw(f.enumeration(b"moverMessage")?);
            v.key = f.int(b"gotoFixedKey")?;
        }
        VobKindMut::TouchDamage(v) => {
            v.damage = f.float(b"damage")?;
            v.barrier = f.boolean(b"Barrier")?;
            v.blunt = f.boolean(b"Blunt")?;
            v.edge = f.boolean(b"Edge")?;
            v.fire = f.boolean(b"Fire")?;
            v.fly = f.boolean(b"Fly")?;
            v.magic = f.boolean(b"Magic")?;
            v.point = f.boolean(b"Point")?;
            v.fall = f.boolean(b"Fall")?;
            v.repeat_delay = f.float(b"damageRepeatDelaySec")?;
            v.volume_scale = f.float(b"damageVolDownScale")?;
            v.collision = TouchCollision::from_raw(f.enumeration(b"damageCollType")?);
        }
        VobKindMut::Earthquake(v) => {
            v.radius = f.float(b"radius")?;
            v.duration = f.float(b"timeSec")?;
            v.amplitude = f.vec3(b"amplitudeCM")?;
        }
        VobKindMut::ZoneMusic(v) | VobKindMut::ZoneMusicDefault(v) => {
            v.enabled = f.boolean(b"enabled")?;
            v.priority = f.int(b"priority")?;
            v.ellipsoid = f.boolean(b"ellipsoid")?;
            v.reverb = f.float(b"reverbLevel")?;
            v.volume = f.float(b"volumeLevel")?;
            v.looped = f.boolean(b"loop")?;
            v.state = if save {
                Some(MusicState {
                    local_enabled: f.boolean(b"local_enabled")?,
                    day_entrance_done: f.boolean(b"dayEntranceDone")?,
                    night_entrance_done: f.boolean(b"nightEntranceDone")?,
                })
            } else {
                None
            };
        }
        VobKindMut::ZoneFarPlane(v) | VobKindMut::ZoneFarPlaneDefault(v) => {
            v.far_plane = f.float(b"vobFarPlaneZ")?;
            v.inner_range = f.float(b"innerRangePerc")?;
        }
        VobKindMut::ZoneFog(v) | VobKindMut::ZoneFogDefault(v) => {
            v.range_center = f.float(b"fogRangeCenter")?;
            v.inner_range = f.float(b"innerRangePerc")?;
            v.color = f.color(b"fogColor")?;
            v.version = if game == GameVersion::Gothic2 {
                FogVersion::Gothic2(Gothic2Fog {
                    fade_out_sky: f.boolean(b"fadeOutSky")?,
                    override_color: f.boolean(b"overrideColor")?,
                })
            } else {
                FogVersion::Gothic1
            };
        }
        VobKindMut::ScreenEffect(v) => {
            v.state = if save {
                let n = if game == GameVersion::Gothic1 { 20 } else { 48 };
                Some(ScreenState {
                    game,
                    blend: f.raw(b"blend", n)?,
                    cinema: f.raw(b"cinema", n)?,
                    fov_morph: f.raw(b"fovMorph", n)?,
                    fov_saved: f.vec2(b"fovSaved")?,
                    fov_saved_first: f.vec2(b"fovSaved1st")?,
                })
            } else {
                None
            };
        }
        VobKindMut::Sound(v) => read_sound(f, v, save)?,
        VobKindMut::SoundDaytime(v) => {
            read_sound(f, &mut v.sound, save)?;
            v.start_time = f.float(b"sndStartTime")?;
            v.end_time = f.float(b"sndEndTime")?;
            v.second_name = f.string(b"sndName2")?;
        }
        VobKindMut::Light(v) => **v = read_light(f, game)?,
        _ => {}
    }
    Ok(())
}
fn read_sound(f: &mut Fields<'_, '_, '_>, v: &mut Sound, save: bool) -> Result<()> {
    v.volume = f.float(b"sndVolume")?;
    v.mode = SoundMode::from_raw(f.enumeration(b"sndMode")?);
    v.random_delay = f.float(b"sndRandDelay")?;
    v.random_delay_var = f.float(b"sndRandDelayVar")?;
    v.initially_playing = f.boolean(b"sndStartOn")?;
    v.ambient_3d = f.boolean(b"sndAmbient3D")?;
    v.obstruction = f.boolean(b"sndObstruction")?;
    v.cone_angle = f.float(b"sndConeAngle")?;
    v.volume_type = SoundVolume::from_raw(f.enumeration(b"sndVolType")?);
    v.radius = f.float(b"sndRadius")?;
    v.name = f.string(b"sndName")?;
    v.state = if save {
        Some(SoundState {
            running: f.boolean(b"soundIsRunning")?,
            allowed_to_run: f.boolean(b"soundAllowedToRun")?,
        })
    } else {
        None
    };
    Ok(())
}
pub(super) fn read_light(f: &mut Fields<'_, '_, '_>, game: GameVersion) -> Result<LightPreset> {
    let mut v = LightPreset::new(game);
    v.preset = f.string(b"lightPresetInUse")?;
    v.light_type = LightType::from_raw(f.enumeration(b"lightType")?);
    v.range = f.float(b"range")?;
    v.color = f.color(b"color")?;
    v.cone_angle = f.float(b"spotConeAngle")?;
    let is_static = f.boolean(b"lightStatic")?;
    v.quality = LightQuality::from_raw(f.enumeration(b"lightQuality")?);
    v.lensflare = f.string(b"lensflareFX")?;
    v.animation = if is_static {
        None
    } else {
        let mut a = LightAnimation::new(game);
        a.on = f.boolean(b"turnedOn")?;
        a.range_scale = RangeAnimation::from_bytes(f.string(b"rangeAniScale")?);
        a.range_fps = f.float(b"rangeAniFPS")?;
        a.range_smooth = f.boolean(b"rangeAniSmooth")?;
        a.colors = ColorAnimation::from_bytes(f.string(b"colorAniList")?);
        a.color_fps = f.float(b"colorAniFPS")?;
        a.color_smooth = f.boolean(b"colorAniSmooth")?;
        if game == GameVersion::Gothic2 {
            a.can_move = Some(f.boolean(b"canMove")?);
        }
        Some(a)
    };
    Ok(v)
}
pub(super) fn write(
    w: &mut archive::Writer,
    kind: VobKindRef<'_>,
    game: GameVersion,
) -> Result<()> {
    match kind {
        VobKindRef::Animate(v) => {
            w.write(b"startOn", V::Bool(v.start_on))?;
            if let Some(state) = v.running {
                w.write(b"isRunning", V::Bool(state))?;
            }
        }
        VobKindRef::Item(v) => {
            w.write(b"itemInstance", V::String(v.instance.as_slice().into()))?;
            if let Some(state) = v.state {
                w.write(b"amount", V::Int(state.amount))?;
                w.write(b"flags", V::Int(state.flags))?;
            }
        }
        VobKindRef::LensFlare(v) => {
            w.write(b"lensflareFX", V::String(v.effect.as_slice().into()))?;
        }
        VobKindRef::PfxController(v) => {
            w.write(b"pfxName", V::String(v.name.as_slice().into()))?;
            w.write(b"killVobWhenDone", V::Bool(v.kill_when_done))?;
            w.write(b"pfxStartOn", V::Bool(v.initially_running))?;
        }
        VobKindRef::MessageFilter(v) => {
            w.write(b"triggerTarget", V::String(v.target.as_slice().into()))?;
            w.write(b"onTrigger", V::Enum(v.on_trigger.raw()))?;
            w.write(b"onUntrigger", V::Enum(v.on_untrigger.raw()))?;
        }
        VobKindRef::MoverController(v) => {
            w.write(b"triggerTarget", V::String(v.target.as_slice().into()))?;
            w.write(b"moverMessage", V::Enum(v.message.raw()))?;
            w.write(b"gotoFixedKey", V::Int(v.key))?;
        }
        VobKindRef::TouchDamage(v) => {
            w.write(b"damage", V::Float(v.damage))?;
            w.write(b"Barrier", V::Bool(v.barrier))?;
            w.write(b"Blunt", V::Bool(v.blunt))?;
            w.write(b"Edge", V::Bool(v.edge))?;
            w.write(b"Fire", V::Bool(v.fire))?;
            w.write(b"Fly", V::Bool(v.fly))?;
            w.write(b"Magic", V::Bool(v.magic))?;
            w.write(b"Point", V::Bool(v.point))?;
            w.write(b"Fall", V::Bool(v.fall))?;
            w.write(b"damageRepeatDelaySec", V::Float(v.repeat_delay))?;
            w.write(b"damageVolDownScale", V::Float(v.volume_scale))?;
            w.write(b"damageCollType", V::Enum(v.collision.raw()))?;
        }
        VobKindRef::Earthquake(v) => {
            w.write(b"radius", V::Float(v.radius))?;
            w.write(b"timeSec", V::Float(v.duration))?;
            w.write(b"amplitudeCM", V::Vec3(v.amplitude))?;
        }
        VobKindRef::ZoneMusic(v) | VobKindRef::ZoneMusicDefault(v) => {
            w.write(b"enabled", V::Bool(v.enabled))?;
            w.write(b"priority", V::Int(v.priority))?;
            w.write(b"ellipsoid", V::Bool(v.ellipsoid))?;
            w.write(b"reverbLevel", V::Float(v.reverb))?;
            w.write(b"volumeLevel", V::Float(v.volume))?;
            w.write(b"loop", V::Bool(v.looped))?;
            if let Some(state) = v.state {
                w.write(b"local_enabled", V::Bool(state.local_enabled))?;
                w.write(b"dayEntranceDone", V::Bool(state.day_entrance_done))?;
                w.write(b"nightEntranceDone", V::Bool(state.night_entrance_done))?;
            }
        }
        VobKindRef::ZoneFarPlane(v) | VobKindRef::ZoneFarPlaneDefault(v) => {
            w.write(b"vobFarPlaneZ", V::Float(v.far_plane))?;
            w.write(b"innerRangePerc", V::Float(v.inner_range))?;
        }
        VobKindRef::ZoneFog(v) | VobKindRef::ZoneFogDefault(v) => {
            w.write(b"fogRangeCenter", V::Float(v.range_center))?;
            w.write(b"innerRangePerc", V::Float(v.inner_range))?;
            w.write(b"fogColor", V::Color(v.color))?;
            if let FogVersion::Gothic2(g) = v.version {
                w.write(b"fadeOutSky", V::Bool(g.fade_out_sky))?;
                w.write(b"overrideColor", V::Bool(g.override_color))?;
            }
        }
        VobKindRef::ScreenEffect(v) => {
            if let Some(s) = &v.state {
                let n = if game == GameVersion::Gothic1 { 20 } else { 48 };
                for (name, bytes) in [
                    (b"blend".as_slice(), &s.blend),
                    (b"cinema", &s.cinema),
                    (b"fovMorph", &s.fov_morph),
                ] {
                    if w.format() == archive::Format::Binary && bytes.len() != n {
                        return Err(Error::operation(
                            ErrorKind::Unsupported,
                            "screen effect tail has no BINARY delimiter",
                        ));
                    }
                    w.write(name, V::Raw(bytes.as_slice().into()))?;
                }
                w.write(b"fovSaved", V::Vec2(s.fov_saved))?;
                w.write(b"fovSaved1st", V::Vec2(s.fov_saved_first))?;
            }
        }
        VobKindRef::Sound(v) => write_sound(w, v)?,
        VobKindRef::SoundDaytime(v) => {
            write_sound(w, &v.sound)?;
            w.write(b"sndStartTime", V::Float(v.start_time))?;
            w.write(b"sndEndTime", V::Float(v.end_time))?;
            w.write(b"sndName2", V::String(v.second_name.as_slice().into()))?;
        }
        VobKindRef::Light(v) => write_light(w, v)?,
        _ => {}
    }
    Ok(())
}
fn write_sound(w: &mut archive::Writer, v: &Sound) -> Result<()> {
    w.write(b"sndVolume", V::Float(v.volume))?;
    w.write(b"sndMode", V::Enum(v.mode.raw()))?;
    w.write(b"sndRandDelay", V::Float(v.random_delay))?;
    w.write(b"sndRandDelayVar", V::Float(v.random_delay_var))?;
    w.write(b"sndStartOn", V::Bool(v.initially_playing))?;
    w.write(b"sndAmbient3D", V::Bool(v.ambient_3d))?;
    w.write(b"sndObstruction", V::Bool(v.obstruction))?;
    w.write(b"sndConeAngle", V::Float(v.cone_angle))?;
    w.write(b"sndVolType", V::Enum(v.volume_type.raw()))?;
    w.write(b"sndRadius", V::Float(v.radius))?;
    w.write(b"sndName", V::String(v.name.as_slice().into()))?;
    if let Some(s) = v.state {
        w.write(b"soundIsRunning", V::Bool(s.running))?;
        w.write(b"soundAllowedToRun", V::Bool(s.allowed_to_run))?;
    }
    Ok(())
}
pub(super) fn write_light(w: &mut archive::Writer, v: &LightPreset) -> Result<()> {
    w.write(b"lightPresetInUse", V::String(v.preset.as_slice().into()))?;
    w.write(b"lightType", V::Enum(v.light_type.raw()))?;
    w.write(b"range", V::Float(v.range))?;
    w.write(b"color", V::Color(v.color))?;
    w.write(b"spotConeAngle", V::Float(v.cone_angle))?;
    w.write(b"lightStatic", V::Bool(v.animation.is_none()))?;
    w.write(b"lightQuality", V::Enum(v.quality.raw()))?;
    w.write(b"lensflareFX", V::String(v.lensflare.as_slice().into()))?;
    if let Some(a) = &v.animation {
        w.write(b"turnedOn", V::Bool(a.on))?;
        w.write(b"rangeAniScale", V::String(a.range_scale.as_bytes().into()))?;
        w.write(b"rangeAniFPS", V::Float(a.range_fps))?;
        w.write(b"rangeAniSmooth", V::Bool(a.range_smooth))?;
        w.write(b"colorAniList", V::String(a.colors.as_bytes().into()))?;
        w.write(b"colorAniFPS", V::Float(a.color_fps))?;
        w.write(b"colorAniSmooth", V::Bool(a.color_smooth))?;
        if let Some(can_move) = a.can_move {
            w.write(b"canMove", V::Bool(can_move))?;
        }
    }
    Ok(())
}
