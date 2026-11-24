//! Single-piece transmuter rules (`ItemTransmuter`): whether a piece can be
//! used on an item and what illusion it leaves ([`transmute_check`],
//! [`transmute_result`]). The Illusionist window and the using systems are
//! `gd_game::illusion`.
//!
//! A single-piece transmuter (`ItemTransmuter`: the DLC illusion pieces and the removal
//! flask, given by the bonus-items NPC's conversations `npc_bonusitems_0{1,2}.cnv`). A right click on one in the
//! bags or stash enters its cursor mode (`ItemTransmuter::CreateSecondaryCursorHandler` makes a
//! `CursorHandlerTransmute`, no requirement test), which is the component / augment apply mode of
//! `gd_game::inventory` (`Inventory::start_apply`; the handler shares `Update` / `Escape` / `CleanupDialog` with
//! `CursorHandlerRelicCombine`, folded, and its `Cancel` callers): a click on a bag, equipped or stash item it
//! `CanBeUsedOn` asks the confirmation, Yes applies it (`ControllerPlayer::UseItemOnConfigCmd` →
//! `Character::UseItemOn` → `ItemTransmuter::UseOn`). `ItemTransmuter::Load` reads
//! `transmuteDbr`, `transmuteTag`,
//! `removeTransmute`; `InitializeItem` loads the look when `transmuteDbr` is set and it is no remover.
//! `CanBeUsedOn` (always "ask first"): a look piece takes an `ItemEquipment` of the look's item type
//! whose current mesh is not the look's, both weapons by the weapon groups of the Illusionist window
//! (`gd_sim::items::illusion::fits`); the remover any equipment with an illusion. The text
//! (`CursorHandlerTransmute::Primary{Inventory,Equip,Stash,Transfer}Activate`):
//! `tagTransmuteConfirmation`, `…SoulboundConfirmation` when the piece is
//! soulbound and the item not; onto an item with an illusion `tagTransmuteOverwrite` / `…SoulboundOverwrite`;
//! the remover `tagTransmuteRemove`. A transfer-stash item is never a target unless the piece
//! `CanBePlacedInTransferStash` (not soulbound, no material), which no stock piece is. `UseOn`: a
//! look piece becomes the item's illusion (`ItemEquipment::AddTransmute(Item)`), one of a stack or
//! the last one; the remover clears it (`RemoveTransmute`) and is used up the same way. No sound.
//! PORT: the engine's illusion is the piece itself (the gdc's transmute name is the piece's record, its look
//! `GetTransmute`, and `AddTransmute` marks the item untradeable, soulbound when the piece is); the port stores
//! the look record, as the Illusionist window does (`AddTransmute(string)`), and an item's soulbound state stays
//! its base record's or augment's (`Inventory::apply_check`). The "capture" kind (no `transmuteDbr`, no
//! `removeTransmute`: `tagTransmuteDestroy`, infuses an item's look) exists only as a sandbox record
//! (`records/sandbox/jakub/transmute_capture_test.dbr`): not ported.
use gd_formats::save::item::Item;
use gd_sim::items::illusion::fits;

/// What the transmuter `tr` (an `ItemTransmuter` record) does to `target` (module doc: `CanBeUsedOn` and the
/// confirmation text): None when it cannot be used on it, else the question's text tag. `bound`: the target is
/// soulbound (the caller's read of `Item.soulbound`).
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
    // The item's current mesh (its illusion's when it has one) against the look's.
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

/// The illusion `tr` (an `ItemTransmuter` record) leaves on the item it is used on (module doc: `UseOn`): its look
/// record, or none for the remover.
pub fn transmute_result(tr: &gd_formats::arz::Record) -> String {
    if tr.bool("removeTransmute") {
        String::new()
    } else {
        tr.str("transmuteDbr").unwrap_or("").to_owned()
    }
}
