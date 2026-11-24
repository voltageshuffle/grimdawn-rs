use super::*;

/// The record's `hiddenFromCombat` (`Character::Load`, `Character.hiddenFromCombat`):
/// `Character::IsHiding` is true for it, and
/// `GameEngine::FilterInvalidTargets` (every skill target query:
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

/// The record's `invincible` (`Character::Load` sets `Character.invincible`, the live flag of
/// `Character::IsInvincible`, and `Character.invincibleInDbr`, `IsInvincibleInDbr`).
/// `Character::SubtractLife` loses no life while it is set
/// (unless forced), `DurationDamageManager::AddDamage` / `AddFixedDamage`
/// add no damage over time, `Character::DebufTarget` applies nothing
/// (`skill_buffs::land`) and `Monster::IsTargetable` is false (the click pick queries in `grimdawn` skip it). PORT: hits
/// still resolve (block, hit reactions); only the life loss is undone and
/// over-time damage skipped. `SetInvincible` is this component's insert / remove (`spirit.rs`
/// uses it); Lua `ToggleInvincible` (no stock use) raises a Lua error naming the API.
#[derive(Component)]
pub struct Invincible;

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
/// defender's). `Player::OnCreatureDeath` (`IncrementKills`: the
/// Crucible score and the kill statistics) takes the killer itself or a `Monster` whose `GetLeader` is the main
/// player; `GameEngine::OnCreatureDeath` pays experience to `GetMasterAttacker` (a
/// monster's leader, a leaderless one itself) only when it is the local player (`HandleBonus`), and
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

/// Who dealt the killing blow to a non-character (a breakable): its `Destructible::TakeAttack` reaction leans away
/// from that attacker ([`crate::breakables`]).
#[derive(Component, Clone, Copy)]
pub struct KilledBy(pub Entity);

/// The killing blow overkilled the creature (`Character::SubtractLife`: one damage type's amount that
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

/// `GraphicsMeshInstance::UpdateBoundingBox` each frame: the box around the hit boxes
/// in the pose [`crate::character::advance_animations`] just sampled. A settled clip keeps its pose (and box);
/// a skeleton without hit boxes has a static box. Per character, in parallel.
/// The write bypasses change detection: the `Changed<Combatant>` readers ([`super::sync_speeds`],
/// [`crate::char_sounds::low_health_vox`]) watch life / profile changes, not the pose box.
pub(super) fn posed_bounds(mut q: Query<(&mut Combatant, &Character)>) {
    q.par_iter_mut().for_each(|(mut cb, ch)| {
        if ch.channel.settled() || ch.body.bones.is_empty() || ch.body.hit_boxes.is_empty() {
            return;
        }
        cb.bypass_change_detection().bounds = body_bounds(ch);
    });
}
