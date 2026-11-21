//! The loaded levels as gameplay sees them: which are loaded (and their root
//! entity), their sector grids (what area a point is in, [`Levels::sector_entry`]
//! the map's entry there) and the map's area level limits, challenge areas and damage sectors.
//! The game's world streamer fills it as levels load and unload; tests can mark
//! levels loaded by hand.
//!
//! The map's regions in the world: [`region_origin`] (a level's positions are
//! relative to it), [`region_distance`] (the streaming distance), [`region_at`]
//! and [`region_zone`] (which region / riftgate map zone holds a point).
//!
//! Streaming never despawns in `Update`: an unloaded level's root, the
//! monsters / NPCs / props of unloaded levels ([`crate::monsters::cull_monsters`]),
//! the chests, breakables, doors and physics props of unloaded levels ([`LevelMember`],
//! [`cull_level_members`]) and story actors whose objects left the world get [`Unload`], and
//! [`despawn_unloaded`] despawns them in `Last`, so inserts other systems
//! queued for them that frame still apply.
//!
//! Every world entity belongs to a level: Engine.dll `Region::UnloadLevel` @0x1801946f0 →
//! `Level::UnloadEntities` takes every entity of the region out of the world, and `Level::SaveState` @0x180131460
//! keeps those whose `ShouldSaveState` holds (Game.dll `Item::ShouldSaveState` @0x180326bf0: every item but a stale
//! Shattered Realm one) for the region's next load. A level's own content hangs under its root; the loose entities
//! the game drops into the world — ground items, effects, the containers a death drops, the Shattered Realm's exit
//! gates — take the loaded level they appear in ([`place_loose`], [`crate::monsters::LevelProp`]) and go with it
//! (ground items come back with it, `crate::loot::LevelItems`); script-created story objects leave the story when
//! their region streams out (`crate::story_world`). PORT: the level is the loaded region nearest the entity's position
//! when it appears ([`Levels::level_at`]; the engine keeps each entity's region pointer).
use bevy::prelude::*;
use gd_formats::map::sector::{Challenge, DamageSector, LevelLimit, SectorEntry, SectorLayers};
use gd_formats::map::{MapFile, RegionInfo};
use std::collections::BTreeMap;

/// Levels whose XZ bounds are within this distance ([`region_distance`]) of the streaming focus are loaded (the
/// world streamer's levels, `crate::story_world`'s story objects).
pub const LOAD_RADIUS: f32 = 160.0;
/// Loaded levels farther than this are unloaded; the gap to [`LOAD_RADIUS`] keeps a level at the edge from
/// reloading back and forth.
pub const UNLOAD_RADIUS: f32 = 220.0;

/// On an entity streaming took out of the world; [`despawn_unloaded`]
/// despawns it (with its children) in `Last` (module doc).
#[derive(Component)]
pub struct Unload;

/// `Last`: despawns the [`Unload`]ed entities, after every command of the
/// frame's `Update` / `PostUpdate` systems has been applied.
pub fn despawn_unloaded(mut commands: Commands, q: Query<Entity, With<Unload>>) {
    for e in &q {
        commands.entity(e).try_despawn();
    }
}

/// A level's placed chest, breakable, door or physics prop (not a story object's): it goes with level `.0`
/// ([`cull_level_members`]).
#[derive(Component, Clone, Copy, Debug)]
pub struct LevelMember(pub usize);

/// Marks the [`LevelMember`]s of unloaded levels [`Unload`]: all of them when the loaded levels changed, else only
/// the members added since the last run (spawned for a level already gone).
pub fn cull_level_members(
    mut commands: Commands,
    world: Res<Levels>,
    all: Query<(Entity, &LevelMember), Without<Unload>>,
    added: Query<(Entity, &LevelMember), (Added<LevelMember>, Without<Unload>)>,
) {
    let mut cull = |(e, m): (Entity, &LevelMember)| {
        if !world.loaded.contains_key(&m.0) {
            commands.entity(e).try_insert(Unload);
        }
    };
    if world.is_changed() {
        all.iter().for_each(&mut cull);
    } else {
        added.iter().for_each(&mut cull);
    }
}

/// `Last`: the loose world entities that appeared this frame take the level they stand in (module doc): top-level
/// ground items, effects, containers and exit gates that are no level's yet and no story actor (the story streams
/// those, `crate::story_world`). `Last`, so the parenting queued with a spawn has been applied: an effect that follows
/// its owner as a child is the owner's.
#[allow(clippy::type_complexity)]
pub fn place_loose(
    mut commands: Commands,
    levels: Res<Levels>,
    layout: Option<Res<crate::save::MapLayout>>,
    new: Query<
        (Entity, &Transform),
        (
            Without<ChildOf>,
            Without<crate::monsters::LevelProp>,
            Without<crate::story::StoryActor>,
            Or<(
                Added<crate::loot::GroundItem>,
                Added<crate::visuals::EffectSpec>,
                Added<crate::world::Breakable>,
                Added<crate::chests::Chest>,
                Added<crate::endless::EndlessGate>,
            )>,
        ),
    >,
) {
    let Some(layout) = layout else { return };
    for (e, tf) in &new {
        if let Some(level) = levels.level_at(&layout.0, tf.translation) {
            commands
                .entity(e)
                .try_insert(crate::monsters::LevelProp(level));
        }
    }
}

/// Where the camera / player is: levels and their story objects stream in around it (the game's world streamer,
/// [`crate::story_world::stream_story_objects`]).
#[derive(Resource, Default, PartialEq)]
pub struct StreamFocus(pub Vec3);

#[derive(Resource, Default)]
pub struct Levels {
    /// Loaded level (region index) → its root entity. Ordered, so overlap
    /// lookups resolve deterministically (lowest region first).
    pub loaded: BTreeMap<usize, Entity>,
    /// Sector-data index grids of loaded levels: region → (origin, extent, grid).
    sectors: BTreeMap<usize, (Vec3, Vec3, SectorLayers)>,
    /// Sector layer 9 (`LevelLimitSectorData`) of the map.
    pub level_limits: Vec<LevelLimit>,
    /// Sector layer 13 (`ChallengeSectorData`) of the map ([`crate::challenge_areas`]).
    pub challenges: Vec<Challenge>,
    /// Sector layer 7 (`DamageSectorData`) of the map ([`crate::damage_sectors`]).
    pub damages: Vec<DamageSector>,
}

impl Levels {
    pub fn new(level_limits: Vec<LevelLimit>) -> Self {
        Self {
            level_limits,
            ..Default::default()
        }
    }

    /// The map's challenge-area table (sector layer 13).
    pub fn with_challenges(mut self, challenges: Vec<Challenge>) -> Self {
        self.challenges = challenges;
        self
    }

    /// The map's damage-sector table (sector layer 7).
    pub fn with_damages(mut self, damages: Vec<DamageSector>) -> Self {
        self.damages = damages;
        self
    }

    /// A level finished loading at `origin` with its sector grid.
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

    /// A level unloaded; returns its root entity.
    pub fn remove(&mut self, level: usize) -> Option<Entity> {
        self.sectors.remove(&level);
        self.loaded.remove(&level)
    }

    pub fn is_loaded(&self, level: usize) -> bool {
        self.loaded.contains_key(&level)
    }

    /// The loaded level a world position belongs to (module doc): the one whose region footprint is nearest (0
    /// inside; where regions overlap, as the Shattered Realm's over main-world ground, the lowest region index).
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

    /// [`Self::sector_guid`] in the grid of one level (region index) instead of
    /// whichever loaded level covers `pos`: `Player::UpdateSelf`
    /// (0x1803D24A0) reads an entity's sector from its own region's level
    /// (`Entity::GetRegion` -> `Region::GetLevelPtr` -> `Level::GetSectorLayers`
    /// -> `SectorLayers::GetTargetId` at the region-local position). The Shattered
    /// Realm's dungeon regions are placed over main-world ground, so the
    /// position alone can hit the wrong level.
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

    /// [`Self::sector_entry`] in the level of one region ([`Self::sector_guid_in`]).
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
        let id = grid.get(layer, fx.min(grid.width - 1), fz.min(grid.height - 1));
        if id == 0 {
            return None;
        }
        Some(grid.palettes.get(layer)?.iter().find(|(i, _)| *i == id)?.1)
    }

    /// The sector-data entries of `layer` around `pos`, weighted by area,
    /// `AmbianceManager` sub_180032E70 (Engine.dll): for every
    /// level whose grid overlaps the square ±`radius` (xz), the cells
    /// `int(local - radius) ..= int(local + radius + 0.5)` (clamped to the
    /// grid, 1 m cells) are counted by palette id; each id with an entry
    /// weighs its count / the count of all cells (id 0 and ids without an
    /// entry included), so the weights add up to at most 1 and the caller
    /// fills the rest with its default. Same GUIDs are merged.
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
    ///
    /// `region`: the level (region index) the spawning proxy / container
    /// belongs to; its own grid is read (`Entity::GetRegion` ->
    /// `Level::GetSectorLayers`, see [`Self::sector_guid_in`]), so a dungeon
    /// level over main-world ground gets its own limits. `None` (or the
    /// story's `usize::MAX`): PORT: whichever loaded level covers `pos`
    /// (script-placed proxies and shrines carry no region here; the engine's
    /// source of their region is not traced).
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

/// The entry of the map's sector-data table `layer` with GUID `guid`.
fn map_entry(map: &MapFile, layer: usize, guid: [i32; 4]) -> Option<&SectorEntry> {
    let (_, entries) = map.sector_data.layers.get(layer)?;
    entries.iter().find(|e| e.base().ints == guid)
}

/// World position of region `r`'s origin.
pub fn region_origin(r: &RegionInfo) -> Vec3 {
    Vec3::new(
        r.position[0] as f32,
        r.position[1] as f32,
        r.position[2] as f32,
    )
}

/// How far `p` is outside the region's box on the ground plane (0 inside): the streaming distance of world and
/// story objects.
pub fn region_distance(r: &RegionInfo, p: Vec3) -> f32 {
    let c = region_origin(r) + Vec3::from(r.bbox.center);
    let h = Vec3::from(r.bbox.half);
    let dx = ((p.x - c.x).abs() - h.x).max(0.0);
    let dz = ((p.z - c.z).abs() - h.z).max(0.0);
    (dx * dx + dz * dz).sqrt()
}

/// The region a world position is in: PORT (the engine keeps each object's region pointer;
/// the port has world positions): the first region whose box holds it, else the first whose
/// ground footprint does.
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
/// `pos` (PORT: by the regions' bounds grown by 1 m; the smallest one when they overlap).
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
