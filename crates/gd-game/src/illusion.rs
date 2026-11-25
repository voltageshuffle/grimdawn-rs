//! The Illusionist (`NpcTransmuter`): unlocked appearances and applying
//! them to worn items. Rules and their sources: `gd_sim::items::illusion`.
//!
//! - [`Illusions`]: the account-wide unlocked looks (`transmutes.gst`, `transmutes.gsh` for a hardcore
//!   character: `gd_formats::save::Shared`; read from the loaded character's `save/` folder like
//!   `transfer.gst`, written to the port's; `save.rs`), one list per `Item_Type`
//!   (`GameEngine::GetPlayerTransmutes`).
//! - [`unlock_items`]: every item in the bags or worn is unlocked the way
//!   `Inventory::AddItemToInventory` / `Player::LoadTransmutes` call
//!   `GameEngine::AddItemToTransmutes` (also on load).
//! - [`choices`]: the window's list for a worn item, [`apply`] (sent as
//!   [`ApplyIllusions`] by the window): the picks, the cost per changed item
//!   and the new `transmute_name` (saved with the item in the gdc; the body
//!   is rebuilt from `look_record`).
//! - [`use_sets`]: a used transmute set (`ItemTransmuterSet`, the DLC illusion packs and the Coven's Black Knight set;
//!   right click, `consumables::use_items` sends [`UseTransmuteSet`]). `ItemTransmuterSet::Use`
//!   (main player only): each record of `transmuteDbrs` goes through `AddItemToTransmutes` (the
//!   [`Illusions::unlock`] dedupe); when any was new, `SaveTransmutes` writes `transmutes.gst` at once (the account's
//!   shared save folder, `GetSharedSavePath`; here the port's save folder root, where `save.rs` writes it too). Then
//!   the `useSound` plays and, when `transmuteTag` is set, the cinematic text type 8, colour (0.9, 0.9, 1): headline
//!   `tagTransmuteNew` ("You have gained a new Illusion: {%s0}") with the localised set name, subline empty, on every
//!   use (also when nothing was new). `AllowUse` always allows it and removes the item when its stack is
//!   1; a larger stack loses one (`DecrementStack`), the generic rule of `consumables::consumed`.
//! - [`gd_game_core::illusion::transmute_check`]: a single-piece transmuter (`ItemTransmuter`: the DLC illusion pieces and the removal
//!   flask, given by the bonus-items NPC's conversations `npc_bonusitems_0{1,2}.cnv`). A right click on one in the
//!   bags or stash enters its cursor mode (`ItemTransmuter::CreateSecondaryCursorHandler` makes a
//!   `CursorHandlerTransmute`, no requirement test), which is the component / augment apply mode of
//!   `crate::inventory` (`Inventory::start_apply`; the handler shares `Update` / `Escape` / `CleanupDialog` with
//!   `CursorHandlerRelicCombine`, folded, and its `Cancel` callers): a click on a bag, equipped or stash item it
//!   `CanBeUsedOn` asks the confirmation, Yes applies it (`ControllerPlayer::UseItemOnConfigCmd` →
//!   `Character::UseItemOn` → `ItemTransmuter::UseOn`). `ItemTransmuter::Load` reads
//!   `transmuteDbr`, `transmuteTag`,
//!   `removeTransmute`; `InitializeItem` loads the look when `transmuteDbr` is set and it is no remover.
//!   `CanBeUsedOn` (always "ask first"): a look piece takes an `ItemEquipment` of the look's item type
//!   whose current mesh is not the look's, both weapons by the weapon groups of the Illusionist window
//!   (`gd_sim::items::illusion::fits`); the remover any equipment with an illusion. The text
//!   (`CursorHandlerTransmute::Primary{Inventory,Equip,Stash,Transfer}Activate`):
//!   `tagTransmuteConfirmation`, `…SoulboundConfirmation` when the piece is
//!   soulbound and the item not; onto an item with an illusion `tagTransmuteOverwrite` / `…SoulboundOverwrite`;
//!   the remover `tagTransmuteRemove`. A transfer-stash item is never a target unless the piece
//!   `CanBePlacedInTransferStash` (not soulbound, no material), which no stock piece is. `UseOn`: a
//!   look piece becomes the item's illusion (`ItemEquipment::AddTransmute(Item)`), one of a stack or
//!   the last one; the remover clears it (`RemoveTransmute`) and is used up the same way. No sound.
//!   PORT: the engine's illusion is the piece itself (the gdc's transmute name is the piece's record, its look
//!   `GetTransmute`, and `AddTransmute` marks the item untradeable, soulbound when the piece is); the port stores
//!   the look record, as the Illusionist window does (`AddTransmute(string)`), and an item's soulbound state stays
//!   its base record's or augment's (`Inventory::apply_check`). The "capture" kind (no `transmuteDbr`, no
//!   `removeTransmute`: `tagTransmuteDestroy`, infuses an item's look) exists only as a sandbox record
//!   (`records/sandbox/jakub/transmute_capture_test.dbr`): not ported.
use bevy::prelude::*;
use gd_formats::save::Shared;
use gd_formats::save::gst::{TransmuteEntry, Transmutes};
use gd_formats::save::item::Item;
use gd_formats::vfs::Database;
use gd_game_core::inventory::{Inventory, SLOTS};
use gd_sim::items::illusion::{fits, item_type, same_look, sort_key, takes_illusion};

/// The window record (`transmuterwindow.tpl`).
pub const WINDOW: &str = "records/ui/transmuter/transmuter_mastertable.dbr";

/// The unlocked looks (`transmutes.gst`).
#[derive(Resource, Clone)]
pub struct Illusions {
    pub file: Transmutes,
    /// Unlocked since loading (`save.rs` writes the file when set).
    pub changed: bool,
}

impl Illusions {
    pub fn new_file() -> Transmutes {
        Transmutes {
            seed: gd_game_core::rng::u32(..),
            file_version: 1,
            version: 2,
            mod_name: String::new(),
            expansion_status: gd_game_core::stash::LOADED_EXPANSIONS,
            count: 0,
            entries: Vec::new(),
        }
    }

    /// The records unlocked for an item type.
    pub fn of_type(&self, ty: u32) -> &[String] {
        self.file
            .entries
            .iter()
            .find(|e| e.key == ty)
            .map_or(&[], |e| &e.records)
    }

    /// `AddItemToTransmutes` (module doc): true when `record` was added.
    pub fn unlock(&mut self, db: &Database, record: &str) -> bool {
        let Some(mut rec) = db.get(record) else {
            return false;
        };
        let mut record = record.to_owned();
        // A DLC unlock item stores the look it unlocks.
        if rec.class == "ItemTransmuter" {
            let Some(target) = rec.str("transmuteDbr").map(str::to_owned) else {
                return false;
            };
            let Some(t) = db.get(&target) else {
                return false;
            };
            rec = t;
            record = target;
        }
        let ty = item_type(&rec.class);
        if !takes_illusion(ty) {
            return false;
        }
        let i = match self.file.entries.iter().position(|e| e.key == ty) {
            Some(i) => i,
            None => {
                // The engine's map is ordered by type.
                let at = self
                    .file
                    .entries
                    .iter()
                    .position(|e| e.key > ty)
                    .unwrap_or(self.file.entries.len());
                self.file.entries.insert(
                    at,
                    TransmuteEntry {
                        block_id: 0,
                        key: ty,
                        records: Vec::new(),
                    },
                );
                at
            }
        };
        let known = self.file.entries[i].records.iter().any(|r| {
            r.eq_ignore_ascii_case(&record)
                || db.get(r).is_some_and(|o| same_look(r, o, &record, rec))
        });
        if known {
            return false;
        }
        self.file.entries[i].records.push(record);
        self.file.count = self.file.entries.len() as u32;
        self.changed = true;
        true
    }
}

/// Loads [`Illusions`] once the session has a character (the shared
/// `transmutes.gst`; an empty list when there is none, and an unreadable one is never overwritten:
/// `gd_game_core::stash::read_shared`).
pub fn init_illusions(
    mut commands: Commands,
    // Option: the existence check (this system creates it).
    have: Option<Res<Illusions>>,
    choice: Res<gd_game_core::session::Choice>,
    save_dir: Res<gd_game_core::session::SaveDir>,
    mut shared: ResMut<gd_game_core::stash::SharedFiles>,
) {
    if have.is_some() {
        return;
    }
    let path = gd_game_core::stash::shared_path(&choice, &save_dir, Shared::Transmutes);
    let file = gd_game_core::stash::read_shared(
        &mut shared,
        &choice,
        &save_dir,
        Shared::Transmutes,
        Transmutes::parse,
    )
    .unwrap_or_else(Illusions::new_file);
    info!(
        "illusions: {} unlocked ({})",
        file.entries.iter().map(|e| e.records.len()).sum::<usize>(),
        path.display()
    );
    commands.insert_resource(Illusions {
        file,
        changed: false,
    });
}

/// Unlocks the looks of the bags' and the worn items when the inventory
/// changes.
pub fn unlock_items(
    inv: Res<Inventory>,
    mut ill: ResMut<Illusions>,
    data: Res<gd_game_core::data::GameData>,
    mut seen: Local<Option<u64>>,
    mut fresh: gd_game_core::session::NewSession,
) {
    if fresh.begun() {
        *seen = None;
    }
    if *seen == Some(inv.version) {
        return;
    }
    *seen = Some(inv.version);
    let records: Vec<&str> = inv
        .bags
        .iter()
        .flat_map(|b| &b.items)
        .map(|p| p.item.base_name.as_str())
        .chain(inv.equipped.iter().flatten().map(|i| i.base_name.as_str()))
        .collect();
    for r in records {
        if ill.unlock(&data.db, r) {
            debug!("illusions: unlocked {r}");
        }
    }
}

/// The unlocked looks offered for the worn `item`, in the window's order
/// (`name`: a record's display name, the last sort key). Empty when the
/// item takes no illusion. A male player (`female` false) is not offered the
/// records flagged `preventMaleTransmute` (page builder
/// `TransmuteWindow::BuildPage`: `Item::IsMaleCompatible`, true unless the record's
/// `preventMaleTransmute` is set and the item's gender, the main player's at
/// `AddItemToTransmutes`, is male).
pub fn choices(
    db: &Database,
    ill: &Illusions,
    item: &Item,
    female: bool,
    name: &dyn Fn(&str) -> String,
) -> Vec<String> {
    let Some(rec) = db.get(&item.base_name) else {
        return Vec::new();
    };
    let ty = item_type(&rec.class);
    if !takes_illusion(ty) {
        return Vec::new();
    }
    let mut list: Vec<(_, String)> = ill
        .of_type(ty)
        .iter()
        .filter_map(|r| {
            let c = db.get(r)?;
            if !female && c.bool("preventMaleTransmute") {
                return None;
            }
            fits(&rec.class, &c.class).then(|| (sort_key(r, c, name(r)), r.clone()))
        })
        .collect();
    list.sort_by(|a, b| a.0.cmp(&b.0));
    list.into_iter().map(|(_, r)| r).collect()
}

/// Whether picking `look` for `item` changes nothing but clearing its
/// illusion (the look is the item's own).
pub fn is_own_look(db: &Database, item: &Item, look: &str) -> bool {
    match (db.get(&item.base_name), db.get(look)) {
        (Some(a), Some(b)) => same_look(&item.base_name, a, look, b),
        _ => false,
    }
}

/// The cost of one changed item at `level` (the window's equation).
pub fn cost_each(db: &Database, level: u32) -> u64 {
    let eq = db
        .get(WINDOW)
        .and_then(|w| w.str("transmuteCost").map(str::to_owned))
        .unwrap_or_default();
    gd_sim::items::illusion::cost(&eq, level, 0)
}

/// What a set of picks (worn slot index, look record) costs: one
/// [`cost_each`] per pick that is not the item's own look.
pub fn picks_cost(db: &Database, inv: &Inventory, picks: &[(usize, String)], level: u32) -> u64 {
    let changed = picks
        .iter()
        .filter(|(s, look)| {
            inv.equipped
                .get(*s)
                .and_then(|i| i.as_ref())
                .is_some_and(|it| !is_own_look(db, it, look))
        })
        .count() as u64;
    changed * cost_each(db, level)
}

/// Applies the picks: pays [`picks_cost`] (refused when short), sets or
/// clears each item's illusion. Returns the price paid.
pub fn apply(
    db: &Database,
    inv: &mut Inventory,
    gold: &mut u64,
    level: u32,
    picks: &[(usize, String)],
) -> Result<u64, &'static str> {
    let cost = picks_cost(db, inv, picks, level);
    if *gold < cost {
        return Err("cannot afford");
    }
    *gold -= cost;
    for (s, look) in picks {
        let Some(it) = inv.equipped.get_mut(*s).and_then(|i| i.as_mut()) else {
            continue;
        };
        let own = match (db.get(&it.base_name), db.get(look)) {
            (Some(a), Some(b)) => same_look(&it.base_name, a, look, b),
            _ => false,
        };
        it.transmute_name = if own { String::new() } else { look.clone() };
        info!(
            "illusion: slot {:?} -> {}",
            SLOTS[*s],
            if own { "own look" } else { look.as_str() }
        );
    }
    if !picks.is_empty() {
        inv.mark_equipment_dirty();
        inv.changed();
    }
    Ok(cost)
}

/// The window's Apply Illusion button: the pending picks.
#[derive(Message, Clone, Debug)]
pub struct ApplyIllusions {
    pub picks: Vec<(usize, String)>,
}

pub fn apply_requests(
    mut requests: MessageReader<ApplyIllusions>,
    mut inv: ResMut<Inventory>,
    mut progress: ResMut<gd_game_core::progress::PlayerProgress>,
    data: Res<gd_game_core::data::GameData>,
) {
    let level = progress.progress.level.max(1) as u32;
    for r in requests.read() {
        let mut gold = progress.gold;
        match apply(&data.db, &mut inv, &mut gold, level, &r.picks) {
            Ok(c) => info!("illusion: applied {} picks for {c}", r.picks.len()),
            Err(e) => info!("illusion: {e}"),
        }
        progress.gold = gold;
    }
}

/// A transmute set (`ItemTransmuterSet` record) was used (module doc).
#[derive(Message, Clone, Debug)]
pub struct UseTransmuteSet(pub String);

/// `ItemTransmuterSet::Use` (module doc): unlocks the set's looks, saves them when one is new, plays `useSound`
/// and sends the cinematic text.
#[allow(clippy::too_many_arguments)]
pub fn use_sets(
    mut commands: Commands,
    mut used: MessageReader<UseTransmuteSet>,
    mut ill: ResMut<Illusions>,
    mut inv: ResMut<Inventory>,
    data: Res<gd_game_core::data::GameData>,
    save_dir: Res<gd_game_core::session::SaveDir>,
    choice: Res<gd_game_core::session::Choice>,
    shared: Res<gd_game_core::stash::SharedFiles>,
    mut cinematic: MessageWriter<gd_game_core::story::CinematicText>,
) {
    for UseTransmuteSet(record) in used.read() {
        let Some(rec) = data.db.get(record) else {
            continue;
        };
        // Test hook `GD_SHOT_ON=<set record text>`: shots of the cinematic text.
        gd_game_core::visuals::notify(&mut commands, record);
        let mut new = false;
        for r in rec.strs("transmuteDbrs") {
            let r = r.trim();
            if !r.is_empty() && ill.unlock(&data.db, r) {
                debug!("illusions: unlocked {r}");
                new = true;
            }
        }
        if new {
            let written = ill.file.encode().and_then(|b| {
                crate::save::write_shared(&shared, &choice, &save_dir, Shared::Transmutes, &b)
            });
            if let Err(e) = written {
                warn!("illusions: writing the transmutes file: {e:#}");
            }
        }
        if let Some(s) = rec.str("useSound").filter(|s| !s.is_empty()) {
            inv.sounds.push(s.to_owned());
        }
        let Some(tag) = rec.str("transmuteTag").filter(|t| !t.is_empty()) else {
            continue;
        };
        // The headline is `Localize("tagTransmuteNew", name)`: formatted here (the event carries no headline
        // arguments).
        let tags = data.tags();
        let name = gd_sim::items::tooltip::format_exact(tags.get(tag).unwrap_or(tag), &[]);
        let head = gd_sim::items::tooltip::format_exact(
            tags.get("tagTransmuteNew").unwrap_or("tagTransmuteNew"),
            &[gd_sim::items::tooltip::Arg::Str(name.trim())],
        );
        info!("illusions: {head} ({record}, new: {new})");
        cinematic.write(gd_game_core::story::CinematicText::plain(
            head.trim().to_owned(),
            8,
        ));
    }
}

/// What [`apply_requests`] needs: the bags and the gold.
fn has_apply_inputs(
    inv: Option<Res<Inventory>>,
    progress: Option<Res<gd_game_core::progress::PlayerProgress>>,
) -> bool {
    inv.is_some() && progress.is_some()
}

/// What [`use_sets`] needs: the bags and the unlocked looks.
fn has_set_inputs(inv: Option<Res<Inventory>>, ill: Option<Res<Illusions>>) -> bool {
    inv.is_some() && ill.is_some()
}

pub struct IllusionPlugin;

impl Plugin for IllusionPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ApplyIllusions>()
            .add_message::<UseTransmuteSet>()
            .add_systems(
                Update,
                (
                    init_illusions.run_if(resource_exists::<Inventory>),
                    unlock_items
                        .run_if(resource_exists::<Inventory>)
                        .run_if(resource_exists::<Illusions>),
                    apply_requests.run_if(has_apply_inputs),
                )
                    .chain()
                    .after(gd_game_core::inventory::InventorySet)
                    .before(gd_game_core::player::PlayerSet),
            )
            .add_systems(
                Update,
                use_sets
                    .after(crate::consumables::ConsumableStage::Use)
                    .run_if(has_set_inputs),
            );
    }
}
