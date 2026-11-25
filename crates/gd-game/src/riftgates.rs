//! Riftgates (`StaticTeleporter`) and respawn points (`StaticRespawner`).
//!
//! * Respawn points bind within `bindRadius` (`BindingInteractable::UpdateSelf`):
//!   `Player::AddRespawnUID` adds the uid to the difficulty's list and makes it current.
//! * Riftgates bind the same way ([`bind_riftgates`]) when inactive and not locked
//!   (`IsBindingEnabled`); `BindToCharacter` shows tutorial page 9, activates the gate and adds it
//!   to the difficulty's discovered list (`Player::AddTeleportUID`). Using a gate does not discover it.
//! * Lock: the record's `locked`, then the scripts' `SetLocked` ([`RiftgateLock`]). A locked gate
//!   is not of interest, doesn't bind and ignores use. The lock is not saved; a load gets it back
//!   from the gate's `onAddToWorld` script (which unlocks it given the quest token).
//!   Look (`SetActive`): `lockedEffect` / `lockedSound` while locked, else `activeEffect` /
//!   `activeSound` once discovered, else `dormantEffect` / `dormatSound`.
//! * Using a gate opens the riftgate map ([`RiftgateMap`], `RiftgateMap::HandleOpenEvent`).
//! * Discovered gates / respawn points round-trip through the save (`player.gdc` blocks 6 / 5, per
//!   difficulty). Their places come from the map's instance groups, not the levels
//!   (`RiftgateMap::BuildDestinations`, `Player::GetClosestRespawnPoint`: region offset + member
//!   position): [`WaypointIndex`]. A uid in no group has no place; the map shows a shrine's group
//!   position even where its placed entity differs.
//!
//! A gate is used like every fixed actor (player radius + its radius + `usableRange`, +1 m at the
//! walk's end). `StaticTeleporter::Load`: `usableRange` (default 1), `bindRadius` (default 5),
//! `radius` (default 5: [`Waypoint::radius`], not the actor's radius, which is sqrt(3) m with
//! `Actor::Load`'s default). Its map label is its `description` tag. Travel to a gate within 4 m of
//! the player is refused (`RiftgateMap::Teleport`); the teleport lands the player on the floor at
//! the gate ([`crate::nav::teleport_landing`]).
//!
//! Travel from the map (`RiftgateMap::HandleMouseEvent`, `RiftgateMap::Travel`):
//! - (first) away from the gate / portal in use (farther than 8 m, or gone) the player's own portal
//!   is no destination, and travel needs [`can_use_portal`], else `tagRiftgateError03`; when it
//!   can, a personal portal opens where the player stands and the travel goes on;
//! - the gate / portal in use as a destination: `tagRiftgateError02`;
//! - else a gate destination becomes the last used gate (`GameEngine::SetLastUsedTeleportId`) and
//!   the player teleports to it. No cost.
//! - PORT: the map closes on travel (the engine's with the teleport's loading); walking away does
//!   not close it.
//!
//! Personal portals ([`PersonalPortal`], `FixedItemTeleport` with `staticPortal` false):
//! - `MainPlayerCanUsePersonalTeleport`: refused in a sector of the riftgate sector layer (10),
//!   when `personalTeleportToken` is set and the player lacks it ([`portal_unlocked`]), and without
//!   a discovered riftgate in the difficulty;
//! - `CreateFixedItemTeleport`: one portal per player, a new one replacing the old;
//!   [`PORTAL_RECORD`] spawns at `Player::GetNearSpawnCoords` ([`near_spawn`]):
//!   `NavManager::FillPointSet(player, 16 m, five spots of radius 2)`, the portal at the first of
//!   spots 1..4 (the 4.1 m ring, farther where blocked) the player can walk to, else the player's
//!   spot;
//! - using it opens the map with the portal in use; on the map it is an icon whose travel goes back
//!   to its spot;
//! - it stays until the player opens another or leaves the game (not saved). An entity no longer
//!   listed is removed after 250 ms (`FixedItemTeleport::UpdateSelf`).
//!   PORT: the entity is kept while its level is unloaded.
//! - HUD button (`UIPlayerHud::Update`): `hudTeleportButton` while the player can use a portal and
//!   has none, `hudActiveTeleportButton` while it has one ([`portal_button`]); pressing it opens a
//!   portal ([`OpenPortal`]), then both are disabled for 1000 ms. While the game menu / options
//!   windows are open the active button is hidden and the other takes no press or key.
//!   PORT: tutorial page 10 isn't unlocked.
use crate::movement::MovePath;
use bevy::prelude::*;
use gd_formats::collections::HashMap;
use gd_formats::map::{Uid, uid_words};
use gd_game_core::player::Player;
use gd_game_core::riftgates::player_region;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaypointKind {
    Riftgate,
    Respawner,
}

/// A riftgate or respawn point placed in a streamed-in level.
#[derive(Component)]
pub struct Waypoint {
    pub kind: WaypointKind,
    pub uid: Uid,
    pub record: String,
    pub radius: f32,
    /// `bindRadius`, default 5.
    pub bind_radius: f32,
    /// The record's `locked`, then the scripts' `SetLocked`.
    pub locked: bool,
}

impl Waypoint {
    /// `StaticTeleporter::IsOfInterest`: not locked.
    pub fn is_of_interest(&self) -> bool {
        !self.locked
    }
}

/// A script's `riftgate:SetLocked` on the gate of `record` at `at`.
#[derive(Message, Clone, Debug)]
pub struct RiftgateLock {
    pub record: String,
    pub at: Vec3,
    pub locked: bool,
}

/// Applies [`RiftgateLock`]s, remembered for gates streamed in later.
pub fn riftgate_locks(
    mut fresh: gd_game_core::session::NewSession,
    mut ev: MessageReader<RiftgateLock>,
    mut gates: Query<(&Transform, &mut Waypoint)>,
    mut locks: Local<Vec<(String, Vec3, bool)>>,
) {
    if fresh.begun() {
        *locks = Default::default();
    }
    let mut any = false;
    for l in ev.read() {
        any = true;
        locks.retain(|(r, p, _)| !(r.eq_ignore_ascii_case(&l.record) && p.distance(l.at) < 1.0));
        locks.push((l.record.clone(), l.at, l.locked));
    }
    if locks.is_empty() || (!any && !gates.iter_mut().any(|(_, w)| w.is_added())) {
        return;
    }
    for (tf, mut w) in &mut gates {
        if let Some(&(_, _, locked)) = locks
            .iter()
            .find(|(r, p, _)| r.eq_ignore_ascii_case(&w.record) && p.distance(tf.translation) < 1.0)
            && w.locked != locked
        {
            w.locked = locked;
        }
    }
}

/// The player's discovered riftgates and respawn points (this difficulty).
#[derive(Resource, Default)]
pub struct TravelState {
    pub difficulty: usize,
    pub teleports: Vec<Uid>,
    pub respawns: Vec<Uid>,
    pub current: Option<Uid>,
    /// `GameEngine::SetLastUsedTeleportId`; the map centres on it when no discovered gate is near.
    pub last_used: Option<Uid>,
    /// Riftgates discovered on other difficulties this session (difficulty unlock item):
    /// (difficulty, uid), merged into the save's lists.
    pub unlocked: Vec<(usize, Uid)>,
}

/// The world map's `Riftgates` instance groups' entries (`GetGroupsOfType("Riftgates")`).
#[derive(Resource, Default)]
pub struct RiftgateGroup(pub Vec<Uid>);

#[derive(Clone, Debug, PartialEq)]
pub struct IndexedPoint {
    pub pos: Vec3,
    /// Lower case.
    pub record: String,
    /// The region's zone record for gates (`Region::GetZoneRecord`), its shrine icon record for
    /// shrines (`Region::GetShrineRecord`).
    pub map_record: String,
}

/// The map's instance-group members that are riftgates, respawn points or devotion shrines, by uid.
#[derive(Resource, Default)]
pub struct WaypointIndex(pub HashMap<Uid, IndexedPoint>);

/// Within this distance of the gate in use the map travels straight from it (`RiftgateMap::Travel`).
pub const AT_GATE: f32 = 8.0;

/// `CreateFixedItemTeleportNetHook`.
pub const PORTAL_RECORD: &str = "records/level art/riftgates/riftgate_personal.dbr";

/// HUD teleport buttons' disable time after opening a portal (`UIPlayerHud::Update`).
pub const BUTTON_WAIT: f32 = 1.0;

/// What opened the map, or a travel destination; a portal by its object id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rift {
    Gate(Uid),
    Portal(u32),
}

/// `at_gate`: still within [`AT_GATE`] of the gate / portal that opened it.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct RiftgateMap {
    pub open: bool,
    pub gate: Option<Rift>,
    pub at_gate: bool,
}

/// The engine's TeleportInfo entry.
#[derive(Clone, Debug, PartialEq)]
pub struct PortalInfo {
    /// A new one per portal.
    pub id: u32,
    pub pos: Vec3,
    /// The player's region when opened: its area name's sector lookups read that region's level.
    pub region: Option<usize>,
    /// `Engine::GetAreaNameTag`; set by the game (`None` until then).
    pub area_tag: Option<String>,
}

/// Not saved.
#[derive(Resource, Default, Debug)]
pub struct PersonalPortal {
    pub portal: Option<PortalInfo>,
    pub button_wait: f32,
    last_id: u32,
}

impl PersonalPortal {
    pub fn open(&mut self, pos: Vec3, region: Option<usize>) -> u32 {
        self.last_id += 1;
        self.portal = Some(PortalInfo {
            id: self.last_id,
            pos,
            region,
            area_tag: None,
        });
        info!("travel: personal portal opened at {pos:?}");
        self.last_id
    }

    pub fn pos(&self, id: u32) -> Option<Vec3> {
        self.portal.as_ref().filter(|p| p.id == id).map(|p| p.pos)
    }
}

/// A personal portal entity; the game dresses it.
#[derive(Component, Debug)]
pub struct Portal {
    pub id: u32,
    /// Milliseconds since it left the list (it goes at 250).
    pub closing: Option<u32>,
}

impl Portal {
    /// `FixedItemTeleport::IsOfInterest` (PORT: read as not closing).
    pub fn is_of_interest(&self) -> bool {
        self.closing.is_none()
    }
}

#[derive(Message, Clone, Copy, Debug, Default)]
pub struct OpenPortal;

/// `active`: `hudActiveTeleportButton` (the player has a portal), else `hudTeleportButton`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortalButton {
    pub active: bool,
    pub disabled: bool,
}

/// `MainPlayerCanUsePersonalTeleport` for a player at `pos`; the no-portal sector is read from
/// `region`'s level (`None`: whichever level covers `pos`).
pub fn can_use_portal(
    pos: Vec3,
    region: Option<usize>,
    levels: Option<&gd_game_core::levels::Levels>,
    state: &TravelState,
    unlocked: bool,
) -> bool {
    let no_portal_sector = levels
        .and_then(|l| match region {
            Some(r) => l.sector_guid_in(r, pos, 10),
            None => l.sector_guid(pos, 10),
        })
        .is_some_and(|g| g != [0; 4]);
    !no_portal_sector && unlocked && !state.teleports.is_empty()
}

/// `personalTeleportToken`: when set, the player needs that story token (empty in stock data).
pub fn portal_unlocked(
    data: &gd_game_core::data::GameData,
    rt: Option<&gd_game_core::story::StoryRuntime>,
) -> bool {
    let token = data
        .db
        .get("records/game/gameengine.dbr")
        .and_then(|t| t.str("personalTeleportToken"))
        .unwrap_or_default();
    token.is_empty()
        || rt.is_none_or(|r| {
            r.story
                .borrow()
                .player
                .tokens
                .iter()
                .any(|t| t.eq_ignore_ascii_case(token))
        })
}

/// `None`: neither shows.
pub fn portal_button(
    pos: Vec3,
    region: Option<usize>,
    levels: Option<&gd_game_core::levels::Levels>,
    state: &TravelState,
    portal: &PersonalPortal,
    unlocked: bool,
) -> Option<PortalButton> {
    can_use_portal(pos, region, levels, state, unlocked).then_some(PortalButton {
        active: portal.portal.is_some(),
        disabled: portal.button_wait > 0.0,
    })
}

/// `Player::GetNearSpawnCoords`.
pub fn near_spawn(
    nav: &crate::nav::Nav,
    scratch: &mut gd_sim::nav::PathScratch,
    tf: &Transform,
) -> Vec3 {
    let p = tf.translation;
    let spots = gd_sim::pattern::fill_point_set(
        (p.x, p.z),
        &[NEAR_SPAWN_RADIUS; 5],
        NEAR_SPAWN_DIST,
        gd_game_core::rng::f32,
        |x, z| nav.0.ground([x, p.y + 2.0, z]),
    );
    spots
        .iter()
        .skip(1)
        .map(|s| Vec3::from(*s))
        .find(|s| {
            nav.0
                .find_path(scratch, p.to_array(), s.to_array(), crate::nav::PATH_BUDGET)
                .is_some()
        })
        .unwrap_or(p)
}

/// `GetNearSpawnCoords`' spot radius and `FillPointSet` distance.
const NEAR_SPAWN_RADIUS: f32 = 2.0;
const NEAR_SPAWN_DIST: f32 = 16.0;

#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub enum RiftgateRequest {
    Travel(Rift),
    Close,
}

/// The game moves the player to `0`.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct GateTravel(pub Vec3);

pub fn waypoint_kind(class: &str) -> Option<WaypointKind> {
    match class {
        "StaticTeleporter" => Some(WaypointKind::Riftgate),
        "StaticRespawner" => Some(WaypointKind::Respawner),
        _ => None,
    }
}

pub fn init_travel(
    mut commands: Commands,
    // Option: `state` is the existence check (created here); `loaded` is absent for a new character.
    state: Option<Res<TravelState>>,
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
    players: Query<(), With<Player>>,
) {
    if state.is_some() || players.is_empty() {
        return;
    }
    let mut s = TravelState::default();
    if let Some(c) = &loaded {
        let d = c.0.info.difficulty();
        s.difficulty = d;
        s.teleports = c.0.teleports.lists[d].iter().map(uid_words).collect();
        s.respawns = c.0.respawns.lists[d].iter().map(uid_words).collect();
        s.current = Some(uid_words(&c.0.respawns.current[d])).filter(|u| *u != [0; 4]);
    }
    info!(
        "travel: {} riftgates, {} respawn points known",
        s.teleports.len(),
        s.respawns.len()
    );
    commands.insert_resource(s);
}

/// The first respawn point within its `bindRadius` (default 8) of the player becomes current.
pub fn bind_respawners(
    mut commands: Commands,
    mut state: ResMut<TravelState>,
    q: Query<(&GlobalTransform, &Waypoint)>,
    players: Query<&Transform, (With<Player>, Without<gd_game_core::combat::Dead>)>,
) {
    let Ok(p) = players.single() else {
        return;
    };
    for (gt, w) in &q {
        if w.kind != WaypointKind::Respawner
            || state.current == Some(w.uid)
            || gt.translation().distance(p.translation) > w.radius
        {
            continue;
        }
        if !state.respawns.contains(&w.uid) {
            state.respawns.push(w.uid);
        }
        state.current = Some(w.uid);
        commands.insert_resource(gd_game_core::session::PlayerStart(gt.translation()));
        info!("travel: respawn point {:?} bound", gt.translation());
        break;
    }
}

/// Unlocked riftgates bind while the player is within their `bindRadius`.
pub fn bind_riftgates(
    mut state: ResMut<TravelState>,
    q: Query<(&GlobalTransform, &Waypoint)>,
    players: Query<&Transform, (With<Player>, Without<gd_game_core::combat::Dead>)>,
    mut tutorial: MessageWriter<crate::tutorials::UnlockTutorial>,
) {
    let Ok(p) = players.single() else {
        return;
    };
    for (gt, w) in &q {
        if w.kind != WaypointKind::Riftgate
            || w.locked
            || gt.translation().distance(p.translation) > w.bind_radius
            || state.teleports.contains(&w.uid)
        {
            continue;
        }
        state.teleports.push(w.uid);
        tutorial.write(crate::tutorials::UnlockTutorial(9));
        info!("travel: riftgate discovered at {:?}", gt.translation());
    }
}

/// Within reach of the clicked gate or personal portal: open the map with it in use. Other targets
/// (Chthonic rifts, Shattered Realm gates and shrines) stay set for their own systems.
#[allow(clippy::too_many_arguments)]
pub fn use_gate(
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
    gates: Query<(&GlobalTransform, &Waypoint)>,
    portals: Query<(&GlobalTransform, &Portal)>,
    mut players: Query<
        (
            &Transform,
            &mut MovePath,
            &mut crate::player_orders::InteractTarget,
            &gd_game_core::combat::Combatant,
        ),
        With<Player>,
    >,
    data: Res<gd_game_core::data::GameData>,
    mut map: ResMut<RiftgateMap>,
) {
    let Ok((ptf, mut path, mut interact, pcb)) = players.single_mut() else {
        return;
    };
    let Some(e) = interact.0 else { return };
    let path_end = path.points.is_empty();
    let reach_of = |record: &str| {
        gd_game_core::combat::fixed_use_range(data.db.get(record), pcb.radius, path_end)
    };
    let (at, reach, rift) = match (gates.get(e), portals.get(e)) {
        (Ok((gt, w)), _) if !w.locked => (gt.translation(), reach_of(&w.record), Rift::Gate(w.uid)),
        // `FixedItemTeleport`: `actorRadius` 0, no `usableRange`: 1 m beyond the player's radius.
        (_, Ok((gt, p))) if p.closing.is_none() => (
            gt.translation(),
            reach_of(PORTAL_RECORD),
            Rift::Portal(p.id),
        ),
        (Err(_), Err(_)) => return,
        _ => {
            interact.0 = None;
            return;
        }
    };
    if ptf.translation.distance(at) > reach {
        return;
    }
    #[cfg(feature = "dev")]
    let keep = hooks.gameplay.use_riftgate;
    #[cfg(not(feature = "dev"))]
    let keep = false;
    if !keep {
        interact.0 = None;
    }
    path.points.clear();
    *map = RiftgateMap {
        open: true,
        gate: Some(rift),
        at_gate: true,
    };
}

fn gate_pos(
    uid: &Uid,
    gates: &Query<(&GlobalTransform, &Waypoint)>,
    index: Option<&WaypointIndex>,
) -> Option<Vec3> {
    gates
        .iter()
        .find(|(_, w)| w.uid == *uid)
        .map(|(g, _)| g.translation())
        .or_else(|| index.and_then(|i| i.0.get(uid)).map(|p| p.pos))
}

/// The map's requests and its "at the gate" flag.
#[allow(clippy::too_many_arguments)]
pub fn travel_requests(
    mut reqs: MessageReader<RiftgateRequest>,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    mut map: ResMut<RiftgateMap>,
    // Option: the map flag and close requests work without travel state.
    state: Option<ResMut<TravelState>>,
    mut portal: ResMut<PersonalPortal>,
    index: Option<Res<WaypointIndex>>,
    gates: Query<(&GlobalTransform, &Waypoint)>,
    players: Query<&Transform, With<Player>>,
    world: (Res<gd_game_core::levels::Levels>, Res<crate::nav::Nav>),
    unlock: (
        Res<gd_game_core::data::GameData>,
        Option<NonSend<gd_game_core::story::StoryRuntime>>,
    ),
    mut notify: MessageWriter<gd_game_core::story::Notify>,
    mut out: MessageWriter<GateTravel>,
) {
    let Ok(ptf) = players.single() else { return };
    let p = ptf.translation;
    let rift_pos = |r: Rift, portal: &PersonalPortal| match r {
        Rift::Gate(uid) => gate_pos(&uid, &gates, index.as_deref()),
        Rift::Portal(id) => portal.pos(id),
    };
    let at = map
        .gate
        .and_then(|g| rift_pos(g, &portal))
        .is_some_and(|g| g.distance(p) <= AT_GATE);
    if map.open && map.at_gate != at {
        map.at_gate = at;
    }
    let Some(mut state) = state else {
        // Nothing travels without travel state; the map still closes.
        for r in reqs.read() {
            if *r == RiftgateRequest::Close {
                map.open = false;
            }
        }
        return;
    };
    for r in reqs.read() {
        let RiftgateRequest::Travel(dest) = *r else {
            map.open = false;
            continue;
        };
        if !map.open {
            continue;
        }
        if !at {
            if matches!(dest, Rift::Portal(_))
                || !can_use_portal(
                    p,
                    player_region(unlock.1.as_deref()),
                    Some(&*world.0),
                    &state,
                    portal_unlocked(&unlock.0, unlock.1.as_deref()),
                )
            {
                notify.write(gd_game_core::story::Notify("tagRiftgateError03".into()));
                continue;
            }
            portal.open(
                near_spawn(&world.1, &mut scratch, ptf),
                player_region(unlock.1.as_deref()),
            );
        }
        if map.gate == Some(dest) {
            notify.write(gd_game_core::story::Notify("tagRiftgateError02".into()));
            continue;
        }
        let pos = match dest {
            Rift::Gate(uid) => {
                let Some(pos) = state
                    .teleports
                    .contains(&uid)
                    .then(|| gate_pos(&uid, &gates, index.as_deref()))
                    .flatten()
                else {
                    continue;
                };
                state.last_used = Some(uid);
                pos
            }
            Rift::Portal(id) => {
                let Some(pos) = portal.pos(id) else { continue };
                pos
            }
        };
        if pos.distance(p) > 4.0 {
            info!("travel: to {dest:?} at {pos:?}");
            out.write(GateTravel(pos));
            map.open = false;
        }
    }
}

/// Opens a portal where the player stands when allowed and the button isn't waiting.
#[allow(clippy::too_many_arguments)]
pub fn hud_portal(
    mut reqs: MessageReader<OpenPortal>,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    mut portal: ResMut<PersonalPortal>,
    // Option: the timer runs and requests drain without travel state.
    state: Option<Res<TravelState>>,
    players: Query<&Transform, With<Player>>,
    levels: Res<gd_game_core::levels::Levels>,
    nav: Res<crate::nav::Nav>,
    time: Res<Time>,
    data: Res<gd_game_core::data::GameData>,
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
) {
    portal.button_wait = (portal.button_wait - time.delta_secs()).max(0.0);
    let (Ok(tf), Some(state)) = (players.single(), state) else {
        reqs.clear();
        return;
    };
    for _ in reqs.read() {
        if portal.button_wait <= 0.0
            && can_use_portal(
                tf.translation,
                player_region(rt.as_deref()),
                Some(&*levels),
                &state,
                portal_unlocked(&data, rt.as_deref()),
            )
        {
            portal.open(
                near_spawn(&nav, &mut scratch, tf),
                player_region(rt.as_deref()),
            );
            portal.button_wait = BUTTON_WAIT;
        }
    }
}

/// Spawns the current [`PersonalPortal`], closes and removes the others.
pub fn sync_portals(
    mut commands: Commands,
    portal: Res<PersonalPortal>,
    mut q: Query<(Entity, &mut Portal)>,
    time: Res<Time>,
) {
    let current = portal.portal.as_ref();
    let mut have = false;
    for (e, mut p) in &mut q {
        if current.is_some_and(|c| c.id == p.id) {
            have = true;
            continue;
        }
        let ms = p.closing.unwrap_or(0) + (time.delta_secs() * 1000.0).clamp(1.0, 100.0) as u32;
        if ms > 250 {
            commands.entity(e).try_despawn();
        } else {
            p.closing = Some(ms);
        }
    }
    if let (Some(c), false) = (current, have) {
        let tf = Transform::from_translation(c.pos);
        commands.spawn((
            tf,
            GlobalTransform::from(tf),
            Portal {
                id: c.id,
                closing: None,
            },
            crate::player_orders::Interactable::fixed(1.0),
            Name::new("personal portal"),
        ));
    }
}

pub struct RiftgatesPlugin;

impl Plugin for RiftgatesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RiftgateMap>()
            .init_resource::<PersonalPortal>()
            .add_message::<RiftgateRequest>()
            .add_message::<GateTravel>()
            .add_message::<OpenPortal>()
            .add_message::<RiftgateLock>()
            .add_systems(Update, riftgate_locks.before(bind_riftgates))
            .add_systems(
                Update,
                (
                    init_travel.run_if(not(resource_exists::<TravelState>)),
                    bind_respawners.run_if(resource_exists::<TravelState>),
                    bind_riftgates
                        .in_set(RiftgateStage::Bind)
                        .run_if(resource_exists::<TravelState>),
                    hud_portal,
                    select_rifts,
                    use_dynamic_rift,
                    use_gate,
                    travel_requests.in_set(RiftgateStage::Travel),
                    sync_portals.in_set(RiftgateStage::Portals),
                )
                    .chain()
                    .after(gd_game_core::player::PlayerSet),
            );
    }
}

#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RiftgateStage {
    Bind,
    Travel,
    Portals,
}

/// A Chthonic rift (`DynamicTeleporter`, `level art/riftgates/dynrift_*`).
/// - `DynamicTeleporter::Load`: `activeEffect` / `activeSound` / `dormantEffect` / `dormatSound`
///   (sic; stock records name it `dormantSound`, so a dormant rift is silent), `radius`, `active`
///   (state 4, else 2), `showOnMinimap`, `activationTime`, `resetTargetLevel`. A grouped rift that
///   `GameEngine::IsUniqueEntitySelected(rift, 1)` does not select is state 1: no effect, not of
///   interest, unusable.
/// - The choice (`crate::unique`): one random member of the rift's `Any Entity` group per session,
///   leaving out the rifts the player arrived at.
/// - `UpdateBoundingBox`: the pick box is a cube of half size `radius`; no mesh, only its effect.
/// - `RequestToUse` (state 4): a random child group of the first group containing the rift, a random
///   member of it (remembered as an arrival); the player teleports there.
///
/// PORT: the picks use the port's RNG; `activationTime` (0 in stock data) is not modelled.
#[derive(Component, Clone, Debug)]
pub struct DynamicRift {
    pub record: String,
    pub uid: Uid,
    /// The pick box's half size.
    pub radius: f32,
    /// `None` until [`select_rifts`] decided; `Some(true)`: state 4.
    pub active: Option<bool>,
}

/// `DynamicTeleporter::RequestToUse`'s destination for the rift `uid`, remembered as an arrival.
pub fn rift_destination(unique: &mut crate::unique::UniqueEntities, uid: &Uid) -> Option<Vec3> {
    let groups = unique.groups.0.clone();
    let Some(g) = groups.containing(uid).next() else {
        warn!("Player used ungrouped DynamicTeleporter!");
        return None;
    };
    if g.links.is_empty() {
        warn!("Player used DynamicTeleporter with no children!");
        return None;
    }
    let id = g.links[gd_game_core::rng::usize(0..g.links.len())];
    let child = groups.by_id(&id)?;
    if child.entries.is_empty() {
        return None;
    }
    let to = &child.entries[gd_game_core::rng::usize(0..child.entries.len())];
    unique.selection.arrivals.insert(to.uid);
    to.world.map(Vec3::from)
}

/// Spawns a rift at `pos`; the game adds its look and clicks.
pub fn spawn_dynamic_rift(
    commands: &mut Commands,
    rec: &gd_formats::arz::Record,
    record: &str,
    pos: Vec3,
    uid: Uid,
) -> Entity {
    let radius = rec.f32("radius").unwrap_or(0.0);
    commands
        .spawn((
            Transform::from_translation(pos),
            GlobalTransform::from_translation(pos),
            DynamicRift {
                record: record.to_ascii_lowercase(),
                uid,
                radius,
                active: None,
            },
            crate::player_orders::Interactable::fixed(radius.min(2.0)),
            Name::new("dynamic rift"),
        ))
        .id()
}

/// A grouped rift is active only when chosen (`IsUniqueEntitySelected(rift, true)`).
pub fn select_rifts(
    mut unique: crate::unique::UniqueEntities,
    data: Res<gd_game_core::data::GameData>,
    mut rifts: Query<&mut DynamicRift>,
) {
    for mut r in &mut rifts {
        if r.active.is_some() {
            continue;
        }
        let on = data.db.get(&r.record).is_some_and(|rec| rec.bool("active"));
        let grouped = unique.groups.0.containing(&r.uid).next().is_some();
        let selected = !grouped || unique.is_selected(&r.uid, true);
        r.active = Some(on && selected);
        debug!("dynamic rift {:?}: active {}", r.uid, on && selected);
    }
}

pub fn use_dynamic_rift(
    rifts: Query<(&GlobalTransform, &DynamicRift)>,
    mut players: Query<
        (
            &Transform,
            &mut MovePath,
            &mut crate::player_orders::InteractTarget,
            Option<&gd_game_core::combat::Combatant>,
        ),
        With<Player>,
    >,
    mut unique: crate::unique::UniqueEntities,
    mut travel: MessageWriter<GateTravel>,
    data: Res<gd_game_core::data::GameData>,
) {
    let Ok((ptf, mut path, mut interact, cb)) = players.single_mut() else {
        return;
    };
    let Some(e) = interact.0 else { return };
    let Ok((gt, rift)) = rifts.get(e) else { return };
    if rift.active != Some(true) {
        interact.0 = None;
        return;
    }
    // The fixed actors' use range, with the engine's defaults for the missing `actorRadius` and
    // `usableRange`.
    let reach = gd_game_core::combat::fixed_use_range(
        data.db.get(&rift.record),
        cb.map_or(0.5, |c| c.radius),
        path.points.is_empty(),
    );
    if ptf.translation.distance(gt.translation()) > reach {
        return;
    }
    interact.0 = None;
    path.points.clear();
    if let Some(to) = rift_destination(&mut unique, &rift.uid) {
        info!("dynamic rift {:?} -> {to}", rift.uid);
        travel.write(GateTravel(to));
    }
}
