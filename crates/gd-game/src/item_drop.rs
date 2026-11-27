//! Dropping the item on the cursor onto the ground.
//!
//! `CursorHandlerItemMove::ActivateWorld`: a left or right press on the world with an item on the
//! cursor drops the whole stack ([`DropHeld`]); quest items too. A `soulbound` or `untradeable`
//! item is not dropped: the Yes / No question `tagDestroySoulbound` (`tagDestroyUntradeable` when
//! only untradeable) comes up ([`Inventory::destroying`]); while it is up presses do nothing and
//! Escape does not put the item back. The answer ([`DestroyAnswer`]): Yes destroys the item, No
//! puts it back ([`Inventory::cancel_held`]).
//!
//! Where it lands (`ControllerCharacter::SendDropItemRandom`): from the player's centre of mass
//! (half its height over its feet) with velocity `1.5 · (cos a, 0, sin a)`,
//! `a = rand() · 2π / 32767` ([`drop_velocity`]), as a thrown rigid body. `Item.dropper` makes
//! every `CanAutoPickup` refuse an item the main player dropped ([`DroppedByPlayer`]).
//!
//! The drop key (key action 32 `DropItem`, [`DropKey`]) gives the active cursor handler its
//! `ActivateWorld` without the mouse-position test (any window may be open): an item on the
//! cursor drops as [`DropHeld`]; the relic combine / enchant / transmute mode is cancelled
//! ([`Inventory::cancel_apply`]; PORT: not while its confirmation is up); an empty cursor does
//! nothing. PORT: only the chat window would block it; the port has none.
//!
//! A dropped item lasts the session only, like the engine: level state is kept in memory across streaming
//! (`Region::Unload` → `Level::SaveState`, restored by `Region::Load`), and `World::Save` — the only writer
//! of a world-state file — is never called, so nothing on the ground is written to the save.
use crate::loot::GroundItem;
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_game_core::character::Character;
use gd_game_core::data::GameData;
use gd_game_core::inventory::Inventory;
use gd_game_core::player::Player;

pub struct ItemDropPlugin;

impl Plugin for ItemDropPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<DropHeld>()
            .add_message::<DropKey>()
            .add_message::<DestroyAnswer>()
            .add_systems(
                Update,
                apply_drops
                    .run_if(resource_exists::<Inventory>)
                    .after(gd_game_core::inventory::InventorySet)
                    .before(gd_game_core::player::PlayerSet)
                    .before(crate::loot::pick_up),
            );
    }
}

/// A press on the world with an item on the cursor.
#[derive(Message, Clone, Copy, Debug)]
pub struct DropHeld;

/// The drop key (action 32) went down.
#[derive(Message, Clone, Copy, Debug)]
pub struct DropKey;

/// The answer to the destroy question (true = Yes: the held item is destroyed).
#[derive(Message, Clone, Copy, Debug)]
pub struct DestroyAnswer(pub bool);

#[derive(Component, Clone, Copy, Debug)]
pub struct DroppedByPlayer;

/// `SendDropItemRandom`'s horizontal speed (m/s).
pub const DROP_SPEED: f32 = 1.5;

/// The drop's velocity for the engine's `rand()` value `r` (0..=32767): `a = r · 0.00019175345`.
pub fn drop_velocity(r: u32) -> [f32; 3] {
    let a = r as f32 * 0.000_191_753_45;
    [a.cos() * DROP_SPEED, 0.0, a.sin() * DROP_SPEED]
}

/// The destroy question's text tag for an item of record `rec`, None when it can be dropped.
pub fn destroy_tag(rec: &Record) -> Option<&'static str> {
    if rec.bool("soulbound") {
        Some("tagDestroySoulbound")
    } else if rec.bool("untradeable") {
        Some("tagDestroyUntradeable")
    } else {
        None
    }
}

/// Applies [`DropHeld`], [`DropKey`] and [`DestroyAnswer`].
#[allow(clippy::too_many_arguments)]
pub fn apply_drops(
    mut commands: Commands,
    mut drops: MessageReader<DropHeld>,
    mut keys: MessageReader<DropKey>,
    mut answers: MessageReader<DestroyAnswer>,
    mut inv: ResMut<Inventory>,
    players: Query<
        (
            &Transform,
            &Character,
            &gd_game_core::inventory::Requirements,
        ),
        With<Player>,
    >,
    data: Res<GameData>,
    // Option: the stash loads after the inventory.
    mut stash: Option<ResMut<gd_game_core::stash::Stash>>,
) {
    for DestroyAnswer(yes) in answers.read() {
        if inv.destroying.take().is_none() {
            continue;
        }
        if *yes {
            if let Some(h) = inv.held.take() {
                info!("destroyed {}", h.base_name);
            }
        } else if let Err(h) = inv.cancel_held(stash.as_deref_mut(), None) {
            // No caravan page comes with the destroy answer: the origin cell takes it back.
            let Ok((t, ch, req)) = players.single() else {
                inv.held = Some(h);
                continue;
            };
            let center = t.translation + crate::monster_skills::character_center(&data, ch);
            crate::loot::give_item(
                &mut commands,
                &mut inv,
                &data,
                crate::loot::Receiver { center, req },
                h,
                false,
            );
        }
        inv.changed();
    }
    let mut pressed = drops.read().count();
    for _ in keys.read() {
        if inv.held.is_some() {
            pressed += 1;
        } else if inv.applying.as_ref().is_some_and(|a| a.confirm.is_none()) {
            info!("drop key: the apply mode ends");
            inv.cancel_apply();
        }
    }
    for _ in 0..pressed {
        if inv.destroying.is_some() {
            continue;
        }
        let Some(item) = inv.held.as_ref() else {
            continue;
        };
        let Ok((tf, ch, _)) = players.single() else {
            continue;
        };
        if let Some(tag) = data.db.get(&item.base_name).and_then(destroy_tag) {
            info!("drop: {} asks {tag}", item.base_name);
            inv.destroying = Some(tag);
            inv.changed();
            continue;
        }
        let Some(item) = inv.held.take() else {
            continue;
        };
        inv.changed();
        let drop = tf.translation + crate::monster_skills::character_center(&data, ch);
        let v = drop_velocity(gd_game_core::rng::u32(0..=0x7FFF));
        info!("dropped {} x{}", item.base_name, item.stack_count.max(1));
        let e = crate::loot::spawn_thrown(
            &mut commands,
            drop,
            v,
            0.0,
            GroundItem {
                item: Some(item),
                gold: 0,
                yaw: 0.0,
            },
        );
        commands.entity(e).insert(DroppedByPlayer);
    }
}
