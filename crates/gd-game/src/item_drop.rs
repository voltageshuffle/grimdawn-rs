//! Dropping the item on the cursor onto the ground.
//!
//! GrimDawn.exe sub_1401ED7C0, Game.dll `CursorHandlerItemMove::ActivateWorld` @0x1801845f0: a
//! left or right press on the world (no window took it) with an item on the cursor drops it, the whole
//! stack ([`DropHeld`], sent by `grimdawn` `ui/apply.rs` `world_click`). Quest items drop like any other.
//! An item whose record is `soulbound` or `untradeable` (`Item::Load` @0x180324c60, Item +3072 / +3074) is
//! not dropped: the Yes / No question `tagDestroySoulbound` (`tagDestroyUntradeable` when it is only
//! untradeable; `DialogManager::AddDialog` type 21) comes up ([`Inventory::destroying`]) and the press
//! does nothing else; while it is up further presses do nothing and Escape does not put the item back
//! (`CursorHandlerItemMove::Escape` @0x180187140). `CursorHandlerItemMove::Update` @0x180186f20 reads the
//! answer ([`DestroyAnswer`]): Yes destroys the item (`SendRemoveItemFromInventory`), No puts it back
//! (the handler's `Cancel`, [`Inventory::cancel_held`]: back to its bag cell, else `GiveItemToPlayer`:
//! equip-empty, bags, else thrown down).
//!
//! Where it lands (`ControllerCharacter::SendDropItemRandom` @0x18012b990): from the player's
//! centre of mass (`Player::GetCenterOfMass` @0x1803e3830: half its height over its feet) with velocity
//! `1.5 · (cos a, 0, sin a)`, `a = rand() · 2π / 32767` ([`drop_velocity`]), through
//! `GameEngine::CreateItemCopy` (a random yaw, the item's rigid body until it touches the world or the path
//! mesh: `gd_sim::loot_toss`, [`crate::loot::spawn_thrown`]). `Item::OnDropped` @0x180328710 records the
//! dropper (Item +1728, carried by the replica info into the ground copy): every `CanAutoPickup`
//! (potions, food, gold, components, quest items) refuses an item the main player dropped, so the player's
//! own drop is only picked up by clicking it ([`DroppedByPlayer`], `crate::loot::pick_up`).
//!
//! The drop key (GrimDawn.exe `CursorHandlerMgr_OnKeyAction` (exe sub_1401ED880)):
//! key action 32 `DropItem` on key down ([`DropKey`], sent by `grimdawn` `ui/apply.rs` `drop_key`) gives the
//! active cursor handler its `ActivateWorld`, as a world press does but with no mouse-position test (any
//! window may be open; only the chat window blocks it, PORT: the port has none). So an item on the cursor
//! drops exactly as [`DropHeld`] (`CursorHandlerItemMove::ActivateWorld`); the relic combine / enchant /
//! transmute mode (`CursorHandlerRelicCombine::ActivateWorld` @0x180181dd0, `CursorHandlerTransmute::ActivateWorld`
//! @0x180189200: the right-click mode of an `ItemTransmuter`, `crate::illusion` docs) is cancelled
//! ([`Inventory::cancel_apply`], the item stays in its cell; PORT: like the world press, not while its
//! confirmation is up); with an empty cursor nothing happens. The hot-slot-option mode
//! (`CursorHandlerHotSlotOption::ActivateWorld` @0x180183eb0: deletes the dragged option) never occurs:
//! nothing creates that handler (its constructors are referenced only by Game.dll's export table, and
//! GrimDawn.exe imports no `CursorHandler` of that class), so stock has no hot-slot drag to drop.
//!
//! PORT: a dropped item is a ground item like any drop: it stays while the session runs (the port keeps
//! ground items across level streaming; `grimdawn` `exit.rs` despawns them with the session) and is not
//! saved (the save holds no world items; it has left the inventory).
use crate::character::Character;
use crate::data::GameData;
use crate::inventory::Inventory;
use crate::loot::GroundItem;
use crate::player::Player;
use bevy::prelude::*;
use gd_formats::arz::Record;

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
                    .after(crate::inventory::InventorySet)
                    .before(crate::player::PlayerSet)
                    .before(crate::loot::pick_up),
            );
    }
}

/// A press on the world with an item on the cursor (module doc).
#[derive(Message, Clone, Copy, Debug)]
pub struct DropHeld;

/// The drop key (action 32) went down: the cursor handler's `ActivateWorld` (module doc).
#[derive(Message, Clone, Copy, Debug)]
pub struct DropKey;

/// The answer to the destroy question (true = Yes: the held item is destroyed).
#[derive(Message, Clone, Copy, Debug)]
pub struct DestroyAnswer(pub bool);

/// A ground item the player dropped: never picked up automatically (module doc).
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

/// Applies [`DropHeld`], [`DropKey`] and [`DestroyAnswer`] (module doc).
#[allow(clippy::too_many_arguments)]
pub fn apply_drops(
    mut commands: Commands,
    mut drops: MessageReader<DropHeld>,
    mut keys: MessageReader<DropKey>,
    mut answers: MessageReader<DestroyAnswer>,
    mut inv: ResMut<Inventory>,
    players: Query<(&Transform, &Character, &crate::inventory::Requirements), With<Player>>,
    data: Res<GameData>,
    // Option: the stash loads after the inventory.
    mut stash: Option<ResMut<crate::stash::Stash>>,
) {
    for DestroyAnswer(yes) in answers.read() {
        if inv.destroying.take().is_none() {
            continue;
        }
        if *yes {
            if let Some(h) = inv.held.take() {
                info!("destroyed {}", h.base_name);
            }
        } else if let Err(h) = inv.cancel_held(stash.as_deref_mut()) {
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
        let v = drop_velocity(crate::rng::u32(0..=0x7FFF));
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
