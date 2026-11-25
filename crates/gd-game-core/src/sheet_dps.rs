//! The worn-gear snapshot the character sheet's DPS compare runs on.
use gd_sim::equip::WeaponType;

#[derive(Clone, Debug, Default)]
pub struct Gear {
    pub items: Vec<gd_formats::save::item::Item>,
    pub hands: (Option<WeaponType>, Option<WeaponType>),
}
