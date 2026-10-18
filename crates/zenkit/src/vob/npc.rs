// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;

/// Separate version-zero `oCNpcTalent` graph object. Signed values are opaque.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NpcTalent {
    pub talent: i32,
    pub value: i32,
    pub skill: i32,
}

/// Raw G2 semicolon-delimited NPC information. Even malformed spelling can be
/// transported; checked decoding requires exactly nine fields. Empty fields are
/// valid, but an empty raw string is not a nine-field view.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct NpcPacked(Vec<u8>);
impl NpcPacked {
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
    pub fn fields(&self) -> Result<[&[u8]; 9]> {
        let mut fields = self.0.split(|b| *b == b';');
        let mut result = [&[][..]; 9];
        for field in &mut result {
            *field = fields.next().ok_or_else(|| bad("NPC packed field count"))?;
        }
        if fields.next().is_some() {
            return Err(bad("NPC packed field count"));
        }
        Ok(result)
    }
    /// Joins exactly nine byte strings. Semicolons in a field are ambiguous and
    /// rejected; the complete output budget is checked before allocating.
    pub fn from_fields(fields: [&[u8]; 9], max_bytes: usize) -> Result<Self> {
        let mut len = 8usize;
        for field in fields {
            if field.contains(&b';') {
                return Err(bad("NPC packed field contains delimiter"));
            }
            len = len
                .checked_add(field.len())
                .filter(|n| *n <= max_bytes)
                .ok_or_else(|| Error::operation(ErrorKind::LimitExceeded, "NPC packed bytes"))?;
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(len)
            .map_err(|_| Error::operation(ErrorKind::LimitExceeded, "NPC packed allocation"))?;
        for (i, field) in fields.into_iter().enumerate() {
            if i != 0 {
                bytes.push(b';');
            }
            bytes.extend_from_slice(field);
        }
        Ok(Self(bytes))
    }
}
impl fmt::Debug for NpcPacked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("NpcPacked").field(&self.0.len()).finish()
    }
}
/// Game-specific fixed arrays and packed information. Conversion requires an
/// explicit replacement; no truncation, inferred defaults or automatic joining.
#[derive(Clone, PartialEq)]
pub enum NpcVersion {
    Gothic1 {
        script_vars: [i32; 50],
        packed: [Vec<u8>; 9],
    },
    Gothic2 {
        script_vars: [i32; 100],
        hit_chance: [i32; 4],
        packed: NpcPacked,
        interruptable_override: i32,
        npc_type: i32,
        spell_mana: i32,
    },
}
impl NpcVersion {
    pub fn new(game: GameVersion) -> Self {
        match game {
            GameVersion::Gothic1 => Self::Gothic1 {
                script_vars: [0; 50],
                packed: std::array::from_fn(|_| Vec::new()),
            },
            GameVersion::Gothic2 => Self::Gothic2 {
                script_vars: [0; 100],
                hit_chance: [0; 4],
                packed: NpcPacked::default(),
                interruptable_override: 0,
                npc_type: 0,
                spell_mana: 0,
            },
        }
    }
    pub const fn game(&self) -> GameVersion {
        match self {
            Self::Gothic1 { .. } => GameVersion::Gothic1,
            Self::Gothic2 { .. } => GameVersion::Gothic2,
        }
    }
    pub fn script_vars(&self) -> &[i32] {
        match self {
            Self::Gothic1 { script_vars, .. } => script_vars,
            Self::Gothic2 { script_vars, .. } => script_vars,
        }
    }
    pub fn script_vars_mut(&mut self) -> &mut [i32] {
        match self {
            Self::Gothic1 { script_vars, .. } => script_vars,
            Self::Gothic2 { script_vars, .. } => script_vars,
        }
    }
}
impl fmt::Debug for NpcVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NpcVersion")
            .field("game", &self.game())
            .field("script_vars", &self.script_vars().len())
            .finish_non_exhaustive()
    }
}
/// Unknown signed news identifiers/spread modes remain ordinary i32 values.
#[derive(Clone, Default, PartialEq)]
pub struct NpcNews {
    pub told: bool,
    pub spread_time: f32,
    pub spread_type: i32,
    pub news_id: i32,
    pub gossip: bool,
    pub guild_victim: bool,
    pub witness_name: Vec<u8>,
    pub offender_name: Vec<u8>,
    pub victim_name: Vec<u8>,
}
impl fmt::Debug for NpcNews {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NpcNews")
            .field("news_id", &self.news_id)
            .field("spread_type", &self.spread_type)
            .field("witness_bytes", &self.witness_name.len())
            .field("offender_bytes", &self.offender_name.len())
            .field("victim_bytes", &self.victim_name.len())
            .finish_non_exhaustive()
    }
}
/// Inventory occurrence. `short_key` is present exactly when the referenced
/// Item's saved flags contain 0x200. Null and normal-mode Items have no key.
/// Shared Items can have different shortcut values at different occurrences.
/// If a saved occurrence references an unfinished ancestor Item definition,
/// tagged formats use exact name lookahead and validate its eventual flags.
/// BINARY uses the exact saved Item schema's fixed flags suffix within its
/// length-bounded definition. Ordinary parsing validates that hint before the
/// graph is returned; object order and per-occurrence shortcut values are retained.
#[derive(Clone, Debug, PartialEq)]
pub struct NpcItem {
    pub item: Link,
    pub short_key: Option<i32>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct NpcSlotEntry {
    pub item: Link,
    pub in_inventory: bool,
}
/// Absence represents an unused slot; a used slot can contain an explicit null.
#[derive(Clone, Default, PartialEq)]
pub struct NpcSlot {
    pub name: Vec<u8>,
    pub entry: Option<NpcSlotEntry>,
}
impl fmt::Debug for NpcSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NpcSlot")
            .field("name_bytes", &self.name.len())
            .field("entry", &self.entry)
            .finish()
    }
}
#[derive(Clone, Default, PartialEq)]
pub struct NpcAiState {
    pub valid: bool,
    pub name: Vec<u8>,
    pub index: i32,
    pub routine: bool,
}
impl fmt::Debug for NpcAiState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NpcAiState")
            .field("valid", &self.valid)
            .field("name_bytes", &self.name.len())
            .field("index", &self.index)
            .field("routine", &self.routine)
            .finish()
    }
}
/// Complete NPC subtype payload, present in normal archives as well as saves.
/// Raw fields retain their fixed prefix and all tagged extension bytes. BINARY
/// cannot delimit those extensions and rejects output with nonempty tails.
/// Counted overlays/news/slots share `sequence_items`; talents/items/used slots
/// share `links`. Strings and retained tails share `copied_bytes`.
#[derive(Clone, PartialEq)]
pub struct NpcData {
    pub instance: Vec<u8>,
    pub model_scale: Vec3,
    pub fatness: f32,
    pub overlays: Vec<Vec<u8>>,
    pub flags: i32,
    pub guild: i32,
    pub guild_true: i32,
    pub level: i32,
    pub xp: i32,
    pub xp_next: i32,
    pub lp: i32,
    pub talents: Vec<Link>,
    pub fight_tactic: i32,
    pub fight_mode: i32,
    pub wounded: bool,
    pub mad: bool,
    pub mad_time: i32,
    pub player: bool,
    pub attributes: [i32; 8],
    pub missions: [i32; 5],
    pub start_ai: Vec<u8>,
    pub version: NpcVersion,
    pub script_vars_tail: Vec<u8>,
    pub waypoint: Vec<u8>,
    pub attitude: i32,
    pub attitude_temp: i32,
    pub name_nr: i32,
    pub spells: [u8; 4],
    pub spells_tail: Vec<u8>,
    pub news: Vec<NpcNews>,
    pub carry: Link,
    pub enemy: Link,
    pub move_lock: bool,
    pub items: Vec<NpcItem>,
    pub slots: Vec<NpcSlot>,
    pub current_state: NpcAiState,
    pub next_state: NpcAiState,
    pub last_ai_state: i32,
    pub has_routine: bool,
    pub routine_changed: bool,
    pub routine_overlay: bool,
    pub routine_overlay_count: i32,
    pub walkmode_routine: i32,
    pub weaponmode_routine: bool,
    pub start_new_routine: bool,
    pub ai_state_driven: i32,
    pub ai_state_pos: Vec3,
    pub current_routine: Vec<u8>,
    pub respawn: bool,
    pub respawn_time: i32,
    pub protection: [i32; 8],
    pub protection_tail: Vec<u8>,
}
impl NpcData {
    /// Empty editable draft. This constructor allocates no buffers.
    pub fn new(game: GameVersion) -> Self {
        Self {
            instance: Vec::new(),
            model_scale: Vec3::default(),
            fatness: 0.,
            overlays: Vec::new(),
            flags: 0,
            guild: 0,
            guild_true: 0,
            level: 0,
            xp: 0,
            xp_next: 0,
            lp: 0,
            talents: Vec::new(),
            fight_tactic: 0,
            fight_mode: 0,
            wounded: false,
            mad: false,
            mad_time: 0,
            player: false,
            attributes: [0; 8],
            missions: [0; 5],
            start_ai: Vec::new(),
            version: NpcVersion::new(game),
            script_vars_tail: Vec::new(),
            waypoint: Vec::new(),
            attitude: 0,
            attitude_temp: 0,
            name_nr: 0,
            spells: [0; 4],
            spells_tail: Vec::new(),
            news: Vec::new(),
            carry: Link::new(Vec::new(), None),
            enemy: Link::new(Vec::new(), None),
            move_lock: false,
            items: Vec::new(),
            slots: Vec::new(),
            current_state: NpcAiState::default(),
            next_state: NpcAiState::default(),
            last_ai_state: 0,
            has_routine: false,
            routine_changed: false,
            routine_overlay: false,
            routine_overlay_count: 0,
            walkmode_routine: 0,
            weaponmode_routine: false,
            start_new_routine: false,
            ai_state_driven: 0,
            ai_state_pos: Vec3::default(),
            current_routine: Vec::new(),
            respawn: false,
            respawn_time: 0,
            protection: [0; 8],
            protection_tail: Vec::new(),
        }
    }
    pub(crate) fn links(&self) -> impl Iterator<Item = &Link> {
        self.talents
            .iter()
            .chain([&self.carry, &self.enemy])
            .chain(self.items.iter().map(|i| &i.item))
            .chain(
                self.slots
                    .iter()
                    .filter_map(|s| s.entry.as_ref().map(|e| &e.item)),
            )
    }
    pub(crate) fn links_mut(&mut self) -> impl Iterator<Item = &mut Link> {
        self.talents
            .iter_mut()
            .chain([&mut self.carry, &mut self.enemy])
            .chain(self.items.iter_mut().map(|i| &mut i.item))
            .chain(
                self.slots
                    .iter_mut()
                    .filter_map(|s| s.entry.as_mut().map(|e| &mut e.item)),
            )
    }
    pub(crate) fn buffers(&self) -> impl Iterator<Item = &[u8]> {
        let (g1, g2) = match &self.version {
            NpcVersion::Gothic1 { packed, .. } => (packed.as_slice(), None),
            NpcVersion::Gothic2 { packed, .. } => (&[][..], Some(packed.as_bytes())),
        };
        [
            &self.instance,
            &self.start_ai,
            &self.script_vars_tail,
            &self.waypoint,
            &self.spells_tail,
            &self.current_state.name,
            &self.next_state.name,
            &self.current_routine,
            &self.protection_tail,
        ]
        .into_iter()
        .map(Vec::as_slice)
        .chain(self.overlays.iter().map(Vec::as_slice))
        .chain(g1.iter().map(Vec::as_slice))
        .chain(g2)
        .chain(self.news.iter().flat_map(|n| {
            [
                n.witness_name.as_slice(),
                n.offender_name.as_slice(),
                n.victim_name.as_slice(),
            ]
        }))
        .chain(self.slots.iter().map(|s| s.name.as_slice()))
    }
    pub(crate) fn check(&self, game: GameVersion) -> Result<()> {
        if self.version.game() != game {
            return Err(bad("NPC game mismatch"));
        }
        for n in [
            self.overlays.len(),
            self.talents.len(),
            self.news.len(),
            self.items.len(),
            self.slots.len(),
        ] {
            if n > i32::MAX as usize {
                return Err(bad("NPC count exceeds i32"));
            }
        }
        Ok(())
    }
}
impl fmt::Debug for NpcData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NpcData")
            .field("instance_bytes", &self.instance.len())
            .field("version", &self.version)
            .field("overlays", &self.overlays.len())
            .field("talents", &self.talents.len())
            .field("news", &self.news.len())
            .field("items", &self.items.len())
            .field("slots", &self.slots.len())
            .finish_non_exhaustive()
    }
}
impl<'a> VobKindRef<'a> {
    pub(crate) fn npc(self) -> Option<&'a NpcData> {
        if let Self::Npc(n) = self {
            Some(n)
        } else {
            None
        }
    }
}

impl VobKindMut<'_> {
    pub(crate) fn npc_mut(&mut self) -> Option<&mut NpcData> {
        if let Self::Npc(n) = self {
            Some(n)
        } else {
            None
        }
    }
}
