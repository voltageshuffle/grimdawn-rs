//! The game's pure rules leaf: equation strings ([`expr`]), the session game
//! mode ([`mode`]), faction standing maths ([`faction`]) and the engine's
//! random stream ([`Rng`]). No Bevy, no I/O beyond reading a [`gd_formats`]
//! record.
//!
//! Shared by gd-sim and gd-story so neither depends on the other.
//! Layering: `gd-formats → gd-rules → {gd-sim ∥ gd-story}`.
pub mod expr;
pub mod faction;
pub mod mode;
pub mod rng;

pub use rng::Rng;
