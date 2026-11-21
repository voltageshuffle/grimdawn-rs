//! The test hooks ([`gd_game::hooks::Hooks`], documented there): `main` builds them once, `Hooks::from_env()` in
//! `dev` builds and the inert `Hooks::default()` otherwise, and inserts the resource before any plugin, so plugin
//! builds, `Default`s and systems all read the same `Hooks` from the world.
pub use gd_game::hooks::Hooks;
