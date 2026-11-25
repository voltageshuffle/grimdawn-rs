//! Where the summon skills put their pets.
//!
//! ## `Skill_SpawnPet` ([`spawn_pet_set`])
//!
//! `Skill_SpawnPet::ActivateNow` (also `Skill_SpawnMiniPet`, `Skill_SpawnQuestPet`,
//! `Skill_SpawnPetMonster` and `Skill_AktaiosMirage`):
//! - The count ([`gd_game_core::pets::placement::spawn_pet_count`]): `min(GetPetLimit(level) - live,
//!   petBurstSpawn)` in unsigned 32-bit arithmetic (a limit of 0, or more live than the limit, wraps to
//!   the full burst), live = the owner's pen entries of this skill not dead; at least `minBurst` (1;
//!   0 for `Skill_SpawnPetMonster`). 0: nothing happens.
//! - The radii ([`spawn_pet_radii`]): the caster's `GetExtents` and `GetPetExtents` (the pet object's
//!   `GetExtents` + 2, none: 2), each times `GetExtentsMultiplier` (1, the mirage's 2); with
//!   `UseLargerRadius` (the mirage's) both become the larger.
//! - `NavManager::FillPointSet(caster, 16, [caster, pet x count], clear, !random)`
//!   ([`gd_sim::pattern::fill_point_set`]): the caster's own spot first, then one per summon; a radius
//!   that does not fit empties the set and nothing spawns. No line of sight, no cast point.
//! - `GetNewCasterCoords` drops the first point (the mirage's own picks a spot,
//!   `monster_skills::MIRAGE`); every point left is put on the floor and gets a pet, except a quest
//!   pet's (`IsQuestPetSkill`), which goes to the cast point instead, once per point.
//!
//! ## `Skill_MonsterGenerator` ([`generator_spots`])
//!
//! `Skill_MonsterGenerator::Update`: count = `min(petBurstSpawn, max(0, limit - live))` (signed: no
//! minimum); the caster blocks its `Actor::GetRadius` at its coords (`BlockPoint`), then
//! `FillPointSet(caster, 16, [GetPetExtents x count], true, false)`; each pet faces a random yaw.
//!
//! ## `Skill_TargetedSpawnPet` ([`targeted_spots`])
//!
//! `Skill_TargetedSpawnPet::ActivateNow`:
//! - The centre: the cast point; with a target the target's coords; with `selfTarget` the caster's
//!   and no target. Raised by half the caster's height. Unless the target passes `IsTargetInLOS`, the
//!   centre `Skill::IsPointInLOS` and it lies on the path mesh, it is pulled back along the caster →
//!   centre ray to the first world intersection `t` less `GetPetExtents` (the caster itself when
//!   `t <= GetPetExtents`); off every region: nothing spawns.
//! - The spacing: `petPadding` when > 0, else `GetPetExtents` ([`pet_extents`]; the pet object comes
//!   from `Skill::PreLoadPet`, for every owner but a player copy). One per summon: `petBurstSpawn`,
//!   0 counting as 1.
//! - With a target and a `targetingMode` other than `Point`, or more than one summon: the target
//!   blocks the centre for its `GetExtents` (`NavManager::BlockPoint`), then `FillPointSet(centre, 16,
//!   spacings, clear, !random)` ([`gd_sim::pattern::fill_point_set_in`]); any that does not fit
//!   empties the set. Otherwise the one spot is the target's coords, else the centre.
//! - Every spot is put on the floor (`TranslateToFloor`).
//!
//! ## The hit spawners ([`hit_spots`])
//!
//! They summon where their attack lands; one spot per `petBurstSpawn` (0 counting as 1), each
//! `GetPetExtents` apart, `FillPointSet(centre, 16, ..., clear, !random)`, on the floor:
//! - `Skill_AttackSpellChaosSpawnPet` (Reap Spirit) `TargetResult`: on any hit, around the last
//!   validated target, each target blocking its `GetExtents`.
//! - `Skill_AttackProjectileSpawnPet` `TargetInformation` (the projectile's end): with a target hit and
//!   a `targetingMode` other than `Point` the target blocks its `GetExtents` and the spots pack around
//!   the impact; else one spot: the target's coords, without one the impact's.
//!
//! PORT: only the spawn event's target blocks (a Reap Spirit beam that hit several foes leaves the
//! others' footprints free). The nav probe stands in for the floor. Targeted: no line-of-sight test
//! and no world ray: a centre off the nav is pulled back to where the straight line from the caster
//! leaves walkable ground ([`crate::nav::straight_move_point`]), less the spacing.
use crate::nav::Nav;
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_formats::vfs::Database;
use gd_game_core::pets::placement::{MAX_DIST, pet_extents};

/// Spawn-set radii (caster, each pet) for a skill of `class` cast by an owner of `GetExtents` `caster`.
pub fn spawn_pet_radii(class: &str, caster: f32, pet: Option<&Record>) -> (f32, f32) {
    let mirage = class == gd_game_core::monster_skills::MIRAGE;
    let mult = if mirage { 2.0 } else { 1.0 };
    let (c, p) = (caster * mult, pet_extents(pet) * mult);
    if mirage {
        let m = c.max(p);
        (m, m)
    } else {
        (c, p)
    }
}

/// The caster's spot first, then `count` pet spots; empty when one does not fit.
pub fn spawn_pet_set(nav: &Nav, centre: Vec3, radii: (f32, f32), count: u32) -> Vec<Vec3> {
    let mut r = vec![radii.1; count as usize + 1];
    r[0] = radii.0;
    let cy = centre.y;
    gd_sim::pattern::fill_point_set(
        (centre.x, centre.z),
        &r,
        MAX_DIST,
        gd_game_core::rng::f32,
        |x, z| nav.0.ground([x, cy + 2.0, z]),
    )
    .into_iter()
    .map(Vec3::from)
    .collect()
}

/// `count` of `GetPetExtents` `pet` around the caster at `centre`, clear of its `radius`.
pub fn generator_spots(nav: &Nav, centre: Vec3, radius: f32, pet: f32, count: u32) -> Vec<Vec3> {
    let mut block = gd_sim::pattern::NavBlock::default();
    block.block_point(centre.x, centre.z, radius);
    let cy = centre.y;
    gd_sim::pattern::fill_point_set_in(
        &mut block,
        (centre.x, centre.z),
        &vec![pet; count as usize],
        MAX_DIST,
        gd_game_core::rng::f32,
        |x, z| nav.0.ground([x, cy + 2.0, z]),
    )
    .into_iter()
    .map(Vec3::from)
    .collect()
}

pub struct Aim {
    pub caster: Vec3,
    pub point: Vec3,
    /// Position and `GetExtents`.
    pub target: Option<(Vec3, f32)>,
}

/// `burst` is the modifier-summed `petBurstSpawn` (0 counting as 1); empty: nothing spawns.
pub fn targeted_spots(
    db: &Database,
    nav: &Nav,
    skill: &Record,
    burst: f32,
    pet: Option<&Record>,
    aim: &Aim,
) -> Vec<Vec3> {
    let target = aim.target.filter(|_| !skill.bool("selfTarget"));
    let mut centre = match (skill.bool("selfTarget"), target) {
        (true, _) => aim.caster,
        (false, Some((t, _))) => t,
        (false, None) => aim.point,
    };
    let padding = skill.f32("petPadding").unwrap_or(0.0);
    let spacing = if padding > 0.0 {
        padding
    } else {
        pet_extents(pet)
    };
    if nav.0.ground([centre.x, centre.y + 2.0, centre.z]).is_none() {
        let edge = crate::nav::straight_move_point(&nav.0, aim.caster, centre);
        let d = (edge - aim.caster).with_y(0.0);
        let len = d.length();
        centre = if len <= spacing {
            aim.caster
        } else {
            aim.caster + d * ((len - spacing) / len)
        };
    }
    let count = burst.max(1.0) as usize;
    let profile = gd_sim::skills::profile_record(db, &skill.name).unwrap_or_else(|| skill.clone());
    let point_mode =
        gd_sim::skills::targeting_mode(&profile) == gd_sim::skills::TargetingMode::Point;
    let floor = |x: f32, z: f32, y: f32| nav.0.ground([x, y + 2.0, z]);
    if (target.is_some() && !point_mode) || count > 1 {
        let mut block = gd_sim::pattern::NavBlock::default();
        if let Some((_, r)) = target {
            block.block_point(centre.x, centre.z, r);
        }
        let cy = centre.y;
        return gd_sim::pattern::fill_point_set_in(
            &mut block,
            (centre.x, centre.z),
            &vec![spacing; count],
            MAX_DIST,
            gd_game_core::rng::f32,
            |x, z| floor(x, z, cy),
        )
        .into_iter()
        .map(Vec3::from)
        .collect();
    }
    let at = target.map_or(centre, |t| t.0);
    vec![at.with_y(floor(at.x, at.z, at.y).unwrap_or(at.y))]
}

/// The classes that summon where their attack lands.
pub fn spawns_on_hit(class: &str) -> bool {
    matches!(
        class,
        "Skill_AttackSpellChaosSpawnPet" | "Skill_AttackProjectileSpawnPet"
    )
}

/// `at` the impact (a chaos beam's target position), `target` the foe hit with its `GetExtents`; empty:
/// nothing spawns.
pub fn hit_spots(
    db: &Database,
    nav: &Nav,
    skill: &Record,
    burst: f32,
    pet: Option<&Record>,
    at: Vec3,
    target: Option<(Vec3, f32)>,
) -> Vec<Vec3> {
    let count = burst.max(1.0) as usize;
    let floor = |p: Vec3| p.with_y(nav.0.ground([p.x, p.y + 2.0, p.z]).unwrap_or(p.y));
    let chaos = skill.class == "Skill_AttackSpellChaosSpawnPet";
    let centre = match (chaos, target) {
        (true, Some((t, _))) => t,
        _ => at,
    };
    if !chaos {
        let profile =
            gd_sim::skills::profile_record(db, &skill.name).unwrap_or_else(|| skill.clone());
        let point_mode =
            gd_sim::skills::targeting_mode(&profile) == gd_sim::skills::TargetingMode::Point;
        if point_mode || target.is_none() {
            return vec![floor(target.map_or(at, |t| t.0))];
        }
    }
    let mut block = gd_sim::pattern::NavBlock::default();
    if let Some((t, r)) = target {
        let b = if chaos { t } else { at };
        block.block_point(b.x, b.z, r);
    }
    let cy = centre.y;
    gd_sim::pattern::fill_point_set_in(
        &mut block,
        (centre.x, centre.z),
        &vec![pet_extents(pet); count],
        MAX_DIST,
        gd_game_core::rng::f32,
        |x, z| nav.0.ground([x, cy + 2.0, z]),
    )
    .into_iter()
    .map(Vec3::from)
    .collect()
}
