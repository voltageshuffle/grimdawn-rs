//! The summon skills' pet sizing and counts (pure record reads); the
//! nav-dependent spot geometry stays in `gd_game::pets::placement`.
use gd_formats::arz::Record;

/// `FillPointSet`'s max distance (the float constant `NavManager::fillPointSetMaxDistance`).
pub const MAX_DIST: f32 = 16.0;

/// `Character::GetExtents` from a character record's `pathingSize`; a
/// missing record (the port's player has none here) is Small.
pub fn extents(rec: Option<&Record>) -> f32 {
    match rec.and_then(|r| r.str("pathingSize")) {
        Some("Medium") => 1.0,
        Some("Large") => 1.75,
        _ => 0.5,
    }
}

/// `Skill::GetPetExtents`: the pet object's `GetExtents` + 2,
/// 2 without one (no record).
pub fn pet_extents(pet: Option<&Record>) -> f32 {
    pet.map_or(2.0, |r| extents(Some(r)) + 2.0)
}

/// `Skill_SpawnPet::ActivateNow`'s summon count (module docs): `limit` and
/// `burst` at the cast level, `live` the owner's living pets of the skill.
pub fn spawn_pet_count(class: &str, limit: u32, live: u32, burst: u32) -> u32 {
    let min = u32::from(class != "Skill_SpawnPetMonster");
    limit.wrapping_sub(live).min(burst).max(min)
}
