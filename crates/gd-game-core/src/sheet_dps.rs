//! The worn-gear snapshot the character sheet's DPS compare runs on
//! ([`Gear`]); the DPS formulas and the compare itself are
//! `gd_game::sheet_dps` (`gd_sim::dps`).
use gd_sim::equip::WeaponType;

/// Worn gear for `gd_game::sheet_dps::compare_dps`: the items and the weapon types of the two
/// hands.
#[derive(Clone, Debug, Default)]
pub struct Gear {
    pub items: Vec<gd_formats::save::item::Item>,
    pub hands: (Option<WeaponType>, Option<WeaponType>),
}
