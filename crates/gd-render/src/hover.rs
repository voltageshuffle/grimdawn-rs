//! The hover glow markers: the light rig (`render::light_rig`) adds the engine's ambient highlight
//! (`Actor::SetHighlight`, the mesh's `GraphicsMeshInstance.ambientAdd` + 0.25) to the hovered actor that took the
//! outline path and to the ground items whose label the label manager lists. Each marker is cleared the frame
//! the hover / list changes.
use bevy::prelude::*;

/// On the hovered actor (`hover.rs`'s pick), character or object.
#[derive(Component)]
pub struct HoverGlow;

/// On a ground item whose label is listed this frame (`loot.rs`'s `place_labels`).
#[derive(Component)]
pub struct LabelGlow;
