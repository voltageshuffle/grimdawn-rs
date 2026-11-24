//! Blacksmith crafting (`ItemArtifactFormula` blueprints at an `NpcCrafter`).
//!
//! (Game.dll / Grim Dawn.exe):
//! - `ItemArtifactFormula::Load`: `reagentBaseBaseName` +
//!   `reagent1..6BaseName` (string arrays: any listed record counts) with
//!   `...Quantity`; `artifactName` (what is made), `artifactCreateQuantity`,
//!   `artifactCreationCost` (an equation; `GetCreationCost` =
//!   `(u32)(max(eval, 1) + 0.5)` with the player level), `forcedRelicCompletion`.
//! - The crafting tab's craft handler (exe `UICraftingTab::Craft`): `artifactName` is a
//!   `LootItemTable_DynWeight`, a `LootMasterTable` or a plain item record,
//!   rolled at the player's level and difficulty; then, if the crafter has an
//!   `enhancementTable` (a `LootRandomizerTable`), one entry of it becomes the
//!   item's `modifierName` (the crafting bonus).
//! - `CreateArtifactConfigCmd::Execute`: money -= cost, each
//!   reagent is taken (`TakeReagents`: from the character's bags
//!   first, then the blacksmith's material storage (`reagents.gst`), the
//!   personal stash, the transfer stash), then `artifactCreateQuantity` items
//!   are given.
//! - Recipes offered (exe `CraftList::LoadCrafter`, `NpcCrafter::Load`): the
//!   crafter's `defaultRecipes`; with `awakenedRecipes` the game's awakened
//!   list (`GameEngine::GetAwakenedRecipes`, `awakenedRecipes` of the
//!   ascendant game record); unless it `restrictRecipes`, the blueprints the
//!   player learned (`formulas.gst`) and the window's `craftingDefaultRecipes`
//!   (merge order, exe `CraftList::Build`; the window then groups and sorts
//!   the list for display).
//! - [`formula_valid`] = `Item::IsBluePrintValid` (virtual): an invalid formula is
//!   refused on use (`ItemArtifactFormula::AllowUse`: "cannot" sound, kept), dropped when
//!   `formulas.gst` is read (`GameEngine::StreamPlayerFormulas`) and from a crafter's list (exe
//!   `CraftList::LoadCrafter`). Only sandbox / test records fail in stock data.
use super::ItemDb;
use super::loot::{LootParams, randomizer_pick, roll_table};
use gd_formats::arz::Record;
use gd_formats::save::item::Item;
use gd_rules::Rng;

/// One blueprint.
#[derive(Debug, Clone, PartialEq)]
pub struct Blueprint {
    pub record: String,
    /// Loot table or item record that is made.
    pub artifact: String,
    /// The item shown in the window (`forcedRandomArtifactName`, else `artifact`).
    pub display: String,
    pub quantity: u32,
    cost: String,
    /// Base reagent first, then reagents 1..6: (accepted records, quantity).
    pub reagents: Vec<(Vec<String>, u32)>,
}

impl Blueprint {
    pub fn load(record: &str, rec: &Record) -> Option<Self> {
        let artifact = rec
            .str("artifactName")
            .filter(|s| !s.is_empty())?
            .to_string();
        let display = made_item(rec)?.to_string();
        let mut reagents = Vec::new();
        for k in ["Base", "1", "2", "3", "4", "5", "6"] {
            let names: Vec<String> = rec
                .strs(&format!("reagent{k}BaseName"))
                .into_iter()
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            let n = rec.i32(&format!("reagent{k}Quantity")).unwrap_or(0);
            if !names.is_empty() && n > 0 {
                reagents.push((names, n as u32));
            }
        }
        Some(Self {
            record: record.to_string(),
            artifact,
            display,
            quantity: rec.i32("artifactCreateQuantity").unwrap_or(1).max(1) as u32,
            cost: rec.str("artifactCreationCost").unwrap_or("0").to_string(),
            reagents,
        })
    }

    /// Iron bits it costs at `player_level`.
    pub fn cost(&self, player_level: u32) -> u64 {
        super::cost_at(&self.cost, player_level)
    }

    /// Makes the items: `artifact` rolled with `p` (at its `parent_level`),
    /// each with a bonus from the crafter's `enhancement` table.
    pub fn craft(
        &self,
        db: &ItemDb,
        p: &LootParams,
        enhancement: Option<&str>,
        rng: &mut Rng,
    ) -> Vec<Item> {
        let level = p.parent_level;
        let Some(roll) = roll_table(db, &self.artifact, p, rng) else {
            return Vec::new();
        };
        let mut item = roll.to_item();
        if let Some(t) = enhancement.filter(|t| !t.is_empty()) {
            item.modifier_name = randomizer_pick(db, t, level, rng);
        }
        vec![item; self.quantity as usize]
    }
}

/// The recipes a crafter offers (`crafter` = its `NpcCrafter` record,
/// `window_defaults` = `craftingDefaultRecipes`, `learned` = `formulas.gst`),
/// without the awakened list; see [`recipes_with_awakened`].
pub fn recipes(crafter: &Record, window_defaults: &[&str], learned: &[&str]) -> Vec<String> {
    recipes_with_awakened(crafter, &[], window_defaults, learned)
}

/// The recipes a crafter offers, in the game's merge order (exe `CraftList::Build`):
/// its `defaultRecipes`, `awakened` (the ascendant game record's
/// `awakenedRecipes`) when the crafter has `awakenedRecipes`, then unless it
/// `restrictRecipes` the learned blueprints and the window defaults.
pub fn recipes_with_awakened(
    crafter: &Record,
    awakened: &[&str],
    window_defaults: &[&str],
    learned: &[&str],
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |r: &str| {
        if !r.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(r)) {
            out.push(r.to_string());
        }
    };
    crafter
        .strs("defaultRecipes")
        .into_iter()
        .for_each(&mut add);
    if crafter.bool("awakenedRecipes") {
        awakened.iter().copied().for_each(&mut add);
    }
    if !crafter.bool("restrictRecipes") {
        learned.iter().copied().for_each(&mut add);
        window_defaults.iter().copied().for_each(&mut add);
    }
    out
}

/// The item a blueprint makes and shows (`ItemArtifactFormula::LoadArtifact`:
/// `Item::CreateItem` of `forcedRandomArtifactName`, else `artifactName`): the faction blueprints' `artifactName` is
/// a loot table (Devil's Cord: `tdyn_craft_waist_f01a`) and their item the forced one. None without `artifactName`.
pub fn made_item(rec: &Record) -> Option<&str> {
    let artifact = rec.str("artifactName").filter(|s| !s.is_empty())?;
    Some(
        rec.str("forcedRandomArtifactName")
            .filter(|s| !s.is_empty())
            .unwrap_or(artifact),
    )
}

/// `IsBluePrintValid` (module doc), per formula class. Game.dll:
/// - `ItemArtifactFormula::IsBluePrintValid`: some `reagent1..6Quantity` list non-empty or `reagentBaseQuantity` != 0,
///   and the made item exists (`LoadArtifact`: `artifactName` non-empty; `Item::CreateItem` of
///   `forcedRandomArtifactName`, else `artifactName`, returns an item).
/// - `ItemSetFormula` / `ItemAscensionFormula` (folded into `ItemSetFormula::IsBluePrintValid`): some reagent list
///   non-empty or base != 0.
/// - `ItemRerollFormula::IsBluePrintValid`: some `reagent1..6Quantity` list non-empty (base ignored).
/// - `AscendantAltarFormula::IsBluePrintValid`: `reagentBaseQuantity` != 0.
///
/// PORT: `ItemRandomSetFormula` (not traced) takes the set formula rule; "an item" is [`is_item_class`].
pub fn formula_valid(db: &(impl super::RecordSource + ?Sized), rec: &Record) -> bool {
    let listed = |k: &str| {
        rec.get(&format!("reagent{k}Quantity"))
            .is_some_and(|v| !v.is_empty())
    };
    let reagents = ["1", "2", "3", "4", "5", "6"].into_iter().any(listed);
    let base = rec.i32("reagentBaseQuantity").unwrap_or(0) != 0;
    match rec.class.as_str() {
        "ItemArtifactFormula" => {
            let Some(made) = made_item(rec) else {
                return false;
            };
            (reagents || base) && db.record(made).is_some_and(|r| is_item_class(&r.class))
        }
        "ItemSetFormula" | "ItemAscensionFormula" | "ItemRandomSetFormula" => reagents || base,
        "ItemRerollFormula" => reagents,
        "AscendantAltarFormula" => base,
        _ => false,
    }
}

/// Whether records of `class` are items (`Item::CreateItem` makes an object of them).
/// PORT: the engine tests the class's `classInfo` chain; the port takes the item class families of the stock
/// database (`Armor*`, `Weapon*`, `Item*`, `OneShot_*`, `QuestItem`).
pub fn is_item_class(class: &str) -> bool {
    ["Armor", "Weapon", "Item", "OneShot_"]
        .iter()
        .any(|p| class.starts_with(p))
        || class == "QuestItem"
}
