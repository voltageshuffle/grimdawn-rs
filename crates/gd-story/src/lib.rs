//! The story layer: quests (`.qst`), conversations (`.cnv`), area triggers,
//! the game's Lua 5.1 scripts ([`story`]) and the Shattered Realm dungeon
//! generator ([`endless`]). Bevy-free.
//!
//! Split out of gd-sim (SPLIT-1) so the vendored Lua build and ~7.4k lines of
//! story code compile in parallel with gd-sim's combat/items core, and so
//! editing a quest binding never rebuilds the simulation. The shared rules
//! (equations, mode, factions, RNG) live in [`gd_rules`]; the inventory grid
//! types in [`gd_formats::inventory`]. The install-backed [`story::lua::ScriptEnv`]
//! implementation lives in gd-game (`gd_game::story_env`), which also resolves
//! item footprints through gd-sim. Layering:
//! `gd-formats → gd-rules → {gd-sim ∥ gd-story} → gd-game`.
pub mod endless;
pub mod story;
