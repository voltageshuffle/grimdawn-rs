//! Loot generation (Game.dll):
//! - `LootLoader::Load(table, location, 6, difficulty)` 0x180362e40: up to six
//!   `loot<Slot>Item<n>` (string array indexed by difficulty) weighted by
//!   `chanceToEquip<Slot>Item<n>`; `GetLootName` 0x180362fe0 draws
//!   `r % (total + 1)` and takes the first entry with `r <= cumulative`.
//! - `LootMasterTable::GetLootName` 0x1803637b0 (`lootName/lootWeight` 1..100,
//!   cumulative, `r % total`, first `r < cum`, recursion depth 5), each entry a
//!   `LevelTable` (`levels`/`records`, last `level >= levels[i]`, `sub_180355D40`).
//! - `LootItemTable_DynWeight` 0x180359790/0x180359d40/0x18035a010: entries
//!   until the first gap, item level from each record's `itemLevel`, valid range
//!   from `min/maxItemLevelEquation` (`(int)(max(x,1)+0.5)`), fallback to the
//!   single highest level below the minimum, `forceHighestLevel`,
//!   `disableLevelLimits`, bell-curve weights around `targetLevelEquation`
//!   (`w = w * (int)bellSlope[|lvl-target|] / 100`, unchanged past the end of
//!   the array; no bell array → total stays 0 → no drop), `r % total`.
//! - `LootItemTableRandomizer_Dyn` 0x18035ca60/0x180361c20: affix tables
//!   `prefix|suffix|rarePrefix|rareSuffixTableName<n>` (1..18, stop at first
//!   missing) filtered by `TableLevelMin/Max` around the target level, the ten
//!   `bothPrefixSuffix`.. weights (+% weight modifiers), one draw selects the
//!   combination, then per affix `r % total` over tables and
//!   `LootRandomizerTable_Dynamic` (`randomizerName/Weight/LevelMin/LevelMax`
//!   1..150, [`DYNAMIC_RANDOMIZER_ENTRIES`], `r % (total + 1)`, `r <= cum`).
//! - A plain `LootRandomizerTable` (no level limits: a relic's completion
//!   bonus, `LootItemTableRandomizer`, the Tinker) reads only entries 1..70
//!   ([`RANDOMIZER_ENTRIES`]); stock affix tables list up to 100.
//! - Monster drops: `Character::LoadLoot` 0x180070030 /
//!   `CreateItemFromLootTable` 0x180070760 and
//!   `ControllerMonster::GetDropMiscItems` 0x180139b60: per slot
//!   `(float)(int)r * 4.6519563e-8 + 0.1 <= chanceToEquip<Slot>`, then the slot's
//!   LootLoader; the item seed is the next draw after the name.
//! - Rarity weights per source: [`weight_mods`] (monsters, containers) and
//!   [`LootParams::no_broken`] (containers, markets).
//! - Ground items: the classification ([`rarity`]), the landing sound ([`drop_sound`]), the
//!   gold bonus ([`gold_with_bonus`]) and the auto-pickup radius ([`auto_pickup_radius`]).
use super::db::{Fields, ItemDb};
use super::resolve::{Classification, ResolvedItem};
use crate::combat::Rng;
use crate::expr::Expr;
use gd_formats::arz::Record;
use gd_formats::save::item::Item;

/// Entries `LootRandomizerTable::Load` @0x180363d30 reads (70
/// iterations over fixed 20-byte name tables, `randomizerWeight1..70` at
/// 0x1807762a0 and `randomizerName1..70` at 0x180775d20; a weight > 0 with a
/// non-empty name joins, the total adds the weight). The loader behind
/// `LootRandomizerTable::ManualLoad` @0x180363f90, which
/// `ItemArtifact::InitializeItem`, `LootItemTableRandomizer::Get{Prefix,
/// Suffix,Broken}Table`, `EnchanterTinkerConfigCmd::Execute` and
/// `LootTable::ValidateSelectRandomizerRecursive` use: entries 71 and up of a
/// table never roll there.
pub const RANDOMIZER_ENTRIES: usize = 70;

/// Entries `LootRandomizerTable_Dynamic::Load` @0x180364150 reads (
/// 150 iterations over a name table it builds once, `randomizerName{i}`,
/// `…Weight{i}`, `…LevelMin{i}`, `…LevelMax{i}` for 1..150). The loader
/// behind `LootRandomizerTable_Dynamic::ManualLoad` @0x18036a1f0, which the
/// affix picks of `LootItemTableRandomizer_Dyn` and
/// `GameEngine::GetAscendedItemName` use.
pub const DYNAMIC_RANDOMIZER_ENTRIES: usize = 150;

/// Context a loot table is evaluated in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LootParams {
    /// `parentLevel` (monster / container level).
    pub parent_level: u32,
    /// `averagePlayerLevel`.
    pub player_level: u32,
    /// 0 Normal, 1 Elite, 2 Ultimate (`gameDifficulty`, string array index).
    pub difficulty: u32,
    /// `RandomizerWeightModifiers`: +% on none, both, prefixOnly, suffixOnly,
    /// broken, rareBoth, rarePrefixOnly, rareSuffixOnly, rarePrefixNormalSuffix,
    /// normalPrefixRareSuffix.
    pub weight_mods: [i32; 10],
    /// The Ascendant `doubleRareWeightModifier` ([`crate::ascendant::Ascendant`]).
    pub double_rare: f32,
    /// The Ascendant `equippedItemDropModifier`.
    pub equipped_drop: f32,
    /// Ultimate Veteran ([`crate::mode::Mode::ultimate_veteran`]): containers
    /// take `lootTable[3]`.
    pub ultimate_veteran: bool,
    /// `LootBase::SetNoBrokenItems` (+0x30): the randomizers' `brokenOnly`
    /// weight is set to 0 after the modifiers (inlined in
    /// `LootItemTable_DynWeight::GetLootName` 0x180359d40 and
    /// `LootItemTable_FixedWeight::GetLootName` 0x18035b100);
    /// `LootMasterTable::GetLootName` passes it to the child table. Only
    /// markets set it (`MarketMiniMart::Refresh`).
    pub no_broken: bool,
}

impl LootParams {
    pub fn new(parent_level: u32, player_level: u32, difficulty: u32) -> Self {
        Self {
            parent_level,
            player_level,
            difficulty,
            weight_mods: [0; 10],
            double_rare: 0.0,
            equipped_drop: 0.0,
            ultimate_veteran: false,
            no_broken: false,
        }
    }

    pub fn at_level(level: u32) -> Self {
        Self::new(level, level, 0)
    }

    /// With the session's mode and altar formulas.
    pub fn with_session(self, mode: crate::mode::Mode, a: &crate::ascendant::Ascendant) -> Self {
        Self {
            double_rare: a.double_rare,
            equipped_drop: a.equipped_drop,
            ultimate_veteran: mode.ultimate_veteran(),
            ..self
        }
    }
}

/// A generated item: what the game stores in an `ItemReplicaInfo`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LootRoll {
    pub base: String,
    pub prefix: String,
    pub suffix: String,
    pub seed: u32,
}

impl LootRoll {
    pub fn to_item(&self) -> Item {
        Item {
            base_name: self.base.clone(),
            prefix_name: self.prefix.clone(),
            suffix_name: self.suffix.clone(),
            seed: self.seed,
            stack_count: 1,
            ..Default::default()
        }
    }
}

#[derive(Default)]
struct Names {
    base: String,
    prefix: String,
    suffix: String,
}

fn level_eq(rec: &Record, key: &str, p: &LootParams) -> Option<f32> {
    let src = rec.str(key)?;
    let e = Expr::parse(src).ok()?;
    Some(e.eval(&|v| match v {
        "minPlayerLevel" | "maxPlayerLevel" | "numberOfPlayers" => Some(1.0),
        "gameDifficulty" => Some(p.difficulty as f32),
        "parentLevel" => Some(p.parent_level as f32),
        "averagePlayerLevel" => Some(p.player_level as f32),
        _ => None,
    }))
}

fn round_level(x: f32) -> u32 {
    (x.max(1.0) + 0.5) as i32 as u32
}

/// Weighted list with cumulative weights.
fn cumulative<'a>(it: impl Iterator<Item = (&'a str, u32)>) -> Vec<(String, u32)> {
    let mut sum = 0u32;
    it.map(|(n, w)| {
        sum = sum.wrapping_add(w);
        (n.to_string(), sum)
    })
    .collect()
}

/// `r % total` (total 0 → 1), first entry with `r < cum`.
fn pick_lt<S: AsRef<str>>(list: &[(S, u32)], rng: &mut Rng) -> Option<String> {
    if list.is_empty() {
        return None;
    }
    let total = list.last().unwrap().1.max(1);
    let r = rng.next() % total;
    list.iter()
        .find(|(_, c)| r < *c)
        .map(|(n, _)| n.as_ref().to_string())
}

/// `r % (total + 1)`, first entry with `r <= cum` (weights, not cumulative).
fn pick_le<S: AsRef<str>>(list: &[(S, u32)], total: u32, rng: &mut Rng) -> Option<String> {
    let m = total.wrapping_add(1);
    let r = rng.next() % if m == 0 { 1 } else { m };
    let mut cum = 0u32;
    for (n, w) in list {
        cum = cum.wrapping_add(*w);
        if r <= cum {
            return Some(n.as_ref().to_string());
        }
    }
    None
}

/// `{a}{b}{i}` in `buf`: field names built without a fresh allocation per key.
fn key<'k>(buf: &'k mut String, a: &str, b: &str, i: usize) -> &'k str {
    use std::fmt::Write;
    buf.clear();
    let _ = write!(buf, "{a}{b}{i}");
    buf
}

/// `LootRandomizerTable_Dynamic::Load` + `GetRandomizerName`.
pub(crate) fn randomizer_pick(db: &ItemDb, table: &str, level: u32, rng: &mut Rng) -> String {
    let Some(rec) = db.get(table) else {
        rng.next();
        return String::new();
    };
    let mut buf = String::with_capacity(32);
    let mut list: Vec<(&str, u32)> = Vec::new();
    let mut total = 0u32;
    for i in 1..=DYNAMIC_RANDOMIZER_ENTRIES {
        let w = rec.int(key(&mut buf, "randomizerWeight", "", i));
        if w <= 0 {
            continue;
        }
        let name = rec.string(key(&mut buf, "randomizerName", "", i));
        if name.is_empty() {
            continue;
        }
        let (lo, hi) = (
            rec.int(key(&mut buf, "randomizerLevelMin", "", i)),
            rec.int(key(&mut buf, "randomizerLevelMax", "", i)),
        );
        if (level as i32) < lo || (level as i32) > hi {
            continue;
        }
        list.push((name, w as u32));
        total = total.wrapping_add(w as u32);
    }
    pick_le(&list, total, rng).unwrap_or_default()
}

/// `LootItemTableRandomizer_Dyn` state.
struct Randomizer {
    level: u32,
    prefix: Vec<(String, u32)>,
    suffix: Vec<(String, u32)>,
    rare_prefix: Vec<(String, u32)>,
    rare_suffix: Vec<(String, u32)>,
    broken: String,
    /// both, none, prefixOnly, suffixOnly, broken, rareBoth, rarePrefixOnly,
    /// rareSuffixOnly, rarePrefixNormalSuffix, normalPrefixRareSuffix
    w: [u32; 10],
}

impl Randomizer {
    fn load(rec: &Record, level: u32, level_filter: bool) -> Self {
        let mut buf = String::with_capacity(32);
        let mut list = |kind: &str| {
            let mut v = Vec::new();
            for i in 1..=18 {
                let w = rec.int(key(&mut buf, kind, "TableWeight", i));
                if w <= 0 {
                    break;
                }
                let name = rec.string(key(&mut buf, kind, "TableName", i));
                if name.is_empty() {
                    break;
                }
                let (lo, hi) = (
                    rec.int(key(&mut buf, kind, "TableLevelMin", i)),
                    rec.int(key(&mut buf, kind, "TableLevelMax", i)),
                );
                if !level_filter || ((level as i32) >= lo && (level as i32) <= hi) {
                    v.push((name, w as u32));
                }
            }
            cumulative(v.into_iter())
        };
        let w = [
            "bothPrefixSuffix",
            "noPrefixNoSuffix",
            "prefixOnly",
            "suffixOnly",
            "brokenOnly",
            "rareBothPrefixSuffix",
            "rarePrefixOnly",
            "rareSuffixOnly",
            "rarePrefixNormalSuffix",
            "normalPrefixRareSuffix",
        ]
        .map(|k| rec.int(k).max(0) as u32);
        Self {
            level,
            prefix: list("prefix"),
            suffix: list("suffix"),
            rare_prefix: list("rarePrefix"),
            rare_suffix: list("rareSuffix"),
            broken: rec.string("brokenTableName1").to_string(),
            w,
        }
    }

    /// `SetWeightModifiers`: modifiers are indexed (none, both, ...) while the
    /// weights are stored (both, none, ...).
    fn modify(&mut self, m: &[i32; 10]) {
        let mi = [1, 0, 2, 3, 4, 5, 6, 7, 8, 9];
        for (i, w) in self.w.iter_mut().enumerate() {
            *w = w.wrapping_add((w.wrapping_mul(m[mi[i]] as u32)) / 100);
        }
    }

    fn table(&self, db: &ItemDb, list: &[(String, u32)], rng: &mut Rng) -> String {
        match pick_lt(list, rng) {
            Some(t) => randomizer_pick(db, &t, self.level, rng),
            None => String::new(),
        }
    }

    fn names(&self, db: &ItemDb, rng: &mut Rng, out: &mut Names) {
        let w = &self.w;
        let sum: u32 = w.iter().fold(0u32, |a, b| a.wrapping_add(*b));
        let r = rng.next() % sum.max(1);
        if sum == 0 {
            return;
        }
        let order = [0usize, 2, 3, 4, 5, 6, 7, 8, 9];
        let mut cum = 0u32;
        for k in order {
            cum = cum.wrapping_add(w[k]);
            if r < cum {
                if w[k] == 0 {
                    return;
                }
                match k {
                    0 => {
                        out.prefix = self.table(db, &self.prefix, rng);
                        out.suffix = self.table(db, &self.suffix, rng);
                    }
                    2 => out.prefix = self.table(db, &self.prefix, rng),
                    3 => out.suffix = self.table(db, &self.suffix, rng),
                    4 => {
                        if !self.broken.is_empty() {
                            out.prefix = randomizer_pick(db, &self.broken, self.level, rng);
                        }
                    }
                    5 => {
                        out.prefix = self.table(db, &self.rare_prefix, rng);
                        out.suffix = self.table(db, &self.rare_suffix, rng);
                    }
                    6 => out.prefix = self.table(db, &self.rare_prefix, rng),
                    7 => out.suffix = self.table(db, &self.rare_suffix, rng),
                    8 => {
                        out.prefix = self.table(db, &self.rare_prefix, rng);
                        out.suffix = self.table(db, &self.suffix, rng);
                    }
                    _ => {
                        out.prefix = self.table(db, &self.prefix, rng);
                        out.suffix = self.table(db, &self.rare_suffix, rng);
                    }
                }
                return;
            }
        }
    }
}

/// One affix from a `LootItemTable_DynWeight` record's prefix / suffix (or
/// rare) tables at `level`: `LoadFromDatabase` + `Get[Rare]PrefixTable` /
/// `Get[Rare]SuffixTable` with the altar overrides off, as the Inventor's
/// reroll panel does (GrimDawn.exe `sub_1401B7CD0`).
pub fn affix_from_table(
    db: &ItemDb,
    table: &str,
    level: u32,
    prefix: bool,
    rare: bool,
    rng: &mut Rng,
) -> String {
    let Some(rec) = db.get(table) else {
        return String::new();
    };
    let rz = Randomizer::load(rec, level, true);
    let list = match (prefix, rare) {
        (true, false) => &rz.prefix,
        (true, true) => &rz.rare_prefix,
        (false, false) => &rz.suffix,
        (false, true) => &rz.rare_suffix,
    };
    rz.table(db, list, rng)
}

fn dyn_weight(db: &ItemDb, rec: &Record, p: &LootParams, rng: &mut Rng, out: &mut Names) {
    struct E {
        name: String,
        level: u32,
        weight: u32,
        valid: bool,
    }
    let mut entries = Vec::new();
    for i in 1..=100 {
        let name = rec.string(&format!("lootName{i}"));
        let w = rec.int(&format!("lootWeight{i}"));
        if name.is_empty() || w == 0 {
            break;
        }
        let level = db.get(name).map_or(0, |r| r.int("itemLevel")) as u32;
        entries.push(E {
            name: name.to_string(),
            level,
            weight: w as u32,
            valid: false,
        });
    }
    let (Some(min_e), Some(max_e)) = (
        level_eq(rec, "minItemLevelEquation", p),
        level_eq(rec, "maxItemLevelEquation", p),
    ) else {
        return;
    };
    if entries.is_empty() {
        return;
    }
    let (lo, hi) = (round_level(min_e), round_level(max_e));
    if rec.bool("disableLevelLimits") {
        entries.iter_mut().for_each(|e| e.valid = true);
    } else {
        let mut any = false;
        for e in &mut entries {
            if e.level >= lo && e.level <= hi {
                e.valid = true;
                any = true;
            }
        }
        if !any {
            let mut best: Option<usize> = None;
            let mut best_lvl = 0;
            for (i, e) in entries.iter().enumerate() {
                if e.level < lo && e.level > best_lvl {
                    best_lvl = e.level;
                    best = Some(i);
                }
            }
            if let Some(i) = best {
                entries[i].valid = true;
            }
        }
        if entries.len() > 1 && rec.bool("forceHighestLevel") {
            let top = entries
                .iter()
                .filter(|e| e.valid)
                .map(|e| e.level)
                .max()
                .unwrap_or(0);
            entries
                .iter_mut()
                .for_each(|e| e.valid = e.valid && e.level == top);
        }
    }
    let target = round_level(level_eq(rec, "targetLevelEquation", p).unwrap_or(0.0));
    let bell = rec.f32s("bellSlope");
    let mut total = 0u32;
    if !bell.is_empty() {
        for e in entries.iter_mut().filter(|e| e.valid) {
            let d = ((e.level as i32 - target as i32) as f32).abs() as i32;
            if (d as usize) < bell.len() {
                e.weight = e.weight.wrapping_mul(bell[d as usize] as i32 as u32) / 100;
            }
            total = total.wrapping_add(e.weight);
        }
    }
    if total == 0 {
        return;
    }
    let r = rng.next() % total;
    let mut cum = 0u32;
    let Some(e) = entries.iter().filter(|e| e.valid).find(|e| {
        cum = cum.wrapping_add(e.weight);
        r < cum
    }) else {
        return;
    };
    out.base = e.name.clone();
    let mut rz = Randomizer::load(rec, target, true);
    rz.modify(&p.weight_mods);
    // The altar flag (on except at the reroll panel): rareBothPrefixSuffix
    // × (1 + the Ascendant `doubleRareWeightModifier`),
    // `SetWeightModifiers`.
    let double_rare = p.double_rare;
    if double_rare != 0.0 {
        rz.w[5] = ((double_rare + 1.0) * rz.w[5] as f32) as i32 as u32;
    }
    if p.no_broken {
        rz.w[4] = 0;
    }
    rz.names(db, rng, out);
}

fn fixed_weight(db: &ItemDb, rec: &Record, p: &LootParams, rng: &mut Rng, out: &mut Names) {
    let mut list = Vec::new();
    for i in 1..=30 {
        let w = rec.int(&format!("lootWeight{i}"));
        let n = rec.string(&format!("lootName{i}"));
        if w > 0 && !n.is_empty() {
            list.push((n, w as u32));
        }
    }
    let list = cumulative(list.into_iter());
    let Some(n) = pick_lt(&list, rng) else { return };
    out.base = n;
    fixed_randomizer_names(db, rec, p, rng, out);
}

/// `LootItemTableRandomizer` (non-dynamic), loaded from the same record by
/// `LootItemTable_FixedWeight::GetLootName` (`LoadFromDatabase`
/// @0x18035ba90, `GetRandomizerNames` @0x18035c040, `GetPrefixTable`
/// @0x18035c170). Its own fields: up to 15 `prefix`/`suffix`/`broken`
/// `RandomizerName{i}` / `RandomizerWeight{i}` (no level limits) with a
/// `…RandomizerChance` gate (`r / 2^31 <= chance`), and the five weights
/// both / none / prefixOnly / suffixOnly / brokenOnly with the weight
/// modifiers applied inline. The picked `LootRandomizerTable` is rolled
/// without a level filter (`ManualLoad` + `GetRandomizerName`). No stock
/// record uses this class.
fn fixed_randomizer_names(
    db: &ItemDb,
    rec: &Record,
    p: &LootParams,
    rng: &mut Rng,
    out: &mut Names,
) {
    let list = |kind: &str| {
        let mut v = Vec::new();
        for i in 1..=15 {
            let w = rec.int(&format!("{kind}RandomizerWeight{i}"));
            let n = rec.string(&format!("{kind}RandomizerName{i}"));
            if w > 0 && !n.is_empty() {
                v.push((n, w as u32));
            }
        }
        cumulative(v.into_iter())
    };
    let pick = |kind: &str, rng: &mut Rng| -> String {
        let l = list(kind);
        if l.is_empty() {
            return String::new();
        }
        let chance = rec.float(&format!("{kind}RandomizerChance"));
        if (rng.next() as i32) as f32 * 4.656_613e-10 > chance {
            return String::new();
        }
        let Some(t) = pick_lt(&l, rng) else {
            return String::new();
        };
        // A missing / non-`LootRandomizerTable` record still rolls once.
        let table = db.get(&t).filter(|r| r.class == "LootRandomizerTable");
        let mut entries = Vec::new();
        let mut total = 0u32;
        for i in 1..=RANDOMIZER_ENTRIES {
            let Some(table) = table.as_ref() else { break };
            let w = table.int(&format!("randomizerWeight{i}"));
            let n = table.string(&format!("randomizerName{i}"));
            if w > 0 && !n.is_empty() {
                entries.push((n.to_string(), w as u32));
                total = total.wrapping_add(w as u32);
            }
        }
        if entries.is_empty() {
            rng.next();
            return String::new();
        }
        pick_le(&entries, total, rng).unwrap_or_default()
    };
    let m = &p.weight_mods;
    let modded = |k: &str, mi: usize| {
        let w = rec.int(k).max(0) as u32;
        w.wrapping_add(w.wrapping_mul(m[mi] as u32) / 100)
    };
    let (both, none, pre, suf, broken) = (
        modded("bothPrefixSuffix", 1),
        modded("noPrefixNoSuffix", 0),
        modded("prefixOnly", 2),
        modded("suffixOnly", 3),
        if p.no_broken {
            0
        } else {
            modded("brokenOnly", 4)
        },
    );
    let total = [both, none, pre, suf, broken]
        .iter()
        .fold(0u32, |a, b| a.wrapping_add(*b));
    let r = rng.next() % if total == 0 { 1 } else { total };
    if r < both {
        out.prefix = pick("prefix", rng);
        out.suffix = pick("suffix", rng);
    } else if r < both.wrapping_add(pre) {
        out.prefix = pick("prefix", rng);
    } else if r < both.wrapping_add(pre).wrapping_add(suf) {
        out.suffix = pick("suffix", rng);
    } else if r < both
        .wrapping_add(pre)
        .wrapping_add(suf)
        .wrapping_add(broken)
    {
        out.prefix = pick("broken", rng);
    }
}

fn level_table_pick(rec: &Record, level: u32) -> Option<String> {
    let levels = rec.f32s("levels");
    let records = rec.strs("records");
    let mut idx = None;
    for (i, l) in levels.iter().enumerate() {
        if (level as i32) < *l as i32 {
            break;
        }
        idx = Some(i);
    }
    records.get(idx?).map(|s| s.to_string())
}

fn loot_name(
    db: &ItemDb,
    name: &str,
    p: &LootParams,
    rng: &mut Rng,
    depth: &mut i32,
    out: &mut Names,
) {
    let Some(rec) = db.get(name) else { return };
    match rec.class.as_str() {
        "LootMasterTable" => {
            if *depth <= 0 {
                return;
            }
            let mut list = Vec::new();
            for i in 1..=100 {
                let w = rec.int(&format!("lootWeight{i}"));
                let n = rec.string(&format!("lootName{i}"));
                if w > 0 && !n.is_empty() {
                    list.push((n, w as u32));
                }
            }
            if list.is_empty() {
                return;
            }
            *depth -= 1;
            let list = cumulative(list.into_iter());
            let Some(entry) = pick_lt(&list, rng) else {
                return;
            };
            loot_name(db, &entry, p, rng, depth, out);
        }
        "LevelTable" => {
            if let Some(r) = level_table_pick(rec, p.parent_level) {
                loot_name(db, &r, p, rng, depth, out);
            }
        }
        "LootItemTable_DynWeight" => dyn_weight(db, rec, p, rng, out),
        "LootItemTable_FixedWeight" => fixed_weight(db, rec, p, rng, out),
        _ => out.base = name.to_string(),
    }
}

/// Rolls one item from any loot table (or plain item record), then draws its
/// seed (the next value of the same stream, as the drop code does).
pub fn roll_table(db: &ItemDb, table: &str, p: &LootParams, rng: &mut Rng) -> Option<LootRoll> {
    let names = roll_table_names(db, table, p, rng, &mut 5);
    if names.base.is_empty() {
        return None;
    }
    Some(LootRoll {
        seed: rng.next(),
        ..names
    })
}

/// The names one roll of `table` picks (seed 0, base empty for nothing).
/// `depth` is the `LootMasterTable` recursion budget (5 per item), which a
/// caller re-rolling the same item shares between its tries.
pub fn roll_table_names(
    db: &ItemDb,
    table: &str,
    p: &LootParams,
    rng: &mut Rng,
    depth: &mut i32,
) -> LootRoll {
    let mut out = Names::default();
    loot_name(db, table, p, rng, depth, &mut out);
    LootRoll {
        base: out.base,
        prefix: out.prefix,
        suffix: out.suffix,
        seed: 0,
    }
}

/// `ScriptableAction_GiveRandomItem::GenerateItem` 0x18046c930:
/// builds a fresh table object from the action's record and rolls it once.
/// Only a `LootItemTable_DynWeight` or a `LootMasterTable` record produces an
/// item (any other class, a plain item record included, gives `NULL`). The
/// table object is default-constructed and never gets
/// `SetRandomizerWeightModifiers`, so the monster / chest rarity weight
/// modifiers are all 0 ([`LootParams::weight_mods`] cleared); the altar flag
/// keeps its default (on, so the Ascendant `doubleRareWeightModifier`
/// applies). A `LootMasterTable` is loaded with `SetNoBrokenItems(1, 1)`
/// (`this+48 = 1`), a `LootItemTable_DynWeight` is not (`+48 = 0`). Both get
/// the main player's level as parent and average level. The item seed is the
/// draw after the names, as in [`roll_table`].
pub fn story_random_item(
    db: &ItemDb,
    table: &str,
    p: &LootParams,
    rng: &mut Rng,
) -> Option<LootRoll> {
    let no_broken = match db.get(table)?.class.as_str() {
        "LootItemTable_DynWeight" => false,
        "LootMasterTable" => true,
        _ => return None,
    };
    let p = LootParams {
        weight_mods: [0; 10],
        no_broken,
        equipped_drop: 0.0,
        ..*p
    };
    roll_table(db, table, &p, rng)
}

/// `roll_loot(table, level, rng)`.
pub fn roll_loot(db: &ItemDb, table: &str, level: u32, rng: &mut Rng) -> Option<LootRoll> {
    roll_table(db, table, &LootParams::at_level(level), rng)
}

/// Monster equipment locations (`LootLoader::Location`, string
/// tables `aLootheaditem1` / `aChancetoequiph_1`).
pub const SLOT_NAMES: [&str; 16] = [
    "Head",
    "Neck",
    "Chest",
    "Legs",
    "Feet",
    "Hands",
    "LeftHand",
    "RightHand",
    "Finger1",
    "Finger2",
    "Misc1",
    "Misc2",
    "Misc3",
    "Waist",
    "Shoulders",
    "Medal",
];

/// `Character::LoadLoot` slot order.
pub const EQUIP_ORDER: [usize; 13] = [0, 1, 15, 2, 3, 4, 5, 13, 14, 6, 7, 8, 9];

/// `LootLoader::Load(table, location, 6, difficulty)` + `GetLootName`.
pub fn loot_loader(
    db: &ItemDb,
    rec: &Record,
    location: usize,
    p: &LootParams,
    rng: &mut Rng,
) -> Option<LootRoll> {
    let slot = SLOT_NAMES[location];
    let mut list = Vec::new();
    let mut total = 0u32;
    for i in 1..=6 {
        let w = rec.int(&format!("chanceToEquip{slot}Item{i}"));
        if w <= 0 {
            continue;
        }
        let n = rec.string_at(&format!("loot{slot}Item{i}"), p.difficulty as usize);
        if n.is_empty() {
            continue;
        }
        list.push((n.to_string(), w as u32));
        total = total.wrapping_add(w as u32);
    }
    let name = pick_le(&list, total, rng)?;
    roll_table(db, &name, p, rng)
}

/// The ten weight kinds in [`LootParams::weight_mods`] order (the record field prefixes).
const WEIGHT_KINDS: [&str; 10] = [
    "noPrefixNoSuffix",
    "bothPrefixSuffix",
    "prefixOnly",
    "suffixOnly",
    "brokenOnly",
    "rareBothPrefixSuffix",
    "rarePrefixOnly",
    "rareSuffixOnly",
    "rarePrefixNormalSuffix",
    "normalPrefixRareSuffix",
];

/// Who rolls the loot, for [`weight_mods`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeightSource<'a> {
    /// A monster's `monsterClassification`.
    Monster(&'a str),
    /// A container's `lootClassification`.
    Chest(&'a str),
}

/// The `RandomizerWeightModifiers` for a loot source on `difficulty`, from
/// `gameengine.dbr` `randomizerWeightsRecord`
/// (`records/game/gamerandomizerweights.dbr`),
/// `GameEngine::LoadFromDatabase`: for the current difficulty (0 plain, 1
/// `Elite`, 2 `Ultimate`, else no entries) it fills two maps by
/// classification with `<kind>[Elite|Ultimate]Modifier<Class>[Chest]`, each
/// value truncated to an int, in `LootParams::weight_mods` order. Monsters
/// (`ResolveMonsterClassificationEnum`, unknown = Common): Common, Champion,
/// Hero, Boss, and Quest with the Boss keys; SuperBoss has no entry (no
/// modifiers). Chests (`ResolveChestClassificationEnum`, unknown = Common):
/// Common, Champion, Hero, Boss. `GetMonsterLootWeightModifiers` /
/// `GetChestLootWeightModifiers` leave the zeroed modifiers of a missing
/// entry. Inside a challenge area its own maps replace these
/// ([`challenge_weight_mods`]).
pub fn weight_mods(db: &ItemDb, difficulty: u32, source: WeightSource) -> [i32; 10] {
    let diff = match difficulty {
        0 => "",
        1 => "Elite",
        2 => "Ultimate",
        _ => return [0; 10],
    };
    let (class, chest) = match source {
        WeightSource::Monster(c) => match c {
            "Champion" | "Hero" | "Boss" => (c, ""),
            "Quest" => ("Boss", ""),
            "SuperBoss" => return [0; 10],
            _ => ("Common", ""),
        },
        WeightSource::Chest(c) => (
            if matches!(c, "Champion" | "Hero" | "Boss") {
                c
            } else {
                "Common"
            },
            "Chest",
        ),
    };
    let Some(rec) = db
        .get("records/game/gameengine.dbr")
        .and_then(|e| db.get(e.string("randomizerWeightsRecord")))
    else {
        return [0; 10];
    };
    WEIGHT_KINDS.map(|k| rec.float(&format!("{k}{diff}Modifier{class}{chest}")) as i32)
}

/// A challenge area's own `RandomizerWeightModifiers` for a loot source `ChallengeArea::Load`
/// @0x18004a310 (the maps `GetMonsterLootWeightModifiers` / `GetChestLootWeightModifiers` read, inlined in
/// `ControllerMonster::GetDropMiscItems` and the container loot job `sub_18003DB20`): at load, for the difficulty
/// (0 plain, 1 `Elite`, 2 `Ultimate`, else no entries: `None`, the engine's stay), one entry per classification
/// with the area record's `<kind>[Elite|Ultimate]Modifier<Class>[Chest]` (missing fields are 0, so an area that
/// lists none, as the stock sandstorms / blizzards, drops the engine's `+%` bonuses: the tables' own weights stay,
/// loot still drops; the entry replaces the engine's unconditionally, no empty-table fallback). Monsters
/// (`ResolveMonsterClassificationEnum`, unknown = Common): Common, Champion, Hero, Boss, and Quest and SuperBoss
/// with the Boss keys (the engine's own table has no SuperBoss entry); chests: Common, Champion, Hero, Boss.
/// `None` also for a missing record (no area object).
pub fn challenge_weight_mods(
    db: &ItemDb,
    area: &str,
    difficulty: u32,
    source: WeightSource,
) -> Option<[i32; 10]> {
    let diff = match difficulty {
        0 => "",
        1 => "Elite",
        2 => "Ultimate",
        _ => return None,
    };
    let rec = db.get(area)?;
    let (class, chest) = match source {
        WeightSource::Monster(c) => match c {
            "Champion" | "Hero" | "Boss" => (c, ""),
            "Quest" | "SuperBoss" => ("Boss", ""),
            _ => ("Common", ""),
        },
        WeightSource::Chest(c) => (
            if matches!(c, "Champion" | "Hero" | "Boss") {
                c
            } else {
                "Common"
            },
            "Chest",
        ),
    };
    Some(WEIGHT_KINDS.map(|k| rec.float(&format!("{k}{diff}Modifier{class}{chest}")) as i32))
}

/// The weight modifiers of a source standing in `area` (a `ChallengeArea` record) or outside one.
pub fn weight_mods_in(
    db: &ItemDb,
    difficulty: u32,
    source: WeightSource,
    area: Option<&str>,
) -> [i32; 10] {
    area.and_then(|a| challenge_weight_mods(db, a, difficulty, source))
        .unwrap_or_else(|| weight_mods(db, difficulty, source))
}

/// A challenge area's extra monster drop `ChallengeArea::GetMiscDropItem` @0x18004cb60 (called by
/// `ControllerMonster::GetDropMiscItems` before the misc slots, same stream): the classification clamped to
/// Common / Champion / Hero picks `dropChanceCommon` / `Champion` / `Hero`; one [`chance_roll`] always, and on a
/// hit with a non-empty `dropItem` that record with the next draw as its seed. No stock area has a `dropItem`.
pub fn challenge_misc_drop(
    db: &ItemDb,
    area: &str,
    classification: &str,
    rng: &mut Rng,
) -> Option<LootRoll> {
    let rec = db.get(area)?;
    let key = match classification {
        "Champion" => "dropChanceChampion",
        "Hero" | "Boss" | "Quest" | "SuperBoss" => "dropChanceHero",
        _ => "dropChanceCommon",
    };
    if !chance_roll(rec.float(key), rng) {
        return None;
    }
    let item = rec.string("dropItem");
    (!item.is_empty()).then(|| LootRoll {
        base: item.to_owned(),
        prefix: String::new(),
        suffix: String::new(),
        seed: rng.next(),
    })
}

/// `(float)(int)r * 4.6519563e-8 + 0.1 <= chance` (a roll in [0.1, 100)).
pub fn chance_roll(chance: f32, rng: &mut Rng) -> bool {
    (rng.next() as i32 as f32 * 4.651_956_3e-8) + 0.1 <= chance
}

/// Everything a monster record drops: equipped slots (`LoadLoot`) then the
/// three misc slots (`GetDropMiscItems`), from one stream (the engine seeds
/// the two passes separately; pass a fresh stream per pass for exactness).
pub fn monster_drops(db: &ItemDb, monster: &str, p: &LootParams, rng: &mut Rng) -> Vec<LootRoll> {
    monster_drops_tagged(db, monster, p, rng)
        .into_iter()
        .map(|(_, r)| r)
        .collect()
}

/// Like [`monster_drops`], with `true` for items the monster wears (equipment
/// slots) — those drop on death only when not Broken
/// (`Monster::DropIfNotBroken` 0x180376e20: classification 5 is
/// destroyed instead); `false` for the misc slots.
pub fn monster_drops_tagged(
    db: &ItemDb,
    monster: &str,
    p: &LootParams,
    rng: &mut Rng,
) -> Vec<(bool, LootRoll)> {
    monster_slots(
        db,
        monster,
        p,
        rng,
        &EQUIP_ORDER
            .into_iter()
            .chain([10, 11, 12])
            .collect::<Vec<_>>(),
        None,
    )
}

/// The items a monster wears (`Character::LoadLoot` @0x180070030: each
/// slot's item is created from its loot table and equipped), rolled
/// when it is created.
pub fn monster_worn(db: &ItemDb, monster: &str, p: &LootParams, rng: &mut Rng) -> Vec<LootRoll> {
    monster_slots(db, monster, p, rng, &EQUIP_ORDER, None)
        .into_iter()
        .map(|(_, r)| r)
        .collect()
}

/// The misc drop slots (`GetDropMiscItems`), rolled on death, for a monster in the challenge area `area`
/// (its `ChallengeArea` record, `Monster` +8344): the area's extra drop ([`challenge_misc_drop`]) first, then
/// the slots with the area's weights.
pub fn monster_misc_in(
    db: &ItemDb,
    monster: &str,
    p: &LootParams,
    area: Option<&str>,
    rng: &mut Rng,
) -> Vec<LootRoll> {
    monster_slots(db, monster, p, rng, &[10, 11, 12], area)
        .into_iter()
        .map(|(_, r)| r)
        .collect()
}

fn monster_slots(
    db: &ItemDb,
    monster: &str,
    p: &LootParams,
    rng: &mut Rng,
    slots: &[usize],
    area: Option<&str>,
) -> Vec<(bool, LootRoll)> {
    let Some(rec) = db.get(monster) else {
        return Vec::new();
    };
    let class = rec.string("monsterClassification");
    // The classification's weight modifiers, Broken allowed (
    // `Character::CreateItemFromLootTable` with the ones `Monster::Load`
    // stores, `ControllerMonster::GetDropMiscItems`; their `LootLoader`
    // keeps `SetNoBrokenItems` off). The misc slots take the challenge
    // area's (`area`, [`weight_mods_in`]); the worn ones never do (`Monster::Load`
    // runs before `InitialUpdate` finds the area).
    let p = &LootParams {
        weight_mods: weight_mods_in(db, p.difficulty, WeightSource::Monster(class), area),
        no_broken: false,
        ..*p
    };
    let mut out = Vec::new();
    if let Some(a) = area
        && let Some(r) = challenge_misc_drop(db, a, class, rng)
    {
        out.push((false, r));
    }
    // Worn slots (`CreateItemFromLootTable`, monsters that are not pets):
    // × (1 + the Ascendant `equippedItemDropModifier`); misc slots
    // are unscaled.
    let worn_mod = 1.0 + p.equipped_drop;
    for &loc in slots {
        let mut chance = rec.float(&format!("chanceToEquip{}", SLOT_NAMES[loc]));
        if !(10..=12).contains(&loc) {
            chance *= worn_mod;
        }
        if chance_roll(chance, rng)
            && let Some(r) = loot_loader(db, rec, loc, p, rng)
        {
            out.push((!(10..=12).contains(&loc), r));
        }
    }
    out
}

/// Containers (`FixedItemContainer` / `Destructible` loot, loader
/// `sub_180358340`): `lootTable[min(difficulty, n - 1)]` (index 3 on
/// Ultimate Veteran) at
/// [`container_level`], then gold (`goldGenerator` with
/// `goldGeneratorChance`, see `progress::gold_drop`). The table is a
/// `fixeditemloot.tpl` record ([`fixed_item_loot_known`]); the game reads any table
/// that way (the container's async loot job `sub_18003EBD0` →
/// `sub_18003D6A0` / `sub_18003E370` only reads the fixed-loot fields, so
/// another table class yields no items; every stock container table is a
/// `fixeditemloot.tpl`). Each item rolls with the container's
/// `lootClassification` chest weight modifiers ([`weight_mods`]) and no
/// Broken items (`sub_18003DB20`: its `LootLoader` gets
/// `GetChestLootWeightModifiers` and `SetNoBrokenItems(true, true)`; the
/// classification is read with `lootTable` by `sub_180358340`).
pub fn container_drops(
    db: &ItemDb,
    container: &str,
    p: &LootParams,
    rng: &mut Rng,
) -> Vec<LootRoll> {
    container_drops_known(db, container, p, &[], &mut DroppedEpics::default(), rng)
}

/// [`container_drops`] re-rolling blueprints the player knows and the
/// session's dropped Epics / Legendaries ([`fixed_item_loot_known`]).
pub fn container_drops_known(
    db: &ItemDb,
    container: &str,
    p: &LootParams,
    known: &[&str],
    dropped: &mut DroppedEpics,
    rng: &mut Rng,
) -> Vec<LootRoll> {
    container_drops_in(db, container, p, None, known, dropped, rng)
}

/// [`container_drops_known`] for a container standing in the challenge area `area` (`sub_18003DB20`:
/// the area at the container's coords, its chest weights replace the engine's, [`weight_mods_in`]).
pub fn container_drops_in(
    db: &ItemDb,
    container: &str,
    p: &LootParams,
    area: Option<&str>,
    known: &[&str],
    dropped: &mut DroppedEpics,
    rng: &mut Rng,
) -> Vec<LootRoll> {
    let Some(rec) = db.get(container) else {
        return Vec::new();
    };
    let p = &LootParams {
        weight_mods: weight_mods_in(
            db,
            p.difficulty,
            WeightSource::Chest(rec.string("lootClassification")),
            area,
        ),
        no_broken: true,
        ..*p
    };
    let tables = rec.strs("lootTable");
    let d = if p.ultimate_veteran {
        3
    } else {
        p.difficulty as usize
    };
    let Some(t) = tables.get(d.min(tables.len().saturating_sub(1))) else {
        return Vec::new();
    };
    let Some(table) = db.get(t) else {
        return Vec::new();
    };
    fixed_item_loot_known(db, table, p, known, dropped, rng)
}

/// A container's item level (`sub_1803585F0`): player level +
/// `levelOffset[difficulty]` + the area's offset (sector layer 9), clamped
/// to the record's `[minLevel, maxLevel]`; when that gives 0 (no maxLevel),
/// the level of the proxy that placed it, else the player level + offsets
/// clamped to the area's limits. `limits` = (min, max, offset) for the
/// difficulty. The last clamp has no special case (an area max of
/// 0 gives level 0). Without area data (`None`, a port gap) the level is the
/// player level + offset, at least 1.
pub fn container_level(
    rec: &Record,
    player_level: i32,
    difficulty: usize,
    limits: Option<(u32, u32, i32)>,
    proxy_level: i32,
) -> u32 {
    let offs = rec.f32s("levelOffset");
    let base =
        player_level + offs.get(difficulty).map_or(0, |v| *v as i32) + limits.map_or(0, |l| l.2);
    let (lo, hi) = (
        rec.i32("minLevel").unwrap_or(0),
        rec.i32("maxLevel").unwrap_or(0),
    );
    let mut level = base.max(lo).min(hi);
    if level == 0 {
        level = proxy_level;
    }
    if level == 0 {
        level = match limits {
            Some((lo, hi, _)) => return base.max(lo as i32).min(hi as i32).max(0) as u32,
            None => base,
        };
    }
    level.max(1) as u32
}

/// `fixeditemloot.tpl` tables (`sub_18003E370`): the item count is
/// `IGenerate(min, max)` of `numSpawnMin/MaxEquation` (`numberOfPlayers` = 1,
/// `averagePlayerLevel`, `gameDifficulty`), or else a pick over
/// `numberWeights` (index = count). Item `i` (`sub_18003DB20`) picks a slot
/// `loot1..6` by its `lootNChance[i]` (cumulative, `FGenerate(0, Σ)`), then
/// `lootNName1..6` by `lootNWeight1..6` like a monster loot slot. Epic /
/// Legendary items already dropped this session and known blueprints
/// (`known`, the learned recipe records, and `dropped`, which gains the kept
/// ones) are re-rolled.
pub fn fixed_item_loot_known(
    db: &ItemDb,
    table: &Record,
    p: &LootParams,
    known: &[&str],
    dropped: &mut DroppedEpics,
    rng: &mut Rng,
) -> Vec<LootRoll> {
    let count = fixed_count(table, p, rng);
    let mut out = Vec::new();
    for i in 0..count.max(0) as usize {
        // `sub_18003E370`: an item that is an Epic / Legendary
        // already dropped from a container this session
        // (`GameEngine::HasDroppedEpicOrLegendaryItem` set) or a blueprint the
        // player already knows (`GameEngine::IsItemInFormulas`) is rolled
        // again, up to 10 more times; the 11th roll is kept whatever it is.
        // A kept Epic / Legendary joins the set (`RegisterEpicOrLegendaryItem`,
        // cleared only by `ExitPlayingMode`).
        let mut tries = 0;
        while let Some(r) = fixed_item_once(db, table, p, i, rng) {
            tries += 1;
            let low = r.base.to_ascii_lowercase();
            let seen = dropped.0.contains(&low);
            if tries > 10 || (!seen && !known.iter().any(|k| k.eq_ignore_ascii_case(&r.base))) {
                if db
                    .get(&r.base)
                    .is_some_and(|b| matches!(b.string("itemClassification"), "Epic" | "Legendary"))
                {
                    dropped.0.insert(low);
                }
                out.push(r);
                break;
            }
        }
    }
    out
}

/// How many items a fixed table drops (`sub_18003D6A0`): uniform in
/// the `numSpawn{Min,Max}Equation` range, else an index into `numberWeights`.
fn fixed_count(table: &Record, p: &LootParams, rng: &mut Rng) -> i32 {
    let vars = |n: &str| match n.to_ascii_lowercase().as_str() {
        "numberofplayers" => Some(1.0),
        "averageplayerlevel" | "minplayerlevel" | "maxplayerlevel" => Some(p.player_level as f32),
        "gamedifficulty" => Some(p.difficulty as f32),
        "proxylevel" => Some(p.parent_level as f32),
        _ => None,
    };
    match (
        table.str("numSpawnMinEquation"),
        table.str("numSpawnMaxEquation"),
    ) {
        (Some(a), Some(b)) => {
            let ev = |s: &str| {
                Expr::parse(s)
                    .ok()
                    .map_or(0, |e| e.eval(&vars).max(0.0) as i32)
            };
            let (a, b) = (ev(a), ev(b));
            let (lo, hi) = (a.min(b), a.max(b));
            // `sub_18003D6A0`: int arithmetic, wrapping.
            let span = hi.wrapping_sub(lo).wrapping_add(1) as u32;
            lo.wrapping_add((rng.next() % span) as i32)
        }
        _ => {
            let w: Vec<u32> = table
                .f32s("numberWeights")
                .into_iter()
                .map(|w| w.max(0.0) as u32)
                .collect();
            // `sub_18003D6A0`: the weight sum and running total wrap.
            let total = w.iter().fold(0u32, |s, w| s.wrapping_add(*w));
            let m = total.wrapping_add(1);
            // (bad data: a sum wrapping to u32::MAX would divide by zero)
            let r = rng.next() % if m == 0 { 1 } else { m };
            let mut cum = 0u32;
            w.iter()
                .position(|w| {
                    cum = cum.wrapping_add(*w);
                    cum >= r
                })
                .unwrap_or(0) as i32
        }
    }
}

/// Epic / Legendary items containers dropped this play session (lower-case
/// records; `GameEngine` +224352). A new play session starts empty
/// (`GameEngine::ExitPlayingMode` / `ClearDroppedEpicsAndLegendaries`).
#[derive(Debug, Clone, Default)]
pub struct DroppedEpics(pub std::collections::BTreeSet<String>);

/// One item of a fixed-loot table (`sub_18003DB20`); `None` when it yields nothing.
fn fixed_item_once(
    db: &ItemDb,
    table: &Record,
    p: &LootParams,
    i: usize,
    rng: &mut Rng,
) -> Option<LootRoll> {
    let slots: Vec<(usize, f32)> = (1..=6)
        .filter_map(|n| {
            table
                .f32s(&format!("loot{n}Chance"))
                .get(i)
                .copied()
                .filter(|c| *c > 0.0)
                .map(|c| (n, c))
        })
        .collect();
    let total: f32 = slots.iter().map(|s| s.1).sum();
    if slots.is_empty() {
        return None;
    }
    let roll = rng.unit() * total;
    let mut cum = 0.0;
    let &(n, _) = slots.iter().find(|(_, c)| {
        cum += c;
        roll <= cum
    })?;
    let mut list = Vec::new();
    let mut wsum = 0u32;
    for k in 1..=6 {
        let w = table.i32(&format!("loot{n}Weight{k}")).unwrap_or(0);
        let Some(name) = table.str(&format!("loot{n}Name{k}")) else {
            continue;
        };
        if w > 0 {
            list.push((name.to_string(), w as u32));
            wsum = wsum.wrapping_add(w as u32);
        }
    }
    let name = pick_le(&list, wsum, rng)?;
    roll_table(db, &name, p, rng)
}

/// The item's classification: the highest of base and affixes (
/// `Item::SetItemClassification`, called for the base, prefix and suffix,
/// keeps the larger enum value).
pub fn rarity(r: &ResolvedItem) -> Classification {
    let rank = |c: Classification| match c {
        Classification::Common => 0,
        Classification::Magical => 1,
        Classification::Rare => 2,
        Classification::Epic => 3,
        Classification::Legendary => 4,
        _ => 5,
    };
    [
        Some(r.classification),
        r.prefix_classification,
        r.suffix_classification,
    ]
    .into_iter()
    .flatten()
    .max_by_key(|c| rank(*c))
    .unwrap_or(Classification::Common)
}

/// The sound a thrown item makes where it lands. `Item::PhysicsUpdate` @0x180325460 (landing: vfunc
/// +1048 = `Item::PlayDropSoundWorld` @0x180325820, surface 100, tracked 3D): the item's classification sound
/// when it has one (`ItemEquipment::InitializeItem` @0x18033c240 stores `GameEngine::GetItemDropSoundName`, the
/// `*ItemDropSound` rows of `gameiteminfo.dbr` keyed by the classification enum: the larger of base / prefix /
/// suffix, or 16 = rare prefix + rare suffix, 17 when the base is rare too; equipment only), else its record's
/// `dropSound3D` (`Item::Load` @0x180324c60). The water case (`dropSoundWater`, surface 7) needs the physics
/// surface under the landing: PORT: always the ground one.
pub fn drop_sound(idb: &ItemDb, item: &Item) -> Option<String> {
    let r = super::resolve(idb, item)?;
    if super::resolve::is_equipment_class(&r.class) {
        let key = if r.prefix_classification == Some(Classification::Rare)
            && r.suffix_classification == Some(Classification::Rare)
        {
            16 + (r.base_classification == Classification::Rare) as u32
        } else {
            rarity(&r) as u32
        };
        let field = match key {
            0 => "commonItemDropSound",
            1 => "magicalItemDropSound",
            2 => "rareItemDropSound",
            3 => "epicItemDropSound",
            4 => "legendaryItemDropSound",
            5 => "brokenItemDropSound",
            16 => "doubleRareItemDropSound",
            _ => "monsterDoubleRareItemDropSound",
        };
        let info = idb.get("records/game/gameiteminfo.dbr");
        if let Some(pak) = info
            .as_ref()
            .and_then(|i| i.str(field))
            .filter(|p| !p.is_empty())
        {
            return Some(pak.to_owned());
        }
    }
    let base = idb.get(&item.base_name)?;
    base.str("dropSound3D")
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
}

/// A gold pickup with the character's `characterIncreasedGold` %
/// (`Character::TakeBonus` @0x180067e30: gold + floor(gold × % +
/// 0.5); the money text shows the sum).
pub fn gold_with_bonus(gold: u32, bonus_pct: f32) -> u32 {
    if bonus_pct == 0.0 {
        return gold;
    }
    let extra = ((bonus_pct * 0.01) * gold as f32 + 0.5).floor() as i64;
    (gold as i64 + extra).clamp(0, u32::MAX as i64) as u32
}

/// The auto-pickup radius of a ground item of `class` (None = gold), 0 when
/// it is not picked up automatically. `Player::UpdateSelf`
/// @0x1803d24a0: per item with `CanAutoPickup` and a radius above 0 the
/// pickup distance is `max(GetAutoPickupRadius, autoLootRadius × 10)`
/// (3D, items within 10 m). Gold (`OneShot_Gold` @0x1803c0f50),
/// `OneShot_Skill` and potions: always, 4 m (potions only when the bags
/// have room: `Player::IsInventorySpaceAvailable`, the pickup itself fails
/// otherwise); `OneShot_Food` 1 m (@0x1803c14b0); `ItemRelic` / `QuestItem`
/// (@0x18034a9c0, @0x180428390): only when the option is above 0, 4 m with
/// room. Each of them refuses an item the main player dropped (Item +1728
/// the player's id, `gd_game::item_drop`: `gd_game::loot::pick_up` skips a
/// `DroppedByPlayer`), and each waits out its 750 ms pick-up delay
/// (`gd_game::loot::PICKUP_DELAY`).
pub fn auto_pickup_radius(class: Option<&str>, loot_radius: f32) -> f32 {
    let base: f32 = match class {
        None => 4.0,
        Some(
            "OneShot_Potion"
            | "OneShot_Scroll"
            | "OneShot_PotionHealth"
            | "OneShot_PotionMana"
            | "OneShot_Skill",
        ) => 4.0,
        Some("OneShot_Food") => 1.0,
        Some("ItemRelic" | "QuestItem") if loot_radius > 0.0 => 4.0,
        _ => return 0.0,
    };
    base.max(loot_radius * 10.0).min(10.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::db::MemorySource;
    use gd_formats::arz::RawValue::*;
    use std::sync::Arc;

    #[test]
    fn pickers() {
        let list = cumulative([("a", 1u32), ("b", 3)].into_iter());
        assert_eq!(list[1].1, 4);
        // r % (total+1) with <= : weight-1 first entry wins r in {0,1}.
        let w = vec![("a".to_string(), 1u32), ("b".to_string(), 3)];
        let mut hits = [0; 2];
        let mut rng = Rng(7);
        for _ in 0..5000 {
            match pick_le(&w, 4, &mut rng).as_deref() {
                Some("a") => hits[0] += 1,
                _ => hits[1] += 1,
            }
        }
        assert!((1800..2200).contains(&hits[0]), "{hits:?}");
    }

    #[test]
    fn dyn_table_with_affix() {
        let mut m = MemorySource::default();
        m.insert(
            "t.dbr",
            "LootItemTable_DynWeight",
            &[
                ("lootName1", Str("i.dbr".into())),
                ("lootWeight1", Int(100)),
                ("minItemLevelEquation", Str("parentLevel-5".into())),
                ("maxItemLevelEquation", Str("parentLevel+5".into())),
                ("targetLevelEquation", Str("parentLevel".into())),
                ("bellSlope", Float(100.0)),
                ("bothPrefixSuffix", Int(1)),
                ("prefixTableName1", Str("pt.dbr".into())),
                ("prefixTableWeight1", Int(10)),
                ("prefixTableLevelMin1", Int(1)),
                ("prefixTableLevelMax1", Int(100)),
                ("suffixTableName1", Str("pt.dbr".into())),
                ("suffixTableWeight1", Int(10)),
                ("suffixTableLevelMin1", Int(1)),
                ("suffixTableLevelMax1", Int(100)),
            ],
        );
        m.insert("i.dbr", "ArmorProtective_Head", &[("itemLevel", Int(10))]);
        m.insert(
            "pt.dbr",
            "LootRandomizerTable",
            &[
                ("randomizerName1", Str("affix.dbr".into())),
                ("randomizerWeight1", Int(5)),
                ("randomizerLevelMin1", Int(1)),
                ("randomizerLevelMax1", Int(50)),
            ],
        );
        let db = ItemDb::new(Arc::new(m));
        let mut rng = Rng(12345);
        let r = roll_loot(&db, "t.dbr", 10, &mut rng).unwrap();
        assert_eq!(
            (r.base.as_str(), r.prefix.as_str(), r.suffix.as_str()),
            ("i.dbr", "affix.dbr", "affix.dbr")
        );
        // Above the range: falls back to the highest item below the minimum.
        assert!(roll_loot(&db, "t.dbr", 40, &mut rng).is_some());
        // Below every item: nothing drops.
        assert!(roll_loot(&db, "t.dbr", 1, &mut rng).is_none());
    }
}
