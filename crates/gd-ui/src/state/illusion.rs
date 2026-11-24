//! The Illusionist window's state ([`IllusionView`]) and its equipment-box table ([`EQUIP_BOXES`]):
//! the binary's `illusion.rs` input changes it, `gd_ui::trade` draws it.
use bevy::prelude::*;
use gd_formats::save::item::Item;
use gd_game_core::inventory::Inventory;

/// The window's equipment boxes: record field and worn slot index
/// (`gd_game_core::inventory::SLOTS`).
pub const EQUIP_BOXES: [(&str, usize); 9] = [
    ("equipHead", 0),
    ("equipChest", 2),
    ("equipLegs", 3),
    ("equipFeet", 4),
    ("equipHands", 5),
    ("equipShoulders", 9),
    ("equipMedal", 10),
    ("equipHandRight", 12),
    ("equipHandLeft", 13),
];

/// Looks per page (`displayBoxCount`).
pub const PAGE: usize = 18;

/// The open window's state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IllusionView {
    /// The selected worn slot.
    pub slot: Option<usize>,
    /// The looks offered for it (`gd_game::illusion::choices`), rebuilt when
    /// the slot, the item or the unlocked list changes.
    pub list: Vec<String>,
    /// What `list` was built for (slot item, unlocked count).
    pub built: Option<(Item, usize)>,
    pub page: usize,
    /// Pending picks: (worn slot, look record).
    pub picks: Vec<(usize, String)>,
    /// The close button was pressed.
    pub close: bool,
}

impl IllusionView {
    /// The pick for the selected slot, else the item's current look.
    pub fn shown_look(&self, inv: &Inventory) -> Option<String> {
        let s = self.slot?;
        if let Some((_, l)) = self.picks.iter().find(|(p, _)| *p == s) {
            return Some(l.clone());
        }
        inv.equipped[s]
            .as_ref()
            .map(|i| gd_sim::items::illusion::look_record(i).to_owned())
    }

    pub fn pages(&self) -> usize {
        self.list.len().div_ceil(PAGE).max(1)
    }
}
