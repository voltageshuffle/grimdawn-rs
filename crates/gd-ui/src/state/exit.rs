//! The Exit-to-Main-Menu message ([`ExitToMenu`]): the game menu and the Esc dispatcher send it, the
//! binary's `exit.rs` ends the session on it.
use bevy::prelude::*;

/// Sent by the game menu's Exit (after its save): ends the session and shows the main menu.
#[derive(Message)]
pub struct ExitToMenu;
