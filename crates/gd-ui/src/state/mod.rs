//! The presentation state the binary's input / world systems share with the UI windows: each piece is
//! written by a binary system and read (or drawn) by a `gd_ui` window, or vice versa. Gameplay rules stay
//! in gd-game; this is only what the UI shows and the input remembers.
pub mod ascendant;
pub mod casting;
pub mod crafting;
pub mod death;
pub mod exit;
pub mod hover;
pub mod illusion;
pub mod inventory;
pub mod loot;
pub mod map;
pub mod options;
pub mod pets;
pub mod riftmap;
pub mod session;
pub mod survival;
pub mod trade;
