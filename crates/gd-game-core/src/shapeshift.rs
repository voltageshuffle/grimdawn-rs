//! The shape-change system set.

/// The body swap of a shape change; what draws the character orders after it.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShapeshiftSet {
    Wear,
}
