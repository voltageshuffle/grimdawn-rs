//! The gameplay's data leaf: components, resources, messages, pure resolvers
//! and system sets. No systems live here — `gd-game` adds those; `gd-render`
//! reads this crate directly. Layering: `docs/architecture.md`.
// Bevy system parameters are long by nature.
#![allow(clippy::type_complexity)]
pub mod afflictions;
pub mod ascendant;
pub mod casting;
pub mod char_sounds;
pub mod character;
pub mod combat;
pub mod combat_sounds;
pub mod consumables;
pub mod creature;
pub mod data;
pub mod day_clock;
pub mod entry;
pub mod factions;
pub mod floating;
#[cfg(feature = "dev")]
pub mod hooks;
pub mod illusion;
pub mod inventory;
pub mod levels;
pub mod monster_skills;
pub mod monsters;
pub mod notify;
pub mod options;
pub mod pause;
pub mod pets;
pub mod player;
pub mod player_orders;
pub mod progress;
pub mod reflect_link;
pub mod riftgates;
pub mod rng;
pub mod rules;
pub mod save;
pub mod secondary;
pub mod session;
pub mod shapeshift;
pub mod sheet_dps;
pub mod skill_buffs;
pub mod skill_fx;
pub mod stack;
pub mod stash;
pub mod story;
pub mod story_env;
pub mod synergy;
pub mod turning;
pub mod visuals;
pub mod weak_cache;
pub mod weapon_pool;
pub mod world;
