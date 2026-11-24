//! Doors (`FixedDoor`, `FixedItemDoor`), story levers (`FixedLever`) and
//! dungeon entrances (`DungeonEntrance`) as gameplay: open state, the
//! obstacles they put on the nav (following their open / close animation,
//! which runs on its own clock here), story locks and `SetDoor`, and the use
//! of the door the player walks to (the interact order, `player_orders::InteractTarget`; [`use_door`]: range,
//! story `onInteract` hooks, locked feedback, open / close, dungeon-entrance
//! travel, the teleport activity: [`run_teleport`]). The game dresses them (model, the same animation),
//! draws the loading screen and streams the destination (`grimdawn::doors`).
//!
//! Use range: `ControllerPlayerState::DefaultRequestInteractableAction`:
//! used once the 3D distance between the player's and the
//! object's origins is at most player radius + object radius + `usableRange`
//! (`FixedActor::GetUsableRange`; both radii are `Actor::GetRadius`,
//! the same vtable slot on the player and the object: `combat::get_radius`, kept
//! in [`Door::radius`]; a missing `usableRange` is 1 m, `combat::fixed_usable_range`),
//! 1 m more once the walk to it has ended
//! (`ControllerPlayerStateMoveToFixedItem::EndOfPathReached`).
//!
//! Travel (`FixedDoor` use, `DungeonEntrance::RequestToUse`, [`destination`]): the groups searched are the map's
//! instance
//! groups holding the door, or, when the first "Dungeon Entrances" group holding it has child groups (random
//! cave set pieces), those children. Every other member of a searched "Dungeon Entrances" group whose region
//! exists is a candidate; the engine draws one with `rand()`, the port with `gd_game_core::rng` (the gameplay dice).
//! An entry is (uid, region id, region-local position). The pair is remembered in both directions
//! ([`DungeonExits`], `Player::AddDungeonEntranceUID`): a door with a remembered other end goes
//! there only (`GetDungeonExitForEntranceUID`), and a door without one never draws a member
//! another door already took (`HasDungeonEntranceUID`). The memory is the `Player` object's
//! map (`Player.dungeonExits`): nothing saves or loads it, only the constructor / destructor touch it, so it lasts the
//! session (`grimdawn/src/exit.rs` resets it). The use sets [`crate::teleport::PendingTravel`];
//! [`run_teleport`] runs the teleport activity of every pending travel and
//! puts the player there once the destination has streamed in (the game's
//! streaming reports it with [`DestinationReady`]).
//!
//! Interest ([`Door::is_of_interest`]): the mouse pick (Grim Dawn.exe
//! `InGameUIActorCapture::CollectCandidates`) keeps a fixed actor only when its `IsOfInterest`
//! holds, so a door that fails it is neither outlined, named nor
//! clicked. `FixedDoor::IsOfInterest` (also `DungeonEntrance`, a
//! `FixedDoor` subclass) / `FixedLever::IsOfInterest`: (not `locked`, or
//! `lockedTag` set) and `FixedDoor::CanInteract` by `interaction` ("Open":
//! only fully closed, state 0; "Close": only fully open, state 3; else
//! always); a `FixedDoor` must also be visible.
//! `FixedItemDoorBase::IsOfInterest`: (not `openOnly`, or closed, state 10)
//! and (not `locked`, or `lockedTextTag` set). `locked` is the live flag
//! (record, then `SetLocked`). So a locked door without a locked text, like
//! the Devil's Crossing prison gate a script opens, is never hovered.
//!
//! Story doors take their story object's open / locked state when they
//! spawn ([`story_doors`]). A dungeon entrance opens on its first use and
//! travels on the next ([`Door::use_action`]).
use crate::chests::LockedUse;
use crate::movement::MovePath;
use crate::nav::NavObstacle;
use crate::teleport::{PendingTravel, TeleportActivity, TeleportStep};
use bevy::prelude::*;
use gd_formats::anm::{self, Animation};
use gd_formats::collections::HashMap;
use gd_formats::msh::Mesh as Msh;
use gd_game_core::character::AnimCache;
use gd_game_core::creature::BodyCache;
use gd_game_core::data::GameData;
use gd_game_core::player::Player;
use gd_game_core::save::MapLayout;
use gd_game_core::story::StoryRuntime;
use gd_sim::nav::Obstacle;
use std::sync::Arc;

pub struct DoorsPlugin;

impl Plugin for DoorsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DungeonExits>()
            .init_resource::<PendingTravel>()
            .init_resource::<TeleportActivity>()
            .add_message::<DestinationReady>()
            .add_systems(
                Update,
                (
                    advance_doors.in_set(DoorStage::Advance),
                    auto_close.after(advance_doors),
                    story_doors,
                    (use_door.in_set(DoorStage::Use), run_teleport)
                        .chain()
                        .after(gd_game_core::player::PlayerSet),
                ),
            );
    }
}

/// The pending travel's destination has streamed in (sent by the game's level streaming every frame it holds; the
/// teleport activity reads it while loading, [`TeleportActivity::step`]).
#[derive(Message, Clone, Copy, Debug)]
pub struct DestinationReady;

/// Runs the teleport activity of a pending travel ([`TeleportActivity`], every travel: doors,
/// riftgates, portals): the player stays frozen where it is (`Character.walkPath`: the walk is cleared every frame)
/// while the screen fades out and the destination loads ([`DestinationReady`]), then it is put on the floor there
/// ([`crate::nav::teleport_landing`]; where the loaded destination has no floor, the point itself, as
/// `TranslateToFloor` leaves it) and the screen fades in; the travel ends with the activity. The loading screen
/// and the destination's streaming follow the activity (grimdawn `doors.rs`).
pub fn run_teleport(
    time: Res<Time>,
    mut pending: ResMut<PendingTravel>,
    mut activity: ResMut<TeleportActivity>,
    mut ready: MessageReader<DestinationReady>,
    nav: Res<crate::nav::Nav>,
    mut players: Query<(&mut Transform, &mut MovePath), With<Player>>,
    mut carry: Local<f32>,
) {
    let ready = ready.read().count() > 0;
    let Some(dest) = pending.0 else { return };
    let Ok((mut tf, mut path)) = players.single_mut() else {
        return;
    };
    if !activity.active() {
        info!("doors: teleport to {dest:?}");
        activity.start();
    }
    path.points.clear();
    let dt = gd_game_core::session::frame_ms(&mut carry, &time).max(0) as u32;
    match activity.step(dt, ready) {
        TeleportStep::Move => {
            let p = crate::nav::teleport_landing(&nav.0, dest).unwrap_or(dest);
            tf.translation = p;
            info!("doors: arrived at {p:?}");
        }
        TeleportStep::Done => pending.0 = None,
        TeleportStep::Load | TeleportStep::Wait => {}
    }
}

#[derive(Component)]
pub struct Door {
    pub record: String,
    pub uid: Option<[u32; 4]>,
    pub open: bool,
    /// `Actor::GetRadius` (`combat::get_radius`): the use range's and the hover pick's.
    pub radius: f32,
    pub story: Option<u32>,
    /// What it blocks while closed.
    pub obstacles: Vec<Obstacle>,
    /// Locked by a script (`SetLocked`): the player cannot open or close it.
    pub locked: bool,
    /// A `FixedItemDoor` (module docs); else a `FixedDoor` / `DungeonEntrance`.
    pub item_door: bool,
    /// `RemoveObstaclesWhileOpen`.
    pub remove_while_open: bool,
    /// The last open animation has an "Obstacle" callback point.
    pub obstacle_cb: bool,
    pub msh: Arc<Msh>,
    /// The open / close animation and its clock (the game shows the same).
    pub anim: Option<DoorAnim>,
    /// `FixedItemDoor` callbacks still due: "Obstacle" points after frame
    /// `done` of an opening (`true`) or closing animation, then "End".
    pub watch: Option<(bool, f32)>,
    /// `autoClose` with its `autoClosePeriod` (ms) of a `FixedDoor` /
    /// `DungeonEntrance` ([`auto_close`]).
    pub auto_close: Option<i32>,
    /// Milliseconds before an auto-closing door closes (`FixedDoor.autoCloseDelay`).
    pub close_in_ms: i32,
}

/// A door's playing animation.
#[derive(Clone)]
pub struct DoorAnim {
    pub anim: Arc<Animation>,
    pub time: f32,
    pub speed: f32,
    /// Bumped each time an animation (re)starts.
    pub generation: u32,
}

impl DoorAnim {
    fn frame(&self) -> f32 {
        (self.time * self.anim.frame_rate as f32).min(last_frame(&self.anim))
    }
}

fn last_frame(an: &Animation) -> f32 {
    an.num_frames.saturating_sub(1) as f32
}

fn posed_obstacles(msh: &Arc<Msh>, tf: &Transform, an: &Animation, frame: f32) -> Vec<Obstacle> {
    let mut abs = Vec::new();
    anm::pose_mesh_abs_into(msh, an, frame, false, &mut abs);
    crate::obstacles::posed_hit_box_obstacles(msh, tf, &abs)
}

fn put_obstacles(commands: &mut Commands, e: Entity, obstacles: Vec<Obstacle>) {
    if obstacles.is_empty() {
        commands.entity(e).try_remove::<NavObstacle>();
    } else {
        commands.entity(e).try_insert(NavObstacle(obstacles));
    }
}

/// Starts the door's open or close animation (at its end: already there).
fn set_anim(
    door: &mut Door,
    anims: &mut AnimCache,
    data: &GameData,
    rec: &gd_formats::arz::Record,
    open: bool,
    at_end: bool,
) {
    let (k, sk) = if open {
        ("openAnimation", "openAnimationSpeed")
    } else {
        ("closeAnimation", "closeAnimationSpeed")
    };
    if let Some(an) = rec.str(k).and_then(|p| anims.anim(data, p)) {
        let time = if at_end {
            an.num_frames as f32 / an.frame_rate.max(1) as f32
        } else {
            0.0
        };
        let generation = door.anim.as_ref().map_or(0, |a| a.generation + 1);
        door.anim = Some(DoorAnim {
            anim: an,
            time,
            speed: rec.f32(sk).unwrap_or(1.0),
            generation,
        });
    }
}

/// Whether everything [`spawn_door`] reads for `record` is parsed: its mesh and its open / close animations (one not
/// yet parsed starts its background load, [`BodyCache::ready`], [`AnimCache::ready`]).
pub fn door_ready(
    data: &GameData,
    bodies: &mut BodyCache,
    anims: &mut AnimCache,
    record: &str,
) -> bool {
    let Some(rec) = data.db.get(record) else {
        return true;
    };
    let mut ready = rec.str("mesh").is_none_or(|m| bodies.ready(data, m));
    for k in ["openAnimation", "closeAnimation"] {
        if let Some(p) = rec.str(k).filter(|p| !p.is_empty()) {
            ready &= anims.ready(data, p);
        }
    }
    ready
}

/// Spawns a door of `record` (its `mesh` is needed); open doors rest at the
/// end of their open animation, closed ones at the end of their close
/// animation (item doors with the obstacles of that pose).
#[allow(clippy::too_many_arguments)]
pub fn spawn_door(
    commands: &mut Commands,
    data: &GameData,
    bodies: &mut BodyCache,
    anims: &mut AnimCache,
    tf: Transform,
    record: &str,
    uid: Option<[u32; 4]>,
    level: Option<usize>,
    story: Option<u32>,
) -> Option<Entity> {
    let rec = data.db.get(record)?;
    let msh = bodies.get(data, rec.str("mesh")?)?;
    let item_door = rec.str("Class") == Some("FixedItemDoor");
    // `FixedItemDoorBase::Load` reads no `open` (module docs).
    let open = !item_door && rec.bool("open");
    // Only the mesh hit boxes block; a mesh without any doesn't (`FixedDoor::CreatePathObstacles`,
    // `FixedItem::CreatePathObstacle`).
    // A closed `FixedItemDoor` is posed at its close animation's end.
    let close_anim = if item_door {
        rec.str("closeAnimation").and_then(|p| anims.anim(data, p))
    } else {
        None
    };
    let obstacles = match &close_anim {
        Some(an) => posed_obstacles(&msh, &tf, an, last_frame(an)),
        None => crate::obstacles::hit_box_obstacles(&msh, &tf),
    };
    let mut door = Door {
        record: record.to_owned(),
        uid,
        open,
        radius: gd_game_core::combat::get_radius(Some(rec), gd_game_core::combat::ACTOR_RADIUS),
        story,
        obstacles: obstacles.clone(),
        locked: rec.bool("locked"),
        item_door,
        remove_while_open: rec.bool("RemoveObstaclesWhileOpen"),
        obstacle_cb: false,
        msh,
        anim: None,
        watch: None,
        auto_close: (!item_door && rec.bool("autoClose"))
            .then(|| (rec.f32("autoClosePeriod").unwrap_or(0.0) * 1000.0) as i32),
        close_in_ms: 0,
    };
    door.close_in_ms = door.auto_close.unwrap_or(0);
    // A closed `FixedDoor` / `DungeonEntrance` rests at its close
    // animation's end too (`FixedDoor::Load` flags a
    // closed door, `FixedDoor::InitialUpdate` then calls
    // `InstantaneousClose`, which poses the close animation at
    // its last frame (`AnimEntry::PoseLastFrame`) unless a `closedLoopAnimation` is set,
    // which it plays instead: PORT, not played, 2 stock records). The
    // Shattered Realm portal's disc is shrunk to nothing there, so a dormant
    // portal draws nothing. Its obstacles stay the hit boxes.
    let closed_loop = rec
        .str("closedLoopAnimation")
        .is_some_and(|s| !s.is_empty());
    if open {
        set_anim(&mut door, anims, data, rec, true, true);
    } else if close_anim.is_some() || !closed_loop {
        set_anim(&mut door, anims, data, rec, false, true);
    }
    let interact = crate::player_orders::Interactable::fixed(door.radius);
    let e = commands
        .spawn((
            tf,
            GlobalTransform::from(tf),
            door,
            interact,
            Name::new(format!("door {}", record.rsplit('/').next().unwrap_or(""))),
        ))
        .id();
    // A level's door goes with its level (None: a story object's).
    if let Some(l) = level {
        commands
            .entity(e)
            .insert(gd_game_core::levels::LevelMember(l));
    }
    if !open {
        put_obstacles(commands, e, obstacles);
    }
    Some(e)
}

/// What using a door does ([`Door::use_action`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoorUse {
    /// The door's own use: `onInteract`, locked feedback, open / close.
    Toggle,
    /// A dungeon entrance's travel to its other end.
    Travel,
}

impl Door {
    /// `DungeonEntrance::RequestToUse` (the class
    /// overrides `FixedDoor`'s): after the range, faction and lock checks
    /// (locked: the locked feedback), a fully closed entrance (`IsClosed`,
    /// state 0) takes the plain door use (`FixedDoor::RequestToUse`: it
    /// opens); otherwise (opening or open) the player travels. So a trap
    /// door opens on the first use and goes down on the second. Other doors
    /// always take their own use.
    pub fn use_action(&self, rec: &gd_formats::arz::Record) -> DoorUse {
        if rec.str("Class") != Some("DungeonEntrance")
            || self.locked
            || (!self.open && self.settled())
        {
            DoorUse::Toggle
        } else {
            DoorUse::Travel
        }
    }

    /// The open / close animation has ended (or there is none).
    pub fn settled(&self) -> bool {
        self.anim
            .as_ref()
            .is_none_or(|a| a.time * a.anim.frame_rate.max(1) as f32 >= a.anim.num_frames as f32)
    }

    /// Hoverable and clickable (module docs, "Interest").
    pub fn is_of_interest(&self, rec: &gd_formats::arz::Record) -> bool {
        let nonempty = |k: &str| rec.str(k).is_some_and(|t| !t.is_empty());
        if self.item_door {
            return (!rec.bool("openOnly") || !self.open)
                && (!self.locked || nonempty("lockedTextTag"));
        }
        if self.locked && !nonempty("lockedTag") {
            return false;
        }
        match rec.str("interaction").unwrap_or("") {
            "Open" => !self.open && self.settled(),
            "Close" => self.open && self.settled(),
            _ => true,
        }
    }
}

/// Opens or closes a door (module docs: item doors' obstacles follow the
/// animation's callbacks; plain doors block only while closed).
pub fn set_open(
    commands: &mut Commands,
    anims: &mut AnimCache,
    data: &GameData,
    e: Entity,
    door: &mut Door,
    rec: &gd_formats::arz::Record,
    open: bool,
) {
    door.open = open;
    if open && let Some(p) = door.auto_close {
        door.close_in_ms = p;
    }
    set_anim(door, anims, data, rec, open, false);
    if door.item_door {
        if open {
            door.obstacle_cb = door.anim.as_ref().is_some_and(|a| {
                a.anim
                    .callback_points()
                    .iter()
                    .any(|c| c.name.eq_ignore_ascii_case("Obstacle"))
            });
            if !door.obstacle_cb {
                put_obstacles(commands, e, Vec::new());
            }
        }
        door.watch = door.anim.is_some().then_some((open, -1.0));
        return;
    }
    if open {
        commands.entity(e).try_remove::<NavObstacle>();
    } else if !door.obstacles.is_empty() {
        commands
            .entity(e)
            .try_insert(NavObstacle(door.obstacles.clone()));
    }
}

/// `FixedDoor::Open` / `Close`: a `FixedDoor` (and so a `DungeonEntrance`, the
/// Shattered Realm's portal) plays its `openSound` / `closeSound` at its coords when it starts opening / closing
/// (`Load`; its loop sounds are `openedLoopSound` / `closedLoopSound`, so the portal's
/// `openLoopSound` is never read). A door that spawns in its state plays nothing. PORT: `FixedItemDoor`s
/// (their own `FixedItemDoorBase` rules, not traced) stay silent.
fn door_sound(
    door: &Door,
    rec: &gd_formats::arz::Record,
    open: bool,
    at: Vec3,
) -> Option<gd_game_core::visuals::PlaySound> {
    if door.item_door {
        return None;
    }
    let pak = rec
        .str(if open { "openSound" } else { "closeSound" })
        .filter(|s| !s.is_empty())?;
    Some(gd_game_core::visuals::PlaySound {
        pak: pak.to_owned(),
        at: Some(at),
    })
}

/// Runs the doors' animation clocks and an item door's callbacks: its
/// "Obstacle" points and its "End" (module docs).
pub fn advance_doors(
    mut commands: Commands,
    time: Res<Time>,
    mut doors: Query<(Entity, &Transform, &mut Door)>,
) {
    let dt = time.delta_secs();
    for (e, tf, mut door) in &mut doors {
        let door = &mut *door;
        let Some(a) = door.anim.as_mut() else {
            continue;
        };
        let end = a.anim.num_frames as f32 / a.anim.frame_rate.max(1) as f32;
        if a.time < end {
            a.time = (a.time + dt * a.speed).min(end);
        }
        let Some((open, done)) = door.watch else {
            continue;
        };
        let (anim, frame) = (a.anim.clone(), a.frame());
        let last = last_frame(&anim);
        let mut points: Vec<f32> = anim
            .callback_points()
            .iter()
            .filter(|c| c.name.eq_ignore_ascii_case("Obstacle"))
            .map(|c| c.frame as f32)
            .filter(|&f| f > done && f <= frame)
            .collect();
        points.sort_by(f32::total_cmp);
        for f in points {
            if open {
                put_obstacles(&mut commands, e, Vec::new());
            } else {
                put_obstacles(&mut commands, e, posed_obstacles(&door.msh, tf, &anim, f));
            }
        }
        if frame < last {
            door.watch = Some((open, frame));
            continue;
        }
        door.watch = None;
        let rebuild = !door.obstacle_cb && !(open && door.remove_while_open);
        if rebuild {
            let o = posed_obstacles(&door.msh, tf, &anim, last);
            info!(
                "doors: {} obstacles rebuilt ({} pose, {} boxes)",
                door.record.rsplit('/').next().unwrap_or(""),
                if open { "open" } else { "closed" },
                o.len()
            );
            put_obstacles(&mut commands, e, o);
        }
    }
}

/// `FixedDoor::UpdateSelf`: an `autoClose` door that
/// is fully open (state 3) counts its timer down (set to `autoClosePeriod` ×
/// 1000 ms by `FixedDoor::Open` / `Load`) and closes when it runs out
/// (`Close`); a 0 period never closes. `FixedItemDoor`s'
/// `autoClose` is another rule (`FixedItemDoorBase`), not this one.
pub fn auto_close(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut anims: ResMut<AnimCache>,
    mut doors: Query<(Entity, &GlobalTransform, &mut Door)>,
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
    mut carry: Local<f32>,
) {
    let ms = gd_game_core::session::frame_ms(&mut carry, &time);
    for (e, gt, mut door) in &mut doors {
        if door.auto_close.is_none() || !door.open || !door.settled() || door.close_in_ms <= 0 {
            continue;
        }
        door.close_in_ms -= ms;
        if door.close_in_ms <= 0
            && let Some(rec) = data.db.get(&door.record)
        {
            info!(
                "doors: {} closes by itself",
                door.record.rsplit('/').next().unwrap_or("")
            );
            set_open(&mut commands, &mut anims, &data, e, &mut door, rec, false);
            sound.write_batch(door_sound(&door, rec, false, gt.translation()));
        }
    }
}

/// Script doors: spawned by the story (`story_world::sync_actors`); `SetDoor` /
/// `SetLocked`. The story object's state is the source (`WorldObject::open`
/// / `locked`, from the record and the scripts' `Open` / `Close` /
/// `SetLocked`): a door spawned after a script already opened or locked it
/// (the Devil's Crossing prison gate's trigger fires as its level streams
/// in, before the door's model is up) takes that state when it appears.
pub fn story_doors(
    mut commands: Commands,
    mut events: MessageReader<gd_game_core::story::StoryEffect>,
    mut doors: Query<(Entity, &GlobalTransform, Mut<Door>)>,
    data: Res<GameData>,
    mut anims: ResMut<AnimCache>,
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
) {
    if let Some(rt) = rt.as_ref() {
        let story = rt.story.borrow();
        for (e, _, mut d) in &mut doors {
            if !d.is_added() {
                continue;
            }
            let Some(o) = d.story.and_then(|id| story.entities.get(id)) else {
                continue;
            };
            let (open, locked) = (o.open, o.locked);
            if d.locked != locked {
                d.locked = locked;
            }
            if d.open != open
                && let Some(rec) = data.db.get(&d.record)
            {
                debug!(
                    "doors: {} spawns {}",
                    d.record.rsplit('/').next().unwrap_or(""),
                    if open { "opening" } else { "closing" }
                );
                set_open(&mut commands, &mut anims, &data, e, &mut d, rec, open);
            }
        }
    }
    for ev in events.read() {
        match ev.0 {
            gd_story::story::Effect::SetLocked { id, locked } => {
                for (_, _, mut d) in &mut doors {
                    if d.story == Some(id) {
                        d.locked = locked;
                    }
                }
            }
            gd_story::story::Effect::SetDoor { id, open } => {
                for (e, gt, mut d) in &mut doors {
                    if d.story == Some(id)
                        && d.open != open
                        && let Some(rec) = data.db.get(&d.record)
                    {
                        set_open(&mut commands, &mut anims, &data, e, &mut d, rec, open);
                        sound.write_batch(door_sound(&d, rec, open, gt.translation()));
                    }
                }
            }
            // `Door:SetAutoClose(bool)` (`FixedDoor::SetAutoClose`, Game.dll `0x1802a6790`): the flag
            // with the record's `autoClosePeriod`; an open door arms its countdown at once (the
            // [`auto_close`] tick closes it). PORT: the port's item doors share the flag (the engine's
            // `FixedItemDoorBase` auto-close is another rule, not traced).
            gd_story::story::Effect::SetAutoClose { id, on } => {
                for (_, _, mut d) in &mut doors {
                    if d.story == Some(id) {
                        d.auto_close = on.then(|| {
                            (data
                                .db
                                .get(&d.record)
                                .and_then(|r| r.f32("autoClosePeriod"))
                                .unwrap_or(0.0)
                                * 1000.0) as i32
                        });
                        if on && d.open {
                            d.close_in_ms = d.auto_close.unwrap_or(0);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// The dungeon entrances whose other end the player took this session, both ways: entrance uid → exit uid and
/// exit uid → entrance uid (module docs, "Travel"; the `Player`'s map `Player.dungeonExits`, filled by
/// `Player::AddDungeonEntranceUID`). The engine's map orders and matches uids by the wrapping sum
/// of their four words; no two stock dungeon-entrance uids share a sum (`world001.map`), so whole uids key it here.
#[derive(Resource, Default)]
pub struct DungeonExits(pub HashMap<[u32; 4], [u32; 4]>);

/// Uses the player's interact target (`player_orders::InteractTarget`) once in range (module docs): the Shattered Realm's portals,
/// dungeon-entrance travel, `onInteract`, `openOnly`, the locked feedback ([`LockedUse`]), else open /
/// close (a story door's object follows).
#[allow(clippy::too_many_arguments)]
pub fn use_door(
    mut commands: Commands,
    mut doors: Query<(&GlobalTransform, &mut Door)>,
    mut players: Query<
        (
            &Transform,
            &mut MovePath,
            &mut crate::player_orders::InteractTarget,
            &gd_game_core::combat::Combatant,
            &gd_game_core::character::Character,
        ),
        With<Player>,
    >,
    mut exits: ResMut<DungeonExits>,
    // Option: exists only once a map is open; without it an entrance does not travel.
    layout: Option<Res<MapLayout>>,
    mut pending: ResMut<PendingTravel>,
    data: Res<GameData>,
    mut anims: ResMut<AnimCache>,
    mut locked_out: MessageWriter<LockedUse>,
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
    // Option: without a story (no session yet) doors still open and close.
    rt: Option<NonSend<StoryRuntime>>,
    activity: Res<crate::teleport::TeleportActivity>,
) {
    let Ok((ptf, mut path, mut interact, pcb, ch)) = players.single_mut() else {
        debug!("doors: no player");
        return;
    };
    // The player is frozen through a teleport's activity (`run_teleport`): the door is used once it ends,
    // not acted on now (a travel granted while another is pending would be lost).
    if activity.active() {
        return;
    }
    let Some(e) = interact.0 else { return };
    let Ok((gt, mut door)) = doors.get_mut(e) else {
        debug!("doors: target {e:?} not a door");
        return;
    };
    let Some(rec) = data.db.get(&door.record) else {
        debug!("doors: no record {}", door.record);
        return;
    };
    // Use range (module docs, `player_orders::usable_range`): player radius + object radius + `usableRange`,
    // measured between the origins in 3D, 1 m more at the end of the walk there.
    let range = gd_game_core::player_orders::usable_range(
        pcb.radius,
        door.radius,
        gd_game_core::combat::fixed_usable_range(Some(rec)),
        path.points.is_empty(),
    );
    let dist = ptf.translation.distance(gt.translation());
    if dist > range {
        debug!("doors: {dist:.1} m > {range:.1} m");
        return;
    }
    interact.0 = None;
    path.points.clear();
    // The Shattered Realm's doors (`EndlessDungeonEntrance::RequestToUse`): a closed one
    // opens like any door; an open one teleports to the active floor's entrance and dispels the soul buffs, when
    // it is the hub door the script gave the generator or an `EndlessDungeonExit`.
    if door.open
        && matches!(
            rec.class.as_str(),
            "EndlessDungeonEntrance" | "EndlessDungeonExit"
        )
        && let Some(rt) = rt.as_ref()
    {
        let mut story = rt.story.borrow_mut();
        let ok = rec.class == "EndlessDungeonExit"
            || (door.story.is_some() && door.story == story.endless.entrance_door);
        let to = story
            .endless
            .dungeon
            .as_ref()
            .and_then(|d| d.floors.get(d.active))
            .and_then(|f| f.entrance.as_ref())
            .map(|e| Vec3::from(e.pos));
        if ok && let Some(to) = to {
            info!("doors: endless entrance -> {to:?}");
            story
                .effects
                .push(gd_story::story::Effect::RemoveEndlessSoulBuffs);
            pending.0.get_or_insert(to);
            return;
        }
    }
    // An open dungeon entrance travels to its other end (none found: the use ends there, as the engine's
    // `RequestToUse` installs no teleport); a closed one opens (`Door::use_action`).
    if door.use_action(rec) == DoorUse::Travel
        && let (Some(uid), Some(layout)) = (door.uid, &layout)
    {
        match destination(
            &layout.0.instance_groups,
            &layout.0.regions,
            &mut exits,
            uid,
        ) {
            Some(dest) => {
                info!(
                    "doors: {} -> {dest:?}",
                    door.record.rsplit('/').next().unwrap_or("")
                );
                pending.0.get_or_insert(dest);
            }
            None => debug!("doors: {} leads nowhere", door.record),
        }
        return;
    }
    // `onInteract` runs first on every use; a `true` result ends the use
    // (`FixedDoor::RequestToUse` / `FixedLever::RequestToUse`, see
    // `ScriptHost::interact_hook`).
    if let Some(id) = door.story
        && let Some(rt) = rt.as_ref()
        && rec.str("onInteract").is_some_and(|h| !h.is_empty())
    {
        info!(
            "doors: {} used (onInteract)",
            door.record.rsplit('/').next().unwrap_or("")
        );
        if rt.host.interact_hook(id) {
            return;
        }
    }
    if door.open && rec.bool("openOnly") {
        return;
    }
    if door.locked {
        locked_out.write(LockedUse {
            record: door.record.clone(),
            at: gt.translation(),
        });
        // The player's locked-door voice (`char_sounds::locked_vox`, `Player::PlayLockedDoorVox`).
        if let Some(pak) =
            crate::char_sounds::locked_vox(&data, ch, crate::char_sounds::LockedKind::Door)
        {
            sound.write(gd_game_core::visuals::PlaySound {
                pak,
                at: Some(ptf.translation),
            });
        }
        return;
    }
    let open = !door.open;
    info!(
        "doors: {} {}",
        door.record.rsplit('/').next().unwrap_or(""),
        if open { "opened" } else { "closed" }
    );
    set_open(&mut commands, &mut anims, &data, e, &mut door, rec, open);
    sound.write_batch(door_sound(&door, rec, open, gt.translation()));
    // A story door's object follows (scripts' `IsOpen`; the state a respawned
    // door takes, [`story_doors`]).
    if let (Some(id), Some(rt)) = (door.story, rt.as_ref())
        && let Some(o) = rt.story.borrow_mut().entities.get_mut(id)
    {
        o.open = open;
    }
}

/// The other end of the entrance with entity uid `uid` (module docs, "Travel"; `DungeonEntrance::RequestToUse`),
/// remembered in `exits` both ways. The draw is gameplay randomness (`gd_game_core::rng`, the
/// engine's `rand()`), so a seeded run lands at the same exit.
pub fn destination(
    groups: &gd_formats::map::sector::InstanceGroups,
    regions: &[gd_formats::map::RegionInfo],
    exits: &mut DungeonExits,
    uid: [u32; 4],
) -> Option<Vec3> {
    const ENTRANCES: &str = "Dungeon Entrances";
    // The groups holding the door, or the children of the first entrance group holding it that has some.
    let searched: Vec<&gd_formats::map::sector::InstanceGroup> = match groups
        .containing(&uid)
        .find(|g| g.kind == ENTRANCES && !g.links.is_empty())
    {
        Some(g) => g.links.iter().filter_map(|id| groups.by_id(id)).collect(),
        None => groups.containing(&uid).collect(),
    };
    let remembered = exits.0.get(&uid).copied();
    let region = |e: &gd_formats::map::sector::InstanceGroupEntry| {
        let rid = e.ints.map(|i| i as u32);
        regions.iter().find(|r| r.id == rid)
    };
    let mut candidates: Vec<&gd_formats::map::sector::InstanceGroupEntry> = Vec::new();
    for e in searched
        .into_iter()
        .filter(|g| g.kind == ENTRANCES)
        .flat_map(|g| &g.entries)
    {
        if e.uid == uid || region(e).is_none() || candidates.iter().any(|c| c.uid == e.uid) {
            continue;
        }
        match remembered {
            Some(exit) if e.uid == exit => {
                candidates.push(e);
                break;
            }
            Some(_) => {}
            None if !exits.0.contains_key(&e.uid) => candidates.push(e),
            None => {}
        }
    }
    if candidates.is_empty() {
        return None;
    }
    // One candidate is taken without a draw (the engine calls `rand()` only for more than one).
    let e = match candidates.len() {
        1 => candidates[0],
        n => candidates[gd_game_core::rng::usize(..n)],
    };
    exits.0.insert(uid, e.uid);
    exits.0.insert(e.uid, uid);
    Some(gd_game_core::levels::region_origin(region(e)?) + Vec3::from(e.floats))
}

/// Door stages: what shows doors orders after `Advance`; the game's door pick runs before `Use`, the
/// teleport activity after it.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DoorStage {
    Advance,
    Use,
}
