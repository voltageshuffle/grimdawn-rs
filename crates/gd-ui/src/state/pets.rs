//! The selected pets ([`PetSelection`], in selection order) and the pet-order input set: the binary's
//! `pets.rs` world input writes them, `gd_ui::pets` draws the portraits.
use bevy::prelude::*;

/// The selected pets, in selection order.
#[derive(Resource, Default, Clone, PartialEq)]
pub struct PetSelection(pub Vec<Entity>);

/// The pet orders given by clicking the world.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PetInputSet;

impl PetSelection {
    /// Selects the pet, or deselects it if it was selected (`InGameUI::TogglePetSelection`).
    pub fn toggle(&mut self, pet: Entity) {
        match self.0.iter().position(|e| *e == pet) {
            Some(i) => {
                self.0.remove(i);
            }
            None => self.0.push(pet),
        }
    }
}
