//! Items and loot (Bevy-free).
//!
//! - [`resolve`]: record names + seed → exact attribute values, requirements,
//!   classification, name tags (`ItemEquipment::InitializeItem`).
//! - [`naming`]: display names from the tag table.
//! - [`loot`]: loot tables, affix rolling, monster/container drops.
//! - [`sets`]: item set bonuses; [`inventory`]: sack grids and equip slots.
pub mod components;
pub mod convert;
pub mod crafting;
pub mod db;
pub mod display_tags;
pub mod illusion;
pub mod inventory;
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
