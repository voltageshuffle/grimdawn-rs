//! The loaded levels as gameplay sees them: which are loaded (and their root
//! entity), their sector grids and the map's area level limits, challenge areas
//! and damage sectors. The world streamer fills it; tests can mark levels loaded by hand.
//!
//! Streaming never despawns in `Update`: entities of unloaded levels get [`Unload`] and
//! `gd_game::levels::despawn_unloaded` despawns them in `Last`, so inserts other systems
//! queued for them that frame still apply.
//!
//! Every world entity belongs to a level (Engine.dll `Region::UnloadLevel` →
//! `Level::UnloadEntities`; `Level::SaveState` keeps those whose `ShouldSaveState` holds).
//! Loose entities (ground items, effects, death containers, Shattered Realm exit gates)
//! take the loaded level they appear in (`gd_game::levels::place_loose`) and go with it.
//! PORT: the level is the loaded region nearest the entity's position when it appears
//! ([`Levels::level_at`]; the engine keeps each entity's region pointer).
use bevy::prelude::*;
use gd_formats::map::sector::{Challenge, DamageSector, LevelLimit, SectorEntry, SectorLayers};
use gd_formats::map::{MapFile, RegionInfo};
use std::collections::BTreeMap;

/// Levels within this XZ distance ([`region_distance`]) of the streaming focus are loaded.
pub const LOAD_RADIUS: f32 = 160.0;
/// Loaded levels farther than this are unloaded; the gap to [`LOAD_RADIUS`] is hysteresis.
pub const UNLOAD_RADIUS: f32 = 220.0;

#[derive(Component)]
pub struct Unload;

/// A level's placed chest, breakable, door or physics prop (not a story object's): it goes with level `.0`.
#[derive(Component, Clone, Copy, Debug)]
pub struct LevelMember(pub usize);

/// Where the camera / player is: levels and their story objects stream in around it.
#[derive(Resource, Default, PartialEq)]
pub struct StreamFocus(pub Vec3);

#[derive(Resource, Default)]
pub struct Levels {
    /// Loaded level (region index) → root. Ordered so overlap lookups pick the lowest region.
    pub loaded: BTreeMap<usize, Entity>,
    /// Sector-data index grids of loaded levels: region → (origin, extent, grid).
    sectors: BTreeMap<usize, (Vec3, Vec3, SectorLayers)>,
    pub level_limits: Vec<LevelLimit>,
    /// Sector layer 13 (`ChallengeSectorData`).
    pub challenges: Vec<Challenge>,
    /// Sector layer 7 (`DamageSectorData`).
    pub damages: Vec<DamageSector>,
}

impl Levels {
    pub fn new(level_limits: Vec<LevelLimit>) -> Self {
        Self {
            level_limits,
            ..Default::default()
        }
    }

    pub fn with_challenges(mut self, challenges: Vec<Challenge>) -> Self {
        self.challenges = challenges;
        self
    }

    pub fn with_damages(mut self, damages: Vec<DamageSector>) -> Self {
        self.damages = damages;
        self
    }

    pub fn add(
        &mut self,
        level: usize,
        root: Entity,
        origin: Vec3,
        sectors: Option<(Vec3, SectorLayers)>,
    ) {
        if let Some((extent, grid)) = sectors {
            self.sectors.insert(level, (origin, extent, grid));
        }
        self.loaded.insert(level, root);
    }

    pub fn remove(&mut self, level: usize) -> Option<Entity> {
        self.sectors.remove(&level);
        self.loaded.remove(&level)
    }

    pub fn is_loaded(&self, level: usize) -> bool {
        self.loaded.contains_key(&level)
    }

    /// The loaded level a world position belongs to: the one whose region footprint is nearest
    /// (0 inside; on overlap the lowest region index).
    pub fn level_at(&self, map: &MapFile, p: Vec3) -> Option<usize> {
        self.loaded
            .keys()
            .filter_map(|&i| Some((i, region_distance(map.regions.get(i)?, p))))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// GUID of the sector-data entry of `layer` (any of the 14, e.g. 4 =
    /// day/night) at a world position; `None` where the cell's id is 0.
    pub fn sector_guid(&self, pos: Vec3, layer: usize) -> Option<[i32; 4]> {
        let found = self.sectors.values().find(|(o, e, _)| {
            pos.x >= o.x && pos.z >= o.z && pos.x < o.x + e.x && pos.z < o.z + e.z
        })?;
        Self::cell_guid(found, pos, layer)
    }

    /// [`Self::sector_guid`] in the grid of one level (region index): `Player::UpdateSelf`
    /// reads an entity's sector from its own region's level; Shattered Realm regions overlap
    /// main-world ground, so the position alone can hit the wrong level.
    pub fn sector_guid_in(&self, level: usize, pos: Vec3, layer: usize) -> Option<[i32; 4]> {
        Self::cell_guid(self.sectors.get(&level)?, pos, layer)
    }

    /// The map's sector-data entry of `layer` (0 fog, 1 name, 2 ambient, 3 boss) at a world position:
    /// [`Self::sector_guid`] → the map table entry with that GUID.
    pub fn sector_entry<'m>(
        &self,
        map: &'m MapFile,
        pos: Vec3,
        layer: usize,
    ) -> Option<&'m SectorEntry> {
        map_entry(map, layer, self.sector_guid(pos, layer)?)
    }

    pub fn sector_entry_in<'m>(
        &self,
        map: &'m MapFile,
        region: usize,
        pos: Vec3,
        layer: usize,
    ) -> Option<&'m SectorEntry> {
        map_entry(map, layer, self.sector_guid_in(region, pos, layer)?)
    }

    fn cell_guid(
        (origin, extent, grid): &(Vec3, Vec3, SectorLayers),
        pos: Vec3,
        layer: usize,
    ) -> Option<[i32; 4]> {
        let fx = ((pos.x - origin.x) / extent.x * grid.width as f32) as u32;
        let fz = ((pos.z - origin.z) / extent.z * grid.height as f32) as u32;
        // `SectorLayers::get` clamps to the grid (and reads 0 from an empty one).
        let id = grid.get(layer, fx, fz);
        if id == 0 {
            return None;
        }
        Some(grid.palettes.get(layer)?.iter().find(|(i, _)| *i == id)?.1)
    }

    /// The sector-data entries of `layer` around `pos`, weighted by area
    /// (`AmbianceManager_SectorWeights`, Engine.dll): cells `int(local - radius) ..=
    /// int(local + radius + 0.5)` of every overlapping grid, each id's count over all cells
    /// (id 0 included), so weights sum to at most 1. Same GUIDs are merged.
    pub fn sector_weights(&self, pos: Vec3, layer: usize, radius: f32) -> Vec<([i32; 4], f32)> {
        let mut total = 0u32;
        let mut counts: Vec<([i32; 4], u32)> = Vec::new();
        for (origin, extent, grid) in self.sectors.values() {
            if pos.x + radius < origin.x
                || pos.z + radius < origin.z
                || pos.x - radius > origin.x + extent.x
                || pos.z - radius > origin.z + extent.z
            {
                continue;
            }
            let Some(palette) = grid.palettes.get(layer) else {
                continue;
            };
            let (w, h) = (grid.width as i32, grid.height as i32);
            if w == 0 || h == 0 {
                continue;
            }
            let (lx, lz) = (pos.x - origin.x, pos.z - origin.z);
            let x0 = ((lx - radius) as i32).clamp(0, w - 1);
            let x1 = ((lx + radius + 0.5) as i32).clamp(0, w - 1);
            let z0 = ((lz - radius) as i32).clamp(0, h - 1);
            let z1 = ((lz + radius + 0.5) as i32).clamp(0, h - 1);
            let mut hist = [0u32; 256];
            for z in z0..=z1 {
                for x in x0..=x1 {
                    hist[grid.get(layer, x as u32, z as u32) as usize] += 1;
                }
            }
            for (id, &n) in hist.iter().enumerate() {
                total += n;
                if id == 0 || n == 0 {
                    continue;
                }
                if let Some((_, guid)) = palette.iter().find(|(i, _)| *i as usize == id) {
                    match counts.iter_mut().find(|(g, _)| g == guid) {
                        Some(c) => c.1 += n,
                        None => counts.push((*guid, n)),
                    }
                }
            }
        }
        if total == 0 {
            return Vec::new();
        }
        counts
            .into_iter()
            .map(|(g, n)| (g, n as f32 / total as f32))
            .collect()
    }

    /// `GameEngine::GetLevelLimits`: the area's `(min, max, offset)` of the
    /// proxy player level for `difficulty` (0 normal, 1 elite, 2 ultimate).
    /// `region`: the level whose grid is read, so a dungeon over main-world ground gets its own
    /// limits. `None` / `usize::MAX`: PORT: whichever loaded level covers `pos` (the engine's
    /// region source for script-placed proxies is not traced).
    pub fn level_limits(
        &self,
        region: Option<usize>,
        pos: Vec3,
        difficulty: usize,
    ) -> Option<(u32, u32, i32)> {
        let guid = match region.filter(|r| *r != usize::MAX) {
            Some(r) => self.sector_guid_in(r, pos, 9)?,
            None => self.sector_guid(pos, 9)?,
        };
        let e = self.level_limits.iter().find(|e| e.base.ints == guid)?;
        Some(e.limits[difficulty.min(2)])
    }
}

fn map_entry(map: &MapFile, layer: usize, guid: [i32; 4]) -> Option<&SectorEntry> {
    let (_, entries) = map.sector_data.layers.get(layer)?;
    entries.iter().find(|e| e.base().ints == guid)
}

pub fn region_origin(r: &RegionInfo) -> Vec3 {
    Vec3::new(
        r.position[0] as f32,
        r.position[1] as f32,
        r.position[2] as f32,
    )
}

/// How far `p` is outside the region's box on the ground plane (0 inside).
pub fn region_distance(r: &RegionInfo, p: Vec3) -> f32 {
    let c = region_origin(r) + Vec3::from(r.bbox.center);
    let h = Vec3::from(r.bbox.half);
    let dx = ((p.x - c.x).abs() - h.x).max(0.0);
    let dz = ((p.z - c.z).abs() - h.z).max(0.0);
    (dx * dx + dz * dz).sqrt()
}

/// The region a world position is in. PORT: the first region whose box holds it, else the
/// first whose ground footprint does (the engine keeps each object's region pointer).
pub fn region_at(map: &MapFile, p: Vec3) -> Option<usize> {
    let inside = |r: &RegionInfo, y: bool| {
        let l = p - region_origin(r) - Vec3::from(r.bbox.center);
        let h = Vec3::from(r.bbox.half);
        l.x.abs() <= h.x && l.z.abs() <= h.z && (!y || l.y.abs() <= h.y)
    };
    (map.regions.iter().position(|r| inside(r, true)))
        .or_else(|| map.regions.iter().position(|r| inside(r, false)))
}

/// The map zone record (`Region::GetZoneRecord`) of the map region holding
/// `pos` (PORT: regions' bounds grown by 1 m; the smallest on overlap).
pub fn region_zone(map: &MapFile, pos: Vec3) -> Option<String> {
    map.regions
        .iter()
        .filter(|r| {
            let c = region_origin(r) + Vec3::from(r.bbox.center);
            let h = Vec3::from(r.bbox.half);
            (pos - c).abs().cmple(h + Vec3::splat(1.0)).all()
        })
        .min_by(|a, b| {
            Vec3::from(a.bbox.half)
                .element_product()
                .total_cmp(&Vec3::from(b.bbox.half).element_product())
        })
        .map(|r| r.location_record.to_ascii_lowercase())
        .filter(|z| !z.is_empty())
}
