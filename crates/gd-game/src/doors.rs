//! Doors (`FixedDoor`, `FixedItemDoor`), story levers (`FixedLever`) and dungeon entrances
//! (`DungeonEntrance`) as gameplay: open state, the nav obstacles they put (following their open / close
//! animation), story locks and `SetDoor`, and the use of the interacted door ([`use_door`]) including
//! travel ([`run_teleport`]).
//!
//! Use range (`ControllerPlayerState::DefaultRequestInteractableAction`): 3D origin distance at most player
//! radius + object radius + `usableRange`, 1 m more once the walk to it has ended
//! (`ControllerPlayerStateMoveToFixedItem::EndOfPathReached`).
//!
//! Travel (`DungeonEntrance::RequestToUse`, [`destination`]): the groups searched are the map's instance
//! groups holding the door, or, when the first "Dungeon Entrances" group holding it has child groups, those
//! children. Every other member of a searched "Dungeon Entrances" group whose region exists is a candidate,
//! drawn with `rand()` (here `gd_game_core::rng`). The pair is remembered both ways ([`DungeonExits`],
//! `Player::AddDungeonEntranceUID`): a door with a remembered other end goes there only, and a door without
//! one never draws a member another door already took (`HasDungeonEntranceUID`). `Player.dungeonExits` is
//! never saved, so it lasts the session. The use sets [`crate::teleport::PendingTravel`].
//!
//! Interest ([`Door::is_of_interest`]): the mouse pick keeps a fixed actor only when `IsOfInterest` holds.
//! `FixedDoor` / `FixedLever::IsOfInterest`: (not `locked`, or `lockedTag` set) and `FixedDoor::CanInteract`
//! by `interaction` ("Open": only fully closed; "Close": only fully open; else always); a `FixedDoor` must
//! also be visible. `FixedItemDoorBase::IsOfInterest`: (not `openOnly`, or closed) and (not `locked`, or
//! `lockedTextTag` set). So a locked door without a locked text is never hovered.
//!
//! Story doors take their story object's open / locked state when they spawn ([`story_doors`]). A dungeon
//! entrance opens on its first use and travels on the next ([`Door::use_action`]).
//!
//! Level doors keep their state across streaming ([`DoorStates`], `Level::SaveState` / `RestoreState`):
//! `FixedDoor::SaveState` writes open for an opening or open door, closed for an `autoClose` one;
//! `RestoreState` sets it fully open or closed at once. The lock is saved too, but only story doors change
//! it, and they follow their story object.
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
            .init_resource::<DoorStates>()
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
            )
            .add_systems(
                PostUpdate,
                restore_doors.run_if(resource_exists::<GameData>),
            )
            .add_systems(Last, remember_doors.before(crate::levels::despawn_unloaded));
    }
}

/// The open state of the level doors whose level streamed out this session, by (level, uid).
#[derive(Resource, Default)]
pub struct DoorStates(pub HashMap<(usize, [u32; 4]), bool>);

/// `FixedDoor::SaveState` as a door's level unloads: an `autoClose` door is saved closed.
pub fn remember_doors(
    mut states: ResMut<DoorStates>,
    doors: Query<(&Door, &gd_game_core::levels::LevelMember), With<gd_game_core::levels::Unload>>,
) {
    for (d, m) in &doors {
        if let Some(uid) = d.uid {
            states
                .0
                .insert((m.0, uid), d.open && d.auto_close.is_none());
        }
    }
}

/// `FixedDoor::RestoreState`: a level door spawned again takes its remembered state at once.
pub fn restore_doors(
    mut commands: Commands,
    states: Res<DoorStates>,
    data: Res<GameData>,
    mut anims: ResMut<AnimCache>,
    mut doors: Query<(Entity, &mut Door, &gd_game_core::levels::LevelMember), Added<Door>>,
) {
    if states.0.is_empty() {
        return;
    }
    for (e, mut d, m) in &mut doors {
        let Some(&open) = d.uid.and_then(|uid| states.0.get(&(m.0, uid))) else {
            continue;
        };
        if d.open == open {
            continue;
        }
        let Some(rec) = data.db.get(&d.record) else {
            continue;
        };
        let d = &mut *d;
        set_open(&mut commands, &mut anims, &data, e, d, rec, open);
        // At once: the animation at its end (an item door's callbacks run to it in `advance_doors`).
        if let Some(a) = d.anim.as_mut() {
            a.time = a.anim.num_frames as f32 / a.anim.frame_rate.max(1) as f32;
        }
    }
}

/// The pending travel's destination has streamed in (sent every frame it holds).
#[derive(Message, Clone, Copy, Debug)]
pub struct DestinationReady;

/// Runs the teleport activity of a pending travel ([`TeleportActivity`]): the player stays frozen (its walk
/// cleared every frame) while the screen fades out and the destination loads, then it is put on the floor
/// there ([`crate::nav::teleport_landing`]; no floor: the point itself, as `TranslateToFloor` leaves it).
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
    /// `Actor::GetRadius`.
    pub radius: f32,
    pub story: Option<u32>,
    pub obstacles: Vec<Obstacle>,
    /// `SetLocked`: the player cannot open or close it.
    pub locked: bool,
    /// A `FixedItemDoor`; else a `FixedDoor` / `DungeonEntrance`.
    pub item_door: bool,
    /// `RemoveObstaclesWhileOpen`.
    pub remove_while_open: bool,
    /// The last open animation has an "Obstacle" callback point.
    pub obstacle_cb: bool,
    pub msh: Arc<Msh>,
    /// The open / close animation and its clock.
    pub anim: Option<DoorAnim>,
    /// `FixedItemDoor` callbacks still due: "Obstacle" points after frame `done` of an opening (`true`) or
    /// closing animation, then "End".
    pub watch: Option<(bool, f32)>,
    /// `autoClose` with its `autoClosePeriod` (ms) of a `FixedDoor` / `DungeonEntrance`.
    pub auto_close: Option<i32>,
    /// `FixedDoor.autoCloseDelay` (ms).
    pub close_in_ms: i32,
}

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

/// Starts the door's open or close animation.
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

/// Whether [`spawn_door`]'s mesh and animations for `record` are parsed; otherwise their background load
/// starts.
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

/// Spawns a door of `record` (its `mesh` is needed), resting at the end of its open or close animation.
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
    // `FixedItemDoorBase::Load` reads no `open`.
    let open = !item_door && rec.bool("open");
    // Only the mesh hit boxes block (`FixedDoor::CreatePathObstacles`). A closed `FixedItemDoor` is posed
    // at its close animation's end.
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
    // A closed `FixedDoor` rests at its close animation's end too (`FixedDoor::InitialUpdate` →
    // `InstantaneousClose`: `AnimEntry::PoseLastFrame`), unless a `closedLoopAnimation` is set, which it
    // plays instead (PORT: not played). The Shattered Realm portal's disc is shrunk to nothing there.
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
    // None: a story object's door.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoorUse {
    Toggle,
    Travel,
}

impl Door {
    /// `DungeonEntrance::RequestToUse`: after the range, faction and lock checks, a fully closed entrance
    /// takes the plain door use (it opens); otherwise the player travels. Other doors always toggle.
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

/// Opens or closes a door: item doors' obstacles follow the animation's callbacks; plain doors block only
/// while closed.
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

/// `FixedDoor::Open` / `Close`: a `FixedDoor` (and a `DungeonEntrance`) plays its `openSound` /
/// `closeSound` when it starts opening / closing; the portal's `openLoopSound` is never read. A door that
/// spawns in its state plays nothing. PORT: `FixedItemDoor`s (not traced) stay silent.
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

/// Runs the doors' animation clocks and an item door's "Obstacle" and "End" callbacks.
pub fn advance_doors(
    mut commands: Commands,
    time: Res<Time>,
    mut doors: Query<(Entity, &Transform, &mut Door)>,
) {
    let dt = time.delta_secs();
    for (e, tf, mut door) in &mut doors {
        // At rest with no callback due: left untouched, so `Changed<Door>` skips it.
        if door.watch.is_none()
            && door.anim.as_ref().is_none_or(|a| {
                a.time >= a.anim.num_frames as f32 / a.anim.frame_rate.max(1) as f32
            })
        {
            continue;
        }
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

/// `FixedDoor::UpdateSelf`: a fully open `autoClose` door counts down `autoClosePeriod` × 1000 ms and
/// closes; a 0 period never closes. `FixedItemDoor`s' `autoClose` is another rule.
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

/// Script doors: `SetDoor` / `SetLocked`. The story object's state (`WorldObject::open` / `locked`) is the
/// source: a door spawned after a script already opened or locked it takes that state when it appears.
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
            // `FixedDoor::SetAutoClose`: the flag with the record's `autoClosePeriod`; an open door arms its
            // countdown at once. PORT: item doors share the flag (`FixedItemDoorBase`'s rule not traced).
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

/// The dungeon entrances whose other end the player took this session, both ways (`Player.dungeonExits`).
/// The engine keys uids by the wrapping sum of their four words; no two stock entrance uids share a sum,
/// so whole uids key it here.
#[derive(Resource, Default)]
pub struct DungeonExits(pub HashMap<[u32; 4], [u32; 4]>);

/// Uses the player's interact target once in range: the Shattered Realm's portals, dungeon-entrance
/// travel, `onInteract`, `openOnly`, the locked feedback ([`LockedUse`]), else open / close.
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
    // Option: without a story doors still open and close.
    rt: Option<NonSend<StoryRuntime>>,
    activity: Res<crate::teleport::TeleportActivity>,
) {
    let Ok((ptf, mut path, mut interact, pcb, ch)) = players.single_mut() else {
        debug!("doors: no player");
        return;
    };
    // Frozen through a teleport: use the door once it ends (a travel granted while another is pending
    // would be lost).
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
    // `EndlessDungeonEntrance::RequestToUse`: a closed one opens like any door; an open one (the hub door
    // given to the generator, or an `EndlessDungeonExit`) teleports to the active floor's entrance and
    // dispels the soul buffs.
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
    // None found: the use ends there (`RequestToUse` installs no teleport).
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
    // `onInteract` runs first on every use; `true` ends the use (`FixedDoor::RequestToUse`).
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
        // `Player::PlayLockedDoorVox`.
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
    // A story door's object follows (scripts' `IsOpen`; a respawned door takes it).
    if let (Some(id), Some(rt)) = (door.story, rt.as_ref())
        && let Some(o) = rt.story.borrow_mut().entities.get_mut(id)
    {
        o.open = open;
    }
}

/// The other end of the entrance with entity uid `uid` (`DungeonEntrance::RequestToUse`), remembered in
/// `exits` both ways.
pub fn destination(
    groups: &gd_formats::map::sector::InstanceGroups,
    regions: &[gd_formats::map::RegionInfo],
    exits: &mut DungeonExits,
    uid: [u32; 4],
) -> Option<Vec3> {
    const ENTRANCES: &str = "Dungeon Entrances";
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
    // The engine calls `rand()` only for more than one candidate.
    let e = match candidates.len() {
        1 => candidates[0],
        n => candidates[gd_game_core::rng::usize(..n)],
    };
    exits.0.insert(uid, e.uid);
    exits.0.insert(e.uid, uid);
    Some(gd_game_core::levels::region_origin(region(e)?) + Vec3::from(e.floats))
}

/// The game's door pick runs before `Use`, the teleport activity after it.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DoorStage {
    Advance,
    Use,
}
