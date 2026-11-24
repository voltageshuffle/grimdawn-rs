//! The click-modifier keyboard ([`ClickKeys`]) and the world-click input stages ([`CastInputStage`]):
//! the binary's casting input orders against them, the UI windows order before them.
use bevy::prelude::*;

/// The keyboard with the held click modifiers: Stationary Attack (action 39, default Shift) and Force Move (action
/// 50, default F). `Grim Dawn.exe` key handler `InGameUI::HandleKeyEvent`: each sets its byte of the input
/// controller while a bound key is down (39 → `InGameUI.stationaryHeld`, 50 → `InGameUI.forceMoveHeld`;
/// cleared on the key's release); the click handler `InGameUI::HandleMouseEvent` passes the stationary byte
/// to `ControllerPlayer::HandleActionFromMouse` (the press attacks in
/// place, also on empty ground) and, with the force-move byte, skips the pick of the object under the cursor (the
/// press acts on the ground point). Derefs to the keyboard, so a system's `keys` can be this.
#[derive(bevy::ecs::system::SystemParam)]
pub struct ClickKeys<'w> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    bindings: Res<'w, gd_ui_kit::keys::KeyBindings>,
}

/// The world-click input of casting, in order: `Cursor` records the world point, `Armed` handles an armed slot's click, `Cast` the key / button casts. The UI systems that must see the mouse first order before these.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CastInputStage {
    Cursor,
    Armed,
    Cast,
}

impl ClickKeys<'_> {
    /// Stationary Attack held.
    pub fn stationary(&self) -> bool {
        self.bindings
            .pressed(gd_ui_kit::keys::STATIONARY_ATTACK, &self.keys)
    }

    /// Force Move held.
    pub fn force_move(&self) -> bool {
        self.bindings
            .pressed(gd_ui_kit::keys::FORCE_MOVE, &self.keys)
    }

    /// A left press does not use the object under the cursor (a chest, door, NPC, …): either modifier is held.
    pub fn skips_objects(&self) -> bool {
        self.stationary() || self.force_move()
    }
}

impl std::ops::Deref for ClickKeys<'_> {
    type Target = ButtonInput<KeyCode>;

    fn deref(&self) -> &Self::Target {
        &self.keys
    }
}
