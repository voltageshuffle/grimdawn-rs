//! The shape-change system set gameplay and presentation order against.

/// The body swap of a shape change (`gd_game::shapeshift::wear_form`): what draws the character orders after it.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShapeshiftSet {
    Wear,
}
