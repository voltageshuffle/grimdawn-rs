//! Crucible (survival mode) host: runs [`gd_story::story::state::Survival`]'s timer and applies the story
//! effects of the `Game.*Survival*` bindings.
//!
//! `PlayStats::UpdateBonusTimer` runs every frame (`Player::UpdateSelf`); `Game.KillAllMonsters` kills every
//! monster hostile to the main player.
//! Score (`PlayStats::IncrementKills`, `GetScore`): each kill adds `survivalScoreEquation` of the monster
//! score from `playerscore.dbr`'s `<classification>Bonus` list at the game difficulty (0 past a list's
//! end); `SaveSurvivalScore` locks it. `GameEngine::ResolveMonsterClassificationEnum` (exact match):
//! Common `commonBonus`, Champion `championBonus`, Hero `heroBonus`, Boss `bossBonus`, Quest `questBonus`,
//! SuperBoss `bossBonus` again; anything else is Common.
//! `Game.SaveSurvivalScore` writes the leaderboard ([`save_score`]); the greatest difficulty / wave / score
//! are merged into `player.gdc` on save. `Game.ResetSurvivalEvent` autosaves first ([`autosave`]).
//!
//! A failed event (`events/survivalevent.lua`, `survival/eventcontrol.lua`): during a wave respawn is
//! disabled, so a dead player stays down ([`respawn_control`]) and the event's next update finds
//! `Game.PlayersDead` (softcore only): `eventFailed`, `eventFinished` (end NPC `npc_event_03`, compensation
//! chests, `SaveSurvivalScore`), `ResetSurvivalTimer`, `SurvivalEvent_End` (`EnableRespawn`) and
//! `KillAllMonstersOnNextRespawn`. `npc_event_03` offers "Reset to the beginning" or, from wave 50 on,
//! "Restart at Checkpoint" for 5 / 15 / 30 tributes (none after 3 restarts): `IncrementSurvivalRestarts`,
//! `ResetSurvivalEvent` (score / wave / ramp / mutators reset; restarts stay), teleport to the spawn point,
//! and the wave tier and ramp set to `floor(wave / 10) * 10 - 20`.
//!
//! Defenses (`survival/defenses.lua`): four sites per arena, each a turret or banner
//! (`records/creatures/defenses/*`, `02` / `03` the upgrades, `Character.Create` at `avg + avg / 50 + 1`);
//! only the first build grants `SURVIVALMODE_DEFENSEBUILT`. The engine has no defense code: they are plain
//! `Monster`s of `faction_survivors`, the banners `ControllerStationaryMonster`s.
//!
//! Difficulties: Aspirant / Challenger / Gladiator are game difficulty 0 / 1 / 2 with no challenge; the
//! engine's differences are the ramp pak ([`gd_sim::actor::Rules::with_survival_ramp`]) and the score
//! column; bonus timer, rewards and defense prices are the scripts'.
//! PORT: `FullscreenGlow` is forwarded as [`gd_game_core::story::StoryEffect`] and drawn by the UI.
use crate::combat::MonsterKilled;
use crate::monsters::{ProfileCache, ProxyStates};
use bevy::prelude::*;
use gd_game_core::combat::{Combatant, Dead};
use gd_game_core::data::GameData;
use gd_game_core::monsters::Monster;
use gd_game_core::story::{StoryEffect, StoryRuntime};
use gd_story::story::Effect;

/// Counts the survival bonus timer down.
pub fn tick_survival(rt: Option<NonSend<StoryRuntime>>, time: Res<Time>, mut carry: Local<f32>) {
    let Some(rt) = rt else { return };
    let mut story = rt.story.borrow_mut();
    // The Shattered Realm's bonus timer is the same player timer (`SetEndlessTimerInbound`).
    let crucible = story.game.survival_mode;
    if crucible || story.endless.dungeon.is_some() {
        story
            .game
            .survival
            .tick(gd_game_core::session::frame_ms(&mut carry, &time), crucible);
    }
}

/// The difficulty ramp (`Game.SetSurvivalDifficulty`) and the mutators (`Game.AddMutator`): the rules are
/// rebuilt, the profile cache cleared, and live monsters and the player marked for a profile rebuild
/// (`Monster::OnDifficultyRampLevel`). [`gd_game_core::session::GameMode`] carries the ramp: `Some`
/// throughout a Crucible session.
#[allow(clippy::too_many_arguments)]
pub fn apply_rules(
    rt: Option<NonSend<StoryRuntime>>,
    data: Res<GameData>,
    mut rules: ResMut<gd_game_core::rules::CombatRules>,
    mut profiles: ResMut<ProfileCache>,
    mut monsters: Query<&mut crate::monster_skills::MonsterSkills>,
    players: Query<Entity, With<gd_game_core::player::Player>>,
    mut commands: Commands,
    mut applied: Local<(u32, Vec<String>)>,
    mut new_session: gd_game_core::session::NewSession,
    (time, mut sound, mut cooldown, mut mode): (
        Res<Time>,
        MessageWriter<gd_game_core::visuals::PlaySound>,
        Local<(f32, f32)>,
        ResMut<gd_game_core::session::GameMode>,
    ),
) {
    cooldown.0 -= time.delta_secs();
    cooldown.1 -= time.delta_secs();
    if new_session.begun() {
        *applied = Default::default();
    }
    let Some(rt) = rt else { return };
    let (crucible, want) = {
        let s = rt.story.borrow();
        let crucible = s.game.survival_mode;
        // `Some` for the whole Crucible session, ramp 0 included (`GameEngine::GetChallengeAdjustment` in
        // mode 1 is the survival pak from the start); `None` would give the first spawns the campaign's offsets.
        let ramp = crucible.then_some(s.game.survival.difficulty_ramp);
        if mode.0.ramp != ramp {
            mode.0.ramp = ramp;
        }
        // The Crucible, or a running Shattered Realm dungeon (its mutators).
        let (ramp, mutators): (u32, &[String]) = if crucible || s.endless.dungeon.is_some() {
            (s.game.survival.difficulty_ramp, &s.game.survival.mutators)
        } else {
            (0, &[])
        };
        if (ramp, mutators) == (applied.0, applied.1.as_slice()) {
            return;
        }
        (crucible, (ramp, mutators.to_vec()))
    };
    // `GameEngine::AddMutatorInbound`: a mutator added plays `survivalinfo.dbr`'s `mutatorSound` as a 2D sound
    // (5 s cooldown), and a new `affectsPlayers` mutator `PlayMutatorPlayerFx` (own 5 s cooldown): a one-shot
    // `mutatorFx` attached to the player at its own coords. The record's `boneList` places nothing:
    // `EffectEntity::PullParentBones` has no caller, so the effect emits at the player's origin.
    let added: Vec<&String> = want.1.iter().filter(|m| !applied.1.contains(m)).collect();
    let info = data.db.get("records/game/survivalinfo.dbr");
    if !added.is_empty()
        && cooldown.0 <= 0.0
        && let Some(pak) = info.as_ref().and_then(|r| r.str("mutatorSound"))
    {
        cooldown.0 = 5.0;
        sound.write(gd_game_core::visuals::PlaySound {
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
        let mut spec = gd_game_core::visuals::EffectSpec::record(fx.to_owned());
        spec.despawn_when_complete = true;
        let fx = commands.spawn((spec, Transform::IDENTITY)).id();
        commands.entity(player).add_child(fx);
    }
    info!(
        "survival: difficulty ramp {}, mutators {:?}",
        want.0, want.1
    );
    // The ramp is the Crucible's (else it would drop an Ultimate Veteran's challenge pak); the Shattered
    // Realm only has mutators.
    let base = if crucible {
        rules.0.with_survival_ramp(&data.db, want.0)
    } else {
        (*rules.0).clone()
    };
    rules.0 = std::sync::Arc::new(base.with_mutators(&data.db, &want.1));
    *applied = want;
    profiles.0.clear();
    for mut ms in &mut monsters {
        ms.mark_profile_dirty();
    }
    for p in &players {
        commands
            .entity(p)
            .try_insert(gd_game_core::player::ProfileDirty);
    }
}

/// A player the Crucible keeps dead (`Game.DisableRespawn`).
#[derive(Component)]
pub struct RespawnHeld;

/// `Game.EnableRespawn` / `DisableRespawn` (`GameEngine.respawnEnabled`): while disabled a dead player
/// stays dead ([`RespawnHeld`]), which is how a failed wave ends the event; `EnableRespawn` brings a dead
/// player back at once. A single player gets no "last death" dialog. `KillAllMonstersOnNextRespawn` is read
/// by the respawn activity on arrival (`RespawnLocalPlayerActivity::Update`).
/// PORT: the kill is sent when the respawn is requested (the body moves at once, no fade).
pub fn respawn_control(
    mut commands: Commands,
    rt: Option<NonSend<StoryRuntime>>,
    mut players: Query<
        (Entity, Option<&mut Dead>, Has<RespawnHeld>),
        With<gd_game_core::player::Player>,
    >,
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

/// A kill's score into the survival run.
pub fn score_kills(
    mut killed: MessageReader<MonsterKilled>,
    rt: Option<NonSend<StoryRuntime>>,
    data: Res<GameData>,
    mode: Res<gd_game_core::session::GameMode>,
    credit: crate::combat::KillCredit,
    by_script: Query<(), With<ScriptKilled>>,
) {
    // `Player::OnCreatureDeath`: `IncrementKills` runs only for a kill by the player or its pets (DoTs
    // included), never by a leaderless monster nor for a death without a killer ([`ScriptKilled`]).
    let Some(rt) = rt else {
        killed.clear();
        return;
    };
    let mut story = rt.story.borrow_mut();
    if !story.game.survival_mode {
        killed.clear();
        return;
    }
    let kills: Vec<&MonsterKilled> = killed
        .read()
        .filter(|k| !by_script.contains(k.entity) && credit.of(k))
        .collect();
    if kills.is_empty() {
        return;
    }
    let Some(table) = data.db.get("records/game/playerscore.dbr") else {
        return;
    };
    let Some(eq) = table
        .str("survivalScoreEquation")
        .and_then(|e| gd_rules::expr::Expr::parse(e).ok())
    else {
        return;
    };
    let difficulty = mode.0.difficulty;
    for k in kills {
        let class = data
            .db
            .get(&k.record)
            .and_then(|r| r.str("monsterClassification"))
            .unwrap_or_default();
        // A SuperBoss reads `bossBonus`.
        let key = match class {
            "Champion" => "championBonus",
            "Hero" => "heroBonus",
            "Boss" | "SuperBoss" => "bossBonus",
            "Quest" => "questBonus",
            _ => "commonBonus",
        };
        // Numeric arrays (`1;2;4`), one entry per difficulty.
        let score = table.f32s_iter(key).nth(difficulty).unwrap_or(0.0);
        story.game.survival.add_kill(&eq, score);
    }
}

/// `GD_KILL_ALL`: the waves of the Crucible and the floors of the Shattered Realm complete without a bot.
#[cfg(feature = "dev")]
pub fn test_kill_all(
    hooks: Res<gd_game_core::hooks::Hooks>,
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
        out.write(StoryEffect(gd_story::story::Effect::KillAllMonsters));
    }
}

/// `ResetSurvivalEvent`'s autosave (`GameEngine::AutoSave`).
pub fn autosave(
    mut events: MessageReader<StoryEffect>,
    mut save: MessageWriter<crate::save::SaveRequest>,
) {
    if events
        .read()
        .any(|e| matches!(e.0, gd_story::story::Effect::SurvivalAutosave))
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
            &mut gd_game_core::character::Character,
            &mut Combatant,
            &Monster,
        ),
        Without<Dead>,
    >,
    mut states: ResMut<ProxyStates>,
    mut killed: MessageWriter<MonsterKilled>,
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
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
        #[cfg(feature = "dev")]
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

/// The Crucible's tokens of `difficulty` (`player.gdc` block 10: one vector per game difficulty, shared by
/// every arena). `Player::GetSurvivalTokens`, used instead of `GetTokens` when `GameInfo::GetMode() == 1`.
pub fn load_tokens(
    g: &gd_formats::save::gdc::Gdc,
    difficulty: usize,
) -> std::collections::BTreeSet<String> {
    g.tokens.lists[difficulty.min(2)].iter().cloned().collect()
}

/// Writes the session's tokens back into the `player.gdc` vector of `difficulty`.
/// PORT: the engine's vector keeps grant order and 40-byte `TriggerToken` entries; here the set is stored sorted.
pub fn store_tokens(
    g: &mut gd_formats::save::gdc::Gdc,
    difficulty: usize,
    tokens: &std::collections::BTreeSet<String>,
) {
    g.tokens.lists[difficulty.min(2)] = tokens.iter().cloned().collect();
}

/// The leaderboard file of the save folder ([`gd_formats::save::survival`]).
pub const LEADERBOARD_FILE: &str = "leaderboard.brd";

/// Adds a run to the leaderboard file under `dir`. A file that exists but cannot be read or parsed is left
/// alone.
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

/// `Game.SaveSurvivalScore`: the run goes to the leaderboard.
pub fn save_score(
    mut events: MessageReader<StoryEffect>,
    rt: Option<NonSend<StoryRuntime>>,
    // Option: absent for a new character.
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
    choice: Res<gd_game_core::session::Choice>,
    mode: Res<gd_game_core::session::GameMode>,
    dir: Res<gd_game_core::session::SaveDir>,
) {
    if !events
        .read()
        .any(|e| matches!(e.0, Effect::SaveSurvivalScore))
    {
        return;
    }
    let Some(rt) = rt else { return };
    let score = rt.story.borrow().game.survival.score;
    let name = loaded.as_ref().map_or(&choice.name, |l| &l.0.header.name);
    record_score(&dir.0, name, mode.0.difficulty as u32, score);
}

/// A monster `Game.KillAllMonsters` killed: a death without a killer, which scores nothing.
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
                // After the scripts raise the ramp and create the wave's proxies, before their monsters
                // spawn, so a wave spawns at its own ramp.
                apply_rules
                    .run_if(resource_exists::<gd_game_core::rules::CombatRules>)
                    .after(gd_game_core::story::StoryStage::Tick)
                    .before(crate::story_world::sync_actors),
                save_score.run_if(resource_exists::<gd_game_core::session::SaveDir>),
                autosave.run_if(resource_exists::<gd_game_core::session::SaveDir>),
                score_kills.run_if(resource_exists::<GameData>),
                // Before the scoring, so its `ScriptKilled` marks are in.
                kill_all_monsters
                    .run_if(resource_exists::<ProxyStates>)
                    .before(score_kills),
            ),
        );
        #[cfg(feature = "dev")]
        app.add_systems(Update, test_kill_all);
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
