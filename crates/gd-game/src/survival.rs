//! Crucible (survival mode) host: runs [`gd_sim::story::state::Survival`]'s timer and
//! applies the story effects of the `Game.*Survival*` bindings.
//!
//! `PlayStats::UpdateBonusTimer`
//! is called every frame by `Player::UpdateSelf` ([`gd_sim::story::state::Survival::tick`]);
//! `Game.KillAllMonsters` kills every monster hostile to the main player (flag, then the
//! combat controller's kill call; [`crate::monsters::is_foe`]).
//! Score (`PlayStats::IncrementKills` @0x1803fdf80, `GetScore` @0x1803fe220): each kill
//! sets the monster score from `playerscore.dbr`'s `<classification>Bonus` list (indexed by the
//! game difficulty) and adds `survivalScoreEquation` of it ([`gd_sim::story::state::Survival::add_kill`]);
//! `SaveSurvivalScore` locks it. `PlayStats::Load` @0x1803fd890 (the table at PlayStats+584, six entries
//! of the game difficulty's column, 0 past a list's end) and `GameEngine::ResolveMonsterClassificationEnum`
//! @0x1802ecfa0 (the `monsterClassification` string, exact match): Common 0 `commonBonus`, Champion 1
//! `championBonus`, Hero 2 `heroBonus`, Boss 3 `bossBonus`, Quest 4 `questBonus`, SuperBoss 5 `bossBonus` again;
//! any other text (or none) is Common.
//! `Game.SaveSurvivalScore` writes the leaderboard ([`save_score`]); the greatest difficulty / wave /
//! score are merged into `player.gdc` by `save::save_game`.
//! `Game.ResetSurvivalEvent`: it autosaves first (`GameEngine::AutoSave`), so
//! `SurvivalAutosave` writes a save ([`autosave`]; the Crucible uses the campaign's character files).
//!
//! A failed event and its restart (the scripts: `events/survivalevent.lua`, `survival/eventcontrol.lua`; test
//! `tests/crucible/restart.rs`): during a wave respawn is disabled (`SurvivalEvent_SpawnNext`: `EnableRespawn` then
//! `DisableRespawn`), so a dead player stays down ([`respawn_control`]) and the event's next update finds
//! `Game.PlayersDead` (softcore only, `Game.IsHardcore`): `eventFailed`, the fail sound, `eventFinished` (the end NPC
//! `npc_event_03`, the compensation chests, `SaveSurvivalScore`), `ResetSurvivalTimer`, `SurvivalEvent_End`
//! (`EnableRespawn`: the player comes back) and `KillAllMonstersOnNextRespawn`. `npc_event_03` offers "Reset to the
//! beginning" (`eventReset`) or, from wave 50 on (`checkRestartCounterGlobalMP`: -1 below 50, else the restarts),
//! "Restart at Checkpoint" for 5 / 15 / 30 tributes (restarts 0 / 1 / 2; none after 3): `eventRestart` =
//! `IncrementSurvivalRestarts`, `ResetSurvivalEvent` (autosave, then score / wave / ramp / mutators reset; restarts
//! stay), `Player.Teleport` to the spawn point, the start NPC back, and the wave tier and ramp set to
//! `floor(wave / 10) * 10 - 20` (wave 55 restarts at 30); the next start runs that wave's tier. The checkpoint
//! starts of the start NPC (waves 51 / 101 / 151 / 181: `startTier05Event` ...) are a separate, bought choice.
//!
//! Defenses (`survival/defenses.lua`, tests `tests/crucible/defenses.rs`): four sites per arena, each a Deathchill /
//! Fire / Lightning turret or a Defense / Offense banner (`records/creatures/defenses/*`, `02` / `03` the two
//! upgrades, `Character.Create` at `avg + avg / 50 + 1`), the site NPC swapped for its upgrade NPC (`b`, `c`) and
//! gone after the second upgrade; only the first build grants `SURVIVALMODE_DEFENSEBUILT`. The engine has no
//! defense code: they are plain `Monster`s of `faction_survivors`, the banners `ControllerStationaryMonster`s
//! (`controller_banner.dbr`: `BuffAllyBehavior` WhenEnemyIsSeen, the upgrades' `buffOtherSkillName`).
//!
//! Difficulties (test `tests/crucible/difficulty.rs`): Aspirant / Challenger / Gladiator are the game difficulty 0 /
//! 1 / 2 with no challenge (`session`); the engine's differences are the ramp pak by difficulty
//! ([`gd_sim::actor::Rules::with_survival_ramp`]) and the score column (`playerscore.dbr`); the bonus timer
//! (`SurvivalEvent_StartBonusTimer`: 75 / 75 / 80 s + 10 / 11 / 12 s per tier), the rewards and the defense prices are
//! the scripts' and conversations'.
//! PORT: `FullscreenGlow` is forwarded as [`crate::story::StoryEffect`] and drawn by the UI.
use crate::combat::{Combatant, Dead, MonsterKilled};
use crate::data::GameData;
use crate::monsters::{Monster, ProfileCache, ProxyStates};
use crate::story::{StoryEffect, StoryRuntime};
use bevy::prelude::*;
use gd_sim::story::Effect;

/// Counts the survival bonus timer down (only in a survival session).
pub fn tick_survival(rt: Option<NonSend<StoryRuntime>>, time: Res<Time>, mut carry: Local<f32>) {
    let Some(rt) = rt else { return };
    let mut story = rt.story.borrow_mut();
    // The Shattered Realm's bonus timer is the same player timer (`SetEndlessTimerInbound`).
    let crucible = story.game.survival_mode;
    if crucible || story.endless.dungeon.is_some() {
        story
            .game
            .survival
            .tick(crate::session::frame_ms(&mut carry, &time), crucible);
    }
}

/// The difficulty ramp (`Game.SetSurvivalDifficulty`) and the mutators (`Game.AddMutator`) change the
/// monsters' (and the player's) attributes: the rules are rebuilt ([`gd_sim::actor::Rules::with_survival_ramp`],
/// [`gd_sim::actor::Rules::with_mutators`]), the profile cache cleared, and live monsters and the
/// player are marked for a profile rebuild (the engine reloads their base attributes:
/// `Monster::OnDifficultyRampLevel`; mutators likewise).
#[allow(clippy::too_many_arguments)]
pub fn apply_rules(
    rt: Option<NonSend<StoryRuntime>>,
    data: Res<GameData>,
    mut rules: ResMut<crate::rules::CombatRules>,
    mut profiles: ResMut<ProfileCache>,
    mut monsters: Query<&mut crate::monster_skills::MonsterSkills>,
    players: Query<Entity, With<crate::player::Player>>,
    mut commands: Commands,
    mut applied: Local<(u32, Vec<String>)>,
    mut new_session: crate::session::NewSession,
    (time, mut sound, mut cooldown, mut mode): (
        Res<Time>,
        MessageWriter<crate::visuals::PlaySound>,
        Local<(f32, f32)>,
        ResMut<crate::session::GameMode>,
    ),
) {
    cooldown.0 -= time.delta_secs();
    cooldown.1 -= time.delta_secs();
    if new_session.begun() {
        *applied = Default::default();
    }
    let Some(rt) = rt else { return };
    let want = {
        let s = rt.story.borrow();
        // The Crucible, or a running Shattered Realm dungeon (its mutators); else nothing.
        let (ramp, mutators): (u32, &[String]) =
            if s.game.survival_mode || s.endless.dungeon.is_some() {
                (s.game.survival.difficulty_ramp, &s.game.survival.mutators)
            } else {
                (0, &[])
            };
        if (ramp, mutators) == (applied.0, applied.1.as_slice()) {
            return;
        }
        (ramp, mutators.to_vec())
    };
    // `GameEngine::AddMutatorInbound` @0x1802f8d70: a mutator added plays the
    // `mutatorSound` pak of `survivalinfo.dbr` as a 2D sound (5 s cooldown), and, when a new mutator `affectsPlayers`,
    // `PlayMutatorPlayerFx` @0x1802f9280 (its own 5 s cooldown): `PFxManager::OneShotParticleEffect(main player,
    // mutatorFx, "")`, a self-deleting `EffectEntity` attached to the player (`Actor::Attach` → `Entity::Attach`
    // @0x18007ed90, Engine.dll: identity coords, the empty attach name: the player's own coords). The
    // record's `boneList` (`playermutator_fx01.dbr`: the weapon bones) places nothing: `EffectEntity::Load`
    // @0x180064000 stores it (+528, with `emitterType` Standard / BoneList / AllBones at +553 / +554), but its only
    // reader `EffectEntity::PullParentBones` @0x180064c70 and the `Effect::SetBoneList` @0x180063230 it would feed
    // have no caller (Engine.dll; Game.dll imports neither), so `Effect::Update` passes the emitters no bones and the
    // effect emits at the player's origin, as here.
    let added: Vec<&String> = want.1.iter().filter(|m| !applied.1.contains(m)).collect();
    let info = data.db.get("records/game/survivalinfo.dbr");
    if !added.is_empty()
        && cooldown.0 <= 0.0
        && let Some(pak) = info.as_ref().and_then(|r| r.str("mutatorSound"))
    {
        cooldown.0 = 5.0;
        sound.write(crate::visuals::PlaySound {
            pak: pak.to_owned(),
            at: None,
        });
    }
    if cooldown.1 <= 0.0
        && added.iter().any(|m| {
            data.db
                .get(m)
                .is_some_and(|r| r.f32("affectsPlayers").is_some_and(|v| v != 0.0))
        })
        && let Some(fx) = info
            .as_ref()
            .and_then(|r| r.str("mutatorFx"))
            .filter(|f| !f.is_empty())
        && let Ok(player) = players.single()
    {
        cooldown.1 = 5.0;
        let mut spec = crate::visuals::EffectSpec::record(fx.to_owned());
        spec.despawn_when_complete = true;
        let fx = commands.spawn((spec, Transform::IDENTITY)).id();
        commands.entity(player).add_child(fx);
    }
    info!(
        "survival: difficulty ramp {}, mutators {:?}",
        want.0, want.1
    );
    // The ramp is the Crucible's (it would drop an Ultimate Veteran's challenge pak otherwise); the Shattered Realm
    // only has mutators.
    let crucible = rt.story.borrow().game.survival_mode;
    let base = if crucible {
        rules.0.with_survival_ramp(&data.db, want.0)
    } else {
        (*rules.0).clone()
    };
    rules.0 = std::sync::Arc::new(base.with_mutators(&data.db, &want.1));
    // The proxies' spawn counts read the ramp (`gd_sim::proxy::run_pool`).
    let ramp = crucible.then_some(want.0);
    if mode.0.ramp != ramp {
        mode.0.ramp = ramp;
    }
    *applied = want;
    profiles.0.clear();
    for mut ms in &mut monsters {
        ms.mark_profile_dirty();
    }
    for p in &players {
        commands.entity(p).try_insert(crate::player::ProfileDirty);
    }
}

/// Marks a player the Crucible keeps dead: respawn is disabled (`Game.DisableRespawn`).
#[derive(Component)]
pub struct RespawnHeld;

/// `Game.EnableRespawn` / `DisableRespawn` (GameEngine byte +4848,
/// and `ControllerPlayer` byte +1328 set by Enable): while respawn is disabled a dead player stays
/// dead ([`RespawnHeld`], read by `combat::dead_bodies`), which is how a failed wave ends the event
/// (`Game.PlayersDead`); `EnableRespawn` brings a dead player back at once.
/// `ControllerPlayer::CharacterIsDying` @0x18015db20 (mode 1): respawn disabled (+4848 false) clears
/// +1328; the "last death" texts (`tagSurvivalDeath` / `tagSurvivalLastDeath`) are for more than one player only, so
/// a single player gets no dialog. `ControllerPlayer::DeathUpdate`: with +1328 set the dead player respawns on
/// his own at `Player::GetClosestRespawnPoint` (`StartRespawnForLocalPlayer`); with it clear he lies there.
/// The `KillAllMonstersOnNextRespawn` flag (+226120) is read by that respawn's activity (sub_18042B740,
/// the vtable `StartRespawnForLocalPlayer` installs): on arrival `GameEngine::KillAllMonsters`, which clears it.
/// PORT: the kill is sent when the respawn is requested (the body moves at once here, no fade).
pub fn respawn_control(
    mut commands: Commands,
    rt: Option<NonSend<StoryRuntime>>,
    mut players: Query<(Entity, Option<&mut Dead>, Has<RespawnHeld>), With<crate::player::Player>>,
    mut out: MessageWriter<StoryEffect>,
) {
    let Some(rt) = rt else { return };
    let mut story = rt.story.borrow_mut();
    if !story.game.survival_mode {
        return;
    }
    let sv = &mut story.game.survival;
    let (hold, request) = (!sv.respawn, std::mem::take(&mut sv.respawn_requested));
    let kill = request && std::mem::take(&mut sv.kill_on_respawn);
    for (e, dead, held) in &mut players {
        if hold != held {
            if hold {
                commands.entity(e).try_insert(RespawnHeld);
            } else {
                commands.entity(e).try_remove::<RespawnHeld>();
            }
        }
        if request && let Some(mut d) = dead {
            d.timer = 0.0;
            commands.entity(e).try_remove::<RespawnHeld>();
            if kill {
                out.write(StoryEffect(Effect::KillAllMonsters));
            }
        }
    }
}

/// A kill's score into the survival run (see the module docs).
pub fn score_kills(
    mut killed: MessageReader<MonsterKilled>,
    rt: Option<NonSend<StoryRuntime>>,
    data: Res<GameData>,
    mode: Option<Res<crate::session::GameMode>>,
    credit: crate::combat::KillCredit,
    by_script: Query<(), With<ScriptKilled>>,
) {
    // `Player::OnCreatureDeath` @0x1803e17a0: `IncrementKills` runs only for a
    // kill by the player or its pets, their DoTs' included ([`crate::combat::KillCredit`]), never by a leaderless
    // monster: a defense turret or guard killing a wave monster, a wave monster killing an `Allied` guard, nor for
    // a death without a killer (`Game.KillAllMonsters`: [`ScriptKilled`]; the tier's end kills the leftovers and
    // the spawn beacons).
    let kills: Vec<String> = killed
        .read()
        .filter(|k| !by_script.contains(k.entity) && credit.of(k))
        .map(|k| k.record.clone())
        .collect();
    let Some(rt) = rt else { return };
    let mut story = rt.story.borrow_mut();
    if !story.game.survival_mode || kills.is_empty() {
        return;
    }
    let Some(table) = data.db.get("records/game/playerscore.dbr") else {
        return;
    };
    let Some(eq) = table
        .str("survivalScoreEquation")
        .and_then(|e| gd_sim::expr::Expr::parse(e).ok())
    else {
        return;
    };
    let difficulty = mode.map_or(0, |m| m.0.difficulty);
    for rec in kills {
        let class = data
            .db
            .get(&rec)
            .and_then(|r| r.str("monsterClassification").map(str::to_owned))
            .unwrap_or_default();
        // The engine's six-entry table by classification (see the module docs): a SuperBoss reads `bossBonus`.
        let key = match class.as_str() {
            "Champion" => "championBonus",
            "Hero" => "heroBonus",
            "Boss" | "SuperBoss" => "bossBonus",
            "Quest" => "questBonus",
            _ => "commonBonus",
        };
        // The bonus lists are numeric arrays (`1;2;4`), one entry per difficulty.
        let score = table.f32s_iter(key).nth(difficulty).unwrap_or(0.0);
        story.game.survival.add_kill(&eq, score);
    }
}

/// `GD_KILL_ALL` (hooks.rs): the waves of the Crucible and the floors of the Shattered Realm complete without a
/// bot that walks to every monster.
pub fn test_kill_all(
    hooks: Res<crate::hooks::Hooks>,
    time: Res<Time>,
    mut since: Local<f32>,
    mut out: MessageWriter<StoryEffect>,
) {
    let Some(every) = hooks.gameplay.kill_all_secs else {
        return;
    };
    *since += time.delta_secs();
    if *since >= every {
        *since = 0.0;
        out.write(StoryEffect(gd_sim::story::Effect::KillAllMonsters));
    }
}

/// `ResetSurvivalEvent`'s autosave (`GameEngine::AutoSave`).
pub fn autosave(
    mut events: MessageReader<StoryEffect>,
    mut save: MessageWriter<crate::save::SaveRequest>,
) {
    if events
        .read()
        .any(|e| matches!(e.0, gd_sim::story::Effect::SurvivalAutosave))
    {
        info!("survival: autosave");
        save.write(crate::save::SaveRequest);
    }
}

/// `Game.KillAllMonsters`.
#[allow(clippy::too_many_arguments)]
pub fn kill_all_monsters(
    mut commands: Commands,
    mut events: MessageReader<StoryEffect>,
    data: Res<GameData>,
    factions: Option<Res<crate::factions::PlayerFactions>>,
    mut monsters: Query<
        (
            Entity,
            &Transform,
            &mut crate::character::Character,
            &mut Combatant,
            &Monster,
        ),
        Without<Dead>,
    >,
    mut states: ResMut<ProxyStates>,
    mut killed: MessageWriter<MonsterKilled>,
    hooks: Res<crate::hooks::Hooks>,
) {
    if !events
        .read()
        .any(|e| matches!(e.0, Effect::KillAllMonsters))
    {
        return;
    }
    for (e, tf, mut ch, mut cb, m) in &mut monsters {
        if !crate::monsters::is_foe(&data.db, factions.as_deref(), &m.record) {
            continue;
        }
        cb.life = 0.0;
        commands.entity(e).try_insert(ScriptKilled);
        // `GD_OVERKILL` (hooks.rs).
        if hooks.gameplay.overkill {
            commands.entity(e).try_insert(crate::combat::Overkill);
        }
        crate::combat::kill(
            &mut commands,
            e,
            tf.translation,
            Some(&mut ch),
            Some(m),
            false,
            &mut states,
            &mut killed,
            None,
        );
    }
}

/// The Crucible's tokens of `difficulty` in `player.gdc` (block 10: three vectors, one per game difficulty,
/// the character's own, shared by every arena). `Player::GetSurvivalTokens` @0x1803d5b10 (vector at
/// `Player + 24 * (763 + difficulty)`, picked instead of `GetTokens` when `GameInfo::GetMode() == 1`;
/// `BestowToken` / `HasToken` / `ClearTokens` likewise), `SaveNewFormatData` block 10.
pub fn load_tokens(
    g: &gd_formats::save::gdc::Gdc,
    difficulty: usize,
) -> std::collections::BTreeSet<String> {
    g.tokens.lists[difficulty.min(2)].iter().cloned().collect()
}

/// Writes the session's tokens back into the `player.gdc` vector of `difficulty` (see [`load_tokens`]).
/// PORT: the engine's vector keeps grant order and 40-byte `TriggerToken` entries; here the set is stored sorted.
pub fn store_tokens(
    g: &mut gd_formats::save::gdc::Gdc,
    difficulty: usize,
    tokens: &std::collections::BTreeSet<String>,
) {
    g.tokens.lists[difficulty.min(2)] = tokens.iter().cloned().collect();
}

/// The leaderboard file of the save folder (see [`gd_formats::save::survival`]).
pub const LEADERBOARD_FILE: &str = "leaderboard.brd";

/// Adds a run to the leaderboard file under `dir` (read, [`Leaderboard::add`], written back). A file that
/// exists but cannot be read or parsed is left alone: a failed read must not replace it with an empty board.
pub fn record_score(dir: &std::path::Path, name: &str, difficulty: u32, score: u32) {
    use gd_formats::save::survival::Leaderboard;
    let path = dir.join(LEADERBOARD_FILE);
    let mut board = match std::fs::read(&path) {
        Ok(b) => match Leaderboard::parse(&b) {
            Ok(board) => board,
            Err(e) => {
                warn!(
                    "survival: {} is corrupt, not overwritten: {e}",
                    path.display()
                );
                return;
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Leaderboard::default(),
        Err(e) => {
            warn!("survival: could not read {}: {e}", path.display());
            return;
        }
    };
    board.add(name, difficulty, score);
    if let Err(e) = crate::save::write(&path, &board.encode()) {
        warn!("survival: could not write {}: {e}", path.display());
    }
}

/// `Game.SaveSurvivalScore`: the run goes to the leaderboard (the score is locked by the binding).
pub fn save_score(
    mut events: MessageReader<StoryEffect>,
    rt: Option<NonSend<StoryRuntime>>,
    loaded: Option<Res<crate::session::LoadedCharacter>>,
    choice: Res<crate::session::Choice>,
    mode: Option<Res<crate::session::GameMode>>,
    dir: Res<crate::save::SaveDir>,
) {
    if !events
        .read()
        .any(|e| matches!(e.0, Effect::SaveSurvivalScore))
    {
        return;
    }
    let Some(rt) = rt else { return };
    let score = rt.story.borrow().game.survival.score;
    let name = loaded
        .as_ref()
        .map_or_else(|| choice.name.clone(), |l| l.0.header.name.clone());
    record_score(
        &dir.0,
        &name,
        mode.map_or(0, |m| m.0.difficulty as u32),
        score,
    );
}

/// A monster `Game.KillAllMonsters` killed: a death without a killer, which scores nothing ([`score_kills`]).
#[derive(Component)]
pub struct ScriptKilled;

pub struct SurvivalPlugin;

impl Plugin for SurvivalPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                tick_survival,
                respawn_control,
                apply_rules.run_if(resource_exists::<crate::rules::CombatRules>),
                save_score.run_if(resource_exists::<crate::save::SaveDir>),
                autosave.run_if(resource_exists::<crate::save::SaveDir>),
                test_kill_all,
                score_kills.run_if(resource_exists::<GameData>),
                // Before the scoring, so its `ScriptKilled` marks are in.
                kill_all_monsters
                    .run_if(resource_exists::<ProxyStates>)
                    .before(score_kills),
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn record_score_appends_to_the_file() {
        let tmp = crate::testing::TempDir::new("gd-survival");
        let dir = tmp.path();
        super::record_score(dir, "A", 0, 100);
        super::record_score(dir, "B", 1, 300);
        let b = std::fs::read(dir.join(super::LEADERBOARD_FILE)).unwrap();
        let board = gd_formats::save::survival::Leaderboard::parse(&b).unwrap();
        assert_eq!(
            board
                .entries
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            ["B", "A"]
        );
    }
}
