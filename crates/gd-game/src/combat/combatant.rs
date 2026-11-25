use super::*;

/// The record's `hiddenFromCombat` (`Character::IsHiding`): `GameEngine::FilterInvalidTargets` drops hiding
/// characters from every skill target query unless asked for. Stock: totems, runes, traps, pets that cause no
/// anger, quest / flavour NPCs. Ported at query time ([`Side::hits`]); the attack itself does not test it.
#[derive(Component)]
pub struct HiddenFromCombat;

/// The record's `invincible` (`Character::IsInvincible` / `IsInvincibleInDbr`): `Character::SubtractLife` loses
/// no life (unless forced), `DurationDamageManager::AddDamage` adds no damage over time, `Character::DebufTarget`
/// applies nothing and `Monster::IsTargetable` is false. PORT: hits still resolve (block, hit reactions); only
/// the life loss is undone and over-time damage skipped. `SetInvincible` / Lua `ToggleInvincible` insert and
/// remove it.
#[derive(Component)]
pub struct Invincible;

/// A monster died.
#[derive(Message, Clone, Debug)]
pub struct MonsterKilled {
    pub record: String,
    pub level: i32,
    pub position: Vec3,
    /// Its `Worn` gear drops.
    pub entity: Entity,
    /// The killing blow's dealer and skill (None: a plain attack), when known.
    pub killer: Option<(Entity, Option<String>)>,
}

/// Whose kill a [`MonsterKilled`] is: the main player's when the killer is the player or one of his pets (a
/// DoT's kill is its applier's; a reflected hit's the reflecting defender's). `Player::OnCreatureDeath`
/// (`IncrementKills`) and `GameEngine::OnCreatureDeath` (`HandleBonus`, `HandleFaction`, Dread) credit only a
/// killer whose master (`GetMasterAttacker` / `GetLeader`) is the player. So nothing for a death without a
/// killer (a [`super::Suicide`]'s is its [`Attackers::last`]), a killer that no longer exists, a leaderless
/// monster (stock `trap_*` creatures are plain `Monster`s) or a hostile monster killing an ally.
#[derive(bevy::ecs::system::SystemParam)]
pub struct KillCredit<'w, 's> {
    players: Query<'w, 's, Entity, With<Player>>,
    pets: Query<'w, 's, &'static crate::pets::Pet>,
}

impl KillCredit<'_, '_> {
    pub fn by_player(&self, killer: Option<Entity>) -> bool {
        let Some(k) = killer else { return false };
        self.players.contains(k)
            || self
                .pets
                .get(k)
                .is_ok_and(|p| self.players.contains(p.owner))
    }

    pub fn of(&self, kill: &MonsterKilled) -> bool {
        self.by_player(kill.killer.as_ref().map(|k| k.0))
    }
}

/// Who dealt the killing blow to a breakable (`Destructible::TakeAttack` leans away from it).
#[derive(Component, Clone, Copy)]
pub struct KilledBy(pub Entity);

/// The killing blow overkilled the creature (`Character::SubtractLife`, [`gd_sim::actor::Hit::overkills`]): its
/// death may gib (`gibEffect`, with the `gore` option). Also a pet expiring with `overkillOnDismiss`.
#[derive(Component)]
pub struct Overkill;

/// `ControllerCombat::KillMe`: the character dies with no killing blow through [`kill`]. Queued, so a marker
/// inserted before it (`Overkill`, `pets::SuppressDyingSkill`) is on the entity when the death systems see `Dead`.
pub fn kill_me(commands: &mut Commands, target: Entity) {
    commands.run_system_cached_with(kill_me_now, target);
}

/// Nothing for a character that is gone or already dead.
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

/// [`Combatant::bounds`] from the character's mesh at its scale in its current pose.
fn body_bounds(ch: &Character) -> gd_sim::bounds::Aabb {
    if ch.body.bones.is_empty() || ch.body.hit_boxes.is_empty() {
        return gd_sim::bounds::Aabb::object(&ch.body, ch.scale);
    }
    gd_sim::bounds::Aabb::posed(&ch.body, ch.scale, ch.posed())
}

pub(super) fn body_bounds_on_combatant(
    ev: On<Insert<Combatant>>,
    mut q: Query<(&mut Combatant, &Character)>,
) {
    if let Ok((mut cb, ch)) = q.get_mut(ev.entity) {
        cb.bounds = body_bounds(ch);
    }
}

pub(super) fn body_bounds_on_character(
    ev: On<Insert<Character>>,
    mut q: Query<(&mut Combatant, &Character)>,
) {
    if let Ok((mut cb, ch)) = q.get_mut(ev.entity) {
        cb.bounds = body_bounds(ch);
    }
}

/// `GraphicsMeshInstance::UpdateBoundingBox` each frame: the box around the hit boxes in the current pose; a
/// settled clip or a skeleton without hit boxes keeps its box. The write bypasses change detection: the
/// `Changed<Combatant>` readers watch life / profile changes, not the pose box.
pub(super) fn posed_bounds(mut q: Query<(&mut Combatant, &Character)>) {
    q.par_iter_mut().for_each(|(mut cb, ch)| {
        if ch.channel.settled() || ch.body.bones.is_empty() || ch.body.hit_boxes.is_empty() {
            return;
        }
        cb.bypass_change_detection().bounds = body_bounds(ch);
    });
}
