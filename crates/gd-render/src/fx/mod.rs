//! Particle effects (`.pfx`, `GAME::Effect`/`Emitter`/`EffectEntity`).
//!
//! Usage from other systems:
//! - [`spawn_effect_record`] for an `EffectEntity` record (reads `effectFile`,
//!   `localOrientFix`, `canBeSoft`), e.g. a skill's or projectile's fx,
//! - or insert an [`EffectSpec`] (`EffectSpec::pfx` for a raw `.pfx`) on any entity with a `Transform` (it
//!   follows the entity; world-space emitters leave trails, local ones move with it).
//!
//! Set `despawn_when_complete` for one-shot hit/cast effects; looping placed
//! effects run while their level is loaded. Effects farther than
//! [`SIM_RADIUS`] from the camera are paused and hidden, and a far looping
//! placed effect's init is deferred until the camera comes near.
//!
//! Each emitter is drawn by its own top-level mesh entity whose transform is
//! the engine's render frame (identity for world-space emitters, the effect's
//! coords for local ones), refilled every frame from the CPU simulation in
//! [`sim`] (storage buffer per emitter, see [`render`]). Lights (bools[6]) become point lights; with
//! bools[7] and not in simple mode one casts a point shadow of priority 1
//! ([`crate::render::point_shadow`]).
//!
//! PORT: a finished emitter's material + storage buffer return to a pool ([`FxAssets`]) and the next
//! emitter reuses them (the fields rewritten, the grown buffer kept) — no fresh GPU allocation per cast;
//! and during the loading cover [`preload_effects`] parses the player's skill effect pfx and every placed
//! effect's pfx + flipbook textures into the caches, so the first cast / first near effect does no file IO
//! on the render thread (the engine pre-loads through its resource manager).
//!
//! PORT: an effect whose files are still loading holds its pfx and the flipbooks already in, and a finished
//! background read is held for [`gd_game_core::weak_cache::FRESH_FRAMES`] (`FxAssets::fresh_*`), so a fight with more
//! than [`gd_game_core::weak_cache::RECENT`] flipbooks in play cannot evict an effect's early files before its last one
//! arrives (the engine's resource manager keeps a released resource 5 s instead).
pub mod enchant;
pub mod items;
pub mod mesh;
pub mod render;
pub mod sim;

use bevy::asset::RenderAssetUsages;
use bevy::asset::embedded_asset;
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::light::NotShadowCaster;
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::render::storage::ShaderBuffer;
use gd_formats::collections::HashMap;
use gd_formats::pfx::Pfx;
use gd_game_core::data::GameData;
use render::ParticleMaterial;
use sim::{Coords, EmitterState, fx_density};
use std::sync::Arc;

/// Effects beyond this distance from the camera's focus are not simulated.
pub const SIM_RADIUS: f32 = 90.0;

/// On a [`gd_game_core::visuals::PendingDecal`]: the decal of an effect spawned with
/// [`WithDecal`] (level-placed, mesh- or gate-attached), kept while that effect
/// lives (and by its own `life`).
#[derive(Component, Clone, Copy)]
pub struct DecalOwner(pub Entity);

/// A dying particle's ground decal (a `DecalType` record) to place at this
/// transform (`sim::EmitterState::dropped`): the blood splatters of the
/// death / gib blood clouds.
#[derive(Component)]
pub struct DropDecal(pub Arc<str>);

/// On a particle's [`DropDecal`]: its quad is tilted to the terrain normal
/// under it (`Emitter::UpdateParticles`: the coords' Y is the level
/// terrain's `GetNormalInterpolated` at the point, up without terrain, with
/// the particle's random yaw about it).
#[derive(Component)]
pub struct GroundTilt;

pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "particle.wesl");
        // Gameplay spawns bare effect requests (`gd_game::visuals`).
        app.register_required_components::<EffectSpec, Visibility>();
        // `GD_NO_FX`: leave effects out (A/B frame-time checks).
        if app
            .world()
            .resource::<gd_game_core::hooks::Hooks>()
            .render
            .no_fx
        {
            return;
        }
        app.add_plugins(MaterialPlugin::<ParticleMaterial>::default())
            .init_resource::<FxAssets>()
            .add_observer(cleanup_effect)
            .add_systems(
                Update,
                preload_effects
                    .run_if(resource_exists::<gd_game_core::session::SessionReady>)
                    .run_if(not(gd_game_core::session::in_game)),
            )
            .add_systems(
                PostUpdate,
                (collect_fx_loads, init_effects, update_effects)
                    .chain()
                    .in_set(FxSystems)
                    .after(TransformSystems::Propagate),
            )
            .add_plugins((
                items::ItemFxPlugin,
                enchant::EnchantPlugin,
                mesh::FxMeshPlugin,
            ));
        #[cfg(feature = "dev")]
        app.init_resource::<FxStats>();
    }
}

pub use gd_game_core::visuals::{EffectSource, EffectSpec, Restart, StopEmitting};

/// CPU time of the last [`update_effects`] (simulation + mesh building), for `GD_FPS` (`dev::fx`).
#[cfg(feature = "dev")]
#[derive(Resource, Default)]
pub struct FxStats {
    pub update_ms: f32,
}

/// The effect start-up and simulation systems (PostUpdate, after transform
/// propagation): systems that place effects by hand run before them.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FxSystems;

/// On an effect drawn by another view than the world camera (a paper doll):
/// its emitter meshes and lights take these render layers, and it runs
/// whatever its distance from the world camera's focus ([`SIM_RADIUS`]).
#[derive(Component, Clone)]
pub struct FxView(pub RenderLayers);

/// Spawns an `EffectEntity` record (e.g. `records/fx/.../x_fx01.dbr`) at
/// `transform`, with the record's ground decal ([`WithDecal`]).
pub fn spawn_effect_record(commands: &mut Commands, record: &str, transform: Transform) -> Entity {
    commands
        .spawn((
            EffectSpec::record(record),
            transform,
            Visibility::default(),
            WithDecal,
        ))
        .id()
}

/// On an effect entity: its record's `decal` is placed when the effect
/// starts and kept while it lives ([`DecalOwner`]). Set by
/// [`spawn_effect_record`] (level-placed, mesh-, gate- and shrine-attached
/// effects); gameplay effects (`gd_game_core::visuals::spawn_fx`) place their
/// own timed decal.
#[derive(Component)]
pub struct WithDecal;

/// A level-placed `EffectEntity` (collected by world streaming).
pub struct PlacedEffect {
    pub record: String,
    pub transform: Transform,
}

/// Spawns a level-placed effect (call with the level root as parent).
pub fn spawn_placed(commands: &mut Commands, p: &PlacedEffect) -> Entity {
    spawn_effect_record(commands, &p.record, p.transform)
}

/// Running effect.
#[derive(Component)]
pub struct Effect {
    pub pfx: Arc<Pfx>,
    pub emitters: Vec<EmitterRuntime>,
    /// `localOrientFix`: local emitters ignore the entity's rotation.
    pub local_orient_fix: bool,
    pub despawn_when_complete: bool,
}

pub struct EmitterRuntime {
    pub state: EmitterState,
    /// Mesh entity (None for emitters that are not drawn: no texture).
    pub draw: Option<Draw>,
    pub light: Option<Entity>,
    /// The emitter's `dropDecal` record path, normalized once (constant per
    /// emitter): a dying particle's [`DropDecal`] clones the Arc, no String.
    pub drop_decal: Option<Arc<str>>,
}

/// An emitter's mesh entity: a shared static quad mesh of `cap` particles and
/// the storage buffer that holds the particles ([`render`]).
pub struct Draw {
    pub entity: Entity,
    pub mesh: Handle<Mesh>,
    pub buffer: Handle<ShaderBuffer>,
    pub cap: usize,
}

/// How many finished emitters' material + buffer pairs the pool keeps (a busy fight draws tens at once;
/// beyond this the pairs drop and their assets free with the despawned draw entities).
const PARTICLE_POOL_CAP: usize = 128;
#[derive(Resource, Default)]
pub struct FxAssets {
    /// Parsed effect files and flipbook textures (with their frame count), held weakly: a running effect holds
    /// its `.pfx` and its emitters' materials the textures (`gd_game_core::weak_cache`, `textures::WeakHandles`); the
    /// last effect files used stay parsed (the cache's recent set), so a monster's now-and-then cast is not parsed
    /// again each time.
    pfx: gd_game_core::weak_cache::WeakCache<Pfx>,
    textures: crate::textures::WeakHandles<Image, u32>,
    /// Effect files and flipbook textures read and decoded in the background on their first use during play
    /// ([`FxAssets::pfx_ready`], [`collect_fx_loads`]): an effect waits for them instead of stalling the frame.
    /// While it waits it holds those already in ([`init_effects`]' `FxWaiting`), and a just-finished read is held
    /// by `fresh_pfx` / `fresh_textures` for [`gd_game_core::weak_cache::FRESH_FRAMES`] until its waiter takes it: the
    /// recent sets alone let a busy fight evict an effect's early files before its last one arrives.
    loading: Vec<bevy::tasks::Task<(String, FxLoad)>>,
    /// The just-finished background reads, held for their waiters (see `loading`).
    fresh_pfx: gd_game_core::weak_cache::Fresh<Arc<Pfx>>,
    fresh_textures: gd_game_core::weak_cache::Fresh<Handle<Image>>,
    /// The keys of `loading`, so a file is read once.
    in_flight: gd_formats::collections::HashSet<String>,
    /// Static quad meshes by particle capacity.
    quads: HashMap<usize, Handle<Mesh>>,
    /// Finished emitters' (material, buffer) pairs, ready for the next emitter ([`cleanup_effect`]).
    pool: Vec<(Handle<ParticleMaterial>, Handle<ShaderBuffer>)>,
    /// Scratch for [`render::fill_buffer`].
    scratch: Vec<[f32; 4]>,
}

/// A background read of [`FxAssets`] (`None`: the file failed to load).
enum FxLoad {
    Pfx(Option<Pfx>),
    Texture(Option<Box<(Image, u32)>>),
}

/// Whether an asset an effect needs is in its cache.
enum Ready<T> {
    Done(T),
    Loading,
    Failed,
}

impl FxAssets {
    /// Cached effect files and textures still in use (the memory audit).
    pub fn count(&self, images: &Assets<Image>) -> (usize, usize) {
        (self.pfx.live(), self.textures.live(images))
    }

    /// Drops the loaded effects and their textures (the mounted mod changed:
    /// `model::drop_on_mod_change`); the quad meshes don't depend on the data.
    pub fn clear(&mut self) {
        self.pfx.clear();
        self.textures.clear();
        self.fresh_pfx.clear();
        self.fresh_textures.clear();
        self.loading.clear();
        self.in_flight.clear();
        self.pool.clear();
    }

    /// Starts reading `key` in the background ([`collect_fx_loads`] files it), once.
    fn fetch(&mut self, data: &GameData, key: &str, texture: bool) {
        if !self.in_flight.insert(key.to_owned()) {
            return;
        }
        let (key, data) = (key.to_owned(), data.clone());
        self.loading
            .push(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
                let bytes = data.vfs.read(&key);
                let load = if texture {
                    FxLoad::Texture(
                        bytes
                            .and_then(gd_formats::tex::Tex::parse_owned)
                            .and_then(|t| render::flipbook_image(&t))
                            .inspect_err(|e| warn!("particle texture {key}: {e:#}"))
                            .ok()
                            .map(Box::new),
                    )
                } else {
                    FxLoad::Pfx(
                        bytes
                            .and_then(|b| Pfx::parse(&b))
                            .inspect_err(|e| warn!("pfx {key}: {e:#}"))
                            .ok(),
                    )
                };
                (key, load)
            }));
    }

    /// The effect file `path` if parsed; else its read starts in the background.
    fn pfx_ready(&mut self, data: &GameData, path: &str) -> Ready<Arc<Pfx>> {
        let key = gd_formats::vfs::normalized(path);
        match self.pfx.get_kept(&key) {
            Some(Some(p)) => Ready::Done(p),
            Some(None) => Ready::Failed,
            None => {
                self.fetch(data, &key, false);
                Ready::Loading
            }
        }
    }

    /// Whether the flipbook `path` is loaded (or failed), its handle pushed to `held`; else its read starts in the
    /// background.
    fn texture_ready_held(
        &mut self,
        data: &GameData,
        path: &str,
        images: &mut Assets<Image>,
        held: &mut Vec<Handle<Image>>,
    ) -> bool {
        let key = gd_formats::vfs::normalized(path);
        match self.textures.get(&key, images) {
            Some(Some((h, _))) => {
                held.push(h);
                true
            }
            Some(None) => true,
            None => {
                self.fetch(data, &key, true);
                false
            }
        }
    }

    pub fn pfx(&mut self, data: &GameData, path: &str) -> Option<Arc<Pfx>> {
        let key = gd_formats::vfs::normalized(path);
        self.pfx.get_or_load(&key, || {
            data.vfs
                .read(&key)
                .and_then(|b| Pfx::parse(&b))
                .inspect_err(|e| warn!("pfx {key}: {e:#}"))
                .ok()
        })
    }

    pub fn texture(
        &mut self,
        data: &GameData,
        path: &str,
        images: &mut Assets<Image>,
    ) -> Option<(Handle<Image>, u32)> {
        let key = gd_formats::vfs::normalized(path);
        if let Some(hit) = self.textures.get(&key, images) {
            return hit;
        }
        let r = data
            .vfs
            .read(&key)
            .and_then(gd_formats::tex::Tex::parse_owned)
            .and_then(|t| render::flipbook_image(&t));
        let t = match r {
            Ok((img, n)) => Some((images.add(img), n)),
            Err(e) => {
                warn!("particle texture {key}: {e:#}");
                None
            }
        };
        self.textures
            .insert(&key, t.as_ref().map(|(h, n)| (h, *n)), images);
        t
    }

    pub fn quads(&mut self, cap: usize, meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
        self.quads
            .entry(cap)
            .or_insert_with(|| meshes.add(render::quad_mesh(cap)))
            .clone()
    }
}

/// A preloaded effect file and its flipbook textures, held so the caches keep them ([`preload_effects`]): on a
/// placed effect entity until it goes, and the player's skill effects for the session.
#[derive(Component, Default)]
pub struct WarmFx {
    _pfx: Vec<Arc<Pfx>>,
    _textures: Vec<Handle<Image>>,
}

/// Parses `path`'s pfx and decodes its emitters' flipbook textures into the caches (no emitter builds), held by
/// `warm`.
fn warm_pfx(
    assets: &mut FxAssets,
    data: &GameData,
    images: &mut Assets<Image>,
    path: &str,
    warm: &mut WarmFx,
) {
    let Some(pfx) = assets.pfx(data, path) else {
        return;
    };
    for d in &pfx.emitters {
        if !d.texture.is_empty()
            && let Some((h, _)) = assets.texture(data, &d.texture, images)
        {
            warm._textures.push(h);
        }
    }
    warm._pfx.push(pfx);
}

/// An `EffectEntity` record's `effectFile` ([`warm_pfx`]); other records do nothing.
fn warm_record(
    assets: &mut FxAssets,
    data: &GameData,
    images: &mut Assets<Image>,
    record: &str,
    warm: &mut WarmFx,
) {
    if let Some(path) = data
        .db
        .get(record)
        .and_then(|r| r.str("effectFile").map(str::to_owned))
        .filter(|p| !p.is_empty())
    {
        warm_pfx(assets, data, images, &path, warm);
    }
}

/// During the loading cover: the skill effect records of the player's loadout (the same resolvers the cast
/// reads, `gd_game::skill_fx`) and every placed effect spec warm the pfx / texture caches once per session.
/// `init_effects` still defers a far placed effect's emitter builds; this only does the file IO early. The caches
/// hold weakly, so what is warmed is held ([`WarmFx`]): by the placed entity, and the skills' by `skills` for the
/// session.
// One system: the loadout's skill records and the placed specs share the caches.
#[allow(clippy::too_many_arguments)]
fn preload_effects(
    mut commands: Commands,
    data: Res<GameData>,
    mut assets: ResMut<FxAssets>,
    mut images: ResMut<Assets<Image>>,
    players: Query<(
        &gd_game_core::player::Loadout,
        Option<&gd_game_core::weapon_pool::SkillLinks>,
    )>,
    placed: Query<(Entity, &EffectSpec), (Without<Effect>, Without<WarmFx>)>,
    mut skills: Local<Option<WarmFx>>,
    mut fresh: gd_game_core::session::NewSession,
) {
    if fresh.begun() {
        *skills = None;
    }
    if skills.is_none() {
        let warm = skills.insert(WarmFx::default());
        for (loadout, links) in &players {
            let book = loadout.effective_skills(&data.db, &data.skill_trees());
            for (skill, _) in &book.skills {
                let Some(rec) = data.db.get(skill) else {
                    continue;
                };
                let li = book.effective_level(&data.db, skill).max(1) as usize - 1;
                let mods = gd_game_core::skill_fx::override_mods(
                    &data.db,
                    links,
                    Some(&loadout.skills),
                    skill,
                );
                let mut records: Vec<String> = Vec::new();
                let mut push = |r: Option<String>| {
                    if let Some(r) = r {
                        records.push(r);
                    }
                };
                push(gd_game_core::skill_fx::target_fx(rec, li, &mods));
                push(gd_game_core::skill_fx::particle_effect1(rec, &mods));
                push(gd_game_core::skill_fx::radius_effect(rec, li, &mods));
                push(gd_game_core::skill_fx::cone_effect(rec, li, &mods));
                push(gd_game_core::skill_fx::spin_aura(rec, li, &mods));
                push(gd_game_core::skill_fx::lightning(rec, &mods));
                for r in gd_game_core::skill_fx::beam_effects(rec, &mods) {
                    push(r);
                }
                for r in gd_game_core::skill_fx::projectile_effects(
                    &data.db,
                    links,
                    Some(&loadout.skills),
                    rec,
                    &mods,
                    None,
                ) {
                    push(Some(r));
                }
                push(gd_game_core::skill_fx::projectile_name(rec, li, &mods));
                for r in records {
                    warm_record(&mut assets, &data, &mut images, &r, warm);
                }
            }
        }
    }
    for (e, spec) in &placed {
        let mut warm = WarmFx::default();
        match &spec.source {
            EffectSource::Pfx(p) => warm_pfx(&mut assets, &data, &mut images, p, &mut warm),
            EffectSource::Record(r) => warm_record(&mut assets, &data, &mut images, r, &mut warm),
        }
        commands.entity(e).try_insert(warm);
    }
}

/// The effect's coords: the entity's frame without its scale, a mirror kept.
/// `Emitter::Update` / `EmitParticle` / `RenderPass`: the
/// emitter takes the entity's `WorldCoords` matrix as it is, so a frame that
/// mirrors (a weapon on the body's "R Hand" point, whose basis has det −1)
/// mirrors the particles' spawn points and velocities, and local particles are
/// drawn through it. Dropping the mirror swapped the dagger's red (+x) and
/// green (−x) flames.
fn coords_of(g: &GlobalTransform) -> Coords {
    let a = g.affine();
    let m = Mat3::from(a.matrix3);
    Coords {
        rot: Mat3::from_cols(
            m.x_axis.normalize_or(Vec3::X),
            m.y_axis.normalize_or(Vec3::Y),
            m.z_axis.normalize_or(Vec3::Z),
        ),
        pos: Vec3::from(a.translation),
    }
}

/// `rot` (orthonormal, possibly a mirror) as a `Transform`'s rotation and a
/// ±1 scale.
fn rot_transform(rot: Mat3) -> (Quat, Vec3) {
    let (s, r, _) = Affine3A::from_mat3(rot).to_scale_rotation_translation();
    (r, s.signum())
}

/// Files the finished background reads ([`FxAssets::fetch`]) into the caches, kept warm until the waiting effects
/// take them.
fn collect_fx_loads(mut assets: ResMut<FxAssets>, mut images: ResMut<Assets<Image>>) {
    use bevy::tasks::{block_on, futures_lite::future};
    assets.fresh_pfx.tick();
    assets.fresh_textures.tick();
    if assets.loading.is_empty() {
        return;
    }
    let assets = &mut *assets;
    let mut i = 0;
    while i < assets.loading.len() {
        let Some((key, load)) = block_on(future::poll_once(&mut assets.loading[i])) else {
            i += 1;
            continue;
        };
        drop(assets.loading.swap_remove(i));
        assets.in_flight.remove(&key);
        match load {
            FxLoad::Pfx(p) => {
                if let Some(p) = assets.pfx.put(&key, p) {
                    assets.fresh_pfx.hold(p);
                }
            }
            FxLoad::Texture(t) => {
                let t = t.map(|b| (images.add(b.0), b.1));
                assets
                    .textures
                    .insert(&key, t.as_ref().map(|(h, n)| (h, *n)), &images);
                if let Some((h, _)) = t {
                    assets.fresh_textures.hold(h);
                }
            }
        }
    }
}

/// What [`init_effects`] holds for an effect still waiting on its flipbooks: its pfx and the flipbooks already in,
/// so a busy fight's other effects don't evict them from the caches' recent sets before the last one arrives
/// (like `character::dress_bodies`' `Waiting`). Dropped when the effect starts or its entity goes.
#[derive(Default)]
struct FxWaiting {
    pfx: Option<Arc<Pfx>>,
    textures: Vec<Handle<Image>>,
}

#[allow(clippy::too_many_arguments)]
fn init_effects(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut commands: Commands,
    q: Query<
        (
            Entity,
            &EffectSpec,
            &Transform,
            &GlobalTransform,
            Has<WithDecal>,
            Option<&FxView>,
        ),
        Without<Effect>,
    >,
    data: Res<GameData>,
    mut assets: ResMut<FxAssets>,
    mut images: ResMut<Assets<Image>>,
    mut mats: ResMut<Assets<ParticleMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    cams: Query<&crate::camera::GameCamera>,
    mut held: Local<HashMap<Entity, FxWaiting>>,
    mut taken: Local<FxWaiting>,
) {
    held.retain(|e, _| q.contains(*e));
    let focus = cams.iter().next().map(|c| c.target);
    for (e, spec, tf, gt, with_decal, view) in &q {
        // Defer a far looping placed effect's init (pfx / texture / material
        // builds) until the camera comes near (`SIM_RADIUS`); one-shots and
        // other-view (paper doll) effects init at once.
        if !spec.despawn_when_complete
            && view.is_none()
            && focus.is_some_and(|f| f.distance(gt.translation()) >= SIM_RADIUS)
        {
            continue;
        }
        let (path, orient_fix) = match &spec.source {
            EffectSource::Pfx(p) => (Some(p.clone()), false),
            EffectSource::Record(r) => match data.db.get(r) {
                Some(rec) => (
                    rec.str("effectFile").map(str::to_owned),
                    rec.i32("localOrientFix").unwrap_or(0) != 0,
                ),
                None => (None, false),
            },
        };
        let pfx = match path.as_deref().filter(|p| !p.is_empty()) {
            None => Ready::Failed,
            Some(p) => assets.pfx_ready(&data, p),
        };
        let pfx = match pfx {
            Ready::Done(pfx) => pfx,
            // Read in the background: the effect starts once its file and flipbooks are in (a few frames late;
            // PORT: the engine's `ResourceLoader` loads in the background too).
            Ready::Loading => continue,
            Ready::Failed => {
                held.remove(&e);
                // A one-shot that never plays is complete: don't leave its bare entity behind.
                if spec.despawn_when_complete {
                    commands.entity(e).try_despawn();
                } else {
                    commands.entity(e).remove::<EffectSpec>();
                }
                continue;
            }
        };
        // This frame's holds go into `taken` and swap with last frame's, which are dropped only after: a flipbook
        // the caches already let go of stays alive in between.
        let mut ready = true;
        for d in pfx.emitters.iter().filter(|d| !d.texture.is_empty()) {
            ready &= assets.texture_ready_held(&data, &d.texture, &mut images, &mut taken.textures);
        }
        if !ready {
            let w = held.entry(e).or_default();
            w.pfx = Some(pfx);
            std::mem::swap(&mut w.textures, &mut taken.textures);
            taken.textures.clear();
            continue;
        }
        taken.textures.clear();
        debug!(
            "effect {:?} -> {:?} ({} emitters) at {} rot {}",
            spec.source,
            path,
            pfx.emitters.len(),
            tf.translation,
            gt.rotation()
        );
        // The record's ground decal ([`WithDecal`]).
        // Its frame is the effect's world coords (`decal.rs`).
        if let (EffectSource::Record(r), true) = (&spec.source, with_decal)
            && data.db.get(r).is_some_and(|rec| rec.str("decal").is_some())
        {
            let (_, rotation, translation) = gt.to_scale_rotation_translation();
            commands.spawn((
                gd_game_core::visuals::PendingDecal(r.clone()),
                Transform {
                    translation,
                    rotation,
                    ..default()
                },
                DecalOwner(e),
            ));
        }
        let mut emitters = Vec::with_capacity(pfx.emitters.len());
        for (order, d) in pfx.emitters.iter().enumerate() {
            let mut frames = 1;
            // `GD_FX_SKIP=<emitter name>` (debug): don't draw that emitter.
            let skip = hooks
                .render
                .fx_skip
                .as_ref()
                .is_some_and(|n| n.split(',').any(|n| n == d.name));
            let draw = if skip || d.texture.is_empty() {
                None
            } else if let Some((img, n)) = assets.texture(&data, &d.texture, &mut images) {
                frames = n;
                // One material (and buffer) per emitter: the particles live in the material's buffer.
                // A pooled pair (a finished emitter's) is rewritten in place; its buffer keeps its grown
                // size and its stale contents are never drawn (the entity hides until the first update).
                let cap = render::MIN_CAPACITY;
                let params = Vec4::new(
                    d.orient_vertically() as u32 as f32,
                    crate::render::distort::SCREEN_TEXTURE_SCALE,
                    d.shader.to_ascii_lowercase().contains("distortadditive") as u32 as f32,
                    0.0,
                );
                let (mat, buffer) = if let Some((mat, buffer)) = assets.pool.pop() {
                    if let Some(mut m) = mats.get_mut(&mat) {
                        m.params = params;
                        m.texture = img;
                        m.blend = d.blend();
                        m.sort = d.sort_order();
                        m.order = order as u32;
                    }
                    (mat, buffer)
                } else {
                    let mut buf =
                        ShaderBuffer::new(vec![[0.0f32; 4]], RenderAssetUsages::RENDER_WORLD);
                    buf.resize_buffer(render::buffer_bytes(cap));
                    let buffer = buffers.add(buf);
                    let mat = mats.add(ParticleMaterial {
                        params,
                        texture: img,
                        particles: buffer.clone(),
                        blend: d.blend(),
                        sort: d.sort_order(),
                        order: order as u32,
                    });
                    (mat, buffer)
                };
                let mesh = assets.quads(cap, &mut meshes);
                let me = commands
                    .spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(mat),
                        Transform::IDENTITY,
                        Visibility::Hidden,
                        NoFrustumCulling,
                        NotShadowCaster,
                    ))
                    .id();
                if let Some(v) = view {
                    commands.entity(me).insert(v.0.clone());
                }
                Some(Draw {
                    entity: me,
                    mesh,
                    buffer,
                    cap,
                })
            } else {
                None
            };
            let mut state = EmitterState::new(d, frames, fx_density());
            state.scale = spec.scale;
            state.loop_allowed = !spec.no_loop;
            let light = d.light().then(|| {
                commands
                    .spawn((
                        PointLight {
                            shadow_maps_enabled: false,
                            intensity: 0.0,
                            ..default()
                        },
                        Transform::IDENTITY,
                    ))
                    .id()
            });
            if let (Some(l), Some(v)) = (light, view) {
                commands.entity(l).insert(v.0.clone());
            }
            if let Some(l) = light {
                if d.light_simple() {
                    commands.entity(l).insert(crate::render::env::SimpleLight);
                } else if d.light_shadows() {
                    // `Emitter::UpdateLight` (priority = bools[7]) and
                    // `Light::AddToScene` (no shadow in simple mode, `Light.simpleMode`).
                    commands
                        .entity(l)
                        .insert(crate::render::point_shadow::ShadowPriority(1));
                }
            }
            let drop_decal = (!d.drop_decal.is_empty())
                .then(|| Arc::from(gd_formats::vfs::normalize(&d.drop_decal)));
            emitters.push(EmitterRuntime {
                state,
                draw,
                light,
                drop_decal,
            });
        }
        commands.entity(e).insert(Effect {
            pfx,
            emitters,
            local_orient_fix: orient_fix,
            despawn_when_complete: spec.despawn_when_complete,
        });
        // The effect and its materials hold its assets now.
        held.remove(&e);
    }
}

/// Despawn the emitter meshes/lights with their effect; the drawn emitters' material + buffer pairs return
/// to the pool for the next effect ([`init_effects`]).
fn cleanup_effect(
    ev: On<Remove<Effect>>,
    q: Query<&Effect>,
    mats: Query<&MeshMaterial3d<ParticleMaterial>>,
    mut assets: ResMut<FxAssets>,
    mut commands: Commands,
) {
    if let Ok(fx) = q.get(ev.entity) {
        for em in &fx.emitters {
            if let Some(dr) = &em.draw {
                if let Ok(mat) = mats.get(dr.entity)
                    && assets.pool.len() < PARTICLE_POOL_CAP
                {
                    assets.pool.push((mat.0.clone(), dr.buffer.clone()));
                }
                commands.entity(dr.entity).try_despawn();
            }
            if let Some(l) = em.light {
                commands.entity(l).try_despawn();
            }
        }
    }
}

/// Whether an effect is simulated this frame: shown and within [`SIM_RADIUS`] of the camera's `focus` (an effect
/// of another view runs at any distance).
pub fn simulated(
    pos: Vec3,
    inherited: Option<&InheritedVisibility>,
    other_view: bool,
    focus: Option<Vec3>,
) -> bool {
    inherited.is_none_or(|v| v.get())
        && (other_view || focus.is_none_or(|f| f.distance(pos) < SIM_RADIUS))
}

#[allow(clippy::too_many_arguments)]
fn update_effects(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut commands: Commands,
    time: Res<Time>,
    mut fx: Query<(
        Entity,
        &mut Effect,
        &GlobalTransform,
        Option<&InheritedVisibility>,
        Option<&Restart>,
        Has<StopEmitting>,
        Has<FxView>,
        Option<&gd_game_core::visuals::EffectAnchors>,
    )>,
    mut draws: Query<
        (&mut Transform, &mut GlobalTransform, &mut Visibility),
        (With<Mesh3d>, Without<Effect>),
    >,
    mut lights: Query<
        (&mut PointLight, &mut Transform, &mut GlobalTransform),
        (Without<Mesh3d>, Without<Effect>),
    >,
    cams: Query<&crate::camera::GameCamera>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut assets: ResMut<FxAssets>,
    #[cfg(feature = "dev")] mut stats: ResMut<FxStats>,
) {
    #[cfg(feature = "dev")]
    let t0 = std::time::Instant::now();
    let dt = time.delta_secs();
    let focus = cams.iter().next().map(|c| c.target);
    for (e, mut effect, g, inherited, restart, stop, other_view, anchored) in &mut fx {
        let coords = coords_of(g);
        let effect = &mut *effect;
        if stop {
            for em in &mut effect.emitters {
                em.state.stopped = true;
            }
        }
        if !simulated(coords.pos, inherited, other_view, focus) {
            if stop && effect.despawn_when_complete {
                commands.entity(e).try_despawn();
                continue;
            }
            // Write only on change: a `Mut` deref marks the draw / light changed, and every placed effect of a
            // loaded level passes here each frame.
            for em in &effect.emitters {
                if let Some(dr) = &em.draw
                    && let Ok((_, _, mut vis)) = draws.get_mut(dr.entity)
                {
                    vis.set_if_neq(Visibility::Hidden);
                }
                if let Some(l) = em.light
                    && let Ok((mut pl, _, _)) = lights.get_mut(l)
                    && pl.intensity != 0.0
                {
                    pl.intensity = 0.0;
                }
            }
            continue;
        }
        let mut complete = true;
        for (d, em) in effect.pfx.emitters.iter().zip(effect.emitters.iter_mut()) {
            // `EffectEntity::UpdateSelf`: a `SetAnchored` effect hands its
            // anchors to every emitter before the update.
            if let Some(a) = anchored {
                em.state.set_anchors(a.0, a.1);
            }
            em.state.update(d, coords, dt);
            // Dead particles' drop decals (sim space → world: local emitters
            // live in the effect's coords).
            for (p, yaw) in em.state.dropped.drain(..) {
                let Some(decal) = &em.drop_decal else {
                    continue;
                };
                let world = if d.local() {
                    if effect.local_orient_fix {
                        coords.pos + p
                    } else {
                        coords.rot * p + coords.pos
                    }
                } else {
                    p
                };
                let rot = Quat::from_rotation_y(yaw.to_radians());
                commands.spawn((
                    DropDecal(decal.clone()),
                    GroundTilt,
                    Transform::from_translation(world).with_rotation(rot),
                ));
            }
            complete &= em.state.is_complete();
            if let Some(dr) = &mut em.draw
                && let Ok((mut t, mut gt, mut vis)) = draws.get_mut(dr.entity)
            {
                // Render frame: local particles live in the effect's coords
                // (translation only with localOrientFix); world ones in world space.
                let frame = if d.local() {
                    if effect.local_orient_fix {
                        Transform::from_translation(coords.pos)
                    } else {
                        let (rotation, scale) = rot_transform(coords.rot);
                        Transform {
                            translation: coords.pos,
                            rotation,
                            scale,
                        }
                    }
                } else {
                    Transform::IDENTITY
                };
                let anchor = frame
                    .compute_affine()
                    .inverse()
                    .transform_point3(coords.pos);
                let n = em.state.particles.len();
                if n == 0 {
                    vis.set_if_neq(Visibility::Hidden);
                } else {
                    // Grow-only power-of-two capacity: a bigger quad mesh and buffer.
                    if n > dr.cap {
                        dr.cap = n.next_power_of_two();
                        dr.mesh = assets.quads(dr.cap, &mut meshes);
                        commands.entity(dr.entity).insert(Mesh3d(dr.mesh.clone()));
                    }
                    render::fill_buffer(
                        &em.state.particles,
                        d.velocity_aligned(),
                        anchor,
                        &mut assets.scratch,
                    );
                    if let Some(mut b) = buffers.get_mut(&dr.buffer) {
                        b.clear();
                        b.extend_from_slice(&assets.scratch);
                        b.resize_buffer(render::buffer_bytes(dr.cap));
                    }
                    // The draw entity sits at the effect origin (the sort centre of the
                    // static mesh); the particles are relative to it. Write only on change:
                    // a stationary placed effect flags nothing changed each frame.
                    let new = Transform {
                        translation: coords.pos,
                        rotation: frame.rotation,
                        scale: frame.scale,
                    };
                    t.set_if_neq(new);
                    gt.set_if_neq(GlobalTransform::from(new));
                    vis.set_if_neq(Visibility::Visible);
                }
            }
            if let Some(l) = em.light
                && let Ok((mut pl, mut t, mut gt)) = lights.get_mut(l)
                && let Some(ls) = em.state.light(d, coords)
            {
                pl.color = Color::linear_rgb(ls.color.x, ls.color.y, ls.color.z);
                pl.range = ls.radius.max(0.1);
                // Same scale as the level FireLights (world.rs); the deferred
                // light buffer reads intensity / (40 000 × range) as brightness.
                pl.intensity = 40_000.0 * ls.radius * ls.intensity;
                // `GD_NO_FX_LIGHTS=1` (debug): effect lights off.
                if hooks.render.no_fx_lights {
                    pl.intensity = 0.0;
                }
                *t = Transform::from_translation(ls.position);
                *gt = GlobalTransform::from(*t);
            }
        }
        if complete && effect.despawn_when_complete {
            commands.entity(e).try_despawn();
        } else if complete && let Some(r) = restart {
            commands.entity(e).remove::<Effect>().insert(r.0.clone());
        }
    }
    #[cfg(feature = "dev")]
    {
        stats.update_ms = t0.elapsed().as_secs_f32() * 1000.0;
    }
}

/// `EffectEntity` placements of a level (record class check, cached per record).
pub fn collect_placed(
    level: &gd_formats::map::Level,
    data: &GameData,
    origin: Vec3,
) -> Vec<PlacedEffect> {
    let mut is_fx: HashMap<u32, bool> = HashMap::default();
    let mut out = Vec::new();
    for e in &level.entities {
        let rec = level.record_of(e);
        if !*is_fx
            .entry(e.record)
            .or_insert_with(|| data.db.get(rec).is_some_and(|r| r.class == "EffectEntity"))
        {
            continue;
        }
        let r = e.rotation;
        // Rows of the entity Mat3 are the local axes.
        let basis = Mat3::from_cols(
            Vec3::new(r[0], r[1], r[2]),
            Vec3::new(r[3], r[4], r[5]),
            Vec3::new(r[6], r[7], r[8]),
        );
        let rot = Quat::from_mat3(&basis).normalize();
        out.push(PlacedEffect {
            record: rec.to_owned(),
            transform: Transform::from_translation(origin + Vec3::from(e.position))
                .with_rotation(rot),
        });
    }
    out
}

/// The objects a mesh creates at its attach points (MIF
/// `CreateEntity { attach, entity }`, e.g. the flame and the light of a torch
/// prop), as (record, record class, attach frame in mesh space). Built once
/// per render model from its parsed mesh and kept on it
/// ([`crate::model::ModelAsset::creations`]).
///
/// Engine.dll `Actor::CreateAttachmentsFromMesh`
/// (called once from `Actor::UpdateSelf` / `FastUpdate` when the actor's
/// mesh has loaded): every actor, whatever its class (decorations, doors,
/// chests, set-piece objects, breakables …), creates each
/// `GraphicsMesh::GetCreationData` entry's object and attaches it at the
/// named attach point (`Actor::Attach`). Static level scenery goes through
/// [`mesh_attached`] (effects) and [`mesh_lights`] (`FireLight` /
/// `DayNightLight`: the light of every candle, fire barrel and lantern),
/// placed once in the world: it never moves and its skeleton stays at the
/// bind pose, so the attached frames never change. Scenery that moves (an
/// animated `Decoration`: swinging lanterns, the arcane crystals; a bobbing
/// `Floater`), record models (`CharAssets::spawn_record_model`, set pieces)
/// go through [`spawn_mesh_attachments`] (both, children of the object),
/// equipment worn by characters (weapons, armour) through [`items`]. PORT:
/// only effects and lights are created.
///
/// The bind-pose mesh-space frame (`MeshCreation::bind`) composes a
/// bone-parented point with its bone's bind frame (`GraphicsMesh::
/// GetAttachedCoords`: bone abs × attach coords); [`spawn_mesh_attachments`]
/// makes such effects and lights follow the posed bone
/// ([`crate::model::BoneFx`], `Actor::GetAttachedCoordsInRegion`): the
/// Korvan rifts' vortex (`DoorFX` on `BN_Root`, which their open animation
/// raises 6.35 m to the ground), the flame of a swinging lantern post
/// (`Fire` on `BN_Lantern02`), the arcane crystal's glow and night light.
pub(crate) fn mesh_creations(data: &GameData, m: &gd_formats::msh::Mesh) -> Arc<[MeshCreation]> {
    let mut out = Vec::new();
    for (attach, entity) in m.create_entities() {
        let Some(class) = data.db.get(&entity).map(|r| r.class.clone()) else {
            continue;
        };
        let Some(ap) = m.attach_point(&attach) else {
            continue;
        };
        let c = &ap.coords;
        let basis = Mat3::from_cols(Vec3::from(c.x), Vec3::from(c.y), Vec3::from(c.z));
        let local = Affine3A::from_mat3_translation(basis, Vec3::from(c.origin));
        let bone = ap.parent_bone;
        let bind = bone.map_or(local, |i| {
            crate::model::coords_to_affine(&m.bones[i].abs) * local
        });
        out.push(MeshCreation {
            record: entity,
            class,
            basis,
            origin: Vec3::from(c.origin),
            parent: ap.parent.clone(),
            bone,
            bind,
        });
    }
    out.into()
}

/// One object a mesh creates ([`mesh_creations`]).
#[derive(Clone, Debug)]
pub struct MeshCreation {
    pub record: String,
    pub class: String,
    /// The attach frame as stored: relative to `parent` when there is one.
    pub basis: Mat3,
    pub origin: Vec3,
    pub parent: Option<String>,
    /// `parent`'s bone index.
    pub bone: Option<usize>,
    /// The attach frame in mesh space at the bind pose.
    pub bind: Affine3A,
}

/// The light attachments (`crate::render::lights::is_light_class` records: `FireLight`, `DayNightLight`, the altars'
/// `PulseLight`, Kurn's `NightLight` lanterns) of `model` ([`mesh_creations`]) as (record, attach point in
/// mesh space).
pub fn mesh_lights(model: &crate::model::ModelAsset) -> impl Iterator<Item = (&str, Vec3)> {
    model
        .creations
        .iter()
        .filter(|c| crate::render::lights::is_light_class(&c.class))
        .map(|c| (c.record.as_str(), Vec3::from(c.bind.translation)))
}

/// The `EffectEntity` and `FxMesh` attachments of `model` ([`mesh_creations`])
/// as (record, attach frame, parent bone, is an `FxMesh`): the frame is
/// relative to the bone when there is one, else in mesh space.
pub fn mesh_effect_points(
    model: &crate::model::ModelAsset,
) -> impl Iterator<Item = (&str, Affine3A, Option<&str>, bool)> {
    model
        .creations
        .iter()
        .filter(|c| c.class == "EffectEntity" || c.class == "FxMesh")
        .map(|c| {
            (
                c.record.as_str(),
                Affine3A::from_mat3_translation(c.basis, c.origin),
                c.parent.as_deref(),
                c.class == "FxMesh",
            )
        })
}

/// An attach frame as an effect's transform: its orientation, no scale
/// (scale is not applied to particles).
pub fn affine_transform(a: Affine3A) -> Transform {
    let m = Mat3::from(a.matrix3);
    let rot = Quat::from_mat3(&Mat3::from_cols(
        m.x_axis.normalize_or(Vec3::X),
        m.y_axis.normalize_or(Vec3::Y),
        m.z_axis.normalize_or(Vec3::Z),
    ))
    .normalize();
    Transform::from_translation(Vec3::from(a.translation)).with_rotation(rot)
}

/// The `EffectEntity` attachments of `model`, a static object placed at `xf`, placed in the world
/// ([`mesh_creations`]): the bind-pose frame through the placement, its orientation only (scale is not applied to
/// particles).
pub fn mesh_attached(
    model: &crate::model::ModelAsset,
    xf: Affine3A,
) -> impl Iterator<Item = PlacedEffect> + '_ {
    model
        .creations
        .iter()
        .filter(|c| c.class == "EffectEntity")
        .map(move |c| PlacedEffect {
            record: c.record.clone(),
            transform: affine_transform(xf * c.bind),
        })
}

/// Spawns the `EffectEntity` and light attachments of `model` (see [`mesh_creations`]) as
/// children of `root`, the object the mesh belongs to: they follow it and go
/// with it, a bone-parented one following its posed bone ([`crate::model::BoneFx`]).
pub fn spawn_mesh_attachments(
    commands: &mut Commands,
    data: &GameData,
    model: &crate::model::ModelAsset,
    root: Entity,
) {
    for c in model.creations.iter() {
        let e = if c.class == "EffectEntity" {
            spawn_effect_record(commands, &c.record, affine_transform(c.bind))
        } else if crate::render::lights::is_light_class(&c.class) {
            let Some(l) = data.db.get(&c.record).and_then(|rec| {
                crate::render::lights::light_from_record(rec, Vec3::from(c.bind.translation))
            }) else {
                continue;
            };
            crate::render::lights::spawn_light(commands, &l)
        } else {
            continue;
        };
        if let Some(bone) = c.bone {
            commands.entity(e).try_insert(crate::model::BoneFx {
                root,
                bone,
                local: Affine3A::from_mat3_translation(c.basis, c.origin),
            });
        }
        crate::model::try_add_child(commands, root, e);
    }
}
