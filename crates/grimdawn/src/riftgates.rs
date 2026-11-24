//! Riftgates and respawn points: the presentation side of
//! `gd_game::riftgates` (rules and engine facts in its module doc). Here:
//! a click is the player's interact order (`gd_game::player_orders`, used in reach by
//! `gd_game::riftgates::use_gate`, which opens the riftgate map
//! `ui::riftmap`), gate effects and looping sounds (locked, else active once
//! discovered, else dormant), the
//! map-wide [`WaypointIndex`] (the riftgates, respawn points and devotion shrines among the map's
//! instance-group members, with their region's map records; built when the session begins, see
//! `gd_game::riftgates`) and granted travels ([`GateTravel`] → `doors::PendingTravel`).
//! Personal portals ([`Portal`]): their model and the area name where one opened ([`name_portal`]); a click
//! interacts with them like a gate.
//! The Shattered Realm's exit gates (`gd_game::endless::EndlessGate`): [`dress_exit_gates`].
//! Chthonic rifts ([`DynamicRift`], rules in `gd_game::riftgates`, their groups in `gd_game::unique`):
//! their look ([`rift_effects`]); a click interacts with an active one.
//!
//! Portal looks, Game.dll `FixedItemTeleport::SetOperation` / `Load`: a new
//! portal plays `openAnimation`
//! once (not looped) with `openSound`, a closing one `closedAnimation` with
//! `closedSound` (`openFxPakName` / `closedFxPakName` would be spawned too;
//! empty for this record). The mesh `personalriftgate01.msh` is a reused
//! zombie skeleton that the record's `baseTexture`
//! (`system/textures/invisible.tex`, fully transparent) hides, with
//! `castsShadows` 0 (`Actor::Load`, `CharAssets::spawn_record_model`); what
//! the player sees is `anm_personalrift_active.anm`'s `CreateEntity` at
//! frame 10: `records/fx/ui/riftgatepersonal_active_fx01.dbr` at the
//! `FXCentered` attach point (2.27 m up; `model::anim_entities`), whose
//! looping emitters run until the portal goes: then
//! `FixedItemTeleport::UpdateSelf` detaches the effect,
//! `StopEmitting`, and it fades out ([`gd_render::model::DetachEffects`]).
//!
//! Gate state sounds, `StaticTeleporter::SetActive` /
//! `Load`: each state change stops the old effect and sound and
//! spawns a `SoundEntity` with the state's pak (`lockedSound` while locked,
//! else `activeSound` once discovered, else `dormatSound`, spelled so in the
//! loader: stock gate records name it `dormantSound`, so gates are silent while
//! dormant) at the gate, looping like a placed `SoundObject`
//! (`audio::spawn_loop`: distance gain, silent beyond 8 × `fallOff`).
//!
//! The arrival: the engine's teleport activity (`InstallTeleportActivity`)
//! puts the player on the floor at the destination (`gd_game::doors::run_teleport`,
//! `gd_game::nav::teleport_landing`).
#[cfg(feature = "dev")]
use crate::player::MovePath;
use bevy::prelude::*;
use gd_formats::collections::{HashMap, HashSet};
use gd_formats::map::{RegionInfo, Uid};
#[cfg(feature = "dev")]
use gd_game::nav::Nav;
#[cfg(feature = "dev")]
use gd_game::riftgates::use_gate;
pub use gd_game::riftgates::{
    DynamicRift, GateTravel, IndexedPoint, PersonalPortal, Portal, TravelState, Waypoint,
    WaypointIndex, WaypointKind, waypoint_kind,
};

pub struct RiftgatesPlugin;

impl Plugin for RiftgatesPlugin {
    fn build(&self, app: &mut App) {
        app.register_required_components::<Waypoint, Visibility>();
        app.register_required_components::<Portal, Visibility>();
        app.register_required_components::<DynamicRift, Visibility>();
        app.register_required_components::<gd_game::endless::EndlessGate, Visibility>();
        app.add_plugins(crate::ui::riftmap::RiftMapUiPlugin);
        app.add_systems(
            Update,
            (
                build_index,
                gate_effects,
                rift_effects,
                #[cfg(feature = "dev")]
                test_portal.before(use_gate),
                #[cfg(feature = "dev")]
                test_riftgate
                    .before(use_gate)
                    .before(gd_game::riftgates::use_dynamic_rift),
                travel.after(gd_game::riftgates::RiftgateStage::Travel),
                name_portal.after(gd_game::riftgates::RiftgateStage::Travel),
                dress_portals.after(gd_game::riftgates::RiftgateStage::Portals),
                dress_exit_gates,
            )
                .chain()
                .after(gd_game_core::player::PlayerSet),
        );
    }
}

/// The gate's locked / active / dormant effect and looping sound entities
/// (if the record has them) and which of the three.
#[derive(Component)]
struct GateFx(Option<Entity>, Option<Entity>, &'static str);

/// The map-wide [`WaypointIndex`] (module doc), once the session's map is open: a Crucible or custom game
/// opens its own map before `SessionReady`, and the campaign world behind the menu needs no index.
fn build_index(
    mut commands: Commands,
    ready: Option<Res<gd_game_core::session::SessionReady>>,
    world: Option<Res<crate::world::StreamedMap>>,
    data: Res<gd_game_core::data::GameData>,
    index: Option<Res<WaypointIndex>>,
) {
    if index.is_some() || ready.is_none() {
        return;
    }
    let Some(world) = world else { return };
    let map = &world.map;
    let regions: HashMap<Uid, &RegionInfo> = map.regions.iter().map(|r| (r.id, r)).collect();
    // Record → Some(is a shrine) for gates / respawn points / shrines.
    let mut kinds: HashMap<&str, Option<bool>> = HashMap::default();
    let mut seen: HashSet<Uid> = HashSet::default();
    let mut idx = WaypointIndex::default();
    for e in map.instance_groups.iter().flat_map(|g| &g.entries) {
        // A member's data is its first entry (`InstanceGroups::entity_data`).
        if !seen.insert(e.uid) {
            continue;
        }
        let kind = *kinds.entry(&e.name).or_insert_with(|| {
            data.db.get(&e.name).and_then(|r| {
                if waypoint_kind(&r.class).is_some() {
                    Some(false)
                } else {
                    (r.class == "StaticShrine").then_some(true)
                }
            })
        });
        // `World::GetRegionById`: a member outside the map's regions has no place.
        let region = regions.get(&e.ints.map(|i| i as u32));
        let (Some(shrine), Some(pos), Some(region)) = (kind, e.world, region) else {
            continue;
        };
        let map_record = if shrine {
            &region.shrine_record
        } else {
            &region.location_record
        };
        idx.0.insert(
            e.uid,
            IndexedPoint {
                pos: Vec3::from(pos),
                record: e.name.to_ascii_lowercase(),
                map_record: map_record.to_ascii_lowercase(),
            },
        );
    }
    let n = |k: &str| idx.0.values().filter(|p| p.record.contains(k)).count();
    info!(
        "travel: {} riftgates/respawn points/shrines in the map ({} riftgate records, {} respawn records)",
        idx.0.len(),
        n("riftgate"),
        n("respawn")
    );
    commands.insert_resource(idx);
    commands.insert_resource(gd_game::riftgates::RiftgateGroup(
        map.instance_groups
            .iter()
            .filter(|g| g.kind == "Riftgates")
            .flat_map(|g| g.entries.iter().map(|e| e.uid))
            .collect(),
    ));
}

/// Riftgates show their locked effect while locked, else the active one
/// once discovered, else the dormant one (`gd_game::riftgates` module doc).
fn gate_effects(
    mut commands: Commands,
    state: Option<Res<TravelState>>,
    data: Res<gd_game_core::data::GameData>,
    q: Query<(Entity, &Waypoint, Option<&GateFx>)>,
) {
    let Some(state) = state else { return };
    for (e, w, fx) in &q {
        if w.kind != WaypointKind::Riftgate {
            continue;
        }
        let (key, sound) = if w.locked {
            ("lockedEffect", "lockedSound")
        } else if state.teleports.contains(&w.uid) {
            ("activeEffect", "activeSound")
        } else {
            ("dormantEffect", "dormatSound")
        };
        if fx.is_some_and(|f| f.2 == key) {
            continue;
        }
        for old in fx.into_iter().flat_map(|f| [f.0, f.1]).flatten() {
            commands.entity(old).try_despawn();
        }
        let rec = data.db.get(&w.record);
        let fx = rec
            .as_ref()
            .and_then(|r| r.str(key).map(str::to_owned))
            .map(|r| {
                let fx = gd_render::fx::spawn_effect_record(&mut commands, &r, Transform::IDENTITY);
                commands.entity(e).add_child(fx);
                fx
            });
        let snd = rec
            .as_ref()
            .and_then(|r| r.str(sound).filter(|s| !s.is_empty()).map(str::to_owned))
            .and_then(|pak| crate::audio::spawn_loop(&mut commands, &data, &pak, Vec3::ZERO));
        if let Some(snd) = snd {
            commands.entity(e).add_child(snd);
        }
        commands.entity(e).insert(GateFx(fx, snd, key));
    }
}

/// A granted travel waits for its destination's nav data (`gd_game::doors::run_teleport`).
fn travel(
    mut granted: MessageReader<GateTravel>,
    mut pending: ResMut<crate::doors::PendingTravel>,
) {
    for GateTravel(pos) in granted.read() {
        pending.0.get_or_insert(*pos);
    }
}

/// A new personal portal gets the area name tag where it opened
/// (`Engine::GetAreaNameTag`: the name sector there).
fn name_portal(
    mut portal: ResMut<PersonalPortal>,
    world: Option<Res<crate::world::StreamedMap>>,
    levels: Res<crate::world::Levels>,
) {
    let Some(world) = world else { return };
    let Some(p) = portal.portal.as_ref().filter(|p| p.area_tag.is_none()) else {
        return;
    };
    let entry = match p.region {
        Some(r) => levels.sector_entry_in(&world.map, r, p.pos, 1),
        None => levels.sector_entry(&world.map, p.pos, 1),
    };
    let tag = match entry {
        Some(gd_formats::map::sector::SectorEntry::Name { tag, .. }) => tag.clone(),
        _ => String::new(),
    };
    if let Some(p) = portal.portal.as_mut() {
        p.area_tag = Some(tag);
    }
}

/// Portal looks (module doc): the model with its open animation and sound
/// when it appears; the closed animation and sound once it closes.
fn dress_portals(
    mut commands: Commands,
    q: Query<
        (
            Entity,
            &GlobalTransform,
            &Portal,
            Option<&gd_render::model::Animator>,
        ),
        (Changed<Portal>, Without<PortalClosed>),
    >,
    new: Query<(), Added<Portal>>,
    mut a: gd_render::character::CharAssets,
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
) {
    if q.is_empty() {
        return;
    }
    let db = a.data.db.clone();
    let Some(rec) = db.get(gd_game::riftgates::PORTAL_RECORD) else {
        return;
    };
    for (e, gt, p, anim) in &q {
        let (key, snd) = match (new.contains(e), p.closing.is_some()) {
            (true, _) => ("openAnimation", "openSound"),
            (false, true) => {
                commands.entity(e).insert(PortalClosed);
                ("closedAnimation", "closedSound")
            }
            _ => continue,
        };
        if anim.is_none() {
            a.spawn_record_model(&mut commands, e, rec);
            commands.entity(e).insert(gd_render::model::DetachEffects);
        }
        if let Some(an) = rec.str(key).and_then(|k| a.cache.anim(&a.data, k)) {
            commands.entity(e).insert(gd_render::model::Animator {
                anim: an,
                time: 0.0,
                speed: rec.f32(&format!("{key}Speed")).unwrap_or(1.0),
                looped: false,
            });
        }
        if let Some(pak) = rec.str(snd).filter(|s| !s.is_empty()) {
            sound.write(gd_game_core::visuals::PlaySound {
                pak: pak.to_owned(),
                at: Some(gt.translation()),
            });
        }
    }
}

/// The portal's closed animation has been started.
#[derive(Component)]
struct PortalClosed;

/// A Shattered Realm exit gate as it appears: its record model (`CharAssets::spawn_record_model`): the mesh
/// `ugdoorobj_endlessdungeonentrance_01.msh` under `invisible.tex`, whose MIF `CreateEntity` puts
/// `records/fx/ui/riftgate_endless_fx01.dbr` (the small Shattered Realm rift, `pfx_endlessrift.pfx`, with its
/// decal) at its `DoorFX` point; and the `openSound` loop. `FixedItemTeleport::InitialUpdate` → `SetOperation(0)`
/// (not locked): `openAnimation` (none in this record),
/// `openSound` at the gate, `openFxPakName` (none).
fn dress_exit_gates(
    mut commands: Commands,
    new: Query<Entity, Added<gd_game::endless::EndlessGate>>,
    mut a: gd_render::character::CharAssets,
) {
    if new.is_empty() {
        return;
    }
    let db = a.data.db.clone();
    let Some(rec) = db.get(gd_game::endless::EXIT_GATE_RECORD) else {
        return;
    };
    for e in &new {
        a.spawn_record_model(&mut commands, e, rec);
        if let Some(snd) = rec
            .str("openSound")
            .filter(|s| !s.is_empty())
            .and_then(|pak| crate::audio::spawn_loop(&mut commands, &a.data, pak, Vec3::ZERO))
        {
            commands.entity(e).add_child(snd);
        }
    }
}

/// `GD_PORTAL` (hooks.rs): after `secs`, presses the HUD teleport
/// button once, then uses the portal (opening the map with it in use).
/// With `:open` it only opens it; a list of times presses it at each time
/// (each new portal replaces the last one).
#[allow(clippy::too_many_arguments)]
#[cfg(feature = "dev")]
fn test_portal(
    hooks: Res<gd_game_core::hooks::Hooks>,
    time: Res<Time>,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    mut out: MessageWriter<gd_game::riftgates::OpenPortal>,
    portals: Query<(Entity, &GlobalTransform, &Portal)>,
    mut state: Local<u8>,
    mut players: Query<
        (
            &Transform,
            &mut MovePath,
            &mut gd_game::player_orders::InteractTarget,
        ),
        With<gd_game_core::player::Player>,
    >,
    nav: Res<Nav>,
) {
    let Some(v) = hooks.gameplay.portal.clone() else {
        return;
    };
    let (secs, open_only) = match v.split_once(':') {
        Some((s, "open")) => (s, true),
        _ => (v.as_str(), false),
    };
    let times: Vec<f32> = secs.split(',').filter_map(|s| s.parse().ok()).collect();
    if open_only {
        // `state` counts the presses.
        if times
            .get(*state as usize)
            .is_some_and(|t| time.elapsed_secs() >= *t)
        {
            info!("riftgates test: opening a personal portal");
            out.write(gd_game::riftgates::OpenPortal);
            *state += 1;
        }
        return;
    }
    let Some(&secs) = times.first() else { return };
    match *state {
        0 if time.elapsed_secs() >= secs => {
            info!("riftgates test: opening a personal portal");
            out.write(gd_game::riftgates::OpenPortal);
            *state = 1;
        }
        1 => {
            if let Some((e, gt, _)) = portals.iter().find(|(_, _, p)| p.closing.is_none()) {
                if let Ok((tf, mut path, mut interact)) = players.single_mut() {
                    interact.0 = Some(e);
                    if let Some(p) = gd_game::chests::path_near(
                        &nav.0,
                        &mut scratch,
                        tf.translation,
                        gt.translation(),
                        1.0,
                    ) {
                        path.points = p;
                    }
                }
                *state = 2;
            }
        }
        _ => {}
    }
}

/// `GD_USE_RIFTGATE` (hooks.rs): the nearest riftgate or active Chthonic rift within 25 m, once (the interact
/// order walks to it and opens the map; `use_gate` keeps the target while the hook holds).
#[allow(clippy::too_many_arguments)]
#[cfg(feature = "dev")]
fn test_riftgate(
    hooks: Res<gd_game_core::hooks::Hooks>,
    gates: Query<(Entity, &GlobalTransform, &Waypoint)>,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    rifts: Query<(Entity, &GlobalTransform, &DynamicRift)>,
    mut players: Query<
        (
            &Transform,
            &mut MovePath,
            &mut gd_game::player_orders::InteractTarget,
        ),
        With<gd_game_core::player::Player>,
    >,
    nav: Res<Nav>,
    mut tested: Local<bool>,
) {
    if !hooks.gameplay.use_riftgate || *tested {
        return;
    }
    let Ok((tf, mut path, mut interact)) = players.single_mut() else {
        return;
    };
    if interact.0.is_some() {
        return;
    }
    let near = gates
        .iter()
        .filter(|(_, _, w)| w.kind == WaypointKind::Riftgate)
        .map(|(e, gt, w)| (e, gt.translation(), w.radius))
        .chain(
            rifts
                .iter()
                .filter(|(_, _, r)| r.active == Some(true))
                .map(|(e, gt, _)| (e, gt.translation(), 0.0)),
        );
    let best = near
        .map(|(e, at, r)| (e, at, r, at.distance(tf.translation)))
        .filter(|x| x.3 < 25.0)
        .min_by(|a, b| a.3.total_cmp(&b.3));
    if let Some((e, at, r, _)) = best {
        *tested = true;
        interact.0 = Some(e);
        if let Some(p) =
            gd_game::chests::path_near(&nav.0, &mut scratch, tf.translation, at, r.min(2.0))
        {
            path.points = p;
        }
    }
}

/// A Chthonic rift's look (`DynamicTeleporter::SetActive`): active, its `activeEffect` and a
/// looping `activeSound`; dormant, `dormantEffect` and the `dormatSound` the loader asks for (stock records
/// have `dormantSound`: silent); unselected (state 1), nothing (`gd_game::riftgates::DynamicRift`).
fn rift_effects(
    mut commands: Commands,
    data: Res<gd_game_core::data::GameData>,
    rifts: Query<(Entity, &DynamicRift), Changed<DynamicRift>>,
) {
    for (e, r) in &rifts {
        let Some(active) = r.active else { continue };
        let Some(rec) = data.db.get(&r.record) else {
            continue;
        };
        if !active && rec.bool("active") {
            continue;
        }
        let (fx, sound) = if active {
            ("activeEffect", "activeSound")
        } else {
            ("dormantEffect", "dormatSound")
        };
        if let Some(fx) = rec.str(fx) {
            let fx = gd_render::fx::spawn_effect_record(&mut commands, fx, Transform::IDENTITY);
            commands.entity(e).add_child(fx);
        }
        if let Some(snd) = rec
            .str(sound)
            .and_then(|pak| crate::audio::spawn_loop(&mut commands, &data, pak, Vec3::ZERO))
        {
            commands.entity(e).add_child(snd);
        }
    }
}
