//! The player's faction reputation (`FactionPack`, save block 13; Game.dll `FactionPack::Adjust` /
//! `AdjustValue` / `SetValue`).
//!
//! - Values come from the character file and are written back (`save.rs`).
//! - Kills: only the player's or his pets' ([`crate::combat::KillCredit`]), never a monster of the running
//!   Shattered Realm dungeon, and only when the monster's experience reward for the player is above 0
//!   ([`gd_sim::progress::experience_reward`]). The victim's `myFaction` pack gives its `killPenalty<Class>` to
//!   that faction and `killBonus1..3<Class>` to the `killBonusNFaction` factions (bonuses only to unlocked or
//!   starting factions; the penalty skipped for a locked `noRepGainIfNegativeFactions` one). Class keys:
//!   Common / Champion / Hero, Boss for Boss / Quest / SuperBoss. A gain scales by the positive boost, a loss
//!   by the negative one, then × (1 + the altar's reputation modifier); gains are dropped for
//!   `noRepGainFactions` and, below 0, `noRepGainIfNegativeFactions`. Past a class's reduction start rank the
//!   change shrinks (only the part beyond the rank when it crosses). The result clamps to the first and last
//!   `factionValueN`. No Crucible gate: its own `gamefactions.dbr` applies.
//! - Story scripts: `GiveFaction` / `SetFaction` / `UnlockFaction`; `GetFaction` reads these values by the
//!   script's upper-case name ([`sync_story`] mirrors them into the story runtime).
//! - Writs / bounties ([`gd_rules::faction::Booster`], [`BoosterUsed`]): refused unless allowed; a writ sets the
//!   positive boost (clamped to `maxBoosterValue`) and clears the negative one, a bounty the reverse.
use crate::combat::MonsterKilled;
use bevy::prelude::*;
use gd_formats::save::gdc::Faction;
use gd_game_core::factions::index_of_name;
use gd_game_core::story::StoryEffect;
use gd_story::story::state::Effect;

pub struct FactionsPlugin;

impl Plugin for FactionsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<BoosterUsed>()
            .add_systems(
                Update,
                (
                    init_factions,
                    use_boosters.run_if(resource_exists::<PlayerFactions>),
                    kill_reputation.run_if(resource_exists::<PlayerFactions>),
                    story_factions
                        .in_set(FactionStage::Story)
                        .run_if(resource_exists::<PlayerFactions>),
                    rank_changes.run_if(resource_exists_and_changed::<PlayerFactions>),
                    turn_creatures.run_if(resource_exists_and_changed::<PlayerFactions>),
                )
                    .chain(),
            )
            .add_systems(
                Update,
                sync_story
                    .after(FactionStage::Story)
                    .run_if(resource_exists::<PlayerFactions>),
            );
    }
}

#[derive(Resource, Clone)]
pub struct PlayerFactions {
    pub list: Vec<Faction>,
    /// Bumped on every change (story / UI sync).
    pub version: u64,
}

impl PlayerFactions {
    /// `AllowUse` of a writ / bounty ([`gd_rules::faction::Booster::allowed`]).
    pub fn booster_allowed(
        &self,
        data: &gd_game_core::data::GameData,
        b: &gd_rules::faction::Booster,
    ) -> bool {
        let Some(f) = self.list.get(b.faction) else {
            return false;
        };
        let current = if b.negative {
            f.negative_boost
        } else {
            f.positive_boost
        };
        b.allowed(f.value, current, rules(data).limits)
    }

    /// `GameEngine::IsFactionPositiveBoosted` / `IsFactionNegativeBoosted`:
    /// the boost is not 0.
    pub fn boosted(&self, i: usize, negative: bool) -> bool {
        self.list.get(i).is_some_and(|f| {
            if negative {
                f.negative_boost != 0.0
            } else {
                f.positive_boost != 0.0
            }
        })
    }

    pub fn value(&self, i: usize) -> f32 {
        self.list.get(i).map_or(0.0, |f| f.value)
    }

    /// `AdjustValue` (see the module docs): gains blocked by the
    /// no-gain lists, a gain × positive boost, a loss × negative boost (when
    /// set), × (1 + `rep_mod`) when `rep_mod` > 0, clamped.
    fn adjust(&mut self, i: usize, delta: f32, rep_mod: f32, rules: &Rules) -> f32 {
        let Some(f) = self.list.get_mut(i) else {
            return 0.0;
        };
        if delta > 0.0
            && (rules.no_gain.contains(&i)
                || (f.value < 0.0 && rules.no_gain_if_negative.contains(&i)))
        {
            return 0.0;
        }
        let boost = if delta > 0.0 {
            f.positive_boost
        } else {
            f.negative_boost
        };
        let mut d = if boost > 0.0 { delta * boost } else { delta };
        if rep_mod > 0.0 {
            d *= 1.0 + rep_mod;
        }
        f.value = (f.value + d).clamp(rules.limits.0, rules.limits.1);
        debug!("faction {i}: {d:+} -> {}", f.value);
        f.modified = 1;
        self.version += 1;
        d
    }
}

/// The script name of a faction index (`SURVIVORS`, `USER4`).
pub fn script_name(i: usize) -> Option<String> {
    gd_rules::faction::KEYS
        .get(i)
        .map(|k| k.trim_start_matches("faction").to_ascii_uppercase())
}

/// The creature's own faction name (`factions` pack's `myFaction`).
pub fn my_faction(db: &gd_formats::vfs::Database, record: &str) -> Option<String> {
    let pack = db
        .get(record)?
        .str("factions")
        .filter(|p| !p.is_empty())
        .and_then(|p| db.get(p))?;
    pack.str("myFaction").map(|m| m.trim().to_owned())
}

/// The faction icon a creature record's character shows (the conversation
/// window's name line). Game.dll `Character::GetVisibleFaction`
/// (the override `Character.visibleFaction` unless -1000, else the character's
/// own faction `Character.faction`) and `GameEngine::GetFactionIcon` (the
/// engine's faction pack of that type, `FactionPack::GetFactionIcon` = the
/// pack's `factionIcon`; empty past the list). The engine's packs are
/// `gamefactions.dbr`'s `faction*` records; the type is the creature's
/// `factions` pack's `myFaction`, so the icon is the `factionIcon` of the
/// `gamefactions.dbr` record with that `myFaction` (`User1` = the neutral
/// NPCs' `faction_neutral_npc.dbr`). None without a pack (-1000).
/// PORT: the `Character.visibleFaction` override (set by `Character::SetVisibleFaction`) is not
/// modelled.
pub fn faction_icon(db: &gd_formats::vfs::Database, record: &str) -> Option<String> {
    let mine = my_faction(db, record)?;
    let gf = db.get(gd_rules::faction::GAME_FACTIONS)?;
    gd_rules::faction::KEYS
        .iter()
        .filter_map(|k| gf.str(k).and_then(|p| db.get(p)))
        .find(|r| {
            r.str("myFaction")
                .is_some_and(|m| m.trim().eq_ignore_ascii_case(&mine))
        })
        .and_then(|r| {
            r.str("factionIcon")
                .filter(|i| !i.is_empty())
                .map(str::to_owned)
        })
}

/// The conversation window's faction icon tooltip (Grim Dawn.exe `ConversationWindow::GetFactionRollover`): the
/// NPC's visible faction's name tag and the player's standing rank tag (`GameEngine::GetFactionLevelName` = the
/// `factionTagN` of the player's level, its alternate neutral tag below 0: [`gd_rules::faction::Levels::tag`]). The window localises them into `tagFaction` and
/// `tagFactionStanding`; `quest_req` is the locked quest's missing standing (`tagFactionQuestReq`, `Story::
/// locked_quest_faction`'s deficit; the engine's value is -1000 when nothing is locked).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FactionTip {
    pub name_tag: String,
    pub rank_tag: String,
    pub quest_req: Option<i32>,
}

/// The save index of the faction a creature record shows (its pack's `myFaction`, as [`faction_icon`]).
pub fn visible_faction(db: &gd_formats::vfs::Database, record: &str) -> Option<usize> {
    let mine = my_faction(db, record)?;
    let gf = db.get(gd_rules::faction::GAME_FACTIONS)?;
    gd_rules::faction::KEYS.iter().position(|k| {
        gf.str(k).and_then(|p| db.get(p)).is_some_and(|r| {
            r.str("myFaction")
                .is_some_and(|m| m.trim().eq_ignore_ascii_case(&mine))
        })
    })
}

/// The tooltip's tags: the faction's `tag<Key>` name (as the rank-change text uses) and the rank tag of `value`;
/// None when either is empty (the engine shows nothing without a faction tag).
pub fn faction_tip(
    gf: &gd_formats::arz::Record,
    faction: usize,
    value: f32,
    quest_req: Option<i32>,
) -> Option<FactionTip> {
    let rank_tag = gd_rules::faction::Levels::from_record(gf).tag(value);
    let name_tag = gd_rules::faction::tag(faction)?;
    (!rank_tag.is_empty()).then_some(FactionTip {
        name_tag,
        rank_tag,
        quest_req,
    })
}

/// `gamefactions.dbr` rules (`GameEngine::LoadFromDatabase`).
pub(crate) struct Rules {
    /// `FactionClamp`: the first and last `factionValueN`.
    limits: (f32, f32),
    no_gain: Vec<usize>,
    no_gain_if_negative: Vec<usize>,
    starting: Vec<usize>,
}

fn rules(data: &gd_game_core::data::GameData) -> Rules {
    let gf = data.db.get(gd_rules::faction::GAME_FACTIONS);
    let v = |n: u32| gf.as_ref().and_then(|g| g.f32(&format!("factionValue{n}")));
    let list = |key: &str| -> Vec<usize> {
        gf.as_ref()
            .map(|g| {
                g.strs(key)
                    .iter()
                    .flat_map(|x| x.split(';'))
                    .filter_map(index_of_name)
                    .collect()
            })
            .unwrap_or_default()
    };
    Rules {
        limits: (v(1).unwrap_or(-25000.0), v(8).unwrap_or(25000.0)),
        no_gain: list("noRepGainFactions"),
        no_gain_if_negative: list("noRepGainIfNegativeFactions"),
        starting: list("startingFactions"),
    }
}

/// A faction writ / bounty was used from the bags (`consumables::use_items`): its record.
#[derive(Message, Clone, Debug)]
pub struct BoosterUsed(pub String);

/// `ItemFactionBooster::Use` / `ItemFactionWarrant::Use` (module doc).
fn use_boosters(
    data: Res<gd_game_core::data::GameData>,
    mut used: MessageReader<BoosterUsed>,
    mut factions: ResMut<PlayerFactions>,
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
) {
    for BoosterUsed(record) in used.read() {
        let Some(rec) = data.db.get(record) else {
            continue;
        };
        let Some(b) = gd_rules::faction::Booster::from_record(rec) else {
            continue;
        };
        if !factions.booster_allowed(&data, &b) {
            info!("faction booster refused: {record}");
            continue;
        }
        let max = data
            .db
            .get(gd_rules::faction::GAME_FACTIONS)
            .and_then(|g| g.f32("maxBoosterValue"))
            .unwrap_or(0.0);
        let boost = gd_rules::faction::booster_clamp(b.multiplier, max);
        let Some(f) = factions.list.get_mut(b.faction) else {
            continue;
        };
        (f.positive_boost, f.negative_boost) = if b.negative {
            (0.0, boost)
        } else {
            (boost, 0.0)
        };
        factions.version += 1;
        info!(
            "faction {} boosted ×{boost} ({})",
            b.faction,
            if b.negative { "losses" } else { "gains" }
        );
        if let Some(pak) = rec.str("useSound").filter(|p| !p.is_empty()) {
            sound.write(gd_game_core::visuals::PlaySound {
                pak: pak.to_owned(),
                at: None,
            });
        }
    }
}

/// Value where a rank starts (`GameEngine::GetFactionLevelValue`):
/// the names Nemesis, Hated, Despised, Tolerated, Friendly,
/// Respected, Honored, Revered (any case) pick `factionValue1..8` in that
/// order; anything else has no start.
fn rank_start(data: &gd_game_core::data::GameData, rank: &str) -> Option<f32> {
    const RANKS: [&str; 8] = [
        "nemesis",
        "hated",
        "despised",
        "tolerated",
        "friendly",
        "respected",
        "honored",
        "revered",
    ];
    let n = RANKS
        .iter()
        .position(|r| r.eq_ignore_ascii_case(rank.trim()))?
        + 1;
    data.db
        .get(gd_rules::faction::GAME_FACTIONS)?
        .f32(&format!("factionValue{n}"))
}

/// `Adjust`'s rank reduction: the whole `delta` shrinks by `reduction` once
/// `value` is past `start` (at or below it for a loss, at or above for a
/// gain); when the change crosses `start` only the part beyond it shrinks.
fn reduce(delta: f32, value: f32, start: f32, reduction: f32) -> f32 {
    let past = if delta < 0.0 {
        value <= start
    } else {
        value >= start
    };
    if past {
        delta - delta * reduction
    } else if (delta < 0.0 && value + delta < start) || (delta > 0.0 && value + delta > start) {
        delta - (delta - (start - value)) * reduction
    } else {
        delta
    }
}

pub fn init_factions(
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
    epoch: Res<gd_game_core::session::SessionEpoch>,
    mut commands: Commands,
    // Option: `have` is the existence check (this system creates it); `loaded` is absent for a new character.
    have: Option<Res<PlayerFactions>>,
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
    players: Query<(), With<gd_game_core::player::Player>>,
) {
    if have.is_some() || players.is_empty() {
        return;
    }
    let mut list: Vec<Faction> = loaded
        .as_ref()
        .map(|c| c.0.factions.factions.clone())
        .unwrap_or_default();
    let n = gd_rules::faction::FACTION_COUNT;
    // A new character (no loaded file: the `GD_NAME` / failed-`GD_SAVE` hook path) starts at the menu's
    // fresh-character values (`save::NEW_FACTION_VALUES`), not all zero; a loaded file's missing slots
    // stay zero (the engine loads what the file holds).
    let fresh = loaded.is_none();
    while list.len() < n {
        let value = if fresh {
            crate::save::NEW_FACTION_VALUES
                .get(list.len())
                .copied()
                .unwrap_or(0.0)
        } else {
            0.0
        };
        list.push(Faction {
            modified: 0,
            unlocked: 0,
            value,
            positive_boost: 0.0,
            negative_boost: 0.0,
        });
    }
    // `GD_FACTION` (hooks.rs).
    #[cfg(feature = "dev")]
    for (name, v) in &hooks.gameplay.faction {
        if let Some(i) = index_of_name(name)
            && let Some(f) = list.get_mut(i)
        {
            f.value = *v;
            f.modified = 1;
        }
    }
    commands.insert_resource(PlayerFactions {
        list,
        version: epoch.version_base() + 1,
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn kill_reputation(
    mut killed: MessageReader<MonsterKilled>,
    mut f: ResMut<PlayerFactions>,
    data: Res<gd_game_core::data::GameData>,
    session: crate::session::Session,
    mut cinematic: MessageWriter<gd_game_core::story::CinematicText>,
    credit: crate::combat::KillCredit,
    (rt, monsters, progress): (
        Option<NonSend<gd_game_core::story::StoryRuntime>>,
        Query<&gd_game_core::monsters::Monster>,
        Option<Res<gd_game_core::progress::PlayerProgress>>,
    ),
    (mut cached, mut fresh): (Local<Option<Rules>>, gd_game_core::session::NewSession),
) {
    // A new session may run another mod's `gamefactions.dbr`.
    if fresh.begun() {
        *cached = None;
    }
    if killed.is_empty() {
        return;
    }
    // The rules only change with the record (`gamefactions.dbr`): build them once.
    let rules = &*cached.get_or_insert_with(|| rules(&data));
    // `OnCreatureDeath` hands `Adjust` the altar's reputation modifier
    // (`GetAscendantReputationMod`).
    let rep = session.ascendant().reputation;
    for k in killed.read() {
        // Only the player's or his pets' kills (`HandleFaction`, [`crate::combat::KillCredit`]).
        if !credit.of(k) {
            continue;
        }
        // `InEndlessDungeon` (module doc's PORT note).
        let in_dungeon = rt.as_ref().is_some_and(|rt| {
            let story = rt.story.borrow();
            monsters.get(k.entity).is_ok_and(|m| {
                story
                    .endless
                    .activated
                    .iter()
                    .any(|(_, key)| *key == (m.slot.0, m.slot.1))
            })
        });
        if in_dungeon {
            continue;
        }
        // `HandleFaction`: the victim's `GetExperienceReward` for the player must be above 0.
        let player_level = progress.as_ref().map_or(1, |p| p.progress.level);
        if gd_sim::progress::experience_reward(
            &data.db,
            &k.record,
            k.level,
            player_level,
            session.mode(),
        ) == 0
        {
            continue;
        }
        let Some(rec) = data.db.get(&k.record) else {
            continue;
        };
        let Some(pack) = rec.str("factions").and_then(|p| data.db.get(p)) else {
            continue;
        };
        let class = match rec.str("monsterClassification").unwrap_or("Common") {
            "Champion" => "Champion",
            "Hero" => "Hero",
            "Boss" | "Quest" | "SuperBoss" => "Boss",
            _ => "Common",
        };
        let lower = class.to_ascii_lowercase();
        debug!("kill {} ({class}): faction pack {}", k.record, pack.name);
        if let Some(me) = pack.str("myFaction").and_then(index_of_name) {
            let mut p = pack.f32(&format!("killPenalty{class}")).unwrap_or(0.0);
            let locked = rules.no_gain_if_negative.contains(&me)
                && !f.list.get(me).is_some_and(|x| x.unlocked != 0);
            if p != 0.0 && !locked {
                if let Some(start) = pack
                    .str(&format!("{lower}KillPenaltyReductionStart"))
                    .and_then(|r| rank_start(&data, r))
                {
                    p = reduce(
                        p,
                        f.value(me),
                        start,
                        pack.f32(&format!("{lower}KillPenaltyReduction"))
                            .unwrap_or(0.0),
                    );
                }
                f.adjust(me, p, rep, rules);
            }
        }
        for n in 1..=3 {
            let Some(i) = pack
                .str(&format!("killBonus{n}Faction"))
                .and_then(index_of_name)
            else {
                continue;
            };
            let mut b = pack.f32(&format!("killBonus{n}{class}")).unwrap_or(0.0);
            if !(f.list.get(i).is_some_and(|x| x.unlocked != 0) || rules.starting.contains(&i)) {
                continue;
            }
            if let Some(start) = pack
                .str(&format!("{lower}MonsterGainReductionStart"))
                .and_then(|r| rank_start(&data, r))
            {
                b = reduce(
                    b,
                    f.value(i),
                    start,
                    pack.f32(&format!("{lower}MonsterGainReduction"))
                        .unwrap_or(0.0),
                );
            }
            let d = f.adjust(i, b, rep, rules);
            // `FactionPack::AdjustValue`: a bonus of at least 1
            // from a Hero or higher kill shows `tagReputationBonus` over `tagRepAwarded(amount, faction)` (type 3).
            if d >= 1.0
                && class != "Common"
                && class != "Champion"
                && let Some(tag_name) = gd_rules::faction::tag(i)
            {
                cinematic.write(gd_game_core::story::CinematicText {
                    head: "tagReputationBonus".into(),
                    sub: "tagRepAwarded".into(),
                    sub_args: vec![
                        gd_game_core::floating::TextArg::Num(d.round()),
                        gd_game_core::floating::TextArg::Tag(tag_name.clone()),
                    ],
                    kind: 3,
                    front: false,
                });
            }
        }
    }
}

/// `FactionPack::SetValue`: when a faction's rank (its level tag)
/// changes, `tagFactionUpdated` over `tagFactionStatusChange(rank, faction)` (`tagFactionNegativeStatusChange` below 0), type 3.
/// PORT: seen from the values when they change (kills and scripts alike); the first sight only records the ranks; the
/// rank name's colour code is not inserted; hidden factions are not checked.
pub fn rank_changes(
    f: Res<PlayerFactions>,
    data: Res<gd_game_core::data::GameData>,
    // The level table (built once a session) and each faction's level and sign (a negative value in the neutral
    // level shows the alternate tag).
    (mut levels, mut ranks): (
        Local<Option<gd_rules::faction::Levels>>,
        Local<Vec<(Option<usize>, bool)>>,
    ),
    mut cinematic: MessageWriter<gd_game_core::story::CinematicText>,
    mut fresh: gd_game_core::session::NewSession,
) {
    if fresh.begun() {
        ranks.clear();
        *levels = None;
    }
    if levels.is_none() {
        let Some(gf) = data.db.get(gd_rules::faction::GAME_FACTIONS) else {
            return;
        };
        *levels = Some(gd_rules::faction::Levels::from_record(gf));
    }
    let Some(levels) = levels.as_ref() else {
        return;
    };
    let first = ranks.len() != f.list.len();
    ranks.resize(f.list.len(), (None, false));
    for (i, old) in ranks.iter_mut().enumerate() {
        let v = f.value(i);
        let now = (levels.level_of(v), v < 0.0);
        if !first
            && now != *old
            && let Some(tag_name) = gd_rules::faction::tag(i)
        {
            let tag = levels.tag(v);
            if !tag.is_empty() {
                cinematic.write(gd_game_core::story::CinematicText {
                    head: "tagFactionUpdated".into(),
                    sub: if v < 0.0 {
                        "tagFactionNegativeStatusChange"
                    } else {
                        "tagFactionStatusChange"
                    }
                    .into(),
                    sub_args: vec![
                        gd_game_core::floating::TextArg::Tag(tag),
                        gd_game_core::floating::TextArg::Tag(tag_name),
                    ],
                    kind: 3,
                    front: false,
                });
            }
        }
        *old = now;
    }
}

/// A reputation change turns the creatures of that faction at once: the engine keeps no friend / foe state, every
/// `FactionManager::IsFoe` (sight, `IsEnemyValid`, the player's targeting) reads the packs as they are
/// now. The port's player-side mark, `combat::Allied`
/// ([`crate::monsters::is_friendly`] and [`crate::monsters::is_fighter`], set at spawn), is re-decided for the
/// fighting monsters of each faction whose player value changed sign (below 0, 0, above 0: the only changes that
/// move `IsFoe`); a monster that turns hostile takes the player up by sight, one that turns friendly drops him
/// (its enemies are re-checked every frame). Callers of the four `IsFoe` overloads (Game.dll): no
/// controller nor `Monster::UpdateSelf` caches the answer, so there is nothing to re-evaluate in the engine; the
/// mark is the port's cache of it. A creature spawned as a plain NPC ([`crate::monsters::stands_as_npc`]) stays
/// one: in stock every such record is of an `Npc*` class ([`crate::monsters::is_npc`]: every creature of class
/// `Monster` has a controller), which never fights nor takes a hit whatever its faction.
pub fn turn_creatures(
    f: Res<PlayerFactions>,
    data: Res<gd_game_core::data::GameData>,
    mut signs: Local<Vec<i8>>,
    mut fresh: gd_game_core::session::NewSession,
    monsters: Query<
        (
            Entity,
            &gd_game_core::monsters::Monster,
            &crate::combat::MonsterAi,
            Has<crate::combat::Allied>,
        ),
        // A converted monster stands on its leader's side until `combat::unjoin_converted` puts it back.
        (
            Without<gd_game_core::combat::Dead>,
            Without<crate::combat::Converted>,
        ),
    >,
    mut commands: Commands,
) {
    if fresh.begun() {
        signs.clear();
    }
    let now: Vec<i8> = (0..gd_rules::faction::FACTION_COUNT)
        .map(|t| {
            let v = crate::monsters::player_value(&data.db, Some(&f), t);
            (v > 0.0) as i8 - (v < 0.0) as i8
        })
        .collect();
    if signs.len() == now.len() && *signs != now {
        for (e, m, ai, allied) in &monsters {
            let Some(t) = ai.faction.as_ref().map(|p| p.mine) else {
                continue;
            };
            if signs.get(t) == now.get(t) {
                continue;
            }
            let want = crate::monsters::is_friendly(&data.db, Some(&f), &m.record);
            if want != allied {
                debug!("faction change turns {} {e}: allied {want}", m.record);
                if want {
                    commands.entity(e).try_insert(crate::combat::Allied);
                } else {
                    commands.entity(e).try_remove::<crate::combat::Allied>();
                }
            }
        }
    }
    *signs = now;
}

/// Mirrors the values into the story's `GetFaction` (module doc) when they changed.
pub fn sync_story(
    factions: Res<PlayerFactions>,
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    mut fresh: gd_game_core::session::NewSession,
    mut seen: Local<u64>,
) {
    if fresh.begun() {
        *seen = 0;
    }
    let Some(rt) = rt else { return };
    if factions.version == *seen {
        return;
    }
    *seen = factions.version;
    let mut s = rt.story.borrow_mut();
    for (i, x) in factions.list.iter().enumerate() {
        if let Some(n) = script_name(i) {
            s.player.factions.insert(n, x.value);
        }
    }
}

fn story_factions(
    mut events: MessageReader<StoryEffect>,
    mut f: ResMut<PlayerFactions>,
    data: Res<gd_game_core::data::GameData>,
    (mut cached, mut fresh): (Local<Option<Rules>>, gd_game_core::session::NewSession),
) {
    // A new session may run another mod's `gamefactions.dbr`.
    if fresh.begun() {
        *cached = None;
    }
    if events.is_empty() {
        return;
    }
    let rules = &*cached.get_or_insert_with(|| rules(&data));
    let lim = rules.limits;
    for StoryEffect(e) in events.read() {
        match e {
            Effect::Faction {
                faction,
                delta,
                set,
            } => {
                let Some(i) = index_of_name(faction) else {
                    warn!("story: unknown faction {faction}");
                    continue;
                };
                if *set {
                    if let Some(x) = f.list.get_mut(i) {
                        x.value = delta.clamp(lim.0, lim.1);
                        x.modified = 1;
                    }
                    f.version += 1;
                } else {
                    f.adjust(i, *delta, 0.0, rules);
                }
                info!("faction {faction} -> {}", f.value(i));
            }
            Effect::UnlockFaction(name) => {
                if let Some(i) = index_of_name(name)
                    && let Some(x) = f.list.get_mut(i)
                {
                    x.unlocked = 1;
                    f.version += 1;
                    info!("faction {name} unlocked");
                }
            }
            _ => {}
        }
    }
}

/// Faction updates: `Story` applies the scripts' reputation changes.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FactionStage {
    Story,
}
