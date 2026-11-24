//! The fixed actors' use ranges (`player_orders` in gd-game applies them).

/// The use range of a fixed item (`ControllerPlayerState::DefaultRequestInteractableAction`,
/// `gd_game::doors` module doc): the 3D distance between the player's and the object's origins is at
/// most player radius + object radius (`actorRadius` × scale) + `usableRange` (0 when absent), 1 m more once the
/// walk to it has ended (`ControllerPlayerStateMoveToFixedItem::EndOfPathReached`, `path_end`).
pub fn usable_range(player_radius: f32, object_radius: f32, usable: f32, path_end: bool) -> f32 {
    player_radius + object_radius + usable + if path_end { 1.0 } else { 0.0 }
}
