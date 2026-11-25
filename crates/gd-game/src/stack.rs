//! The stack split of the stack quantity window (`gd_ui` `stack.rs`
//! draws it); the stack-size and merging rules are `gd_game_core::stack`.
//!
//! The split ([`SplitRequest`], GrimDawn.exe `StackWindow` OK
//! `StackWindow::OnOk`): `n = min(amount, stack − 1)`; nothing when 0; else a copy of
//! the item with `n` goes on the cursor (its item-move handler, the open
//! market's id set, so it can be sold on the merchant window,
//! [`crate::trade::SellHeld`]) and the source keeps `stack − n`. The window's
//! amount rules are [`gd_game_core::stack::split_max`], [`gd_game_core::stack::typed_amount`],
//! [`gd_game_core::stack::increase`], [`gd_game_core::stack::decrease`]. PORT: a request while an item is held is refused (the engine
//! hides the window on every bag / stash click, so its OK never meets a held
//! item from the mouse).
use bevy::prelude::*;
use gd_formats::save::item::Item;
use gd_game_core::inventory::{Inventory, ItemLoc};
use gd_game_core::stack::count;
use gd_game_core::stash::Stash;

/// The stack window's OK (module doc): `amount` of the stack at `from` (a
/// bag or stash cell of it) goes on the cursor.
#[derive(Message, Clone, Copy, Debug)]
pub struct SplitRequest {
    pub from: ItemLoc,
    pub amount: u32,
}

/// The stack at `loc` (bags or the stash).
fn stack_mut<'a>(
    inv: &'a mut Inventory,
    stash: Option<&'a mut Stash>,
    loc: ItemLoc,
) -> Option<&'a mut Item> {
    let (sack, x, y) = match loc {
        ItemLoc::Bag { bag, x, y } => (inv.bags.get_mut(bag)?, x, y),
        ItemLoc::Stash {
            transfer,
            page,
            x,
            y,
        } => (stash?.view_mut(transfer).get_mut(page)?, x, y),
        ItemLoc::Equip(_) => return None,
    };
    sack.items
        .iter_mut()
        .find(|p| x >= p.x && x < p.x + p.size.w && y >= p.y && y < p.y + p.size.h)
        .map(|p| &mut p.item)
}

/// Applies [`SplitRequest`]s.
fn apply_splits(
    mut msgs: MessageReader<SplitRequest>,
    mut inv: ResMut<Inventory>,
    // Optional: a bag split needs no stash (it exists once `init_stash` ran).
    mut stash: Option<ResMut<Stash>>,
    mut done: MessageWriter<crate::trade::TradeDone>,
) {
    for r in msgs.read() {
        if inv.held.is_some() {
            info!("stack: an item is held, no split");
            continue;
        }
        let Some(src) = stack_mut(&mut inv, stash.as_deref_mut(), r.from) else {
            continue;
        };
        let n = r.amount.min(count(src).saturating_sub(1));
        if n == 0 {
            continue;
        }
        src.stack_count = count(src) - n;
        let mut part = src.clone();
        part.stack_count = n;
        info!(
            "stack: split {n} of {} off (left {})",
            part.base_name.rsplit('/').next().unwrap_or(""),
            src.stack_count
        );
        inv.held = Some(part);
        inv.changed();
        if matches!(r.from, ItemLoc::Stash { .. }) {
            done.write(Default::default());
        }
    }
}

pub struct StackPlugin;

impl Plugin for StackPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SplitRequest>().add_systems(
            Update,
            apply_splits
                .run_if(resource_exists::<Inventory>)
                .after(gd_game_core::inventory::InventorySet)
                .before(gd_game_core::player::PlayerSet),
        );
    }
}
