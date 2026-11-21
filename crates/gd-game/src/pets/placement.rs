//! Where the summon skills put their pets.
//!
//! ## `Skill_SpawnPet` ([`spawn_pet_set`])
//!
//! Game.dll `Skill_SpawnPet::ActivateNow` @0x180508bb0 (also
//! `Skill_SpawnMiniPet`, `Skill_SpawnQuestPet`, `Skill_SpawnPetMonster` and
//! `Skill_AktaiosMirage`, which do not override it):
//! - The count ([`spawn_pet_count`]): `min(GetPetLimit(level) - live,
//!   petBurstSpawn)` in unsigned 32-bit arithmetic (a limit of 0, or more
//!   live than the limit, wraps to the full burst), live = the owner's pen
//!   entries of this skill not dead; at least +1576 (1, set by the
//!   `Skill_SpawnPet` constructor @0x180508a10; 0 by `Skill_SpawnPetMonster`'s
//!   @0x18050a35f). 0: nothing happens.
//! - The radii ([`spawn_pet_radii`]): the caster's `Character::GetExtents`
//!   (vtable +448) and `GetPetExtents` (the +816 pet object's `GetExtents` +
//!   2, none: 2), each times `GetExtentsMultiplier` (+1872: 1, the mirage's
//!   2); with "use the larger radius" (+1880: false, the mirage's true) both
//!   become the larger.
//! - `NavManager::FillPointSet(caster coords, 16, [caster, pet x count],
//!   clear = true, random = false)` ([`gd_sim::pattern::fill_point_set`]): the
//!   caster's own spot first, then one per summon; a radius that does not fit
//!   empties the set and nothing spawns. No line of sight, no cast point: the
//!   pets always come around the caster.
//! - `GetNewCasterCoords` (+1856, base @0x1805099a0) drops the first point
//!   (the caster stays; the mirage's own picks a spot,
//!   `monster_skills::MIRAGE`); every point left is put on the floor and gets
//!   a pet (`SpawnPet`, +1848) at it, except a quest pet's (+1840
//!   `Skill_SpawnQuestPet` true, else false), which goes to the cast point
//!   instead, once per point.
//!
//! ## `Skill_MonsterGenerator` ([`generator_spots`])
//!
//! `Skill_MonsterGenerator::Update`: count = `min(petBurstSpawn,
//! max(0, limit - live))` (signed: no minimum); the caster blocks its
//! `Actor::GetRadius` (Engine.dll @0x18002b410: the scale vector's
//! length x `actorRadius`) at its coords (`BlockPoint`), then `FillPointSet(caster,
//! 16, [GetPetExtents x count], true, false)`; each pet faces a random yaw
//! (`rand() * 2 pi / 32767`, the port's RNG); the caster's radius is its
//! `Combatant::radius`.
//!
//! ## `Skill_TargetedSpawnPet` ([`targeted_spots`])
//!
//! Game.dll `Skill_TargetedSpawnPet::ActivateNow` @0x18050acf0:
//! - The centre: the cast point; with a target (`a4`) the target's coords;
//!   with `selfTarget` (+1558) the caster's and no target. It is raised by
//!   half the caster's height. Unless the target passes `IsTargetInLOS` (vtable
//!   +1240), the centre `IsTargetInLOS` (+1232) and it lies on the path mesh,
//!   it is pulled back along the caster → centre ray: at the first world
//!   intersection `t` (`World::GetIntersection`), the caster itself when
//!   `t <= GetPetExtents`, else `t - GetPetExtents` from the caster (no hit:
//!   the same against the visible entities the ray meets); off every region:
//!   nothing spawns.
//! - The spacing: `petPadding` (`Load` @0x18050aa20, +1552) when > 0, else
//!   `Skill::GetPetExtents` @0x18048e490 = the pet object's
//!   `Character::GetExtents` @0x18005dd50 + 2 ([`pet_extents`]: `pathingSize`
//!   Small 0.5, Medium 1.0, Large 1.75, `Character::Load`; the object is made
//!   from the level's spawn object by `Skill::PreLoadPet` @0x18048db10, which
//!   `SkillLevelChange` @0x180481c30 calls for every owner but a player copy
//!   (`Player::IsCopy`), monsters included; none: 2). One per
//!   summon: `petBurstSpawn` (`SkillProfile_Modifiers` +124, `sub_18054A660`),
//!   0 counting as 1.
//! - With a target and a `targetingMode` other than `Point` (`SkillProfile`
//!   +0xE80), or more than one summon: the target (`ResolveValidateTarget`)
//!   blocks the centre for its `GetExtents` (`NavManager::BlockPoint`), then
//!   `NavManager::FillPointSet(centre, 16, spacings, clear = true, random =
//!   false)` packs the spots around it ([`gd_sim::pattern::fill_point_set_in`]);
//!   any that does not fit empties the set and nothing spawns. So a summon
//!   cast on a foe lands on the first ring clear of the foe's footprint.
//!   Otherwise the one spot is the target's coords, else the centre.
//! - Every spot is put on the floor (`TranslateToFloor`) and the pets are
//!   spawned there together.
//!
//! ## The hit spawners ([`hit_spots`])
//!
//! They summon where their attack lands, not at the cast; one spot per
//! `petBurstSpawn` (0 counting as 1), each
//! `GetPetExtents` apart, `FillPointSet(centre, 16, ..., clear, !random)`,
//! every spot put on the floor:
//! - `Skill_AttackSpellChaosSpawnPet` (Reap Spirit) `TargetResult`
//!   @0x1804caf60: on any hit, around the target (the last validated one of
//!   the list), each target blocking its `GetExtents` at its coords.
//! - `Skill_AttackProjectileSpawnPet` `TargetInformation` @0x1804c3650 (the
//!   projectile's end): with a target hit and a `targetingMode` other than
//!   `Point` the target blocks its `GetExtents` at the impact and the spots
//!   pack around the impact; else one spot: the target's coords, without one
//!   the impact's.
//!
//! PORT: only the spawn event's target blocks (a Reap Spirit beam that hit
//! several foes leaves the others' footprints free).
//!
//! PORT (all classes): the nav probe stands in for the floor; the engine
//! RNG of `RequestAndBlockNearPoint`'s start angle is the port's.
//!
//! PORT (targeted): no line-of-sight test and no world ray: a centre off the nav is
//! pulled back to where the straight line from the caster leaves walkable
//! ground ([`crate::nav::straight_move_point`]), less the spacing.
use crate::nav::Nav;
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_formats::vfs::Database;

/// `FillPointSet`'s max distance (flt @0x1807a80f4).
const MAX_DIST: f32 = 16.0;

/// `Character::GetExtents` from a character record's `pathingSize`; a
/// missing record (the port's player has none here) is Small.
pub fn extents(rec: Option<&Record>) -> f32 {
    match rec.and_then(|r| r.str("pathingSize")) {
        Some("Medium") => 1.0,
        Some("Large") => 1.75,
        _ => 0.5,
    }
}

/// `Skill::GetPetExtents` @0x18048e490: the pet object's `GetExtents` + 2,
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

/// `Skill_SpawnPet::ActivateNow`'s spawn-set radii (caster, each pet) for a
/// skill of `class` cast by an owner of `GetExtents` `caster` (module docs).
pub fn spawn_pet_radii(class: &str, caster: f32, pet: Option<&Record>) -> (f32, f32) {
    let mirage = class == crate::monster_skills::MIRAGE;
    let mult = if mirage { 2.0 } else { 1.0 };
    let (c, p) = (caster * mult, pet_extents(pet) * mult);
    if mirage {
        let m = c.max(p);
        (m, m)
    } else {
        (c, p)
    }
}

/// `Skill_SpawnPet::ActivateNow`'s point set around the caster at `centre`:
/// the caster's spot first, then `count` pet spots; empty when one does not
/// fit (module docs).
pub fn spawn_pet_set(nav: &Nav, centre: Vec3, radii: (f32, f32), count: u32) -> Vec<Vec3> {
    let mut r = vec![radii.1; count as usize + 1];
    r[0] = radii.0;
    let cy = centre.y;
    gd_sim::pattern::fill_point_set(
        (centre.x, centre.z),
        &r,
        MAX_DIST,
        crate::rng::f32,
        |x, z| nav.0.ground([x, cy + 2.0, z]),
    )
    .into_iter()
    .map(Vec3::from)
    .collect()
}

/// `Skill_MonsterGenerator::Update`'s spots: `count` of `GetPetExtents`
/// `pet` around the caster at `centre`, clear of its `radius` (module docs).
pub fn generator_spots(nav: &Nav, centre: Vec3, radius: f32, pet: f32, count: u32) -> Vec<Vec3> {
    let mut block = gd_sim::pattern::NavBlock::default();
    block.block_point(centre.x, centre.z, radius);
    let cy = centre.y;
    gd_sim::pattern::fill_point_set_in(
        &mut block,
        (centre.x, centre.z),
        &vec![pet; count as usize],
        MAX_DIST,
        crate::rng::f32,
        |x, z| nav.0.ground([x, cy + 2.0, z]),
    )
    .into_iter()
    .map(Vec3::from)
    .collect()
}

/// The aim of one summon cast.
pub struct Aim {
    pub caster: Vec3,
    pub point: Vec3,
    /// The target's position and `GetExtents`.
    pub target: Option<(Vec3, f32)>,
}

/// The spots of a `Skill_TargetedSpawnPet` cast of `skill` spawning `burst`
/// (its modifier-summed `petBurstSpawn`, 0 counting as 1) `pet`s (module docs);
/// empty: nothing spawns.
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
            crate::rng::f32,
            |x, z| floor(x, z, cy),
        )
        .into_iter()
        .map(Vec3::from)
        .collect();
    }
    let at = target.map_or(centre, |t| t.0);
    vec![at.with_y(floor(at.x, at.z, at.y).unwrap_or(at.y))]
}

/// The classes that summon where their attack lands ([`hit_spots`]).
pub fn spawns_on_hit(class: &str) -> bool {
    matches!(
        class,
        "Skill_AttackSpellChaosSpawnPet" | "Skill_AttackProjectileSpawnPet"
    )
}

/// The spots of a hit spawner's summons (module docs): `at` the impact (a
/// chaos beam's target position), `target` the foe hit with its
/// `GetExtents`; empty: nothing spawns.
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
        crate::rng::f32,
        |x, z| nav.0.ground([x, cy + 2.0, z]),
    )
    .into_iter()
    .map(Vec3::from)
    .collect()
}
