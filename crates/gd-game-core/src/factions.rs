//! Faction names and indices (the reputation values and the kill rules are
//! gd-game's `factions`).

/// Save index of a faction by the name scripts and records use (`Survivors`, `Outlaws`, `USER4`, `User0`, ...): the
/// engine's type names, case-insensitive ([`gd_rules::faction::type_of`], `FactionPack::GetFactionTag`;
/// type 6 is `User0`, not the `factionDrifters` key's name).
pub fn index_of_name(name: &str) -> Option<usize> {
    gd_rules::faction::type_of(name)
}
