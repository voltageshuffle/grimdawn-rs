//! Sector data (map chunk 0x18, `SectorDataManager::Load`), per-level sector
//! layers (level chunk 23, `SectorLayers::Load`) and instance
//! groups (map chunk 0x11, `InstanceGroupManager::Load`/`InstanceGroup::Load`).
use crate::util::Reader;
use anyhow::{Result, bail};

/// The 14 sector-data layers, in file order (SectorDataManager::Load switch).
pub const SECTOR_LAYER_NAMES: [&str; 14] = [
    "fog",
    "name",
    "ambient",
    "boss",
    "daynight",
    "bloom",
    "climate",
    "damage",
    "adjustment",
    "levellimit",
    "riftgate",
    "viewdistance",
    "pvp",
    "challenge",
];

/// `SectorDataManager::ReadSectorData` common header.
#[derive(Debug, Clone)]
pub struct SectorData {
    pub skipped: u32,
    pub name: String,
    /// 4 ints stored reversed into SectorData+40..52 (inferred: id/color).
    pub ints: [i32; 4],
    /// 8 dwords at SectorData+56..84.
    pub values: [u32; 8],
}

#[derive(Debug, Clone)]
pub enum SectorEntry {
    /// 22 dwords of fog parameters.
    Fog(SectorData, Vec<u32>),
    /// Display tag (e.g. `tagMapArea...`), u32 flag, v>1: u32 flag.
    Name {
        base: SectorData,
        tag: String,
        flag_a: u32,
        flag_b: Option<u32>,
    },
    /// Ambient sound/music paths (2 or 3 strings).
    Ambient {
        base: SectorData,
        sounds: Vec<String>,
    },
    Boss(SectorData),
}

impl SectorEntry {
    /// The `SectorDataManager::ReadSectorData` header every entry kind starts with.
    pub fn base(&self) -> &SectorData {
        match self {
            Self::Fog(b, _)
            | Self::Ambient { base: b, .. }
            | Self::Name { base: b, .. }
            | Self::Boss(b) => b,
        }
    }
}

/// The `FogSectorData` (layer 0, "environment") fields, per
/// `SectorDataManager::ReadFogData` (22 dwords stored in order after the common header) and
/// their consumer `AmbianceManager::UpdateModifierSlot`: `fogColor` (rgba),
/// `lightColorMod`, `skyAmbientMod`,
/// `groundAmbientMod` (rgba each; multiplied into the day/night
/// colours by `GetLightColor` / `GetSkyAmbientColor` /
/// `GetGroundAmbientColor`), `depthFogDensity` / `heightFogDensity`,
/// then the depth fog start / end and height fog start / end (ints,
/// metres; the constructor defaults 10, 50, 10, 0). Fog: `AmbianceManager::UpdateModifierSlot` and
/// `GraphicsDeferredRenderer::RenderFog` (gd-render `render/post.rs`).
#[derive(Debug, Clone, Copy)]
pub struct EnvironmentFields {
    pub fog_color: [f32; 4],
    pub light_mod: [f32; 4],
    pub sky_mod: [f32; 4],
    pub ground_mod: [f32; 4],
    /// Depth, height fog density.
    pub fog_floats: [f32; 2],
    /// Depth start, depth end, height start, height end.
    pub fog_ints: [i32; 4],
}

impl EnvironmentFields {
    pub fn from_dwords(v: &[u32]) -> Option<Self> {
        if v.len() < 22 {
            return None;
        }
        let f = |i: usize| f32::from_bits(v[i]);
        let c = |i: usize| [f(i), f(i + 1), f(i + 2), f(i + 3)];
        Some(Self {
            fog_color: c(0),
            light_mod: c(4),
            sky_mod: c(8),
            ground_mod: c(12),
            fog_floats: [f(16), f(17)],
            fog_ints: [v[18] as i32, v[19] as i32, v[20] as i32, v[21] as i32],
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct SectorDataSet {
    pub version: u32,
    pub layer_count: i32,
    /// Decoded layers 0..=3 (fog, name, ambient, boss): (layer version, entries).
    pub layers: Vec<(u32, Vec<SectorEntry>)>,
    /// Layers 4..=13 (day/night onwards), decoded once at load.
    tail: TailLayers,
    /// Why the tail failed to decode (the rest of the map stays usable).
    tail_error: Option<String>,
}

fn sector_base(r: &mut Reader) -> Result<SectorData> {
    let skipped = r.u32()?;
    let name = r.string()?;
    let mut ints = [0i32; 4];
    for x in &mut ints {
        *x = r.i32()?;
    }
    let mut values = [0u32; 8];
    for x in &mut values {
        *x = r.u32()?;
    }
    Ok(SectorData {
        skipped,
        name,
        ints,
        values,
    })
}

impl SectorDataSet {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let version = r.u32()?;
        if !(1..=2).contains(&version) {
            bail!("sector data version {version}");
        }
        let layer_count = r.i32()?;
        let mut layers = vec![];
        for li in 0..layer_count.min(4) {
            let lv = r.u32()?;
            let n = r.i32()?.max(0);
            let mut v = vec![];
            for _ in 0..n {
                let base = sector_base(r)?;
                v.push(match li {
                    0 => {
                        let mut f = vec![];
                        for _ in 0..22 {
                            f.push(r.u32()?);
                        }
                        SectorEntry::Fog(base, f)
                    }
                    1 => {
                        let tag = r.string()?;
                        let flag_a = r.u32()?;
                        let flag_b = if lv > 1 { Some(r.u32()?) } else { None };
                        SectorEntry::Name {
                            base,
                            tag,
                            flag_a,
                            flag_b,
                        }
                    }
                    2 => {
                        let mut sounds = vec![r.string()?, r.string()?];
                        if lv > 1 {
                            sounds.push(r.string()?);
                        }
                        SectorEntry::Ambient { base, sounds }
                    }
                    _ => SectorEntry::Boss(base),
                });
            }
            layers.push((lv, v));
        }
        let mut tail_r = Reader::new(r.bytes(r.remaining())?);
        let (tail, tail_error) = match read_tail(&mut tail_r, layer_count) {
            Ok(t) => (t, None),
            Err(e) => (TailLayers::default(), Some(format!("{e:#}"))),
        };
        Ok(Self {
            version,
            layer_count,
            layers,
            tail,
            tail_error,
        })
    }

    pub fn names(&self) -> impl Iterator<Item = (&str, &str)> {
        self.layers
            .iter()
            .flat_map(|(_, v)| v.iter())
            .filter_map(|e| match e {
                SectorEntry::Name { base, tag, .. } => Some((base.name.as_str(), tag.as_str())),
                _ => None,
            })
    }
}

/// Per-level sector id grid: for each of `layers.len()` sector-data layers a
/// byte per cell selecting an entry (palette) of that layer.
#[derive(Debug, Clone)]
pub struct SectorLayers {
    /// Per layer: palette entries (u8, 4 x i32), i32s stored reversed in memory.
    pub palettes: Vec<Vec<(u8, [i32; 4])>>,
    pub width: u32,
    pub height: u32,
    /// File order: for x in 0..width { for z in 0..height { for layer } }.
    pub cells: Vec<u8>,
}

impl SectorLayers {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let v = r.i32()?;
        if v != 1 {
            bail!("SectorLayers version {v}");
        }
        let nl = r.i32()?.max(0) as usize;
        let mut palettes = Vec::with_capacity(r.capacity(nl, 4));
        for _ in 0..nl {
            let n = r.i32()?.max(0);
            let mut p = vec![];
            for _ in 0..n {
                p.push((r.u8()?, [r.i32()?, r.i32()?, r.i32()?, r.i32()?]));
            }
            palettes.push(p);
        }
        let width = r.u32()?;
        let height = r.u32()?;
        let ncells = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(nl))
            .ok_or_else(|| anyhow::anyhow!("sector cells {width}x{height}x{nl} overflow"))?;
        let cells = r.bytes(ncells)?.to_vec();
        Ok(Self {
            palettes,
            width,
            height,
            cells,
        })
    }

    /// Value of `layer` at cell (x, z); out-of-range coordinates clamp to the edge, an
    /// out-of-range layer reads 0.
    pub fn get(&self, layer: usize, x: u32, z: u32) -> u8 {
        let nl = self.palettes.len();
        if layer >= nl || self.width == 0 || self.height == 0 {
            return 0;
        }
        let (x, z) = (x.min(self.width - 1), z.min(self.height - 1));
        self.cells[(x as usize * self.height as usize + z as usize) * nl + layer]
    }
}

/// One member of an [`InstanceGroup`] (`InstanceGroup::GetEntityData`).
#[derive(Debug, Clone)]
pub struct InstanceGroupEntry {
    pub uid: [u32; 4],
    /// v>=3 only: the member's record.
    pub name: String,
    /// The member's region id (`RegionInfo::id`).
    pub ints: [i32; 4],
    /// The member's position in its region.
    pub floats: [f32; 3],
    /// World position: its region's origin plus `floats` (`MapFile::open` fills it in; `None` when the
    /// region is not in the map).
    pub world: Option<[f32; 3]>,
}

#[derive(Debug, Clone)]
pub struct InstanceGroup {
    pub version: u32,
    pub name: String,
    pub kind: String,
    pub entries: Vec<InstanceGroupEntry>,
    pub group_id: Option<[u32; 4]>,
    pub links: Vec<[u32; 4]>,
}

impl InstanceGroup {
    pub fn contains(&self, uid: &[u32; 4]) -> bool {
        self.entries.iter().any(|e| e.uid == *uid)
    }
}

/// The map's instance groups (`InstanceGroupManager`), in map order, indexed by member uid. Derefs to the
/// group list.
#[derive(Debug, Clone, Default)]
pub struct InstanceGroups {
    groups: Vec<InstanceGroup>,
    by_uid: crate::collections::HashMap<[u32; 4], Vec<u32>>,
    by_group_id: crate::collections::HashMap<[u32; 4], u32>,
}

impl InstanceGroups {
    pub fn new(groups: Vec<InstanceGroup>) -> Self {
        let mut by_uid: crate::collections::HashMap<[u32; 4], Vec<u32>> = Default::default();
        let mut by_group_id: crate::collections::HashMap<[u32; 4], u32> = Default::default();
        for (i, g) in groups.iter().enumerate() {
            if let Some(id) = g.group_id {
                by_group_id.entry(id).or_insert(i as u32);
            }
            for e in &g.entries {
                let list = by_uid.entry(e.uid).or_default();
                if list.last() != Some(&(i as u32)) {
                    list.push(i as u32);
                }
            }
        }
        Self {
            groups,
            by_uid,
            by_group_id,
        }
    }

    /// The groups holding `uid`, in map order (Engine.dll
    /// `InstanceGroupManager::GetGroupsThatContain`: a scan of the group list).
    pub fn containing(&self, uid: &[u32; 4]) -> impl Iterator<Item = &InstanceGroup> {
        self.by_uid
            .get(uid)
            .into_iter()
            .flatten()
            .map(|&i| &self.groups[i as usize])
    }

    /// The first group whose `group_id` is `id` (a link target).
    pub fn by_id(&self, id: &[u32; 4]) -> Option<&InstanceGroup> {
        self.by_group_id.get(id).map(|&i| &self.groups[i as usize])
    }

    /// `uid`'s member entry: its record, region and position (Engine.dll
    /// `InstanceGroupManager::GetEntityData`: the first group of `GetGroupsThatContain`, then
    /// `InstanceGroup::GetEntityData`, that group's first entry with the uid).
    pub fn entity_data(&self, uid: &[u32; 4]) -> Option<&InstanceGroupEntry> {
        self.containing(uid)
            .next()?
            .entries
            .iter()
            .find(|e| e.uid == *uid)
    }
}

impl std::ops::Deref for InstanceGroups {
    type Target = [InstanceGroup];
    fn deref(&self) -> &[InstanceGroup] {
        &self.groups
    }
}

impl<'a> IntoIterator for &'a InstanceGroups {
    type Item = &'a InstanceGroup;
    type IntoIter = std::slice::Iter<'a, InstanceGroup>;
    fn into_iter(self) -> Self::IntoIter {
        self.groups.iter()
    }
}

pub fn read_instance_groups(r: &mut Reader) -> Result<Vec<InstanceGroup>> {
    r.u32()?; // skipped
    let n = r.i32()?.max(0);
    let mut out = vec![];
    let uid = |r: &mut Reader| -> Result<[u32; 4]> { Ok([r.u32()?, r.u32()?, r.u32()?, r.u32()?]) };
    for _ in 0..n {
        let version = r.u32()?;
        let mut g = InstanceGroup {
            version,
            name: String::new(),
            kind: String::new(),
            entries: vec![],
            group_id: None,
            links: vec![],
        };
        if version >= 1 {
            g.name = r.string()?;
            g.kind = r.string()?;
            let m = r.i32()?.max(0);
            for _ in 0..m {
                let u = uid(r)?;
                let name = if version >= 3 {
                    r.string()?
                } else {
                    String::new()
                };
                let ints = [r.i32()?, r.i32()?, r.i32()?, r.i32()?];
                let floats = [r.f32()?, r.f32()?, r.f32()?];
                g.entries.push(InstanceGroupEntry {
                    uid: u,
                    name,
                    ints,
                    floats,
                    world: None,
                });
            }
        }
        if version >= 2 {
            g.group_id = Some(uid(r)?);
            let m = r.i32()?.max(0);
            for _ in 0..m {
                g.links.push(uid(r)?);
            }
        }
        out.push(g);
    }
    Ok(out)
}

/// One `LevelLimitSectorData` entry: per difficulty (normal, elite,
/// ultimate) the `(min, max, offset)` of the proxies' player level
/// (`GameEngine::GetLevelLimits`; 0 = no bound).
#[derive(Debug, Clone)]
pub struct LevelLimit {
    pub base: SectorData,
    pub limits: [(u32, u32, i32); 3],
}

/// One `BloomSectorData` entry (`ReadBloomData`: 3 floats at +88, then 20
/// bytes the engine skips). Blended by `AmbianceManager` into the slot's
/// bloom parameters (`GetBloomParameters`): x clamped 0..1, y 0..4, z 0..1,
/// each plus the weather increment (`IncrementBloomParams`).
#[derive(Debug, Clone)]
pub struct Bloom {
    pub base: SectorData,
    pub params: [f32; 3],
}

/// One `AdjustmentSectorData` entry (`ReadAdjustmentData`, 6 floats at
/// +88..+108; constructor defaults 0, 0, 0, 1, 1, 1): the final-image
/// enhancement (`GraphicsCanvas::ApplyEnhancement`, postprocessing/enhance.ssh).
#[derive(Debug, Clone)]
pub struct Adjustment {
    pub base: SectorData,
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub tint: [f32; 3],
}

/// Layers 5 (bloom) ..= 13 (challenge) of the sector data.
#[derive(Debug, Clone, Default)]
pub struct LaterLayers {
    pub bloom: Vec<Bloom>,
    pub adjustment: Vec<Adjustment>,
    pub level_limits: Vec<LevelLimit>,
    /// Layer 6 (climate): the sector's climate-set record path.
    pub climate: Vec<Climate>,
    /// Layer 7 (damage): the sector's periodic damage.
    pub damage: Vec<DamageSector>,
    /// Layer 13 (challenge): the sector's `ChallengeArea` record path.
    pub challenge: Vec<Challenge>,
}

/// One `DamageSectorData` entry (`SectorDataManager::ReadDamageData`, Engine.dll: the common
/// header, then a float at +88, an int at +92 and a u32-length record path at +96): the share of max life a
/// player standing in the sector loses each tick, its combat type (`CombatType` id) and the effect spawned on it.
#[derive(Debug, Clone)]
pub struct DamageSector {
    pub base: SectorData,
    pub damage: f32,
    pub kind: i32,
    pub record: String,
}

/// One challenge-layer entry (`SectorDataManager::ReadChallengeData`: the common header,
/// then a u32-length record path at +88, normalised): the `ChallengeArea` record (`records/game/challengeareas/`)
/// of that sector, read by `Player::UpdateSelf` (`GameEngine::GetChallengeArea`).
#[derive(Debug, Clone)]
pub struct Challenge {
    pub base: SectorData,
    pub record: String,
}

/// One `ClimateSectorData` entry (`ReadClimateData`): the record path of the climate set
/// (`records/fx/climate/<set>.dbr`) the weather manager draws from in that sector.
#[derive(Debug, Clone)]
pub struct Climate {
    pub base: SectorData,
    pub record: String,
}

/// Reads the layers after day/night (5 bloom ..= 13 challenge) from `r`,
/// positioned just after the day/night layer.
/// `layer_count` is [`SectorDataSet::layer_count`].
pub fn read_later_layers(r: &mut Reader, layer_count: i32) -> Result<LaterLayers> {
    let mut out = LaterLayers::default();
    for li in 5..layer_count.min(14) {
        let lv = r.u32()?;
        let ok = match li {
            6 => lv == 2,
            9 => lv <= 2,
            // `ReadRiftgateData` / `ReadViewDistanceData` / `ReadPvpData` / `ReadChallengeData` skip the version.
            10..=13 => true,
            _ => lv == 1,
        };
        if !ok {
            bail!("sector layer {li} version {lv}");
        }
        let n = r.u32()?;
        for _ in 0..n {
            let base = sector_base(r)?;
            match li {
                // Riftgate and PvP (10, 12): the header only.
                10 | 12 => {}
                // ReadViewDistanceData: an int at +88.
                11 => {
                    r.skip(4)?;
                }
                // ReadChallengeData: a record path.
                13 => {
                    let record = r.string()?.replace('\\', "/").to_lowercase();
                    out.challenge.push(Challenge { base, record });
                }
                // SectorDataManager::ReadBloomData: 3 floats + 20 skipped bytes.
                5 => {
                    let params = [r.f32()?, r.f32()?, r.f32()?];
                    r.skip(20)?;
                    out.bloom.push(Bloom { base, params });
                }
                // ReadClimateData: a record path.
                6 => {
                    let record = r.string()?;
                    out.climate.push(Climate { base, record });
                }
                // ReadDamageData: float, int, record path.
                7 => {
                    let damage = r.f32()?;
                    let kind = r.i32()?;
                    let record = r.string()?.replace('\\', "/").to_lowercase();
                    out.damage.push(DamageSector {
                        base,
                        damage,
                        kind,
                        record,
                    });
                }
                // ReadAdjustmentData: 6 floats.
                8 => {
                    let v = [r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?];
                    out.adjustment.push(Adjustment {
                        base,
                        brightness: v[0],
                        contrast: v[1],
                        saturation: v[2],
                        tint: [v[3], v[4], v[5]],
                    });
                }
                // ReadLevelLimitData: min/max per difficulty, v2: then the offsets.
                _ => {
                    let mut limits = [(1, 200, 0); 3];
                    for l in &mut limits {
                        l.0 = r.u32()?;
                        l.1 = r.u32()?;
                    }
                    if lv > 1 {
                        for l in &mut limits {
                            l.2 = r.i32()?;
                        }
                    }
                    out.level_limits.push(LevelLimit { base, limits });
                }
            }
        }
    }
    Ok(out)
}

/// One `DayNightCycleSectorData` entry's cycle, as stored (`DayNightCycle::Read`
/// Engine.dll): "DNC" + version (2 or 3), sunset, sunrise,
/// v3: night start / end, sun azimuth, sun min angle, moon azimuth, moon min
/// angle, then the keys (time + light, sky, ground rgb). Zero azimuths and the
/// v2 night times keep the engine defaults (gd-render `render::env`).
#[derive(Debug, Clone)]
pub struct DayNightData {
    pub version: u8,
    pub sunset: f32,
    pub sunrise: f32,
    /// v3 only: (night start, night end).
    pub night: Option<(f32, f32)>,
    pub sun_azimuth: f32,
    pub sun_min_angle: f32,
    pub moon_azimuth: f32,
    pub moon_min_angle: f32,
    /// (time, [light rgb, sky rgb, ground rgb]).
    pub keys: Vec<(f32, [f32; 9])>,
}

impl DayNightData {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let magic = r.bytes(3)?;
        let version = r.u8()?;
        if magic != b"DNC" || version < 2 {
            bail!("bad DayNightCycle header");
        }
        let sunset = r.f32()?;
        let sunrise = r.f32()?;
        let night = if version == 3 {
            Some((r.f32()?, r.f32()?))
        } else {
            None
        };
        let (sun_azimuth, sun_min_angle, moon_azimuth, moon_min_angle) =
            (r.f32()?, r.f32()?, r.f32()?, r.f32()?);
        let n = r.u32()?;
        let mut keys = Vec::with_capacity(r.capacity(n as usize, 40));
        for _ in 0..n {
            let t = r.f32()?;
            let mut v = [0.0f32; 9];
            for x in &mut v {
                *x = r.f32()?;
            }
            keys.push((t, v));
        }
        Ok(Self {
            version,
            sunset,
            sunrise,
            night,
            sun_azimuth,
            sun_min_angle,
            moon_azimuth,
            moon_min_angle,
            keys,
        })
    }
}

/// Layers 4 (day/night) ..= 13 of the sector data.
#[derive(Debug, Clone, Default)]
pub struct TailLayers {
    pub daynight: Vec<(SectorData, DayNightData)>,
    pub later: LaterLayers,
}

fn read_tail(r: &mut Reader, layer_count: i32) -> Result<TailLayers> {
    let lv = r.u32()?;
    if lv != 1 {
        bail!("daynight layer version {lv}");
    }
    let n = r.i32()?.max(0);
    let mut daynight = Vec::with_capacity(r.capacity(n as usize, 88));
    for _ in 0..n {
        let base = sector_base(r)?;
        daynight.push((base, DayNightData::read(r)?));
    }
    let later = read_later_layers(r, layer_count)?;
    Ok(TailLayers { daynight, later })
}

impl SectorDataSet {
    /// The day/night layer and layers 5..=13 (decoded at load; this clones them), or why they failed to decode.
    pub fn tail_layers(&self) -> Result<TailLayers> {
        match &self.tail_error {
            Some(e) => Err(anyhow::anyhow!("{e}")),
            None => Ok(self.tail.clone()),
        }
    }
}
