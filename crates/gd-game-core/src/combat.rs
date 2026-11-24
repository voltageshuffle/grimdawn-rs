//! The combat data: actor radii and reach (the combat systems are gd-game's
//! `combat`).
use bevy::prelude::*;
use gd_sim::actor::Profile;
use std::sync::Arc;

/// `gameengine.dbr` `meleeRange` (1.25 m): the gap between two actors' radii
/// a melee attack reaches (`GameEngine::GetMeleeRange`).
pub const MELEE_RANGE: f32 = 1.25;

/// `actorRadius` of a record that has none: Engine.dll `Actor::Load` reads `actorRadius`,
/// `actorHeight` and `scale` with the default 1.0 each (no riftgate record has an `actorRadius`).
pub const ACTOR_RADIUS: f32 = 1.0;

/// The radius of an actor spawned from `rec`: `Actor::GetRadius` (Engine.dll, an `Object` virtual)
/// = the length of the current scale vector (`Actor.scale`) x `actorRadius` (`Actor.actorRadius`); `Actor::Load`
/// copies the record's `scale` into all three components of the base scale (`Actor.baseScale`) and hands it to
/// `SetScale(Vec3)` (`Actor::SetScale`), so it is sqrt(3) x `scale` x `actorRadius`. The crowd agent's radius
/// (`Character::CrowdAgentCreated`), the reach (`Character::GetTargetDistance`: skill
/// range + both radii), `Character::NearTarget` and a fixed actor's use range
/// (`FixedActor::GetUsableRange` and `ControllerPlayerState::DefaultRequestInteractableAction`
/// call the same vtable slot on the chest / door / shrine / riftgate; Game.dll overrides it only for
/// `AreaOfInterest` and `PatrolPoint`) all read it: the one radius of characters and usable objects alike, which
/// their components keep (`Combatant::radius`, `Chest::radius`, `Door::radius`, `Shrine::radius`) for the hover
/// pick and the use range (a riftgate's `Waypoint::radius` is its teleporter `radius`, not this). `default`
/// stands for a missing `actorRadius` ([`ACTOR_RADIUS`] is the engine's); a missing or zero `scale` is 1 (as
/// `gd_game::creature::spawn_creature`). The Colossus growth scales it (`gd_game::colossus`).
/// Not this: the loot drop radius and the nav obstacle of a breakable read the raw `actorRadius` field
/// (`gd_sim::loot_toss::drop_radius`, `gd_game::breakables` module doc).
pub fn get_radius(rec: Option<&gd_formats::arz::Record>, default: f32) -> f32 {
    let actor_radius = rec.and_then(|r| r.f32("actorRadius")).unwrap_or(default);
    let scale = rec
        .and_then(|r| r.f32("scale"))
        .filter(|s| *s > 0.0)
        .unwrap_or(1.0);
    3f32.sqrt() * scale * actor_radius
}

/// The actor's own cylinder radius in metres (`actorRadius` x `scale`) behind a [`get_radius`] value, which is
/// that x sqrt(3) (the scale vector's length). A pick sphere that stands in for the mesh (`grimdawn::hover`)
/// takes this, so the hover and the use range come from the one radius without the pick growing by sqrt(3).
pub fn body_radius(radius: f32) -> f32 {
    radius / 3f32.sqrt()
}

/// A fixed actor's `usableRange`, the term its use range adds to both radii ([`crate::player_orders::usable_range`]):
/// Game.dll, every fixed actor's `Load` (`FixedActor`, `FixedItem`, `FixedDoor`, `FixedLever`,
/// `DynamicDoor`, `StaticShrine`, `StaticTeleporter`, `AscendantAltar`, `BindingInteractable`) reads
/// it with the default 1.0, so a record without one (nearly every chest and door) is used from 1 m beyond the
/// radii, not 0.
pub fn fixed_usable_range(rec: Option<&gd_formats::arz::Record>) -> f32 {
    rec.and_then(|r| r.f32("usableRange")).unwrap_or(1.0)
}

/// The use range of the fixed actor of `rec` for a player of `player_radius`: [`crate::player_orders::usable_range`]
/// with the object's [`get_radius`] and [`fixed_usable_range`], for the objects that keep no radius of their own
/// (riftgates, portals, rifts, the gravestone).
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
    /// The actor's radius, `Actor::GetRadius` ([`get_radius`]): the one radius of the crowd, the reach
    /// (`GetTargetDistance`), `NearTarget` and the other range checks.
    pub radius: f32,
    /// The object-space bounding box (`GraphicsMeshInstance::GetObjectSpaceBoundingBox`, `gd_sim::bounds`): what
    /// the skill target queries test ([`Self::in_radius`], [`Self::around_ray`]) and the cursor pick
    /// (`gd_game::combat::pick_monster`). Set from the `gd_game::character::Character`'s mesh and scale
    /// (`gd_game::combat::body_bounds`) or by the spawner (a breakable's mesh); a point at the origin until then
    /// (no mesh).
    pub bounds: gd_sim::bounds::Aabb,
    pub target: Option<Entity>,
    /// Seconds until the next attack may start.
    pub cooldown: f32,
    /// Milliseconds until the shield can block again (`CombatManager.blockCooldown`,
    /// counted down by `CombatManager::Update`).
    pub block_cooldown_ms: i32,
}

/// Who attacked a character: its `CombatManager.lastAttacker`, which
/// `TakeAttack` sets early for every attack (a miss too) and `DurationDamageManager::ExecuteDamage`
/// before each duration entry that deals damage (life leech included, mana leech not; the
/// killer of a death, a `gd_game::combat::Suicide`'s too), and `GameEngine::RegisterDamage`, which `CombatManager::ApplyDamage`
/// calls with each damage application (hits and DoT ticks) on a character that is not a `Player`:
/// the damage summed per `GetMasterAttacker` (a pet's owner, a leaderless monster itself). The experience share
/// is the main player's part of that sum (`PlayerManagerServer::GetExperienceMultiplier`). A hit
/// registers every amount it applies, overkill included ([`gd_sim::actor::Hit::applied_total`]).
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

/// A dying or dead combatant; `timer` counts down to its next step (the
/// body's removal, or the player's respawn).
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

    /// Whether `GameEngine::GetTargetsInRadius(at, r)` finds this body under `m` (the
    /// sphere meets its world box, `gd_sim::bounds::Aabb::meets_sphere`; 3D).
    pub fn in_radius(&self, m: &bevy::math::Affine3A, at: Vec3, r: f32) -> bool {
        self.body_box(m).meets_sphere(at.to_array(), r)
    }

    /// Whether `GameEngine::GetTargetsAroundRay(ray, len, thick)` finds this body under `m`
    /// (the ray from `a` along `dir` enters its world box grown by `thick` before `len`,
    /// `gd_sim::bounds::Aabb::around_ray`; 3D). `dir` need not be normalised.
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

/// The controller record's `Class` (the RTTI class `Monster::Load`
/// creates; Game.dll classInfo chain). Every class reads
/// `ControllerMonster::Load`'s fields; they differ in their states.
///
/// * `ControllerStationaryMonster`: `CanMove` returns false, and its
///   `RegisterStates` has no Patrol, Roam, Wander,
///   WaitToAttack, RepositionForAttack, NavigateObstacle, DodgeAttack or
///   QuestMove state; Flee, Return, FollowLeader, DefendLeader and Move are a
///   stub whose `OnBegin` goes "Idle" (`SetState` of a missing
///   state only logs "State %s is trying to be set, but not found." and
///   stays). Its Pursue `OnBegin`: a valid enemy that is
///   `CloseEnoughToUseSkill` → "Attack", else "Idle". Idle's roaming
///   (`ControllerMonsterStateIdle::StartedRoaming`) and Attack's
///   `randomRepositionChance` need `CanMove`, so the monster never moves: it
///   turns to and attacks what comes in reach (the record's run speed is
///   not used by its AI).
/// * `ControllerTotem` (parent Stationary): a stationary monster whose `AngerManager` comparator is
///   the distance one (`SetDistanceAngerComparator`: the
///   entry's distance, set by `AngerManager::Update`): it goes
///   for the nearest foe on its anger list, not the angriest.
/// * `ControllerMonsterHidden` (`ControllerMonsterHidden::Create`): `appearDistance`
///   (`ControllerMonsterHidden.appearDistance`, default 5, `Load`) and the monster's states with
///   "Startup" replaced by `ControllerMonsterStateHidden` (`RegisterStates`),
///   see `gd_game::combat::Hidden`.
/// * `ControllerGraeae` and `ControllerMonsterSynergy`: the monster's states
///   plus their own (`gd_game::graeae`, `gd_game::synergy`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ControllerClass {
    #[default]
    Monster,
    Stationary,
    Totem,
    Hidden,
    /// `ControllerGraeae` (`gd_game::graeae`).
    Graeae,
    /// `ControllerMonsterSynergy` (`gd_game::synergy`).
    Synergy,
    /// `ControllerSpirit` (`gd_game::spirit`).
    Spirit,
    /// `ControllerSpiritHost` (`gd_game::spirit`).
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

    /// `ControllerMonster::CanMove` as far as the class decides it (the
    /// stationary classes: never; the port's `movement::Rooted` stands in for
    /// the rest of `ControllerMonster::CanMove`).
    pub fn can_move(self) -> bool {
        !matches!(self, Self::Stationary | Self::Totem)
    }
}
