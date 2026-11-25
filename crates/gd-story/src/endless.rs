//! The Shattered Realm's (endless dungeon) generator rules: the `EndlessDungeonGenerator` ruleset
//! records (`records/endlessdungeon/rulesets/dungeonsetNN.dbr`, one per shard range) and the floor
//! picking. Bevy-free; the host (`gd-game`) owns the running dungeon.
//!
//! `EndlessDungeon_Generator::LoadFromDatabase` reads the fields of [`Ruleset`] (the difficulty picks the
//! Normal / Elite / Ultimate triple of the equations; the timer and floor total always use `*Ultimate`);
//! `PickFloors` shuffles `floorList`, skips the last [`RECENT_FLOORS`] floors used
//! (a queue that survives generations), takes `floors` of them, then the first of the shuffled
//! `bossFloorList` as the boss floor (no queue). `specialFloors` is not read (the special
//! floors are inside `floorList`). `PickProxies`: a nemesis / shrine count is a
//! weighted index of its weight list (shrines capped by the shared `maxShrines`).
//! `ResolveEquationVariable`: `tierDifficulty` = dungeons generated, `commonProxies` / `heroProxies` = the
//! proxies `LoadProxies` placed on the floor being totalled (not the ruleset's counts), `killCounter`.
//!
//! PORT: the shuffle is the port's Park–Miller [`Rng`] (the engine uses C `rand()` through
//! `std::shuffle`); when exactly the recent queue is filled is not traced (the picked floors are pushed when
//! they are picked).
//!
//! PORT test hook: [`EndlessMap::forced_floor`] names a floor record that takes floor 0's slot in every
//! generated section, bypassing the recent queue.
use gd_formats::vfs::Database;
use gd_rules::Rng;
use gd_rules::expr::Expr;
use std::collections::VecDeque;

/// `kMaxFloorQueueSize`: floors used by the last generations are not picked again.
pub const RECENT_FLOORS: usize = 5;

/// What an essence / classification pays: index 0 common, 1 champion, 2 hero, 3 quest boss.
pub const CLASSES: usize = 4;

#[derive(Debug, Clone)]
pub struct Ruleset {
    pub name: String,
    pub floors: usize,
    pub floor_list: Vec<String>,
    pub boss_floor_list: Vec<String>,
    /// Proxy counts per floor: common, hero, boss (boss floors), trap / boost.
    pub common_proxies: usize,
    pub hero_proxies: usize,
    pub boss_proxies: usize,
    pub trap_proxies: usize,
    pub max_shrines: usize,
    pub nemesis_weights: Vec<u32>,
    pub shrine_weights: Vec<u32>,
    pub floor_total: Option<Expr>,
    pub timer: Option<Expr>,
    pub kill_count_bonus: Option<Expr>,
    pub kill_count_minimum: u32,
    pub kill_count_reset_ms: i32,
    /// Bonus time per kill (ms): hero, boss, nemesis.
    pub bonus_time_ms: [i32; 3],
    pub dying_penalty_ms: i32,
    /// `healthGainOnKillPct` and its `healthGainOnKillDifficulty` gate (the Veteran challenge index it needs): the
    /// percent of its own max life every aggroed monster heals when the player dies (`Monster::OnKilledPlayer`).
    pub health_gain_pct: f32,
    pub health_gain_challenge: u32,
    /// Essence drop chance (percent) and item by classification (common, champion, hero, quest boss).
    pub essence_chance: [f32; CLASSES],
    pub essence_item: [String; CLASSES],
    pub blacklisted_heroes: Vec<String>,
    pub allowed_items: Vec<String>,
    pub disable_monster_drops: bool,
    /// The `GameAdjustment` record replacing the balance tables in the dungeon (by difficulty).
    pub difficulty_adjustment: Option<String>,
}

fn lower_list(v: Vec<&str>) -> Vec<String> {
    v.into_iter().map(|s| s.to_ascii_lowercase()).collect()
}

impl Ruleset {
    /// The ruleset slot of a game mode ([`Self::load`]): the difficulty, 3 for Ultimate Veteran.
    pub fn slot(mode: &gd_rules::mode::Mode) -> usize {
        mode.ascendant_index()
    }

    /// Loads `record` for a ruleset slot ([`Self::slot`]: 0 Normal, 1 Elite, 2 Ultimate, 3 Ultimate Veteran).
    /// `EndlessDungeon_Generator::LoadFromDatabase`: on Ultimate the timer and floor total are always
    /// `*Ultimate`; the adjustment is `difficultyAdjustmentUltimateVeteran` while `GameEngine::GetDifficultyRamp`
    /// is on; a missing field is an empty name (no adjustment, no fallback).
    pub fn load(db: &Database, record: &str, slot: usize) -> Option<Self> {
        let r = db.get(record)?;
        let pick = |base: &str| -> Option<&str> {
            let suffix = match (slot, base) {
                (3.., "difficultyAdjustment") => "UltimateVeteran",
                _ => ["Normal", "Elite", "Ultimate", "Ultimate"][slot.min(3)],
            };
            r.str(&format!("{base}{suffix}"))
        };
        let count = |k: &str| r.f32(k).unwrap_or(0.0).max(0.0) as usize;
        let ms = |k: &str| (r.f32(k).unwrap_or(0.0) * 1000.0) as i32;
        let weights = |k: &str| {
            r.f32s(k)
                .into_iter()
                .map(|w| w.max(0.0) as u32)
                .collect::<Vec<_>>()
        };
        let expr = |s: Option<&str>| s.and_then(|e| Expr::parse(e).ok());
        let class_key = ["Common", "Champion", "Hero", "QuestBoss"];
        Some(Self {
            name: record.to_owned(),
            floors: count("floors"),
            floor_list: lower_list(r.strs("floorList")),
            boss_floor_list: lower_list(r.strs("bossFloorList")),
            common_proxies: count("proxies"),
            hero_proxies: count("heroProxies"),
            boss_proxies: count("bossProxies"),
            trap_proxies: count("trapProxies"),
            max_shrines: count("maxShrines"),
            nemesis_weights: weights("nemesisProxyWeights"),
            shrine_weights: weights("shrineProxyWeights"),
            floor_total: expr(pick("floorTotal")),
            timer: expr(pick("timer")),
            kill_count_bonus: expr(r.str("killCountBonusTime")),
            kill_count_minimum: count("killCountMinimum") as u32,
            kill_count_reset_ms: ms("killCountResetTime"),
            bonus_time_ms: [
                ms("bonusTimeHero"),
                ms("bonusTimeBoss"),
                ms("bonusTimeNemesis"),
            ],
            dying_penalty_ms: ms("dyingPenalty"),
            health_gain_pct: r.f32("healthGainOnKillPct").unwrap_or(0.0).max(0.0),
            health_gain_challenge: count("healthGainOnKillDifficulty") as u32,
            essence_chance: class_key.map(|c| {
                r.f32(&format!(
                    "{}Chance",
                    c.to_ascii_lowercase().replace("questboss", "questBoss")
                ))
                .unwrap_or(0.0)
            }),
            essence_item: class_key.map(|c| {
                r.str(&format!("itemName{c}"))
                    .unwrap_or("")
                    .to_ascii_lowercase()
            }),
            blacklisted_heroes: lower_list(r.strs("blacklistedHeroes")),
            allowed_items: lower_list(r.strs("allowedItems")),
            disable_monster_drops: r.bool("disableMonsterDrops"),
            difficulty_adjustment: pick("difficultyAdjustment").map(str::to_owned),
        })
    }

    /// `ResolveEquationVariable`. `proxies`: the common / hero proxies the last `LoadProxies` placed; the timer and
    /// kill-chain equations get 0.
    fn vars(
        &self,
        tier: u32,
        kill_counter: u32,
        proxies: [usize; 2],
    ) -> impl Fn(&str) -> Option<f32> {
        move |k| match k {
            "tierDifficulty" => Some(tier as f32),
            "commonProxies" => Some(proxies[0] as f32),
            "heroProxies" => Some(proxies[1] as f32),
            "killCounter" => Some(kill_counter as f32),
            _ => None,
        }
    }

    /// A floor's progress total (`PickProxies`): a normal floor's is the `floorTotal*` equation with the common /
    /// hero proxies actually placed on it, truncated to an integer; a boss floor's is 100.
    pub fn floor_total(&self, tier: u32, boss: bool, proxies: [usize; 2]) -> f32 {
        if boss {
            return 100.0;
        }
        self.floor_total.as_ref().map_or(0.0, |e| {
            finite(e.eval(&self.vars(tier, 0, proxies)))
                .max(0.0)
                .trunc()
        })
    }

    pub fn timer_ms(&self, tier: u32) -> i32 {
        self.timer
            .as_ref()
            .map_or(0, |e| finite(e.eval(&self.vars(tier, 0, [0; 2]))) as i32)
    }

    /// Bonus time (ms) a kill of this `monsterClassification` adds (`OnCreatureDeath`).
    pub fn bonus_for_class(&self, class: &str) -> i32 {
        match class {
            "Hero" => self.bonus_time_ms[0],
            "Boss" | "Quest" => self.bonus_time_ms[1],
            _ => 0,
        }
    }

    pub fn essence_index(class: &str) -> usize {
        match class {
            "Champion" => 1,
            "Hero" => 2,
            "Boss" | "Quest" => 3,
            _ => 0,
        }
    }

    /// The kill chain's bonus (ms) once it pauses with `kill_counter >= killCountMinimum` kills; the equation's
    /// seconds are truncated before the ×1000 (`EndlessDungeon_Generator::Update`).
    pub fn kill_chain_bonus_ms(&self, kill_counter: u32) -> Option<i32> {
        (kill_counter >= self.kill_count_minimum).then(|| {
            self.kill_count_bonus.as_ref().map_or(0, |e| {
                (e.eval(&self.vars(0, kill_counter, [0; 2])) as i32).saturating_mul(1000)
            })
        })
    }
}

/// `GameEngine::SetEndlessTimerInbound`'s text for added bonus time: `tagEndlessDungeonBonusTime` with the kill
/// chain's kills, or, without a chain, `tagEndlessDungeonHeroBonusTime`. `AddTagToMap` strips the loaded tags'
/// braces (`%.0f0`), so both forms are substituted.
pub fn bonus_text(template: &str, kills: Option<u32>, ms: i32) -> String {
    let secs = format!("{:.0}", ms as f32 * 0.001);
    let mut t = template.to_owned();
    match kills {
        Some(k) => {
            t = t
                .replace("{%d0}", &k.to_string())
                .replace("%d0", &k.to_string());
            t = t.replace("{%.0f1}", &secs).replace("%.0f1", &secs);
        }
        None => t = t.replace("{%.0f0}", &secs).replace("%.0f0", &secs),
    }
    t.replace("^n", "\n")
}

/// A floor of a section: its record, the level files of its regions and whether it is the boss floor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Floor {
    pub record: String,
    /// `levels/endlessdungeon/.../x.lvl` paths (lower case): the floor's clusters of regions.
    pub regions: Vec<String>,
    pub boss: bool,
}

pub fn shuffle<T>(v: &mut [T], rng: &mut Rng) {
    for i in (1..v.len()).rev() {
        let j = rng.next() as usize % (i + 1);
        v.swap(i, j);
    }
}

/// `weights` index by a roll over their sum (`PickProxies`' nemesis / shrine count).
pub fn weighted_index(weights: &[u32], rng: &mut Rng) -> usize {
    // Summed in u64 so huge weights don't overflow.
    let sum: u64 = weights.iter().map(|&w| w as u64).sum();
    if sum == 0 {
        return 0;
    }
    let mut roll = rng.next() as u64 % sum;
    for (i, &w) in weights.iter().enumerate() {
        let w = w as u64;
        if roll < w {
            return i;
        }
        roll -= w;
    }
    weights.len().saturating_sub(1)
}

/// `PickFloors`: `floors` normal floors of the shuffled `floorList` that are not in `recent`, then the
/// first of the shuffled boss list. The picked normal floors join `recent` (capped at [`RECENT_FLOORS`]).
/// `forced` ([`EndlessMap::forced_floor`]) takes the first slot even when recent; an unknown record is ignored.
pub fn pick_floors(
    rs: &Ruleset,
    db: &Database,
    recent: &mut VecDeque<String>,
    rng: &mut Rng,
    forced: Option<&str>,
) -> Vec<Floor> {
    let load = |record: &str, boss: bool| {
        db.get(record).map(|r| Floor {
            record: record.to_owned(),
            regions: lower_list(r.strs("regions")),
            boss,
        })
    };
    let mut list = rs.floor_list.clone();
    shuffle(&mut list, rng);
    let mut out: Vec<Floor> = Vec::new();
    if rs.floors > 0
        && let Some(f) = forced.and_then(|name| load(name, false))
    {
        out.push(f);
    }
    for name in list {
        if out.len() >= rs.floors {
            break;
        }
        if recent.contains(&name) || out.iter().any(|f| f.record == name) {
            continue;
        }
        if let Some(f) = load(&name, false) {
            out.push(f);
        }
    }
    for f in &out {
        recent.push_back(f.record.clone());
        while recent.len() > RECENT_FLOORS {
            recent.pop_front();
        }
    }
    let mut bosses = rs.boss_floor_list.clone();
    shuffle(&mut bosses, rng);
    if let Some(b) = bosses.first().and_then(|b| load(b, true)) {
        out.push(b);
    }
    out
}

/// An entity of an instance group (an entrance, an exit, a proxy): its uid, region index and world position.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    pub uid: [u32; 4],
    pub region: usize,
    pub pos: [f32; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProxyKind {
    Common,
    Hero,
    Boss,
    Traps,
    Nemesis,
    Shrine,
}

impl ProxyKind {
    pub fn group_kind(self) -> &'static str {
        match self {
            Self::Common => "Endless Dungeon Proxies Common",
            Self::Hero => "Endless Dungeon Proxies Hero",
            Self::Boss => "Endless Dungeon Proxies Boss",
            Self::Traps => "Endless Dungeon Proxies Traps/Boosts",
            Self::Nemesis => "Endless Dungeon Proxies Nemesis",
            Self::Shrine => "Endless Dungeon Proxies Shrine",
        }
    }

    pub fn of_group_kind(kind: &str) -> Option<Self> {
        [
            Self::Common,
            Self::Hero,
            Self::Boss,
            Self::Traps,
            Self::Nemesis,
            Self::Shrine,
        ]
        .into_iter()
        .find(|k| k.group_kind() == kind)
    }
}

/// What the generator reads of the world map: region names, their neighbour lists, the "Endless Dungeon
/// Entrances" entities and the proxy groups by kind.
#[derive(Debug, Clone, Default)]
pub struct EndlessMap {
    /// Level file names (lower case, `/` separators), by region index.
    pub region_names: Vec<String>,
    pub neighbours: Vec<Vec<usize>>,
    pub entrances: Vec<Entity>,
    pub proxy_groups: Vec<(ProxyKind, Vec<Entity>)>,
    /// PORT test hook: the floor record forced into floor 0's slot of every generation.
    pub forced_floor: Option<String>,
}

impl EndlessMap {
    /// Reads what the generator needs from the world map: region names and neighbours, the entrances and the
    /// proxy groups with each member's region and world position, found by scanning the `endlessdungeon` regions'
    /// levels for the members' uids. PORT: the engine keeps the entities loaded with the world.
    pub fn from_world(map: &gd_formats::map::MapFile) -> Self {
        use gd_formats::collections::{HashMap, HashSet};
        let mut wanted: HashSet<[u32; 4]> = HashSet::default();
        for g in map
            .instance_groups
            .iter()
            .filter(|g| g.kind.starts_with("Endless Dungeon"))
        {
            wanted.extend(g.entries.iter().map(|e| e.uid));
        }
        let mut found: HashMap<[u32; 4], Entity> = HashMap::default();
        for (i, r) in map.regions.iter().enumerate() {
            if !r.name.to_ascii_lowercase().contains("endlessdungeon")
                || found.len() == wanted.len()
            {
                continue;
            }
            let Ok(level) = map.level(i) else { continue };
            for e in &level.entities {
                if let Some(uid) = e.uid.filter(|u| wanted.contains(u)) {
                    found.insert(
                        uid,
                        Entity {
                            uid,
                            region: i,
                            pos: [0, 1, 2].map(|k| r.position[k] as f32 + e.position[k]),
                        },
                    );
                }
            }
        }
        let members = |g: &gd_formats::map::sector::InstanceGroup| -> Vec<Entity> {
            g.entries
                .iter()
                .filter_map(|e| found.get(&e.uid).cloned())
                .collect()
        };
        let mut out = Self {
            region_names: map
                .regions
                .iter()
                .map(|r| r.name.replace('\\', "/").to_ascii_lowercase())
                .collect(),
            neighbours: gd_formats::map::links::neighbours(&map.regions),
            ..Self::default()
        };
        for g in map.instance_groups.iter() {
            if g.kind == "Endless Dungeon Entrances" {
                out.entrances.extend(members(g));
            } else if let Some(k) = ProxyKind::of_group_kind(&g.kind) {
                out.proxy_groups.push((k, members(g)));
            }
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProxyInfo {
    pub kind: ProxyKind,
    pub entity: Entity,
    /// `ProxyInfo.activated`: the generator has run it.
    pub activated: bool,
}

#[derive(Debug, Clone)]
pub struct FloorState {
    pub floor: Floor,
    /// The floor's connected regions (empty: no entrance matched, the floor is unusable).
    pub regions: Vec<usize>,
    pub entrance: Option<Entity>,
    pub proxies: Vec<ProxyInfo>,
    pub progress: f32,
    pub total: f32,
}

/// A generated dungeon: a section of floors (normal floors, then the boss floor).
#[derive(Debug, Clone)]
pub struct Dungeon {
    pub ruleset: String,
    /// Dungeons generated, this one included (`tierDifficulty`).
    pub tier: u32,
    pub floors: Vec<FloorState>,
    /// `activeFloor`.
    pub active: usize,
    /// `dungeonId`: makes objects of an older dungeon stale.
    pub id: u32,
}

impl Dungeon {
    /// `GetRevealMonsters`: the active floor is the boss floor or its progress reached 80 % of its total. While it
    /// holds, `Monster::AppendDetailMapData` marks every live foe of the dungeon on the maps instead of its own icon.
    pub fn reveal_monsters(&self) -> bool {
        self.floors
            .get(self.active)
            .is_some_and(|f| f.floor.boss || (f.total > 0.0 && f.progress / f.total >= 0.8))
    }

    /// `IncrementProgress` for a normal active floor: it ends when the progress reached its total, or
    /// all its proxies ran and none of their monsters (`alive`) is left; the last floor of the section never
    /// advances (the boss floor ends by [`Self::boss_floor_done`]).
    pub fn normal_floor_done(&self, alive: usize) -> bool {
        let Some(f) = self.floors.get(self.active) else {
            return false;
        };
        let all_ran = !f.proxies.is_empty() && f.proxies.iter().all(|p| p.activated);
        (all_ran && alive == 0 || f.progress >= f.total) && self.active + 1 < self.floors.len()
    }

    /// A boss floor ends when every boss proxy ran and no boss (`alive_bosses`) is left (`IsReallyDead`).
    pub fn boss_floor_done(&self, alive_bosses: usize) -> bool {
        let Some(f) = self.floors.get(self.active) else {
            return false;
        };
        let mut bosses = f
            .proxies
            .iter()
            .filter(|p| p.kind == ProxyKind::Boss)
            .peekable();
        bosses.peek().is_some() && bosses.all(|p| p.activated) && alive_bosses == 0
    }
}

/// `\` and `/` alike, case-insensitive.
fn norm(p: &str) -> String {
    p.replace('\\', "/").to_ascii_lowercase()
}

/// An equation result, 0 when not finite.
fn finite(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

/// `PickExits`: for each entrance (in shuffled order) the regions connected to it
/// within 50 hops; a floor not yet placed whose level path is a suffix of a connected region name
/// takes that region set and that entrance.
pub fn pick_exits(floors: &mut [FloorState], map: &EndlessMap, rng: &mut Rng) {
    let mut entrances = map.entrances.clone();
    shuffle(&mut entrances, rng);
    // `region_names` are already normalised by `EndlessMap::from_world`.
    let paths: Vec<Vec<String>> = floors
        .iter()
        .map(|f| f.floor.regions.iter().map(|p| norm(p)).collect())
        .collect();
    for e in entrances {
        let set = gd_formats::map::links::connected_set(&map.neighbours, e.region, 50);
        for (f, paths) in floors.iter_mut().zip(&paths) {
            if f.entrance.is_some() {
                continue;
            }
            let hit = set.iter().any(|r| {
                let name = &map.region_names[*r];
                paths.iter().any(|p| name.ends_with(p.as_str()))
            });
            if hit {
                f.regions = set.clone();
                f.entrance = Some(e.clone());
            }
        }
    }
}

/// `LoadProxies`: up to `count` entities of the groups of `kind` that lie in the floor's regions, each group's
/// members shuffled; returns how many it placed. PORT: the engine shuffles every group on a separate `rand()`, so
/// returning early picks the same proxies.
fn load_proxies(
    floor: &mut FloorState,
    map: &EndlessMap,
    kind: ProxyKind,
    count: usize,
    rng: &mut Rng,
) -> usize {
    let mut taken = 0;
    for (k, members) in &map.proxy_groups {
        if *k != kind {
            continue;
        }
        let mut members: Vec<&Entity> = members.iter().collect();
        shuffle(&mut members, rng);
        for m in members {
            if taken >= count {
                return taken;
            }
            if floor.regions.contains(&m.region) {
                floor.proxies.push(ProxyInfo {
                    kind,
                    entity: m.clone(),
                    activated: false,
                });
                taken += 1;
            }
        }
    }
    taken
}

/// `PickProxies`: every floor (boss and entrance-less ones too) rolls a nemesis count and a shrine count under the
/// shared `maxShrines` budget, then loads the common, hero, trap, nemesis, shrine and boss groups lying in its
/// regions, and sets its total.
pub fn pick_proxies(
    floors: &mut [FloorState],
    rs: &Ruleset,
    map: &EndlessMap,
    rng: &mut Rng,
    tier: u32,
) {
    let mut shrines_left = rs.max_shrines;
    for f in floors.iter_mut() {
        let nemesis = weighted_index(&rs.nemesis_weights, rng);
        let shrines = weighted_index(&rs.shrine_weights, rng).min(shrines_left);
        shrines_left -= shrines;
        let common = load_proxies(f, map, ProxyKind::Common, rs.common_proxies, rng);
        let hero = load_proxies(f, map, ProxyKind::Hero, rs.hero_proxies, rng);
        load_proxies(f, map, ProxyKind::Traps, rs.trap_proxies, rng);
        load_proxies(f, map, ProxyKind::Nemesis, nemesis, rng);
        load_proxies(f, map, ProxyKind::Shrine, shrines, rng);
        load_proxies(f, map, ProxyKind::Boss, rs.boss_proxies, rng);
        f.total = rs.floor_total(tier, f.floor.boss, [common, hero]);
    }
}

/// `GameEngine::GenerateEndlessDungeon`: floor 0 active. `tier` is the dungeons generated count including this
/// one; `id` the new stamp.
#[allow(clippy::too_many_arguments)]
pub fn generate(
    rs: &Ruleset,
    db: &Database,
    map: &EndlessMap,
    recent: &mut VecDeque<String>,
    rng: &mut Rng,
    tier: u32,
    id: u32,
) -> Dungeon {
    let mut floors: Vec<FloorState> = pick_floors(rs, db, recent, rng, map.forced_floor.as_deref())
        .into_iter()
        .map(|floor| FloorState {
            floor,
            regions: Vec::new(),
            entrance: None,
            proxies: Vec::new(),
            progress: 0.0,
            total: 0.0,
        })
        .collect();
    pick_exits(&mut floors, map, rng);
    pick_proxies(&mut floors, rs, map, rng, tier);
    Dungeon {
        ruleset: rs.name.clone(),
        tier,
        floors,
        active: 0,
        id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(n: u32, region: usize) -> Entity {
        Entity {
            uid: [n, 0, 0, 0],
            region,
            pos: [n as f32, 0.0, 0.0],
        }
    }

    /// Two clusters (regions 0-1 and 2-3); one entrance each; the floor whose level path is a suffix
    /// of a cluster's region name takes that cluster's regions and entrance; proxies come from the
    /// groups, in the floor's regions only, under the counts.
    #[test]
    fn exits_and_proxies_follow_the_clusters() {
        let map = EndlessMap {
            region_names: [
                "a/x_a01.lvl",
                "a/x_a02.lvl",
                "levels/b/y_a01.lvl",
                "b/y_a02.lvl",
            ]
            .map(String::from)
            .to_vec(),
            neighbours: vec![vec![1], vec![0], vec![3], vec![2]],
            entrances: vec![entity(1, 0), entity(2, 2)],
            proxy_groups: vec![
                (
                    ProxyKind::Common,
                    (10..16).map(|n| entity(n, (n % 4) as usize)).collect(),
                ),
                (ProxyKind::Hero, vec![entity(30, 1), entity(31, 3)]),
            ],
            forced_floor: None,
        };
        let floor = |rec: &str, path: &str, boss| FloorState {
            floor: Floor {
                record: rec.into(),
                regions: vec![path.into()],
                boss,
            },
            regions: vec![],
            entrance: None,
            proxies: vec![],
            progress: 0.0,
            total: 0.0,
        };
        let mut floors = vec![
            floor("f1", "levels/b/y_a01.lvl", false),
            floor("f2", "x_a02.lvl", false),
            floor("f3", "z_a01.lvl", false),
        ];
        let mut rng = Rng(99);
        pick_exits(&mut floors, &map, &mut rng);
        assert_eq!(floors[0].entrance.as_ref().unwrap().uid[0], 2);
        assert_eq!(floors[0].regions.len(), 2);
        assert_eq!(floors[1].entrance.as_ref().unwrap().uid[0], 1);
        assert!(floors[2].entrance.is_none(), "no cluster has it: unusable");
        let rs = Ruleset {
            name: "r".into(),
            floors: 2,
            floor_list: vec![],
            boss_floor_list: vec![],
            common_proxies: 2,
            hero_proxies: 5,
            boss_proxies: 1,
            trap_proxies: 0,
            max_shrines: 0,
            nemesis_weights: vec![],
            shrine_weights: vec![],
            floor_total: None,
            timer: None,
            kill_count_bonus: None,
            kill_count_minimum: 0,
            kill_count_reset_ms: 0,
            bonus_time_ms: [0; 3],
            dying_penalty_ms: 0,
            health_gain_pct: 0.0,
            health_gain_challenge: 0,
            essence_chance: [0.0; CLASSES],
            essence_item: Default::default(),
            blacklisted_heroes: vec![],
            allowed_items: vec![],
            disable_monster_drops: false,
            difficulty_adjustment: None,
        };
        pick_proxies(&mut floors, &rs, &map, &mut rng, 1);
        for f in &floors[..2] {
            let common: Vec<_> = f
                .proxies
                .iter()
                .filter(|p| p.kind == ProxyKind::Common)
                .collect();
            assert_eq!(common.len(), 2, "the common count");
            assert!(
                f.proxies
                    .iter()
                    .all(|p| f.regions.contains(&p.entity.region))
            );
            let heroes = f
                .proxies
                .iter()
                .filter(|p| p.kind == ProxyKind::Hero)
                .count();
            assert_eq!(
                heroes, 1,
                "only one hero lies in the floor, though 5 are asked"
            );
        }
        assert!(floors[2].proxies.is_empty());
    }
}
