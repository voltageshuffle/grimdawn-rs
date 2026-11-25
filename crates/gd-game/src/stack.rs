//! The stack split of the stack quantity window (`StackWindow::OnOk`):
//! `n = min(amount, stack − 1)`; nothing when 0; else a copy of the item with `n` goes on
//! the cursor and the source keeps `stack − n`.
//! PORT: a request while an item is held is refused (the engine hides the window on every
//! bag / stash click, so its OK never meets a held item).
use bevy::prelude::*;
use gd_formats::save::item::Item;
use gd_game_core::inventory::{Inventory, ItemLoc};
use gd_game_core::stack::count;
use gd_game_core::stash::Stash;

/// `amount` of the stack at `from` (a bag or stash cell of it) goes on the cursor.
#[derive(Message, Clone, Copy, Debug)]
pub struct SplitRequest {
    pub from: ItemLoc,
    pub amount: u32,
}

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

fn apply_splits(
    mut msgs: MessageReader<SplitRequest>,
    mut inv: ResMut<Inventory>,
    // Option: a bag split needs no stash.
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
