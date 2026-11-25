//! The story's objects in the world (the host side of `gd_story::story`'s object list).
//!
//! - Streaming ([`stream_story_objects`]): the story objects of the levels within [`LOAD_RADIUS`] of
//!   [`StreamFocus`] are scanned off-thread ([`scan_level`]) and registered with the Lua host once the
//!   devotion state is in (`onAddToWorld` may ask `Shrine:IsRestored`); levels beyond [`UNLOAD_RADIUS`]
//!   remove theirs (`onRemoveFromWorld`, then `onDestroy`). A level streamed back in is restored from its
//!   saved state (`Region::UnloadLevel` → `Level::SaveState`, `Level::RestoreState`): the saved entities
//!   come back marked restored (Lua `IsReloaded`, `HostFacts::restored_objects`), destroyed ones do not. A
//!   script-created object (`Entity.Create`, `Character.Create`, `Proxy.Create`) is not saved
//!   (`Entity::Entity` clears `Entity.saveState`), so it leaves the story with its region.
//! - Spawning ([`sync_actors`]): an object present in the story gets its entity and [`StoryActor`]: doors
//!   and levers, chests, a proxy's monsters ([`StoryProxyChild`]), nav blockers, effects, breakables, plain
//!   entities and characters. Deaths run `onDie`, broken breakables `onBroken`, opened chests `onOpen` /
//!   `onOpenServer`; `Proxy:AllKilled` follows the proxy's monsters.
//! - Effects ([`world_effects`]): `Kill`, `TeleportPlayer`, the character commands (`UseSkillAction`,
//!   `MoveAction`, `TeleportHome`) and `DestroyItems`.
//! - Record hooks ([`hooked_characters`], [`record_hooks`]): every spawned character whose record has hooks
//!   becomes a story object whatever spawned it (`Character::Load` loads the hook pack from every
//!   character's record), as does a story proxy's monster when its proxy has `onSpawn`; `onCallbackDie`,
//!   `onCallbackSkill`, `onRestored` and the item hooks `onPickUp` / `onDrop`.
//!
//! Placed characters spawn at their map coordinates whatever the navmesh (`Character::Load` does no nav
//! query); [`actor_ground`] snaps them onto a walkable cell within 3 m when there is one, and the spawn waits
//! until the object's level has loaded.
//!
//! A script-placed `delayedRun` proxy spawns nothing until `Proxy:Run()` (`Story::is_present`); its
//! `AllKilled()` answers true meanwhile (`LuaProxy::AllKilled` of an empty spawned list).
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, futures_lite::future};
use gd_formats::collections::{HashMap, HashSet};
use gd_game_core::data::GameData;
use gd_game_core::levels::{
    LOAD_RADIUS, Levels, StreamFocus, UNLOAD_RADIUS, region_distance, region_origin,
};
use gd_game_core::story::{
    PLAYER_ID, StoryActor, StoryEffect, StoryProxyChild, StoryRuntime, StoryStage,
};
use gd_story::story::state::ObjClass;
use gd_story::story::{Coords, Effect};

/// The record fields that make a record a story object ([`is_story_record`]): the entity, character,
/// destructible and shrine hook packs, `onInteract`, `onEnter`, `onOpen`, and an NPC's `conversation`.
const STORY_HOOKS: [&str; 13] = [
    "onAddToWorld",
    "onRemoveFromWorld",
    "onDestroy",
    "onInteract",
    "onEnter",
    "onDie",
    "onOpen",
    "conversation",
    "onSpawn",
    "onBroken",
    "onRestored",
    "onCallbackDie",
    "onCallbackSkill",
];

pub struct StoryWorldPlugin;

impl Plugin for StoryWorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_non_send::<StoryWorld>()
            .init_resource::<SpawnedActors>()
            .add_systems(
                Update,
                (
                    stream_story_objects
                        .after(StoryStage::Sync)
                        .before(StoryStage::Tick)
                        .run_if(resource_exists::<gd_game_core::save::MapLayout>)
                        .run_if(resource_exists::<StreamFocus>),
                    (
                        sync_actors.before(StoryStage::Arm),
                        hooked_characters,
                        record_hooks,
                        world_effects.after(StoryStage::Arm),
                    )
                        .chain()
                        .after(StoryStage::Tick),
                    despawn_broken,
                ),
            );
    }
}

/// By story id.
#[derive(Resource, Default)]
pub struct SpawnedActors(pub HashMap<u32, Entity>);

/// The game's side of the story: streamed-in levels and their scans in flight, level objects' uids and
/// placement scales, the levels streamed out this session and the placed objects destroyed before that.
#[derive(Default)]
pub struct StoryWorld {
    regions: HashMap<usize, Vec<u32>>,
    pending: HashMap<usize, Task<Vec<LevelObject>>>,
    uids: HashMap<u32, [u32; 4]>,
    scales: HashMap<u32, f32>,
    saved: HashSet<usize>,
    destroyed: HashSet<u32>,
    focus: Option<Vec3>,
    /// Object ids of [`HookedChild`]ren removed since [`hooked_characters`] last ran.
    gone_children: Vec<u32>,
    /// `Destructible:Destroy`'s timed despawns: object id and seconds left ([`despawn_broken`]).
    broken_despawn: Vec<(u32, f32)>,
}

/// The story object of a spawned character [`hooked_characters`] bound hooks to; its removal takes the
/// object out of the story.
#[derive(Component)]
#[component(on_remove = queue_gone_child)]
pub struct HookedChild(pub u32);

fn queue_gone_child(
    mut world: bevy::ecs::world::DeferredWorld,
    ctx: bevy::ecs::lifecycle::HookContext,
) {
    let Some(id) = world.get::<HookedChild>(ctx.entity).map(|c| c.0) else {
        return;
    };
    if let Some(mut sw) = world.get_non_send_mut::<StoryWorld>() {
        sw.gone_children.push(id);
    }
}

/// Computed off-thread.
struct LevelObject {
    id: u32,
    record: String,
    coords: Coords,
    uid: Option<[u32; 4]>,
    /// `Actor::Read`.
    scale: f32,
}

/// Records the story layer cares about: anything with a Lua hook ([`STORY_HOOKS`]), triggers, NPCs with a
/// conversation, doors.
pub fn is_story_record(rec: &gd_formats::arz::Record) -> bool {
    STORY_HOOKS.iter().any(|h| rec.str(h).is_some())
        || matches!(rec.class.as_str(), "BoxTrigger" | "Door" | "ScriptEntity")
}

/// A record with a Lua hook ([`STORY_HOOKS`] but `conversation`).
fn has_hooks(rec: &gd_formats::arz::Record) -> bool {
    STORY_HOOKS
        .iter()
        .any(|h| *h != "conversation" && rec.str(h).is_some_and(|f| !f.is_empty()))
}

/// The names a `Monsters` / `AnyEntity` trigger detects: every value of the multi-value `detectableNames`
/// (`AreaTrigger::Load` hashes each whole); parsed like `gd_story::story::trigger`: ';' splits, backslashes
/// become slashes, case-insensitive.
fn detected_names(rec: &gd_formats::arz::Record) -> Vec<String> {
    rec.strs_iter("detectableNames")
        .flat_map(|s| s.split(';'))
        .filter(|s| !s.is_empty())
        .map(|s| s.replace('\\', "/").to_ascii_lowercase())
        .collect()
}

/// The story objects of region `index`'s level; a placed object's id is `region << 16 | entity index + 1`.
fn scan_level(map: &gd_formats::map::MapFile, data: &GameData, index: usize) -> Vec<LevelObject> {
    let Ok(level) = map.level(index) else {
        return Vec::new();
    };
    let origin = region_origin(&map.regions[index]);
    // The monsters a `Monsters` / `AnyEntity` area trigger detects by name are script objects too: the
    // trigger hands their object id to its hooks.
    let mut detected: HashSet<String> = HashSet::default();
    let mut seen: HashSet<u32> = HashSet::default();
    for e in &level.entities {
        if seen.insert(e.record)
            && let Some(r) = data.db.get(level.record_of(e))
            && r.class == "BoxTrigger"
            && matches!(r.str("detects"), Some("Monsters" | "AnyEntity"))
        {
            detected.extend(detected_names(r));
        }
    }
    let mut cache: HashMap<u32, bool> = HashMap::default();
    let mut out = Vec::new();
    for (i, e) in level.entities.iter().enumerate() {
        let keep = *cache.entry(e.record).or_insert_with(|| {
            data.db.get(level.record_of(e)).is_some_and(|r| {
                is_story_record(r)
                    || (r.class == "Monster"
                        && detected.contains(&level.record_of(e).to_ascii_lowercase()))
            })
        });
        if !keep {
            continue;
        }
        let m = e.rotation;
        let axes = [[m[0], m[1], m[2]], [m[3], m[4], m[5]], [m[6], m[7], m[8]]];
        let p = origin + Vec3::from(e.position);
        out.push(LevelObject {
            id: ((index as u32) << 16) | (i as u32 + 1),
            record: level.record_of(e).to_ascii_lowercase(),
            coords: Coords {
                origin: p.to_array(),
                axes,
            },
            uid: e.uid,
            scale: e.scale()[1],
        });
    }
    out
}

/// Streams the levels' story objects in and out around [`StreamFocus`]. The load / unload scan runs when the
/// focus moved (or the map changed); finished level scans are polled every frame.
pub fn stream_story_objects(
    rt: Option<NonSendMut<StoryRuntime>>,
    mut sw: NonSendMut<StoryWorld>,
    layout: Res<gd_game_core::save::MapLayout>,
    focus: Res<StreamFocus>,
    data: Res<GameData>,
    devotion: Option<Res<crate::devotion::DevotionState>>,
    mut unique: crate::unique::UniqueEntities,
) {
    let Some(mut rt) = rt else { return };
    let rt = &mut *rt;
    let sw = &mut *sw;
    let p = focus.0;
    if sw.focus != Some(p) || layout.is_changed() {
        sw.focus = Some(p);
        scan_regions(rt, sw, &layout, &data, p);
    }
    // `onAddToWorld` hooks may ask `Shrine:IsRestored`: the devotion state must be in first.
    if devotion.is_none() {
        return;
    }
    crate::story::publish_shrines(&mut rt.story.borrow_mut(), devotion.as_deref());
    let mut done: Vec<(usize, Vec<LevelObject>)> = Vec::new();
    sw.pending
        .retain(|&i, t| match block_on(future::poll_once(t)) {
            Some(objs) => {
                done.push((i, objs));
                false
            }
            None => true,
        });
    // Region order: the `onAddToWorld` hooks run in the same order every run.
    done.sort_unstable_by_key(|d| d.0);
    for (i, mut objs) in done {
        // A `ScriptEntity` its unique-entity group leaves out loads without its hooks (`crate::unique`).
        // PORT: it is not registered at all.
        objs.retain(|o| {
            !data
                .db
                .get(&o.record)
                .is_some_and(|r| r.class == "ScriptEntity")
                || unique.loads(o.uid, crate::unique::Loader::ScriptEntity)
        });
        let restored = sw.saved.contains(&i);
        if restored {
            objs.retain(|o| !sw.destroyed.contains(&o.id));
        }
        let ids: Vec<u32> = objs.iter().map(|o| o.id).collect();
        for o in objs {
            {
                let mut st = rt.story.borrow_mut();
                sw.scales.insert(o.id, o.scale);
                if let Some(u) = o.uid {
                    sw.uids.insert(o.id, u);
                    st.host_facts.object_uids.insert(o.id, u);
                }
                if restored {
                    st.host_facts.restored_objects.insert(o.id);
                }
            }
            rt.host.add_object(o.id, &o.record, o.coords);
        }
        debug!("story: region {i} registered {} objects", ids.len());
        sw.regions.insert(i, ids);
    }
}

fn scan_regions(
    rt: &StoryRuntime,
    sw: &mut StoryWorld,
    layout: &gd_game_core::save::MapLayout,
    data: &GameData,
    p: Vec3,
) {
    let regions = &layout.0.regions;
    for (i, r) in regions.iter().enumerate() {
        if r.level_size == 0
            || region_distance(r, p) >= LOAD_RADIUS
            || sw.regions.contains_key(&i)
            || sw.pending.contains_key(&i)
        {
            continue;
        }
        let (map, data) = (layout.0.clone(), data.clone());
        sw.pending.insert(
            i,
            AsyncComputeTaskPool::get().spawn(async move { scan_level(&map, &data, i) }),
        );
        debug!("story: scanning region {i}");
    }
    let far = |i: usize| {
        regions
            .get(i)
            .is_some_and(|r| region_distance(r, p) > UNLOAD_RADIUS)
    };
    let mut gone: Vec<usize> = sw.regions.keys().copied().filter(|&i| far(i)).collect();
    gone.sort_unstable();
    for &i in &gone {
        let Some(ids) = sw.regions.remove(&i) else {
            continue;
        };
        for id in ids {
            // Destroyed meanwhile (`Object:Destroy()`): not in the level's saved state.
            if rt.story.borrow().entities.get(id).is_none() {
                sw.destroyed.insert(id);
                continue;
            }
            rt.host.remove_object(id);
        }
        sw.saved.insert(i);
    }
    // Dropping the Task cancels a scan for a region that left the radius.
    sw.pending.retain(|&i, _| !far(i));
    // Script-placed objects in the regions that streamed out leave with them, unless also in a region still
    // streamed in or scanning.
    if gone.is_empty() {
        return;
    }
    let inside = |i: &usize, q: Vec3| {
        regions
            .get(*i)
            .is_some_and(|r| region_distance(r, q) == 0.0)
    };
    let left: Vec<u32> = rt
        .story
        .borrow()
        .entities
        .objects
        .values()
        .filter(|o| o.scripted && o.in_world && o.class != ObjClass::Player)
        .filter(|o| {
            let q = Vec3::from(o.coords.origin);
            gone.iter().any(|i| inside(i, q))
                && !sw
                    .regions
                    .keys()
                    .chain(sw.pending.keys())
                    .any(|i| inside(i, q))
        })
        .map(|o| o.id)
        .collect();
    for id in left {
        rt.host.remove_object(id);
    }
}

/// Where a placed story character stands: a walkable cell within 3 m of 2 m over the placed position, else
/// the placed position once the object's level has loaded (`None` until then). Script-created objects
/// (`Entities::SCRIPT_ID_BASE` and up) belong to no level and never wait.
pub fn actor_ground(
    nav: &gd_sim::nav::NavWorld,
    levels: &Levels,
    id: u32,
    origin: Vec3,
) -> Option<Vec3> {
    let mut p = origin;
    if let Some(cell) = nav.nearest((p + Vec3::Y * 2.0).to_array(), 3.0) {
        p.y = nav.pos(cell)[1];
        return Some(p);
    }
    if id < gd_story::story::state::Entities::SCRIPT_ID_BASE
        && !levels.is_loaded((id >> 16) as usize)
    {
        return None;
    }
    Some(p)
}

fn placed(c: &Coords) -> Transform {
    let basis = Mat3::from_cols(
        Vec3::from(c.axes[0]),
        Vec3::from(c.axes[1]),
        Vec3::from(c.axes[2]),
    );
    Transform::from_matrix(Mat4::from_mat3_translation(basis, Vec3::from(c.origin)))
}

/// Spawns the world's side of the story objects present in the world and takes away the gone ones;
/// scripted monsters' deaths run their `onDie` hooks.
#[allow(clippy::too_many_arguments)]
pub fn sync_actors(
    mut commands: Commands,
    (rt, sw): (Option<NonSendMut<StoryRuntime>>, NonSend<StoryWorld>),
    (mut spawned_actors, mut bosses): (ResMut<SpawnedActors>, ResMut<crate::monsters::HeroBosses>),
    actors: Query<(Entity, &StoryActor, Option<&gd_game_core::world::Breakable>)>,
    died: Query<
        &StoryActor,
        (
            Added<gd_game_core::combat::Dead>,
            Without<gd_game_core::world::Breakable>,
        ),
    >,
    proxy_children: Query<(Entity, &StoryProxyChild)>,
    live_children: Query<
        &StoryProxyChild,
        (
            With<gd_game_core::monsters::Monster>,
            Without<gd_game_core::combat::Dead>,
        ),
    >,
    (chest_states, mut broken): (
        Res<gd_game_core::world::ChestStates>,
        ResMut<gd_game_core::world::Broken>,
    ),
    mut chests_opened: MessageReader<crate::chests::ChestOpened>,
    (session, factions): (
        crate::session::Session,
        // Option: factions are optional input (the friendly check).
        Option<Res<crate::factions::PlayerFactions>>,
    ),
    levels: Res<Levels>,
    // Option: only proxies and destructibles need the rules (they wait for them).
    rules: Option<Res<gd_game_core::rules::CombatRules>>,
    (progress, patrol_groups): (
        // Option: the player's level is optional input (level 1).
        Option<Res<gd_game_core::progress::PlayerProgress>>,
        Res<crate::patrol::PatrolGroups>,
    ),
    (nav, mut anims): (
        Res<crate::nav::Nav>,
        ResMut<gd_game_core::character::AnimCache>,
    ),
    mut spawner: crate::creature::Spawner,
    (mut clips, mut new): (
        Local<crate::creature::ClipLists>,
        gd_game_core::session::NewSession,
    ),
) {
    if new.begun() {
        clips.clear();
    }
    let spawned = &mut spawned_actors.0;

    let Some(rt) = rt else { return };
    // A breakable has no `onDie` (`onBroken`, [`record_hooks`]).
    for a in &died {
        rt.host.on_died(a.0);
    }
    for o in chests_opened.read() {
        rt.host.object_hook(o.0, "onOpen");
        rt.host.object_hook(o.0, "onOpenServer");
    }
    let want: Vec<(u32, String, Coords, ObjClass)> = {
        let st = rt.story.borrow();
        // Whether object `id` keeps its entity: present and spawnable, or a dead story creature's corpse while
        // its object is in the world.
        let keeps = |id: u32, in_spawned: bool| {
            st.entities.get(id).is_some_and(|o| {
                (spawnable(o.class) && st.is_present(o)) || (in_spawned && o.in_world && !o.alive)
            })
        };
        // Despawned in `Last`: this frame's other systems may still command them.
        for (e, a, b) in &actors {
            if keeps(a.0, spawned.contains_key(&a.0)) {
                continue;
            }
            commands.entity(e).try_insert(gd_game_core::levels::Unload);
            spawned.remove(&a.0);
            // A breakable a script broke (`Destructible:Destroy`) stays broken.
            if let Some(b) = b
                && st.entities.get(a.0).is_some_and(|o| !o.alive)
            {
                broken.0.insert(b.key);
            }
        }
        for (e, c) in &proxy_children {
            if !keeps(c.0, spawned.contains_key(&c.0)) {
                commands.entity(e).try_insert(gd_game_core::levels::Unload);
                spawned.remove(&c.0);
            }
        }
        // Objects that left the story come back as new ones.
        spawned.retain(|&id, _| keeps(id, true));
        st.entities
            .objects
            .values()
            .filter(|o| spawnable(o.class) && st.is_present(o) && !spawned.contains_key(&o.id))
            .map(|o| (o.id, o.record.clone(), o.coords, o.class))
            .collect()
    };
    // Proxies whose monsters spawned this frame (not in the queries yet).
    let mut fresh: HashSet<u32> = HashSet::default();
    let deadline = std::time::Instant::now() + crate::creature::CLIPS_BUDGET;
    for (id, record, c, class) in want {
        if spawned.contains_key(&id) {
            continue;
        }
        // PORT: an object waits until the meshes and clips it reads are parsed in the background.
        if !object_ready(
            &mut spawner,
            &mut anims,
            &mut clips,
            &record,
            class,
            id,
            deadline,
        ) {
            continue;
        }
        // `FixedLever`: a door-like model whose use runs its `onInteract` hook.
        if matches!(class, ObjClass::Door | ObjClass::Lever) {
            let e = crate::doors::spawn_door(
                &mut commands,
                &spawner.data,
                &mut spawner.bodies,
                &mut anims,
                placed(&c),
                &record,
                sw.uids.get(&id).copied(),
                None,
                Some(id),
            )
            .unwrap_or(Entity::PLACEHOLDER);
            debug!("story door/lever {id} {record} -> {e:?}");
            if e != Entity::PLACEHOLDER {
                commands.entity(e).insert(StoryActor(id));
            }
            spawned.insert(id, e);
            continue;
        }
        if class == ObjClass::Chest {
            let key = gd_game_core::world::ChestKey::Story(id);
            let opened = chest_states.opened.contains(&key);
            let e = crate::chests::spawn_chest(
                &mut commands,
                &spawner.data.db,
                placed(&c),
                &record,
                key,
                0,
                opened,
            )
            .unwrap_or(Entity::PLACEHOLDER);
            if e != Entity::PLACEHOLDER {
                commands.entity(e).insert(StoryActor(id));
            }
            spawned.insert(id, e);
            continue;
        }
        // A proxy's monsters spawn once while it is in the world, once its level's nav data is in.
        if class == ObjClass::Proxy {
            let Some(rules) = &rules else { continue };
            if nav
                .0
                .nearest((Vec3::from(c.origin) + Vec3::Y * 2.0).to_array(), 3.0)
                .is_none()
            {
                continue;
            }
            let level = progress.as_ref().map_or(1, |p| p.progress.level);
            let es = crate::monsters::spawn_story_proxy(
                &mut commands,
                &spawner.data,
                &mut spawner.bodies,
                &nav,
                rules,
                Some(&*levels),
                &record,
                Vec3::from(c.origin),
                level,
                &session,
                factions.as_deref(),
                id,
                &patrol_groups
                    .groups
                    .for_proxy(gd_sim::patrol::story_proxy_uid(id)),
                {
                    let st = rt.story.borrow();
                    st.entities.proxy_limits.get(&id).map(|v| Vec3::from(*v))
                },
                {
                    let st = rt.story.borrow();
                    st.entities.proxy_ignore_boss.contains(&id)
                },
                &mut bosses,
            );
            for &e in &es {
                commands.entity(e).insert(StoryProxyChild(id));
            }
            spawned.insert(id, es.first().copied().unwrap_or(Entity::PLACEHOLDER));
            fresh.insert(id);
            continue;
        }
        let rec = spawner.data.db.get(&record);
        if let Some(r) = rec.as_ref().filter(|r| r.class == "NavBlocker") {
            let p = Vec3::from(c.origin);
            let e = commands
                .spawn((
                    Transform::from_translation(p),
                    crate::nav::NavObstacle(vec![crate::obstacles::nav_blocker(r, p)]),
                    StoryActor(id),
                ))
                .id();
            spawned.insert(id, e);
            continue;
        }
        // A script's `FxPak` / `EffectEntity`: plays where it is added (`FxPak::OnAddToLevel`) and removes
        // itself once complete; the object only stands for it.
        let fx_record = rec
            .as_ref()
            .is_some_and(|r| matches!(r.class.as_str(), "FxPak" | "EffectEntity"));
        if class == ObjClass::Entity && fx_record {
            info!("story: effect {record} at {:?}", c.origin);
            gd_game_core::visuals::spawn_fx(&mut commands, &spawner.data, &record, placed(&c));
            spawned.insert(id, Entity::PLACEHOLDER);
            continue;
        }
        // A `Destructible` is a breakable keyed by its level and entity index (`Broken`); a script-created
        // one keys on its object id.
        if class == ObjClass::Destructible {
            let Some(rules) = &rules else { continue };
            let key = if id < gd_story::story::state::Entities::SCRIPT_ID_BASE {
                ((id >> 16) as usize, (id & 0xFFFF).saturating_sub(1))
            } else {
                (usize::MAX, id)
            };
            let level = progress.as_ref().map_or(1, |p| p.progress.level);
            let e = crate::breakables::spawn_breakable(
                &mut commands,
                &spawner.data,
                &mut spawner.bodies,
                rules,
                &broken,
                placed(&c),
                &record,
                key,
                level,
            )
            .unwrap_or(Entity::PLACEHOLDER);
            if e != Entity::PLACEHOLDER {
                commands
                    .entity(e)
                    .insert(StoryActor(id))
                    .remove::<gd_game_core::levels::LevelMember>();
            }
            spawned.insert(id, e);
            continue;
        }
        // Static story objects: their record's mesh, placed with the full axes. An `Npc*` record is a
        // character whatever Lua class created it.
        let npc_record = rec.as_ref().is_some_and(|r| r.class.starts_with("Npc"));
        if class == ObjClass::Entity && !npc_record {
            let mesh = rec.as_ref().and_then(|r| r.str("mesh").map(str::to_owned));
            let Some((mesh, msh)) =
                mesh.and_then(|m| Some((m.clone(), spawner.bodies.get(&spawner.data, &m)?)))
            else {
                spawned.insert(id, Entity::PLACEHOLDER);
                continue;
            };
            let tf = placed(&c);
            let e = commands
                .spawn((
                    tf,
                    StoryActor(id),
                    Name::new(format!("story object {id}")),
                    gd_game_core::visuals::DecorationRequest {
                        record: record.clone(),
                        mesh,
                    },
                ))
                .id();
            // `Entity::SetDynamicObstacle` (`useBoundingBoxesForDynamicObstacles`) and every `FixedItem*`
            // (`FixedItem::UpdateSelf` → `CreatePathObstacle`) block with their mesh hit boxes; doors and
            // destructibles are handled elsewhere, other entities are scenery (`Actor::OnAddToLevel`).
            let hit_boxes = rec.as_ref().is_some_and(|r| {
                r.bool("useBoundingBoxesForDynamicObstacles")
                    || (r.class.starts_with("FixedItem") && r.class != "FixedItemDoor")
            });
            let o = if hit_boxes {
                crate::obstacles::hit_box_obstacles(&msh, &tf)
            } else {
                Vec::new()
            };
            debug!("story obstacle {record}: {} shapes", o.len());
            if !o.is_empty() {
                commands.entity(e).insert(crate::nav::NavObstacle(o));
            }
            spawned.insert(id, e);
            continue;
        }
        let x = Vec3::from(c.axes[0]);
        let z = Vec3::from(c.axes[2]);
        let rot = Quat::from_mat3(&Mat3::from_cols(
            x.normalize_or(Vec3::X),
            Vec3::Y,
            z.normalize_or(Vec3::Z),
        ));
        // Off-nav characters spawn at their placed coordinates once the level's nav data is in.
        let Some(p) = actor_ground(&nav.0, &levels, id, Vec3::from(c.origin)) else {
            continue;
        };
        let tf = Transform::from_translation(p).with_rotation(rot);
        // A placed character at its placement's scale; a script-created one at its record's.
        let placed = sw.scales.get(&id).copied();
        let Some(e) =
            crate::creature::spawn_placed(&mut commands, &mut spawner, tf, &record, id, placed)
        else {
            // Failures too, so they are not retried every frame.
            spawned.insert(id, Entity::PLACEHOLDER);
            continue;
        };
        commands
            .entity(e)
            .insert((StoryActor(id), Name::new(format!("story {id}"))));
        debug!("story actor {id} {record} at {:?} -> y {}", c.origin, p.y);
        spawned.insert(id, e);
    }
    // `LuaProxy::AllKilled`: false while a Monster the proxy spawned lives; a run proxy stays active until
    // its monsters have spawned and all died.
    let mut st = rt.story.borrow_mut();
    if st.host_facts.proxies_active.is_empty() {
        return;
    }
    let live: HashSet<u32> = live_children.iter().map(|c| c.0).collect();
    let st = &mut *st;
    let entities = &st.entities;
    st.host_facts.proxies_active.retain(|id| {
        let in_world = entities.get(*id).is_some_and(|o| o.in_world);
        in_world && !(spawned.contains_key(id) && !fresh.contains(id) && !live.contains(id))
    });
}

fn spawnable(class: ObjClass) -> bool {
    matches!(
        class,
        ObjClass::Npc
            | ObjClass::Character
            | ObjClass::Entity
            | ObjClass::Destructible
            | ObjClass::Proxy
            | ObjClass::Chest
            | ObjClass::Door
            | ObjClass::Lever
    )
}

/// Whether everything [`sync_actors`] reads to spawn story object `id` is parsed; one not yet parsed starts
/// its background load.
fn object_ready(
    s: &mut crate::creature::Spawner,
    anims: &mut gd_game_core::character::AnimCache,
    clips: &mut crate::creature::ClipLists,
    record: &str,
    class: ObjClass,
    id: u32,
    deadline: std::time::Instant,
) -> bool {
    let crate::creature::Spawner { data, bodies } = s;
    let Some(rec) = data.db.get(record) else {
        return true;
    };
    let mesh_ready = |bodies: &mut gd_game_core::creature::BodyCache| {
        rec.str("mesh").is_none_or(|m| bodies.ready(data, m))
    };
    match class {
        ObjClass::Chest | ObjClass::Proxy => true,
        ObjClass::Door | ObjClass::Lever => crate::doors::door_ready(data, bodies, anims, record),
        ObjClass::Destructible => mesh_ready(bodies),
        ObjClass::Entity if !rec.class.starts_with("Npc") => mesh_ready(bodies),
        _ => crate::creature::ready_to_spawn(s, anims, clips, record, id, deadline),
    }
}

/// A spawned character whose record has hooks becomes a story object whatever spawned it (`Character::Load`
/// loads the hook pack from every character's record), as does a story proxy's monster when its proxy has
/// `onSpawn`. It gets a script object id and [`StoryActor`], runs its `onAddToWorld`, then, for a story
/// proxy's monster, the proxy's `onSpawn` with that id (`Proxy::PlaceObjects`). When the character goes
/// ([`HookedChild`]), its object leaves the story: `onRemoveFromWorld`, `onDestroy`.
#[allow(clippy::type_complexity)]
pub fn hooked_characters(
    mut commands: Commands,
    rt: Option<NonSend<StoryRuntime>>,
    mut sw: NonSendMut<StoryWorld>,
    mut spawned: ResMut<SpawnedActors>,
    data: Res<GameData>,
    new: Query<
        (
            Entity,
            &gd_game_core::character::Character,
            Option<&StoryProxyChild>,
            &Transform,
        ),
        (
            Added<gd_game_core::character::Character>,
            Without<StoryActor>,
        ),
    >,
) {
    let Some(rt) = rt else { return };
    for id in std::mem::take(&mut sw.gone_children) {
        spawned.0.remove(&id);
        rt.host.remove_object(id);
    }
    for (e, c, proxy, tf) in &new {
        if c.is_player {
            continue;
        }
        let proxy = proxy.map(|p| p.0);
        let on_spawn = proxy.is_some_and(|proxy| {
            let proxy_record = rt
                .story
                .borrow()
                .entities
                .get(proxy)
                .map(|o| o.record.clone());
            proxy_record
                .and_then(|r| data.db.get(&r))
                .is_some_and(|r| r.str("onSpawn").is_some_and(|f| !f.is_empty()))
        });
        // A story proxy's monster with a conversation is a story object too.
        let story = |r: &gd_formats::arz::Record| {
            if proxy.is_some() {
                is_story_record(r)
            } else {
                has_hooks(r)
            }
        };
        if !on_spawn && !data.db.get(&c.record).is_some_and(story) {
            continue;
        }
        let id = rt
            .story
            .borrow_mut()
            .entities
            .create(&c.record, ObjClass::Character);
        commands.entity(e).insert((StoryActor(id), HookedChild(id)));
        spawned.0.insert(id, e);
        rt.host
            .add_object(id, &c.record, Coords::at(tf.translation.to_array()));
        if let Some(proxy) = proxy.filter(|_| on_spawn) {
            rt.host.object_hook_arg(proxy, "onSpawn", id);
        }
    }
}

/// The record hooks fired by breaks, animations, shrines and items:
/// - `onBroken`: a story breakable broke (`Destructible::TakeAttack` after `BreakApart`).
/// - `onCallbackDie`: the death animation's `OnDie` callback of a `registerCallbackToDie` record
///   (`DefaultDeathHandler::AnimationCallback` → `Character::OnCallbackDie`; after `onDie`).
/// - `onCallbackSkill`: a `SkillScript` animation callback (`Character::OnCallbackSkillScript`). PORT: the
///   engine routes it through the animation's skill; the port fires it for any animation's callback.
/// - `onRestored`: a devotion shrine restored this session (`StaticShrine::AnimationCallback`, state 5 → 6).
///   PORT: fired when `crate::devotion` marks it restored, not at the end of the restoring animation.
/// - `onPickUp` / `onDrop` (looter id, item id): `Inventory::AddItemToInventory` /
///   `RemoveItemFromInventory`. PORT: fired only for a picked-up ground item and an item the player
///   dropped on the ground; the item id is 0 (no object ids).
#[allow(clippy::too_many_arguments)]
pub fn record_hooks(
    rt: Option<NonSend<StoryRuntime>>,
    data: Res<GameData>,
    actors: Query<&StoryActor>,
    mut anims: MessageReader<gd_game_core::character::AnimCallback>,
    mut picked: MessageReader<crate::loot::ItemPickedUp>,
    mut broke: MessageReader<crate::breakables::Broke>,
    // A restored item was not dropped now.
    dropped: Query<
        &crate::loot::GroundItem,
        (
            Added<crate::item_drop::DroppedByPlayer>,
            Without<crate::loot::Restored>,
        ),
    >,
    // Option: no devotion state, no shrine to restore.
    devotion: Option<Res<crate::devotion::DevotionState>>,
    (mut restored_seen, mut fresh): (Local<Option<usize>>, gd_game_core::session::NewSession),
) {
    if fresh.begun() {
        *restored_seen = None;
    }
    let Some(rt) = rt else {
        anims.clear();
        picked.clear();
        broke.clear();
        return;
    };
    for id in broke.read().filter_map(|b| b.story) {
        rt.host.object_hook(id, "onBroken");
    }
    for ev in anims.read() {
        let Ok(&StoryActor(id)) = actors.get(ev.entity) else {
            continue;
        };
        let hook =
            if ev.anim == gd_sim::anim::AnimType::Die && ev.name.eq_ignore_ascii_case("OnDie") {
                let record = rt.story.borrow().entities.get(id).map(|o| o.record.clone());
                if !record
                    .and_then(|r| data.db.get(&r))
                    .is_some_and(|r| r.bool("registerCallbackToDie"))
                {
                    continue;
                }
                "onCallbackDie"
            } else if ev.name.eq_ignore_ascii_case("SkillScript") {
                "onCallbackSkill"
            } else {
                continue;
            };
        rt.host.object_hook(id, hook);
    }
    if let Some(d) = devotion.as_deref() {
        let n = d.restored.len();
        let from = restored_seen.replace(n).unwrap_or(n).min(n);
        for uid in &d.restored[from..] {
            let id = rt
                .story
                .borrow()
                .host_facts
                .object_uids
                .iter()
                .find(|(_, u)| *u == uid)
                .map(|(id, _)| *id);
            if let Some(id) = id {
                rt.host.object_hook(id, "onRestored");
            }
        }
    }
    let player = rt.story.borrow().player.id;
    let item_hook = |record: &str, hook: &str| {
        let f = data
            .db
            .get(record)
            .and_then(|r| r.str(hook).filter(|f| !f.is_empty()).map(str::to_owned));
        if let Some(f) = f {
            rt.host.call(&f, (player, 0u32));
            rt.host.run_queued();
        }
    };
    for p in picked.read() {
        item_hook(&p.item.base_name, "onPickUp");
    }
    for g in &dropped {
        if let Some(item) = &g.item {
            item_hook(&item.base_name, "onDrop");
        }
    }
}

type PlayerMove<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut Transform,
        &'static mut crate::movement::MovePath,
    ),
    (With<gd_game_core::player::Player>, Without<StoryActor>),
>;

/// Story effects on the world: `Kill` (counts as a normal death), `TeleportPlayer`, the character commands
/// and `DestroyItems`.
#[allow(clippy::too_many_arguments)]
pub fn world_effects(
    mut commands: Commands,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    mut events: MessageReader<StoryEffect>,
    mut actors: Query<
        (
            Entity,
            &StoryActor,
            &Transform,
            &mut gd_game_core::character::Character,
            &mut gd_game_core::combat::Combatant,
            Option<&gd_game_core::monsters::Monster>,
        ),
        (
            Without<gd_game_core::combat::Dead>,
            Without<gd_game_core::player::Player>,
        ),
    >,
    mut player: PlayerMove,
    mut ais: Query<&mut crate::combat::MonsterAi>,
    positions: Query<
        (Entity, &StoryActor, &Transform),
        (
            Without<gd_game_core::player::Player>,
            Without<gd_game_core::combat::Dead>,
        ),
    >,
    items: Query<(Entity, &GlobalTransform, &crate::loot::GroundItem)>,
    rt: Option<NonSend<StoryRuntime>>,
    nav: Res<crate::nav::Nav>,
    mut states: ResMut<crate::monsters::ProxyStates>,
    mut killed: MessageWriter<crate::combat::MonsterKilled>,
    chars: Query<(), With<gd_game_core::character::Character>>,
    (data, factions): (
        Res<GameData>,
        // Option: factions are optional input (the player's pack, `monsters::player_pack`).
        Option<Res<crate::factions::PlayerFactions>>,
    ),
    (mut broken, mut broke, mut spawned_actors): (
        ResMut<gd_game_core::world::Broken>,
        MessageWriter<crate::breakables::Broke>,
        ResMut<SpawnedActors>,
    ),
    (mut sw, breakables): (
        NonSendMut<StoryWorld>,
        Query<
            (
                Entity,
                &StoryActor,
                &gd_game_core::world::Breakable,
                &Transform,
            ),
            Without<gd_game_core::combat::Dead>,
        >,
    ),
) {
    // The player or a story object (entity, position).
    let target_of = |id: u32, player: &PlayerMove| -> Option<(Entity, Vec3)> {
        if id == PLAYER_ID {
            return player.single().ok().map(|(e, tf, _)| (e, tf.translation));
        }
        positions
            .iter()
            .find(|(_, a, _)| a.0 == id)
            .map(|(e, _, tf)| (e, tf.translation))
    };
    for ev in events.read() {
        match &ev.0 {
            Effect::Kill { id } => {
                for (e, a, tf, mut ch, mut cb, monster) in &mut actors {
                    if a.0 == *id {
                        cb.life = 0.0;
                        crate::combat::kill(
                            &mut commands,
                            e,
                            tf.translation,
                            Some(&mut ch),
                            monster,
                            false,
                            &mut states,
                            &mut killed,
                            None,
                        );
                    }
                }
            }
            Effect::TeleportPlayer { coords } => {
                if let Ok((_, mut tf, mut path)) = player.single_mut() {
                    tf.translation = Vec3::from(coords.origin);
                    path.points.clear();
                    info!("story: teleported to {:?}", coords.origin);
                }
            }
            // `Character::GetSpawnPoint`: the placement (or the last `MoveAction`'s end); anger is forgotten.
            Effect::TeleportHome { id } => {
                for (e, a, _, mut ch, mut cb, _) in &mut actors {
                    if a.0 != *id {
                        continue;
                    }
                    let Ok(mut ai) = ais.get_mut(e) else { continue };
                    let home = ai.home;
                    ai.go_idle();
                    cb.target = None;
                    ch.state = gd_game_core::character::CharState::Idle;
                    commands
                        .entity(e)
                        .remove::<(crate::combat::QuestMove, gd_game_core::casting::Casting)>();
                    commands.queue(move |w: &mut World| {
                        if let Some(mut tf) = w.get_mut::<Transform>(e) {
                            tf.translation = home;
                        }
                    });
                }
            }
            // The target object's position when it exists, else the given coordinates; no path, no move.
            Effect::MoveAction {
                id,
                target,
                coords,
                interruptible,
            } => {
                let dest = target_of(*target, &player).map_or(Vec3::from(coords.origin), |t| t.1);
                for (e, a, tf, _, _, _) in &actors {
                    if a.0 != *id {
                        continue;
                    }
                    if let Some(p) = nav.0.find_path(
                        &mut scratch,
                        tf.translation.to_array(),
                        dest.to_array(),
                        crate::nav::PATH_BUDGET,
                    ) {
                        if let Ok(mut ai) = ais.get_mut(e) {
                            ai.clear_path();
                            ai.aggro = false;
                            ai.patrol.interrupt();
                        }
                        commands.entity(e).insert(crate::combat::QuestMove {
                            path: p.into_iter().skip(1).map(Vec3::from).collect(),
                            interruptible: *interruptible,
                            anim: false,
                        });
                    }
                }
            }
            Effect::UseSkill { id, skill, target } => {
                for (e, a, tf, _, _, monster) in &actors {
                    if a.0 != *id || monster.is_none() {
                        continue;
                    }
                    // `ControllerMonsterStateQuestUseSkill::OnBegin`: a Character target is used at its
                    // position; any other id makes the monster use the skill on itself.
                    let (te, point) =
                        match target_of(*target, &player).filter(|t| chars.contains(t.0)) {
                            Some((te, p)) => (te, p),
                            None => (e, tf.translation),
                        };
                    commands
                        .entity(e)
                        .insert(crate::monster_skills::QuestUseSkill {
                            skill: skill.clone(),
                            target: Some(te),
                            point,
                        });
                }
            }
            Effect::DestroyItems { trigger } => {
                let Some(rt) = rt.as_ref() else { continue };
                let story = rt.story.borrow();
                let Some(t) = story.triggers.iter().find(|t| t.id == *trigger) else {
                    continue;
                };
                for (e, gt, _) in &items {
                    if t.contains(gt.translation().to_array()) {
                        commands.entity(e).try_despawn();
                    }
                }
            }
            // `Character::RequestMove`: a plain walk, not interruptible.
            Effect::Move { id, coords } => {
                let dest = Vec3::from(coords.origin);
                for (e, a, tf, _, _, _) in &actors {
                    if a.0 != *id {
                        continue;
                    }
                    if let Some(p) = nav.0.find_path(
                        &mut scratch,
                        tf.translation.to_array(),
                        dest.to_array(),
                        crate::nav::PATH_BUDGET,
                    ) {
                        if let Ok(mut ai) = ais.get_mut(e) {
                            ai.clear_path();
                            ai.aggro = false;
                            ai.patrol.interrupt();
                        }
                        commands.entity(e).insert(crate::combat::QuestMove {
                            path: p.into_iter().skip(1).map(Vec3::from).collect(),
                            interruptible: false,
                            anim: false,
                        });
                    }
                }
            }
            // `Character::RequestAttack`: engage the target when the factions are hostile (`IsEnemyValid`).
            Effect::Attack { id, target } => {
                let Some((te, _)) = target_of(*target, &player) else {
                    continue;
                };
                let player_e = player.single().ok().map(|(e, _, _)| e);
                let target_pack = if Some(te) == player_e {
                    Some(std::sync::Arc::new(crate::monsters::player_pack(
                        &data.db,
                        factions.as_deref(),
                    )))
                } else {
                    ais.get(te).ok().and_then(|t| t.faction.clone())
                };
                for (e, a, _, _, _, _) in &actors {
                    if a.0 != *id {
                        continue;
                    }
                    let Ok(mut ai) = ais.get_mut(e) else { continue };
                    if !gd_game_core::monsters::FactionPack::foes(&ai.faction, &target_pack) {
                        debug!("story: attack {id} -> {target} refused (not hostile)");
                        continue;
                    }
                    ai.enemy = Some(te);
                    ai.aggro = true;
                    if !ai.anger.iter().any(|(t, _)| *t == te) {
                        ai.anger.push((te, 100.0));
                    }
                }
            }
            // `Character:ToggleInvincible(bool)`.
            Effect::SetInvincible { id, on } => {
                for (e, a, _, _, _, _) in &actors {
                    if a.0 == *id {
                        if *on {
                            commands.entity(e).insert(crate::combat::Invincible);
                        } else {
                            commands.entity(e).remove::<crate::combat::Invincible>();
                        }
                    }
                }
            }
            // `Character:ToggleSkills(bool)`.
            Effect::SetSkillsEnabled { id, on } => {
                for (e, a, _, _, _, _) in &actors {
                    if a.0 == *id {
                        if *on {
                            commands
                                .entity(e)
                                .remove::<crate::monster_skills::SkillsDisabled>();
                        } else {
                            commands
                                .entity(e)
                                .insert(crate::monster_skills::SkillsDisabled);
                        }
                    }
                }
            }
            // `Destructible::BreakApart`: the object breaks but stays valid and queryable; no `onBroken`
            // (the engine fires it only from `TakeAttack`). The bool arms a 1000 ms despawn.
            Effect::BreakDestructible {
                id,
                dir,
                force,
                timed,
            } => {
                for (e, a, b, tf) in &breakables {
                    if a.0 != *id {
                        continue;
                    }
                    broken.0.insert(b.key);
                    broke.write(crate::breakables::Broke {
                        record: b.record.clone(),
                        transform: *tf,
                        dir: Vec3::from(dir.origin),
                        force: *force,
                        story: None,
                    });
                    commands.entity(e).try_despawn();
                    spawned_actors.0.remove(id);
                    if *timed {
                        sw.broken_despawn.push((*id, 1.0));
                    }
                }
            }
            _ => {}
        }
    }
}

/// `Destructible:Destroy`'s timed despawn: the broken object leaves the world after the delay but stays
/// in the story.
pub fn despawn_broken(
    time: Res<Time>,
    rt: Option<NonSend<StoryRuntime>>,
    mut sw: NonSendMut<StoryWorld>,
) {
    if sw.broken_despawn.is_empty() {
        return;
    }
    let Some(rt) = rt else { return };
    let dt = time.delta_secs();
    let mut story = rt.story.borrow_mut();
    sw.broken_despawn.retain_mut(|(id, left)| {
        *left -= dt;
        if *left > 0.0 {
            return true;
        }
        if let Some(o) = story.entities.get_mut(*id) {
            o.in_world = false;
        }
        false
    });
}
