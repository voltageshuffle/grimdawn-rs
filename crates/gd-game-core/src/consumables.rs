//! The ask-first consumable state: a tonic / writ / bounty right-clicked in
//! the bags ([`UseAsk`]), pending its requirement test and yes / no
//! question. The using systems are `gd_game::consumables`.

/// A tonic / writ / bounty right-clicked in the bags (`Inventory::use_ask`, `gd_game::consumables::asks`, module doc).
#[derive(Clone, Debug, PartialEq)]
pub struct UseAsk {
    pub bag: usize,
    pub x: u32,
    pub y: u32,
    pub record: String,
    pub class: String,
    /// The yes / no question up (its text tag), else not yet tested.
    pub question: Option<&'static str>,
}
