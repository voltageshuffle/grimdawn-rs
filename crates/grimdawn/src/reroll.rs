//! The Inventor's reroll tab and the Runekeeper's window, input side: the action button sends the slot
//! item's reroll / ascension (rules, costs and notes: `gd_game::reroll`; window UI:
//! `ui/trade.rs`; the slot's clicks and give-back: `crate::trade` `panel_clicks`).
//! `GD_REROLL` (hooks.rs; `ascend` with `GD_TRADE=ascension`): once the crafter is open, puts the first
//! bag item into the slot and presses the button.
use crate::trade::{Panel, Trade, TradeOpen};
use bevy::prelude::*;
pub use gd_game::reroll::{Kind, Plan, kind_state, plan, shortfall};

/// Sends the open reroll tab / ascension window's button press for the slot
/// item (changed in place, `CreateArtifactCmd`);
/// `gd_game::reroll::apply_reroll` runs it and the item comes back as a
/// `gd_game::crafting::SlotReturn`.
pub fn reroll(
    hooks: Res<gd_game::hooks::Hooks>,
    mut trade: ResMut<Trade>,
    mut requests: MessageWriter<gd_game::reroll::RerollRequest>,
    mut stage: MessageWriter<gd_game::crafting::StageItem>,
    mut tested: Local<bool>,
) {
    let test = hooks.items.reroll.clone();
    // `GD_REROLL`: put the first bag item into the slot and press the button.
    if let Some(k) = &test
        && !*tested
        && matches!(
            trade.open,
            Some(TradeOpen::Inventor { .. } | TradeOpen::Ascension { .. })
        )
        && trade.slot.is_none()
    {
        *tested = true;
        trade.kind = Some(match k.as_str() {
            "ascended" => Kind::Ascended,
            "prefix" => Kind::Prefix,
            "suffix" => Kind::Suffix,
            "ascend" => Kind::Ascend,
            _ => Kind::Seed,
        });
        if matches!(trade.open, Some(TradeOpen::Inventor { .. })) {
            trade.panel = Panel::Reroll;
        }
        stage.write(gd_game::crafting::StageItem {
            which: gd_game::crafting::Stage::Contains(String::new()),
            go: true,
        });
    }
    let kind = match trade.open {
        Some(TradeOpen::Ascension { .. }) => Some(Kind::Ascend),
        Some(TradeOpen::Inventor { .. }) if trade.panel == Panel::Reroll => trade.kind,
        _ => return,
    };
    if !trade.go {
        return;
    }
    trade.go = false;
    let (Some(kind), Some(item)) = (kind, trade.slot.take()) else {
        return;
    };
    requests.write(gd_game::reroll::RerollRequest {
        item,
        kind,
        test: test.is_some(),
    });
}
