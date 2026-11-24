//! Game rules, independent of rendering: stats, combat, skills, items, nav.
//! The pure rules leaf (equations, game mode, factions, the engine RNG) is
//! [`gd_rules`]; the story / Lua layer and Shattered Realm generator live in
//! `gd_story`. Both compile in parallel with this crate (SPLIT-1).
pub mod actor;
pub mod aim;
pub mod anim;
pub mod ascendant;
pub mod attributes;
pub mod beam;
pub mod bounds;
pub mod catalog;
pub mod combat;
pub mod combo;
pub mod crowd;
pub mod crt;
pub mod devotion;
pub mod dps;
pub mod drop_body;
pub mod edit;
pub mod equip;
pub mod grid;
pub mod items;
pub mod jukebox;
pub mod loot_toss;
pub mod minimap_raster;
pub mod nav;
pub mod patrol;
pub mod pattern;
pub mod potions;
pub mod progress;
pub mod proxy;
pub mod racial;
pub mod reclamation;
pub mod setpiece;
pub mod sheet;
pub mod skill_tooltip;
pub mod skills;
pub mod slots;
pub mod sound3d;
pub mod stats;
pub mod surface;
#[cfg(feature = "testdata")]
pub mod testdata;
pub mod turn;
pub mod weapon_pool;
pub mod weather;
