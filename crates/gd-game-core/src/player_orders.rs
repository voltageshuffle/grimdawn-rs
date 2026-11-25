//! The fixed actors' use ranges.

/// Use range of a fixed item (`ControllerPlayerState::DefaultRequestInteractableAction`): player radius + object
/// radius + `usableRange`, 1 m more once the walk has ended (`ControllerPlayerStateMoveToFixedItem::EndOfPathReached`).
pub fn usable_range(player_radius: f32, object_radius: f32, usable: f32, path_end: bool) -> f32 {
    player_radius + object_radius + usable + if path_end { 1.0 } else { 0.0 }
}
