//! The ground-loot label / hide view state: the label the cursor is on ([`HoveredLabel`]), the
//! loot filter's hide-all flag ([`ItemsHidden`]) and the filtered-out marker ([`FilteredOut`]).
//! The binary's `loot.rs` writes them; `gd_ui`'s loot filter / tooltip / HUD read them.
use bevy::prelude::*;

/// The ground item whose label the cursor is on (`label_at`), set by `place_labels` every frame.
#[derive(Resource, Default)]
pub struct HoveredLabel(pub Option<Entity>);

/// A ground item the loot filter hides in the current `LabelMode`.
#[derive(Component)]
pub struct FilteredOut;

/// "Toggle Hide All Items" is on (the engine's HUD flag `UIPlayerHud.itemsShown` is the opposite).
#[derive(Resource, Default)]
pub struct ItemsHidden(pub bool);
