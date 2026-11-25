//! Single-piece transmuter rules (`ItemTransmuter`: DLC illusion pieces and the removal flask).
//!
//! A right click enters the component / augment apply cursor mode (`CursorHandlerTransmute`, no
//! requirement test); a click on an item it `CanBeUsedOn` asks, Yes applies (`ItemTransmuter::UseOn`).
//! `CanBeUsedOn`: a look piece takes equipment of the look's item type whose current mesh differs
//! (weapons by the Illusionist window's groups, `gd_sim::items::illusion::fits`); the remover any
//! equipment with an illusion. Text: `tagTransmuteConfirmation` / `…SoulboundConfirmation` (piece
//! soulbound, item not), `tagTransmuteOverwrite` / `…SoulboundOverwrite` onto an illusion,
//! `tagTransmuteRemove`. Transfer-stash items are never targets (no stock piece
//! `CanBePlacedInTransferStash`). `UseOn` consumes one of the stack. No sound.
//! PORT: the engine's illusion is the piece itself (marking the item untradeable / soulbound); the
//! port stores the look record and keeps the item's own soulbound state. The sandbox-only "capture"
//! kind (`tagTransmuteDestroy`) is not ported.
use gd_formats::save::item::Item;
use gd_sim::items::illusion::fits;

/// None when `tr` cannot be used on `target`, else the question's text tag. `bound`: the target is soulbound.
pub fn transmute_check(
    db: &gd_sim::items::ItemDb,
    tr: &gd_formats::arz::Record,
    target: &Item,
    bound: bool,
) -> Option<&'static str> {
    if tr.bool("removeTransmute") {
        return (!target.transmute_name.is_empty()).then_some("tagTransmuteRemove");
    }
    let look = db.get(tr.str("transmuteDbr")?)?;
    let base = db.get(&target.base_name)?;
    if !fits(&base.class, &look.class) {
        return None;
    }
    // Current mesh (the illusion's when set) against the look's.
    let mesh = |r: &gd_formats::arz::Record| {
        r.str("mesh")
            .or(r.str("armorMaleMesh"))
            .unwrap_or("")
            .to_ascii_lowercase()
    };
    let now = db
        .get(gd_sim::items::illusion::look_record(target))
        .unwrap_or(base);
    if mesh(now) == mesh(look) {
        return None;
    }
    let binds = tr.bool("soulbound") && !bound;
    Some(match (target.transmute_name.is_empty(), binds) {
        (true, false) => "tagTransmuteConfirmation",
        (true, true) => "tagTransmuteSoulboundConfirmation",
        (false, false) => "tagTransmuteOverwrite",
        (false, true) => "tagTransmuteSoulboundOverwrite",
    })
}

/// The look record `tr` leaves on its target; empty for the remover.
pub fn transmute_result(tr: &gd_formats::arz::Record) -> String {
    if tr.bool("removeTransmute") {
        String::new()
    } else {
        tr.str("transmuteDbr").unwrap_or("").to_owned()
    }
}
