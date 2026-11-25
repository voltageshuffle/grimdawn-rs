//! The altar window's state ([`AltarMenu`]): the binary's `ascendant.rs` opens it, `gd_ui::altar` draws it.
use bevy::prelude::*;

/// The altar window's state (`ui::altar` draws it, its clicks change it).
#[derive(Resource, Default)]
pub struct AltarMenu {
    pub open: bool,
    /// The altar it was opened at (`AltarWindow.altarId`): the reagent list and the boxes take clicks and the
    /// activate button can be pressed. `None` (opened by the Altar Window key): a view of the boxes as activated
    /// (the binary's `ascendant::altar_key`).
    pub altar: Option<Entity>,
    /// The boxes (the offering in progress, or the activated one in the view).
    pub offering: gd_game_core::ascendant::Offering,
    /// The reagent list's tab (`AltarWindow.reagentTab`): 0 = `reagentTabButton1` (the inclusive formulas),
    /// 1 = `reagentTabButton2` (the exclusive ones).
    pub tab: u8,
    /// Scroll offsets (window px) of the reagent list, the loot bonus box and the monster bonus box.
    pub scroll: [f32; 3],
}

impl AltarMenu {
    /// Opens the window at `altar` (`None`: the view) with the boxes `offering`; the tab stays as last shown
    /// (`AltarWindow.reagentTab` is set only by the tab buttons, `AltarWindow::SetReagentTab`).
    pub fn show(&mut self, altar: Option<Entity>, offering: gd_game_core::ascendant::Offering) {
        *self = Self {
            open: true,
            altar,
            offering,
            tab: self.tab,
            scroll: [0.0; 3],
        };
    }
}
