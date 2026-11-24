//! Riftgates (`StaticTeleporter`) and respawn points (`StaticRespawner`).
//!
//! * Respawn points bind by proximity (`BindingInteractable::UpdateSelf`:
//!   characters within `bindRadius`): `Player::AddRespawnUID`
//!   adds the uid to the difficulty's list and makes it the
//!   current one, where the player comes back after dying.
//! * Riftgates bind by proximity too ([`bind_riftgates`]),
//!   `BindingInteractable::UpdateSelf` with `StaticTeleporter::
//!   IsBindingEnabled` (the gate is inactive and not locked)
//!   and `BindToCharacter` (a player: tutorial page 9 (`crate::tutorials`), the gate
//!   turns active, `Player::AddTeleportUID` adds it to the
//!   difficulty's discovered list; the only caller besides the difficulty
//!   unlock item). `bindRadius` is 8 m in 73 of the 86 gate records
//!   (`BindingInteractable::Load`). Using a gate does not discover it.
//! * Lock (`StaticTeleporter.locked`): the record's `locked` (10 gates), then
//!   the scripts' `SetLocked` ([`RiftgateLock`]). A locked gate
//!   is not of interest (`IsOfInterest`), doesn't bind and
//!   ignores use (`RequestToUse` sends nothing). The lock is not
//!   in the save: `SaveState` / `RestoreState` stream it only
//!   for the in-session entity restoration (no entity state in the
//!   character's files); a load gets it back from the gate's `onAddToWorld`
//!   script, which unlocks it when the player has the quest token (e.g.
//!   `devilscrossingwightmireriftgate.lua`, `WIGHTMIRE_RIFTGATE_SECURED`).
//!   Look (`SetActive`): `lockedEffect` / `lockedSound` while
//!   locked, else `activeEffect` / `activeSound` once discovered (also
//!   `InitialUpdate`: `IsActiveForMainPlayer` = in the difficulty's list),
//!   else `dormantEffect` / `dormatSound`.
//! * Using a gate opens the riftgate map ([`RiftgateMap`]; the game sends
//!   event 28 to the exe, whose handler `RiftgateMap::HandleOpenEvent` stores the gate's object
//!   id in the map widget and shows the map window in its world-map mode).
//! * Discovered gates/respawn points come from the save (`player.gdc` blocks
//!   6 / 5, per difficulty) and go back into it
//!   (`save.rs`). Their places come from the map's instance groups, not the
//!   levels: `Grim Dawn.exe` riftgate map `RiftgateMap::BuildDestinations` (each
//!   discovered gate and shrine uid → `InstanceGroupManager::GetEntityData`
//!   → `World::GetRegionById`: the region's offset + the member's position,
//!   the region's zone / shrine record) and `Player::GetClosestRespawnPoint`
//!   (the current respawn uid the same way). The map-wide
//!   [`WaypointIndex`] holds those members (every gate, respawn point and
//!   shrine of the stock world is one; a uid in no group has no place). A
//!   shrine's group position may differ from its placed entity (one stock
//!   shrine by 3 m); the map shows the group's.
//!
//! A gate is used like every fixed actor (`combat::fixed_use_range`: the player's radius + its
//! `Actor::GetRadius` + its `usableRange`, +1 m at the walk's end). `StaticTeleporter::Load` reads
//! `usableRange` (default 1; 2 m in 75 of the 86 gate records), `bindRadius` (default 5) and `radius`
//! (default 5: [`Waypoint::radius`], not the actor's radius); no gate record has an `actorRadius`, so
//! `Actor::Load`'s default 1 gives it a radius of sqrt(3) m. Its name is its `description`
//! tag (the map label, `StaticTeleporter::AppendDetailMapData` →
//! `GetGameDescription`); the travel target is the chosen gate's position,
//! refused within 4 m of the player (GrimDawn.exe map teleport
//! `RiftgateMap::Teleport` → `GameEngine::InitiatePlayerTeleport`; the list here
//! leaves out the gate in use). The engine's teleport activity
//! (`InstallTeleportActivity`) puts the player on the floor at that point
//! ([`crate::nav::teleport_landing`], run by [`crate::doors::run_teleport`]).
//!
//! Travel from the map, `Grim Dawn.exe` riftgate map mouse handler
//! `RiftgateMap::HandleMouseEvent` (release on a gate / portal icon) and `RiftgateMap::Travel`:
//! - (checked first) away from the gate or portal in use (the one that opened
//!   the map; farther than 8 m, or it is gone) the player's own portal is no
//!   destination, and travel needs `MainPlayerCanUsePersonalTeleport`
//!   ([`can_use_portal`]), else `tagRiftgateError03`; when it can, a personal
//!   portal opens where the player stands (`CreateFixedItemTeleport`, see
//!   below) and the travel goes on;
//! - the gate / portal in use as a destination is refused with
//!   `tagRiftgateError02`;
//! - otherwise a gate destination becomes the last used gate
//!   (`GameEngine::SetLastUsedTeleportId`; a portal has no uid) and the
//!   player teleports to it (refused within 4 m, see above). No cost.
//! - PORT: the map closes on travel (the real one closes with the teleport's
//!   loading); it doesn't close by walking away (no range check found in the
//!   map's update `RiftgateMap::Update`).
//!
//! Personal portals ([`PersonalPortal`], `FixedItemTeleport` with
//! `staticPortal` false), Game.dll:
//! - `MainPlayerCanUsePersonalTeleport`: refused where the
//!   player stands in a sector of the riftgate sector layer (10), when the
//!   `gameengine.dbr` `personalTeleportToken` is set and the player lacks it
//!   ([`portal_unlocked`]; empty in stock data), and without a discovered
//!   riftgate in the difficulty;
//! - `CreateFixedItemTeleport` → `CreateFixedItemTeleportNetHook`:
//!   the engine's TeleportInfo list holds (object id, owner,
//!   coords, area name tag `Engine::GetAreaNameTag`) per portal; a new
//!   portal of a player replaces the player's old one (one per player), and
//!   `records/level art/riftgates/riftgate_personal.dbr` ([`PORTAL_RECORD`])
//!   is spawned at the coords if that level is loaded. The coords are
//!   `Player::GetNearSpawnCoords` ([`near_spawn`]):
//!   `NavManager::FillPointSet(player, 16 m, five spots of radius 2)`
//!   (Engine.dll, `RequestAndBlockNearPoint` per spot,
//!   [`gd_sim::pattern::fill_point_set`]): each spot starts at a random angle
//!   (`rand()`) on rings 0, 4.1, 8.2, ... m out and takes the first walkable
//!   one clear of the spots before it, so spot 0 is the player's own and
//!   spots 1..4 lie on the 4.1 m ring (farther where it is blocked). The
//!   portal goes to the first of spots 1..4 the player can walk to
//!   (`Player::CanMoveTo`, a path); no set (a spot found no
//!   place) or none reachable: the player's spot. So it opens about 4 m from
//!   the player in a random direction;
//! - using the portal (`FixedItemTeleport::RequestToUse`) opens
//!   the riftgate map with the portal in use (event 28, like a gate); someone
//!   else's portal says `tagRiftgateError01` (no other players here). On the
//!   map the portal is an icon of its region's zone (see `ui::riftmap`)
//!   whose travel goes back to the portal's spot;
//! - it stays until the player opens another one or leaves the game
//!   (`GameEngine::ExitPlayingMode` clears the list; it is not saved: no
//!   stream of it; nothing destroys it on use or zone change). An entity no
//!   longer in the list closes and is removed once 250 ms have passed
//!   (`FixedItemTeleport::UpdateSelf`, frames counted 1..100 ms).
//!   PORT: the entity is kept while its level is unloaded (the engine spawns
//!   it only into a loaded level).
//! - the HUD teleport button (`Grim Dawn.exe` `UIPlayerHud::OnToolbarButton`, update
//!   `UIPlayerHud::Update`): `hudTeleportButton` shows while the player can use a
//!   portal and has none, `hudActiveTeleportButton` while the player has
//!   one, neither otherwise ([`portal_button`]); pressing it (when
//!   [`can_use_portal`]) opens a portal ([`OpenPortal`]); then both are
//!   disabled for 1000 ms. It does not open the map: the player uses the
//!   portal for that. While the game menu / options windows are open
//!   (the update's `UIPlayerHud.gameMenuWindow` / `UIPlayerHud.optionsWindow`, the windows that pause the
//!   game; the map and other windows don't count), the open-portal button is
//!   hidden and the no-portal one stays drawn but takes no press or key
//!   (`UIPlayerHud::Update`; applied in `ui::view` and
//!   `ui::riftmap`). PORT: the tutorial
//!   page (10) isn't unlocked.
//!
//! The game shows the gates (`grimdawn::riftgates`: effects, clicks) and the
//! map (`grimdawn::ui::riftmap`), and builds the map-wide [`WaypointIndex`].
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
    /// The record's `bindRadius` (0 when absent).
    pub bind_radius: f32,
    pub level: usize,
    /// `StaticTeleporter.locked`: the record's `locked`, then the scripts'
    /// `SetLocked` ([`RiftgateLock`]).
    pub locked: bool,
}

impl Waypoint {
    /// `StaticTeleporter::IsOfInterest`: not locked.
    pub fn is_of_interest(&self) -> bool {
        !self.locked
    }
}

/// A script's `riftgate:SetLocked` on the gate of `record` at `at` (the
/// game resolves the story object, as for shrines).
#[derive(Message, Clone, Debug)]
pub struct RiftgateLock {
    pub record: String,
    pub at: Vec3,
    pub locked: bool,
}

/// Applies [`RiftgateLock`]s, remembered for gates streamed in later. Runs its scan only when a lock
/// arrives or a gate streams in.
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
    /// The last riftgate travelled to (`GameEngine::SetLastUsedTeleportId`;
    /// the map centres on it when no discovered gate is near).
    pub last_used: Option<Uid>,
    /// Riftgates discovered on other difficulties this session (the campaign's difficulty unlock item,
    /// `crate::consumables`): (difficulty, uid), merged into the save's lists.
    pub unlocked: Vec<(usize, Uid)>,
}

/// The world map's riftgates: the entries of its `Riftgates` instance groups (`InstanceGroupManager::
/// GetGroupsOfType("Riftgates")`, the stock map's "World Riftgates", 72 gates), set by the game with the map.
#[derive(Resource, Default)]
pub struct RiftgateGroup(pub Vec<Uid>);

/// One riftgate, respawn point or devotion shrine of the map.
#[derive(Clone, Debug, PartialEq)]
pub struct IndexedPoint {
    pub pos: Vec3,
    /// Its record (lower case).
    pub record: String,
    /// Its region's map record: the zone record (`Region::GetZoneRecord`) for
    /// gates, the shrine icon record (`Region::GetShrineRecord`) for shrines.
    pub map_record: String,
}

/// Every riftgate / respawn point / devotion shrine of the map by uid: the
/// map's instance-group members with such a record (module doc).
#[derive(Resource, Default)]
pub struct WaypointIndex(pub HashMap<Uid, IndexedPoint>);

/// Within this distance of the gate in use the map travels straight from it
/// (`RiftgateMap::Travel`: `<= 8`).
pub const AT_GATE: f32 = 8.0;

/// The personal portal's record (`CreateFixedItemTeleportNetHook`).
pub const PORTAL_RECORD: &str = "records/level art/riftgates/riftgate_personal.dbr";

/// The HUD teleport buttons are disabled this long after opening a portal
/// (`Grim Dawn.exe` `UIPlayerHud::OnToolbarButton` / `UIPlayerHud::Update`: 1000 ms).
pub const BUTTON_WAIT: f32 = 1.0;

/// A riftgate or the personal portal (by its object id): what opened the map,
/// or a travel destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rift {
    Gate(Uid),
    Portal(u32),
}

/// The riftgate map window: open, the gate / portal that opened it, and
/// whether the player is still at it (within [`AT_GATE`]; kept by
/// [`travel_requests`]).
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct RiftgateMap {
    pub open: bool,
    pub gate: Option<Rift>,
    pub at_gate: bool,
}

/// The player's personal portal: the engine's TeleportInfo entry (module doc).
#[derive(Clone, Debug, PartialEq)]
pub struct PortalInfo {
    /// Its object id (a new one per portal).
    pub id: u32,
    pub pos: Vec3,
    /// The region it opened in (the player's: the sector lookups of its area
    /// name read that region's level, not whichever level covers `pos`).
    pub region: Option<usize>,
    /// The area name tag where it was opened (`Engine::GetAreaNameTag`); set
    /// by the game, which has the map's sector names (`None` until then).
    pub area_tag: Option<String>,
}

/// The personal portal of the session (none at the start: not saved).
#[derive(Resource, Default, Debug)]
pub struct PersonalPortal {
    pub portal: Option<PortalInfo>,
    /// Seconds the HUD teleport buttons stay disabled ([`BUTTON_WAIT`]).
    pub button_wait: f32,
    last_id: u32,
}

impl PersonalPortal {
    /// Opens a portal at `pos`, replacing the player's old one.
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

    /// The portal's position if `id` is the current one.
    pub fn pos(&self, id: u32) -> Option<Vec3> {
        self.portal.as_ref().filter(|p| p.id == id).map(|p| p.pos)
    }
}

/// A personal portal entity (`riftgate_personal.dbr`; the game dresses it).
#[derive(Component, Debug)]
pub struct Portal {
    pub id: u32,
    /// Milliseconds since it left the list (it closes, then goes at 250).
    pub closing: Option<u32>,
}

impl Portal {
    /// `FixedItemTeleport::IsOfInterest`: `FixedItemTeleport.closing` clear or `FixedItemTeleport.closed` set
    /// (PORT: read as not closing).
    pub fn is_of_interest(&self) -> bool {
        self.closing.is_none()
    }
}

/// The HUD teleport button was pressed.
#[derive(Message, Clone, Copy, Debug, Default)]
pub struct OpenPortal;

/// The HUD teleport button shown: `active` = `hudActiveTeleportButton` (the
/// player has a portal), else `hudTeleportButton`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortalButton {
    pub active: bool,
    pub disabled: bool,
}

/// `MainPlayerCanUsePersonalTeleport` (module doc) for a player at `pos` in
/// `region` (the no-portal sector is read from that region's level,
/// `Player::UpdateSelf`; `None`: whichever level covers `pos`); `unlocked` is
/// [`portal_unlocked`].
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

/// The `gameengine.dbr` `personalTeleportToken` check of
/// `MainPlayerCanUsePersonalTeleport` (when the field
/// is set, the player needs that story token; empty in stock data, and
/// unlocked without a story).
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

/// Which HUD teleport button shows (module doc); `None`: neither.
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

/// Where a portal opens for a player at `tf`: `Player::GetNearSpawnCoords` (module doc).
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

/// `GetNearSpawnCoords`' spot radius (its five-float vector of 2.0) and `FillPointSet` distance (16.0).
const NEAR_SPAWN_RADIUS: f32 = 2.0;
const NEAR_SPAWN_DIST: f32 = 16.0;

/// What the map window asks for.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub enum RiftgateRequest {
    Travel(Rift),
    Close,
}

/// A travel to `0` was granted (the game moves the player there).
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
    // Option: `state` is the existence check (this system creates it); `loaded` is absent for a new character.
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

/// The first respawn point within the player's reach (`radius`: the respawner's `bindRadius`,
/// default 8) becomes current.
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

/// Riftgates bind (are discovered) while the player is within their
/// `bindRadius`, unless locked (module doc).
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

/// Within reach of the clicked gate or personal portal: open the map with it
/// in use (a locked gate ignores the use, module doc). Other targets (a Chthonic rift, the Shattered Realm's exit
/// gate or buff shrine) are their own systems' and stay set while the player walks to them.
#[allow(clippy::too_many_arguments)]
pub fn use_gate(
    hooks: Res<gd_game_core::hooks::Hooks>,
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
        // The portal (`FixedItemTeleport`: `actorRadius` 0, no `usableRange`, so 1 m beyond the player's radius).
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
    if !hooks.gameplay.use_riftgate {
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

/// The map's requests (rules in the module doc) and its "at the gate" flag.
#[allow(clippy::too_many_arguments)]
pub fn travel_requests(
    mut reqs: MessageReader<RiftgateRequest>,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    mut map: ResMut<RiftgateMap>,
    // Option: the map flag and the open map's close requests are handled without travel state.
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
    let Some(mut state) = state else { return };
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

/// The HUD teleport button: opens a portal where the player stands when
/// allowed and the button isn't waiting (module doc).
#[allow(clippy::too_many_arguments)]
pub fn hud_portal(
    mut reqs: MessageReader<OpenPortal>,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    mut portal: ResMut<PersonalPortal>,
    // Option: the button timer runs and the requests are drained without travel state.
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

/// Keeps the portal entity in step with [`PersonalPortal`]: spawns the
/// current one, closes and removes the others (module doc).
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

/// The riftgate pipeline: `Bind` ties gates to the player, `Travel` handles travel requests, `Portals` syncs portals.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RiftgateStage {
    Bind,
    Travel,
    Portals,
}

/// A Chthonic rift (`DynamicTeleporter`, `level art/riftgates/dynrift_*`: the rifts of the Void, Ashen Waste,
/// Bastion of Chaos and Malmouth outskirts). Game.dll:
/// - `DynamicTeleporter::Load`: `activeEffect` / `activeSound` / `dormantEffect` / `dormatSound`
///   (spelled so; stock records name it `dormantSound`, so a dormant rift is silent), `radius`,
///   `active` (state 4, else 2: every stock record is active), `showOnMinimap` (unset in stock: no icon,
///   `AppendDetailMapData`), `activationTime`, `resetTargetLevel`. A rift in an instance group
///   that `GameEngine::IsUniqueEntitySelected(rift, 1)` does not select is state 1: no effect
///   (`InitialUpdate` starts the active / dormant look only for states 4 / 2), not of interest
///   (`IsOfInterest`: state 4) and unusable.
/// - The choice (`crate::unique`) falls on a rift's `Any Entity` group (each stock rift is in one; the last
///   group holding it when none is a unique group): one random member per group name and session, leaving
///   out the members a rift sent the player to ([`crate::unique::UniqueSelection::arrivals`]), so the rift
///   the player arrives at is not the active one.
/// - `UpdateBoundingBox`: the pick box is a cube of half size `radius` around the rift (15 m for
///   `dynrift_02`); it has no mesh, only its effect.
/// - `RequestToUse` (state 4, the controlling player): the first group containing the rift, a
///   random one of its child groups (the links: `Rift_..._toX` ↔ `Rift_..._fromX` pairs), a random member of
///   that group (selected or not) is remembered as an arrival (`GameEngine.riftArrivals`) and the player teleports
///   to its position (`InitiatePlayerTeleport`). An ungrouped rift logs "Player used ungrouped
///   DynamicTeleporter!".
///
/// PORT: the picks use the port's RNG; `activationTime` (0 in stock data) is not modelled. A rift is used like
/// every fixed actor (`DynamicTeleporter::Load` → `FixedItem::Load`; `combat::fixed_use_range`).
#[derive(Component, Clone, Debug)]
pub struct DynamicRift {
    pub record: String,
    pub uid: Uid,
    /// The pick box's half size (`radius`).
    pub radius: f32,
    /// `None` until [`select_rifts`] has decided; `Some(true)`: state 4 (usable, active look).
    pub active: Option<bool>,
}

/// `DynamicTeleporter::RequestToUse`'s destination for the rift `uid` (struct doc), remembered as an
/// arrival in the unique-entity choice ([`crate::unique::UniqueSelection::arrivals`]).
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

/// Spawns a rift at `pos` (its look and clicks: the game).
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

/// New rifts take their state (`DynamicTeleporter::Load`, [`DynamicRift`]): a grouped rift is active only
/// when chosen (`IsUniqueEntitySelected(rift, true)`, `crate::unique`).
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

/// The clicked active rift in reach sends the player on ([`rift_destination`], [`GateTravel`]).
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
    // A `DynamicTeleporter` is a `FixedItem`: the fixed actors' use range, with the engine's defaults for its
    // missing `actorRadius` and `usableRange`.
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
