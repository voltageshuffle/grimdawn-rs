//! Character stats: the attribute accumulator, base stats from character and
//! bio records, level/XP tables and the derived OA/DA (Game.dll).
use crate::attributes::{AttrFamily, CHAR_ATTR_SLOTS, CharAttr, CombatType, family};
use anyhow::{Context, Result, bail};
use gd_formats::arz::{RawValue, Record};
use gd_rules::expr::Expr;

/// `0.0099999998f` — the f32 the engine multiplies by for "/100".
pub const PCT: f32 = 0.01;

/// `GAME::CharAttributeAccumulator`: per [`CharAttr`] a flat value, a
/// percentage modifier and a multiplier (3 x 58 f32).
#[derive(Debug, Clone, PartialEq)]
pub struct CharAttrAccumulator {
    pub value: [f32; CHAR_ATTR_SLOTS],
    pub modifier: [f32; CHAR_ATTR_SLOTS],
    pub multiplier: [f32; CHAR_ATTR_SLOTS],
}

impl Default for CharAttrAccumulator {
    fn default() -> Self {
        Self {
            value: [0.0; CHAR_ATTR_SLOTS],
            modifier: [0.0; CHAR_ATTR_SLOTS],
            multiplier: [0.0; CHAR_ATTR_SLOTS],
        }
    }
}

impl CharAttrAccumulator {
    pub fn add_value(&mut self, a: CharAttr, v: f32) {
        self.value[a as usize] += v;
    }
    /// `characterTotalSpeedModifier` (`CharAttributeMod_TotalSpeed::AddToAccumulator`)
    /// adds its value to the run,
    /// attack and cast speed modifiers; the slot itself stays unused.
    pub fn add_modifier(&mut self, a: CharAttr, v: f32) {
        if a == CharAttr::TotalSpeed {
            for t in [
                CharAttr::RunSpeed,
                CharAttr::AttackSpeed,
                CharAttr::CastSpeed,
            ] {
                self.modifier[t as usize] += v;
            }
            return;
        }
        self.modifier[a as usize] += v;
    }
    pub fn add_multiplier(&mut self, a: CharAttr, v: f32) {
        self.multiplier[a as usize] += v;
    }

    /// `CharAttributeAccumulator::GetValue` (exact f32
    /// order): `m = max((mult + 1) * 100, 0)`; positive values become
    /// `max(((mod * 0.01 + 1) * v * m) * 0.01, 0)`, negative ones
    /// `min(((|v| * mod) * 0.01 + v) * m * 0.01, 0)`.
    pub fn get(&self, a: CharAttr) -> f32 {
        let i = a as usize;
        let (v, m) = (self.value[i], self.modifier[i]);
        let k = ((self.multiplier[i] + 1.0) * 100.0).max(0.0);
        if v < 0.0 {
            ((((v.abs() * m) * PCT) + v) * k * PCT).min(0.0)
        } else {
            ((((m * PCT) + 1.0) * v) * k * PCT).max(0.0)
        }
    }

    /// Adds every `character*` field of a record (item, skill level, buff).
    /// `level_index` picks from per-level arrays like the engine
    /// (`min(level - 1, len - 1)`).
    pub fn add_record(&mut self, rec: &Record, level_index: usize) {
        for (fam, f) in [
            (
                AttrFamily::CharacterValue,
                Self::add_value as fn(&mut Self, CharAttr, f32),
            ),
            (AttrFamily::CharacterModifier, Self::add_modifier),
            (AttrFamily::CharacterMultiplier, Self::add_multiplier),
        ] {
            for d in family(fam) {
                let (Some(field), Some(attr)) = (d.value, d.char_attr()) else {
                    continue;
                };
                if let Some(v) = level_value(rec, field, level_index) {
                    f(self, attr, v);
                }
            }
        }
    }
}

/// Per-level array lookup used by every attribute class
/// (`index = min(level - 1, len - 1)`, 0 when absent).
pub fn level_value(rec: &Record, field: &str, level_index: usize) -> Option<f32> {
    let mut last = None;
    for (i, v) in rec
        .get(field)?
        .iter()
        .filter_map(RawValue::as_f32)
        .enumerate()
    {
        if i == level_index {
            return Some(v);
        }
        last = Some(v);
    }
    last
}

/// Per-record parsed field equations ([`Record::derived`] slot 2): a field's equation string
/// parsed once and shared by the record's clones. Bio, proxy and loot equations are otherwise
/// re-parsed per spawn / roll. A parse failure is cached as `None`.
#[derive(Default)]
pub(crate) struct RecordExprs(
    parking_lot::Mutex<gd_formats::collections::HashMap<Box<str>, Option<std::sync::Arc<Expr>>>>,
);

/// [`Expr::parse`] of `src` cached on `rec` under `field` (slot 2); `None` on a parse error. The
/// caller reads the field's string first (absent / empty fields are not cached here).
pub(crate) fn record_expr(rec: &Record, field: &str, src: &str) -> Option<std::sync::Arc<Expr>> {
    match rec.derived::<2, _>(RecordExprs::default) {
        Some(cache) => {
            let mut m = cache.0.lock();
            if let Some(e) = m.get(field) {
                return e.clone();
            }
            let e = Expr::parse(src).ok().map(std::sync::Arc::new);
            m.insert(field.into(), e.clone());
            e
        }
        None => Expr::parse(src).ok().map(std::sync::Arc::new),
    }
}

/// Life/energy granted per point of Physique/Cunning/Spirit gained from a
/// contribution source, computed at load from `playerlevels.dbr`
/// (`ExperienceLevelControl::Load`): `lifeIncrement/strengthIncrement`,
/// `lifeIncrementDexterity/dexterityIncrement`,
/// `lifeIncrementIntelligence * (1/intelligenceIncrement)`,
/// `manaIncrement * (1/intelligenceIncrement)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttributeRates {
    pub life_per_physique: f32,
    pub life_per_cunning: f32,
    pub life_per_spirit: f32,
    pub energy_per_spirit: f32,
}

impl CharAttrAccumulator {
    /// Applies one contribution source the way `Character::GetSkillCharAttributes`,
    /// `GetEquipCharAttributes` and
    /// `ContributeMiscCharAttributes` do: the change in
    /// *effective* Physique/Cunning/Spirit caused by the source is converted
    /// to flat life/energy, in this exact order.
    pub fn contribute(&mut self, rates: &AttributeRates, source: impl FnOnce(&mut Self)) {
        let p0 = self.get(CharAttr::Physique);
        let c0 = self.get(CharAttr::Cunning);
        let s0 = self.get(CharAttr::Spirit);
        source(self);
        let dc = self.get(CharAttr::Cunning) - c0;
        let ds = self.get(CharAttr::Spirit) - s0;
        let life = (self.get(CharAttr::Physique) - p0) * rates.life_per_physique
            + self.value[CharAttr::Life as usize];
        let life = dc * rates.life_per_cunning + life;
        self.value[CharAttr::Life as usize] = ds * rates.life_per_spirit + life;
        self.value[CharAttr::Energy as usize] += ds * rates.energy_per_spirit;
    }
}

/// `records/creatures/pc/playerlevels.dbr` (the engine's experience level
/// control, loaded by `ExperienceLevelControl::Load` from `levelFileName`).
#[derive(Debug, Clone)]
pub struct LevelTable {
    pub strength_increment: i32,
    pub dexterity_increment: i32,
    pub intelligence_increment: i32,
    pub life_increment: i32,
    pub life_increment_dexterity: i32,
    pub life_increment_intelligence: i32,
    pub mana_increment: i32,
    /// Attribute points per level (`characterModifierPoints`).
    pub attribute_points_per_level: i32,
    /// Skill points granted on reaching level `i + 2` (`skillModifierPoints[i]`). PORT: the
    /// indexing is inferred from the data, not read off the engine's level-up code.
    pub skill_points: Vec<i32>,
    pub initial_skill_points: i32,
    pub max_devotion_points: i32,
    pub max_level: i32,
    pub experience_equation: Expr,
}

impl LevelTable {
    pub fn from_record(rec: &Record) -> Result<Self> {
        let i = |k: &str| {
            rec.i32(k)
                .or_else(|| rec.f32(k).map(|f| f as i32))
                .unwrap_or(0)
        };
        // `rates()` divides by these (bad data: a missing or non-positive
        // increment would make every Life/Energy rate NaN or infinite).
        for k in [
            "strengthIncrement",
            "dexterityIncrement",
            "intelligenceIncrement",
        ] {
            if i(k) <= 0 {
                bail!("{}: {k} must be positive", rec.name);
            }
        }
        Ok(Self {
            strength_increment: i("strengthIncrement"),
            dexterity_increment: i("dexterityIncrement"),
            intelligence_increment: i("intelligenceIncrement"),
            life_increment: i("lifeIncrement"),
            life_increment_dexterity: i("lifeIncrementDexterity"),
            life_increment_intelligence: i("lifeIncrementIntelligence"),
            mana_increment: i("manaIncrement"),
            attribute_points_per_level: i("characterModifierPoints"),
            skill_points: rec
                .f32s("skillModifierPoints")
                .into_iter()
                .map(|f| f as i32)
                .collect(),
            initial_skill_points: i("initialSkillPoints"),
            max_devotion_points: i("maxDevotionPoints"),
            max_level: i("maxPlayerLevel"),
            experience_equation: Expr::parse(
                rec.str("experienceLevelEquation")
                    .context("experienceLevelEquation")?,
            )?,
        })
    }

    /// Exact f32 rates (the `ExperienceLevelControl.*Increment` fields).
    pub fn rates(&self) -> AttributeRates {
        let inv_int = 1.0 / self.intelligence_increment as f32;
        AttributeRates {
            life_per_physique: self.life_increment as f32 / self.strength_increment as f32,
            life_per_cunning: self.life_increment_dexterity as f32
                / self.dexterity_increment as f32,
            life_per_spirit: self.life_increment_intelligence as f32 * inv_int,
            energy_per_spirit: self.mana_increment as f32 * inv_int,
        }
    }

    /// Experience listed for `level` by `Player::DumpExperienceLevels`
    /// (`ExperienceLevelControl::GetExperienceForLevel`): `(int)(eq(playerLevel = level - 1) + 0.5)`,
    /// i.e. the cumulative total needed to *reach* `level` (the
    /// level-up loop `ExperienceLevelControl::AddExperience`; see
    /// `progress::Progress::threshold`).
    pub fn experience_for_level(&self, level: i32) -> i64 {
        let pl = (level - 1) as f32;
        let v = self
            .experience_equation
            .eval(&|n| (n.eq_ignore_ascii_case("playerLevel")).then_some(pl));
        (v + 0.5) as i64
    }
}

/// Combat formula record (`records/game/combatformulas.dbr`), loaded by
/// `CombatManager::LoadRecord`.
#[derive(Debug, Clone)]
pub struct CombatFormulas {
    pub offensive_ability: Option<Expr>,
    pub defensive_ability: Option<Expr>,
    pub probability_to_hit: Option<Expr>,
    pub normal_pth: Option<Expr>,
    pub physical_damage: Option<Expr>,
    pub physical_duration_damage: Option<Expr>,
    pub pierce_damage: Option<Expr>,
    pub magical_damage: Option<Expr>,
    pub magical_duration_damage: Option<Expr>,
    /// `physicalDamagePercentage` / `physicalDamageBonus` (`CombatManager.physicalDamagePercentage` /
    /// `CombatManager.physicalDamageBonus`, `CombatManager::LoadRecord`): only the character window's
    /// Cunning rollover reads them (`DesignerCalculatePhysicalDamagePercentage`
    /// / `DesignerCalculatePhysicalDamageBonus`).
    pub physical_damage_percentage: Option<Expr>,
    pub physical_damage_bonus: Option<Expr>,
    /// `physcialDamageDefenseEquationDLEP` (protection >= damage). Note the
    /// field-name typo is the engine's.
    pub physical_defense_dlep: Option<Expr>,
    /// `physicalDamageDefenseEquationDGP` (damage > protection).
    pub physical_defense_dgp: Option<Expr>,
    pub melee_block: Option<Expr>,
    pub projectile_block: Option<Expr>,
    /// `shieldDamageReductionEquationDGB` (damage > block amount).
    pub shield_dgb: Option<Expr>,
    /// `shieldDamageReductionEquationDLEB` (damage <= block amount).
    pub shield_dleb: Option<Expr>,
    pub physics_strength: Option<Expr>,
    pub physics_strength_min: f32,
    pub physics_strength_max: f32,
    /// `pthMinimum` (constructor default 66.0 when absent).
    pub pth_minimum: f32,
    /// `(pthThresholdN, pthDamageModifierN)`, N = 1..6.
    pub pth_tiers: [(f32, f32); 6],
    /// Region pick table in load order (`PickRegion` walks it cumulatively).
    pub regions: Vec<(Region, i32)>,
}

/// `GAME::CombatAttributeRegion` (ids from `CombatManager::LoadRecord`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Region {
    FullyProtected = 0,
    Unprotected = 1,
    Head = 2,
    Torso = 3,
    Arms = 4,
    Legs = 5,
    Feet = 6,
    Shoulders = 8,
}

fn opt_expr(rec: &Record, k: &str) -> Result<Option<Expr>> {
    match rec.str(k) {
        Some(s) if !s.is_empty() => Ok(Some(
            Expr::parse_with(s, &CombatVars::slot).with_context(|| k.to_owned())?,
        )),
        _ => Ok(None),
    }
}

impl CombatFormulas {
    pub fn from_record(rec: &Record) -> Result<Self> {
        let f = |k: &str| rec.f32(k).unwrap_or(0.0);
        let mut tiers = [(0.0, 0.0); 6];
        for (n, t) in tiers.iter_mut().enumerate() {
            *t = (
                f(&format!("pthThreshold{}", n + 1)),
                f(&format!("pthDamageModifier{}", n + 1)),
            );
        }
        let regions = [
            (Region::FullyProtected, "combatRegionFullyProtectedChance"),
            (Region::Unprotected, "combatRegionUnprotectedChance"),
            (Region::Head, "combatRegionHeadChance"),
            (Region::Torso, "combatRegionTorsoChance"),
            (Region::Arms, "combatRegionArmsChance"),
            (Region::Legs, "combatRegionLegsChance"),
            (Region::Feet, "combatRegionFeetChance"),
            (Region::Shoulders, "combatRegionShouldersChance"),
        ]
        .into_iter()
        .map(|(r, k)| {
            (
                r,
                rec.i32(k)
                    .or_else(|| rec.f32(k).map(|v| v as i32))
                    .unwrap_or(0),
            )
        })
        .collect();
        Ok(Self {
            offensive_ability: opt_expr(rec, "offensiveAbilityEquation")?,
            defensive_ability: opt_expr(rec, "defensiveAbilityEquation")?,
            probability_to_hit: opt_expr(rec, "probabilityToHitEquation")?,
            normal_pth: opt_expr(rec, "normalPTHEquation")?,
            physical_damage: opt_expr(rec, "physicalDamageEquation")?,
            physical_duration_damage: opt_expr(rec, "physicalDurationDamageEquation")?,
            pierce_damage: opt_expr(rec, "pierceDamageEquation")?,
            magical_damage: opt_expr(rec, "magicalDamageEquation")?,
            magical_duration_damage: opt_expr(rec, "magicalDurationDamageEquation")?,
            physical_damage_percentage: opt_expr(rec, "physicalDamagePercentage")?,
            physical_damage_bonus: opt_expr(rec, "physicalDamageBonus")?,
            physical_defense_dlep: opt_expr(rec, "physcialDamageDefenseEquationDLEP")?,
            physical_defense_dgp: opt_expr(rec, "physicalDamageDefenseEquationDGP")?,
            melee_block: opt_expr(rec, "meleeBlockEquation")?,
            projectile_block: opt_expr(rec, "projectileBlockEquation")?,
            shield_dgb: opt_expr(rec, "shieldDamageReductionEquationDGB")?,
            shield_dleb: opt_expr(rec, "shieldDamageReductionEquationDLEB")?,
            physics_strength: opt_expr(rec, "physicsStrengthEquation")?,
            physics_strength_min: f("physicsStrengthMin"),
            physics_strength_max: f("physicsStrengthMax"),
            pth_minimum: rec.f32("pthMinimum").unwrap_or(66.0),
            pth_tiers: tiers,
            regions,
        })
    }

    /// The stock `combatformulas.dbr` (as merged base+expansions), for tests
    /// that run without game data.
    #[cfg(test)]
    pub(crate) fn stock() -> Self {
        let p = |s: &str| Some(Expr::parse_with(s, &CombatVars::slot).unwrap());
        Self {
            offensive_ability: p(
                "(offensiveAbilityDV + (characterLevelDV * 12) + ((dexterityDV + bonusDV) *0.5)) * (1 + (offensiveAbilityModifierDV / 100))+53",
            ),
            defensive_ability: p(
                "(defensiveAbilityDV + (characterLevelDV * 12) + ((strengthDV + bonusDV) *0.5)) * (1 + (defensiveAbilityModifierDV / 100))+53",
            ),
            probability_to_hit: p(
                "((((offensiveAbilityDV/((defensiveAbilityDV/3.5)+offensiveAbilityDV))*300)*0.3)+(((((offensiveAbilityDV*3.25)+10000) - (defensiveAbilityDV*3.25))/100)*0.7))-50",
            ),
            normal_pth: p("probabilityToHitDV/70"),
            physical_damage: p("(physicalDamageDV*((dexterityDV/245)+1))"),
            physical_duration_damage: p("(physicalDamageDV*((dexterityDV/215)+1))"),
            pierce_damage: p("pierceDamageDV*((dexterityDV/245)+1)"),
            magical_damage: p("magicalDamageDV*((intelligenceDV/215)+1)"),
            magical_duration_damage: p("magicalDamageDV*((intelligenceDV/200)+1)"),
            physical_damage_percentage: p("(physicalDamageDV*((dexterityDV/245)+1))"),
            physical_damage_bonus: p("0"),
            physical_defense_dlep: p("physicalDamageDV * (1 - sumAbsorptionDV)"),
            physical_defense_dgp: p(
                "(sumProtectionDV * (1 - sumAbsorptionDV)) + (physicalDamageDV - sumProtectionDV)",
            ),
            melee_block: p("(blockChanceDV + blockChanceModifierDV)"),
            projectile_block: p("(blockChanceDV + blockChanceModifierDV)"),
            shield_dgb: p("damageDV - (shieldDefenseDV * (shieldAbsorptionDV / 100))"),
            shield_dleb: p("damageDV * ((100 - shieldAbsorptionDV) / 100 )"),
            physics_strength: p("(2*(damage/(maxLife*.3)))+4"),
            physics_strength_min: 7.0,
            physics_strength_max: 15.0,
            pth_minimum: 55.0,
            pth_tiers: [
                (70.0, 1.0),
                (90.0, 1.1),
                (105.0, 1.2),
                (120.0, 1.3),
                (130.0, 1.4),
                (135.0, 1.5),
            ],
            regions: vec![
                (Region::FullyProtected, 0),
                (Region::Unprotected, 0),
                (Region::Head, 15),
                (Region::Torso, 26),
                (Region::Arms, 12),
                (Region::Legs, 20),
                (Region::Feet, 12),
                (Region::Shoulders, 15),
            ],
        }
    }
}

/// Values the `CombatManager` equations can see (`ResolveEquationVariable`).
/// Unset variables read 0 like the engine.
#[derive(Debug, Clone, Copy, Default)]
pub struct CombatVars {
    pub physical_damage: f32,
    pub pierce_damage: f32,
    pub magical_damage: f32,
    pub sum_protection: f32,
    pub sum_absorption: f32,
    pub bonus: f32,
    pub offensive_ability: f32,
    pub offensive_ability_modifier: f32,
    pub defensive_ability: f32,
    pub defensive_ability_modifier: f32,
    pub probability_to_hit: f32,
    pub block_chance: f32,
    pub block_chance_modifier: f32,
    pub damage_dv: f32,
    pub shield_defense: f32,
    pub shield_absorption: f32,
    /// `damage` (physics strength input).
    pub damage: f32,
    /// `strengthDV` = effective Physique.
    pub strength: f32,
    /// `dexterityDV` = effective Cunning.
    pub dexterity: f32,
    /// `intelligenceDV` = effective Spirit.
    pub intelligence: f32,
    pub character_level: f32,
    pub max_life: f32,
}

impl CombatVars {
    /// The variable names in slot order (lower case; [`Self::slot`]).
    const NAMES: [&'static str; 22] = [
        "physicaldamagedv",
        "piercedamagedv",
        "magicaldamagedv",
        "sumprotectiondv",
        "sumabsorptiondv",
        "bonusdv",
        "offensiveabilitydv",
        "offensiveabilitymodifierdv",
        "defensiveabilitydv",
        "defensiveabilitymodifierdv",
        "probabilitytohitdv",
        "blockchancedv",
        "blockchancemodifierdv",
        "damagedv",
        "shielddefensedv",
        "shieldabsorptiondv",
        "damage",
        "strengthdv",
        "dexteritydv",
        "intelligencedv",
        "characterleveldv",
        "maxlife",
    ];

    /// The slot of an equation variable (ASCII case-insensitive), for
    /// [`Expr::parse_with`].
    pub fn slot(name: &str) -> Option<u8> {
        Self::NAMES
            .iter()
            .position(|k| name.eq_ignore_ascii_case(k))
            .map(|i| i as u8)
    }

    /// The values in [`Self::NAMES`] order.
    fn values(&self) -> [f32; 22] {
        [
            self.physical_damage,
            self.pierce_damage,
            self.magical_damage,
            self.sum_protection,
            self.sum_absorption,
            self.bonus,
            self.offensive_ability,
            self.offensive_ability_modifier,
            self.defensive_ability,
            self.defensive_ability_modifier,
            self.probability_to_hit,
            self.block_chance,
            self.block_chance_modifier,
            self.damage_dv,
            self.shield_defense,
            self.shield_absorption,
            self.damage,
            self.strength,
            self.dexterity,
            self.intelligence,
            self.character_level,
            self.max_life,
        ]
    }

    /// `e` with these values ([`CombatFormulas`] equations are parsed with
    /// their variables resolved to slots; any other expression resolves its
    /// names here).
    pub fn eval(&self, e: &Expr) -> f32 {
        e.eval_slots(&self.values(), &Self::slot)
    }

    /// Character-dependent variables from the final accumulator.
    pub fn for_character(acc: &CharAttrAccumulator, level: i32) -> Self {
        Self {
            strength: acc.get(CharAttr::Physique),
            dexterity: acc.get(CharAttr::Cunning),
            intelligence: acc.get(CharAttr::Spirit),
            character_level: level as f32,
            max_life: acc.get(CharAttr::Life),
            ..Default::default()
        }
    }
}

impl CombatFormulas {
    /// `DesignerCalculateOffensiveAbility(bonus)`:
    /// `max(eq - offensiveAbilityDebuff, 0)` where the debuff is the
    /// duration-damage manager's `OffensiveAbility` (22) total.
    pub fn offensive_ability(
        &self,
        acc: &CharAttrAccumulator,
        level: i32,
        bonus: f32,
        debuff: f32,
    ) -> f32 {
        let mut v = CombatVars::for_character(acc, level);
        v.bonus = bonus;
        v.offensive_ability = acc.get(CharAttr::OffensiveAbility);
        v.offensive_ability_modifier = acc.get(CharAttr::OffensiveAbilityModifier);
        let r = self.offensive_ability.as_ref().map_or(0.0, |e| v.eval(e));
        (r - debuff).max(0.0)
    }

    /// `DesignerCalculateDefensiveAbility(bonus)`;
    /// `TakeAttack` always passes bonus 0.
    pub fn defensive_ability(
        &self,
        acc: &CharAttrAccumulator,
        level: i32,
        bonus: f32,
        debuff: f32,
    ) -> f32 {
        let mut v = CombatVars::for_character(acc, level);
        v.bonus = bonus;
        v.defensive_ability = acc.get(CharAttr::DefensiveAbility);
        v.defensive_ability_modifier = acc.get(CharAttr::DefensiveAbilityModifier);
        let r = self.defensive_ability.as_ref().map_or(0.0, |e| v.eval(e));
        (r - debuff).max(0.0)
    }
}

/// Engine constants from `records/game/gameengine.dbr` that combat reads.
#[derive(Debug, Clone)]
pub struct EngineConstants {
    /// Resistance cap per difficulty (`playerDefenseCap`, Normal/Elite/Ultimate);
    /// returned by `Player::GetDefenseAttributeCap`.
    pub player_defense_cap: [f32; 3],
    pub monster_defense_cap: [f32; 3],
    pub player_pet_defense_cap: [f32; 3],
    pub player_reflect_cap: f32,
    /// Absorption % given to every armor piece (`armorDefensiveAbsorption`,
    /// read by `DefenseAttribute_BaseProtectionAbsorption::LoadFromTable`).
    pub armor_absorption: f32,
}

impl EngineConstants {
    pub fn from_record(rec: &Record) -> Self {
        let tri = |k: &str| {
            let v = rec.f32s(k);
            let g = |i: usize| v.get(i).or(v.last()).copied().unwrap_or(0.0);
            [g(0), g(1), g(2)]
        };
        Self {
            player_defense_cap: tri("playerDefenseCap"),
            monster_defense_cap: tri("monsterDefenseCap"),
            player_pet_defense_cap: tri("playerPetDefenseCap"),
            player_reflect_cap: rec.f32("playerReflectCap").unwrap_or(0.0),
            armor_absorption: rec.f32("armorDefensiveAbsorption").unwrap_or(0.0),
        }
    }
}

/// Base character values: the `CharAttributeStore_Bio` loaded from the
/// creature record (`characterStrength`, `characterLife`, ...), optionally
/// overridden by a bio equation record (`characterAttributeEquations`).
#[derive(Debug, Clone, Default)]
pub struct BioStore {
    pub base: CharAttrAccumulator,
    /// `characterLifeRegen`/`characterManaRegen` equations: the creature
    /// record's own field (a number is a constant equation) or the bio
    /// equation record's (`characterAttributeEquations`), variables
    /// `lifeRegen`, `lifeRegenMod`, `lifeTotal`, `charLevel`, `elapsedTime`,
    /// ... ([`Self::life_regen_per_sec`]).
    pub life_regen_equation: Option<Expr>,
    pub mana_regen_equation: Option<Expr>,
}

/// Slots `CharacterBio::EvaluateEquations` fills from the bio
/// equation record, in order.
pub const BIO_EQUATION_SLOTS: [(CharAttr, &str); 9] = [
    (CharAttr::Physique, "characterStrength"),
    (CharAttr::Cunning, "characterDexterity"),
    (CharAttr::Spirit, "characterIntelligence"),
    (CharAttr::Life, "characterLife"),
    (CharAttr::Energy, "characterMana"),
    (CharAttr::OffensiveAbility, "characterOffensiveAbility"),
    (CharAttr::DefensiveAbility, "characterDefensiveAbility"),
    (CharAttr::LifeRegen, "characterLifeRegen"),
    (CharAttr::EnergyRegen, "characterManaRegen"),
];

impl BioStore {
    /// Flat `character*` values of a creature record (index 0 of arrays).
    pub fn from_record(rec: &Record) -> Self {
        let mut base = CharAttrAccumulator::default();
        for d in family(AttrFamily::CharacterValue) {
            let (Some(field), Some(attr)) = (d.value, d.char_attr()) else {
                continue;
            };
            // A field can hold an equation string in bio records; flat
            // numbers only here.
            if let Some(v) = rec
                .get(field)
                .and_then(|vs| vs.first())
                .and_then(RawValue::as_f32)
            {
                base.value[attr as usize] = v;
            }
        }
        // `CharacterBio::Load` keeps the regen fields as equations (a flat
        // number is a constant one); `GetLifeRegenValue` returns 0 without
        // an equation, so the flat attribute alone regens nothing.
        let equation = |field: &str| match rec.get(field).and_then(|v| v.first()) {
            Some(RawValue::Str(s)) if !s.is_empty() => {
                record_expr(rec, field, s).map(|e| (*e).clone())
            }
            Some(other) => Some(Expr::Num(other.as_f32().unwrap_or(0.0))),
            None => None,
        };
        Self {
            base,
            life_regen_equation: equation("characterLifeRegen"),
            mana_regen_equation: equation("characterManaRegen"),
        }
    }

    /// Evaluates a bio equation record at `char_level` (`charLevel`
    /// variable `CharacterBio::ResolveEquationVariable`), replacing
    /// the base values like `EvaluateEquations` does.
    pub fn apply_equations(&mut self, bio: &Record, char_level: i32) -> Result<()> {
        for (attr, field) in BIO_EQUATION_SLOTS {
            let Some(src) = bio.get(field).and_then(|v| v.first()).cloned() else {
                continue;
            };
            let e = match src {
                RawValue::Str(s) if !s.is_empty() => match record_expr(bio, field, &s) {
                    Some(e) => (*e).clone(),
                    None => Expr::parse(&s).with_context(|| field.to_owned())?,
                },
                RawValue::Str(_) => continue,
                other => Expr::Num(other.as_f32().unwrap_or(0.0)),
            };
            match attr {
                CharAttr::LifeRegen => self.life_regen_equation = Some(e),
                CharAttr::EnergyRegen => self.mana_regen_equation = Some(e),
                _ => {
                    let lvl = char_level as f32;
                    self.base.value[attr as usize] =
                        e.eval(&|n| n.eq_ignore_ascii_case("charLevel").then_some(lvl));
                }
            }
        }
        Ok(())
    }

    /// `CharacterBio::GetLifeRegenValue` at elapsedTime 1: the
    /// life regen equation with `CharacterBio::ResolveEquationVariable`'s
    /// variables — `lifeRegen` the total regen attribute
    /// (`GetValue(6)`), `lifeRegenMod` its modifier %, `lifeTotal` the total
    /// life, `physique` / `intelligence` the totals, `charLevel`. No
    /// equation: 0. `CharacterBio::Update` adds the value every
    /// update for every character (no player gate); the player's sheet uses
    /// `playerresourcebehavior.dbr`'s equations instead
    /// ([`CharacterSheet::apply_player_regen`]).
    pub fn life_regen_per_sec(&self, level: i32, acc: &CharAttrAccumulator) -> f32 {
        self.life_regen_equation.as_ref().map_or(0.0, |e| {
            e.eval(&|n| {
                let is = |k: &str| n.eq_ignore_ascii_case(k);
                match () {
                    _ if is("charlevel") => Some(level as f32),
                    _ if is("liferegen") => Some(acc.get(CharAttr::LifeRegen)),
                    _ if is("liferegenmod") => Some(acc.modifier[CharAttr::LifeRegen as usize]),
                    _ if is("lifetotal") => Some(acc.get(CharAttr::Life)),
                    _ if is("physique") => Some(acc.get(CharAttr::Physique)),
                    _ if is("intelligence") => Some(acc.get(CharAttr::Spirit)),
                    _ if is("elapsedtime") => Some(1.0),
                    _ => None,
                }
            })
        })
    }
}

/// Attribute points spent by a player (one point = `*Increment` of the stat).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AllocatedPoints {
    pub physique: u32,
    pub cunning: u32,
    pub spirit: u32,
}

impl BioStore {
    /// `Character::IncrementBase{Strength,Dexterity,Intelligence}`
    /// add `*Increment` to the bio store. The life
    /// and energy that come with a point are separate `IncrementBaseLife(n)` /
    /// `IncrementBaseMana(n)` commands issued by the UI in Grim Dawn.exe with
    /// `GetStrengthLifeIncrement()` etc. (the getters are only
    /// exported for the exe; inside Game.dll only the `Character` constructor
    /// touches those members).
    pub fn allocate(&mut self, levels: &LevelTable, pts: AllocatedPoints) {
        // One command per point, each `value += (float)increment` (bad data:
        // a corrupt save's counts are capped at MAX_ALLOCATED, far above any
        // level-plus-quest total, so the per-point loop stays bounded).
        const MAX_ALLOCATED: u32 = 10_000;
        let b = &mut self.base.value;
        let mut add = |a: CharAttr, n: u32, inc: i32| {
            for _ in 0..n.min(MAX_ALLOCATED) {
                b[a as usize] += inc as f32;
            }
        };
        add(CharAttr::Physique, pts.physique, levels.strength_increment);
        add(CharAttr::Life, pts.physique, levels.life_increment);
        add(CharAttr::Cunning, pts.cunning, levels.dexterity_increment);
        add(CharAttr::Life, pts.cunning, levels.life_increment_dexterity);
        add(CharAttr::Spirit, pts.spirit, levels.intelligence_increment);
        add(
            CharAttr::Life,
            pts.spirit,
            levels.life_increment_intelligence,
        );
        add(CharAttr::Energy, pts.spirit, levels.mana_increment);
    }
}

/// Derived character sheet (what the character window shows).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CharacterSheet {
    pub level: i32,
    pub physique: f32,
    pub cunning: f32,
    pub spirit: f32,
    pub life: f32,
    pub energy: f32,
    /// Energy held by active toggled skills (`Character::GetReserveMana`:
    /// reserve × (1 − reduction %)).
    pub energy_reserve: f32,
    /// Raw `characterLifeRegen` / `characterManaRegen` attributes (inputs of
    /// the regen equations, not per-second values).
    pub life_regen: f32,
    pub energy_regen: f32,
    /// Per-second regeneration the character window shows and the bio adds
    /// each update (`CharacterBio::GetLifeRegenValue` / `GetManaRegenValue`
    /// at elapsedTime 1): the player's `playerresourcebehavior.dbr` equations
    /// ([`CharacterSheet::apply_player_regen`]), a creature's bio equation
    /// ([`BioStore::life_regen_per_sec`], set by `actor::monster_full`); the
    /// raw attribute until one of them runs.
    pub life_regen_per_sec: f32,
    pub energy_regen_per_sec: f32,
    pub offensive_ability: f32,
    pub defensive_ability: f32,
    pub dodge: f32,
    pub deflect: f32,
    /// `characterHealIncreasePercent` (`CharacterBio.healIncreasePercent`, `CharacterBio::TakeBonus`):
    /// flat, % and over-time heals × (1 + this %).
    pub heal_increase: f32,
    /// `characterEnergyAbsorptionPercent` total (`Character::GetTotalCharAttribute(26)`,
    /// the sheet's Energy Absorption row `CharacterWindow::UpdatePage2`).
    pub energy_absorption: f32,
    /// `characterDefensiveBlockRecoveryReduction` total (the sheet's Block
    /// Recovery row, `GetTotalCharAttribute(25)` capped at 100;
    /// `CharacterWindow::UpdatePage3`).
    pub block_recovery_reduction: f32,
    /// `characterPercentHealIncreaseModifier` (`CharacterBio.percentHealIncreaseModifier`): adds to the above
    /// for the instant % heal only.
    pub percent_heal_increase: f32,
    /// `characterConstitutionModifier` total (`GetTotalCharModifier(10)`, the
    /// sheet's Constitution Bonus row `CharacterWindow::UpdatePage2`).
    pub constitution_modifier: f32,
    /// Constitution limit (`GetConstitutionLimit` = `CharAttributeAccumulator::GetValue(10)`).
    pub constitution: f32,
    /// `characterIncreasedExperience` (`CharAttributeAccumulator::GetValue(55)`):
    /// every experience gain + round(xp × this %) (
    /// `Character::ReceiveExperience`).
    pub experience_bonus: f32,
    /// `characterIncreasedGold` (`CharAttributeAccumulator::GetValue(56)`):
    /// every gold pickup + floor(gold × this % + 0.5) (
    /// `Character::TakeBonus`, the bonus's gold).
    pub gold_bonus: f32,
    /// `characterLightRadius` total (`GetTotalCharAttribute(57)`, the sheet's
    /// Light Radius row), a percentage on the player light's radius
    /// ([`light_radius`]).
    pub light_radius: f32,
    /// A player's bio base values (zero for creatures), the split of the
    /// character window's rollovers (`gd_sim::sheet::numbers_and_rollovers`).
    pub base: BioBase,
}

/// The player bio's own values next to the totals (Game.dll):
/// `Character::GetBaseCharAttributes` reads the bio's base store
/// (`CharacterBio.baseAttributes`: the pc record, the save's values and the points
/// spent, `IncrementBase*`); `CharacterBio::GetOriginalCharAttribute`
/// the store `CharacterBio.originalAttributes`, loaded from the same pc record
/// (`CharacterBio::Load`) and never incremented.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BioBase {
    /// Base Physique, Cunning, Spirit, Life, Energy (`GetValue` 1, 2, 3, 4, 5).
    pub attributes: [f32; 5],
    /// Base `characterLifeRegen` / `characterManaRegen` (`GetValue` 6 / 7).
    pub regen: [f32; 2],
    /// The pc record's Physique, Cunning, Spirit.
    pub original: [f32; 3],
}

/// One contribution (adds a record's attributes to the accumulator).
pub type Source<'a> = Box<dyn Fn(&mut CharAttrAccumulator) + 'a>;

/// Accumulation order of `CharacterBio::UpdateResources`:
/// bio store -> skills -> misc (item sets) -> game balance ->
/// mutators -> equipment. Sources are closures that add their records; the
/// profile builders put mutators in `game_balance` after the paks (same
/// order, same raw add).
#[derive(Default)]
pub struct Contributions<'a> {
    pub skills: Vec<Source<'a>>,
    pub item_sets: Vec<Source<'a>>,
    pub game_balance: Vec<Source<'a>>,
    pub equipment: Vec<Source<'a>>,
}

/// Builds the final accumulator. Only the skill, item-set and equipment
/// passes convert attribute gains to life/energy; game balance and mutators
/// add raw values (only those three Character functions do the
/// delta conversion).
pub fn accumulate(
    bio: &BioStore,
    rates: &AttributeRates,
    c: &Contributions,
) -> CharAttrAccumulator {
    let mut acc = bio.base.clone();
    acc.contribute(rates, |a| c.skills.iter().for_each(|f| f(a)));
    acc.contribute(rates, |a| c.item_sets.iter().for_each(|f| f(a)));
    c.game_balance.iter().for_each(|f| f(&mut acc));
    acc.contribute(rates, |a| c.equipment.iter().for_each(|f| f(a)));
    acc
}

/// `records/game/playerresourcebehavior.dbr` `lifeRegenEquation` /
/// `manaRegenEquation`, evaluated by `CharacterBio::GetLifeRegenValue` /
/// `GetManaRegenValue` with `CharacterBio::ResolveEquationVariable`'s
/// variables: `lifeRegen`/`manaRegen` = total regen attribute,
/// `physique`, `intelligence` = total Physique / Spirit, `elapsedTime` in
/// seconds. `CharacterBio::Update` adds the result every tick.
#[derive(Debug, Clone, Default)]
pub struct PlayerRegen {
    pub life: Option<Expr>,
    pub mana: Option<Expr>,
}

impl PlayerRegen {
    pub fn from_record(rec: &Record) -> Self {
        let p = |k: &str| rec.str(k).and_then(|s| Expr::parse(s).ok());
        Self {
            life: p("lifeRegenEquation"),
            mana: p("manaRegenEquation"),
        }
    }
}

impl PlayerRegen {
    fn eval(e: &Expr, regen_var: &str, regen: f32, physique: f32, intelligence: f32) -> f32 {
        e.eval(&|n| {
            let is = |k: &str| n.eq_ignore_ascii_case(k);
            match () {
                _ if is("physique") => Some(physique),
                _ if is("intelligence") => Some(intelligence),
                _ if is("elapsedtime") => Some(1.0),
                _ if is(regen_var) => Some(regen),
                _ => None,
            }
        })
    }

    /// `CharacterBio::GetLifeRegenValue(regen, mod, 1)`: the
    /// life equation at elapsedTime 1 with `lifeRegen` = `regen` (no equation: 0).
    pub fn life_value(&self, regen: f32, physique: f32, intelligence: f32) -> f32 {
        self.life.as_ref().map_or(0.0, |e| {
            Self::eval(e, "liferegen", regen, physique, intelligence)
        })
    }

    /// `CharacterBio::GetManaRegenValue(regen, mod, 1, intelligence)`:
    /// the mana equation with `manaRegen` = `regen`; a non-zero
    /// `intelligence` replaces the bio's for the call.
    pub fn mana_value(&self, regen: f32, physique: f32, intelligence: f32) -> f32 {
        self.mana.as_ref().map_or(0.0, |e| {
            Self::eval(e, "manaregen", regen, physique, intelligence)
        })
    }
}

impl CharacterSheet {
    /// Replaces the per-second regen with the player equations at
    /// elapsedTime 1 (what the character window shows).
    pub fn apply_player_regen(&mut self, eq: &PlayerRegen) {
        let (phys, spirit) = (self.physique, self.spirit);
        if eq.life.is_some() {
            self.life_regen_per_sec = eq.life_value(self.life_regen, phys, spirit);
        }
        if eq.mana.is_some() {
            self.energy_regen_per_sec = eq.mana_value(self.energy_regen, phys, spirit);
        }
    }

    pub fn compute(acc: &CharAttrAccumulator, level: i32, formulas: &CombatFormulas) -> Self {
        Self {
            level,
            physique: acc.get(CharAttr::Physique),
            cunning: acc.get(CharAttr::Cunning),
            spirit: acc.get(CharAttr::Spirit),
            life: acc.get(CharAttr::Life),
            energy: acc.get(CharAttr::Energy),
            energy_reserve: acc.get(CharAttr::EnergyReserve)
                * (1.0 - acc.get(CharAttr::EnergyReserveReduction) * 0.01),
            life_regen: acc.get(CharAttr::LifeRegen),
            energy_regen: acc.get(CharAttr::EnergyRegen),
            life_regen_per_sec: acc.get(CharAttr::LifeRegen),
            energy_regen_per_sec: acc.get(CharAttr::EnergyRegen),
            offensive_ability: formulas.offensive_ability(acc, level, 0.0, 0.0),
            defensive_ability: formulas.defensive_ability(acc, level, 0.0, 0.0),
            dodge: acc.get(CharAttr::Dodge),
            deflect: acc.get(CharAttr::DeflectProjectile),
            heal_increase: acc.get(CharAttr::HealIncrease),
            energy_absorption: acc.get(CharAttr::EnergyAbsorption),
            block_recovery_reduction: acc.get(CharAttr::BlockRecoveryReduction),
            percent_heal_increase: acc.get(CharAttr::PercentHealIncrease),
            constitution_modifier: acc.modifier[CharAttr::Constitution as usize],
            constitution: acc.get(CharAttr::Constitution),
            experience_bonus: acc.get(CharAttr::IncreasedExperience),
            gold_bonus: acc.get(CharAttr::IncreasedGold),
            light_radius: acc.get(CharAttr::LightRadius),
            base: BioBase::default(),
        }
    }
}

/// The player light's radius: its record's `radius` (`Light::GetRadius`,
/// kept by `Player::Load` in `Player.baseLightRadius`) × (1 + `characterLightRadius` total %
/// / 100). `Player::UpdateSelf`: every update
/// `Light::SetRadius(((GetValue(57) × 0.01) + 1) × base)`; it is the only
/// consumer of attribute 57 besides the sheet. Nothing clamps it (a
/// negative total shrinks the light).
pub fn light_radius(base: f32, total_pct: f32) -> f32 {
    (total_pct * 0.01 + 1.0) * base
}

/// Defensive stats of a record (`defensiveX` resistances etc.) as
/// `(type, value)` ([`expand_resistances`]).
pub fn record_resistances(rec: &Record) -> Vec<(CombatType, f32)> {
    expand_resistances(|f| rec.f32(f).unwrap_or(0.0))
}

/// The `defensiveX` resistances of a field reading `get` as `(type, value)`,
/// expanding the all / elemental groups like the engine (
/// `DefenseAttributeAbs_AllResistance::AddToAccumulator`); zero fields are
/// left out.
pub fn expand_resistances(get: impl Fn(&str) -> f32) -> Vec<(CombatType, f32)> {
    let mut out = Vec::new();
    for d in family(AttrFamily::Defense) {
        let (Some(field), Some(ty)) = (d.value, d.combat_type()) else {
            continue;
        };
        let v = get(field);
        if v == 0.0 {
            continue;
        }
        match ty {
            CombatType::TotalDamageModifier => {
                out.extend(CombatType::ALL_RESISTANCES.iter().map(|t| (*t, v)))
            }
            CombatType::Elemental => out.extend(CombatType::ELEMENTAL.iter().map(|t| (*t, v))),
            t => out.push((t, v)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn get_value_exact() {
        let mut a = CharAttrAccumulator::default();
        a.add_value(CharAttr::Life, 250.0);
        a.add_modifier(CharAttr::Life, 10.0);
        // ((10*0.01+1)*250*100)*0.01
        assert_eq!(
            a.get(CharAttr::Life),
            ((10.0f32 * 0.01 + 1.0) * 250.0 * 100.0) * 0.01
        );
        a.add_multiplier(CharAttr::Life, 0.5);
        assert_eq!(
            a.get(CharAttr::Life),
            ((10.0f32 * 0.01 + 1.0) * 250.0 * 150.0) * 0.01
        );
        a.add_multiplier(CharAttr::Life, -2.0); // multiplier floors at 0
        assert_eq!(a.get(CharAttr::Life), 0.0);
        let mut b = CharAttrAccumulator::default();
        b.add_value(CharAttr::RunSpeed, -20.0);
        b.add_modifier(CharAttr::RunSpeed, 50.0); // negative values move toward 0... by + |v|*mod
        assert_eq!(
            b.get(CharAttr::RunSpeed),
            (((20.0f32 * 50.0) * 0.01) + -20.0) * 100.0 * 0.01
        );
    }

    #[test]
    fn attribute_gains_give_life() {
        let rates = AttributeRates {
            life_per_physique: 2.5,
            life_per_cunning: 1.0,
            life_per_spirit: 1.5,
            energy_per_spirit: 2.0,
        };
        let mut a = CharAttrAccumulator::default();
        a.add_value(CharAttr::Physique, 50.0);
        a.add_value(CharAttr::Life, 250.0);
        a.contribute(&rates, |a| {
            a.add_value(CharAttr::Physique, 10.0);
            a.add_value(CharAttr::Spirit, 4.0);
            a.add_modifier(CharAttr::Physique, 10.0); // 10% of all 60 Physique
        });
        // Physique 50 -> 66: +16 * 2.5 = 40 life; Spirit +4: +6 life, +8 energy.
        assert_eq!(a.value[CharAttr::Life as usize], 296.0);
        assert_eq!(a.value[CharAttr::Energy as usize], 8.0);
    }

    #[test]
    fn oa_da_level1() {
        let f = CombatFormulas::stock();
        let mut a = CharAttrAccumulator::default();
        for (k, v) in [
            (CharAttr::Physique, 50.0),
            (CharAttr::Cunning, 50.0),
            (CharAttr::OffensiveAbility, 65.0),
            (CharAttr::DefensiveAbility, 65.0),
        ] {
            a.add_value(k, v);
        }
        // (65 + 1*12 + (50 + 0)*0.5) * (1 + 0/100) + 53
        assert_eq!(f.offensive_ability(&a, 1, 0.0, 0.0), 155.0);
        assert_eq!(f.defensive_ability(&a, 1, 0.0, 0.0), 155.0);
        assert_eq!(f.offensive_ability(&a, 1, 0.0, 200.0), 0.0);
        a.add_value(CharAttr::OffensiveAbilityModifier, 10.0);
        assert_eq!(f.offensive_ability(&a, 1, 0.0, 0.0), 102.0 * 1.1 + 53.0);
    }
}
