//! Faction nemeses: a faction the player is at "nemesis" standing with
//! sends one of its nemesis bosses after enough kills of its monsters.
//!
//! Game.dll: `FactionPack::Adjust` @0x1802a3620 (the player's
//! pack, after a kill's reputation change) calls `NemesisSpawn` when the
//! player's value for the victim's faction is at or below the "nemesis"
//! level; `FactionPack::IncrementNemesisSpawnCounter` @0x1802a5630 (the
//! victim faction's pack) then does nothing while its nemesis lives, else
//! adds `GameEngine::GetNemesisKillCount` (`gamefactions.dbr`
//! `nemesisSpawnKillCountCommon` for Common/Champion, `…Hero`, `…Boss` for
//! Boss/Quest/…, × `nemesisSpawnChallengeModifier` with the flag below);
//! the threshold is a random int in the faction's
//! `nemesisSpawnKillsMin..Max` (`…Elite` / `…Ultimate` per difficulty); at
//! the threshold a random `nemesisSpawn` record spawns near the player
//! (`FindSuperBossSpawnPoint`) hating the killer, the counter resets, the
//! spawned one leaves the list unless Ultimate Veteran, and the next
//! threshold is a new roll × `nemesisRespawnKillModifier`;
//! `GameEngine::OnCreatureDeath` clears the living nemesis (`ResetNemesis`).
//!
//! The "nemesis" level is `factionValue1` (the first rank,
//! `GameEngine::GetFactionLevelValue` / `LoadFromDatabase`); the kill-count
//! multiplier applies while the player stands in a challenge area
//! (`Player` challenge-area pointer, set in `Player::UpdateSelf`:
//! [`crate::challenge_areas::InChallengeArea`]); the kill count goes by classification
//! (`ResolveMonsterClassificationEnum`: an unknown string counts as Common);
//! a pack with `nemesisSpawnKillsMin` 0 never rolls a threshold, so never
//! spawns; the spawn point is a random nav point 40 m from the player with
//! 2.5 m clearance, 12 tries, none = no spawn
//! (`FindSuperBossSpawnPoint` → `NavManager::RequestAndBlockRandomPointOnRadius`).
//!
//! Kills by the player's pets count (`GameEngine::OnCreatureDeath`
//! swaps the killer for its controller's master before `HandleFaction`);
//! the nemesis level comes from its record like any monster's
//! (`ExperienceLevelControl`, `sub_18028ED10`: the `charLevel` equation of
//! the player level, clamped to `minLevel..maxLevel`); the kill counters
//! live in the factions' `FactionPack` (`nemesisSpawnCounter`, threshold),
//! which `FactionPack::WriteProperties` does not save — they start over each
//! session. Hooks `GD_FACTION`, `GD_NEMESIS_KILLS` (hooks.rs).
use crate::combat::{CombatRules, Combatant, MonsterAi, MonsterKilled};
use crate::monsters::Monster;
use crate::player::Player;
use crate::session::MessageDrain;
use bevy::prelude::*;
use std::collections::HashMap;

pub struct NemesisPlugin;

impl Plugin for NemesisPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Nemeses>()
            .drain_messages_unless::<MonsterKilled, _>(has_nemesis_inputs)
            .add_systems(Update, nemesis_kills.run_if(has_nemesis_inputs));
    }
}

#[derive(Default, Debug)]
struct NemesisState {
    counter: u32,
    threshold: u32,
    list: Option<Vec<String>>,
    alive: Option<Entity>,
}

/// Per faction pack record (lower-case).
#[derive(Resource, Default)]
pub struct Nemeses(HashMap<String, NemesisState>);

/// `GD_NEMESIS_KILLS` (hooks.rs) overrides the kill threshold.
fn roll(pack: &gd_formats::arz::Record, difficulty: usize, hooks: &crate::hooks::Hooks) -> u32 {
    if let Some(n) = hooks.gameplay.nemesis_kills {
        return n;
    }
    let sfx = ["", "Elite", "Ultimate"][difficulty.min(2)];
    let lo = pack
        .i32(&format!("nemesisSpawnKillsMin{sfx}"))
        .unwrap_or(0)
        .max(0) as u32;
    let hi = (pack
        .i32(&format!("nemesisSpawnKillsMax{sfx}"))
        .unwrap_or(0)
        .max(0) as u32)
        .max(lo);
    if lo == 0 {
        return 0;
    }
    crate::rng::u32(lo..=hi)
}

/// The Ascendant `nemesisSpawnModifier` cuts a threshold (t − t × mod, at
/// least 1, `IncrementNemesisSpawnCounter`, applied last).
fn ascendant_cut(t: u32, nemesis_spawn: f32) -> u32 {
    let t = t as f32;
    ((t - t * nemesis_spawn) as i32).max(1) as u32
}

/// What [`nemesis_kills`] needs besides the world: the player's factions and the combat rules.
fn has_nemesis_inputs(
    factions: Option<Res<crate::factions::PlayerFactions>>,
    rules: Option<Res<CombatRules>>,
) -> bool {
    factions.is_some() && rules.is_some()
}

#[allow(clippy::too_many_arguments)]
fn nemesis_kills(
    mut commands: Commands,
    mut killed: MessageReader<MonsterKilled>,
    mut state: ResMut<Nemeses>,
    factions: Res<crate::factions::PlayerFactions>,
    rules: Res<CombatRules>,
    nav: Res<crate::nav::Nav>,
    mut assets: crate::creature::Spawner,
    players: Query<
        (
            Entity,
            &Transform,
            &Combatant,
            Has<crate::challenge_areas::InChallengeArea>,
        ),
        With<Player>,
    >,
    credit: crate::combat::KillCredit,
    session: crate::session::Session,
    hooks: Res<crate::hooks::Hooks>,
    mut cinematic: MessageWriter<crate::story::CinematicText>,
) {
    let Ok((player, ptf, pcb, challenged)) = players.single() else {
        killed.clear();
        return;
    };
    let difficulty = rules.0.balance / 4;
    let nemesis_spawn = session.ascendant().nemesis_spawn;
    // The database by its own handle: its records stay borrowed while `assets` spawns.
    let db = assets.data.db.clone();
    // `challenged`: the player is in a challenge area (`Character::InChallengeArea`, crate::challenge_areas).
    let gf = db.get("records/game/gamefactions.dbr");
    let nemesis_level = gf
        .as_ref()
        .and_then(|g| g.f32("factionValue1"))
        .unwrap_or(-20000.0);
    let kills: Vec<MonsterKilled> = killed.read().cloned().collect();
    for k in kills {
        // The living nemesis died.
        for s in state.0.values_mut() {
            if s.alive == Some(k.entity) {
                s.alive = None;
            }
        }
        if !credit.of(&k) {
            continue;
        }
        let Some(rec) = db.get(&k.record) else {
            continue;
        };
        let Some(pack_name) = rec.str("factions").map(str::to_owned) else {
            continue;
        };
        let Some(pack) = db.get(&pack_name) else {
            continue;
        };
        let Some(me) = pack
            .str("myFaction")
            .and_then(crate::factions::index_of_name)
        else {
            continue;
        };
        if factions.value(me) > nemesis_level {
            continue;
        }
        let list = pack
            .strs("nemesisSpawn")
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        if list.is_empty() {
            continue;
        }
        let s = state.0.entry(pack_name.to_ascii_lowercase()).or_default();
        if s.alive.is_some() {
            continue;
        }
        let list = s.list.get_or_insert(list);
        if list.is_empty() {
            continue;
        }
        let class = rec.str("monsterClassification").unwrap_or("Common");
        let key = match class {
            "Hero" => "nemesisSpawnKillCountHero",
            "Boss" | "Quest" | "SuperBoss" => "nemesisSpawnKillCountBoss",
            _ => "nemesisSpawnKillCountCommon",
        };
        let mut n = gf.as_ref().and_then(|g| g.i32(key)).unwrap_or(1).max(0) as u32;
        if challenged {
            n = (n as f32
                * gf.as_ref()
                    .and_then(|g| g.f32("nemesisSpawnChallengeModifier"))
                    .unwrap_or(1.0)) as u32;
        }
        s.counter += n;
        if s.threshold == 0 {
            // No roll at all with a 0 minimum (the pack never spawns).
            let t = roll(pack, difficulty, &hooks);
            s.threshold = if t == 0 {
                0
            } else {
                ascendant_cut(t, nemesis_spawn)
            };
        }
        debug!(
            "nemesis {}: {}/{}",
            pack_name.rsplit('/').next().unwrap_or(""),
            s.counter,
            s.threshold
        );
        if s.threshold == 0 || s.counter < s.threshold {
            continue;
        }
        // Spawn one of the faction's nemeses at a random point 40 m away
        // (12 tries; none = no spawn this kill).
        let i = crate::rng::usize(..list.len());
        let record = list[i].clone();
        let mut at = None;
        for _ in 0..12 {
            let a = crate::rng::f32() * std::f32::consts::TAU;
            let p = ptf.translation + Vec3::new(a.cos(), 0.0, a.sin()) * 40.0;
            if let Some(g) = nav.0.ground((p + Vec3::Y * 2.0).to_array()) {
                at = Some(Vec3::new(p.x, g, p.z));
                break;
            }
        }
        let Some(at) = at else { continue };
        let Some(nrec) = db.get(&record) else {
            continue;
        };
        // `ExperienceLevelControl` (`sub_18028ED10`): the record's `charLevel`
        // equation with `charLevel` = the player level, truncated, clamped
        // to `minLevel..maxLevel` (both default 1); no equation = `minLevel`.
        let (lo, hi) = (
            nrec.i32("minLevel").unwrap_or(1).max(0),
            nrec.i32("maxLevel").unwrap_or(1).max(0),
        );
        let eq = nrec
            .str("charLevel")
            .and_then(|s| gd_sim::expr::Expr::parse(s).ok());
        let level = match eq {
            Some(e) => (e.eval(&|n| {
                n.eq_ignore_ascii_case("charLevel")
                    .then_some(pcb.profile.level as f32)
            }) as i32)
                .max(lo)
                .min(hi),
            None => lo,
        };
        // Facing the player: +Z forward (`character::facing`), `looking_to` aims −Z.
        let tf = Transform::from_translation(at).looking_to(
            at - Vec3::new(ptf.translation.x, at.y, ptf.translation.z),
            Vec3::Y,
        );
        let Some(e) =
            crate::creature::spawn(&mut commands, &mut assets, tf, &record, 9000 + i as u32)
        else {
            continue;
        };
        let Some(prof) = gd_sim::actor::monster(
            &assets.data.db,
            &rules.0,
            &record,
            level,
            gd_sim::actor::Side::Monster,
        ) else {
            continue;
        };
        let radius = crate::combat::get_radius(Some(nrec), 0.5);
        let mut ai = MonsterAi::from_record(&assets.data, &record, at);
        ai.aggro = true;
        ai.add_anger(player, 1000.0);
        // `IncrementNemesisSpawnCounter`: `EnableSpawnAnimation` before `FastSpawnEntity`.
        ai.enable_spawn_animation();
        commands.entity(e).insert((
            Monster {
                record: record.clone(),
                level,
                slot: (usize::MAX, 0, 0),
            },
            Combatant::new(std::sync::Arc::new(prof), radius),
            ai,
        ));
        info!(
            "nemesis {} ({}) spawned at level {level} after {} kills",
            record.rsplit('/').next().unwrap_or(""),
            nrec.str("description").unwrap_or(""),
            s.counter
        );
        // `GameEngine::SuperBossNotificationInbound` @0x1802f4ea0: type 10,
        // pushed to the front of the queue.
        cinematic.write(crate::story::CinematicText {
            head: "tagFactionNemesisSpawned".into(),
            sub: String::new(),
            sub_args: Vec::new(),
            kind: 10,
            front: true,
        });
        s.alive = Some(e);
        s.counter = 0;
        if !session.mode().ultimate_veteran() {
            list.swap_remove(i);
        }
        let respawn = gf
            .as_ref()
            .and_then(|g| g.f32("nemesisRespawnKillModifier"))
            .unwrap_or(1.0);
        s.threshold = ascendant_cut(
            (roll(pack, difficulty, &hooks) as f32 * respawn) as u32,
            nemesis_spawn,
        );
    }
}
