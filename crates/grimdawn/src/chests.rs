//! Loot containers (`FixedItemContainer`) and food crates (`FixedItemShrine`, gd-game `chests`): chests placed in levels, chests
//! from proxy accessory pools (`gd_sim::proxy::run_proxy`) and script chests
//! (story objects of class Chest). A click is the player's interact order (`gd_game::player_orders`), which
//! walks over and opens it: the open animation plays and its loot and gold drop around it
//! (`gd_sim::items::loot::container_drops` / `container_level`). Opened chests
//! stay open for the session; the port never writes saves.
//!
//! The chest opens when the player's centre is within player
//! radius + chest radius + `usableRange` (0 when absent) + 1 m of the
//! chest's (`ControllerPlayerStateMoveToFixedItem::EndOfPathReached`,
//! straight-line distance); a chest whose record has `locked`
//! only plays its locked sound until unlocked (`FixedItem::RequestToUse`,
//! `FixedItem::Load` reads `locked` for every fixed item; `SetLock`). Loot
//! is thrown from the chest's drop point (`gd_game::chests`, `gd_game::loot` doc).
//!
//! A chest's model sits on its entity: the level / set-piece / proxy
//! transform is the model root, the skinned parts are drawn from joints
//! posed in world space from it (`model::animate_skeletons`; every stock
//! container mesh's bind box is centred within 0.8 m of its pivot, posed
//! joints within 1.5 m). The hover pick (`hover.rs`, a sphere of `actorRadius` + 0.5 m around the pivot + 0.5 m
//! up) chooses the chest the interact order targets; the use itself is `gd_game::chests::open_chests`.
//!
//! A container's effects (`FixedItemContainer::UpdateSelf`, [`chest_effects`]): the
//! record's `IdleEffect` (the loot chests' glow and twinkle) is created and attached at the container's origin
//! while it is not open; its `LockedEffect` is attached while it is locked and stops emitting where it is when
//! the lock goes (the radius lock, gd-game `chests`). An open container (state 3) counts 1000 ms down (the
//! ctor's timer), then every attached `EffectEntity` stops emitting: the idle effect and the mesh's own
//! `CreateEntity` ones (`fx::spawn_mesh_attachments`: the lore objects' and corpses' glow). One loaded open
//! counts from its first update.
use bevy::prelude::*;
pub use gd_game::chests::{Chest, open_chests};
use gd_render::character::CharAssets;
use gd_render::model::Animator;

pub struct ChestsPlugin;

impl Plugin for ChestsPlugin {
    fn build(&self, app: &mut App) {
        app.register_required_components::<Chest, Visibility>()
            .add_systems(
                Update,
                (
                    dress_chests.after(open_chests),
                    chest_effects.after(dress_chests),
                ),
            );
    }
}

/// New chests get their model (with the record's texture overrides,
/// `CharAssets::spawn_record_model`) and their idle animation (an opened one
/// rests at the end of its open animation) once those are loaded (`CharAssets::record_ready`,
/// `AnimCache::ready`: in the background, so a streamed-in level's chests never load them on the game thread);
/// opening one plays it.
fn dress_chests(
    mut commands: Commands,
    new: Query<(Entity, &Chest), Without<gd_render::character::RecordDressed>>,
    opened: Query<(Entity, &Chest, Option<&ShrineIdleFx>), (Changed<Chest>, Without<ShownOpen>)>,
    mut a: CharAssets,
    mut anims: ResMut<gd_game_core::character::AnimCache>,
) {
    for (e, chest) in &new {
        let db = a.data.db.clone();
        let Some(rec) = db.get(&chest.record) else {
            commands
                .entity(e)
                .try_insert(gd_render::character::RecordDressed);
            continue;
        };
        let (open_anim, open_speed) = open_animation(chest);
        let (anim, speed, looped) = if chest.opened {
            (open_anim, open_speed, false)
        } else {
            ("idleAnimation", "idleAnimationSpeed", true)
        };
        let mut ready = a.record_ready(rec);
        if let Some(p) = rec.str(anim).filter(|p| !p.is_empty()) {
            ready &= anims.ready(&a.data, p);
        }
        if !ready {
            continue;
        }
        commands
            .entity(e)
            .try_insert(gd_render::character::RecordDressed);
        if a.spawn_record_model(&mut commands, e, rec).is_none() {
            continue;
        }
        if chest.opened {
            commands.entity(e).try_insert(ShownOpen);
        }
        if let Some(an) = rec.str(anim).and_then(|p| a.cache.anim(&a.data, p)) {
            // Already open: start at the end of the open animation.
            let time = if chest.opened {
                an.num_frames as f32 / an.frame_rate.max(1) as f32
            } else {
                0.0
            };
            commands.entity(e).try_insert(Animator {
                anim: an,
                time,
                speed: rec.f32(speed).unwrap_or(1.0),
                looped,
            });
        }
        if !chest.shrine {
            commands.entity(e).try_insert(ChestFx {
                locked: None,
                stop: Some(1.0),
            });
            if !chest.opened
                && let Some(record) = rec.str("IdleEffect")
            {
                attach_effect(&mut commands, e, record);
            }
        }
        // A food crate's idle effect (`FixedItemShrine::StartIdleEffect`, gd-game `chests`): it runs until the
        // crate is used, then stops emitting where it is.
        if chest.shrine
            && !chest.opened
            && let Some(record) = rec.str("IdleLoopingEffect")
        {
            let fx = gd_render::fx::spawn_effect_record(&mut commands, record, Transform::IDENTITY);
            commands
                .entity(fx)
                .insert(gd_game_core::visuals::EffectSpec {
                    despawn_when_complete: true,
                    ..gd_game_core::visuals::EffectSpec::record(record)
                });
            gd_render::model::try_add_child(&mut commands, e, fx);
            commands.entity(e).try_insert(ShrineIdleFx(fx));
        }
    }
    for (e, chest, idle) in &opened {
        if !chest.opened || new.contains(e) {
            continue;
        }
        commands.entity(e).try_insert(ShownOpen);
        if let Some(idle) = idle {
            commands
                .entity(idle.0)
                .try_insert(gd_game_core::visuals::StopEmitting);
        }
        let Some(rec) = a.data.db.get(&chest.record) else {
            continue;
        };
        let (anim, speed) = open_animation(chest);
        if let Some(an) = rec.str(anim).and_then(|p| a.cache.anim(&a.data, p)) {
            commands.entity(e).try_insert(Animator {
                anim: an,
                time: 0.0,
                speed: rec.f32(speed).unwrap_or(1.0),
                looped: false,
            });
        }
    }
}

/// The chest's open animation has been started.
#[derive(Component)]
struct ShownOpen;

/// A food crate's running idle effect.
#[derive(Component)]
struct ShrineIdleFx(Entity);

/// A container's effect state (module doc): the running `LockedEffect`, and the seconds an open one still waits
/// before its attached effects stop.
#[derive(Component)]
struct ChestFx {
    locked: Option<Entity>,
    stop: Option<f32>,
}

/// An `EffectEntity` of `record` attached at `chest`'s origin; it goes once stopped and finished.
fn attach_effect(commands: &mut Commands, chest: Entity, record: &str) -> Entity {
    let fx = gd_render::fx::spawn_effect_record(commands, record, Transform::IDENTITY);
    commands
        .entity(fx)
        .insert(gd_game_core::visuals::EffectSpec {
            despawn_when_complete: true,
            ..gd_game_core::visuals::EffectSpec::record(record)
        });
    gd_render::model::try_add_child(commands, chest, fx);
    fx
}

/// The containers' locked effect and the stop of an open one's attached effects (module doc).
fn chest_effects(
    time: Res<Time>,
    mut commands: Commands,
    data: Res<gd_game_core::data::GameData>,
    mut chests: Query<(Entity, Ref<Chest>, &mut ChestFx, Option<&Children>)>,
    effects: Query<(), With<gd_game_core::visuals::EffectSpec>>,
) {
    for (e, chest, mut fx, children) in &mut chests {
        if chest.is_changed() || fx.is_added() {
            match (chest.locked, fx.locked) {
                (true, None) => {
                    if let Some(rec) = data.db.get(&chest.record)
                        && let Some(record) = rec.str("LockedEffect")
                    {
                        fx.locked = Some(attach_effect(&mut commands, e, record));
                    }
                }
                (false, Some(l)) => {
                    commands
                        .entity(l)
                        .try_insert(gd_game_core::visuals::StopEmitting);
                    fx.locked = None;
                }
                _ => {}
            }
        }
        if !chest.opened {
            continue;
        }
        let Some(left) = fx.stop else { continue };
        let left = left - time.delta_secs();
        if left > 0.0 {
            fx.stop = Some(left);
            continue;
        }
        fx.stop = None;
        for c in children.into_iter().flatten() {
            if effects.contains(*c) {
                commands
                    .entity(*c)
                    .try_insert(gd_game_core::visuals::StopEmitting);
            }
        }
    }
}

/// The animation (and speed field) of opening: a chest's `openAnimation`, a food crate's
/// `idleToActiveAnimation` (`FixedItemShrine::GoIdleToActive`; no stock crate has an active or dormant one,
/// so it rests at the end).
fn open_animation(chest: &Chest) -> (&'static str, &'static str) {
    if chest.shrine {
        ("idleToActiveAnimation", "idleToActiveAnimationSpeed")
    } else {
        ("openAnimation", "openAnimationSpeed")
    }
}
