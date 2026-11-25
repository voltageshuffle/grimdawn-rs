//! Faction names and indices.

/// Save index of a faction by the engine's type name, case-insensitive (`FactionPack::GetFactionTag`; type 6 is
/// `User0`, not the `factionDrifters` key's name).
pub fn index_of_name(name: &str) -> Option<usize> {
    gd_rules::faction::type_of(name)
}
