//! Versioned materials with raw names and lossless validated texture-vector text.
//!
//! ```
//! use zenkit::{GameVersion, material::{Material, MaterialVersion, TextVec2}};
//! let mut material = Material::new(GameVersion::Gothic2);
//! material.texture_scale = TextVec2::from_bytes(b" 512 256 ".to_vec())?;
//! assert_eq!(material.texture_scale.value().x, 512.0);
//! if let MaterialVersion::Gothic2(fields) = &mut material.version {
//!     fields.environment_mapping = true;
//! }
//! # Ok::<(), zenkit::Error>(())
//! ```
// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use crate::{
    Error, ErrorKind, GameVersion, Result, archive,
    values::{Color, Vec2},
};
use std::fmt;

pub(crate) const CLASS: &[u8] = b"zCMaterial";
fn error(kind: ErrorKind, context: &'static str) -> Error {
    Error::operation(kind, context)
}
fn at(kind: ErrorKind, offset: usize, context: &'static str) -> Error {
    Error::new(kind, offset, context)
}
macro_rules! raw_enum {
    ($name:ident { $($constant:ident = $value:expr),+ $(,)? }) => {
        /// A raw archive enum value. Unknown integers are preserved, never narrowed.
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
raw_enum!(MaterialGroup { UNDEFINED=0, METAL=1, STONE=2, WOOD=3, EARTH=4, WATER=5, SNOW=6, NONE=255 });
raw_enum!(AnimationMapping { NONE=0, LINEAR=1 });
raw_enum!(WaveMode { NONE=0, GROUND_AMBIENT=1, GROUND=2, WALL_AMBIENT=3, WALL=4, ENVIRONMENT=5, WIND_AMBIENT=6, WIND=7 });
raw_enum!(WaveSpeed { NONE=0, SLOW=1, NORMAL=2, FAST=3 });
raw_enum!(AlphaFunction { DEFAULT=0, NONE=1, BLEND=2, ADD=3, SUBTRACT=4, MULTIPLY=5, MULTIPLY_ALT=6 });

/// Exactly two finite ASCII float tokens, retaining their original byte spelling.
/// Whitespace and signed zero survive read/write. Nonfinite values are unsupported
/// because these fields are textual even in BINARY/BIN_SAFE archives.
#[derive(Clone, PartialEq)]
pub struct TextVec2 {
    bytes: Vec<u8>,
    value: Vec2,
}
impl TextVec2 {
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        let value = parse_pair(&bytes)?;
        Ok(Self { bytes, value })
    }
    pub fn from_value(value: Vec2) -> Result<Self> {
        if !value.x.is_finite() || !value.y.is_finite() {
            return Err(error(
                ErrorKind::Unsupported,
                "nonfinite material text vector",
            ));
        }
        Ok(Self {
            bytes: format!("{} {}", value.x, value.y).into_bytes(),
            value,
        })
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn value(&self) -> Vec2 {
        self.value
    }
}
impl Default for TextVec2 {
    fn default() -> Self {
        Self {
            bytes: b"0 0".to_vec(),
            value: Vec2::ZERO,
        }
    }
}
impl fmt::Debug for TextVec2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextVec2")
            .field("bytes", &self.bytes.len())
            .field("value", &self.value)
            .finish()
    }
}
fn vector_space(byte: &u8) -> bool {
    byte.is_ascii_whitespace() || *byte == 0x0b
}
fn parse_pair(bytes: &[u8]) -> Result<Vec2> {
    let mut cursor = 0;
    let mut values = [0.0; 2];
    for value in &mut values {
        while bytes.get(cursor).is_some_and(vector_space) {
            cursor += 1;
        }
        let start = cursor;
        while bytes.get(cursor).is_some_and(|v| !vector_space(v)) {
            cursor += 1;
        }
        if cursor == start {
            return Err(at(
                ErrorKind::InvalidData,
                start,
                "material text vector component",
            ));
        }
        let token = std::str::from_utf8(&bytes[start..cursor])
            .map_err(|_| at(ErrorKind::InvalidData, start, "material text vector ASCII"))?;
        if !token.is_ascii() {
            return Err(at(
                ErrorKind::InvalidData,
                start,
                "material text vector ASCII",
            ));
        }
        *value = token
            .parse::<f32>()
            .map_err(|_| at(ErrorKind::InvalidData, start, "material text vector number"))?;
        if !value.is_finite() {
            return Err(at(
                ErrorKind::Unsupported,
                start,
                "nonfinite material text vector",
            ));
        }
    }
    while bytes.get(cursor).is_some_and(vector_space) {
        cursor += 1;
    }
    if cursor != bytes.len() {
        return Err(at(
            ErrorKind::InvalidData,
            cursor,
            "extra material text vector component",
        ));
    }
    Ok(values.into())
}

/// Fields present in the early Gothic II 0x6002 layout. It has no alpha function.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gothic2EarlyMaterial {
    pub detail_object_scale: f32,
    pub force_occluder: bool,
    pub environment_mapping: bool,
    pub environment_mapping_strength: f32,
    pub wave_mode: WaveMode,
    pub wave_speed: WaveSpeed,
    pub wave_max_amplitude: f32,
    pub wave_grid_size: f32,
    pub ignore_sun: bool,
}
impl Gothic2Material {
    /// Select the early fields, explicitly discarding the modern alpha function.
    pub fn without_alpha(&self) -> Gothic2EarlyMaterial {
        Gothic2EarlyMaterial {
            detail_object_scale: self.detail_object_scale,
            force_occluder: self.force_occluder,
            environment_mapping: self.environment_mapping,
            environment_mapping_strength: self.environment_mapping_strength,
            wave_mode: self.wave_mode,
            wave_speed: self.wave_speed,
            wave_max_amplitude: self.wave_max_amplitude,
            wave_grid_size: self.wave_grid_size,
            ignore_sun: self.ignore_sun,
        }
    }
}
/// Binary-only 0x7401 material layout found in original 0x0705 model data.
/// The five bytes after `wave_speed` are retained without inferring their
/// parameter meanings or assigning modern float/alpha fields to them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gothic2LegacyMaterial {
    pub detail_object_scale: f32,
    pub force_occluder: bool,
    pub environment_mapping: bool,
    pub environment_mapping_strength: f32,
    pub wave_mode: WaveMode,
    pub wave_speed: WaveSpeed,
    pub parameters: [u8; 5],
}
impl Gothic2LegacyMaterial {
    fn write_fields(&self, w: &mut archive::Writer) -> Result<()> {
        use archive::Value as V;
        w.write(b"detailObjectScale", V::Float(self.detail_object_scale))?;
        w.write(b"forceOccluder", V::Bool(self.force_occluder))?;
        w.write(b"environmentalMapping", V::Bool(self.environment_mapping))?;
        w.write(
            b"environmentalMappingStrength",
            V::Float(self.environment_mapping_strength),
        )?;
        w.write(b"waveMode", V::Enum(self.wave_mode.raw()))?;
        w.write(b"waveSpeed", V::Enum(self.wave_speed.raw()))?;
        w.unframed(&self.parameters)
    }
}
impl Gothic2EarlyMaterial {
    /// Select the modern layout, explicitly supplying its additional field.
    pub fn with_alpha(self, alpha_func: AlphaFunction) -> Gothic2Material {
        Gothic2Material {
            detail_object_scale: self.detail_object_scale,
            force_occluder: self.force_occluder,
            environment_mapping: self.environment_mapping,
            environment_mapping_strength: self.environment_mapping_strength,
            wave_mode: self.wave_mode,
            wave_speed: self.wave_speed,
            wave_max_amplitude: self.wave_max_amplitude,
            wave_grid_size: self.wave_grid_size,
            ignore_sun: self.ignore_sun,
            alpha_func,
        }
    }
    fn write_fields(&self, w: &mut archive::Writer) -> Result<()> {
        use archive::Value as V;
        w.write(b"detailObjectScale", V::Float(self.detail_object_scale))?;
        w.write(b"forceOccluder", V::Bool(self.force_occluder))?;
        w.write(b"environmentalMapping", V::Bool(self.environment_mapping))?;
        w.write(
            b"environmentalMappingStrength",
            V::Float(self.environment_mapping_strength),
        )?;
        w.write(b"waveMode", V::Enum(self.wave_mode.raw()))?;
        w.write(b"waveSpeed", V::Enum(self.wave_speed.raw()))?;
        w.write(b"waveMaxAmplitude", V::Float(self.wave_max_amplitude))?;
        w.write(b"waveGridSize", V::Float(self.wave_grid_size))?;
        w.write(b"ignoreSunLight", V::Bool(self.ignore_sun))
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Gothic2Material {
    pub detail_object_scale: f32,
    pub force_occluder: bool,
    pub environment_mapping: bool,
    pub environment_mapping_strength: f32,
    pub wave_mode: WaveMode,
    pub wave_speed: WaveSpeed,
    pub wave_max_amplitude: f32,
    pub wave_grid_size: f32,
    pub ignore_sun: bool,
    pub alpha_func: AlphaFunction,
}
impl Default for Gothic2Material {
    fn default() -> Self {
        Self {
            detail_object_scale: 0.0,
            force_occluder: false,
            environment_mapping: false,
            environment_mapping_strength: 0.0,
            wave_mode: WaveMode::NONE,
            wave_speed: WaveSpeed::NONE,
            wave_max_amplitude: 0.0,
            wave_grid_size: 0.0,
            ignore_sun: false,
            alpha_func: AlphaFunction::NONE,
        }
    }
}
/// G1 has no G2 payload. Changing game requires an explicit caller conversion;
/// output never silently discards or fabricates these fields.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum MaterialVersion {
    #[default]
    Gothic1,
    /// Binary 0x7401 with explicitly retained legacy parameter bytes.
    Gothic2Legacy(Gothic2LegacyMaterial),
    /// Original early Gothic II material record, wire version 0x6002.
    Gothic2Early(Gothic2EarlyMaterial),
    Gothic2(Gothic2Material),
}
impl MaterialVersion {
    pub fn game(&self) -> GameVersion {
        match self {
            Self::Gothic1 => GameVersion::Gothic1,
            Self::Gothic2Legacy(_) | Self::Gothic2Early(_) | Self::Gothic2(_) => {
                GameVersion::Gothic2
            }
        }
    }
}
#[derive(Clone, Default, PartialEq)]
pub struct Material {
    pub name: Vec<u8>,
    pub group: MaterialGroup,
    pub color: Color,
    pub smooth_angle: f32,
    pub texture: Vec<u8>,
    pub texture_scale: TextVec2,
    pub texture_anim_fps: f32,
    pub texture_anim_map_mode: AnimationMapping,
    pub texture_anim_map_dir: TextVec2,
    pub disable_collision: bool,
    pub disable_lightmap: bool,
    pub dont_collapse: bool,
    pub detail_object: Vec<u8>,
    pub default_mapping: Vec2,
    pub version: MaterialVersion,
}
impl fmt::Debug for Material {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Material")
            .field("name_bytes", &self.name.len())
            .field("texture_bytes", &self.texture.len())
            .field("detail_bytes", &self.detail_object.len())
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}
impl Material {
    pub fn new(game: GameVersion) -> Self {
        Self {
            version: match game {
                GameVersion::Gothic1 => MaterialVersion::Gothic1,
                GameVersion::Gothic2 => MaterialVersion::Gothic2(Gothic2Material::default()),
            },
            ..Default::default()
        }
    }
    pub fn version_identifier(&self) -> u16 {
        match self.version {
            MaterialVersion::Gothic1 => 0x4400,
            MaterialVersion::Gothic2Legacy(_) => 0x7401,
            MaterialVersion::Gothic2Early(_) => 0x6002,
            MaterialVersion::Gothic2(_) => 0x9c03,
        }
    }
    pub(crate) fn check_game(&self, game: GameVersion) -> Result<()> {
        if self.version.game() != game {
            return Err(error(
                ErrorKind::Unsupported,
                "material game conversion requires explicit fields",
            ));
        }
        Ok(())
    }
    pub(crate) fn buffers(&self) -> [&[u8]; 5] {
        [
            &self.name,
            &self.texture,
            &self.detail_object,
            self.texture_scale.as_bytes(),
            self.texture_anim_map_dir.as_bytes(),
        ]
    }
    pub(crate) fn read_fields(
        reader: &mut archive::Reader<'_>,
        game: GameVersion,
        wire_version: u16,
        copied: &mut usize,
        limit: usize,
    ) -> Result<Self> {
        if wire_version == 0x7401 && reader.header().format != archive::Format::Binary {
            return Err(at(
                ErrorKind::Unsupported,
                reader.position(),
                "legacy material requires Binary",
            ));
        }
        let mut r = Fields {
            reader,
            copied,
            limit,
        };
        let name = r.string(b"name")?;
        let group = r.enumeration(b"matGroup")?.into();
        let color = match r.field(b"color", archive::ValueType::Color)? {
            archive::Value::Color(v) => v,
            _ => unreachable!(),
        };
        let smooth_angle = r.float(b"smoothAngle")?;
        let texture = r.string(b"texture")?;
        let texture_scale = r.vector(b"texScale")?;
        let texture_anim_fps = r.float(b"texAniFPS")?;
        let texture_anim_map_mode = r.enumeration(b"texAniMapMode")?.into();
        let texture_anim_map_dir = r.vector(b"texAniMapDir")?;
        let disable_collision = r.boolean(b"noCollDet")?;
        let disable_lightmap = r.boolean(b"noLightmap")?;
        let dont_collapse = r.boolean(b"lodDontCollapse")?;
        let detail_object = r.string(b"detailObject")?;
        let version = match game {
            GameVersion::Gothic1 => MaterialVersion::Gothic1,
            GameVersion::Gothic2 => {
                let detail_object_scale = r.float(b"detailObjectScale")?;
                let force_occluder = r.boolean(b"forceOccluder")?;
                let environment_mapping = r.boolean(b"environmentalMapping")?;
                let environment_mapping_strength = r.float(b"environmentalMappingStrength")?;
                let wave_mode = r.enumeration(b"waveMode")?.into();
                let wave_speed = r.enumeration(b"waveSpeed")?.into();
                if wire_version == 0x7401 {
                    let parameters = r
                        .reader
                        .unframed(5)?
                        .take_bytes(5, "legacy material parameters")?
                        .try_into()
                        .expect("exact legacy parameter span");
                    MaterialVersion::Gothic2Legacy(Gothic2LegacyMaterial {
                        detail_object_scale,
                        force_occluder,
                        environment_mapping,
                        environment_mapping_strength,
                        wave_mode,
                        wave_speed,
                        parameters,
                    })
                } else {
                    let fields = Gothic2EarlyMaterial {
                        detail_object_scale,
                        force_occluder,
                        environment_mapping,
                        environment_mapping_strength,
                        wave_mode,
                        wave_speed,
                        wave_max_amplitude: r.float(b"waveMaxAmplitude")?,
                        wave_grid_size: r.float(b"waveGridSize")?,
                        ignore_sun: r.boolean(b"ignoreSunLight")?,
                    };
                    if wire_version == 0x6002 {
                        MaterialVersion::Gothic2Early(fields)
                    } else {
                        MaterialVersion::Gothic2(
                            fields.with_alpha(r.enumeration(b"alphaFunc")?.into()),
                        )
                    }
                }
            }
        };
        let default_mapping = match r.field(b"defaultMapping", archive::ValueType::Vec2)? {
            archive::Value::Vec2(v) => v,
            _ => unreachable!(),
        };
        Ok(Self {
            name,
            group,
            color,
            smooth_angle,
            texture,
            texture_scale,
            texture_anim_fps,
            texture_anim_map_mode,
            texture_anim_map_dir,
            disable_collision,
            disable_lightmap,
            dont_collapse,
            detail_object,
            default_mapping,
            version,
        })
    }
    pub(crate) fn write_fields(&self, w: &mut archive::Writer, game: GameVersion) -> Result<()> {
        self.check_game(game)?;
        if matches!(self.version, MaterialVersion::Gothic2Legacy(_))
            && w.format() != archive::Format::Binary
        {
            return Err(error(
                ErrorKind::Unsupported,
                "legacy material requires Binary",
            ));
        }
        use archive::Value as V;
        w.write(b"name", V::String(self.name.as_slice().into()))?;
        w.write(b"matGroup", V::Enum(self.group.raw()))?;
        w.write(b"color", V::Color(self.color))?;
        w.write(b"smoothAngle", V::Float(self.smooth_angle))?;
        w.write(b"texture", V::String(self.texture.as_slice().into()))?;
        w.write(b"texScale", V::String(self.texture_scale.as_bytes().into()))?;
        w.write(b"texAniFPS", V::Float(self.texture_anim_fps))?;
        w.write(b"texAniMapMode", V::Enum(self.texture_anim_map_mode.raw()))?;
        w.write(
            b"texAniMapDir",
            V::String(self.texture_anim_map_dir.as_bytes().into()),
        )?;
        w.write(b"noCollDet", V::Bool(self.disable_collision))?;
        w.write(b"noLightmap", V::Bool(self.disable_lightmap))?;
        w.write(b"lodDontCollapse", V::Bool(self.dont_collapse))?;
        w.write(
            b"detailObject",
            V::String(self.detail_object.as_slice().into()),
        )?;
        match &self.version {
            MaterialVersion::Gothic1 => {}
            MaterialVersion::Gothic2Legacy(v) => v.write_fields(w)?,
            MaterialVersion::Gothic2Early(v) => v.write_fields(w)?,
            MaterialVersion::Gothic2(v) => {
                v.without_alpha().write_fields(w)?;
                w.write(b"alphaFunc", V::Enum(v.alpha_func.raw()))?;
            }
        }
        w.write(b"defaultMapping", V::Vec2(self.default_mapping))
    }
}
pub(crate) fn check_version(version: u16, game: GameVersion, p: usize) -> Result<()> {
    if !matches!(
        (version, game),
        (0x4400, GameVersion::Gothic1) | (0x6002 | 0x7401 | 0x9c03, GameVersion::Gothic2)
    ) {
        Err(at(
            ErrorKind::Unsupported,
            p,
            "material version or game mismatch",
        ))
    } else {
        Ok(())
    }
}
fn copy(bytes: &[u8], total: &mut usize, limit: usize, p: usize) -> Result<Vec<u8>> {
    *total = total
        .checked_add(bytes.len())
        .filter(|v| *v <= limit)
        .ok_or_else(|| at(ErrorKind::LimitExceeded, p, "material copied bytes"))?;
    let mut out = Vec::new();
    out.try_reserve(bytes.len())
        .map_err(|_| at(ErrorKind::LimitExceeded, p, "material allocation"))?;
    out.extend_from_slice(bytes);
    Ok(out)
}
struct Fields<'r, 'a> {
    reader: &'r mut archive::Reader<'a>,
    copied: &'r mut usize,
    limit: usize,
}
impl<'a> Fields<'_, 'a> {
    fn field(&mut self, name: &[u8], kind: archive::ValueType) -> Result<archive::Value<'a>> {
        let p = self.reader.position();
        let e = self.reader.read(kind)?;
        if e.name.is_some_and(|actual| {
            actual != name && !(name == b"lodDontCollapse" && actual == b"losDontCollapse")
        }) {
            return Err(at(ErrorKind::InvalidData, p, "material field name"));
        }
        Ok(e.value)
    }
    fn string(&mut self, name: &[u8]) -> Result<Vec<u8>> {
        let p = self.reader.position();
        let archive::Value::String(v) = self.field(name, archive::ValueType::String)? else {
            unreachable!()
        };
        copy(&v, self.copied, self.limit, p)
    }
    fn vector(&mut self, name: &[u8]) -> Result<TextVec2> {
        let p = self.reader.position();
        let archive::Value::String(v) = self.field(name, archive::ValueType::String)? else {
            unreachable!()
        };
        let value = parse_pair(&v).map_err(|mut e| {
            e.offset = self
                .reader
                .offset_of(&v)
                .and_then(|base| e.offset.and_then(|n| base.checked_add(n)));
            e
        })?;
        Ok(TextVec2 {
            bytes: copy(&v, self.copied, self.limit, p)?,
            value,
        })
    }
    fn float(&mut self, name: &[u8]) -> Result<f32> {
        match self.field(name, archive::ValueType::Float)? {
            archive::Value::Float(v) => Ok(v),
            _ => unreachable!(),
        }
    }
    fn enumeration(&mut self, name: &[u8]) -> Result<u32> {
        match self.field(name, archive::ValueType::Enum)? {
            archive::Value::Enum(v) => Ok(v),
            _ => unreachable!(),
        }
    }
    fn boolean(&mut self, name: &[u8]) -> Result<bool> {
        match self.field(name, archive::ValueType::Bool)? {
            archive::Value::Bool(v) => Ok(v),
            _ => unreachable!(),
        }
    }
}

/// Mesh-style external name prefix plus one direct material definition.
/// The prefix may differ from the material's own name and is retained separately.
/// References/nulls are unsupported here;
/// use `objects::ObjectArchive` for shared graph identity instead.
#[derive(Clone, PartialEq)]
pub struct MaterialRecord {
    pub prefix: Vec<u8>,
    pub prefix_field_name: Option<Vec<u8>>,
    pub object_name: Vec<u8>,
    pub material: Material,
}
impl fmt::Debug for MaterialRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MaterialRecord")
            .field("prefix_bytes", &self.prefix.len())
            .field(
                "field_name_bytes",
                &self.prefix_field_name.as_ref().map(Vec::len),
            )
            .field("object_name_bytes", &self.object_name.len())
            .field("material", &self.material)
            .finish()
    }
}
impl MaterialRecord {
    /// Read one streaming wrapper. On error this consumes partial archive state;
    /// discard the reader after failure. `max_copied_bytes` bounds this record;
    /// the caller's archive limits bound total input, keys, records and nesting.
    /// Budgets for collections of retained records are the caller's responsibility.
    pub fn read(
        reader: &mut archive::Reader<'_>,
        game: GameVersion,
        max_copied_bytes: usize,
    ) -> Result<Self> {
        Self::read_impl(reader, Some(game), max_copied_bytes)
    }
    /// Read one wrapper, selecting the game from its material definition version.
    /// G1 `0x4400` and G2 `0x6002`/`0x9c03` are supported in all encodings.
    /// Legacy G2 `0x7401` is Binary-only; its unclassified parameters stay raw.
    /// Unknown versions return
    /// [`ErrorKind::Unsupported`] instead of guessing a schema. The selected game
    /// is available through `record.material.version.game()`.
    ///
    /// Limits, prefix/name retention and consuming-on-error behavior match
    /// [`Self::read`]. Discard the archive reader after any error.
    pub fn read_auto(reader: &mut archive::Reader<'_>, max_copied_bytes: usize) -> Result<Self> {
        Self::read_impl(reader, None, max_copied_bytes)
    }
    fn read_impl(
        reader: &mut archive::Reader<'_>,
        game: Option<GameVersion>,
        max_copied_bytes: usize,
    ) -> Result<Self> {
        let mut total = 0;
        let p = reader.position();
        let entry = reader.read(archive::ValueType::String)?;
        let prefix_field_name = entry
            .name
            .map(|n| copy(n, &mut total, max_copied_bytes, p))
            .transpose()?;
        let archive::Value::String(v) = entry.value else {
            unreachable!()
        };
        let prefix = copy(&v, &mut total, max_copied_bytes, p)?;
        let p = reader.position();
        let h = reader
            .begin_object()?
            .ok_or_else(|| at(ErrorKind::InvalidData, p, "expected material record"))?;
        if h.class_name != CLASS {
            return Err(at(
                ErrorKind::WrongObjectType,
                p,
                "expected material definition",
            ));
        }
        let game = match game {
            Some(game) => {
                check_version(h.version, game, p)?;
                game
            }
            None => match h.version {
                0x4400 => GameVersion::Gothic1,
                0x6002 | 0x7401 | 0x9c03 => GameVersion::Gothic2,
                _ => return Err(at(ErrorKind::Unsupported, p, "unknown material version")),
            },
        };
        let object_name = copy(h.object_name, &mut total, max_copied_bytes, p)?;
        let material =
            Material::read_fields(reader, game, h.version, &mut total, max_copied_bytes)?;
        if !reader.end_object()? {
            return Err(at(
                ErrorKind::Unsupported,
                reader.position(),
                "extra material payload",
            ));
        }
        Ok(Self {
            prefix,
            prefix_field_name,
            object_name,
            material,
        })
    }
    /// Write one streaming wrapper. On error this may append partial output;
    /// discard the writer after failure. BINARY has no field names; converting a
    /// record with no prefix field name to a tagged format uses `material`.
    pub fn write(
        &self,
        writer: &mut archive::Writer,
        game: GameVersion,
        max_copied_bytes: usize,
    ) -> Result<()> {
        self.material.check_game(game)?;
        let total = self
            .material
            .buffers()
            .into_iter()
            .chain([
                self.prefix.as_slice(),
                self.object_name.as_slice(),
                self.prefix_field_name.as_deref().unwrap_or_default(),
            ])
            .try_fold(0usize, |sum, v| sum.checked_add(v.len()))
            .ok_or_else(|| error(ErrorKind::LimitExceeded, "material copied bytes"))?;
        if total > max_copied_bytes {
            return Err(error(ErrorKind::LimitExceeded, "material copied bytes"));
        }
        writer.write(
            self.prefix_field_name.as_deref().unwrap_or(b"material"),
            archive::Value::String(self.prefix.as_slice().into()),
        )?;
        writer.begin_object(&self.object_name, CLASS, self.material.version_identifier())?;
        self.material.write_fields(writer, game)?;
        writer.end_object()
    }
}
