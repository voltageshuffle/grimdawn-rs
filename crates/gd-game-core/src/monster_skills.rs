//! Monster skill data: the `specialAttackN` slots ([`Special`]) and the
//! mirage skill-class name ([`MIRAGE`]). The skill-choosing, buffing and
//! summoning systems are gd-game's `monster_skills` (its module docs carry
//! the full engine trace).
use crate::casting::Kind;

/// One `specialAttackN` slot.
#[derive(Clone, Debug)]
pub struct Special {
    /// The controller slot (0 = `specialAttack`, 1 = `specialAttack2`, …;
    /// `ControllerMonster.specialAttacks[n]`).
    pub slot: usize,
    pub skill: String,
    pub level_index: usize,
    pub kind: Kind,
    pub chance: f32,
    pub delay: f32,
    /// 0 any, 1 short, 2 medium, 3 long.
    pub band: usize,
    /// `Skill::GetRange` (`distanceProfile`).
    pub range: f32,
    /// `specialAttackNTimeout` (s).
    pub timeout: f32,
    pub delay_left: f32,
    pub timeout_left: f32,
}

impl Special {
    /// The slot as `Monster::InitSkillsInController` fills it: no delay
    /// left, the whole timeout (`SpecialAttackSlot.delay` = 0, `SpecialAttackSlot.timeout` = timeout).
    pub fn rearmed(&self) -> Self {
        Self {
            delay_left: 0.0,
            timeout_left: self.timeout,
            ..self.clone()
        }
    }
}

/// `Skill_AktaiosMirage` (Lagothak's, the Gazer hero's and the Dreeg
/// mindreaper's copies), a `Skill_SpawnPet` (Game.dll):
/// `Load` adds `casterHintEffect`; `GetExtentsMultiplier`
/// is 2 and "use the larger radius" (`Skill_SpawnPet::UseLargerRadius`) true, so
/// every point of the spawn set has radius max(2 x owner `GetExtents`, 2 x
/// (copy `GetExtents` + 2)) = 2 x (copy + 2): the skill's pet object exists
/// for a monster owner (`Skill::PreLoadPet`, which `SkillLevelChange` calls
/// for every owner but a player copy; `pets::placement`): Lagothak's and the
/// mindreaper's copies (Small) 5, the Gazer hero's (Medium) 6.
/// `Skill_SpawnPet::ActivateNow` fills the point set (the
/// caster's slot first, then one per summon,
/// `pets::placement::spawn_pet_set`), asks `GetNewCasterCoords`
/// where the caster goes — with more than one point a draw
/// `IGenerate(0, n)` (inclusive): below n the caster takes that point
/// (removed from the set), at n it stays and the set is kept whole (base
/// class: the first point is dropped, the caster stays) — spawns a summon
/// on every point left, and with any spawned `SetCasterCoords`
/// teleports the caster there (`Character::Teleport`, then animation type 19
/// (`Spawn`, the Startup / Hidden states' appear animation) started
/// directly on its animation set, no controller state and no wait for its
/// end; nothing plays when the set has no type 19; the `casterHintEffect`
/// there, PORT: no stock record has one). `PostPetSpawned` records the
/// caster on each copy (`Monster.mirageCaster`; its reader not traced).
/// `SkillSpawnObject` (foes within 50 m forget their anger and
/// go idle, players lose their target) is only reached through
/// `SkillServices::SendSpawnObject`, which the spawn path does not send for
/// this class: by design never triggered in stock, not ported.
pub const MIRAGE: &str = "Skill_AktaiosMirage";
