use super::*;

/// `SkillActivated_Suicide` used (`casting::fire_casts`): the user kills
/// itself. `StartAction` plays the skill's special
/// animation (else the character's action 14), and `ReceiveActiveUpdate`
/// gives the user a `Bonus` of −100 % life
/// (`Character::TakeBonus` → `CharacterBio::TakeBonus`, no attacker), so it
/// dies of its own hand: the killer is whoever attacked it last (the `CombatManager.lastAttacker` the death
/// event sends, [`Attackers::last`]; none if nothing did), its dying skill goes off as for any
/// death (`ControllerMonsterStateDying::StartDyingSkill`; monsters:
/// `monster_skills`, pets: `pets.rs`). It deals no damage itself. Stock
/// users take it as a special attack (`specialAttackSkillName`): monster
/// traps (`firetrap_suicide`: chthonic shards, ice spikes, cave-ins, the
/// aether anomaly), Elemental Seeker and Black Blood of Yugol pets, the
/// Gaze of Dreeg pet. The player's Thermite Mines list it in `skillName1`
/// only, so they never use it (they end by `spawnObjectsTimeToLive`); the
/// outlaw boss's mines' `heroskills/pets/thermitemine_skill_suicide` is a
/// `Skill_BuffSelfToggled`. PORT: the death comes when the cast fires.
#[derive(Component)]
pub struct Suicide;

/// [`Suicide`]: life to 0 and [`kill`] by its last attacker.
pub(super) fn suicides(
    mut commands: Commands,
    mut q: Query<
        (
            Entity,
            &Transform,
            &mut Combatant,
            Option<&mut Character>,
            Option<&Monster>,
            Has<Player>,
            &Attackers,
        ),
        (With<Suicide>, Without<Dead>),
    >,
    mut states: ResMut<ProxyStates>,
    mut killed: MessageWriter<MonsterKilled>,
) {
    for (e, tf, mut cb, mut ch, monster, is_player, attackers) in &mut q {
        commands.entity(e).remove::<Suicide>();
        cb.life = 0.0;
        cb.target = None;
        debug!("{e} kills itself");
        kill(
            &mut commands,
            e,
            tf.translation,
            ch.as_deref_mut(),
            monster,
            is_player,
            &mut states,
            &mut killed,
            attackers.last.map(|by| (by, None)),
        );
    }
}

/// A character's life ran out (hit or damage over time).
#[allow(clippy::too_many_arguments)]
pub fn kill(
    commands: &mut Commands,
    target: Entity,
    at: Vec3,
    tch: Option<&mut Character>,
    monster: Option<&Monster>,
    is_player: bool,
    states: &mut ProxyStates,
    killed: &mut MessageWriter<MonsterKilled>,
    killer: Option<(Entity, Option<String>)>,
) {
    let by = killer.as_ref().map(|k| k.0);
    if let Some(tch) = tch {
        tch.state = CharState::Death;
    }
    let dissolve = if is_player { 4.0 } else { 10.0 };
    match monster {
        Some(m) => {
            info!(
                "killed {} (level {})",
                m.record.rsplit('/').next().unwrap_or(""),
                m.level
            );
            killed.write(MonsterKilled {
                entity: target,
                record: m.record.clone(),
                level: m.level,
                position: at,
                killer,
            });
        }
        None if is_player => info!("the player died"),
        None => {}
    }
    commands.entity(target).try_insert(Dead { timer: dissolve });
    if monster.is_none()
        && !is_player
        && let Some(by) = by
    {
        commands.entity(target).try_insert(super::KilledBy(by));
    }
    if let Some(m) = monster
        && let Some(list) = states.0.get_mut(&(m.slot.0, m.slot.1))
        && let Some(s) = list.get_mut(m.slot.2)
    {
        s.alive = false;
    }
}

/// How long after `Character::DestroyMe`'s countdown starts the dead character is removed: the
/// `Character` ctor sets `Character.destroyDelay` to 10000 ms; `Character::UpdateSelf` counts it down once the
/// character is dead (state 4, set by `Character::ImDead`, which the death manager calls when every death
/// handler has finished, `DeathManager::OnHandlersFinished`) and then calls `DestroyMe` unless
/// `Character.destroyOnDeath` is clear (only `Corpse` clears it).
const DESTROY_DELAY: f32 = 10.0;

/// Seconds a monster's or pet's body stays after death (`f32::INFINITY`: until an
/// animation callback starts it, or for good). `Character::InitialUpdate`
/// picks the death handler by `deleteBehavior`:
/// - `Dissolve`: `DissolveActorDeathHandler::Execute` waits
///   the `corpsePersistence` option (Engine.dll `Options`, in
///   minutes: `× 60000` ms) — none with `ignoreDissolveDelay` — then the mesh dissolves
///   over `dissolveTime` s and the handler finishes (`Update`);
/// - `DissolveOnCallback`: the dissolve starts at the death animation's
///   `dissolve` callback (`AnimationCallback`);
/// - `Fade`: `FadeActorDeathHandler` fades over `dissolveTime` s at once;
/// - `Delete`: hidden when the handler is processed (`PostProcess`);
/// - `Corpse`: no handler, `Character.destroyOnDeath` cleared: the body stays for good;
/// - anything else (`None`, `PlayEffect`): only `DefaultDeathHandler`, which finishes when the death animation /
///   ragdoll has (`Update`); the body goes [`DESTROY_DELAY`] later. PORT: the countdown runs from
///   the death (the animation's length is not added).
///
/// The removal itself is `DestroyMe` [`DESTROY_DELAY`] after the handlers finish; a dissolved / faded / hidden
/// body is already invisible then, so the port removes those at the handler's end.
pub fn corpse_time(rec: &gd_formats::arz::Record, persistence_min: f32) -> f32 {
    let dissolve = rec.f32("dissolveTime").unwrap_or(0.0).max(0.0);
    match rec.str("deleteBehavior").unwrap_or("") {
        "Dissolve" => {
            (if rec.bool("ignoreDissolveDelay") {
                0.0
            } else {
                persistence_min.max(0.0) * 60.0
            }) + dissolve
        }
        "Fade" => dissolve,
        "Delete" => 0.0,
        "DissolveOnCallback" | "Corpse" => f32::INFINITY,
        _ => DESTROY_DELAY,
    }
}

/// The record's death-handler presentation data, kept on the body from the
/// death; the presentation dissolves / fades the body from it and the
/// [`Dead`] countdown (grimdawn `monsters::draw_corpses`)
/// (`Character::InitialUpdate` picks the handler by
/// `deleteBehavior`, [`corpse_time`];
/// `DissolveActorDeathHandler::StartDissolve` reads the
/// `dissolve*` fields). Inserted for monsters and pets at the death (the
/// player respawns; its body is not dissolved).
#[derive(Component, Clone, Debug)]
pub struct Corpse {
    /// `deleteBehavior`: `Dissolve`, `DissolveOnCallback`, `Fade`, `Delete`,
    /// `Corpse`, or none / `PlayEffect` (the default handler).
    pub delete_behavior: String,
    /// `dissolveTime` (s): the dissolve / fade duration.
    pub dissolve_time: f32,
    /// `dissolveTexture`.
    pub dissolve_texture: Option<String>,
    /// `dissolveColorR/G/B` (0..255).
    pub dissolve_color: [f32; 3],
    /// `dissolveEffect` (the particle pak played while dissolving).
    pub dissolve_effect: Option<String>,
    /// `dissolveLight` (the light that shrinks with the dissolve).
    pub dissolve_light: Option<String>,
    /// `ignoreDissolveDelay`: skip the `corpsePersistence` wait.
    pub ignore_dissolve_delay: bool,
    /// The `corpsePersistence` option (minutes) at the death.
    pub persistence_min: f32,
}

impl Corpse {
    fn from_record(rec: &gd_formats::arz::Record, persistence_min: f32) -> Self {
        let s = |k: &str| rec.str(k).filter(|s| !s.is_empty()).map(str::to_owned);
        Self {
            delete_behavior: rec.str("deleteBehavior").unwrap_or("").to_owned(),
            dissolve_time: rec.f32("dissolveTime").unwrap_or(0.0).max(0.0),
            dissolve_texture: s("dissolveTexture"),
            dissolve_color: [
                rec.f32("dissolveColorR").unwrap_or(0.0),
                rec.f32("dissolveColorG").unwrap_or(0.0),
                rec.f32("dissolveColorB").unwrap_or(0.0),
            ],
            dissolve_effect: s("dissolveEffect"),
            dissolve_light: s("dissolveLight"),
            ignore_dissolve_delay: rec.bool("ignoreDissolveDelay"),
            persistence_min,
        }
    }
}

/// Dead monsters and pets stay for their [`corpse_time`]; the player respawns.
/// The player's death ends its move: `ControllerPlayer::CharacterIsDying` hands the death
/// to the running controller state (`ControllerState::CharacterIsDying`), so a `MoveTo` / `MoveToSkill` state is
/// left, and
/// `Character::StartRespawn` teleports (`TeleportToLocation`). PORT: the states' own handlers were
/// not decompiled; the pending walk ([`crate::movement::MovePath`]) and skill move
/// ([`crate::movement::MoveToUseSkill`]) are dropped at the death and again with the respawn teleport, as every
/// other teleport does. What the death clears of DoTs and crowd control: `crate::afflictions`, "Death".
#[allow(clippy::too_many_arguments)]
pub(super) fn dead_bodies(
    mut commands: Commands,
    time: Res<Time>,
    // Option: the player respawns in place without a start; the corpses still go.
    start: Option<Res<PlayerStart>>,
    nav: Res<Nav>,
    data: Res<GameData>,
    mut callbacks: MessageReader<AnimCallback>,
    mut q: Query<(
        Entity,
        &mut Dead,
        &mut Transform,
        &mut Combatant,
        &mut Character,
        Has<Player>,
        Option<&Monster>,
        Option<&crate::pets::Pet>,
        Has<crate::survival::RespawnHeld>,
        Option<&mut crate::movement::MovePath>,
    )>,
    // Option: a new character has no `LoadedCharacter`.
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
    choice: Res<gd_game_core::session::Choice>,
    options: Res<gd_game_core::options::GameOptions>,
) {
    let hardcore = gd_game_core::session::is_hardcore(loaded.as_deref(), &choice);
    let dissolve_cb: Vec<Entity> = callbacks
        .read()
        .filter(|c| c.name.eq_ignore_ascii_case("dissolve"))
        .map(|c| c.entity)
        .collect();
    for (e, mut dead, mut tf, mut cb, mut ch, is_player, monster, pet, held, mut path) in &mut q {
        if is_player && dead.is_added() {
            end_moves(&mut commands, e, path.as_deref_mut());
        }
        // The record only at the death and at a dissolve callback.
        let record = monster.map(|m| &m.record).or(pet.map(|p| &p.record));
        if (dead.is_added() || dissolve_cb.contains(&e))
            && let Some(rec) = record.and_then(|r| data.db.get(r))
        {
            // (`Added<Dead>` in a second query would conflict with `&mut Dead`.)
            if dead.is_added() {
                let persistence = options.float("corpsePersistence");
                dead.timer = corpse_time(rec, persistence);
                commands
                    .entity(e)
                    .try_insert(Corpse::from_record(rec, persistence));
            }
            if dead.timer.is_infinite()
                && dissolve_cb.contains(&e)
                && rec.str("deleteBehavior") == Some("DissolveOnCallback")
            {
                dead.timer = rec.f32("dissolveTime").unwrap_or(0.0).max(0.0);
            }
        }
        dead.timer -= time.delta_secs();
        if is_player && hardcore {
            continue;
        }
        if is_player {
            // The Crucible keeps a dead player down while respawn is disabled
            // (`Game.DisableRespawn`, [`crate::survival::respawn_control`]).
            if held && dead.timer <= 0.0 {
                dead.timer = 0.0;
                continue;
            }
            if dead.timer <= 0.0 {
                cb.life = cb.profile.sheet.life;
                cb.target = None;
                ch.state = CharState::Idle;
                end_moves(&mut commands, e, path.as_deref_mut());
                if let Some(s) = &start {
                    let y = nav
                        .0
                        .nearest(s.0.to_array(), 8.0)
                        .map(|c| nav.0.pos(c)[1])
                        .unwrap_or(s.0.y);
                    tf.translation = Vec3::new(s.0.x, y, s.0.z);
                }
                // The respawn teleport ends a throw still running (`Character::StartRespawn`).
                commands
                    .entity(e)
                    .try_remove::<(Dead, crate::ragdoll::Ragdoll)>();
            }
        } else if dead.timer <= 0.0 {
            commands.entity(e).try_despawn();
        }
    }
}

/// Drops the walk and the pending skill move of `e` ([`dead_bodies`]).
fn end_moves(commands: &mut Commands, e: Entity, path: Option<&mut crate::movement::MovePath>) {
    if let Some(path) = path {
        path.points.clear();
    }
    commands
        .entity(e)
        .try_remove::<crate::movement::MoveToUseSkill>();
}

/// Life regen for every living character: `CharacterBio::Update`
/// runs for all of them (no player gate — monsters and pets
/// recover between pulls; their `life_regen_per_sec` is the bio equation,
/// `gd_sim::stats::BioStore::life_regen_per_sec`). Negative regen is a
/// non-lethal drain (the update's `LifeRegenValue < 0` branch zeroes it
/// when it would take the life to 0, or the life is already ≤ 1). The
/// constitution / accelerated regen stays player-only
/// (`crate::constitution`). PORT: the name stays `player_regen` for
/// `combat/mod.rs`'s chain.
pub(super) fn player_regen(
    time: Res<Time>,
    mut q: Query<&mut Combatant, (With<Character>, Without<Dead>)>,
) {
    let dt = time.delta_secs();
    for mut cb in &mut q {
        // A character killed this frame (not `Dead` yet) does not regen.
        if cb.life <= 0.0 {
            continue;
        }
        let max = cb.profile.sheet.life;
        let mut regen = cb.profile.sheet.life_regen_per_sec * dt;
        if regen < 0.0 && (-regen >= cb.life || cb.life <= 1.0) {
            regen = 0.0;
        }
        let life = (cb.life + regen).min(max);
        if life != cb.life {
            cb.life = life;
        }
    }
}

/// The candidate frustum's stand-in margin ([`pick_monster`], [`pick_enemy`]): the ray passes within 0.4 m of a
/// body box (the engine's sub-frustum is 0.011 of the viewport wide each way; the frustum needs the camera).
const NEAR_CURSOR: f32 = 0.4;
/// The widened margin while a held click is pathing to an enemy / repeated
/// (`InGameUI::HandleMouseEvent` sets the frustum factor 0.035 for
/// `IsPathing || IsCommandRepeated`): 0.4 m × 0.035 / 0.011.
const NEAR_CURSOR_HELD: f32 = NEAR_CURSOR * (0.035 / 0.011);

/// The monster (alive) under the cursor ray. Grim Dawn.exe `InGameUIActorCapture::PickObjectUnderCursor`: of
/// the candidates near the cursor, the one whose mesh the ray meets first (`Actor::GetIntersection`:
/// its posed, scaled hit boxes, else its object-space box, under its coords); none: the one whose centre of mass
/// is nearest to the ray. The candidates (`Game::GatherCursorCandidates`) are the actors whose box meets a
/// sub-frustum around the
/// cursor, 0.011 of the viewport wide each way (0.035 while a held click is pathing to an enemy).
/// PORT: the world box ([`Combatant::body_box`], the posed hit boxes' union) stands in for the mesh test, and
/// the candidates are the boxes the ray passes within 0.4 m of (the frustum needs the camera); the centre of mass
/// is the box centre.
pub fn pick_monster<'a>(
    ray: Ray3d,
    monsters: impl Iterator<Item = (Entity, &'a GlobalTransform, &'a Combatant)>,
) -> Option<Entity> {
    let (o, d) = (ray.origin.to_array(), ray.direction.to_array());
    let mut hit: Option<(f32, Entity)> = None;
    let mut near: Option<(f32, Entity)> = None;
    for (e, gt, cb) in monsters {
        let b = cb.body_box(&gt.affine());
        let t = b.ray_entry(o, d);
        if t < f32::INFINITY {
            if hit.is_none_or(|(bt, _)| t < bt) {
                hit = Some((t, e));
            }
            continue;
        }
        let mut grown = b;
        grown.half = grown.half.map(|h| h + NEAR_CURSOR);
        if grown.ray_entry(o, d) < f32::INFINITY {
            let c = Vec3::from(b.center);
            let k = (c - ray.origin).dot(*ray.direction).max(0.0);
            let dist = ray.get_point(k).distance(c);
            if near.is_none_or(|(bd, _)| dist < bd) {
                near = Some((dist, e));
            }
        }
    }
    hit.or(near).map(|b| b.1)
}

/// The hover / combat enemy under the cursor: of the enemies the cursor touches (their posed body box,
/// [`pick_monster`]'s candidate test), the one nearest to the **player**, one pick feeding both the hover and the
/// attack ([`crate::player_orders`]). Grim Dawn.exe picks nearest to the player only on the controller targeting's
/// candidates (`InGameUI::PickNearestEnemy`); its mouse path picks the mesh hit, else the one nearest to the ray
/// (`InGameUIActorCapture::PickObjectUnderCursor`, [`pick_monster`]). `wide` widens
/// the candidate margin while a held click is pathing / repeated ([`NEAR_CURSOR_HELD`], the stickiness).
pub fn pick_enemy<'a>(
    ray: Ray3d,
    monsters: impl Iterator<Item = (Entity, &'a GlobalTransform, &'a Combatant)>,
    player: Vec3,
    wide: bool,
) -> Option<Entity> {
    let margin = if wide { NEAR_CURSOR_HELD } else { NEAR_CURSOR };
    let (o, d) = (ray.origin.to_array(), ray.direction.to_array());
    let mut best: Option<(f32, Entity)> = None;
    for (e, gt, cb) in monsters {
        let b = cb.body_box(&gt.affine());
        let mut grown = b;
        grown.half = grown.half.map(|h| h + margin);
        if grown.ray_entry(o, d) < f32::INFINITY {
            let dist = Vec3::from(b.center).distance(player);
            if best.is_none_or(|(bd, _)| dist < bd) {
                best = Some((dist, e));
            }
        }
    }
    best.map(|b| b.1)
}
