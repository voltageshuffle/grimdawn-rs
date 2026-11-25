//! The Inventor's convert tab: a set item becomes another piece of the same
//! set, or a piece of a random other set of its rarity.
//!
//! (GrimDawn.exe `UIEnchanterConvertTab`: loader `UIEnchanterConvertTab::Load`,
//! update `UIEnchanterConvertTab::Update`, ready test `UIEnchanterConvertTab::IsReady`, button handler
//! `UIEnchanterConvertTab::OnButton`, convert `UIEnchanterConvertTab::Convert`; Game.dll
//! `ItemSetFormula::Load`, `ItemRandomSetFormula::Load`, `GetItemSetNames`):
//! - The tab record (`convert_table.dbr`) names three formulas:
//!   `sameSetRecipe` (`ItemSetFormula`), `randomSetRecipe` and
//!   `randomSetRecipeLegendary` (`ItemRandomSetFormula`, with
//!   `randomItemSets`), and `blacklistedSets`.
//! - The item slot (`reagentBaseBox`, `UIEnchanterConvertTab::CanAcceptItem` / `InventorItemBox::AddItem`)
//!   takes any `ItemEquipment` whose record has an `itemSetName`.
//! - The radio buttons `sameSetButton` (the default) / `randomSetButton` pick
//!   the mode; in random mode a Legendary item (`GetItemClassification` = 4)
//!   uses `randomSetRecipeLegendary`, anything else `randomSetRecipe`.
//! - A formula costs `creationCost` (an equation, `GetCreationCost`:
//!   `(u32)(max(eval, 1) + 0.5)`) and its `reagent1..6BaseName` /
//!   `...Quantity`; the create button needs the item, every reagent, the
//!   money and `IsBluePrintValid`.
//! - Pressing it: an item of a `blacklistedSets` set only shows the
//!   `tagConvertBlacklist` dialog. Otherwise the target set is the item's
//!   own (`sameSetRecipe` has no `randomItemSets`), or: `randomItemSets`
//!   without the item's set, shuffled (CRT `rand`), the first whose set
//!   record `itemLevel` is within the item's `itemLevel` ± 12 (inclusive).
//!   The new item is a uniform pick (`IGenerate(0, n - 1)`) of the target
//!   set's `setMembers` without the item's own record, made with a fresh
//!   seed and nothing else (no affixes, component or augment).
//! - `CreateArtifactConfigCmd::Execute`: money −= cost, the
//!   slot item is destroyed (`TakeItemFromCharacter`), the reagents taken,
//!   the new item given (`Player::GiveSetItemToCharacter`: into the shown
//!   convert tab's slot).
use super::ItemDb;
use super::db::Fields;
use gd_formats::arz::Record;
use gd_rules::Rng;

pub const TABLE: &str = "records/ui/inventor/convertpanel/convert_table.dbr";

/// How far (levels) a random target set's `itemLevel` may be from the item's.
pub const LEVEL_RANGE: i32 = 12;

/// A convert formula (`ItemSetFormula` / `ItemRandomSetFormula`).
#[derive(Debug, Clone, PartialEq)]
pub struct SetFormula {
    pub record: String,
    cost: String,
    /// `reagent1..6BaseName` with their quantity.
    pub reagents: Vec<(String, u32)>,
    /// `randomItemSets` (empty: the item's own set).
    pub random_sets: Vec<String>,
}

impl SetFormula {
    pub fn load(record: &str, rec: &Record) -> Self {
        let reagents = (1..=6)
            .filter_map(|n| {
                let name = rec
                    .str(&format!("reagent{n}BaseName"))
                    .filter(|s| !s.is_empty())?;
                let q = rec.int(&format!("reagent{n}Quantity"));
                (q > 0).then(|| (name.to_owned(), q as u32))
            })
            .collect();
        Self {
            record: record.to_owned(),
            cost: rec.str("creationCost").unwrap_or("0").to_owned(),
            reagents,
            random_sets: rec
                .strs("randomItemSets")
                .into_iter()
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
        }
    }

    /// Iron bits it costs at `player_level` (`GetCreationCost`).
    pub fn cost(&self, player_level: u32) -> u64 {
        super::cost_at(&self.cost, player_level)
    }
}

/// The formula record the tab uses: `random` = the random-set mode;
/// `legendary` = the item is Legendary.
pub fn formula_record(table: &Record, random: bool, legendary: bool) -> Option<&str> {
    let field = match (random, legendary) {
        (false, _) => "sameSetRecipe",
        (true, true) => "randomSetRecipeLegendary",
        (true, false) => "randomSetRecipe",
    };
    table.str(field).filter(|s| !s.is_empty())
}

/// Whether the tab refuses items of `set` (`blacklistedSets`).
pub fn blacklisted(table: &Record, set: &str) -> bool {
    table
        .strs("blacklistedSets")
        .iter()
        .any(|s| s.eq_ignore_ascii_case(set))
}

/// The set the item converts into: its own (`item_set`) for a formula
/// without `randomItemSets`, else a random listed set other than its own
/// whose `itemLevel` is within `item_level` ± [`LEVEL_RANGE`].
pub fn target_set(
    db: &ItemDb,
    formula: &SetFormula,
    item_set: &str,
    item_level: u32,
    rng: &mut Rng,
) -> Option<String> {
    if formula.random_sets.is_empty() {
        return Some(item_set.to_owned());
    }
    // A level past i32::MAX or within LEVEL_RANGE of it clamps.
    let level = i32::try_from(item_level).unwrap_or(i32::MAX);
    let (lo, hi) = (
        level.saturating_sub(LEVEL_RANGE),
        level.saturating_add(LEVEL_RANGE),
    );
    // PORT: the engine shuffles with CRT `rand` and takes the first match;
    // here a uniform pick among the matches from `rng`.
    let fits: Vec<&String> = formula
        .random_sets
        .iter()
        .filter(|s| !s.eq_ignore_ascii_case(item_set))
        .filter(|s| {
            db.get(s)
                .is_some_and(|r| (lo..=hi).contains(&r.int("itemLevel")))
        })
        .collect();
    if fits.is_empty() {
        return None;
    }
    Some(fits[(rng.next() % fits.len() as u32) as usize].clone())
}

/// The record the item (`base_name`, of `item_set` at `item_level`) becomes
/// with `formula`; `None` when no set or member fits.
pub fn convert(
    db: &ItemDb,
    formula: &SetFormula,
    base_name: &str,
    item_set: &str,
    item_level: u32,
    rng: &mut Rng,
) -> Option<String> {
    let set = target_set(db, formula, item_set, item_level, rng)?;
    let members: Vec<String> = db
        .get(&set)?
        .strs("setMembers")
        .into_iter()
        .filter(|m| !m.is_empty() && !m.eq_ignore_ascii_case(base_name))
        .map(str::to_owned)
        .collect();
    if members.is_empty() {
        return None;
    }
    Some(members[(rng.next() % members.len() as u32) as usize].clone())
}
