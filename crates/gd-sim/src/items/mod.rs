//! Items and loot (Bevy-free).
//!
//! - [`resolve`]: record names + seed → exact attribute values, requirements,
//!   classification, name tags (`ItemEquipment::InitializeItem`).
//! - [`naming`]: display names from the tag table.
//! - [`loot`]: loot tables, affix rolling, monster/container drops.
//! - [`sets`]: item set bonuses. The sack grids and equip slots are
//!   [`gd_formats::inventory`].
pub mod components;
pub mod convert;
pub mod crafting;
pub mod db;
pub mod display_tags;
pub mod illusion;
pub mod item_style;
pub mod jitter;
pub mod loot;
pub mod loot_filter;
pub mod market;
pub mod naming;
pub(crate) mod order;
pub mod req_reduction;
pub mod resolve;
pub mod sets;
pub mod tooltip;
pub mod tooltip_item;

pub use db::{ItemDb, RecordSource};
pub use loot::{LootParams, LootRoll, roll_loot, roll_table};
pub use resolve::{Classification, ResolvedItem, resolve};

/// Iron bits a creation-cost equation costs at `player_level`
/// (`GetCreationCost`: `(u32)(max(eval, 1) + 0.5)`;
/// [`convert::SetFormula::cost`], [`crafting::Blueprint::cost`]).
pub(crate) fn cost_at(cost: &str, player_level: u32) -> u64 {
    let v = gd_rules::expr::eval_var(cost, "playerLevel", player_level as f32).unwrap_or(0.0);
    (v.max(1.0) + 0.5) as u64
}
