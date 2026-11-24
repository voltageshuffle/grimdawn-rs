//! Doors, presentation side: their models (posed by the door's animation, which `gd_game::doors` runs and which
//! also places the obstacles), locked feedback, and what a pending travel's teleport activity
//! (`gd_game::doors::run_teleport`) shows and streams ([`report_destination`], [`travel_screen`]). A click is the
//! player's interact order (`gd_game::player_orders`); the use itself, its range, `onInteract` hooks and the
//! travel destination are `gd_game::doors::use_door`.
//!
//! Open effects, `DungeonEntrance` (Game.dll; the Shattered Realm
//! hub portal's swirl, its floor runes and their lights): the record's
//! `openEffect` / `openEffect2` are created at load at `openEffect[2]AttachPoint`
//! (a mesh attach point, following its bone), not emitting; `Open`
//! (`DungeonEntrance::Open`) starts each after its `openEffect[2]Delay` ms (or at once
//! when 0), `Close` (`DungeonEntrance::Close`) stops both emitting; a door that is open
//! at load emits from its `InitialUpdate`. Here each start spawns the effect
//! and a stop lets it finish and go ([`open_effects`]).
//!
//! Landing: the game hands the destination (cast to integers) to
//! `GameEngine::InstallTeleportActivity`, whose activity puts
//! the player on the floor under it ([`gd_game::nav::teleport_landing`], all
//! travels: riftgates, portals, the Shattered Realm's doors and gates).
#[cfg(feature = "dev")]
use crate::player::MovePath;
use bevy::prelude::*;
pub use gd_game::chests::LockedUse;
pub use gd_game::doors::Door;
#[cfg(feature = "dev")]
use gd_game::nav::Nav;
pub use gd_game::teleport::PendingTravel;
use gd_render::character::CharAssets;
use gd_render::model::Animator;

pub struct DoorsPlugin;

impl Plugin for DoorsPlugin {
    fn build(&self, app: &mut App) {
        app.register_required_components::<Door, gd_game_core::world::DoorMarker>()
            .register_required_components::<Door, Visibility>()
            .add_systems(
                Update,
                (
                    #[cfg(feature = "dev")]
                    test_use
                        .after(gd_game_core::player::PlayerSet)
                        .before(gd_game::doors::DoorStage::Use),
                    report_destination.before(gd_game::doors::run_teleport),
                    travel_screen.after(gd_game::doors::run_teleport),
                ),
            )
            .add_systems(
                Update,
                (
                    locked_feedback,
                    dress_doors.after(gd_game::doors::DoorStage::Advance),
                    (open_effects, follow_door_fx)
                        .chain()
                        .after(gd_game::doors::DoorStage::Advance),
                ),
            );
    }
}

fn locked_feedback(
    mut commands: Commands,
    mut events: MessageReader<LockedUse>,
    data: Res<gd_game_core::data::GameData>,
    mut sounds: ResMut<gd_audio::Sounds>,
    mut notify: MessageWriter<gd_game_core::story::Notify>,
) {
    for ev in events.read() {
        let Some(rec) = data.db.get(&ev.record) else {
            continue;
        };
        info!("locked: {}", ev.record.rsplit('/').next().unwrap_or(""));
        if let Some(snd) = rec.str("lockedSound").filter(|s| !s.is_empty()) {
            gd_audio::play_pak(&mut commands, &data, &mut sounds, snd, Some(ev.at));
        }
        if let Some(tag) = rec
            .str("lockedTag")
            .or(rec.str("lockedTextTag"))
            .filter(|s| !s.is_empty())
        {
            notify.write(gd_game_core::story::Notify(tag.to_owned()));
        }
    }
}

/// The teleport's loading screen (`ui::loading::spawn_cover`), faded by [`travel_screen`].
#[derive(Component)]
struct TravelCover;

/// Tells the teleport activity that the pending travel's destination has streamed in
/// ([`gd_game::doors::DestinationReady`]: every level around it is in, [`gd_world::StreamedMap::area_ready`]; always
/// in the test arena, which has no streaming).
fn report_destination(
    pending: Res<PendingTravel>,
    world: Option<Res<gd_world::StreamedMap>>,
    levels: Option<Res<gd_game_core::levels::Levels>>,
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut ready: MessageWriter<gd_game::doors::DestinationReady>,
) {
    let Some(dest) = pending.0 else { return };
    let loaded = hooks.session.testworld.is_some()
        || match (&world, &levels) {
            (Some(w), Some(l)) => w.area_ready(l, dest),
            _ => true,
        };
    if loaded {
        ready.write(gd_game::doors::DestinationReady);
    }
}

/// What the teleport activity ([`gd_game::teleport::TeleportActivity`]) shows and streams: the loading screen while it
/// runs, at the activity's opacity, and the destination's levels from the end of the fade-out to the end
/// ([`gd_world::TravelPreload`]). The cover is the loading screen (`ui::loading`: background, tip; its pieces
/// follow the cover's alpha).
fn travel_screen(
    mut commands: Commands,
    pending: Res<PendingTravel>,
    activity: Res<gd_game::teleport::TeleportActivity>,
    mut preload: ResMut<gd_world::TravelPreload>,
    mut covers: Query<(Entity, &mut BackgroundColor, Option<&Children>), With<TravelCover>>,
    mut texts: Query<&mut TextColor>,
) {
    use gd_game::teleport::TeleportPhase;
    let streaming = matches!(
        activity.phase,
        TeleportPhase::Loading | TeleportPhase::FadeIn
    );
    let want = pending.0.filter(|_| streaming);
    if preload.0 != want {
        preload.0 = want;
    }
    if !activity.active() {
        for (e, ..) in &covers {
            commands.entity(e).try_despawn();
        }
        return;
    }
    let a = activity.cover_alpha();
    if covers.is_empty() {
        let e = gd_ui::loading::spawn_cover(&mut commands);
        commands.entity(e).insert((
            TravelCover,
            DespawnOnExit(gd_ui::state::session::AppState::InGame),
        ));
        return;
    }
    for (_, mut bg, children) in &mut covers {
        bg.0.set_alpha(a);
        for c in children.into_iter().flatten() {
            if let Ok(mut t) = texts.get_mut(*c) {
                t.0.set_alpha(a);
            }
        }
    }
}

/// `GD_USE_DOOR` (hooks.rs): walks to the nearest
/// (matching) door or lever within 20 m and uses it, not before `secs` of play; a list uses one door per
/// text, in order (a Shattered Realm run's portal, its treasure room portal, the treasure room's exit). A
/// later door may be 40 m away and waits until a script unlocks it, and with `GD_OPEN_CHESTS` until the
/// chests in its range are open.
#[allow(clippy::too_many_arguments)]
#[cfg(feature = "dev")]
fn test_use(
    hooks: Res<gd_game_core::hooks::Hooks>,
    time: Res<Time>,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    mut done: Local<usize>,
    nav: Res<Nav>,
    doors: Query<(Entity, &GlobalTransform, &Door)>,
    chests: Query<(Entity, &GlobalTransform, &gd_game::chests::Chest)>,
    mut players: Query<
        (
            &Transform,
            &mut MovePath,
            &mut gd_game::player_orders::InteractTarget,
        ),
        With<gd_game_core::player::Player>,
    >,
) {
    let Some(list) = &hooks.gameplay.use_door else {
        return;
    };
    let Some(want) = list.split(',').nth(*done) else {
        return;
    };
    let (want, after) = match want.rsplit_once('@') {
        Some((w, s)) if s.parse::<f32>().is_ok() => (w, s.parse::<f32>().unwrap_or(0.0)),
        _ => (want, 0.0),
    };
    if time.elapsed_secs() < after {
        return;
    }
    let Ok((tf, mut path, mut interact)) = players.single_mut() else {
        return;
    };
    if *done > 0
        && hooks.gameplay.open_chests
        && (interact.0.is_some_and(|e| chests.get(e).is_ok())
            || chests.iter().any(|(_, gt, c)| {
                !c.opened
                    && !c.locked
                    && gt.translation().distance(tf.translation) < hooks.gameplay.open_chests_range
            }))
    {
        return;
    }
    // `GD_USE_DOOR` text: only records containing it ("1": any).
    let Some((e, at, r)) = doors
        .iter()
        .filter(|(_, _, d)| want == "1" || d.record.to_ascii_lowercase().contains(want))
        // A later door of the list waits until a script unlocks it.
        .filter(|(_, _, d)| *done == 0 || !d.locked)
        .map(|(e, gt, d)| (e, gt.translation(), d.radius))
        .filter(|d| d.1.distance(tf.translation) < if *done == 0 { 20.0 } else { 40.0 })
        .min_by(|a, b| {
            a.1.distance(tf.translation)
                .total_cmp(&b.1.distance(tf.translation))
        })
    else {
        return;
    };
    *done += 1;
    info!(
        "doors: test use of {e:?} at {at:?} from {:?}",
        tf.translation
    );
    interact.0 = Some(e);
    if let Some(p) = gd_game::chests::path_near(&nav.0, &mut scratch, tf.translation, at, r) {
        path.points = p;
    }
}

/// New doors get their model (with the record's texture overrides,
/// `CharAssets::spawn_record_model`) once it is loaded (`CharAssets::record_ready`, in the background); a door's
/// animation (re)start is shown.
fn dress_doors(
    mut commands: Commands,
    new: Query<(Entity, &Door), Without<gd_render::character::RecordDressed>>,
    doors: Query<(Entity, &Door, Option<&ShownAnim>), Changed<Door>>,
    mut a: CharAssets,
) {
    for (e, door) in &new {
        let db = a.data.db.clone();
        let rec = db.get(&door.record);
        if rec.is_some_and(|r| !a.record_ready(r)) {
            continue;
        }
        commands
            .entity(e)
            .try_insert(gd_render::character::RecordDressed);
        if let Some(rec) = rec {
            a.spawn_record_model(&mut commands, e, rec);
        }
    }
    for (e, door, shown) in &doors {
        let Some(an) = &door.anim else { continue };
        if shown.is_some_and(|s| s.0 == an.generation) {
            continue;
        }
        commands.entity(e).insert((
            ShownAnim(an.generation),
            Animator {
                anim: an.anim.clone(),
                time: an.time,
                speed: an.speed,
                looped: false,
            },
        ));
    }
}

/// The `DoorAnim::generation` whose animation is playing.
#[derive(Component)]
struct ShownAnim(u32);

/// A door's `openEffect` / `openEffect2` (module doc): the effects running
/// and the starts still waiting for their delay.
#[derive(Component, Default)]
struct OpenFx {
    open: bool,
    live: Vec<Entity>,
    /// (slot, seconds left).
    pending: Vec<(usize, f32)>,
}

/// A door whose record has no open effect ([`open_effects`]): marked on first sight so its record is not
/// re-read every frame.
#[derive(Component)]
struct NoOpenFx;

/// An open effect following its door's attach point.
#[derive(Component)]
struct DoorFx {
    door: Entity,
    /// The attach point's bone (its joint), if it has one.
    joint: Option<Entity>,
    /// The attach frame in the bone's (or the door's) space.
    local: Transform,
}

const OPEN_FX: [(&str, &str, &str); 2] = [
    ("openEffect", "openEffectAttachPoint", "openEffectDelay"),
    ("openEffect2", "openEffect2AttachPoint", "openEffect2Delay"),
];

fn open_effects(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<gd_game_core::data::GameData>,
    mut doors: Query<
        (
            Entity,
            &Door,
            Option<&mut OpenFx>,
            Option<&gd_render::model::Skeleton>,
        ),
        Without<NoOpenFx>,
    >,
    frames: Query<&GlobalTransform>,
) {
    for (e, door, state, skel) in &mut doors {
        let Some(mut state) = state else {
            // First sight: only doors with open effects are tracked; one
            // open at load starts them at once (`InitialUpdate`). An effect-less
            // door is marked ([`NoOpenFx`]) so its record is read only once.
            let has_fx = data.db.get(&door.record).is_some_and(|rec| {
                OPEN_FX
                    .iter()
                    .any(|(k, _, _)| rec.str(k).is_some_and(|s| !s.is_empty()))
            });
            if !has_fx {
                commands.entity(e).try_insert(NoOpenFx);
                continue;
            }
            commands.entity(e).try_insert(OpenFx {
                open: door.open,
                live: Vec::new(),
                pending: if door.open {
                    vec![(0, 0.0), (1, 0.0)]
                } else {
                    Vec::new()
                },
            });
            continue;
        };
        if door.open != state.open {
            state.open = door.open;
            state.pending.clear();
            if door.open {
                let Some(rec) = data.db.get(&door.record) else {
                    continue;
                };
                for (slot, (_, _, delay)) in OPEN_FX.iter().enumerate() {
                    let ms = rec.f32(delay).unwrap_or(0.0);
                    state.pending.push((slot, ms / 1000.0));
                }
            } else {
                for fx in state.live.drain(..) {
                    commands
                        .entity(fx)
                        .try_insert(gd_game_core::visuals::StopEmitting);
                }
            }
        }
        if state.pending.is_empty() {
            continue;
        }
        let dt = time.delta_secs();
        let mut due = Vec::new();
        state.pending.retain_mut(|(slot, left)| {
            *left -= dt;
            if *left <= 0.0 {
                due.push(*slot);
                false
            } else {
                true
            }
        });
        let Some(rec) = data.db.get(&door.record) else {
            continue;
        };
        for slot in due {
            let (k, attach, _) = OPEN_FX[slot];
            let Some(record) = rec.str(k).filter(|r| !r.is_empty()) else {
                continue;
            };
            let ap = rec.str(attach).and_then(|name| door.msh.attach_point(name));
            let (local, joint) = match &ap {
                Some(ap) => {
                    let c = &ap.coords;
                    let axes = Mat3::from_cols(
                        Vec3::from(c.x).normalize_or(Vec3::X),
                        Vec3::from(c.y).normalize_or(Vec3::Y),
                        Vec3::from(c.z).normalize_or(Vec3::Z),
                    );
                    // Attach frames can be mirrored (`FX_Floor` on the endless rift: y = -Y); a quaternion
                    // cannot hold a mirror, so keep the whole frame (negative scale).
                    let local = Transform::from_matrix(Mat4::from_mat3_translation(
                        axes,
                        Vec3::from(c.origin),
                    ));
                    let joint = ap.parent.as_deref().and_then(|b| {
                        let i = door.msh.bone_index(b)?;
                        skel?.joints.get(i).copied()
                    });
                    (local, joint)
                }
                None => (Transform::IDENTITY, None),
            };
            let mut spec = gd_game_core::visuals::EffectSpec::record(record.to_owned());
            spec.despawn_when_complete = true;
            let at = joint
                .or(Some(e))
                .and_then(|f| frames.get(f).ok())
                .map_or(local, |g| g.mul_transform(local).compute_transform());
            let fx = commands
                .spawn((
                    spec,
                    at,
                    Visibility::default(),
                    DoorFx {
                        door: e,
                        joint,
                        local,
                    },
                ))
                .id();
            state.live.push(fx);
        }
    }
}

/// Open effects follow their attach point (the bone's posed frame); a door
/// that goes takes them with it.
fn follow_door_fx(
    mut commands: Commands,
    mut fx: Query<(Entity, &DoorFx, &mut Transform)>,
    frames: Query<&GlobalTransform>,
) {
    for (e, f, mut tf) in &mut fx {
        let Ok(door) = frames.get(f.door) else {
            commands.entity(e).try_despawn();
            continue;
        };
        let base = f.joint.and_then(|j| frames.get(j).ok()).unwrap_or(door);
        *tf = base.mul_transform(f.local).compute_transform();
    }
}
