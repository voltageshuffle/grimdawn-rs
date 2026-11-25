//! The summon skills' pet sizing and counts; nav-dependent spot geometry is in `gd_game::pets::placement`.
use gd_formats::arz::Record;

/// `NavManager::fillPointSetMaxDistance`.
pub const MAX_DIST: f32 = 16.0;

/// `Character::GetExtents` from a record's `pathingSize`; no record is Small.
pub fn extents(rec: Option<&Record>) -> f32 {
    match rec.and_then(|r| r.str("pathingSize")) {
        Some("Medium") => 1.0,
        Some("Large") => 1.75,
        _ => 0.5,
    }
}

/// `Skill::GetPetExtents`: the pet's `GetExtents` + 2, 2 without a record.
pub fn pet_extents(pet: Option<&Record>) -> f32 {
    pet.map_or(2.0, |r| extents(Some(r)) + 2.0)
}

/// `Skill_SpawnPet::ActivateNow`'s summon count; `live` is the owner's living pets of the skill.
pub fn spawn_pet_count(class: &str, limit: u32, live: u32, burst: u32) -> u32 {
    let min = u32::from(class != "Skill_SpawnPetMonster");
    limit.wrapping_sub(live).min(burst).max(min)
}
