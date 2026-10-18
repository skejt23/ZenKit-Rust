// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;
use crate::{
    archive::{self, Value as V, ValueType as T},
    io,
    objects::{self, ObjectMut, ObjectRef},
};

pub(crate) fn check_object(
    class: &[u8],
    version: u16,
    game: GameVersion,
    p: usize,
) -> Result<bool> {
    let invalid = || Error::new(ErrorKind::Unsupported, p, "VOB/visual object version");
    if let Some(schema) = storage::Schema::from_class(class) {
        if matches!(schema, storage::Schema::CameraFrame)
            && !CameraFrame::version_supported(version, game)
        {
            return Err(invalid());
        }
        if schema.version_identifier(game, version) != version {
            return Err(invalid());
        }
        return Ok(true);
    }
    match class {
        b"zCDecal" => {
            if !Decal::version_supported(version, game) {
                return Err(invalid());
            }
        }
        b"zCMesh" | b"zCProgMeshProto" | b"zCParticleFX" | b"zCAICamera" | b"zCModel"
        | b"zCMorphMesh" => {}
        EVENT_MANAGER | AI_MOVE | AI_HUMAN => {
            if version != 0 {
                return Err(invalid());
            }
        }
        _ => return Ok(false),
    }
    Ok(true)
}
pub(super) struct Fields<'r, 'a, 'c> {
    pub(super) r: &'r mut archive::Reader<'a>,
    pub(super) copied: &'c mut usize,
    pub(super) limit: usize,
}
macro_rules! reader_field {
    ($method:ident,$ty:ty,$variant:ident) => {
        pub(super) fn $method(&mut self, name: &[u8]) -> Result<$ty> {
            match self.field(name, T::$variant)? {
                V::$variant(v) => Ok(v),
                _ => unreachable!(),
            }
        }
    };
}
impl<'a> Fields<'_, 'a, '_> {
    pub(super) fn field(&mut self, name: &[u8], kind: T) -> Result<V<'a>> {
        let p = self.r.position();
        let v = self.r.read(kind)?;
        if v.name.is_some_and(|n| n != name) {
            return Err(Error::new(ErrorKind::InvalidData, p, "VOB field name"));
        }
        Ok(v.value)
    }
    pub(super) fn string(&mut self, name: &[u8]) -> Result<Vec<u8>> {
        let p = self.r.position();
        let V::String(v) = self.field(name, T::String)? else {
            unreachable!()
        };
        objects::copy(&v, self.copied, self.limit, p)
    }
    reader_field!(int, i32, Int);
    reader_field!(float, f32, Float);
    reader_field!(byte, u8, Byte);
    reader_field!(boolean, bool, Bool);
    reader_field!(enumeration, u32, Enum);
    reader_field!(vec2, Vec2, Vec2);
    reader_field!(vec3, Vec3, Vec3);
    reader_field!(aabb, Aabb, Aabb);
    reader_field!(matrix, Mat3, Mat3);
    reader_field!(color, crate::values::Color, Color);
    pub(super) fn raw(&mut self, name: &[u8], minimum: usize) -> Result<Vec<u8>> {
        let p = self.r.position();
        let V::Raw(v) = self.field(name, T::Raw(minimum))? else {
            unreachable!()
        };
        objects::copy(&v, self.copied, self.limit, p)
    }
}
pub(crate) fn read_prefix(
    r: &mut archive::Reader<'_>,
    game: GameVersion,
    copied: &mut usize,
    limit: usize,
    mut object: ObjectMut<'_>,
) -> Result<()> {
    let save = r.header().save_game;
    let mut f = Fields { r, copied, limit };
    match &mut object {
        ObjectMut::Vob(v) => {
            let marker = f.int(b"pack")?;
            if marker != 0 {
                let size = if game == GameVersion::Gothic1 { 74 } else { 83 };
                let p = f.r.position();
                let V::Raw(raw) = f.field(b"dataRaw", T::Raw(size))? else {
                    unreachable!()
                };
                let mut input = io::Reader::new(&raw);
                v.base.bbox = Aabb::read(&mut input, "packed VOB bbox")?;
                v.base.position = Vec3::read(&mut input, "packed VOB position")?;
                v.base.rotation = Mat3::read(&mut input, "packed VOB rotation")?;
                let a = input.u8("packed VOB flags")?;
                let b = if game == GameVersion::Gothic1 {
                    u16::from(input.u8("packed VOB flags")?)
                } else {
                    input.u16("packed VOB flags")?
                };
                v.base.show_visual = a & 1 != 0;
                v.base.camera_alignment = CameraAlignment::from_raw(u32::from((a >> 1) & 3));
                v.base.collision_static = a & 8 != 0;
                v.base.collision_dynamic = a & 16 != 0;
                v.base.static_vob = a & 32 != 0;
                v.base.shadow = Shadow::from_raw(u32::from(a >> 6));
                v.base.physics_enabled = b & 64 != 0;
                if let VobVersion::Gothic2(g) = &mut v.base.version {
                    g.animation = AnimationMode::from_raw(u32::from((b >> 7) & 3));
                    g.bias = i32::from((b >> 9) & 31);
                    g.ambient = b & 0x4000 != 0;
                    g.strength = input.f32("packed VOB animation strength")?;
                    g.far_clip = input.f32("packed VOB far clip")?;
                }
                let mask = if game == GameVersion::Gothic1 {
                    0x80
                } else {
                    0x8000
                } | if save { 0 } else { 0x20 };
                v.base.packing = Packing::Packed {
                    marker,
                    reserved: b & mask,
                    tail: objects::copy(&raw[size..], f.copied, f.limit, p)?,
                };
                if b & 1 != 0 {
                    v.base.preset = Some(f.string(b"presetName")?);
                }
                if b & 2 != 0 {
                    v.base.name = Some(f.string(b"vobName")?);
                }
                if b & 4 != 0 {
                    v.base.visual_name = Some(f.string(b"visual")?);
                }
                v.base.visual = (b & 8 != 0).then(|| Link::new(Vec::new(), None));
                v.base.ai = (b & 16 != 0).then(|| Link::new(Vec::new(), None));
                v.base.event_manager = (save && b & 32 != 0).then(|| Link::new(Vec::new(), None));
            } else {
                v.base.packing = Packing::Unpacked;
                v.base.preset = Some(f.string(b"presetName")?);
                v.base.bbox = f.aabb(b"bbox3DWS")?;
                v.base.rotation = f.matrix(b"trafoOSToWSRot")?;
                v.base.position = f.vec3(b"trafoOSToWSPos")?;
                v.base.name = Some(f.string(b"vobName")?);
                v.base.visual_name = Some(f.string(b"visual")?);
                v.base.show_visual = f.boolean(b"showVisual")?;
                v.base.camera_alignment =
                    CameraAlignment::from_raw(f.enumeration(b"visualCamAlign")?);
                if let VobVersion::Gothic2(g) = &mut v.base.version {
                    g.animation = AnimationMode::from_raw(f.enumeration(b"visualAniMode")?);
                    g.strength = f.float(b"visualAniModeStrength")?;
                    g.far_clip = f.float(b"vobFarClipZScale")?;
                }
                v.base.collision_static = f.boolean(b"cdStatic")?;
                v.base.collision_dynamic = f.boolean(b"cdDyn")?;
                v.base.static_vob = f.boolean(b"staticVob")?;
                v.base.shadow = Shadow::from_raw(f.enumeration(b"dynShadow")?);
                if let VobVersion::Gothic2(g) = &mut v.base.version {
                    g.bias = f.int(b"zbias")?;
                    g.ambient = f.boolean(b"isAmbient")?;
                }
                v.base.visual = Some(Link::new(Vec::new(), None));
                v.base.ai = Some(Link::new(Vec::new(), None));
            }
        }
        ObjectMut::Visual(Visual {
            kind: VisualKind::Decal(d),
            ..
        }) => {
            d.name = f.string(b"name")?;
            d.dimension = f.vec2(b"decalDim")?;
            d.offset = f.vec2(b"decalOffset")?;
            d.two_sided = f.boolean(b"decal2Sided")?;
            d.alpha = AlphaFunction::from_raw(f.enumeration(b"decalAlphaFunc")?);
            d.fps = f.float(b"decalTexAniFPS")?;
            if let DecalVersion::Gothic2(g) = &mut d.version {
                g.alpha_weight = f.byte(b"decalAlphaWeight")?;
                g.ignore_daylight = f.boolean(b"ignoreDayLight")?;
            }
        }
        ObjectMut::EventManager(v) => {
            v.cleared = f.boolean(b"cleared")?;
            v.active = f.boolean(b"active")?;
        }
        ObjectMut::AiHuman(v) => {
            v.water_level = f.int(b"waterLevel")?;
            v.floor_y = f.float(b"floorY")?;
            v.water_y = f.float(b"waterY")?;
            v.ceil_y = f.float(b"ceilY")?;
            v.feet_y = f.float(b"feetY")?;
            v.head_y = f.float(b"headY")?;
            v.fall_dist_y = f.float(b"fallDistY")?;
            v.fall_start_y = f.float(b"fallStartY")?;
        }
        _ => {}
    }
    Ok(())
}
pub(super) fn read_rigid_body(f: &mut Fields<'_, '_, '_>) -> Result<RigidBody> {
    Ok(RigidBody {
        velocity: f.vec3(b"vel")?,
        mode: f.byte(b"mode")?,
        gravity: f.boolean(b"gravOn")?,
        gravity_scale: f.float(b"gravScale")?,
        slide: f.vec3(b"slideDir")?,
    })
}
pub(super) fn write_rigid_body(w: &mut archive::Writer, body: &RigidBody) -> Result<()> {
    w.write(b"vel", V::Vec3(body.velocity))?;
    w.write(b"mode", V::Byte(body.mode))?;
    w.write(b"gravOn", V::Bool(body.gravity))?;
    w.write(b"gravScale", V::Float(body.gravity_scale))?;
    w.write(b"slideDir", V::Vec3(body.slide))
}
pub(crate) fn read_suffix(
    r: &mut archive::Reader<'_>,
    mut object: ObjectMut<'_>,
    copied: &mut usize,
    limit: usize,
) -> Result<()> {
    let save = r.header().save_game;
    let mut f = Fields { r, copied, limit };
    match &mut object {
        ObjectMut::Vob(v) if save => {
            let mut state = SaveState {
                sleep: f.byte(b"sleepMode")?,
                next_timer: f.float(b"nextOnTimer")?,
                rigid_body: None,
            };
            if v.base.physics_enabled {
                state.rigid_body = Some(read_rigid_body(&mut f)?);
            }
            v.base.state = Some(state);
        }
        ObjectMut::AiHuman(v) => {
            v.walk_mode = f.int(b"walkMode")?;
            v.weapon_mode = f.int(b"weaponMode")?;
            v.weapon_mode_last = f.int(b"wmodeLast")?;
            v.weapon_mode_select = f.int(b"wmodeSelect")?;
            v.change_weapon = f.boolean(b"changeWeapon")?;
            v.action_mode = f.int(b"actionMode")?;
        }
        _ => {}
    }
    if let ObjectMut::Vob(mut v) = object {
        let game = v.game();
        super::special_codec::read(&mut f, &mut v.kind, game, save)?;
    }
    Ok(())
}
pub(crate) fn write_prefix(
    w: &mut archive::Writer,
    object: ObjectRef<'_>,
    limits: archive::Limits,
) -> Result<()> {
    match object {
        ObjectRef::Vob(v) => match &v.base.packing {
            Packing::Packed { marker, tail, .. } => {
                if w.format() == archive::Format::Binary && !tail.is_empty() {
                    return Err(Error::operation(
                        ErrorKind::Unsupported,
                        "packed VOB tail has no BINARY delimiter",
                    ));
                }
                w.write(b"pack", V::Int(*marker))?;
                let size = if v.game() == GameVersion::Gothic1 {
                    74usize
                } else {
                    83
                };
                let size = size.checked_add(tail.len()).ok_or_else(|| {
                    Error::operation(ErrorKind::LimitExceeded, "packed VOB bytes")
                })?;
                let field_fits = match w.format() {
                    archive::Format::Ascii => size
                        .checked_mul(2)
                        .and_then(|n| n.checked_add(b"dataRaw=raw:".len()))
                        .is_some_and(|n| n <= limits.field_bytes),
                    archive::Format::BinSafe => size <= limits.field_bytes.min(u16::MAX as usize),
                    archive::Format::Binary => size <= limits.field_bytes,
                };
                if size > limits.bytes || !field_fits {
                    return Err(Error::operation(
                        ErrorKind::LimitExceeded,
                        "packed VOB bytes",
                    ));
                }
                let mut raw = io::Writer::new(size);
                v.base.bbox.write(&mut raw, "packed VOB bbox")?;
                v.base.position.write(&mut raw, "packed VOB position")?;
                v.base.rotation.write(&mut raw, "packed VOB rotation")?;
                let (a, b) = v.packed_flags()?.expect("packed variant");
                raw.u8(a, "packed VOB flags")?;
                if let VobVersion::Gothic2(g) = v.base.version {
                    raw.u16(b, "packed VOB flags")?;
                    raw.f32(g.strength, "packed VOB animation strength")?;
                    raw.f32(g.far_clip, "packed VOB far clip")?;
                } else {
                    raw.u8(b as u8, "packed VOB flags")?;
                }
                raw.bytes(tail, "packed VOB tail")?;
                w.write(b"dataRaw", V::Raw(raw.into_inner().into()))?;
                for (name, value) in [
                    (b"presetName".as_slice(), &v.base.preset),
                    (b"vobName", &v.base.name),
                    (b"visual", &v.base.visual_name),
                ] {
                    if let Some(value) = value {
                        w.write(name, V::String(value.as_slice().into()))?;
                    }
                }
            }
            Packing::Unpacked => {
                w.write(b"pack", V::Int(0))?;
                w.write(
                    b"presetName",
                    V::String(v.base.preset.as_deref().expect("validated presence").into()),
                )?;
                w.write(b"bbox3DWS", V::Aabb(v.base.bbox))?;
                w.write(b"trafoOSToWSRot", V::Mat3(v.base.rotation))?;
                w.write(b"trafoOSToWSPos", V::Vec3(v.base.position))?;
                w.write(
                    b"vobName",
                    V::String(v.base.name.as_deref().expect("validated presence").into()),
                )?;
                w.write(
                    b"visual",
                    V::String(
                        v.base
                            .visual_name
                            .as_deref()
                            .expect("validated presence")
                            .into(),
                    ),
                )?;
                w.write(b"showVisual", V::Bool(v.base.show_visual))?;
                w.write(b"visualCamAlign", V::Enum(v.base.camera_alignment.raw()))?;
                if let VobVersion::Gothic2(g) = v.base.version {
                    w.write(b"visualAniMode", V::Enum(g.animation.raw()))?;
                    w.write(b"visualAniModeStrength", V::Float(g.strength))?;
                    w.write(b"vobFarClipZScale", V::Float(g.far_clip))?;
                }
                w.write(b"cdStatic", V::Bool(v.base.collision_static))?;
                w.write(b"cdDyn", V::Bool(v.base.collision_dynamic))?;
                w.write(b"staticVob", V::Bool(v.base.static_vob))?;
                w.write(b"dynShadow", V::Enum(v.base.shadow.raw()))?;
                if let VobVersion::Gothic2(g) = v.base.version {
                    w.write(b"zbias", V::Int(g.bias))?;
                    w.write(b"isAmbient", V::Bool(g.ambient))?;
                }
            }
        },
        ObjectRef::Visual(Visual {
            kind: VisualKind::Decal(d),
            ..
        }) => {
            w.write(b"name", V::String(d.name.as_slice().into()))?;
            w.write(b"decalDim", V::Vec2(d.dimension))?;
            w.write(b"decalOffset", V::Vec2(d.offset))?;
            w.write(b"decal2Sided", V::Bool(d.two_sided))?;
            w.write(b"decalAlphaFunc", V::Enum(d.alpha.raw()))?;
            w.write(b"decalTexAniFPS", V::Float(d.fps))?;
            if let DecalVersion::Gothic2(g) = d.version {
                w.write(b"decalAlphaWeight", V::Byte(g.alpha_weight))?;
                w.write(b"ignoreDayLight", V::Bool(g.ignore_daylight))?;
            }
        }
        ObjectRef::EventManager(v) => {
            w.write(b"cleared", V::Bool(v.cleared))?;
            w.write(b"active", V::Bool(v.active))?;
        }
        ObjectRef::AiHuman(v) => {
            w.write(b"waterLevel", V::Int(v.water_level))?;
            w.write(b"floorY", V::Float(v.floor_y))?;
            w.write(b"waterY", V::Float(v.water_y))?;
            w.write(b"ceilY", V::Float(v.ceil_y))?;
            w.write(b"feetY", V::Float(v.feet_y))?;
            w.write(b"headY", V::Float(v.head_y))?;
            w.write(b"fallDistY", V::Float(v.fall_dist_y))?;
            w.write(b"fallStartY", V::Float(v.fall_start_y))?;
        }
        _ => {}
    }
    Ok(())
}
pub(crate) fn write_suffix(w: &mut archive::Writer, object: ObjectRef<'_>) -> Result<()> {
    match object {
        ObjectRef::Vob(VobRef {
            base: VobBase { state: Some(s), .. },
            ..
        }) => {
            w.write(b"sleepMode", V::Byte(s.sleep))?;
            w.write(b"nextOnTimer", V::Float(s.next_timer))?;
            if let Some(b) = s.rigid_body {
                write_rigid_body(w, &b)?;
            }
        }
        ObjectRef::AiHuman(v) => {
            w.write(b"walkMode", V::Int(v.walk_mode))?;
            w.write(b"weaponMode", V::Int(v.weapon_mode))?;
            w.write(b"wmodeLast", V::Int(v.weapon_mode_last))?;
            w.write(b"wmodeSelect", V::Int(v.weapon_mode_select))?;
            w.write(b"changeWeapon", V::Bool(v.change_weapon))?;
            w.write(b"actionMode", V::Int(v.action_mode))?;
        }
        _ => {}
    }
    if let ObjectRef::Vob(v) = object {
        super::special_codec::write(w, v.kind, v.game())?;
    }
    Ok(())
}
