//! The Inventor's reroll tab and the Runekeeper's window, input side: the action button sends the slot
//! item's reroll / ascension (rules: `gd_game::reroll`).
use crate::trade::TradeOpen;
use bevy::prelude::*;
pub use gd_game::reroll::Kind;
use gd_ui::state::trade::{Panel, Trade};

/// Sends the open reroll tab / ascension window's button press for the slot item (changed in place,
/// `CreateArtifactCmd`); it comes back as a `gd_game::crafting::SlotReturn`.
pub fn reroll(
    mut trade: ResMut<Trade>,
    mut requests: MessageWriter<gd_game::reroll::RerollRequest>,
    #[cfg(feature = "dev")] (hooks, mut stage, mut tested): (
        Res<gd_game_core::hooks::Hooks>,
        MessageWriter<gd_game::crafting::StageItem>,
        Local<bool>,
    ),
) {
    #[cfg(feature = "dev")]
    let test = hooks.items.reroll.is_some();
    #[cfg(not(feature = "dev"))]
    let test = false;
    #[cfg(feature = "dev")]
    if let Some(k) = &hooks.items.reroll
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
    requests.write(gd_game::reroll::RerollRequest { item, kind, test });
}
