//! The game's pure rules leaf: equation strings ([`expr`]), the session game
//! mode ([`mode`]), faction standing maths ([`faction`]) and the engine's
//! random stream ([`Rng`]). No Bevy, no I/O beyond reading a [`gd_formats`]
//! record.
//!
//! Shared by the two big consumers — gd-sim (combat, items, proxies) and
//! gd-story (quests, Lua, the Shattered Realm generator) — so they use these
//! rules without depending on each other and compile in parallel.
//! Layering: `gd-formats → gd-rules → {gd-sim ∥ gd-story}`.
pub mod expr;
pub mod faction;
pub mod mode;
pub mod rng;

pub use rng::Rng;
