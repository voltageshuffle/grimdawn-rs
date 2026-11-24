//! The player's level, experience, unspent points and gold
//! (rules in `gd_sim::progress`): created from the save, raised by kills
//! ([`kill_experience`]) and story rewards ([`story_progress`]).
//!
//! [`PointsGained`] mirrors the engine's game events 49 (skill points) and 50 (attribute points), which start the
//! HUD's level-up pulses (`grimdawn` `ui::hud::pulse_hud`). Senders (Game.dll `EventManager::Send` callers):
//! - 49: each level gained whose `skillModifierPoints` entry is > 0 (`ExperienceLevelControl::AddExperience`, the
//!   level-up loop);
//!   `ItemDifficultyUnlock::Use` when the skipped quests' skill points are > 0; `Character::TakeBonus`
//!   for a bonus with `bonusSkillPoints` (no stock record sets it);
//! - 50: `ScriptableAction_GiveAttribPoint::Execute` (every run that applies, any amount);
//!   `ItemDifficultyUnlock::Use` when its attribute points are > 0; `TakeBonus` (`bonusAttributePoints`, none stock).
//! - No event: `ScriptableAction_GiveSkillPoint::Execute` and the Lua `GiveSkillPoints`
//!   (`Character::AddSkillPoints` = `AddToSkillPoints`) only add the points.
use crate::combat::MonsterKilled;
use bevy::prelude::*;
use gd_game_core::combat::Combatant;
use gd_game_core::data::GameData;
use gd_game_core::player::{Player, PlayerSet, ProfileDirty};
use gd_game_core::progress::{LeveledUp, PlayerProgress, PointsGained};
use gd_game_core::rules::CombatRules;
use gd_game_core::session::LoadedCharacter;
use gd_game_core::story::StoryEffect;

pub struct ProgressPlugin;

impl Plugin for ProgressPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<LeveledUp>()
            .add_message::<PointsGained>()
            .add_systems(
                Update,
                (
                    init_progress,
                    kill_experience
                        .run_if(resource_exists::<CombatRules>)
                        .run_if(resource_exists::<PlayerProgress>),
                    story_progress
                        .run_if(resource_exists::<CombatRules>)
                        .run_if(resource_exists::<PlayerProgress>),
                )
                    .chain()
                    .after(PlayerSet)
                    .before(crate::player::rebuild_profile),
            );
    }
}

/// The messages a level-up / point grant sends.
pub(crate) type LevelUpOut<'w> = (
    MessageWriter<'w, LeveledUp>,
    MessageWriter<'w, PointsGained>,
);

pub(crate) type LevelUpQuery<'w, 's> =
    Query<'w, 's, (Entity, &'static mut Combatant), With<Player>>;

/// Creates [`PlayerProgress`] once the player exists.
pub fn init_progress(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut commands: Commands,
    // Option: `progress` is the existence check (this system creates it); `loaded` is absent for a new character.
    progress: Option<Res<PlayerProgress>>,
    loaded: Option<Res<LoadedCharacter>>,
    players: Query<(), With<Player>>,
) {
    if progress.is_some() || players.is_empty() {
        return;
    }
    let mut p = PlayerProgress::from_save(loaded.as_deref());
    // `GD_XP` (hooks.rs).
    if let Some(xp) = hooks.gameplay.xp {
        p.progress.experience = xp;
    }
    commands.insert_resource(p);
}

/// Experience for killed monsters (`gd_sim::progress::experience_reward`
/// at the player's level and difficulty) with the ascendant altar's
/// experience modifier (`PlayerManagerServer::HandleBonus`:
/// `+ round(amount × mod)`). Every gain then takes the character's
/// `characterIncreasedExperience` % (`Character::ReceiveExperience`, see
/// [`xp_bonus`]).
#[allow(clippy::too_many_arguments)]
pub fn kill_experience(
    mut events: MessageReader<MonsterKilled>,
    mut commands: Commands,
    data: Res<GameData>,
    rules: Res<CombatRules>,
    mut progress: ResMut<PlayerProgress>,
    session: crate::session::Session,
    mut players: LevelUpQuery,
    mut leveled: LevelUpOut,
    (spots, mut texts, mut cinematic): (
        Query<(Entity, &Transform), With<gd_game_core::player::Player>>,
        MessageWriter<crate::floating::FloatingText>,
        MessageWriter<gd_game_core::story::CinematicText>,
    ),
    (rt, monsters, credit, attackers): (
        Option<NonSend<gd_game_core::story::StoryRuntime>>,
        Query<&gd_game_core::monsters::Monster>,
        crate::combat::KillCredit,
        Query<&gd_game_core::combat::Attackers>,
    ),
) {
    for ev in events.read() {
        // Experience only for the player's or his pets' kill, his DoTs' included ([`crate::combat::KillCredit`]:
        // nothing for a leaderless defense turret or guard, a trap, a hostile monster killing an `Allied` guard),
        // and not for a monster of the Shattered Realm's dungeon (`PlayerManagerServer::HandleBonus`:
        // the monster's own XP is zeroed by its in-dungeon flag).
        // The reward × the player's damage share, truncated (`HandleBonus`: `(int64)(xp × GetExperienceMultiplier)`,
        // [`gd_game_core::combat::Attackers`]: his and his pets' hits and DoTs).
        let by_ally = !credit.of(ev);
        let share = spots
            .single()
            .ok()
            .and_then(|(p, _)| attackers.get(ev.entity).ok().map(|a| a.share(p)))
            .unwrap_or(0.0);
        let in_dungeon = rt.as_ref().is_some_and(|rt| {
            let story = rt.story.borrow();
            monsters.get(ev.entity).is_ok_and(|m| {
                story
                    .endless
                    .activated
                    .iter()
                    .any(|(_, k)| *k == (m.slot.0, m.slot.1))
            })
        });
        let xp = gd_sim::progress::experience_reward(
            &data.db,
            &ev.record,
            ev.level,
            progress.progress.level,
            session.mode(),
        );
        let xp = (xp as f32 * share) as i64 as u32;
        // Then + round(xp × `GameEngine.ascendantExperienceMod` = `GetAscendantExperienceMod`: `HandleBonus`
        // reads it there; 0 in the Shattered Realm's dungeon and treasure rooms).
        let xp = gd_sim::ascendant::apply_bonus(xp as u64, session.ascendant().experience) as _;
        let xp = if by_ally || in_dungeon { 0 } else { xp };
        if xp > 0 {
            info!("{}: {xp} xp", ev.record.rsplit('/').next().unwrap_or(""));
        }
        // A hero / boss kill's "XP Awarded" text (`HandleExperienceNotification`).
        if let Ok((p, tf)) = spots.single()
            && let Some(t) = crate::floating::FloatingText::experience(
                &data.db,
                p,
                tf.translation,
                &ev.record,
                xp as u64,
            )
        {
            texts.write(t);
            // Its cinematic text (type 5): "Enemy Hero Killed" / "Enemy Boss Killed" over "N XP Awarded".
            let hero = data
                .db
                .get(&ev.record)
                .is_some_and(|r| r.str("monsterClassification") == Some("Hero"));
            cinematic.write(gd_game_core::story::CinematicText {
                head: if hero {
                    "tagHeroKilled"
                } else {
                    "tagBossKilled"
                }
                .into(),
                sub: "tagXPAwarded".into(),
                sub_args: vec![gd_game_core::floating::TextArg::Num(xp as f32)],
                kind: 5,
                front: false,
            });
        }
        let gained = progress
            .progress
            .add_experience(&rules.0.levels, xp, xp_bonus(&players));
        if gained > 0 {
            level_up(
                gained,
                &progress,
                &rules,
                &mut players,
                &mut leveled,
                &mut commands,
            );
        }
    }
}

/// `Character::ReceiveExperience`: every experience
/// gain + (int)(xp × `CharAttributeAccumulator::GetValue(55)` % + 0.5), the
/// sheet's Experience Gained row (`CharacterSheet::experience_bonus`).
fn xp_bonus(players: &LevelUpQuery) -> f32 {
    players
        .iter()
        .next()
        .map_or(0.0, |p| p.1.profile.sheet.experience_bonus)
}

/// Story rewards that change progress (`ScriptableAction_Give*`, Lua
/// `Player:Give*`): experience, levels, money (`GiveMoney`: negative
/// amounts take money, never below 0) and points. Items are the game's.
/// Experience: `ScriptableAction_GiveExperience::Execute` ->
/// `GameEngine::CharacterExperienceOutbound` (a `Bonus` of source 2, no monster) straight to the
/// client (`HandleBonusToClient` -> `PlayerManagerClient::HandleBonus` -> `Character::TakeBonus`, which takes a
/// source-2 gain even while dead) -> `ReceiveExperience`: the character's bonus % ([`xp_bonus`]); no damage
/// share and no ascendant modifier (those are the server's `HandleBonus`, kills only).
pub fn story_progress(
    mut events: MessageReader<StoryEffect>,
    mut commands: Commands,
    rules: Res<CombatRules>,
    mut progress: ResMut<PlayerProgress>,
    mut players: LevelUpQuery,
    mut leveled: LevelUpOut,
) {
    use gd_story::story::Effect;
    for ev in events.read() {
        match &ev.0 {
            Effect::GiveExperience(xp) => {
                let gained =
                    progress
                        .progress
                        .add_experience(&rules.0.levels, *xp, xp_bonus(&players));
                if gained > 0 {
                    level_up(
                        gained,
                        &progress,
                        &rules,
                        &mut players,
                        &mut leveled,
                        &mut commands,
                    );
                }
            }
            Effect::GiveLevels(n) => {
                let gained = progress.progress.give_levels(&rules.0.levels, *n);
                if gained > 0 {
                    level_up(
                        gained,
                        &progress,
                        &rules,
                        &mut players,
                        &mut leveled,
                        &mut commands,
                    );
                }
            }
            Effect::GiveMoney(n) => crate::loot::add_gold(&mut progress.gold, *n),
            // No event 49 (quest action, Lua; the difficulty unlock sends its own, `consumables`).
            Effect::GiveSkillPoints(n) => progress.progress.skill_points += *n as i32,
            // Event 50: the quest action and the difficulty unlock both send it.
            Effect::GiveAttributePoints(n) => {
                progress.progress.attribute_points += *n as i32;
                leveled.1.write(PointsGained {
                    attribute: true,
                    ..default()
                });
            }
            _ => {}
        }
    }
}

/// After `gained` levels were gained: logs, refills life and marks the player [`ProfileDirty`] so
/// `player::rebuild_profile` rebuilds it at the new level (with the challenge area's mutators; life keeps its
/// fraction, so the refill stays full), and sends event 49 when one of the levels gave skill points
/// (`ExperienceLevelControl::AddExperience`: the `skillModifierPoints` entry min(level − 2, last), > 0).
/// `Character::ReceiveExperience` sets the `CharacterBio` reload flag, on which
/// `CharacterBio::Update` sets current life and energy to their maximums (energy:
/// `loot::refill_energy` on [`LeveledUp`]).
pub(crate) fn level_up(
    gained: i32,
    progress: &PlayerProgress,
    rules: &CombatRules,
    players: &mut LevelUpQuery,
    out: &mut LevelUpOut,
    commands: &mut Commands,
) {
    let level = progress.progress.level;
    info!(
        "level up: {level} ({} attribute, {} skill points unspent)",
        progress.progress.attribute_points, progress.progress.skill_points
    );
    let table = &rules.0.levels.skill_points;
    let skill = !table.is_empty()
        && (level - gained + 1..=level)
            .any(|l| table[((l - 2).max(0) as usize).min(table.len() - 1)] > 0);
    if let Ok((player, mut cb)) = players.single_mut() {
        // Refill life now; the rebuild keeps its fraction, so full stays full at the new level.
        cb.life = cb.profile.sheet.life;
        // Rebuild through ProfileDirty (not inline) so a challenge area's mutators apply
        // (`player::rebuild_profile`); energy refills on the LeveledUp message (`loot::refill_energy`).
        commands.entity(player).insert(ProfileDirty);
        out.0.write(LeveledUp {
            player,
            level,
            gained: gained as u32,
        });
        if skill {
            out.1.write(PointsGained {
                skill: true,
                ..default()
            });
        }
    }
}
