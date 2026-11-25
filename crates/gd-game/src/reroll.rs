//! The Inventor's reroll panel (Fangs of Asterkarn): a new seed for an item, a new prefix / suffix or a new
//! ascendant affix; and the Runekeeper's item ascension.
//!
//! The reroll panel (`UIEnchanterRerollTab`, Game.dll `ItemArtifactFormula::GetRerollCost` /
//! `GetReagent<n>QuantityForReroll`): the panel (`reroll_table.dbr`) names a `seedRecipe` (`ItemRerollFormula`), an
//! `affixRecipe` and `ascended<Rarity>Recipe`s (`ItemAscensionFormula`); the cost (`rerollCost`) and each reagent's
//! quantity (`reagent<n>Quantity`) are indexed by the item's reroll count (seed rerolls for the seed, affix rerolls
//! otherwise): index 0 for counts 0 and 1, else count − 1, clamped; a seed reroll draws a new random seed, the
//! others a different affix; the count goes up by one (prefix, suffix and ascendant share `affix_rerolls`). Items
//! whose record sets `preventReroll` are refused. Using the panel needs the `gameengine.dbr` `rerollToken`.
//! Button states (`UpdateButtonStates`): the seed button needs an item; the prefix / suffix buttons need the item to
//! have that affix and are off for Epic and Legendary items; the ascendant button needs an ascendant affix.
//! Reagents are taken from the bags, the material store, the personal stash and the transfer stash, in that order,
//! as for crafting. The reroll runs at once (no confirm prompt; the item stays in the slot and is changed in place).
//!
//! The Runekeeper (`NpcItemAscension`, `UIItemAscensionWindow::AscendItem`): the item's rarity picks the window's
//! `<rarity>Recipe`, which costs its `creationCost` and first reagent quantities and draws the affix as an ascended
//! reroll does (no counter, no token). Only items without an ascendant affix; `preventReroll` is not checked. The
//! ascend button runs at once, no prompt.
use bevy::prelude::*;
use gd_formats::save::item::Item;
use gd_game_core::inventory::Inventory;
use gd_game_core::stash::Stash;
use gd_sim::items::ItemDb;
use gd_sim::items::resolve::Classification;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Seed,
    Prefix,
    Suffix,
    Ascended,
    /// The Runekeeper's item ascension: an ascendant affix for an item without one.
    Ascend,
}

impl Kind {
    /// The text `Rerolled (..)` shows.
    fn name(self) -> &'static str {
        match self {
            Kind::Seed => "Seed",
            Kind::Prefix => "Prefix",
            Kind::Suffix => "Suffix",
            Kind::Ascended => "Ascended",
            Kind::Ascend => "Ascend",
        }
    }
}

pub struct Plan {
    pub cost: u64,
    pub reagents: Vec<(String, u32)>,
}

/// The quantity / cost index for a reroll count.
pub fn index(count: u32, len: usize) -> usize {
    if count == 0 {
        0
    } else {
        (count as usize - 1).min(len.saturating_sub(1))
    }
}

/// `UIEnchanterRerollTab::AcceptsItem`: equipment that can be rerolled; the Runekeeper only needs equipment.
pub fn accepts(idb: &ItemDb, item: &Item, ascension: bool) -> Result<(), String> {
    let resolved = gd_sim::items::resolve(idb, item);
    if resolved
        .as_ref()
        .is_none_or(|r| !gd_sim::items::resolve::is_equipment_class(&r.class))
    {
        Err("Only equipment can be rerolled".to_owned())
    } else if !ascension
        && idb
            .get(&item.base_name)
            .is_some_and(|r| gd_sim::items::db::Fields::int(r, "preventReroll") != 0)
    {
        Err("This item cannot be rerolled".to_owned())
    } else {
        Ok(())
    }
}

/// Whether `kind` can be used on the item (`UIEnchanterRerollTab::UpdateButtonStates`; the Runekeeper needs no
/// ascendant affix yet).
pub fn kind_state(idb: &ItemDb, item: &Item, kind: Kind, test: bool) -> Result<(), String> {
    let epic = gd_sim::items::resolve(idb, item).is_some_and(|r| {
        matches!(
            r.classification,
            Classification::Epic | Classification::Legendary
        )
    });
    match kind {
        Kind::Ascend if !item.ascendant_name.is_empty() => {
            Err("The item is already ascended".to_owned())
        }
        Kind::Ascended if item.ascendant_name.is_empty() && !test => {
            Err("The item has no ascendant affix".to_owned())
        }
        Kind::Prefix | Kind::Suffix if epic => {
            Err("Epic and legendary affixes cannot be rerolled".to_owned())
        }
        Kind::Prefix if item.prefix_name.is_empty() => Err("The item has no such affix".to_owned()),
        Kind::Suffix if item.suffix_name.is_empty() => Err("The item has no such affix".to_owned()),
        _ => Ok(()),
    }
}

/// The cost and reagents of `kind` for the item (`None` without a recipe).
pub fn plan(
    data: &gd_game_core::data::GameData,
    idb: &ItemDb,
    item: &Item,
    kind: Kind,
) -> Option<Plan> {
    let class = gd_sim::items::resolve(idb, item)
        .map(|r| r.classification.name())
        .unwrap_or_default();
    let recipe = match kind {
        Kind::Seed => gd_sim::ascendant::reroll_recipe(idb, "seed", class),
        Kind::Prefix | Kind::Suffix => gd_sim::ascendant::affix_recipe(idb),
        Kind::Ascended => gd_sim::ascendant::reroll_recipe(idb, "ascended", class),
        Kind::Ascend => gd_sim::ascendant::ascension_recipe(idb, class),
    }
    .and_then(|r| data.db.get(&r))?;
    let count = match kind {
        Kind::Seed => item.seed_rerolls,
        Kind::Ascend => 0,
        _ => item.affix_rerolls,
    };
    let costs = recipe.f32s(if kind == Kind::Ascend {
        "creationCost"
    } else {
        "rerollCost"
    });
    let cost = costs.get(index(count, costs.len())).copied().unwrap_or(1.0) as u64;
    let reagents = (1..=6)
        .filter_map(|n| {
            let name = recipe
                .str(&format!("reagent{n}BaseName"))
                .filter(|s| !s.is_empty())?
                .to_owned();
            let q = recipe.f32s(&format!("reagent{n}Quantity"));
            let q = q.get(index(count, q.len())).copied().unwrap_or(1.0) as u32;
            Some((name, q))
        })
        .collect();
    Some(Plan { cost, reagents })
}

/// Why the plan cannot be paid yet.
pub fn shortfall(
    plan: &Plan,
    gold: u64,
    inv: &Inventory,
    stash: &gd_game_core::stash::Stash,
) -> Option<String> {
    if gold < plan.cost {
        return Some(format!("Rerolling costs {} iron bits", plan.cost));
    }
    // A reagent listed twice needs both amounts.
    crate::crafting::summed_reagents(plan.reagents.iter().map(|(r, q)| (r, *q)))
        .into_iter()
        .find(|(r, q)| gd_game_core::stash::count(std::slice::from_ref(*r), inv, stash) < *q)
        .map(|(r, q)| {
            format!(
                "Rerolling needs {q} x {}",
                r.rsplit('/').next().unwrap_or("")
            )
        })
}

/// Rerolls `item` (`kind`) when the rules allow it: pays the plan's gold and reagents; `Ok` / `Err` is the notice.
/// `unlocked`: the reroll token is known; `test`: a test hook that also gets the costs.
#[allow(clippy::too_many_arguments)]
pub fn reroll_item(
    data: &gd_game_core::data::GameData,
    idb: &ItemDb,
    item: &mut Item,
    kind: Kind,
    unlocked: bool,
    test: bool,
    masteries: &[usize],
    level: u32,
    gold: &mut u64,
    inv: &mut Inventory,
    stash: &mut Stash,
) -> Result<String, String> {
    if !unlocked && kind != Kind::Ascend {
        return Err("Rerolling is not unlocked yet".to_owned());
    }
    accepts(idb, item, kind == Kind::Ascend)?;
    kind_state(idb, item, kind, test)?;
    let plan = plan(data, idb, item, kind).ok_or("No reroll recipe for this item")?;
    if test {
        crate::loot::add_gold(gold, plan.cost as i64);
        for (r, q) in &plan.reagents {
            for _ in 0..*q {
                inv.add_to_bags(Item {
                    base_name: r.clone(),
                    stack_count: 1,
                    seed: 1,
                    ..Default::default()
                });
            }
        }
    }
    if let Some(w) = shortfall(&plan, *gold, inv, stash) {
        return Err(w);
    }
    let resolved = gd_sim::items::resolve(idb, item);
    let class = resolved
        .as_ref()
        .map(|r| r.classification.name())
        .unwrap_or_default();
    let mut rng = gd_rules::Rng(gd_game_core::rng::u32(1..0x7FFF_FFFF));
    match kind {
        Kind::Seed => {
            item.seed = gd_game_core::rng::u32(1..0x7FFF_FFFF);
            item.seed_rerolls += 1;
        }
        Kind::Prefix | Kind::Suffix => {
            let prefix = kind == Kind::Prefix;
            let current = if prefix {
                item.prefix_name.clone()
            } else {
                item.suffix_name.clone()
            };
            let a = resolved
                .as_ref()
                .and_then(|r| {
                    gd_sim::ascendant::reroll_affix(
                        idb,
                        &r.class,
                        r.item_level,
                        prefix,
                        &current,
                        &mut rng,
                    )
                })
                .ok_or("No other affix")?;
            if prefix {
                item.prefix_name = a;
            } else {
                item.suffix_name = a;
            }
            item.affix_rerolls += 1;
        }
        Kind::Ascend => {
            item.ascendant_name = resolved
                .as_ref()
                .and_then(|r| {
                    gd_sim::ascendant::ascend_item(idb, &r.class, class, masteries, level, &mut rng)
                })
                .ok_or("No ascendant affix for this item")?;
        }
        Kind::Ascended => {
            item.ascendant_name = resolved
                .as_ref()
                .and_then(|r| {
                    gd_sim::ascendant::reroll_ascended(
                        idb,
                        &r.class,
                        class,
                        masteries,
                        level,
                        &item.ascendant_name,
                        &mut rng,
                    )
                })
                .ok_or("No other ascendant affix")?;
            item.affix_rerolls += 1;
        }
    }
    *gold -= plan.cost;
    for (r, q) in &plan.reagents {
        gd_game_core::stash::take(std::slice::from_ref(r), *q, inv, stash);
    }
    let affix = match kind {
        Kind::Seed => String::new(),
        Kind::Prefix => item.prefix_name.clone(),
        Kind::Suffix => item.suffix_name.clone(),
        Kind::Ascended | Kind::Ascend => item.ascendant_name.clone(),
    };
    Ok(format!(
        "Rerolled ({}) for {} iron bits: {}",
        kind.name(),
        plan.cost,
        affix.rsplit('/').next().unwrap_or("")
    ))
}

/// [`apply_reroll`] runs [`reroll_item`] for each [`RerollRequest`] and answers with a
/// [`crate::crafting::SlotReturn`].
pub struct RerollPlugin;

impl Plugin for RerollPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<RerollRequest>().add_systems(
            Update,
            apply_reroll
                .run_if(resource_exists::<Inventory>)
                .run_if(resource_exists::<Stash>)
                .run_if(resource_exists::<gd_game_core::progress::PlayerProgress>)
                .run_if(resource_exists::<gd_game_core::data::GameData>)
                .after(gd_game_core::inventory::InventorySet)
                .before(gd_game_core::player::PlayerSet),
        );
    }
}

/// `test`: the `GD_REROLL` hook (the token is not needed and the costs are provided).
#[derive(Message, Clone, Debug)]
pub struct RerollRequest {
    pub item: Item,
    pub kind: Kind,
    pub test: bool,
}

#[allow(clippy::too_many_arguments)]
fn apply_reroll(
    mut msgs: MessageReader<RerollRequest>,
    mut back: MessageWriter<crate::crafting::SlotReturn>,
    data: Res<gd_game_core::data::GameData>,
    mut inv: ResMut<Inventory>,
    mut stash: ResMut<Stash>,
    mut progress: ResMut<gd_game_core::progress::PlayerProgress>,
    // Option: the story (the unlock token) exists only in a session.
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    mut notify: MessageWriter<gd_game_core::story::Notify>,
    players: Query<&gd_game_core::player::Loadout, With<gd_game_core::player::Player>>,
) {
    for m in msgs.read() {
        let mut item = m.item.clone();
        let idb = data.item_db();
        let unlocked = m.test || crate::crafting::reroll_unlocked(&data, rt.as_deref());
        let masteries: Vec<usize> = players
            .single()
            .map(|l| {
                data.skill_trees()
                    .learned(&l.skills)
                    .map(|t| t.class.saturating_sub(1) as usize)
                    .collect()
            })
            .unwrap_or_default();
        let level = progress.progress.level.max(1) as u32;
        let result = reroll_item(
            &data,
            &idb,
            &mut item,
            m.kind,
            unlocked,
            m.test,
            &masteries,
            level,
            &mut progress.gold,
            &mut inv,
            &mut stash,
        );
        let text = result.unwrap_or_else(|e| e);
        info!("reroll: {text}");
        notify.write(gd_game_core::story::Notify(text));
        inv.changed();
        back.write(crate::crafting::SlotReturn {
            item: Some(item),
            go: false,
        });
    }
}
