//! The combat data: actor radii and reach.
use bevy::prelude::*;
use gd_sim::actor::Profile;
use std::sync::Arc;

/// `gameengine.dbr` `meleeRange`: the gap between two actors' radii a melee attack reaches
/// (`GameEngine::GetMeleeRange`).
pub const MELEE_RANGE: f32 = 1.25;

/// `actorRadius` of a record that has none (`Actor::Load` defaults `actorRadius`, `actorHeight` and `scale`
/// to 1.0).
pub const ACTOR_RADIUS: f32 = 1.0;

/// `Actor::GetRadius`: the scale vector's length x `actorRadius` = sqrt(3) x `scale` x `actorRadius`
/// (`Actor::Load` copies `scale` into all three components). The one radius of characters and usable
/// objects: crowd agent, reach (`Character::GetTargetDistance`), `Character::NearTarget` and a fixed actor's
/// use range (`FixedActor::GetUsableRange`). `default` stands for a missing `actorRadius`; a missing or
/// zero `scale` is 1. Not the loot drop radius or a breakable's nav obstacle, which read raw `actorRadius`.
pub fn get_radius(rec: Option<&gd_formats::arz::Record>, default: f32) -> f32 {
    let actor_radius = rec.and_then(|r| r.f32("actorRadius")).unwrap_or(default);
    let scale = rec
        .and_then(|r| r.f32("scale"))
        .filter(|s| *s > 0.0)
        .unwrap_or(1.0);
    3f32.sqrt() * scale * actor_radius
}

/// The actor's own cylinder radius (`actorRadius` x `scale`) behind a [`get_radius`] value; a pick sphere
/// standing in for the mesh takes this.
pub fn body_radius(radius: f32) -> f32 {
    radius / 3f32.sqrt()
}

/// A fixed actor's `usableRange`, the term its use range adds to both radii: every fixed actor's `Load`
/// defaults it to 1.0.
pub fn fixed_usable_range(rec: Option<&gd_formats::arz::Record>) -> f32 {
    rec.and_then(|r| r.f32("usableRange")).unwrap_or(1.0)
}

/// The use range of the fixed actor of `rec` for objects that keep no radius of their own.
pub fn fixed_use_range(
    rec: Option<&gd_formats::arz::Record>,
    player_radius: f32,
    path_end: bool,
) -> f32 {
    crate::player_orders::usable_range(
        player_radius,
        get_radius(rec, ACTOR_RADIUS),
        fixed_usable_range(rec),
        path_end,
    )
}

/// Combat systems (skill casting runs before them).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CombatSet;

#[derive(Component)]
#[require(Attackers, crate::casting::Shields)]
pub struct Combatant {
    pub profile: Arc<Profile>,
    pub life: f32,
    /// `Actor::GetRadius` ([`get_radius`]).
    pub radius: f32,
    /// The object-space bounding box (`GraphicsMeshInstance::GetObjectSpaceBoundingBox`) the skill target
    /// queries and the cursor pick test; a point at the origin until a mesh sets it.
    pub bounds: gd_sim::bounds::Aabb,
    pub target: Option<Entity>,
    /// Seconds until the next attack may start.
    pub cooldown: f32,
    /// `CombatManager.blockCooldown`: milliseconds until the shield can block again.
    pub block_cooldown_ms: i32,
}

/// `CombatManager.lastAttacker` (set by `TakeAttack` for every attack, a miss too, and by
/// `DurationDamageManager::ExecuteDamage` before each damaging duration entry, mana leech excluded), and
/// `GameEngine::RegisterDamage`: on a non-`Player`, every applied amount (overkill included) summed per
/// `GetMasterAttacker`, for `PlayerManagerServer::GetExperienceMultiplier`.
#[derive(Component, Default, Debug, Clone)]
pub struct Attackers {
    pub last: Option<Entity>,
    by: Vec<(Entity, f32)>,
}

impl Attackers {
    /// An attack by `attacker` (master `master`) that dealt `damage`; `share`: the defender is no player.
    pub fn register(&mut self, attacker: Entity, master: Entity, damage: f32, share: bool) {
        self.last = Some(attacker);
        if !share || damage <= 0.0 {
            return;
        }
        match self.by.iter_mut().find(|b| b.0 == master) {
            Some(b) => b.1 += damage,
            None => self.by.push((master, damage)),
        }
    }

    /// `GetExperienceMultiplier`: `master`'s damage / everyone's (0 without any).
    pub fn share(&self, master: Entity) -> f32 {
        let total: f32 = self.by.iter().map(|b| b.1).sum();
        let own: f32 = self.by.iter().filter(|b| b.0 == master).map(|b| b.1).sum();
        if own <= 0.0 { 0.0 } else { own / total }
    }
}

/// A dying or dead combatant; `timer` counts down to its next step (body removal or player respawn).
#[derive(Component)]
pub struct Dead {
    pub timer: f32,
}

impl Combatant {
    pub fn new(profile: Arc<Profile>, radius: f32) -> Self {
        Self {
            life: profile.sheet.life.max(1.0),
            profile,
            radius,
            bounds: gd_sim::bounds::Aabb::POINT,
            target: None,
            cooldown: 0.0,
            block_cooldown_ms: 0,
        }
    }
    /// [`Self::bounds`] in the world under `m` (`Actor::UpdateBoundingBox`: `Entity.boundingBox`).
    pub fn body_box(&self, m: &bevy::math::Affine3A) -> gd_sim::bounds::Aabb {
        let c = m.matrix3;
        self.bounds.transformed(
            [
                c.x_axis.to_array(),
                c.y_axis.to_array(),
                c.z_axis.to_array(),
            ],
            m.translation.to_array(),
        )
    }

    /// Whether `GameEngine::GetTargetsInRadius(at, r)` finds this body under `m` (3D).
    pub fn in_radius(&self, m: &bevy::math::Affine3A, at: Vec3, r: f32) -> bool {
        self.body_box(m).meets_sphere(at.to_array(), r)
    }

    /// Whether `GameEngine::GetTargetsAroundRay(ray, len, thick)` finds this body under `m` (3D).
    /// `dir` need not be normalised.
    pub fn around_ray(
        &self,
        m: &bevy::math::Affine3A,
        a: Vec3,
        dir: Vec3,
        len: f32,
        thick: f32,
    ) -> bool {
        dir.try_normalize().is_some_and(|d| {
            self.body_box(m)
                .around_ray(a.to_array(), d.to_array(), len, thick)
        })
    }

    pub fn reach(&self, other: &Combatant) -> f32 {
        self.radius + other.radius + MELEE_RANGE
    }
}

/// The controller record's `Class` (the RTTI class `Monster::Load` creates). Every class reads
/// `ControllerMonster::Load`'s fields; they differ in their states.
///
/// * `ControllerStationaryMonster`: `CanMove` is false; `RegisterStates` lacks Patrol, Roam, Wander,
///   WaitToAttack, RepositionForAttack, NavigateObstacle, DodgeAttack and QuestMove; Flee, Return,
///   FollowLeader, DefendLeader and Move are stubs that go "Idle". Pursue: an enemy
///   `CloseEnoughToUseSkill` → "Attack", else "Idle". It never moves; it turns to and attacks what is in reach.
/// * `ControllerTotem` (parent Stationary): its `AngerManager` comparator is distance
///   (`SetDistanceAngerComparator`), so it goes for the nearest foe, not the angriest.
/// * `ControllerMonsterHidden`: `appearDistance` (default 5) and "Startup" replaced by
///   `ControllerMonsterStateHidden`.
/// * `ControllerGraeae` and `ControllerMonsterSynergy`: the monster's states plus their own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ControllerClass {
    #[default]
    Monster,
    Stationary,
    Totem,
    Hidden,
    Graeae,
    Synergy,
    Spirit,
    SpiritHost,
}

impl ControllerClass {
    /// PORT: the other classes run as `ControllerMonster`.
    pub fn of(class: Option<&str>) -> Self {
        match class {
            Some("ControllerStationaryMonster") => Self::Stationary,
            Some("ControllerTotem") => Self::Totem,
            Some("ControllerMonsterHidden") => Self::Hidden,
            Some("ControllerGraeae") => Self::Graeae,
            Some("ControllerMonsterSynergy") => Self::Synergy,
            Some("ControllerSpirit") => Self::Spirit,
            Some("ControllerSpiritHost") => Self::SpiritHost,
            _ => Self::Monster,
        }
    }

    /// `ControllerMonster::CanMove` as far as the class decides it (`movement::Rooted` covers the rest).
    pub fn can_move(self) -> bool {
        !matches!(self, Self::Stationary | Self::Totem)
    }
}
