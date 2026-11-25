//! Monster skill data: the `specialAttackN` slots ([`Special`]) and the
//! mirage skill-class name ([`MIRAGE`]).
use crate::casting::Kind;

#[derive(Clone, Debug)]
pub struct Special {
    /// 0 = `specialAttack`, 1 = `specialAttack2`, … (`ControllerMonster.specialAttacks[n]`).
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
    /// The slot as `Monster::InitSkillsInController` fills it: no delay left, the whole timeout.
    pub fn rearmed(&self) -> Self {
        Self {
            delay_left: 0.0,
            timeout_left: self.timeout,
            ..self.clone()
        }
    }
}

/// `Skill_AktaiosMirage` (Lagothak's, the Gazer hero's and the Dreeg mindreaper's copies), a
/// `Skill_SpawnPet` (Game.dll) with `UseLargerRadius`: spawn-point radius 2 x (copy `GetExtents` + 2).
/// `Skill_SpawnPet::ActivateNow` fills the point set (caster's slot first,
/// `pets::placement::spawn_pet_set`); `GetNewCasterCoords` draws `IGenerate(0, n)` (inclusive):
/// below n the caster takes that point, at n it stays and the set is kept whole. A summon spawns on
/// every point left, then `SetCasterCoords` teleports the caster (animation type 19 `Spawn`, no
/// controller state). PORT: no stock record has a `casterHintEffect`. `PostPetSpawned` sets
/// `Monster.mirageCaster` (reader not traced). `SkillSpawnObject` is never sent for this class: not ported.
pub const MIRAGE: &str = "Skill_AktaiosMirage";
