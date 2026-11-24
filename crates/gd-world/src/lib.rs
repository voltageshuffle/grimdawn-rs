//! World streaming: levels of `world001.map` near the camera target are parsed
//! on a background thread (terrain mesh, splat textures, object list); their GPU
//! assets are then instantiated on the main thread a bounded slice per frame
//! ([`spawn_levels`]), so crossing into a new area does not freeze the frame. A
//! level counts as loaded only once fully spawned, so [`StreamedMap::area_ready`] (the
//! initial load and teleports alike) waits for it. Far levels are despawned (at
//! the end of the frame: `gd_game_core::levels::Unload`).
//!
//! PORT: a level's object meshes all start loading in the background when its build lands
//! (`ModelCache::model_ready`); [`spawn_levels`] spawns its objects in order as their models are built, then clips
//! its decals in order, within [`SPAWN_BUDGET`] (~3 ms) a frame, at least one object or decal and at most one new
//! level root a frame. While it spawns, the level holds every model already built ([`SpawnJob`]`::held`): the model
//! cache only keeps its last `gd_game_core::weak_cache::RECENT` models warm, so a big level's early arrivals would go
//! (and load again) before the cursor reaches them. A retextured / reshaded object and a decal wait for their
//! textures and override shader in the background (`MaterialAssets::variant_ready`, `decal::textures_ready`)
//! instead of decoding them on the spot.
//!
//! An unloaded level's models and terrain layers linger ([`Lingering`]): held for [`LINGER`] after it goes, at most
//! [`LINGER_LEVELS`] levels, so walking back over the unload radius finds them loaded. The engine releases a resource
//! 5 s after its last user (`gd_game_core::weak_cache` module doc); PORT: longer here, since a whole level goes at once.
//! Cleared at session end (`exit`) and when the mounted mod changes.
//!
//! Coordinates: Y-up, ~1 unit = 1 m, world = region origin +
//! level-local position; entity rotation rows are the object's local axes.
#![allow(clippy::type_complexity)]

pub mod decoration_anim;

use bevy::math::Affine3A;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use bevy::tasks::futures_lite::future;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on};
use gd_formats::collections::{HashMap, HashSet};
use gd_formats::map::{Level, MapFile};
use gd_game_core::data::GameData;
use gd_game_core::levels::{LOAD_RADIUS, UNLOAD_RADIUS, region_origin};
use gd_render::model::{self, ModelCache};
use gd_render::render::lights::{
    LightCpu, LightRand, light_from_record, spawn_light, update_fire_lights,
};
use gd_render::textures::Textures;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Terrain material: the game's layer compositing + deferred lighting (render/terrain.rs).
pub type TerrainMaterial = gd_render::render::terrain::GdTerrainMaterial;

pub use gd_render::decal::DecalMaterial;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(gd_render::decal::DecalPlugin)
            .init_resource::<LightRand>()
            .init_resource::<TravelPreload>()
            .init_resource::<Lingering>()
            .add_systems(Startup, open_world)
            // Before the unloaded roots despawn (`gd_game_core::levels::Unload`).
            .add_systems(
                Last,
                linger_unloaded.before(gd_game::levels::despawn_unloaded),
            )
            // Nothing streams behind the main menu: levels stream once the session is ready, and only when the
            // focus, the travel preload or the loaded set moved.
            .add_systems(
                Update,
                (
                    (
                        stream_levels.run_if(
                            resource_exists::<gd_game_core::session::SessionReady>.and_then(
                                resource_changed::<StreamFocus>
                                    .or_eager(resource_changed::<TravelPreload>)
                                    .or_eager(resource_changed::<Levels>),
                            ),
                        ),
                        finish_levels,
                        spawn_levels,
                    )
                        .chain(),
                    build_patrol_groups,
                    update_fire_lights,
                ),
            );
    }
}

#[derive(Resource)]
pub struct StreamedMap {
    pub map: Arc<MapFile>,
    pending: HashMap<usize, Task<anyhow::Result<LevelCpu>>>,
    failed: HashSet<usize>,
    /// Levels whose CPU build finished, spawning a slice per frame ([`spawn_levels`]).
    spawning: HashMap<usize, SpawnJob>,
}

pub use gd_game::nav::Nav;
pub use gd_game_core::levels::{Levels, StreamFocus};

/// A teleport's destination, loaded while the player is still at the start (the teleport activity's
/// `RegionLoader`, `gd_game::teleport::TeleportActivity`): levels stream around it as around [`StreamFocus`] and none
/// near it unloads. Set and cleared by `doors::travel_screen`. PORT: the levels within [`LOAD_RADIUS`] (the engine's: the
/// regions in the player's view frustum there).
#[derive(Resource, Default)]
pub struct TravelPreload(pub Option<Vec3>);

/// How long an unloaded level's assets stay held ([`Lingering`]; module doc).
const LINGER: Duration = Duration::from_secs(45);
/// At most this many unloaded levels linger; the oldest goes first.
const LINGER_LEVELS: usize = 8;

/// The models and terrain layers of the levels unloaded in the last [`LINGER`] (module doc), oldest first.
#[derive(Resource, Default)]
pub struct Lingering {
    /// The levels unloaded this frame, whose assets [`linger_unloaded`] takes before their roots despawn: the root
    /// (a cancelled spawn may have none yet) and the models its spawn held ([`SpawnJob`]).
    unloaded: Vec<(Option<Entity>, Vec<Arc<model::ModelAsset>>)>,
    levels: VecDeque<Linger>,
    /// Lingering levels let go so far (`heap.rs` trims after it moves).
    released: u64,
}

/// One unloaded level's held assets ([`Lingering`]).
struct Linger {
    /// Real time since startup when they go.
    until: Duration,
    _models: Vec<Arc<model::ModelAsset>>,
    _layers: Vec<gd_render::render::terrain::HeldLayers>,
}

impl Lingering {
    /// Lingering levels let go so far: their assets were freed if nothing else holds them.
    // Read only by `heap.rs`'s trim, which exists on glibc only.
    #[cfg_attr(not(all(target_os = "linux", target_env = "gnu")), expect(dead_code))]
    pub fn released(&self) -> u64 {
        self.released
    }
}

/// `Last`: takes the models and terrain layers of the level roots unloaded this frame into [`Lingering`], and lets
/// go the lingering levels past [`LINGER`] or beyond [`LINGER_LEVELS`] (all of them when the mounted mod changed).
fn linger_unloaded(
    mut lingering: ResMut<Lingering>,
    children: Query<&Children>,
    parts: Query<&model::ModelRef>,
    mut layers: Query<&mut gd_render::render::terrain::HeldLayers>,
    time: Res<Time<Real>>,
    mut mod_changed: MessageReader<model::ModChanged>,
) {
    let now = time.elapsed();
    if mod_changed.read().count() > 0 {
        let l = &mut *lingering;
        l.released += l.levels.len() as u64;
        l.levels.clear();
        l.unloaded.clear();
    }
    for (root, mut models) in std::mem::take(&mut lingering.unloaded) {
        let mut seen: HashSet<*const model::ModelAsset> = models.iter().map(Arc::as_ptr).collect();
        let mut held = Vec::new();
        for e in root
            .into_iter()
            .flat_map(|r| std::iter::once(r).chain(children.iter_descendants(r)))
        {
            if let Ok(m) = parts.get(e)
                && seen.insert(Arc::as_ptr(&m.0))
            {
                models.push(m.0.clone());
            }
            if let Ok(mut h) = layers.get_mut(e) {
                held.push(std::mem::take(&mut *h));
            }
        }
        lingering.levels.push_back(Linger {
            until: now + LINGER,
            _models: models,
            _layers: held,
        });
    }
    let l = &mut *lingering;
    while l
        .levels
        .front()
        .is_some_and(|f| f.until <= now || l.levels.len() > LINGER_LEVELS)
    {
        l.levels.pop_front();
        l.released += 1;
    }
}

impl StreamedMap {
    /// Every level that streams in around `p` is in (none still loading; failed ones do not count).
    pub fn area_ready(&self, levels: &Levels, p: Vec3) -> bool {
        self.map.regions.iter().enumerate().all(|(i, r)| {
            r.level_size == 0
                || self.failed.contains(&i)
                || gd_game_core::levels::region_distance(r, p) >= LOAD_RADIUS
                || (levels.is_loaded(i) && !self.pending.contains_key(&i))
        })
    }

    pub fn spawn_point(&self) -> Option<Vec3> {
        let sp = self.map.spawn_points.first()?;
        let r = self.map.regions.get(sp.region as usize)?;
        Some(region_origin(r) + Vec3::from(sp.position))
    }

    /// Levels are still building or spawning (`heap.rs` holds its trim until streaming settles).
    #[cfg_attr(not(all(target_os = "linux", target_env = "gnu")), expect(dead_code))]
    pub fn busy(&self) -> bool {
        !self.pending.is_empty() || !self.spawning.is_empty()
    }
}

/// The session's map, read from its archive entry (`MapFile::open_resource`).
/// `GD_MAP` selects another map resource (e.g. `levels/mainmenu/mainmenu_gdx3.map`).
fn open_world_map(
    data: &GameData,
    hooks: &gd_game_core::hooks::Hooks,
    choice: &gd_game_core::session::Choice,
) -> anyhow::Result<MapFile> {
    // A Crucible arena or a custom game's map (`Choice::map_path`, read through the mounted mod,
    // `World::Load` opens it through the `FileSystem`), else `GD_MAP`, else the campaign's world.
    let res = (choice.game_mode() != 0)
        .then(|| choice.map_path())
        .or(hooks.session.map.as_deref())
        .map(gd_formats::vfs::normalize)
        .filter(|m| data.vfs.exists(m))
        .unwrap_or_else(|| "levels/world001.map".into());
    MapFile::open_resource(&data.vfs, &res)
}

pub fn open_world(
    commands: Commands,
    data: Res<GameData>,
    hooks: Res<gd_game_core::hooks::Hooks>,
    choice: Res<gd_game_core::session::Choice>,
) {
    let map = open_world_map(&data, &hooks, &choice).expect("open world map");
    install_world(In(map), commands);
}

/// Installs an opened map as the session's world: its streaming state, level limits and map-derived resources.
fn install_world(In(map): In<MapFile>, mut commands: Commands) {
    info!(
        "world: {} regions, {} spawn points",
        map.regions.len(),
        map.spawn_points.len()
    );
    let later = map
        .sector_data
        .tail_layers()
        .map(|t| t.later)
        .unwrap_or_else(|e| {
            warn!("sector level limits / challenge areas: {e:#}");
            Default::default()
        });
    info!(
        "{} area level limits, {} challenge areas",
        later.level_limits.len(),
        later.challenge.len()
    );
    let map = Arc::new(map);
    // The save's `map.dat` needs the region ids (`gd_game_core::save::MapLayout`).
    commands.insert_resource(gd_game_core::save::MapLayout(map.clone()));
    // The unique-entity rules read the map's own group list (`gd_game::unique`).
    commands.insert_resource(gd_game::unique::InstanceGroups(map.instance_groups.clone()));
    let world = StreamedMap {
        map,
        pending: HashMap::default(),
        failed: HashSet::default(),
        spawning: HashMap::default(),
    };
    let focus = world.spawn_point().unwrap_or(Vec3::ZERO);
    commands.insert_resource(StreamFocus(focus));
    commands.insert_resource(
        Levels::new(later.level_limits)
            .with_challenges(later.challenge)
            .with_damages(later.damage),
    );
    // The patrol groups are this map's instance groups: `build_patrol_groups` reads them again from the new map
    // (the campaign world opened behind the menu, then the arena or custom map of `open_session_map`).
    commands.insert_resource(gd_game::patrol::PatrolGroups::default());
    commands.insert_resource(world);
}

#[allow(clippy::too_many_arguments)]
fn stream_levels(
    mut world: ResMut<StreamedMap>,
    mut levels: ResMut<Levels>,
    focus: Res<StreamFocus>,
    preload: Res<TravelPreload>,
    data: Res<GameData>,
    layers: Res<gd_render::render::terrain::TerrainLayers>,
    grass: Res<gd_render::render::grass::GrassQuality>,
    mut commands: Commands,
    mut nav: ResMut<Nav>,
    mut coll: ResMut<gd_game::collision::WorldCollision>,
    mut receivers: ResMut<gd_render::decal::DecalReceivers>,
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut lingering: ResMut<Lingering>,
) {
    // The test arena replaces the levels.
    if hooks.session.testworld.is_some() {
        return;
    }
    let p = focus.0;
    let StreamedMap {
        map,
        pending,
        failed,
        spawning,
    } = &mut *world;
    let dist = |i: usize| {
        let r = &map.regions[i];
        gd_game_core::levels::region_distance(r, p).min(
            preload
                .0
                .map_or(f32::MAX, |t| gd_game_core::levels::region_distance(r, t)),
        )
    };
    // Loads: only the regions in the radius look at the bookkeeping (the map has thousands, a handful are near).
    for (i, r) in map.regions.iter().enumerate() {
        if r.level_size == 0
            || dist(i) >= LOAD_RADIUS
            || levels.is_loaded(i)
            || pending.contains_key(&i)
            || failed.contains(&i)
            || spawning.contains_key(&i)
        {
            continue;
        }
        let (map, data, layers) = (map.clone(), data.clone(), layers.clone());
        let high = grass.0;
        pending.insert(
            i,
            AsyncComputeTaskPool::get()
                .spawn(async move { build_level(&map, &data, &layers, high, i) }),
        );
    }
    // Unloads: only the levels held or loading. A build still in flight is for a level nobody wants now: dropping
    // the task cancels it (else `finish_levels` spawns the whole level and the next frame unloads it again).
    pending.retain(|&i, _| dist(i) <= UNLOAD_RADIUS);
    // A spawn still in flight for a level nobody wants now: cancel it (its nav
    // is in already; its partial root despawns in `Last`).
    let cancel: Vec<usize> = spawning
        .keys()
        .copied()
        .filter(|&i| dist(i) > UNLOAD_RADIUS)
        .collect();
    for i in cancel {
        if let Some(job) = spawning.remove(&i) {
            if let Some(root) = job.root {
                commands
                    .entity(root)
                    .try_insert(gd_game_core::levels::Unload);
            }
            // What it spawned and the models it held linger ([`Lingering`]).
            lingering.unloaded.push((job.root, job.held));
            nav.0.remove(i as u64);
            // Its decal receivers may be registered already (decals clip after the objects).
            receivers.remove_level(i);
        }
    }
    // Read through `Deref` first: `remove` through `ResMut` marks `Levels` changed, and `cull_monsters` walks every
    // level entity on a change.
    let mut far: Vec<usize> = levels
        .loaded
        .keys()
        .copied()
        .filter(|&i| dist(i) > UNLOAD_RADIUS)
        .collect();
    far.sort_unstable();
    for i in far {
        if let Some(e) = levels.remove(i) {
            // Despawned in `Last` (`gd_game_core::levels::Unload`), its assets lingering ([`Lingering`]).
            commands.entity(e).try_insert(gd_game_core::levels::Unload);
            lingering.unloaded.push((Some(e), Vec::new()));
            nav.0.remove(i as u64);
            coll.remove_level(i);
            receivers.remove_level(i);
        }
    }
}

/// CPU-side result of loading one level.
pub struct LevelCpu {
    index: usize,
    origin: Vec3,
    terrain: Option<gd_render::render::terrain::TerrainCpu>,
    /// The terrain's material once spawned ([`spawn_level_root`]): the terrain patches share its layers.
    terrain_mat: Option<Handle<TerrainMaterial>>,
    objects: Vec<ObjectCpu>,
    nav: Vec<gd_sim::nav::NavLayer>,
    sectors: Option<(Vec3, gd_formats::map::sector::SectorLayers)>,
    lights: Vec<LightCpu>,
    decals: Vec<DecalCpu>,
    water: Vec<gd_render::water::WaterSurface>,
    effects: Vec<gd_render::fx::PlacedEffect>,
    proxies: Vec<gd_game::monsters::ProxyCpu>,
    npcs: Vec<(u32, String, Transform)>,
    chests: Vec<(u32, String, Transform)>,
    set_pieces: Vec<(u32, String, Vec3, f32, Option<[u32; 4]>)>,
    doors: Vec<(String, Transform, Option<[u32; 4]>)>,
    breakables: Vec<(u32, String, Transform)>,
    /// `PhysicsDecoration` props: (record, transform) (`gd_game::physics_decoration::LevelPhysicsProps`).
    physics_props: Vec<(String, Transform)>,
    blockers: Vec<(String, Vec3)>,
    waypoints: Vec<(String, Vec3, [u32; 4])>,
    shrines: Vec<(String, Transform, [u32; 4])>,
    sounds: Vec<(String, Vec3)>,
    patrol_points: Vec<(String, Vec3, [u32; 4])>,
    /// Gameplay objects placed in the level (`gd_game::monsters::placed_class`).
    placed: Vec<(u32, String, Transform)>,
    /// Map nuggets of objects nothing else spawns: `AreaOfInterest`s and
    /// static scenery with a `mapNuggetType` (record, transform; `ui/localmap.rs`).
    map_points: Vec<(String, Transform)>,
    /// Terrain heightfield for projectile collision (`collision.rs`).
    heights: Option<(u32, u32, Vec<f32>)>,
    /// What the terrain and water are made of (footsteps, `collision.rs`).
    ground: gd_sim::surface::Ground,
}

impl LevelCpu {
    pub fn origin(&self) -> Vec3 {
        self.origin
    }

    pub fn take_sectors(&mut self) -> Option<(Vec3, gd_formats::map::sector::SectorLayers)> {
        self.sectors.take()
    }
}

/// A level `Decal` entity: its record, frame and terrain faces (`decal.rs`; a
/// static decal: box half-extent max(width, height) × 0.5); `spawn_level`
/// adds the mesh faces once the level's receivers are registered.
struct DecalCpu {
    patch: gd_render::decal::DecalPatch,
    rec: gd_render::decal::DecalRecord,
    centre: Vec3,
    /// The entity's local Z.
    axis: Vec3,
    width: f32,
    height: f32,
}

/// A scenery record's texture overrides, as `Actor::Load` applies them
/// (see `CharAssets::spawn_record_model`): `baseTexture` /
/// `bumpTexture` for every msh material, else `baseTextures[i]` /
/// `bumpTextures[i]` (`;`-separated, per material; empty = keep). E.g. the
/// village floor tiles (`floor1x_wood01`: the stone `housemodular_floorsquare`
/// mesh with `housemodular_floorsquare_wood01.tex`).
struct Retex {
    base_all: Option<String>,
    bump_all: Option<String>,
    base: Vec<String>,
    bump: Vec<String>,
    /// `shader` / `glowTexture` / `glowTextures`
    /// ([`gd_render::render::material::Reshade`]).
    shader: Option<String>,
    glow_all: Option<String>,
    glow: Vec<String>,
}

impl Retex {
    fn from_record(rec: &gd_formats::arz::Record) -> Option<Self> {
        let one = |k: &str| {
            rec.str(k)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        };
        let list = |k: &str| gd_render::character::texture_list(rec, k);
        let r = Self {
            base_all: one("baseTexture"),
            bump_all: one("bumpTexture"),
            base: list("baseTextures"),
            bump: list("bumpTextures"),
            shader: one("shader"),
            glow_all: one("glowTexture"),
            glow: list("glowTextures"),
        };
        (r.base_all.is_some()
            || r.bump_all.is_some()
            || !r.base.is_empty()
            || !r.bump.is_empty()
            || r.shader.is_some()
            || r.glow_all.is_some()
            || !r.glow.is_empty())
        .then_some(r)
    }

    /// The shader / glow overrides of msh material `i`.
    fn reshade(&self, i: usize) -> gd_render::render::material::Reshade<'_> {
        gd_render::render::material::Reshade {
            shader: self.shader.as_deref(),
            glow: self
                .glow_all
                .as_deref()
                .or(self.glow.get(i).map(String::as_str)),
        }
    }

    /// (base, bump) of msh material `i`.
    fn of(&self, i: usize) -> (Option<&str>, Option<&str>) {
        (
            self.base_all
                .as_deref()
                .or(self.base.get(i).map(String::as_str)),
            self.bump_all
                .as_deref()
                .or(self.bump.get(i).map(String::as_str)),
        )
    }
}

struct ObjectCpu {
    mesh: String,
    transform: Affine3A,
    /// The record's texture overrides ([`Retex`]).
    retex: Option<Arc<Retex>>,
    /// Gets a static physics body in the world (`collision.rs`).
    solid: bool,
    /// Takes decals (`decal::receives_decals`).
    decals: bool,
    /// `maxTransparency` of an `allowTransparency` record (`see_through`).
    see_through: Option<f32>,
    /// The record's `castsShadows` (default 1). Engine.dll
    /// `Actor::Load` stores it at `Actor.castsShadows` (and the mesh instance's
    /// `GraphicsMeshInstance.castsShadows`); `GraphicsMTRenderer::BuildShadowDirectional`
    /// adds only actors with it set to the sun's shadow scene. In Devil's
    /// Crossing: floors, weeds, bedrolls, camp clutter, candles, torches.
    casts_shadows: bool,
    /// `SoundPeriodic` (`periodicSound` of a `Decoration`).
    periodic: Option<gd_game_core::world::PeriodicSound>,
    /// The record's animations (`decoration_anim`).
    anims: Option<crate::decoration_anim::DecorationAnims>,
    /// A `Floater`'s bob (`decoration_anim`).
    floater: Option<crate::decoration_anim::Floater>,
    /// A `Decoration` the hover names (`gd_game_core::world::Described`).
    described: Option<gd_game_core::world::Described>,
    /// A `TerrainDecoration`'s painted terrain: (paint, `maxPaintableAngle`, rounded region-local x / z), drawn
    /// over its upward faces (`render/terrain.rs`).
    patch: Option<(gd_formats::map::level::TerrainPatch, f32, Vec2)>,
}

impl ObjectCpu {
    /// It animates (`decoration_anim`) or bobs: what its mesh creates is attached to it and follows its
    /// posed bones (`fx::spawn_mesh_attachments`), not placed in the world.
    fn moves(&self) -> bool {
        self.anims.is_some() || self.floater.is_some()
    }
}

/// Static scenery classes. Everything else is either gameplay (spawners,
/// creatures, items — instantiated by gameplay code) or an editor helper with
/// a placeholder mesh (SoundObject, AreaOfInterest, ...).
///
/// Left out on purpose (every stock map the game loads): `PlayerSpawnPoint` (one per
/// map, at the map header's spawn point, which is what the port starts at; Game.dll
/// `PlayerSpawnPoint::InitialUpdate` hides it outside the editor); `GridEntrance` /
/// `GridEntranceDynamic` / `GridSystem`, `NpcWanderPoint`, `LightOfRaMarker` / `CerberusGeyserMarker`
/// (placed in no stock map; no script or record names them, the markers' skills exist only as base
/// templates).
fn is_scenery_class(class: &str) -> bool {
    matches!(
        class,
        "Decoration"
            | "TerrainDecoration"
            | "Destructible"
            | "Prop"
            | "WeatherDecoration"
            | "Floater"
    )
}

fn build_decal(
    rec: &gd_formats::arz::Record,
    e: &gd_formats::map::level::Entity,
    t: &gd_formats::map::terrain::Terrain,
    origin: Vec3,
) -> Option<DecalCpu> {
    let mut dec = gd_render::decal::DecalRecord::from_record(rec)?;
    // PORT: a level decal's `life` is ignored (`decal.rs`).
    dec.timing.life_ms = 0;
    // PORT: the entity's scale stretches the decal.
    let s = Vec3::from(e.scale());
    let (w, h) = (
        dec.width * s.x.abs().max(1e-3),
        dec.height * s.z.abs().max(1e-3),
    );
    let r = e.rotation;
    let c = origin + Vec3::from(e.position);
    let height = |x: i32, z: i32| {
        (x >= 0 && z >= 0 && (x as u32) < t.width && (z as u32) < t.height)
            .then(|| t.height_at_vertex(x as u32, z as u32))
    };
    let mut patch = gd_render::decal::DecalPatch::default();
    patch.gather(
        &gd_render::decal::Surface::Terrain {
            origin,
            height: &height,
        },
        c,
        gd_render::decal::extent(w, h, false),
    );
    Some(DecalCpu {
        patch,
        rec: dec,
        centre: c,
        axis: Vec3::new(r[6], r[7], r[8]),
        width: w,
        height: h,
    })
}

/// What a level entity's record places: resolved once per record id in [`build_level`].
enum Placement {
    /// Nothing (a story NPC, an `Npc` subclass's other records, an unknown proxy).
    Skip,
    /// An NPC without story hooks or a placed creature (`npcs`).
    Npc,
    /// A Shattered Realm proxy.
    Proxy,
    /// Loot containers without story hooks (`chests.rs`), lore notes placed as items (gd-game `lore`) and the
    /// food crates (`FixedItemShrine`, gd-game `chests`).
    Chest,
    SetPiece,
    Door,
    /// A breakable / a physics prop with the record's `scale` (used unless the entity has its own).
    Breakable(f32),
    PhysicsProp(f32),
    Blocker,
    /// Riftgates, respawn points and the Chthonic rifts (`gd_game::riftgates::DynamicRift`).
    Waypoint,
    Shrine,
    /// The Ascendant altar rides with the shrines (no uid needed).
    Altar,
    Sound,
    Patrol,
    AreaOfInterest,
    /// Gameplay objects placed in the level (`gd_game::monsters::placed_class`).
    Placed,
    /// Anything else: a light / decal candidate (by its path) or scenery; `nugget`: static scenery with its own
    /// map icon (dynamic obstacles: `Actor::AppendDetailMapData`).
    Other {
        nugget: bool,
        light: bool,
        decal: bool,
        scenery: Option<Box<SceneryRecord>>,
    },
}

/// A scenery record's facts every placement of it shares.
struct SceneryRecord {
    mesh: String,
    scale: f32,
    retex: Option<Arc<Retex>>,
    solid: bool,
    decals: bool,
    see_through: Option<f32>,
    casts_shadows: bool,
    periodic: Option<gd_game_core::world::PeriodicSound>,
    anims: Option<crate::decoration_anim::DecorationAnims>,
    floater: Option<crate::decoration_anim::Floater>,
    described: Option<gd_game_core::world::Described>,
    /// A `TerrainDecoration`'s `maxPaintableAngle`.
    paintable: Option<f32>,
}

impl SceneryRecord {
    fn from_record(rec: &gd_formats::arz::Record) -> Option<Self> {
        if !is_scenery_class(&rec.class) {
            return None;
        }
        // Engine.dll `Actor::InitialUpdate` → `Entity::SetPhysicsSimulation(1)` only when
        // `IsStatic()` and `GetIsPartOfLevel()`: Game.dll `Decoration::IsStatic` is `!allowPathing`,
        // `Decoration::GetIsPartOfLevel` is "no `description`". The other scenery classes keep the always-false
        // `Entity` defaults (`Destructible` overrides IsStatic to false), so only a `Decoration` gets a body.
        let solid = rec.class == "Decoration"
            && !rec.bool("allowPathing")
            && rec.str("description").is_none_or(str::is_empty);
        Some(Self {
            mesh: rec.str("mesh")?.to_owned(),
            scale: rec.f32("scale").unwrap_or(1.0),
            retex: Retex::from_record(rec).map(Arc::new),
            solid,
            decals: gd_render::decal::receives_decals(&rec.class, rec.bool("allowPathing")),
            see_through: gd_render::see_through::max_transparency(rec),
            casts_shadows: rec.i32("castsShadows") != Some(0),
            periodic: gd_game_core::world::PeriodicSound::from_record(rec),
            anims: crate::decoration_anim::DecorationAnims::from_record(rec),
            floater: crate::decoration_anim::Floater::from_record(rec),
            described: gd_game_core::world::Described::from_record(rec),
            paintable: (rec.class == "TerrainDecoration").then(|| {
                rec.f32("maxPaintableAngle")
                    .unwrap_or(gd_render::render::terrain::MAX_PAINTABLE_ANGLE)
            }),
        })
    }
}

/// What `rec_name` places (see [`Placement`]).
fn classify(data: &GameData, rec_name: &str) -> Placement {
    let lower = rec_name.to_ascii_lowercase();
    let rec = data.db.get(rec_name);
    let story = |r: &gd_formats::arz::Record| gd_game::story_world::is_story_record(r);
    // NPCs without story hooks (the story layer shows the others), and every other creature placed as a
    // `Monster` (town guards, faction champions, the Dranghoul tribe, training dummies, floor-spike traps, the
    // Loghorrean / Dermapteran queen / Master of Flesh bosses: placed only, no proxy or script spawns them):
    // gd-game makes them monsters at their record's level that fight by faction (`monsters::is_fighter`,
    // `placed_char_level`); `defaultTeamMajor` is not read by the game. Other records under `records/creatures/`
    // (breakables) go on below.
    let npc_dir = lower.starts_with("records/creatures/npcs/");
    if npc_dir || lower.starts_with("records/creatures/") {
        if rec.is_some_and(|r| {
            (r.class == "Monster" || (npc_dir && r.class.starts_with("Npc"))) && !story(r)
        }) {
            return Placement::Npc;
        }
        if npc_dir {
            return Placement::Skip;
        }
    }
    // The Shattered Realm's proxies (`ProxyEndless`, dormant until the generator activates them).
    if lower.starts_with("records/proxies/") || lower.starts_with("records/endlessdungeon/proxies/")
    {
        return if rec
            .is_some_and(|r| matches!(r.class.as_str(), "Proxy" | "ProxyAmbush" | "ProxyEndless"))
        {
            Placement::Proxy
        } else {
            Placement::Skip
        };
    }
    let Some(r) = rec else {
        return Placement::Other {
            nugget: false,
            light: false,
            decal: false,
            scenery: None,
        };
    };
    let class = r.class.as_str();
    match class {
        "FixedItemContainer" | "ItemNote" | "FixedItemShrine" if !story(r) => Placement::Chest,
        "SetPiece" => Placement::SetPiece,
        "FixedDoor" | "FixedItemDoor" | "DungeonEntrance" if !story(r) => Placement::Door,
        "Destructible" if !story(r) => Placement::Breakable(r.f32("scale").unwrap_or(1.0)),
        "PhysicsDecoration" if !story(r) => Placement::PhysicsProp(r.f32("scale").unwrap_or(1.0)),
        "NavBlocker" if !story(r) => Placement::Blocker,
        _ if gd_game::riftgates::waypoint_kind(class).is_some() || class == "DynamicTeleporter" => {
            Placement::Waypoint
        }
        "StaticShrine" | "MonsterShrine" => Placement::Shrine,
        "AscendantAltar" => Placement::Altar,
        "SoundObject" => Placement::Sound,
        "PatrolPoint" => Placement::Patrol,
        "AreaOfInterest" => Placement::AreaOfInterest,
        _ if gd_game::monsters::placed_class(class) && !story(r) => Placement::Placed,
        _ => Placement::Other {
            nugget: is_scenery_class(class)
                && class != "Destructible"
                && r.str("mapNuggetType")
                    .is_some_and(|t| !t.is_empty() && t != "None")
                && !story(r),
            light: lower.contains("light"),
            decal: lower.contains("decal") && class == "Decal",
            scenery: SceneryRecord::from_record(r).map(Box::new),
        },
    }
}

/// A level entity's frame: its rotation rows scaled by `scale`, at its region-local position.
fn entity_affine(e: &gd_formats::map::level::Entity, origin: Vec3, scale: Vec3) -> Affine3A {
    let r = e.rotation;
    let basis = Mat3::from_cols(
        Vec3::new(r[0], r[1], r[2]),
        Vec3::new(r[3], r[4], r[5]),
        Vec3::new(r[6], r[7], r[8]),
    );
    Affine3A::from_mat3_translation(
        basis * Mat3::from_diagonal(scale),
        origin + Vec3::from(e.position),
    )
}

fn entity_transform(e: &gd_formats::map::level::Entity, origin: Vec3, scale: Vec3) -> Transform {
    Transform::from_matrix(Mat4::from(entity_affine(e, origin, scale)))
}

/// The entity's own scale, else the record's.
fn placed_scale(e: &gd_formats::map::level::Entity, record: f32) -> Vec3 {
    let s = Vec3::from(e.scale());
    if s == Vec3::ONE {
        Vec3::splat(record)
    } else {
        s
    }
}

pub fn build_level(
    map: &MapFile,
    data: &GameData,
    layers: &gd_render::render::terrain::TerrainLayers,
    high: bool,
    index: usize,
) -> anyhow::Result<LevelCpu> {
    let region = &map.regions[index];
    let level: Level = map.level(index)?;
    let origin = region_origin(region);
    let terrain = match &level.terrain {
        Some(t) => Some(gd_render::render::terrain::build(t, data, layers, high)?),
        None => None,
    };
    let mut objects = Vec::new();
    let mut lights = Vec::new();
    let mut decals = Vec::new();
    let mut placements: HashMap<u32, Placement> = HashMap::default();
    let mut proxies = Vec::new();
    let mut npcs = Vec::new();
    let mut chests = Vec::new();
    let mut set_pieces = Vec::new();
    let mut doors = Vec::new();
    let mut breakables = Vec::new();
    let mut physics_props = Vec::new();
    let mut blockers = Vec::new();
    let mut waypoints = Vec::new();
    let mut shrines = Vec::new();
    let mut sounds = Vec::new();
    let mut patrol_points = Vec::new();
    let mut placed_objects = Vec::new();
    let mut map_points = Vec::new();
    for (ei, e) in level.entities.iter().enumerate() {
        let rec_name = level.record_of(e);
        let at = origin + Vec3::from(e.position);
        let placement = placements
            .entry(e.record)
            .or_insert_with(|| classify(data, rec_name));
        match placement {
            Placement::Skip => {}
            Placement::Npc => {
                let r = e.rotation;
                let x = Vec3::new(r[0], r[1], r[2]).normalize_or(Vec3::X);
                let z = Vec3::new(r[6], r[7], r[8]).normalize_or(Vec3::Z);
                let rot = Quat::from_mat3(&Mat3::from_cols(x, Vec3::Y, z));
                // The placement's scale replaces the record's (`gd_game::creature` module doc).
                npcs.push((
                    ei as u32,
                    rec_name.to_owned(),
                    Transform::from_translation(at)
                        .with_rotation(rot)
                        .with_scale(Vec3::splat(e.scale()[1])),
                ));
            }
            Placement::Proxy => proxies.push(gd_game::monsters::ProxyCpu {
                entity: ei as u32,
                record: rec_name.to_owned(),
                position: at,
                uid: e.uid,
            }),
            Placement::Placed => placed_objects.push((
                ei as u32,
                rec_name.to_owned(),
                entity_transform(e, origin, Vec3::ONE),
            )),
            Placement::AreaOfInterest => {
                map_points.push((rec_name.to_owned(), entity_transform(e, origin, Vec3::ONE)))
            }
            Placement::Patrol => {
                if let Some(uid) = e.uid {
                    patrol_points.push((rec_name.to_owned(), at, uid));
                }
            }
            Placement::Sound => sounds.push((rec_name.to_owned(), at)),
            Placement::Altar => shrines.push((
                rec_name.to_owned(),
                entity_transform(e, origin, Vec3::ONE),
                e.uid.unwrap_or_default(),
            )),
            Placement::Shrine => {
                if let Some(uid) = e.uid {
                    shrines.push((
                        rec_name.to_owned(),
                        entity_transform(e, origin, Vec3::ONE),
                        uid,
                    ));
                }
            }
            Placement::Waypoint => {
                if let Some(uid) = e.uid {
                    waypoints.push((rec_name.to_owned(), at, uid));
                }
            }
            Placement::Blocker => blockers.push((rec_name.to_owned(), at)),
            Placement::Breakable(s) => {
                let tf = entity_transform(e, origin, placed_scale(e, *s));
                breakables.push((ei as u32, rec_name.to_owned(), tf));
            }
            Placement::PhysicsProp(s) => {
                physics_props.push((
                    rec_name.to_owned(),
                    entity_transform(e, origin, placed_scale(e, *s)),
                ));
            }
            Placement::Door => doors.push((
                rec_name.to_owned(),
                entity_transform(e, origin, Vec3::ONE),
                e.uid,
            )),
            Placement::SetPiece => {
                let yaw = e.rotation[6].atan2(e.rotation[8]);
                set_pieces.push((ei as u32, rec_name.to_owned(), at, yaw, e.uid));
            }
            Placement::Chest => chests.push((
                ei as u32,
                rec_name.to_owned(),
                entity_transform(e, origin, Vec3::ONE),
            )),
            Placement::Other {
                nugget,
                light,
                decal,
                scenery,
            } => {
                if *nugget {
                    map_points.push((rec_name.to_owned(), entity_transform(e, origin, Vec3::ONE)));
                }
                if *light
                    && let Some(rec) = data.db.get(rec_name)
                    && let Some(l) = light_from_record(rec, at)
                {
                    lights.push(l);
                    continue;
                }
                if *decal && let Some(t) = &level.terrain {
                    if let Some(rec) = data.db.get(rec_name)
                        && let Some(d) = build_decal(rec, e, t, origin)
                    {
                        decals.push(d);
                    }
                    continue;
                }
                let Some(s) = scenery else { continue };
                // Engine.dll `TerrainPatch::CreateGeometry`: the paint's texel grid is
                // centred on floor(position + ½) in x / z.
                let patch = s.paintable.and_then(|angle| {
                    let p = e.terrain_patch()?;
                    let c = Vec2::new(e.position[0], e.position[2]) + Vec2::splat(0.5);
                    Some((p, angle, c.floor()))
                });
                objects.push(ObjectCpu {
                    mesh: s.mesh.clone(),
                    transform: entity_affine(e, origin, placed_scale(e, s.scale)),
                    retex: s.retex.clone(),
                    solid: s.solid,
                    decals: s.decals,
                    see_through: s.see_through,
                    casts_shadows: s.casts_shadows,
                    periodic: s.periodic.clone(),
                    anims: s.anims.clone(),
                    floater: s.floater.clone(),
                    described: s.described.clone(),
                    patch,
                });
            }
        }
    }
    let nav = match &level.pathing {
        Some(p) => {
            let all: Vec<_> = p.tiles.iter().flatten().cloned().collect();
            let ch = gd_sim::nav::NavWorld::cell_height(&all);
            all.iter()
                .filter_map(|t| gd_sim::nav::NavLayer::from_tile(t, ch))
                .collect()
        }
        None => Vec::new(),
    };
    let extent = level
        .terrain
        .as_ref()
        .map(|t| Vec3::new((t.width - 1) as f32, 0.0, (t.height - 1) as f32))
        .unwrap_or(Vec3::from(region.bbox.half) * 2.0);
    let sectors = level.sector_layers.clone().map(|s| (extent, s));
    let water = level
        .water
        .as_ref()
        .map(|w| gd_render::water::build(w, level.terrain.as_ref(), origin))
        .unwrap_or_default();
    let effects = gd_render::fx::collect_placed(&level, data, origin);
    let heights = level
        .terrain
        .as_ref()
        .map(|t| (t.width, t.height, t.heights.clone()));
    let surface_of = |record: &str| {
        data.db
            .get(record)
            .and_then(|r| r.str("surfaceType").map(gd_sim::surface::surface_type))
            .unwrap_or(gd_sim::surface::NO_SURFACE)
    };
    let ground =
        gd_sim::surface::Ground::new(level.terrain.as_ref(), level.water.as_ref(), surface_of);
    Ok(LevelCpu {
        index,
        origin,
        terrain,
        terrain_mat: None,
        heights,
        ground,
        objects,
        nav,
        sectors,
        lights,
        decals,
        water,
        effects,
        proxies,
        npcs,
        chests,
        set_pieces,
        doors,
        breakables,
        physics_props,
        blockers,
        waypoints,
        shrines,
        sounds,
        patrol_points,
        placed: placed_objects,
        map_points,
    })
}

/// Polls the background level builds; a finished one's nav is in at once, every mesh of its objects starts
/// loading in the background (`ModelCache::model_ready`) and its GPU spawn is queued for [`spawn_levels`] (a
/// bounded slice per frame), not built synchronously here.
fn finish_levels(
    mut world: ResMut<StreamedMap>,
    mut nav: ResMut<Nav>,
    data: Res<GameData>,
    mut cache: ResMut<ModelCache>,
    bodies: Res<gd_game_core::creature::BodyCache>,
) {
    let done: Vec<(usize, anyhow::Result<LevelCpu>)> = world
        .pending
        .iter_mut()
        .filter_map(|(i, t)| block_on(future::poll_once(t)).map(|r| (*i, r)))
        .collect();
    for (i, r) in done {
        world.pending.remove(&i);
        match r {
            Ok(mut cpu) => {
                nav.0.add(i as u64, std::mem::take(&mut cpu.nav));
                // Every mesh starts loading; the ones already built are held from now on ([`SpawnJob`]`::held`).
                let (mut held, mut loading) = (Vec::new(), Vec::new());
                let mut requested = HashSet::default();
                for o in cpu
                    .objects
                    .iter()
                    .filter(|o| requested.insert(o.mesh.as_str()))
                {
                    match cache.model_ready(&data, &bodies, &o.mesh) {
                        model::Ready::Done(m) => held.push(m),
                        model::Ready::Loading => loading.push(o.mesh.clone()),
                        model::Ready::Failed => {}
                    }
                }
                drop(requested);
                let sectors = cpu.sectors.take();
                world.spawning.insert(
                    i,
                    SpawnJob {
                        cpu,
                        sectors,
                        root: None,
                        cursor: 0,
                        acc: SpawnAcc::default(),
                        decals: None,
                        held,
                        loading,
                        textures: Vec::new(),
                    },
                );
            }
            Err(e) => {
                warn!("level {i}: {e:#}");
                world.failed.insert(i);
            }
        }
    }
}

/// Spawns a built level under a new root. `gameplay`: the world's levels
/// (their proxies, NPCs, chests, ... go to gd-game through `LevelProxies`, map
/// points to the local map); without it only what is drawn (the main menu's
/// scene, `render::menu`).
#[allow(clippy::too_many_arguments)]
pub fn spawn_level(
    mut cpu: LevelCpu,
    gameplay: bool,
    commands: &mut Commands,
    data: &GameData,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    terrain_mats: &mut Assets<TerrainMaterial>,
    decal_mats: &mut Assets<DecalMaterial>,
    mats: &mut gd_render::render::material::MaterialAssets,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
    cache: &mut ModelCache,
    bodies: &mut gd_game_core::creature::BodyCache,
    textures: &mut Textures,
    coll: &mut gd_game::collision::WorldCollision,
    receivers: &mut gd_render::decal::DecalReceivers,
    hooks: &gd_game_core::hooks::RenderHooks,
) -> Entity {
    let root = spawn_level_root(
        &mut cpu,
        gameplay,
        commands,
        data,
        meshes,
        images,
        terrain_mats,
        mats,
        textures,
        hooks,
    );
    if gameplay {
        attach_gameplay(commands, root, &mut cpu);
    }
    // Built on the spot (`ModelCache::model`): the caller (the menu scene) takes the whole level at once.
    let mut acc = SpawnAcc::default();
    let terrain = cpu.terrain_mat.as_ref().map(|h| (h, cpu.origin));
    for o in &cpu.objects {
        if let Some(m) = cache.model(
            data, bodies, &o.mesh, meshes, mats, bindposes, textures, images,
        ) {
            spawn_object(
                o,
                &m,
                root,
                commands,
                data,
                meshes,
                images,
                mats,
                textures,
                &mut acc,
                terrain,
                terrain_mats,
            );
        }
    }
    receivers.add_level(
        cpu.index,
        acc.receiving
            .iter()
            .map(|(p, m, tf)| (p.as_str(), &**m, *tf)),
    );
    let all = std::mem::take(&mut cpu.decals);
    let mut decals = if hooks.no_decals { Vec::new() } else { all }.into_iter();
    // No budget here: the whole level at once.
    let far = Instant::now() + Duration::from_secs(3600);
    spawn_level_decals(
        &mut decals,
        far,
        root,
        commands,
        data,
        &mats.defaults,
        meshes,
        images,
        decal_mats,
        textures,
        receivers,
        None,
    );
    spawn_level_collision(cpu.index, cpu.origin, coll, cpu.heights, cpu.ground, acc);
    root
}

/// A scenery mesh box for collision / decals: (record, mesh, placement).
type SceneryBox = (String, Arc<gd_formats::msh::Mesh>, Affine3A);
/// A line-of-sight box: (mesh, placement).
type LosBox = (Arc<gd_formats::msh::Mesh>, Affine3A);

/// The accumulators of the scenery object loop ([`spawn_level_objects`]), kept
/// across the per-frame slices of a streaming spawn.
#[derive(Default)]
struct SpawnAcc {
    phys: Vec<SceneryBox>,
    floors: Vec<SceneryBox>,
    los: Vec<LosBox>,
    receiving: Vec<SceneryBox>,
    nobj: usize,
}

/// A level whose CPU build finished and whose GPU spawn is spread over frames
/// ([`spawn_levels`]): the root once made, the object cursor and accumulators, then the decals still to clip. It
/// holds what it waits on (module doc): the models built so far, and the textures of the object or decal at the
/// cursor already decoded.
pub(crate) struct SpawnJob {
    cpu: LevelCpu,
    sectors: Option<(Vec3, gd_formats::map::sector::SectorLayers)>,
    root: Option<Entity>,
    cursor: usize,
    acc: SpawnAcc,
    /// The models of its objects already built, held until the level is spawned (its parts hold them then).
    held: Vec<Arc<model::ModelAsset>>,
    /// The meshes still loading in the background; each moves to `held` as it is built ([`spawn_levels`]).
    loading: Vec<String>,
    /// The decoded textures of the object or decal waiting at the cursor (`MaterialAssets::variant_ready`,
    /// `decal::textures_ready`); cleared once it spawns.
    textures: Vec<Handle<Image>>,
    /// The decals still to clip, in order; `None` until the objects are spawned and the receivers registered.
    decals: Option<std::vec::IntoIter<DecalCpu>>,
}

/// Hands a level's gameplay content to gd-game (`LevelProxies`: proxies, NPCs, chests, doors, …;
/// `LevelPhysicsProps`). Only once the level is in `Levels` (the same frame as `Levels::add`): gd-game's cull
/// systems unload whatever belongs to a level that is not loaded, so content handed over while the level is still
/// streaming in slices was despawned as it appeared, and the root never repopulated.
fn attach_gameplay(commands: &mut Commands, root: Entity, cpu: &mut LevelCpu) {
    commands
        .entity(root)
        .insert(gd_game::monsters::LevelProxies {
            level: cpu.index,
            proxies: std::mem::take(&mut cpu.proxies),
            npcs: std::mem::take(&mut cpu.npcs),
            chests: std::mem::take(&mut cpu.chests),
            set_pieces: std::mem::take(&mut cpu.set_pieces),
            doors: std::mem::take(&mut cpu.doors),
            breakables: std::mem::take(&mut cpu.breakables),
            blockers: std::mem::take(&mut cpu.blockers),
            waypoints: std::mem::take(&mut cpu.waypoints),
            shrines: std::mem::take(&mut cpu.shrines),
            sounds: std::mem::take(&mut cpu.sounds),
            patrol_points: std::mem::take(&mut cpu.patrol_points),
            placed: std::mem::take(&mut cpu.placed),
        });
    commands
        .entity(root)
        .insert(gd_game::physics_decoration::LevelPhysicsProps {
            level: cpu.index,
            props: std::mem::take(&mut cpu.physics_props),
        });
}

/// The level root and everything but the scenery objects: map points, terrain + grass, placed effects, water,
/// lights. Consumes those fields of `cpu`, leaving the gameplay content for [`attach_gameplay`] and the objects /
/// decals / collision for the slices.
#[allow(clippy::too_many_arguments)]
fn spawn_level_root(
    cpu: &mut LevelCpu,
    gameplay: bool,
    commands: &mut Commands,
    data: &GameData,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    terrain_mats: &mut Assets<TerrainMaterial>,
    mats: &mut gd_render::render::material::MaterialAssets,
    textures: &mut Textures,
    hooks: &gd_game_core::hooks::RenderHooks,
) -> Entity {
    let root = commands
        .spawn((
            Transform::IDENTITY,
            Visibility::default(),
            Name::new(format!("level {}", cpu.index)),
        ))
        .id();
    for (record, at) in std::mem::take(&mut cpu.map_points)
        .into_iter()
        .filter(|_| gameplay)
    {
        let e = commands
            .spawn((gd_game_core::world::MapPoint { record }, at))
            .id();
        commands.entity(root).add_child(e);
    }
    if let Some(mut t) = cpu.terrain.take().filter(|_| !hooks.no_terrain) {
        let blocks = t.blocks.clone();
        // The terrain layers' grass (render/grass.rs): terrain-local blades.
        gd_render::render::grass::spawn(
            commands,
            root,
            cpu.origin,
            std::mem::take(&mut t.grass),
            data,
            textures,
            images,
            meshes,
            mats,
        );
        let held = std::mem::take(&mut t.held);
        let (mesh, mat) = t.material(cpu.origin, images, &mats.defaults);
        let mat = terrain_mats.add(mat);
        cpu.terrain_mat = Some(mat.clone());
        let e = commands
            .spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(mat),
                Transform::from_translation(cpu.origin),
                // The terrain only receives sun shadows (render/terrain.rs).
                bevy::light::NotShadowCaster,
                blocks,
                // Its layer textures stay decoded for the next level builds while it is loaded.
                held,
            ))
            .id();
        commands.entity(root).add_child(e);
    }
    for fx in &cpu.effects {
        let e = gd_render::fx::spawn_placed(commands, fx);
        commands.entity(root).add_child(e);
    }
    for w in std::mem::take(&mut cpu.water) {
        let e = commands
            .spawn((w, Transform::IDENTITY, Visibility::default()))
            .id();
        commands.entity(root).add_child(e);
    }
    for l in std::mem::take(&mut cpu.lights) {
        let id = spawn_light(commands, &l);
        commands.entity(root).add_child(id);
    }
    root
}

/// Spawns the scenery objects of `objects[*cursor..]` in order while their model is built, advancing the cursor
/// and folding their collision / decal boxes into `acc`; one whose mesh cannot load is skipped. Stops at the first
/// model still loading in the background (`ModelCache::model_ready`, retried next frame), at a retextured object
/// whose textures or override shader are still loading ([`retex_ready`]; the decoded ones held in `held`), or once
/// `deadline` has passed and `spawned` (this frame's objects across the levels) is not 0, so a frame always spawns
/// one.
#[allow(clippy::too_many_arguments)]
fn spawn_level_objects(
    objects: &[ObjectCpu],
    cursor: &mut usize,
    deadline: Instant,
    spawned: &mut usize,
    root: Entity,
    commands: &mut Commands,
    data: &GameData,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    mats: &mut gd_render::render::material::MaterialAssets,
    cache: &mut ModelCache,
    bodies: &gd_game_core::creature::BodyCache,
    textures: &mut Textures,
    held: &mut Vec<Handle<Image>>,
    acc: &mut SpawnAcc,
    // The level's terrain material and origin, and the terrain materials (the terrain patches).
    terrain: Option<(&Handle<TerrainMaterial>, Vec3)>,
    terrain_mats: &mut Assets<TerrainMaterial>,
) {
    while let Some(o) = objects.get(*cursor) {
        if *spawned > 0 && Instant::now() >= deadline {
            return;
        }
        match cache.model_ready(data, bodies, &o.mesh) {
            model::Ready::Loading => return,
            model::Ready::Failed => {}
            model::Ready::Done(m) => {
                // This check's holds replace the last one's only after it took them again (no duplicates).
                let mut now = Vec::new();
                let ready = retex_ready(o, &m, data, mats, textures, images, &mut now);
                *held = now;
                if !ready {
                    return;
                }
                spawn_object(
                    o,
                    &m,
                    root,
                    commands,
                    data,
                    meshes,
                    images,
                    mats,
                    textures,
                    acc,
                    terrain,
                    terrain_mats,
                );
                *spawned += 1;
                held.clear();
            }
        }
        *cursor += 1;
    }
}

/// Whether [`spawn_object`]'s retexture / reshade of `o` with model `m` decodes and analyses nothing on the game
/// thread (`MaterialAssets::variant_ready` of each part); the decoded textures go into `held`.
fn retex_ready(
    o: &ObjectCpu,
    m: &model::ModelAsset,
    data: &GameData,
    mats: &mut gd_render::render::material::MaterialAssets,
    textures: &mut Textures,
    images: &mut Assets<Image>,
    held: &mut Vec<Handle<Image>>,
) -> bool {
    let Some(rt) = &o.retex else {
        return true;
    };
    let mut ready = true;
    for (mi, (_, def)) in m.part_materials.iter().zip(&m.parts) {
        let Some(msh) = m.msh_material(*mi) else {
            continue;
        };
        let (base, bump) = rt.of(*mi as usize);
        ready &= mats.variant_ready(
            data,
            def,
            msh,
            m.mesh,
            rt.reshade(*mi as usize),
            base,
            bump,
            textures,
            images,
            held,
        );
    }
    ready
}

/// Spawns one scenery object with its model `m` under `root`, folding its collision / decal boxes into `acc`.
#[allow(clippy::too_many_arguments)]
fn spawn_object(
    o: &ObjectCpu,
    m: &Arc<model::ModelAsset>,
    root: Entity,
    commands: &mut Commands,
    data: &GameData,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    mats: &mut gd_render::render::material::MaterialAssets,
    textures: &mut Textures,
    acc: &mut SpawnAcc,
    terrain: Option<(&Handle<TerrainMaterial>, Vec3)>,
    terrain_mats: &mut Assets<TerrainMaterial>,
) {
    acc.nobj += 1;
    acc.los.push((m.msh.clone(), o.transform));
    if o.solid {
        acc.phys.push((o.mesh.clone(), m.msh.clone(), o.transform));
    } else {
        acc.floors
            .push((o.mesh.clone(), m.msh.clone(), o.transform));
    }
    if o.decals {
        acc.receiving
            .push((o.mesh.clone(), m.msh.clone(), o.transform));
    }
    let e = commands
        .spawn((
            Transform::from_matrix(Mat4::from(o.transform)),
            Visibility::default(),
        ))
        .id();
    let parts = match &o.retex {
        Some(rt) => {
            let materials: HashMap<u32, gd_render::render::material::PartMaterial> = m
                .part_materials
                .iter()
                .zip(&m.parts)
                .filter_map(|(mi, (_, def))| {
                    let (base, bump) = rt.of(*mi as usize);
                    let msh = m.msh_material(*mi)?;
                    let h = mats.reshade(
                        data,
                        def,
                        msh,
                        m.mesh,
                        rt.reshade(*mi as usize),
                        textures,
                        images,
                    );
                    Some((*mi, mats.retexture(data, &h, base, bump, textures, images)))
                })
                .collect();
            model::spawn_model_with(commands, e, m, |mi, d| {
                materials.get(&mi).cloned().unwrap_or_else(|| d.clone())
            })
        }
        None => model::spawn_model_with(commands, e, m, |_, d| d.clone()),
    };
    if !o.casts_shadows {
        for p in &parts {
            commands.entity(*p).insert(bevy::light::NotShadowCaster);
        }
    }
    // A `TerrainDecoration`'s painted terrain over its upward faces (`render/terrain.rs`).
    if let (Some((paint, angle, centre)), Some((level_mat, origin))) = (&o.patch, terrain)
        && let Some(level) = terrain_mats.get(level_mat).cloned()
    {
        let rotation = Mat3::from_cols(
            o.transform.matrix3.x_axis.normalize_or_zero().into(),
            o.transform.matrix3.y_axis.normalize_or_zero().into(),
            o.transform.matrix3.z_axis.normalize_or_zero().into(),
        );
        let half = Vec2::new(paint.width as f32, paint.depth as f32) * 0.5;
        let corner = origin + (*centre - half).extend(0.0).xzy();
        if let Some(mesh) = gd_render::render::terrain::patch_mesh(&m.msh, rotation, *angle)
            && let Some(mat) =
                gd_render::render::terrain::patch_material(&level, paint, corner, images)
        {
            let p = commands
                .spawn((
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(terrain_mats.add(mat)),
                    Transform::IDENTITY,
                    bevy::light::NotShadowCaster,
                    gd_render::render::gbuffer_blend::GBufferBlend::TerrainPatch,
                ))
                .id();
            commands.entity(e).add_child(p);
        }
    }
    if let Some(p) = &o.periodic {
        commands.entity(e).insert(p.clone());
    }
    if let Some(a) = &o.anims {
        commands.entity(e).insert(a.clone());
    }
    if o.moves() {
        gd_render::fx::spawn_mesh_attachments(commands, data, m, e);
    } else {
        // What the mesh of still scenery creates (`fx::mesh_creations`), placed in the world once: its
        // effects and lights (candles, fire barrels, lanterns).
        for fx in gd_render::fx::mesh_attached(m, o.transform) {
            let id = gd_render::fx::spawn_placed(commands, &fx);
            commands.entity(root).add_child(id);
        }
        for (record, at) in gd_render::fx::mesh_lights(m) {
            if let Some(l) = data
                .db
                .get(record)
                .and_then(|rec| light_from_record(rec, o.transform.transform_point3(at)))
            {
                let id = spawn_light(commands, &l);
                commands.entity(root).add_child(id);
            }
        }
    }
    if let Some(d) = o.described.clone() {
        commands.entity(e).insert(d);
    }
    if let Some(mut f) = o.floater.clone() {
        // The region box: the mesh box's corners through the placement (module doc of `decoration_anim`).
        let (lo, hi) = m.msh.bounding_box.unwrap_or_default();
        let (mut min, mut max) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
        for i in 0..8 {
            let c = Vec3::new(
                if i & 1 == 0 { lo[0] } else { hi[0] },
                if i & 2 == 0 { lo[1] } else { hi[1] },
                if i & 4 == 0 { lo[2] } else { hi[2] },
            );
            let w = Vec3::from(o.transform.transform_point3a(c.into()));
            (min, max) = (min.min(w), max.max(w));
        }
        f.place(o.transform.translation.y, min, max);
        commands.entity(e).insert(f);
    }
    if let Some(st) = o.see_through.and_then(|max| {
        gd_render::see_through::SeeThrough::new(max, &m.msh, o.transform, &parts, &m.part_sections)
    }) {
        commands.entity(e).insert(st);
    }
    commands.entity(root).add_child(e);
}

/// Clips and spawns the level's decals in order while `deadline` has not passed (at least one a frame), the
/// receivers registered first ([`spawn_levels`]); true once none is left. With `held` (streaming), a decal whose
/// textures are still decoding (`decal::textures_ready`) stops the slice until a later frame, the decoded ones held
/// in `held`; without it (the menu scene's whole-level spawn) they decode on the spot.
#[allow(clippy::too_many_arguments)]
fn spawn_level_decals(
    decals: &mut std::vec::IntoIter<DecalCpu>,
    deadline: Instant,
    root: Entity,
    commands: &mut Commands,
    data: &GameData,
    defaults: &gd_render::render::GdDefaults,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    decal_mats: &mut Assets<DecalMaterial>,
    textures: &mut Textures,
    receivers: &gd_render::decal::DecalReceivers,
    mut held: Option<&mut Vec<Handle<Image>>>,
) -> bool {
    let mut done = 0;
    loop {
        if done > 0 && Instant::now() >= deadline {
            return false;
        }
        if let (Some(held), Some(next)) = (held.as_deref_mut(), decals.as_slice().first()) {
            // This check's holds replace the last one's only after it took them again (no duplicates).
            let mut now = Vec::new();
            let ready =
                gd_render::decal::textures_ready(&next.rec, data, textures, images, &mut now);
            *held = now;
            if !ready {
                return false;
            }
        }
        let Some(mut d) = decals.next() else {
            return true;
        };
        done += 1;
        // A level decal takes the static meshes in its box (`decal.rs`; `Decal::InitialUpdate`: meshes of
        // levels loaded later are not added).
        if !d.rec.terrain_only {
            receivers.gather(
                &mut d.patch,
                d.centre,
                gd_render::decal::extent(d.width, d.height, false),
                &d.rec,
                false,
            );
        }
        let Some(mesh) = d.patch.build(d.centre, d.axis, d.width, d.height) else {
            continue;
        };
        let mesh = meshes.add(mesh);
        // Level decals fade in / pulse and go at their `life` (`decal` module docs).
        let fade = d.rec.timing.animated().then_some(None);
        for e in gd_render::decal::spawn(
            commands, &d.rec, mesh, defaults, data, textures, images, decal_mats, fade,
        ) {
            commands
                .entity(e)
                .insert((gd_render::decal::LevelDecal, Visibility::default()))
                .insert(ChildOf(root));
        }
    }
}

/// Adds the level's collision once its objects and decals are spawned.
fn spawn_level_collision(
    index: usize,
    origin: Vec3,
    coll: &mut gd_game::collision::WorldCollision,
    heights: Option<(u32, u32, Vec<f32>)>,
    ground: gd_sim::surface::Ground,
    acc: SpawnAcc,
) {
    debug!(
        "level {}: {nobj} objects, {} with physics",
        index,
        acc.phys.len(),
        nobj = acc.nobj
    );
    coll.add_level(index, origin, heights, acc.phys.into_iter());
    coll.add_surfaces(index, acc.floors.into_iter());
    coll.add_los_boxes(index, acc.los.into_iter());
    coll.add_ground(index, ground);
}

/// The streaming spawn's time a frame (module doc): [`spawn_levels`] stops spawning scenery objects and clipping
/// decals once it has run this long (after at least one of each), so a fresh area's spawn is spread over frames
/// instead of freezing one (the engine streams regions incrementally).
const SPAWN_BUDGET: Duration = Duration::from_millis(3);

/// Drains the streaming spawn queue ([`finish_levels`]) a bounded slice per
/// frame: at most one new level root (terrain / grass / effects) plus the scenery objects whose models are built,
/// in order, then the level's decals, within [`SPAWN_BUDGET`]; a level is marked loaded only
/// when fully spawned, so `StreamedMap::area_ready` waits for it (initial load and
/// teleports alike).
#[allow(clippy::too_many_arguments)]
fn spawn_levels(
    mut world: ResMut<StreamedMap>,
    mut levels: ResMut<Levels>,
    mut commands: Commands,
    data: Res<GameData>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut terrain_mats: ResMut<Assets<TerrainMaterial>>,
    mut decal_mats: ResMut<Assets<DecalMaterial>>,
    mut mats: gd_render::render::material::MaterialAssets,
    (mut cache, bodies): (ResMut<ModelCache>, Res<gd_game_core::creature::BodyCache>),
    mut textures: ResMut<Textures>,
    mut coll: ResMut<gd_game::collision::WorldCollision>,
    mut receivers: ResMut<gd_render::decal::DecalReceivers>,
    hooks: Res<gd_game_core::hooks::Hooks>,
) {
    if world.spawning.is_empty() {
        return;
    }
    let mut keys: Vec<usize> = world.spawning.keys().copied().collect();
    keys.sort_unstable();
    let deadline = Instant::now() + SPAWN_BUDGET;
    let mut spawned = 0;
    let mut base_done = false;
    let mut finished = Vec::new();
    for i in keys {
        let Some(job) = world.spawning.get_mut(&i) else {
            continue;
        };
        // Each model of the level is held from the frame it is built (module doc).
        job.loading
            .retain(|p| match cache.model_ready(&data, &bodies, p) {
                model::Ready::Done(m) => {
                    job.held.push(m);
                    false
                }
                model::Ready::Loading => true,
                model::Ready::Failed => false,
            });
        if job.root.is_none() {
            // One new level root per frame: its terrain / grass is a slice too.
            if base_done {
                continue;
            }
            // Its grass layers' textures decode in the background first (a new one would stall the root's frame).
            if let Some(t) = job
                .cpu
                .terrain
                .as_ref()
                .filter(|_| !hooks.render.no_terrain)
            {
                let mut ready = true;
                for g in &t.grass.batches {
                    ready &= textures.ready(&data, &g.texture, false, &mut images);
                }
                if !ready {
                    continue;
                }
            }
            job.root = Some(spawn_level_root(
                &mut job.cpu,
                true,
                &mut commands,
                &data,
                &mut meshes,
                &mut images,
                &mut terrain_mats,
                &mut mats,
                &mut textures,
                &hooks.render,
            ));
            base_done = true;
        }
        let root = job.root.unwrap();
        if job.cursor < job.cpu.objects.len() {
            spawn_level_objects(
                &job.cpu.objects,
                &mut job.cursor,
                deadline,
                &mut spawned,
                root,
                &mut commands,
                &data,
                &mut meshes,
                &mut images,
                &mut mats,
                &mut cache,
                &bodies,
                &mut textures,
                &mut job.textures,
                &mut job.acc,
                job.cpu.terrain_mat.as_ref().map(|h| (h, job.cpu.origin)),
                &mut terrain_mats,
            );
        }
        if job.cursor < job.cpu.objects.len() {
            continue;
        }
        // The objects are in: their decal receivers, then the decals a slice at a time.
        let decals = job.decals.get_or_insert_with(|| {
            receivers.add_level(
                i,
                job.acc
                    .receiving
                    .iter()
                    .map(|(p, m, tf)| (p.as_str(), &**m, *tf)),
            );
            let all = std::mem::take(&mut job.cpu.decals);
            if hooks.render.no_decals {
                Vec::new()
            } else {
                all
            }
            .into_iter()
        });
        if spawn_level_decals(
            decals,
            deadline,
            root,
            &mut commands,
            &data,
            &mats.defaults,
            &mut meshes,
            &mut images,
            &mut decal_mats,
            &mut textures,
            &receivers,
            Some(&mut job.textures),
        ) {
            finished.push(i);
        }
    }
    for i in finished {
        let Some(mut job) = world.spawning.remove(&i) else {
            continue;
        };
        let root = job.root.unwrap();
        attach_gameplay(&mut commands, root, &mut job.cpu);
        let (index, origin) = (job.cpu.index, job.cpu.origin);
        spawn_level_collision(
            index,
            origin,
            &mut coll,
            job.cpu.heights,
            job.cpu.ground,
            job.acc,
        );
        levels.add(i, root, origin, job.sectors);
    }
}

/// The map's patrol and proxy-patroller instance groups (`gd_game::patrol`).
fn build_patrol_groups(
    world: Option<Res<StreamedMap>>,
    mut groups: ResMut<gd_game::patrol::PatrolGroups>,
) {
    let (Some(world), false) = (world, groups.built) else {
        return;
    };
    groups.built = true;
    let list: Vec<gd_sim::patrol::Group> = world
        .map
        .instance_groups
        .iter()
        .filter(|g| {
            g.kind == gd_sim::patrol::PATROL_POINTS
                || g.kind == gd_sim::patrol::PROXY_PATROLLERS
                || g.kind == gd_sim::patrol::UNIFIED_ENTITIES
        })
        .map(|g| gd_sim::patrol::Group {
            name: g.name.clone(),
            kind: g.kind.clone(),
            id: g.group_id,
            entities: g.entries.iter().map(|e| e.uid).collect(),
            children: g.links.clone(),
        })
        .collect();
    info!("patrol: {} groups", list.len());
    groups.groups = gd_sim::patrol::PatrolGroups::new(list);
}

/// Opens a Crucible or custom game's map from the menu, before its session begins: the session's mod is
/// mounted (`Engine::InitializeMod`; `Game::LoadModMap`: the save's `modName` mounted before
/// `World::Load(mapPath)`), the map is opened, then the levels streamed behind the
/// menu are dropped with their skeleton joints and the opened map installed (`install_world`). A mod that does
/// not mount or a map it does not serve or that does not parse is an error that leaves the menu's world and
/// the install as they were.
/// PORT: the engine reports "Couldn't load world" after unloading the old world; here the map is opened
/// first.
pub fn open_session_map(
    world: &mut World,
    choice: &gd_game_core::session::Choice,
) -> anyhow::Result<()> {
    gd_game::session::set_mod_data(world, Some(choice.mod_name()))?;
    if !world.resource::<GameData>().vfs.exists(choice.map_path()) {
        gd_game::session::set_mod_data(world, None)?;
        anyhow::bail!(
            "{} is not in the mod {} or the game",
            choice.map_path(),
            choice.mod_name()
        );
    }
    let map = open_world_map(
        world.resource::<GameData>(),
        world.resource::<gd_game_core::hooks::Hooks>(),
        choice,
    );
    let map = match map {
        Ok(map) => map,
        Err(e) => {
            gd_game::session::set_mod_data(world, None)?;
            return Err(e.context(format!("open {}", choice.map_path())));
        }
    };
    world.insert_resource(choice.clone());
    drop_streamed_levels(world);
    world.run_system_cached_with(install_world, map)?;
    Ok(())
}

/// Despawns the levels streamed so far: their roots, with their trees and the skeleton joints of the models in
/// them (`model::Joints`).
fn drop_streamed_levels(world: &mut World) {
    let roots: Vec<Entity> = world
        .resource::<gd_game_core::levels::Levels>()
        .loaded
        .values()
        .copied()
        .collect();
    for e in roots {
        if let Ok(e) = world.get_entity_mut(e) {
            e.despawn();
        }
    }
}
