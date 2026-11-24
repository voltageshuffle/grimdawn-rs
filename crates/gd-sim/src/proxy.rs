//! Monster spawning from level proxies.
//!
//! A `Proxy` entity (records/proxies/area*/…) runs once, as soon as the nav
//! data of its region is loaded (`Proxy::UpdateSelf`):
//!
//! 1. `chanceToRun` (0 = always) gates the whole proxy (`RunProxy`).
//! 2. A pool is picked from `pool1..6` by `weight1..6`, among those whose
//!    `time1..6` is "Always" or matches day/night (`SelectPool`).
//! 3. The pool (`ProxyPool::SpawnMonsters`, loader `ProxyPool::Load`) spawns its creatures:
//!    - spawn/champion min/max get the difficulty's `gameproxies.dbr` values
//!      added (unless `ignoreGameBalance`), then pass through the
//!      `proxyPoolEquation` record's equations with `poolValue` = that value;
//!    - with chance `championChance` (≤ 0 = always): every `alwaysSpawnChampion`
//!      entry first, then `rand[championMin, championMax]` weighted picks, then
//!      picks until `championMin` monsters exist;
//!    - normal entries the same way, the count `rand[min(spawnMin, spawnMax),
//!      spawnMax]` *including* champions already spawned, topped up to spawnMin.
//!
//!    Weighted picks (`ProxyPool::PickEntry`) draw `IGenerate(0, Σw)` over the entries
//!    whose `[minPlayerLevel, maxPlayerLevel]` (defaults 0, 200) holds the
//!    player level and whose `limit` (default: the pool's raw spawnMax,
//!    championMax for champions) is not used up.
//! 4. Each monster's level is `rand[min, max]` of its `levelVarianceEquation`
//!    record's `min/maxVarianceEquationNormal` (a missing one: the area
//!    level), with `averagePlayerLevel` = the area level; never below 1
//!    (`ProxyPool::CreateEntry`).
//!
//! The area level (`Proxy::ClampedPlayerAverage`, set by
//! `Proxy::SetDesignerLimits`) is the player level plus the
//! area's offset, clamped to its `[min, max]` (0 = unbounded); the limits come
//! from sector layer 9 (`GameEngine::GetLevelLimits`), or for proxies with a
//! `difficultyLimitsFile` (only the main menu's) from that record. Pool
//! entries are still filtered by the raw player level.
use gd_formats::arz::Record;
use gd_formats::vfs::Database;
use gd_rules::Rng;
use gd_rules::mode::Mode;

#[derive(Debug, Clone, PartialEq)]
pub struct Spawn {
    pub record: String,
    pub level: i32,
    /// Picked from the pool's champion list. The game does nothing more with
    /// it: the entry is created like any other (`ProxyPool::CreateEntry`);
    /// being a champion comes from the record's `monsterClassification`.
    pub champion: bool,
}

/// What a proxy record says about placement.
#[derive(Debug, Clone, Default)]
pub struct ProxyInfo {
    pub chance_to_run: f32,
    /// Half-size of the square the monsters are scattered in.
    pub placement_extents: f32,
}

pub fn proxy_info(db: &Database, proxy: &str) -> Option<ProxyInfo> {
    let r = db.get(proxy)?;
    Some(ProxyInfo {
        chance_to_run: r.f32("chanceToRun").unwrap_or(0.0),
        placement_extents: r.f32("placementExtents").unwrap_or(5.0),
    })
}

/// `IGenerate(lo, hi)`: uniform integer in `[lo, hi]`.
fn irange(rng: &mut Rng, lo: i32, hi: i32) -> i32 {
    if hi <= lo {
        return lo;
    }
    // Widened: equation outputs saturate to `i32::MAX` / `MIN` (bad data: a
    // span past `u32`); identical to the engine's 32-bit modulus on stock data.
    let span = i64::from(hi) - i64::from(lo) + 1;
    (i64::from(lo) + i64::from(rng.next()) % span) as i32
}

/// A weight sum; saturating (bad data: weights summing past `i32::MAX`).
fn weight_sum(weights: impl Iterator<Item = i32>) -> i32 {
    weights.fold(0, i32::saturating_add)
}

/// Spawn-count ceiling (bad data: a `proxyPoolEquation` output of `i32::MAX`
/// would otherwise emit billions of spawns). Stock pools stay far below it.
const MAX_SPAWNS: i32 = 1000;

fn eval(rec: &Record, field: &str, src: &str, vars: &dyn Fn(&str) -> Option<f32>) -> Option<f32> {
    crate::stats::record_expr(rec, field, src).map(|e| e.eval(vars))
}

#[derive(Debug, Clone)]
struct Entry {
    name: String,
    weight: i32,
    variance: Option<String>,
    limit: i32,
    always: bool,
    min_level: i32,
    max_level: i32,
}

fn entries(r: &Record, prefix: &str, default_limit: i32) -> Vec<Entry> {
    let mut out = Vec::new();
    for i in 1..=20 {
        let key = |k: &str| format!("{k}{prefix}{i}");
        let Some(name) = r.str(&key("name")) else {
            continue;
        };
        let limit = r
            .i32(&key("limit"))
            .filter(|&l| l != 0)
            .unwrap_or(default_limit);
        out.push(Entry {
            name: name.to_string(),
            weight: r.i32(&key("weight")).unwrap_or(0),
            variance: r.str(&key("levelVarianceEquation")).map(str::to_string),
            limit,
            always: r.bool(&key("alwaysSpawn")),
            min_level: r.i32(&key("minPlayerLevel")).unwrap_or(0),
            max_level: r.i32(&key("maxPlayerLevel")).unwrap_or(200),
        });
    }
    out
}

fn eligible(e: &Entry, level: i32) -> bool {
    // The engine tests `limit != 0` (`ProxyPool::SpawnMonsters`); a negative limit (bad
    // data) would never count down to 0 and loop the always-spawn pass forever,
    // so only a positive limit is eligible.
    e.limit > 0 && level >= e.min_level && level <= e.max_level
}

fn pick(entries: &mut [Entry], level: i32, rng: &mut Rng) -> Option<usize> {
    let total = weight_sum(
        entries
            .iter()
            .filter(|e| eligible(e, level))
            .map(|e| e.weight),
    );
    if !entries.iter().any(|e| eligible(e, level)) {
        return None;
    }
    let roll = irange(rng, 0, total);
    let mut acc = 0;
    for (i, e) in entries.iter_mut().enumerate() {
        if eligible(e, level) {
            acc = i32::saturating_add(acc, e.weight);
            if roll <= acc {
                e.limit -= 1;
                return Some(i);
            }
        }
    }
    None
}

/// Difficulty column (0 normal, 1 elite, 2 ultimate) of a `a;b;c` table value.
fn by_difficulty(r: Option<&Record>, key: &str, difficulty: usize) -> f32 {
    r.map(|r| r.f32s(key))
        .and_then(|v| v.get(difficulty).or(v.first()).copied())
        .unwrap_or(0.0)
}

/// `Proxy::SetDesignerLimits`: the area level for `player_level` under the
/// area's `(min, max, offset)` limits.
pub fn area_level(player_level: i32, limits: Option<(u32, u32, i32)>) -> i32 {
    let Some((min, max, offset)) = limits else {
        return player_level;
    };
    let v = (player_level + offset).max(0) as u32;
    let v = if min != 0 && v < min {
        min
    } else if max != 0 {
        v.min(max)
    } else {
        v
    };
    v as i32
}

fn monster_level(db: &Database, e: &Entry, area_level: i32, rng: &mut Rng) -> i32 {
    let vars =
        |k: &str| (k.eq_ignore_ascii_case("averagePlayerLevel")).then_some(area_level as f32);
    let r = e.variance.as_deref().and_then(|v| db.get(v));
    let eq = |key: &str| {
        r.as_ref()
            .and_then(|r| r.str(key).and_then(|s| eval(r, key, s, &vars)))
            .map_or(area_level, |x| x as i32)
    };
    let (mut lo, hi) = (
        eq("minVarianceEquationNormal"),
        eq("maxVarianceEquationNormal"),
    );
    if lo > hi {
        lo = hi;
    }
    irange(rng, lo, hi).max(1)
}

/// Entry `index` of a challenge adjustment's spawn offsets (spawnMin,
/// spawnMax, championMin, championMax), entry 0 when `index` is past the
/// `spawnMinAdj` list; shorter other lists read 0 (`ChallengeAdjustment::Load`,
/// `ProxyPool::SpawnMonsters`).
fn challenge_spawn_adj(c: &Record, index: usize) -> [i32; 4] {
    let lists = [
        "spawnMinAdj",
        "spawnMaxAdj",
        "spawnChampionMinAdj",
        "spawnChampionMaxAdj",
    ]
    .map(|k| c.f32s(k));
    let n = lists[0].len();
    if n == 0 {
        return [0; 4];
    }
    let i = if index < n { index } else { 0 };
    lists.map(|l| l.get(i).map_or(0, |&v| v as i32))
}

/// Runs one proxy pool for a player of `player_level` in an area of
/// [`area_level`] `area`.
pub fn run_pool(
    db: &Database,
    pool: &str,
    player_level: i32,
    area: i32,
    mode: Mode,
    rng: &mut Rng,
) -> Vec<Spawn> {
    let difficulty = mode.difficulty;
    let Some(r) = db.get(pool) else {
        return Vec::new();
    };
    let raw_max = r.i32("spawnMax").unwrap_or(0);
    let raw_champ_max = r.i32("championMax").unwrap_or(0);
    let mut spawn_min = r.i32("spawnMin").unwrap_or(0);
    let mut spawn_max = raw_max;
    let mut champ_min = r.i32("championMin").unwrap_or(0);
    let mut champ_max = raw_champ_max;
    if !r.bool("ignoreGameBalance") {
        let g = db.get("records/game/gameproxies.dbr");
        let adj = |v: &mut i32, key: &str, modkey: &str| {
            *v = (*v as f32 + by_difficulty(g, key, difficulty)) as i32;
            let m = by_difficulty(g, modkey, difficulty);
            if m > 0.0 {
                *v = (m * 0.01 * *v as f32) as i32;
            }
        };
        adj(&mut spawn_min, "spawnMin", "spawnMinModifier");
        adj(&mut spawn_max, "spawnMax", "spawnMaxModifier");
        adj(&mut champ_min, "championMin", "championMinModifier");
        adj(&mut champ_max, "championMax", "championMaxModifier");
        // Challenge adjustment spawn offsets, added after the % modifiers
        // (`ProxyPool::SpawnMonsters`). `GameEngine::GetChallengeAdjustment`
        // picks by `GameInfo::GetMode` / `GetDifficulty` (the plain
        // 0-2 difficulty, no challenge/Veteran test): campaign (mode 0)
        // difficulty 0 / 1 → `gameengine.dbr` `challengeAdjustment`, 2 →
        // `gameascendant.dbr` `ultimateChallangeAdjustment` (Crucible, mode
        // 1, would take the survival adjustment). The entry is indexed by
        // `GameEngine::GetDifficultyRamp` = `GameInfo::GetChallenge` at game
        // start (`SetDifficultyRamp`, GrimDawn.exe `Game::LoadModMap`),
        // falling back to entry 0 when out of range; `ChallengeAdjustment::Load`
        // builds one entry per `spawnMinAdj` element (missing
        // max/champion elements = 0). The shipped records are scalars, so
        // entry 0 applies on every campaign difficulty, Veteran or not.
        let challenge = if mode.ramp.is_some() {
            // The Crucible's: the survival adjustment of the game difficulty (`survivalinfo.dbr`).
            let key = ["survivalAdjustmentNormal", "survivalAdjustmentElite"]
                .get(difficulty)
                .copied()
                .unwrap_or("survivalAdjustmentUltimate");
            db.get("records/game/gameengine.dbr")
                .and_then(|e| e.str("survivalRecord").and_then(|p| db.get(p)))
                .and_then(|i| i.str(key).and_then(|p| db.get(p)))
        } else if difficulty == 2 {
            crate::actor::ultimate_challenge(db)
        } else {
            db.get("records/game/gameengine.dbr")
                .and_then(|e| e.str("challengeAdjustment").and_then(|p| db.get(p)))
        };
        if let Some(c) = challenge {
            let [a, b, cmin, cmax] =
                challenge_spawn_adj(c, mode.ramp.unwrap_or(mode.challenge) as usize);
            spawn_min += a;
            spawn_max += b;
            champ_min += cmin;
            champ_max += cmax;
        }
    }
    if let Some(eq) = r.str("proxyPoolEquation").and_then(|p| db.get(p)) {
        let run = |v: &mut i32, key: &str| {
            if let Some(src) = eq.str(key) {
                let pv = *v as f32;
                let vars = move |k: &str| (k.eq_ignore_ascii_case("poolValue")).then_some(pv);
                if let Some(x) = eval(eq, key, src, &vars) {
                    *v = x as i32;
                }
            }
        };
        run(&mut spawn_min, "spawnMinEquation");
        run(&mut spawn_max, "spawnMaxEquation");
        run(&mut champ_min, "championMinEquation");
        run(&mut champ_max, "championMaxEquation");
    }
    for v in [
        &mut spawn_min,
        &mut spawn_max,
        &mut champ_min,
        &mut champ_max,
    ] {
        *v = (*v).min(MAX_SPAWNS);
    }

    let mut normal = entries(r, "", raw_max);
    let mut champs = entries(r, "Champion", raw_champ_max);
    let mut out: Vec<Spawn> = Vec::new();
    let emit =
        |list: &mut Vec<Entry>, i: usize, champion: bool, out: &mut Vec<Spawn>, rng: &mut Rng| {
            let e = &list[i];
            out.push(Spawn {
                record: e.name.clone(),
                level: monster_level(db, e, area, rng),
                champion,
            });
        };

    let chance = r.f32("championChance").unwrap_or(0.0);
    if chance <= 0.0 || rng.unit() * 100.0 <= chance {
        while let Some(i) = champs
            .iter()
            .position(|e| e.always && eligible(e, player_level))
        {
            champs[i].limit -= 1;
            emit(&mut champs, i, true, &mut out, rng);
        }
        let n = irange(rng, champ_min, champ_max);
        for _ in 0..n {
            let Some(i) = pick(&mut champs, player_level, rng) else {
                break;
            };
            emit(&mut champs, i, true, &mut out, rng);
        }
        while (out.len() as i32) < champ_min {
            let Some(i) = pick(&mut champs, player_level, rng) else {
                break;
            };
            emit(&mut champs, i, true, &mut out, rng);
        }
    }

    while let Some(i) = normal
        .iter()
        .position(|e| e.always && eligible(e, player_level))
    {
        normal[i].limit -= 1;
        emit(&mut normal, i, false, &mut out, rng);
    }
    let lo = spawn_min.max(0).min(spawn_max);
    let n = irange(rng, lo, spawn_max) - out.len() as i32;
    for _ in 0..n.max(0) {
        let Some(i) = pick(&mut normal, player_level, rng) else {
            break;
        };
        emit(&mut normal, i, false, &mut out, rng);
    }
    while (out.len() as i32) < lo {
        let Some(i) = pick(&mut normal, player_level, rng) else {
            break;
        };
        emit(&mut normal, i, false, &mut out, rng);
    }
    out
}

/// What a proxy run produced.
#[derive(Debug, Clone, Default)]
pub struct ProxyRun {
    pub monsters: Vec<Spawn>,
    /// Accessory objects (chests, ...): records placed like the monsters.
    pub accessories: Vec<String>,
}

/// Runs a proxy record: the chance to run, the pool choice, the pool, the
/// accessory pool (a trove at `trove_chance` %, [`crate::ascendant::trove_chance`]).
#[allow(clippy::too_many_arguments)]
pub fn run_proxy(
    db: &Database,
    proxy: &str,
    player_level: i32,
    area: i32,
    mode: Mode,
    trove_chance: f32,
    is_day: bool,
    rng: &mut Rng,
) -> ProxyRun {
    run_proxy_shared(
        db,
        proxy,
        player_level,
        area,
        mode,
        trove_chance,
        is_day,
        &mut None,
        rng,
    )
}

/// [`run_proxy`] for a proxy of a "Unified Entities" group (`Proxy::InitializePools`' use of
/// `GameEngine::GetEntityUnificationIndex` / `SetEntityUnificationIndex`): `pick` is
/// the group's stored index into this proxy's candidate pools (the ones its
/// time of day allows); with `None` the pool is chosen by weight and its index stored in `pick`. An index past
/// the candidate list falls back to the weighted choice.
#[allow(clippy::too_many_arguments)]
pub fn run_proxy_shared(
    db: &Database,
    proxy: &str,
    player_level: i32,
    area: i32,
    mode: Mode,
    trove_chance: f32,
    is_day: bool,
    pick: &mut Option<usize>,
    rng: &mut Rng,
) -> ProxyRun {
    let mut out = ProxyRun::default();
    let Some(r) = db.get(proxy) else { return out };
    let chance = r.f32("chanceToRun").unwrap_or(0.0);
    if chance > 0.0 && rng.unit() * 100.0 > chance {
        return out;
    }
    let want = if is_day { 1 } else { 2 };
    let pools: Vec<(String, i32)> = (1..=6)
        .filter_map(|i| {
            let pool = r.str(&format!("pool{i}"))?;
            let time = match r.str(&format!("time{i}")).unwrap_or("Always") {
                t if t.eq_ignore_ascii_case("always") => 0,
                t if t.eq_ignore_ascii_case("day") => 1,
                _ => 2,
            };
            (time == 0 || time == want)
                .then(|| (pool.to_string(), r.i32(&format!("weight{i}")).unwrap_or(0)))
        })
        .collect();
    let total = weight_sum(pools.iter().map(|p| p.1));
    // The group's stored choice first (it does not roll), else the weighted roll.
    let chosen = match pick.filter(|i| *i < pools.len()) {
        Some(i) => Some(i),
        None => {
            let roll = irange(rng, 0, total);
            let mut acc = 0;
            let found = pools.iter().position(|(_, w)| {
                acc = i32::saturating_add(acc, *w);
                roll <= acc
            });
            *pick = found;
            found
        }
    };
    if let Some(i) = chosen {
        out.monsters = run_pool(db, &pools[i].0, player_level, area, mode, rng);
    }
    out.accessories
        .extend(run_accessory(db, r, mode.difficulty, trove_chance, rng));
    out
}

/// An endless-dungeon proxy's pool choice (`ProxyEndless::InitializePools`): the pool
/// columns follow the game difficulty (Normal `poolN` / `weightN`, Elite `poolEpicN` / `weightEpicN`,
/// Ultimate `poolLegendaryN` / `weightLegendaryN`, N up to 50; an empty column falls back to the one below);
/// one pool is chosen by weight (`Proxy::SelectPool`) and run like a `Proxy`'s ([`run_pool`]). There is no
/// time-of-day or `chanceToRun` gate.
///
/// Unification (same function): a proxy of an "Endless Dungeon
/// Proxies <type>" group asks `GameEngine::GetEntityUnificationIndex` for the index stored under
/// the name of its "Unified Entities" group; `pick` is that index (`Some`) or `None`. A stored index is used
/// as is, into THIS proxy's candidate list (`Proxy::GetPoolName`: past the list the name is empty,
/// the proxy gets no pool and spawns nothing); without one the weighted pick is made and its index stored in
/// `pick` (`SetEntityUnificationIndex`; the store lives for the session). In stock data no
/// `ProxyEndless` is in a "Unified Entities" group (checked against `world001.map`), so every proxy picks its own.
/// The store's key on `Set` is the name of the first group holding the proxy (no type check); for every stock
/// unified member that first group is its "Unified Entities" one, so the caller keys both by that name. PORT: the
/// caller (`gd_game::monsters`) passes the stored index for any `ProxyEndless` of a unified group; the engine only
/// asks for one of an "Endless Dungeon Proxies" group (type != 0), which no stock data tells apart.
pub fn run_endless_proxy(
    db: &Database,
    proxy: &str,
    player_level: i32,
    area: i32,
    mode: Mode,
    pick: &mut Option<usize>,
    rng: &mut Rng,
) -> Vec<Spawn> {
    let Some(r) = db.get(proxy) else {
        return Vec::new();
    };
    let columns: &[(&str, &str)] = match mode.difficulty {
        0 => &[("pool", "weight")],
        1 => &[("poolEpic", "weightEpic"), ("pool", "weight")],
        _ => &[
            ("poolLegendary", "weightLegendary"),
            ("poolEpic", "weightEpic"),
            ("pool", "weight"),
        ],
    };
    let pools: Vec<(String, i32)> = columns
        .iter()
        .map(|(p, w)| {
            (1..=50)
                .filter_map(|i| {
                    let pool = r.str(&format!("{p}{i}"))?;
                    Some((pool.to_owned(), r.i32(&format!("{w}{i}")).unwrap_or(0)))
                })
                .collect::<Vec<_>>()
        })
        .find(|v| !v.is_empty())
        .unwrap_or_default();
    let chosen = match *pick {
        Some(i) => pools.get(i),
        None => {
            let total = weight_sum(pools.iter().map(|p| p.1));
            let roll = irange(rng, 0, total);
            let mut acc = 0;
            let found = pools.iter().position(|(_, w)| {
                acc = i32::saturating_add(acc, *w);
                roll <= acc
            });
            *pick = found;
            found.map(|i| &pools[i])
        }
    };
    match chosen {
        Some((pool, _)) => run_pool(db, pool, player_level, area, mode, rng),
        None => Vec::new(),
    }
}

/// A shrine proxy's accessory (`ProxyEndless::InitializePools` / `ProxyAccessoryPool::RunPool`,
/// notes): one of the `accessoryN` pools (N = 1..5) by `accessoryWeightN` (the Epic / Legendary
/// columns on those difficulties, an empty column falls back to the one below), then the pool's
/// `fixedItemName1..10` like [`run_accessory`]. A proxy without accessory pools (a monster proxy) gives nothing.
/// Rolled without the dungeon's used pools: [`pick_endless_accessory`] is the one the generator runs at activation.
pub fn run_endless_accessory(
    db: &Database,
    proxy: &str,
    difficulty: usize,
    rng: &mut Rng,
) -> Option<String> {
    pick_endless_accessory(db, proxy, difficulty, &[], rng).and_then(|(_, item)| item)
}

/// [`run_endless_accessory`] as `ProxyEndless::InitializeAccessoryPool` runs it: the pools in
/// `used` (the dungeon's set, `DungeonGenerator.blacklistedShrinePools`, `GetBlacklistedShrinePools`) are not
/// candidates; returns the chosen
/// pool's name and its item (None when the pool's `fixedItemChance` failed: the pool is still the proxy's).
/// `InitializeAccessoryPool` / `SelectAccessory`: with
/// every pool used the candidate list is empty, `SelectAccessory` returns -1 and no accessory pool is made, so
/// nothing is returned (the proxy places no shrine; the exclusion is never dropped).
pub fn pick_endless_accessory(
    db: &Database,
    proxy: &str,
    difficulty: usize,
    used: &[String],
    rng: &mut Rng,
) -> Option<(String, Option<String>)> {
    let r = db.get(proxy)?;
    let columns: &[(&str, &str)] = match difficulty {
        0 => &[("accessory", "accessoryWeight")],
        1 => &[
            ("accessoryEpic", "accessoryWeightEpic"),
            ("accessory", "accessoryWeight"),
        ],
        _ => &[
            ("accessoryLegendary", "accessoryWeightLegendary"),
            ("accessoryEpic", "accessoryWeightEpic"),
            ("accessory", "accessoryWeight"),
        ],
    };
    let pools: Vec<(String, i32)> = columns
        .iter()
        .map(|(p, w)| {
            (1..=5)
                .filter_map(|i| {
                    let pool = r.str(&format!("{p}{i}"))?;
                    Some((pool.to_owned(), r.i32(&format!("{w}{i}")).unwrap_or(0)))
                })
                .collect::<Vec<_>>()
        })
        .find(|v| !v.is_empty())?;
    let pools: Vec<(String, i32)> = pools
        .into_iter()
        .filter(|(p, _)| !used.iter().any(|u| u.eq_ignore_ascii_case(p)))
        .collect();
    let roll = irange(rng, 0, weight_sum(pools.iter().map(|p| p.1)));
    let mut acc = 0;
    let (pool_name, _) = pools.iter().find(|(_, w)| {
        acc = i32::saturating_add(acc, *w);
        roll <= acc
    })?;
    let pool_name = pool_name.clone();
    let Some(pool) = db.get(&pool_name) else {
        return Some((pool_name, None));
    };
    if irange(rng, 0, 100) > pool.i32("fixedItemChance").unwrap_or(0) {
        return Some((pool_name, None));
    }
    let items: Vec<(&str, i32)> = (1..=10)
        .filter_map(|i| {
            Some((
                pool.str(&format!("fixedItemName{i}"))?,
                pool.i32(&format!("fixedItemWeight{i}")).unwrap_or(0),
            ))
        })
        .collect();
    let roll = irange(rng, 0, weight_sum(items.iter().map(|i| i.1)));
    let mut acc = 0;
    let item = items
        .iter()
        .find(|(_, w)| {
            acc = i32::saturating_add(acc, *w);
            acc >= roll
        })
        .map(|(n, _)| n.to_string());
    Some((pool_name, item))
}

/// The accessory pool (`Proxy::InitializePools`,
/// `ProxyAccessoryPool::RunPool`): `accessoryLegendary1` on
/// Ultimate, `accessoryEpic1` on Elite/Ultimate, else `accessory1` (the
/// first non-empty one). The pool record gives `fixedItemChance`
/// (`IGenerate(0, 100) <= chance`) and `fixedItemName/Weight1..10`
/// (`IGenerate(0, Σw)`, first cumulative weight ≥ the roll). An accessory
/// that names a `Proxy` instead of a pool has no fixed items: nothing.
fn run_accessory(
    db: &Database,
    proxy: &Record,
    difficulty: usize,
    trove_chance: f32,
    rng: &mut Rng,
) -> Option<String> {
    let mut keys = Vec::new();
    if difficulty == 2 {
        keys.push("accessoryLegendary1");
    }
    if difficulty >= 1 {
        keys.push("accessoryEpic1");
    }
    keys.push("accessory1");
    let pool = db.get(keys.into_iter().find_map(|k| proxy.str(k))?)?;
    // A treasure trove: the pool's `ascendantFixedItem` instead
    // (`Proxy::RunProxy` picks the ascendant list on the trove roll;
    // `crate::ascendant`).
    let trove = pool
        .str("ascendantFixedItem")
        .filter(|t| !t.is_empty())
        .map(str::to_owned);
    let trove = trove.filter(|_| crate::ascendant::roll(rng, trove_chance));
    if irange(rng, 0, 100) > pool.i32("fixedItemChance").unwrap_or(0) {
        return None;
    }
    if trove.is_some() {
        return trove;
    }
    let items: Vec<(&str, i32)> = (1..=10)
        .filter_map(|i| {
            Some((
                pool.str(&format!("fixedItemName{i}"))?,
                pool.i32(&format!("fixedItemWeight{i}")).unwrap_or(0),
            ))
        })
        .collect();
    let roll = irange(rng, 0, weight_sum(items.iter().map(|i| i.1)));
    let mut acc = 0;
    items
        .iter()
        .find(|(_, w)| {
            acc = i32::saturating_add(acc, *w);
            acc >= roll
        })
        .map(|(n, _)| n.to_string())
}
