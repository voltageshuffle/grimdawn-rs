//! Faction nemeses: a faction the player is at "nemesis" standing with sends one of its nemesis
//! bosses after enough kills of its monsters.
//!
//! `FactionPack::Adjust` calls `NemesisSpawn` when the player's value for the victim's faction is
//! at or below `factionValue1`; `FactionPack::IncrementNemesisSpawnCounter` then does nothing
//! while its nemesis lives, else adds `gamefactions.dbr`'s `nemesisSpawnKillCount{Common,Hero,Boss}`
//! by classification (unknown counts as Common), × `nemesisSpawnChallengeModifier` while the
//! player is in a challenge area. The threshold is a random int in `nemesisSpawnKillsMin..Max`
//! (`…Elite` / `…Ultimate` per difficulty; min 0 never spawns). At the threshold a random
//! `nemesisSpawn` record spawns at a random nav point 40 m from the player (12 tries, none = no
//! spawn) hating the killer, the counter resets, the spawned one leaves the list unless Ultimate
//! Veteran, and the next threshold is a new roll × `nemesisRespawnKillModifier`.
//! `GameEngine::OnCreatureDeath` clears the living nemesis.
//!
//! Kills by the player's pets count. The counters are not saved: they start over each session.
use crate::combat::{MonsterAi, MonsterKilled};
use bevy::prelude::*;
use gd_formats::collections::HashMap;
use gd_game_core::combat::Combatant;
use gd_game_core::monsters::Monster;
use gd_game_core::player::Player;
use gd_game_core::rules::CombatRules;

pub struct NemesisPlugin;

impl Plugin for NemesisPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Nemeses>()
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

#[derive(Resource, Default)]
pub struct Nemeses(HashMap<String, NemesisState>);

/// A faction pack's kill threshold roll.
fn roll(pack: &gd_formats::arz::Record, difficulty: usize) -> u32 {
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
    gd_game_core::rng::u32(lo..=hi)
}

/// The Ascendant `nemesisSpawnModifier` cuts a threshold (t − t × mod, at least 1, applied last).
fn ascendant_cut(t: u32, nemesis_spawn: f32) -> u32 {
    let t = t as f32;
    ((t - t * nemesis_spawn) as i32).max(1) as u32
}

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
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
    mut cinematic: MessageWriter<gd_game_core::story::CinematicText>,
) {
    #[cfg(feature = "dev")]
    let roll = |pack: &gd_formats::arz::Record, difficulty: usize| {
        hooks
            .gameplay
            .nemesis_kills
            .unwrap_or_else(|| roll(pack, difficulty))
    };
    let Ok((player, ptf, pcb, challenged)) = players.single() else {
        killed.clear();
        return;
    };
    let difficulty = rules.0.balance / 4;
    let nemesis_spawn = session.ascendant().nemesis_spawn;
    let db = assets.data.db.clone();
    // `challenged`: the player is in a challenge area (`Character::InChallengeArea`).
    let gf = db.get("records/game/gamefactions.dbr");
    let nemesis_level = gf
        .as_ref()
        .and_then(|g| g.f32("factionValue1"))
        .unwrap_or(-20000.0);
    let kills: Vec<MonsterKilled> = killed.read().cloned().collect();
    for k in kills {
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
            .and_then(gd_game_core::factions::index_of_name)
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
            // A 0 minimum never rolls (the pack never spawns).
            let t = roll(pack, difficulty);
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
        // At a random point 40 m away (12 tries; none = no spawn this kill).
        let i = gd_game_core::rng::usize(..list.len());
        let record = list[i].clone();
        let mut at = None;
        for _ in 0..12 {
            let a = gd_game_core::rng::f32() * std::f32::consts::TAU;
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
        // `ExperienceLevelControl::ApplyLevel` with `charLevel` = the player level.
        let (lo, hi) = (
            nrec.i32("minLevel").unwrap_or(1).max(0),
            nrec.i32("maxLevel").unwrap_or(1).max(0),
        );
        let eq = nrec
            .str("charLevel")
            .and_then(|s| gd_rules::expr::Expr::parse(s).ok());
        let level = match eq {
            Some(e) => (e.eval(&|n| {
                n.eq_ignore_ascii_case("charLevel")
                    .then_some(pcb.profile.level as f32)
            }) as i32)
                .max(lo)
                .min(hi),
            None => lo,
        };
        // Facing the player: +Z forward, `looking_to` aims −Z.
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
        let radius = gd_game_core::combat::get_radius(Some(nrec), 0.5);
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
        // `GameEngine::SuperBossNotificationInbound`: type 10, pushed to the front.
        cinematic.write(gd_game_core::story::CinematicText {
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
            (roll(pack, difficulty) as f32 * respawn) as u32,
            nemesis_spawn,
        );
    }
}
