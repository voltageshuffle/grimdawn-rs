use super::*;

/// The record's `hiddenFromCombat` (`Character::Load` +0x24B8):
/// `Character::IsHiding` @0x18005bba0 (vtable +0x468) is true for it, and
/// `GameEngine::FilterInvalidTargets` @0x1802eca00 (every skill target query:
/// `GetTargetsInRadius`, `…AlongRay`, `…AroundRay`, `…InCone`,
/// `…InFrustum`, `GetAllTargetsInRadius`) drops hiding characters unless
/// the query asks for them, so no skill attack reaches it. Stock: the
/// totems, runes, traps, mini pets and other pets that cause no anger
/// (every such pet record also has `causesAnger` 0 and `angerMultiplier` 0,
/// so monsters never pick them), quest / flavour NPCs. Ported at query time:
/// [`Side::hits`] (and [`NotHiding`] for the player's channels) leaves it out of
/// every skill target query, projectiles' and orbiters' paths included; the
/// attack itself does not test it.
#[derive(Component)]
pub struct HiddenFromCombat;

/// The record's `invincible` (`Character::Load` sets +8048, the live flag of
/// `Character::IsInvincible` @0x18006e300, and +8049, `IsInvincibleInDbr`).
/// `Character::SubtractLife` @0x180068ab0 loses no life while it is set
/// (unless forced), `DurationDamageManager::AddDamage` / `AddFixedDamage`
/// add no damage over time, `Character::DebufTarget` applies nothing
/// (`skill_buffs::land`) and `Monster::IsTargetable` is false (the click pick queries in `grimdawn` skip it). PORT: hits
/// still resolve (block, hit reactions); only the life loss is undone and
/// over-time damage skipped. `SetInvincible` is this component's insert / remove (`spirit.rs`
/// uses it); Lua `ToggleInvincible` (no stock use) stays a stub.
#[derive(Component)]
pub struct Invincible;

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
    /// ([`pick_monster`]). Set from the [`Character`]'s mesh and scale ([`body_bounds`]) or by the spawner (a
    /// breakable's mesh); a point at the origin until then (no mesh).
    pub bounds: gd_sim::bounds::Aabb,
    pub target: Option<Entity>,
    /// Seconds until the next attack may start.
    pub cooldown: f32,
    /// Milliseconds until the shield can block again (`CombatManager` +720,
    /// counted down by `CombatManager::Update` @0x18011c2a0).
    pub block_cooldown_ms: i32,
}

/// Who attacked a character: its `CombatManager` +364 attacker, which
/// `TakeAttack` @0x18011d950 sets early for every attack (a miss too) and `DurationDamageManager::ExecuteDamage`
/// @0x1802796b0 before each duration entry that deals damage (life leech included, mana leech not; the
/// killer of a death, a [`Suicide`]'s too), and `GameEngine::RegisterDamage` @0x1802c5bc0, which `CombatManager::ApplyDamage`
/// @0x18011d340 calls with each damage application (hits and DoT ticks) on a character that is not a `Player`:
/// the damage summed per `GetMasterAttacker` (a pet's owner, a leaderless monster itself). The experience share
/// is the main player's part of that sum (`PlayerManagerServer::GetExperienceMultiplier` @0x1803f9580). A hit
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

/// A monster died (experience: [`crate::progress`]; loot and gold: the
/// game's `loot.rs`).
#[derive(Message, Clone, Debug)]
pub struct MonsterKilled {
    pub record: String,
    pub level: i32,
    pub position: Vec3,
    /// The dead monster (its `Worn` gear drops).
    pub entity: Entity,
    /// Who dealt the killing blow and with which skill (None: a plain
    /// attack), when known.
    pub killer: Option<(Entity, Option<String>)>,
}

/// Whose kill a [`MonsterKilled`] is: the main player's when the killer is the player or one of his pets
/// (a DoT's kill is its applier's, `crate::afflictions` docs; a reflected / retaliated hit's the reflecting
/// defender's). `Player::OnCreatureDeath` @0x1803e17a0 (`IncrementKills`: the
/// Crucible score and the kill statistics) takes the killer itself or a `Monster` whose `GetLeader` is the main
/// player; `GameEngine::OnCreatureDeath` @0x1802eff40 pays experience to `GetMasterAttacker` @0x1802c5b50 (a
/// monster's leader, a leaderless one itself) only when it is the local player (`HandleBonus` @0x1803f97f0), and
/// hands the player's reputation (`HandleFaction`) and the Dread counter only a killer whose master is a `Player`
/// (a monster's kill adjusts its own faction pack, not the player's). So nothing for a death without a killer
/// (`Game.KillAllMonsters`; a [`super::Suicide`]'s is its [`Attackers::last`]), a killer that no longer exists (an expired pet's DoT: the id no longer
/// resolves), a leaderless monster (a turret, a guard, an environmental trap: stock `trap_*` creatures are plain
/// `Monster`s) or a hostile monster killing an ally.
#[derive(bevy::ecs::system::SystemParam)]
pub struct KillCredit<'w, 's> {
    players: Query<'w, 's, Entity, With<Player>>,
    pets: Query<'w, 's, &'static crate::pets::Pet>,
}

impl KillCredit<'_, '_> {
    /// The killer is the player or his pet.
    pub fn by_player(&self, killer: Option<Entity>) -> bool {
        let Some(k) = killer else { return false };
        self.players.contains(k)
            || self
                .pets
                .get(k)
                .is_ok_and(|p| self.players.contains(p.owner))
    }

    /// [`Self::by_player`] for a kill.
    pub fn of(&self, kill: &MonsterKilled) -> bool {
        self.by_player(kill.killer.as_ref().map(|k| k.0))
    }
}

/// A dying or dead combatant; `timer` counts down to its next step (the
/// body's removal, or the player's respawn).
#[derive(Component)]
pub struct Dead {
    pub timer: f32,
}

/// Who dealt the killing blow to a non-character (a breakable): its `Destructible::TakeAttack` reaction leans away
/// from that attacker ([`crate::breakables`]).
#[derive(Component, Clone, Copy)]
pub struct KilledBy(pub Entity);

/// The killing blow overkilled the creature (`Character::SubtractLife` @0x180068b00: one damage type's amount that
/// leaves the life at or below 0 and is at least `maxLife * gibThreshold / 100`, [`gd_sim::actor::Hit::overkills`]):
/// its death may gib (`gibEffect`, with the `gore` option). A pet expiring with
/// `overkillOnDismiss` set gets it too (`pets` docs).
#[derive(Component)]
pub struct Overkill;

/// `ControllerCombat::KillMe`: the character dies with no killing blow (a pet's disband or expiry, the `GD_DIE`
/// hook) through the one death path, [`kill`] (death state, `Dead`, `MonsterKilled` for a monster, the proxy's
/// state). Queued: it runs with the caller's other commands, so a marker inserted before it (`Overkill`,
/// `pets::SuppressDyingSkill`) is on the entity when the death systems see `Dead`.
pub fn kill_me(commands: &mut Commands, target: Entity) {
    commands.run_system_cached_with(kill_me_now, target);
}

/// [`kill_me`]'s one-shot system: life to 0 and [`kill`] with no killer; nothing for a character that is gone or
/// already dead.
fn kill_me_now(
    In(target): In<Entity>,
    mut commands: Commands,
    mut q: Query<
        (
            &Transform,
            &mut Combatant,
            Option<&mut Character>,
            Option<&Monster>,
            Has<Player>,
        ),
        Without<Dead>,
    >,
    mut states: ResMut<ProxyStates>,
    mut killed: MessageWriter<MonsterKilled>,
) {
    let Ok((tf, mut cb, mut ch, monster, is_player)) = q.get_mut(target) else {
        return;
    };
    cb.life = 0.0;
    cb.target = None;
    kill(
        &mut commands,
        target,
        tf.translation,
        ch.as_deref_mut(),
        monster,
        is_player,
        &mut states,
        &mut killed,
        None,
    );
}

/// `gameengine.dbr` `meleeRange` (1.25 m): the gap between two actors' radii
/// a melee attack reaches (`GameEngine::GetMeleeRange`).
pub const MELEE_RANGE: f32 = 1.25;

/// `actorRadius` of a record that has none: Engine.dll `Actor::Load` @0x180028c50 reads `actorRadius`,
/// `actorHeight` and `scale` with the default 1.0 each (no riftgate record has an `actorRadius`).
pub const ACTOR_RADIUS: f32 = 1.0;

/// The radius of an actor spawned from `rec`: `Actor::GetRadius` (Engine.dll @0x18002b410, Object vtable
/// +456) = the length of the current scale vector (Actor +1180) x `actorRadius` (Actor +936); `Actor::Load`
/// @0x180028c50 copies the record's `scale` into all three components of the base scale (+1192) and hands it to
/// `SetScale(Vec3)` (vtable +776), so it is sqrt(3) x `scale` x `actorRadius`. The crowd agent's radius
/// (`Character::CrowdAgentCreated` @0x180066c20), the reach (`Character::GetTargetDistance` @0x18005df40: skill
/// range + both radii), `Character::NearTarget` @0x18005e0b0 and a fixed actor's use range
/// (`FixedActor::GetUsableRange` @0x1802a6130 and `ControllerPlayerState::DefaultRequestInteractableAction`
/// @0x180163080 call the same vtable slot on the chest / door / shrine / riftgate; Game.dll overrides it only for
/// `AreaOfInterest` and `PatrolPoint`) all read it: the one radius of characters and usable objects alike, which
/// their components keep (`Combatant::radius`, `Chest::radius`, `Door::radius`, `Shrine::radius`) for the hover
/// pick and the use range (a riftgate's `Waypoint::radius` is its teleporter `radius`, not this). `default`
/// stands for a missing `actorRadius` ([`ACTOR_RADIUS`] is the engine's); a missing or zero `scale` is 1 (as
/// `creature::spawn_creature`). The Colossus growth scales it (`colossus.rs`).
/// Not this: the loot drop radius and the nav obstacle of a breakable read the raw `actorRadius` field
/// (`gd_sim::loot_toss::drop_radius`, `breakables` module doc).
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

/// A fixed actor's `usableRange`, the term its use range adds to both radii (`player_orders::usable_range`):
/// Game.dll, every fixed actor's `Load` (`FixedActor`, `FixedItem`, `FixedDoor`, `FixedLever`,
/// `DynamicDoor`, `StaticShrine`, `StaticTeleporter` @0x18056b110, `AscendantAltar`, `BindingInteractable`) reads
/// it with the default 1.0, so a record without one (nearly every chest and door) is used from 1 m beyond the
/// radii, not 0.
pub fn fixed_usable_range(rec: Option<&gd_formats::arz::Record>) -> f32 {
    rec.and_then(|r| r.f32("usableRange")).unwrap_or(1.0)
}

/// The use range of the fixed actor of `rec` for a player of `player_radius`: `player_orders::usable_range` with
/// the object's [`get_radius`] and [`fixed_usable_range`], for the objects that keep no radius of their own
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

/// [`Combatant::bounds`] from the character's mesh at its scale in its current pose
/// (`gd_sim::bounds::Aabb::posed`).
fn body_bounds(ch: &Character) -> gd_sim::bounds::Aabb {
    if ch.body.bones.is_empty() || ch.body.hit_boxes.is_empty() {
        return gd_sim::bounds::Aabb::object(&ch.body, ch.scale);
    }
    gd_sim::bounds::Aabb::posed(&ch.body, ch.scale, ch.posed())
}

/// [`body_bounds`] when either component is inserted.
pub(super) fn body_bounds_on_combatant(
    ev: On<Insert<Combatant>>,
    mut q: Query<(&mut Combatant, &Character)>,
) {
    if let Ok((mut cb, ch)) = q.get_mut(ev.entity) {
        cb.bounds = body_bounds(ch);
    }
}

/// See [`body_bounds_on_combatant`].
pub(super) fn body_bounds_on_character(
    ev: On<Insert<Character>>,
    mut q: Query<(&mut Combatant, &Character)>,
) {
    if let Ok((mut cb, ch)) = q.get_mut(ev.entity) {
        cb.bounds = body_bounds(ch);
    }
}

/// `GraphicsMeshInstance::UpdateBoundingBox` each frame (@0x1800c3230): the box around the hit boxes
/// in the pose [`crate::character::advance_animations`] just sampled. A settled clip keeps its pose (and box);
/// a skeleton without hit boxes has a static box. Per character, in parallel.
pub(super) fn posed_bounds(mut q: Query<(&mut Combatant, &Character)>) {
    q.par_iter_mut().for_each(|(mut cb, ch)| {
        if ch.channel.settled() || ch.body.bones.is_empty() || ch.body.hit_boxes.is_empty() {
            return;
        }
        cb.bounds = body_bounds(ch);
    });
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
    /// [`Self::bounds`] in the world under `m` (`Actor::UpdateBoundingBox`: Entity +92).
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

    /// Whether `GameEngine::GetTargetsInRadius(at, r)` @0x1802ebe50 finds this body under `m` (the
    /// sphere meets its world box, `gd_sim::bounds::Aabb::meets_sphere`; 3D).
    pub fn in_radius(&self, m: &bevy::math::Affine3A, at: Vec3, r: f32) -> bool {
        self.body_box(m).meets_sphere(at.to_array(), r)
    }

    /// Whether `GameEngine::GetTargetsAroundRay(ray, len, thick)` @0x1802ec0b0 finds this body under `m`
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
