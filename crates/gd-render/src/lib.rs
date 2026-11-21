//! Grim Dawn's look on Bevy: the game's own surface / terrain shading in gamma space, its lighting
//! environment, the deferred renderer's glow and post chain (`render`), models and textures (`model`,
//! `textures`), character bodies (`character`), particle effects (`fx`), decals, water, lightning,
//! trails, beams and the see-through scenery, the game camera (`camera`) and the level lights
//! (`render::lights`). Gameplay is gd-game's; this crate only draws it. The weather / water drawing
//! reads the presentation state (`render::weather::WeatherState`) the app's weather driver writes; the
//! main menu backdrop lights itself through [`render::backdrop::MenuBackdrop`].
//!
//! Like grimdawn's `main.rs`, the gd-game modules the drawing reads are imported at the root so
//! `crate::data::GameData` etc. resolve here too.
// Bevy system parameters (queries with filters, param tuples) are long by nature; Bevy recommends
// allowing this lint for them (as gd-game and grimdawn do).
#![allow(clippy::type_complexity)]
pub mod beams;
pub mod camera;
pub mod chaos_beam;
pub mod character;
pub mod decal;
pub mod fx;
pub mod hover;
pub mod icy;
pub mod lightning;
pub mod model;
pub mod render;
pub mod see_through;
pub mod textures;
pub mod trails;
pub mod water;

use gd_game::{combat, data, hooks, player, story};
