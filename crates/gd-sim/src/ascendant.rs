//! The Ascendant system (Fangs of Asterkarn): the Ascendant Altar's formulas
//! and what they change for the rest of the session, plus the Ultimate
//! Veteran base chances of troves, totems, The Dread and soul drops.
//!
//! (Game.dll / GrimDawn.exe):
//! - The altar window has a centre box (one of `gameascendant.dbr`
//!   `altarExclusiveRecipes`) and five boxes (`altarInclusiveRecipes`); a
//!   formula is found by its reagent item (`reagentBaseBaseName`,
//!   `GameEngine::FindAltarRecipe`) and its level is the number of boxes
//!   holding it (exe `sub_14011A4C0`). Using the altar needs the
//!   `gameengine.dbr` `ascendantToken` (`MainPlayerCanUseAscendantAltar`).
//! - `AscendantAltarFormula::Install(level)` adds the per-level values (%,
//!   index level − 1, clamped) of `goldModifier`, `reputationModifier`,
//!   `experienceModifier`, `doubleRareWeightModifier`,
//!   `bonusTroveChanceModifier`, `bonusTotemChanceModifier`,
//!   `superBossChanceModifier` (+ its `superBossSpawn` list),
//!   `superBossSpawnModifier`, `equippedItemDropModifier`,
//!   `nemesisSpawnModifier`, `ascensionReagentModifier`,
//!   `itemAscensionModifier`, and registers `monsterAttributePak` /
//!   `nonBossMonsterAttributePak` / `bossMonsterAttributePak`
//!   (`LoadAscendant*Bonuses`: column level + 5 × difficulty, Ultimate
//!   Veteran counting as difficulty 3; the non-boss pak for Common / Champion
//!   / Hero, the boss one for Boss and up, the first for every monster).
//!   `EnableGameEngine` resets it all: it lasts the session.
//! - Chances (`CalculateTroveChance` / `…Totem…` / `…SuperBoss…` and
//!   `GetAscendant*Chance`): with the formula modifier installed the
//!   `troveEquation` / `totemEquation` / `superBossEquation` result
//!   (`troveItemMod`… = the summed modifier, `gameDifficultyDV`,
//!   `gameChallengeDV`), else on Ultimate Veteran the
//!   `ultimateChallenge*Chance`, else none.
//! - The Dread: kills count up to `superBossKillsMin` × (1 − spawn modifier)
//!   (`GetSuperBossSpawnKills`, at least 1); then the chance roll; the spawn
//!   list is indexed by difficulty (+1 on Ultimate Veteran).
//! - Soul drops (`GetAscensionReagentDropChance`, `ControllerMonster::
//!   DropLoot`): Hero → `itemAscensionHero`, Boss → `itemAscensionNemesis` +
//!   `…Nemesis2`, Quest / SuperBoss → `itemAscensionBoss`, each rolled at its
//!   chance (UV base or equation, as above), one item each.
//!
//! - Gold / XP (`PlayerManagerServer::HandleBonus`): the amount plus
//!   `round(amount × mod)` (half up), not applied inside Shattered Realm or
//!   its treasure rooms. The `AttributePak` column is 1-based (level 1 on
//!   Normal = index 0; `CharAttributeMod::AddToAccumulator` reads level − 1,
//!   clamped, nothing at 0). Trove / totem chances are integer rolls
//!   `IGenerate(0, 100) <= chance` (`Proxy::RunProxy`, `SetPiece::Generate`),
//!   soul chances a float roll `FGenerate(0, 100) <= chance`
//!   (`ControllerMonster::DropLoot`).
//!
//! The reputation modifier: `GameEngine::OnCreatureDeath` puts it
//! in the kill adjustment handed to `HandleFaction` → `FactionPack::Adjust`
//! → `FactionPack::AdjustValue` @0x1802a32e0, which scales every change
//! (gain or loss, after the faction's own gain/loss multiplier) by
//! `1 + mod` when the mod is positive. `doubleRareWeightModifier` scales the dyn-table rare/rare weight
//! (`items::loot`). The engine's affix swap and loot-table override lists
//! (`SetAscendantPrefixOverride` ... `SetAscendantLootTableOverride`) are
//! never filled in the shipped Game.dll (no callers), so they
//! are left out. `luaToken` (Voidmarked / roguelike chests) is kept for
//! Lua.
use crate::combat::Rng;
use crate::expr::Expr;
use crate::mode::Mode;
use gd_formats::arz::Record;
use gd_formats::vfs::Database;

pub const GAME_ASCENDANT: &str = "records/game/gameascendant.dbr";

/// Where a monster attribute pak applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    All,
    NonBoss,
    Boss,
}

/// The session's installed altar formulas (all modifiers as fractions).
#[derive(Debug, Clone, Default)]
pub struct Ascendant {
    pub active: bool,
    pub gold: f32,
    pub reputation: f32,
    pub experience: f32,
    pub double_rare: f32,
    pub trove: f32,
    pub totem: f32,
    pub super_boss_chance: f32,
    pub super_boss_spawn: f32,
    pub equipped_drop: f32,
    pub nemesis_spawn: f32,
    pub reagent: f32,
    pub item_ascension: f32,
    /// (pak record, column, scope).
    pub monster_paks: Vec<(String, usize, Scope)>,
    pub super_boss_list: Vec<String>,
    /// Formulas installed: (record, level).
    pub formulas: Vec<(String, u32)>,
    /// `luaToken` → the formula's level (`AddAscendantAltarToken`, set
    /// not summed), read by Lua `Game.GetAscendantTokenModifier`.
    pub tokens: Vec<(String, u32)>,
    /// `gameascendant.dbr`, parsed on first use; reset with the session.
    tables: std::sync::OnceLock<Tables>,
}

/// The `gameascendant.dbr` values the chance rolls read.
#[derive(Debug, Clone)]
struct Tables {
    /// The chance equations (field, parsed): only evaluated with a modifier installed.
    equations: Vec<(&'static str, Expr)>,
    /// `superBossKillsMin`.
    kills_min: f32,
    /// `ultimateChallengeSuperBossSpawn`.
    uv_super_boss: Vec<String>,
    /// The [`UV_CHANCES`] values.
    uv: [f32; UV_CHANCES.len()],
    /// The [`REAGENTS`] items.
    reagents: [Option<String>; REAGENTS.len()],
}

/// The `gameascendant.dbr` equation fields evaluated by the chance rolls.
const EQUATIONS: [&str; 11] = [
    "troveEquation",
    "totemEquation",
    "superBossEquation",
    "itemAscensionHeroEquation",
    "itemAscensionNemesisEquation",
    "itemAscensionNemesisEquation2",
    "itemAscensionBossEquation",
    "itemAscensionAltarCommonEquation",
    "itemAscensionAltarRareEquation",
    "itemAscensionAltarEpicEquation",
    "itemAscensionAltarLegendaryEquation",
];

/// The Ultimate Veteran base chances (%), indexed by the `UV_*` constants.
const UV_CHANCES: [&str; 7] = [
    "ultimateChallengeTroveChance",
    "ultimateChallengeTotemChance",
    "ultimateChallengeSuperBossChance",
    "ultimateChallengeItemAscensionHeroChance",
    "ultimateChallengeItemAscensionNemesisChance",
    "ultimateChallengeItemAscensionNemesisChance2",
    "ultimateChallengeItemAscensionBossChance",
];
const UV_TROVE: usize = 0;
const UV_TOTEM: usize = 1;
const UV_SUPER_BOSS: usize = 2;

/// The soul drop items: (item field, [`UV_CHANCES`] index, equation).
const REAGENTS: [(&str, usize, &str); 4] = [
    ("itemAscensionHero", 3, "itemAscensionHeroEquation"),
    ("itemAscensionNemesis", 4, "itemAscensionNemesisEquation"),
    ("itemAscensionNemesis2", 5, "itemAscensionNemesisEquation2"),
    ("itemAscensionBoss", 6, "itemAscensionBossEquation"),
];

impl Tables {
    fn parse(g: Option<&Record>) -> Self {
        let Some(g) = g else {
            return Self {
                equations: Vec::new(),
                kills_min: 180.0,
                uv_super_boss: Vec::new(),
                uv: [0.0; UV_CHANCES.len()],
                reagents: Default::default(),
            };
        };
        Self {
            equations: EQUATIONS
                .iter()
                .filter_map(|&f| Some((f, Expr::parse(g.str(f)?).ok()?)))
                .collect(),
            kills_min: g.i32("superBossKillsMin").unwrap_or(180) as f32,
            uv_super_boss: g
                .strs("ultimateChallengeSuperBossSpawn")
                .iter()
                .map(|s| s.to_string())
                .collect(),
            uv: UV_CHANCES.map(|f| g.f32(f).unwrap_or(0.0)),
            reagents: REAGENTS.map(|(f, _, _)| g.str(f).map(str::to_owned)),
        }
    }

    /// An equation with its item modifier.
    fn equation(&self, mode: Mode, field: &str, var: &str, value: f32) -> f32 {
        let (d, c) = (mode.difficulty, mode.challenge);
        let Some((_, e)) = self.equations.iter().find(|(f, _)| *f == field) else {
            return 0.0;
        };
        let v = e.eval(&|v| match v {
            "gameDifficultyDV" => Some(d as f32),
            "gameChallengeDV" => Some(c as f32),
            _ if v == var => Some(value),
            _ => None,
        });
        // (bad data: a NaN / infinite result would pass every `<= chance` roll.)
        if v.is_finite() { v.max(0.0) } else { 0.0 }
    }

    /// An Ultimate Veteran base chance ([`UV_CHANCES`] index); 0 below Ultimate Veteran.
    fn uv_base(&self, mode: Mode, i: usize) -> f32 {
        if mode.ultimate_veteran() {
            self.uv[i]
        } else {
            0.0
        }
    }

    /// With the modifier `m` installed, `eq`'s result when > 0; else the UV base chance `uv`.
    fn chance(&self, mode: Mode, m: f32, eq: &str, var: &str, uv: usize) -> f32 {
        let c = if m > 0.0 {
            self.equation(mode, eq, var, m)
        } else {
            0.0
        };
        if c > 0.0 { c } else { self.uv_base(mode, uv) }
    }
}

fn level_pct(rec: &Record, field: &str, level: u32) -> f32 {
    let v = rec.f32s(field);
    if v.is_empty() || level == 0 {
        return 0.0;
    }
    v[(level as usize - 1).min(v.len() - 1)] * 0.01
}

impl Ascendant {
    fn tables(&self, db: &Database) -> &Tables {
        self.tables
            .get_or_init(|| Tables::parse(db.get(GAME_ASCENDANT)))
    }

    /// `AscendantAltarFormula::Install`; a level of 0 (no box holds the formula) installs nothing.
    pub fn install(&mut self, db: &Database, mode: Mode, formula: &str, level: u32) {
        if level == 0 {
            return;
        }
        let Some(r) = db.get(formula) else { return };
        if let Some(t) = r.str("luaToken").filter(|t| !t.is_empty()) {
            self.tokens.retain(|(k, _)| k != t);
            self.tokens.push((t.to_owned(), level));
        }
        let diff = mode.ascendant_index();
        for (field, scope) in [
            ("monsterAttributePak", Scope::All),
            ("nonBossMonsterAttributePak", Scope::NonBoss),
            ("bossMonsterAttributePak", Scope::Boss),
        ] {
            if let Some(p) = r.str(field).filter(|p| !p.is_empty()) {
                self.monster_paks
                    .push((p.to_owned(), level as usize - 1 + 5 * diff, scope));
            }
        }
        self.gold += level_pct(r, "goldModifier", level);
        self.reputation += level_pct(r, "reputationModifier", level);
        self.experience += level_pct(r, "experienceModifier", level);
        self.double_rare += level_pct(r, "doubleRareWeightModifier", level);
        self.trove += level_pct(r, "bonusTroveChanceModifier", level);
        self.totem += level_pct(r, "bonusTotemChanceModifier", level);
        let sb = level_pct(r, "superBossChanceModifier", level);
        if !r.f32s("superBossChanceModifier").is_empty() {
            self.super_boss_chance += sb;
            self.super_boss_list.extend(
                r.strs("superBossSpawn")
                    .iter()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string()),
            );
        }
        self.super_boss_spawn += level_pct(r, "superBossSpawnModifier", level);
        self.equipped_drop += level_pct(r, "equippedItemDropModifier", level);
        self.nemesis_spawn += level_pct(r, "nemesisSpawnModifier", level);
        self.reagent += level_pct(r, "ascensionReagentModifier", level);
        self.item_ascension += level_pct(r, "itemAscensionModifier", level);
        self.formulas.push((formula.to_owned(), level));
        self.active = true;
    }
}

/// Chance (%) that a trove replaces a proxy's accessory chest.
pub fn trove_chance(db: &Database, a: &Ascendant, mode: Mode) -> f32 {
    let t = a.tables(db);
    t.chance(mode, a.trove, "troveEquation", "troveItemMod", UV_TROVE)
}

/// Chance (%) that a set piece uses its `ascendantPool` (monster totems).
pub fn totem_chance(db: &Database, a: &Ascendant, mode: Mode) -> f32 {
    let t = a.tables(db);
    t.chance(mode, a.totem, "totemEquation", "totemItemMod", UV_TOTEM)
}

/// Chance (%) of The Dread once enough kills are in.
pub fn super_boss_chance(db: &Database, a: &Ascendant, mode: Mode) -> f32 {
    a.tables(db).chance(
        mode,
        a.super_boss_chance,
        "superBossEquation",
        "superBossItemMod",
        UV_SUPER_BOSS,
    )
}

/// Kills needed between Dread rolls.
pub fn super_boss_kills(db: &Database, a: &Ascendant) -> u32 {
    let base = a.tables(db).kills_min;
    ((base - base * a.super_boss_spawn) as i32).max(1) as u32
}

/// The Dread record to spawn: the formulas' list, else the UV one, by
/// difficulty (+1 on Ultimate Veteran).
pub fn super_boss_record(db: &Database, a: &Ascendant, mode: Mode) -> Option<String> {
    let i = mode.ascendant_index();
    let list = if a.super_boss_list.is_empty() {
        &a.tables(db).uv_super_boss
    } else {
        &a.super_boss_list
    };
    list.get(i.min(list.len().checked_sub(1)?)).cloned()
}

/// Soul drops of a monster of `classification`: (item, chance %).
pub fn reagent_drops(
    db: &Database,
    a: &Ascendant,
    mode: Mode,
    classification: &str,
) -> Vec<(String, f32)> {
    // [`REAGENTS`] indices.
    let entries: &[usize] = match classification {
        "Hero" => &[0],
        "Boss" => &[1, 2],
        "Quest" | "SuperBoss" => &[3],
        _ => return Vec::new(),
    };
    let t = a.tables(db);
    entries
        .iter()
        .filter_map(|&i| {
            let item = t.reagents[i].as_ref()?;
            let (_, uv, eq) = REAGENTS[i];
            let c = t.chance(mode, a.reagent, eq, "ascensionReagentItemMod", uv);
            (c > 0.0).then(|| (item.clone(), c))
        })
        .collect()
}

/// The ascendant attribute paks (`Ascendant::monster_paks`) for a monster
/// of `classification`: (pak, column).
pub fn monster_paks(
    db: &Database,
    paks: &[(String, usize, Scope)],
    classification: &str,
) -> Vec<(Record, usize)> {
    let boss = !matches!(classification, "Common" | "Champion" | "Hero");
    paks.iter()
        .filter(|(_, _, s)| match s {
            Scope::All => true,
            Scope::NonBoss => !boss,
            Scope::Boss => boss,
        })
        .filter_map(|(p, c, _)| db.get_owned(p).map(|r| (r, *c)))
        .collect()
}

/// The column of a Shattered Realm ruleset's `difficultyAdjustment*` pak for `dungeons_generated` (the tier).
/// `CharAttributeVal::AddToAccumulator` @0x1800cc380: the
/// engine's index `i` is 1-based, `A[min(i - 1, len - 1)]` (a lookup past the table's end takes its last row, no
/// wrap), and `i == 0` contributes nothing (`None`). The caller's column clamp is `stats::level_value`'s.
pub fn adjustment_column(dungeons_generated: u32) -> Option<usize> {
    (dungeons_generated as usize).checked_sub(1)
}

/// A % roll (`IGenerate(0, 100) <= chance`).
pub fn roll(rng: &mut Rng, chance: f32) -> bool {
    chance > 0.0 && (rng.next() % 101) as f32 <= chance
}

/// A float % roll (`FGenerate(0, 100) <= chance`, the soul drops).
pub fn roll_float(rng: &mut Rng, chance: f32) -> bool {
    chance > 0.0 && rng.unit() * 100.0 <= chance
}

/// An Ascendant gold / experience bonus (`PlayerManagerServer::HandleBonus`):
/// `amount + round(amount × modifier)`, rounding half up.
pub fn apply_bonus(amount: u64, modifier: f32) -> u64 {
    let extra = (amount as f32 * modifier + 0.5).floor() as i64;
    (amount as i64).saturating_add(extra).max(0) as u64
}

/// Ascended drops (`GameEngine::GetAscendedItemName` @0x1802eb3c0, called for
/// monster drops): an equipment drop of item classification
/// `classification` becomes ascended at the UV base chance
/// (`ultimateChallengeItemAscension<X>Chance`) or, with an item-ascension
/// formula installed, the `itemAscensionAltar<X>Equation` result
/// (`IGenerate(…) % 101 <= chance`); the class's recipe
/// (`itemAscension<X>Recipe`, an `ItemAscensionFormula`) gives the tables for
/// the item type: one `…TablesMastery` table per player mastery (weight
/// `masteryWeight`, × 0.375 with a single mastery) and one random
/// `…TablesAffix` table (weight `affixWeight`), picked by weight
/// (`ItemAscensionFormula::GetRandomizerName`); that table rolled at level 100
/// (`LootRandomizerTable_Dynamic::ManualLoad(…, 100)`) names the affix.
/// Magical uses the Common entry (`GameEngine::LoadFromDatabase`
/// fills classification 1 from the `…Common…` fields too); the mastery index
/// is `MasteryEnumeration` `SkillClassNN` − 1 (`Skill_Mastery::Load`,
/// `SkillManager::GetAllSkillMasteries`); the group comes from the item type
/// / weapon type (`ItemAscensionFormula::GetAscendedName`: armour = head,
/// chest, legs, feet, hands, shoulders; accessory = amulet, ring, waist,
/// medal; staves get none).
#[allow(clippy::too_many_arguments)]
pub fn ascend_drop(
    db: &crate::items::ItemDb,
    a: &Ascendant,
    mode: Mode,
    item_class: &str,
    classification: &str,
    masteries: &[usize],
    rng: &mut Rng,
) -> Option<String> {
    let group = item_group(item_class)?;
    let key = rarity_key(classification)?;
    use crate::items::db::Fields;
    let g = db.get(GAME_ASCENDANT)?;
    let mut chance = 0.0;
    if a.item_ascension > 0.0 {
        chance = a.tables.get_or_init(|| Tables::parse(Some(g))).equation(
            mode,
            &format!("itemAscensionAltar{key}Equation"),
            "itemAscensionMod",
            a.item_ascension,
        );
    }
    if chance <= 0.0 && mode.ultimate_veteran() {
        chance = g.float(&format!("ultimateChallengeItemAscension{key}Chance"));
    }
    if chance <= 0.0 || (rng.next() % 101) as f32 > chance {
        return None;
    }
    ascended_affix(
        db,
        g.string(&format!("itemAscension{key}Recipe")),
        group,
        masteries,
        100,
        rng,
    )
}

/// The Inventor's ascended-affix reroll (exe `sub_1401B7CD0` mode 4):
/// the reroll panel's `ascended<X>Recipe` for the item's rarity,
/// the same table pick, the table rolled at the character level, up to 100
/// tries for an affix different from `current`.
pub fn reroll_ascended(
    db: &crate::items::ItemDb,
    item_class: &str,
    classification: &str,
    masteries: &[usize],
    char_level: u32,
    current: &str,
    rng: &mut Rng,
) -> Option<String> {
    use crate::items::db::Fields;
    let group = item_group(item_class)?;
    let key = rarity_key(classification)?;
    let panel = db.get(REROLL_PANEL)?;
    let recipe = panel.string(&format!("ascended{key}Recipe")).to_owned();
    for _ in 0..100 {
        let a = ascended_affix(db, &recipe, group, masteries, char_level, rng)?;
        if !a.eq_ignore_ascii_case(current) {
            return Some(a);
        }
    }
    None
}

/// The Inventor's reroll panel (`records/ui/inventor/rerollpanel`).
pub const REROLL_PANEL: &str = "records/ui/inventor/rerollpanel/reroll_table.dbr";

/// The reroll recipe for an item of `classification` (`kind`: "seed" or
/// "ascended").
pub fn reroll_recipe(
    db: &crate::items::ItemDb,
    kind: &str,
    classification: &str,
) -> Option<String> {
    use crate::items::db::Fields;
    let panel = db.get(REROLL_PANEL)?;
    let field = if kind == "seed" {
        "seedRecipe".to_owned()
    } else {
        format!("ascended{}Recipe", rarity_key(classification)?)
    };
    Some(panel.string(&field).to_owned()).filter(|s| !s.is_empty())
}

/// The reroll panel's affix table for an item class (`<type>Table`;
/// key order: weapons by weapon type, daggers and scepters both on
/// `caster1hTable`, armour by slot).
fn reroll_table_field(item_class: &str) -> Option<&'static str> {
    Some(match item_class {
        "WeaponMelee_Axe" => "axe1hTable",
        "WeaponMelee_Axe2h" => "axe2hTable",
        "WeaponMelee_Mace" => "mace1hTable",
        "WeaponMelee_Mace2h" => "mace2hTable",
        "WeaponMelee_Sword" => "sword1hTable",
        "WeaponMelee_Sword2h" => "sword2hTable",
        "WeaponMelee_Spear2h" => "spear2hTable",
        "WeaponHunting_Ranged1h" => "ranged1hTable",
        "WeaponHunting_Ranged2h" => "ranged2hTable",
        "WeaponMelee_Dagger" | "WeaponMelee_Scepter" => "caster1hTable",
        "WeaponArmor_Offhand" => "focusTable",
        "WeaponArmor_Shield" => "shieldTable",
        "ArmorProtective_Head" => "headTable",
        "ArmorProtective_Shoulders" => "shouldersTable",
        "ArmorProtective_Chest" => "chestTable",
        "ArmorProtective_Waist" => "waistTable",
        "ArmorProtective_Hands" => "handsTable",
        "ArmorProtective_Legs" => "legsTable",
        "ArmorProtective_Feet" => "feetTable",
        "ArmorJewelry_Medal" => "medalTable",
        "ArmorJewelry_Amulet" => "necklaceTable",
        "ArmorJewelry_Ring" => "ringTable",
        _ => return None,
    })
}

/// A new prefix (or suffix) for an item at the Inventor (
/// GrimDawn.exe `sub_1401B7CD0` modes 2 / 3): the item type's table loaded
/// at `targetLevelEquation` (stock: `itemLevel`, at least 1); each of up to
/// 100 tries draws from the rare tables when a 0..100 roll is ≤ `rareChance`
/// or the current affix is rare (`itemClassification` Rare), else from the
/// normal ones; the first name differing from `current` wins.
pub fn reroll_affix(
    db: &crate::items::ItemDb,
    item_class: &str,
    item_level: u32,
    prefix: bool,
    current: &str,
    rng: &mut Rng,
) -> Option<String> {
    use crate::items::db::Fields;
    let panel = db.get(REROLL_PANEL)?;
    let table = panel.string(reroll_table_field(item_class)?).to_owned();
    // exe `sub_1401B7900` / resolver `sub_1401B86B0`: `itemLevel` is
    // the item's level (the stock equation is just that); the game also
    // offers parentLevel = character level, averagePlayerLevel,
    // gameDifficulty and min/maxPlayerLevel = numberOfPlayers = 1, not
    // provided here.
    let level = Expr::parse(panel.string("targetLevelEquation")).map_or(item_level as f32, |e| {
        e.eval(&|v| {
            v.eq_ignore_ascii_case("itemLevel")
                .then_some(item_level as f32)
        })
    });
    let level = (level.max(1.0) + 0.5) as u32;
    let rare_chance = panel.float("rareChance");
    let current_rare = !current.is_empty()
        && db
            .get(current)
            .is_some_and(|r| r.string("itemClassification") == "Rare");
    for _ in 0..100 {
        let rare = (rng.next() as f32) * 4.656_613e-8 <= rare_chance || current_rare;
        let a = crate::items::loot::affix_from_table(db, &table, level, prefix, rare, rng);
        if !a.is_empty() && !a.eq_ignore_ascii_case(current) {
            return Some(a);
        }
    }
    None
}

/// The item ascension window's recipe for an item (`itemascension_table.dbr`
/// `<rarity>Recipe`; Rare / Epic / Legendary, anything else common,
/// GrimDawn.exe `sub_140112610`).
pub fn ascension_recipe(db: &crate::items::ItemDb, classification: &str) -> Option<String> {
    use crate::items::db::Fields;
    let key = match classification {
        "Rare" => "rare",
        "Epic" => "epic",
        "Legendary" => "legendary",
        _ => "common",
    };
    Some(
        db.get(ASCENSION_WINDOW)?
            .string(&format!("{key}Recipe"))
            .to_owned(),
    )
    .filter(|s| !s.is_empty())
}

pub const ASCENSION_WINDOW: &str = "records/ui/itemascension/itemascension_table.dbr";

/// An ascendant affix from the item ascension window (
/// GrimDawn.exe `sub_140114680`: `GetAscendedName` with the character's
/// masteries, its table rolled at the character level).
pub fn ascend_item(
    db: &crate::items::ItemDb,
    item_class: &str,
    classification: &str,
    masteries: &[usize],
    char_level: u32,
    rng: &mut Rng,
) -> Option<String> {
    let recipe = ascension_recipe(db, classification)?;
    ascended_affix(
        db,
        &recipe,
        item_group(item_class)?,
        masteries,
        char_level,
        rng,
    )
}

/// The reroll panel's recipe for an affix reroll (`affixRecipe`).
pub fn affix_recipe(db: &crate::items::ItemDb) -> Option<String> {
    use crate::items::db::Fields;
    Some(db.get(REROLL_PANEL)?.string("affixRecipe").to_owned()).filter(|s| !s.is_empty())
}

fn rarity_key(classification: &str) -> Option<&'static str> {
    Some(match classification {
        "Common" | "Magical" => "Common",
        "Rare" => "Rare",
        "Epic" => "Epic",
        "Legendary" => "Legendary",
        _ => return None,
    })
}

fn item_group(item_class: &str) -> Option<&'static str> {
    Some(match item_class {
        "ArmorProtective_Head"
        | "ArmorProtective_Chest"
        | "ArmorProtective_Legs"
        | "ArmorProtective_Feet"
        | "ArmorProtective_Hands"
        | "ArmorProtective_Shoulders" => "armor",
        "ArmorJewelry_Amulet"
        | "ArmorJewelry_Ring"
        | "ArmorJewelry_Medal"
        | "ArmorProtective_Waist" => "accessory",
        "WeaponMelee_Axe"
        | "WeaponMelee_Sword"
        | "WeaponMelee_Mace"
        | "WeaponMelee_Dagger"
        | "WeaponMelee_Scepter" => "oneHandMelee",
        "WeaponHunting_Ranged1h" => "oneHandRanged",
        "WeaponMelee_Axe2h"
        | "WeaponMelee_Sword2h"
        | "WeaponMelee_Mace2h"
        | "WeaponMelee_Spear2h" => "twoHandMelee",
        "WeaponHunting_Ranged2h" => "twoHandRanged",
        "WeaponArmor_Shield" => "shield",
        "WeaponArmor_Offhand" => "offhand",
        _ => return None,
    })
}

/// `ItemAscensionFormula::GetRandomizerName` + the table roll at `level`.
fn ascended_affix(
    db: &crate::items::ItemDb,
    recipe: &str,
    group: &str,
    masteries: &[usize],
    level: u32,
    rng: &mut Rng,
) -> Option<String> {
    use crate::items::db::Fields;
    let f = db.get(recipe)?;
    let mastery_tables = f.strs(&format!("{group}TablesMastery"));
    let affix_tables = f.strs(&format!("{group}TablesAffix"));
    let mw = f.int("masteryWeight");
    let mw = if masteries.len() == 1 {
        (mw as f32 * 0.375) as i32
    } else {
        mw
    };
    let mut list: Vec<(String, i32)> = masteries
        .iter()
        .filter_map(|&m| mastery_tables.get(m).map(|t| (t.to_string(), mw)))
        .collect();
    // The affix-table pick draws even when the list is empty (`% max(n, 1)`).
    let i = (rng.next() % (affix_tables.len() as u32).max(1)) as usize;
    if let Some(t) = affix_tables.get(i) {
        list.push((t.to_string(), f.int("affixWeight")));
    }
    // (bad data: the engine's int sum; i64 and ≥ 0 so huge / negative
    // weights neither overflow nor wrap the modulus.)
    let total = list.iter().map(|l| l.1 as i64).sum::<i64>().max(0);
    let roll = (rng.next() as u64 % (total as u64 + 1)) as i64;
    let mut acc = 0i64;
    let table = list.iter().find(|(_, w)| {
        acc += *w as i64;
        roll <= acc
    })?;
    let affix = crate::items::loot::randomizer_pick(db, &table.0, level, rng);
    (!affix.is_empty()).then_some(affix)
}
