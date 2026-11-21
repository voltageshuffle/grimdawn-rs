//! This session's play stats: [`PlayCounts`], added to the loaded save's `PlayStats` (block 16) when the game
//! saves ([`PlayCounts::apply`]), when the character window shows them and when the `Stat` achievements read
//! them (Game.dll `PlayStats`).
//! - Play time: whole seconds of the virtual clock (a paused game adds nothing).
//! - Kills (the player's or his pet's, [`crate::combat::KillCredit`]): every kill; Hero..SuperBoss as champion
//!   kills, Hero also a hero kill; the monster's experience reward at its level (no damage share, no ascendant
//!   bonus); the dead monster's attacks taken as `hits_inflicted`. With `giveXP`, outside the Crucible: Boss /
//!   SuperBoss add a boss kill of the difficulty, Boss / Quest / SuperBoss in Ultimate Veteran an
//!   `ultimate_veteran_boss_kills`, and the difficulty's greatest monster ((life + mana) × level) is kept.
//! - Every attack a character takes (a miss too) counts: the player's `hits_received`, a monster's at its death.
//! - A player crit adds `crits_inflicted`; `crits_received` only counts other players' crits (none here).
//! - Greatest hits: the player's largest attack total and the largest single life subtraction taken.
//! - Last foes (attacks not fumbled, dodged or deflected): the player's target's tag and DA, the player's
//!   attacker's tag and OA; pets record nothing.
//! - One `max_level` per level gained; potions, crafts, one-shot / hidden chests, shrines restored and
//!   Shattered Realm souls / essence come as [`PlayStat`] messages. The essence is a maximum.
use crate::combat::{AttackMade, Combatant, MonsterKilled};
use crate::data::GameData;
use crate::player::Player;
use bevy::prelude::*;
use gd_formats::save::gdc::PlayStats;

pub struct PlayStatsPlugin;

impl Plugin for PlayStatsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PlayStat>()
            .init_resource::<PlayCounts>()
            .add_systems(Update, count_play_time)
            .add_systems(
                Update,
                (count_attacks, count_kills, count_levels, count_events)
                    .chain()
                    .after(crate::combat::CombatSet)
                    .after(crate::player::PlayerSet),
            );
    }
}

/// A counted event another module reports (the module doc's callers).
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub enum PlayStat {
    /// The crafting command made items; the first one's record.
    Crafted(String),
    /// The player opened the container of this record.
    ChestOpened(String),
    /// The player used a one-shot of this class.
    PotionUsed(String),
    /// The player restored a devotion shrine.
    ShrineRestored,
    /// A charged buff (`buff` record) landed on the player at `level` (its charge).
    Essence { buff: String, level: u32 },
}

/// The greatest monster killed on a difficulty (`Set/GetGreatestMonsterKilled*`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Greatest {
    /// Its description tag.
    pub name: String,
    pub level: u32,
    pub life_and_mana: u32,
}

/// This session's counts (module doc), added to the loaded stats by [`PlayCounts::apply`].
#[derive(Resource, Default, Debug)]
pub struct PlayCounts {
    pub deaths: u32,
    pub kills: u32,
    pub champion_kills: u32,
    pub hero_kills: u32,
    /// Whole seconds played.
    pub play_seconds: u32,
    /// Milliseconds not yet a whole second.
    play_ms: f32,
    pub greatest_damage_inflicted: f32,
    pub greatest_damage_received: f32,
    /// Lore notes read.
    pub lore_notes: u32,
    /// The last monster hit / that hit: (difficulty, description tag, its DA resp. the attack's OA).
    pub last_hit: Option<(usize, String, f32)>,
    pub last_hit_by: Option<(usize, String, f32)>,
    pub boss_kills: [u32; 3],
    pub ultimate_veteran_boss_kills: u32,
    pub experience_from_kills: u32,
    pub hits_inflicted: u32,
    pub hits_received: u32,
    pub crits_inflicted: u32,
    /// Levels gained (`max_level`).
    pub levels: u32,
    pub health_potions_used: u32,
    pub mana_potions_used: u32,
    pub items_crafted: u32,
    pub relics_crafted: u32,
    pub transcendent_relics_crafted: u32,
    pub mythical_relics_crafted: u32,
    pub shrines_restored: u32,
    pub one_shot_chests_opened: u32,
    pub hidden_chests_opened: u32,
    pub endless_souls: u32,
    /// The greatest essence charge reached this session.
    pub endless_essence: u32,
    /// Per difficulty: this session's greatest monster, when one beat the stored one.
    pub greatest: [Option<Greatest>; 3],
}

impl PlayCounts {
    /// The character's play stats now: the loaded `stats` plus this session's counts (what a save writes, and
    /// what the character sheet shows).
    pub fn apply(&self, stats: &mut PlayStats) {
        let add = |a: &mut u32, b: u32| *a = a.wrapping_add(b);
        add(&mut stats.deaths, self.deaths);
        add(&mut stats.kills, self.kills);
        add(&mut stats.champion_kills, self.champion_kills);
        add(&mut stats.hero_kills, self.hero_kills);
        add(&mut stats.lore_notes_collected, self.lore_notes);
        add(&mut stats.play_time, self.play_seconds);
        stats.greatest_damage_inflicted = stats
            .greatest_damage_inflicted
            .max(self.greatest_damage_inflicted);
        stats.greatest_damage_received = stats
            .greatest_damage_received
            .max(self.greatest_damage_received);
        if let Some((d, name, v)) = &self.last_hit
            && let Some(p) = stats.per_difficulty.get_mut(*d)
        {
            p.last_monster_hit = name.clone();
            stats.last_hit = *v;
        }
        if let Some((d, name, v)) = &self.last_hit_by
            && let Some(p) = stats.per_difficulty.get_mut(*d)
        {
            p.last_monster_hit_by = name.clone();
            stats.last_hit_by = *v;
        }
        for (a, b) in stats.boss_kills.iter_mut().zip(self.boss_kills) {
            add(a, b);
        }
        add(
            &mut stats.ultimate_veteran_boss_kills,
            self.ultimate_veteran_boss_kills,
        );
        add(&mut stats.experience_from_kills, self.experience_from_kills);
        add(&mut stats.hits_inflicted, self.hits_inflicted);
        add(&mut stats.hits_received, self.hits_received);
        add(&mut stats.crits_inflicted, self.crits_inflicted);
        add(&mut stats.max_level, self.levels);
        add(&mut stats.health_potions_used, self.health_potions_used);
        add(&mut stats.mana_potions_used, self.mana_potions_used);
        add(&mut stats.items_crafted, self.items_crafted);
        add(&mut stats.relics_crafted, self.relics_crafted);
        add(
            &mut stats.transcendent_relics_crafted,
            self.transcendent_relics_crafted,
        );
        add(
            &mut stats.mythical_relics_crafted,
            self.mythical_relics_crafted,
        );
        add(&mut stats.shrines_restored, self.shrines_restored);
        add(
            &mut stats.one_shot_chests_opened,
            self.one_shot_chests_opened,
        );
        add(&mut stats.hidden_chests_opened, self.hidden_chests_opened);
        add(&mut stats.endless_souls, self.endless_souls);
        stats.endless_essence = stats.endless_essence.max(self.endless_essence);
        for (d, g) in self.greatest.iter().enumerate() {
            if let Some(g) = g
                && let Some(p) = stats.per_difficulty.get_mut(d)
            {
                p.greatest_monster_killed_name = g.name.clone();
                p.greatest_monster_killed_level = g.level;
                p.greatest_monster_killed_life_and_mana = g.life_and_mana;
            }
        }
    }

    /// The greatest monster of `difficulty`: this session's, else the loaded stats'.
    pub fn greatest(&self, loaded: Option<&PlayStats>, difficulty: usize) -> Greatest {
        self.greatest
            .get(difficulty)
            .cloned()
            .flatten()
            .or_else(|| {
                let p = loaded?.per_difficulty.get(difficulty)?;
                Some(Greatest {
                    name: p.greatest_monster_killed_name.clone(),
                    level: p.greatest_monster_killed_level,
                    life_and_mana: p.greatest_monster_killed_life_and_mana,
                })
            })
            .unwrap_or_default()
    }

    /// The character window's last foes of `difficulty`: this session's, else the loaded stats' (as
    /// [`PlayCounts::apply`] leaves them; the value is kept once for all difficulties).
    pub fn last_foes(
        &self,
        loaded: Option<&PlayStats>,
        difficulty: usize,
    ) -> gd_sim::sheet::LastFoes {
        let saved = |name: fn(&gd_formats::save::gdc::PerDifficultyStats) -> &String,
                     v: fn(&PlayStats) -> f32| {
            loaded
                .map(|ps| {
                    (
                        ps.per_difficulty
                            .get(difficulty)
                            .map(|d| name(d).clone())
                            .unwrap_or_default(),
                        v(ps),
                    )
                })
                .unwrap_or_default()
        };
        let pick = |own: &Option<(usize, String, f32)>, saved: (String, f32)| match own {
            Some((d, n, v)) if *d == difficulty => (n.clone(), *v),
            Some((_, _, v)) => (saved.0, *v),
            None => saved,
        };
        gd_sim::sheet::LastFoes {
            hit: pick(
                &self.last_hit,
                saved(|d| &d.last_monster_hit, |p| p.last_hit),
            ),
            hit_by: pick(
                &self.last_hit_by,
                saved(|d| &d.last_monster_hit_by, |p| p.last_hit_by),
            ),
        }
    }
}

/// A monster's count of attacks taken (`Character::TakeAttack`'s `PlayStats` +104), inserted on its first hit and
/// read when it dies.
#[derive(Component, Default)]
pub struct AttacksTaken(u32);

/// `Monster::GiveExperience`: the record's `giveXP` (missing: true).
fn gives_experience(rec: Option<&gd_formats::arz::Record>) -> bool {
    rec.and_then(|r| r.f32("giveXP")).is_none_or(|v| v != 0.0)
}

/// `PlayStats::Update`: the living or dead player's time.
fn count_play_time(
    time: Res<Time>,
    mut counts: ResMut<PlayCounts>,
    players: Query<(), With<Player>>,
) {
    if players.is_empty() {
        return;
    }
    counts.play_ms += time.delta_secs() * 1000.0;
    let whole = (counts.play_ms * 0.001).floor();
    if whole >= 1.0 {
        counts.play_seconds += whole as u32;
        counts.play_ms -= whole * 1000.0;
    }
}

/// The attacks' counters: `Character::TakeAttack` / `CombatManager::TakeAttack` (hits, crits, `RecordHit`'s last
/// foes, `TallyDamageInflicted`) and `TallyDamageReceived` from the life subtractions (module doc).
#[allow(clippy::too_many_arguments)]
fn count_attacks(
    mut made: MessageReader<AttackMade>,
    mut lifes: MessageReader<crate::combat::LifeSubtracted>,
    mut counts: ResMut<PlayCounts>,
    mut commands: Commands,
    mut taken: Query<&mut AttacksTaken>,
    players: Query<(), With<Player>>,
    monsters: Query<&crate::monsters::Monster>,
    (data, mode): (Res<GameData>, Res<crate::session::GameMode>),
) {
    use gd_sim::combat::MissReason as M;
    let difficulty = mode.0.difficulty;
    // Keeps the stored tag when it is unchanged: no allocation per hit on the same foe.
    let set_tag = |slot: &mut Option<(usize, String, f32)>, rec: &str, v: f32| {
        let r = data.db.get(rec);
        let tag = r.as_ref().and_then(|r| r.str("description")).unwrap_or("");
        match slot {
            Some((d, name, val)) if name.as_str() == tag => {
                *d = difficulty;
                *val = v;
            }
            _ => *slot = Some((difficulty, tag.to_owned(), v)),
        }
    };
    for m in made.read() {
        let by_player = players.contains(m.attacker);
        if players.contains(m.target) {
            counts.hits_received += 1;
        } else if monsters.contains(m.target) {
            match taken.get_mut(m.target) {
                Ok(mut t) => t.0 += 1,
                // Several hits this frame before the insert applies all count.
                Err(_) => {
                    commands
                        .entity(m.target)
                        .entry::<AttacksTaken>()
                        .and_modify(|mut t| t.0 += 1)
                        .or_insert(AttacksTaken(1));
                }
            }
        }
        if by_player && m.miss.is_none() {
            counts.greatest_damage_inflicted = counts.greatest_damage_inflicted.max(m.total);
            if m.critical {
                counts.crits_inflicted += 1;
            }
        }
        if matches!(m.miss, Some(M::Fumble | M::Dodge | M::Deflect)) {
            continue;
        }
        if by_player && let Ok(mon) = monsters.get(m.target) {
            set_tag(&mut counts.last_hit, &mon.record, m.defensive_ability);
        } else if players.contains(m.target)
            && let Ok(mon) = monsters.get(m.attacker)
        {
            set_tag(&mut counts.last_hit_by, &mon.record, m.offensive_ability);
        }
    }
    for l in lifes.read() {
        if players.contains(l.target) {
            counts.greatest_damage_received = counts.greatest_damage_received.max(l.amount);
        }
    }
}

/// `IncrementKills` and the greatest monster (module doc).
#[allow(clippy::too_many_arguments)]
fn count_kills(
    mut killed: MessageReader<MonsterKilled>,
    mut counts: ResMut<PlayCounts>,
    taken: Query<&AttacksTaken>,
    credit: crate::combat::KillCredit,
    data: Res<GameData>,
    session: crate::session::Session,
    choice: Res<crate::session::Choice>,
    // Option: absent before the player exists / for a new character.
    (progress, loaded): (
        Option<Res<crate::progress::PlayerProgress>>,
        Option<Res<crate::session::LoadedCharacter>>,
    ),
    bodies: Query<&Combatant>,
) {
    let mode = session.mode();
    let difficulty = mode.difficulty.min(2);
    let crucible = choice.survival.is_some();
    for k in killed.read() {
        let attacks = taken.get(k.entity).map_or(0, |t| t.0);
        if !credit.of(k) {
            continue;
        }
        let rec = data.db.get(&k.record);
        counts.kills += 1;
        let class = match rec.as_ref().and_then(|r| r.str("monsterClassification")) {
            Some("Hero") => 2,
            Some("Boss") => 3,
            Some("Quest") => 4,
            Some("SuperBoss") => 5,
            _ => 0,
        };
        if class >= 2 {
            counts.champion_kills += 1;
        }
        if class == 2 {
            counts.hero_kills += 1;
        }
        let player_level = progress.as_ref().map_or(1, |p| p.progress.level);
        counts.experience_from_kills =
            counts
                .experience_from_kills
                .wrapping_add(gd_sim::progress::experience_reward(
                    &data.db,
                    &k.record,
                    k.level,
                    player_level,
                    mode,
                ));
        counts.hits_inflicted = counts.hits_inflicted.wrapping_add(attacks);
        if gives_experience(rec) && !crucible {
            if class == 3 || class == 5 {
                counts.boss_kills[difficulty] += 1;
            }
            if class >= 3 && mode.ultimate_veteran() {
                counts.ultimate_veteran_boss_kills += 1;
            }
        }
        let Ok(body) = bodies.get(k.entity) else {
            continue;
        };
        let sheet = &body.profile.sheet;
        let level = k.level.max(0) as u32;
        let worth = ((sheet.life + sheet.energy) * level as f32) as i64 as u32;
        let stored = counts.greatest(loaded.as_ref().map(|c| &c.0.stats), difficulty);
        if worth > stored.level.wrapping_mul(stored.life_and_mana) {
            counts.greatest[difficulty] = Some(Greatest {
                name: rec
                    .as_ref()
                    .and_then(|r| r.str("description"))
                    .unwrap_or_default()
                    .to_owned(),
                level,
                life_and_mana: (sheet.life + sheet.energy) as i64 as u32,
            });
        }
    }
}

/// `Character::ReceiveExperience`'s `max_level` count.
fn count_levels(
    mut leveled: MessageReader<crate::progress::LeveledUp>,
    mut counts: ResMut<PlayCounts>,
) {
    for l in leveled.read() {
        counts.levels += l.gained;
    }
}

/// The reported events (module doc).
fn count_events(
    mut events: MessageReader<PlayStat>,
    mut counts: ResMut<PlayCounts>,
    data: Res<GameData>,
) {
    for e in events.read() {
        match e {
            PlayStat::Crafted(record) => {
                counts.items_crafted += 1;
                let rec = data.db.get(record);
                if rec.as_ref().and_then(|r| r.str("Class")) == Some("ItemArtifact") {
                    counts.relics_crafted += 1;
                    match rec.as_ref().and_then(|r| r.str("artifactClassification")) {
                        Some("Greater") => counts.transcendent_relics_crafted += 1,
                        Some("Divine") => counts.mythical_relics_crafted += 1,
                        _ => {}
                    }
                }
            }
            PlayStat::ChestOpened(record) => {
                if let Some(rec) = data.db.get(record) {
                    if rec.bool("countsAsOneShot") {
                        counts.one_shot_chests_opened += 1;
                    }
                    if rec.bool("countsAsHidden") {
                        counts.hidden_chests_opened += 1;
                    }
                }
            }
            PlayStat::PotionUsed(class) => match class.as_str() {
                "OneShot_PotionHealth" => counts.health_potions_used += 1,
                "OneShot_PotionMana" => counts.mana_potions_used += 1,
                _ => {}
            },
            PlayStat::ShrineRestored => counts.shrines_restored += 1,
            PlayStat::Essence { buff, level } => {
                if data
                    .db
                    .get(buff)
                    .is_some_and(|r| r.bool("achievementTracking"))
                {
                    counts.endless_souls += 1;
                    counts.endless_essence = counts.endless_essence.max(*level);
                }
            }
        }
    }
}
