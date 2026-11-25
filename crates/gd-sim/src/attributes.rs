//! Attribute schema: the engine's attribute enums and the database field
//! names that feed them (Game.dll).
//!
//! - [`CharAttr`] = `CharAttributeType`: index into the 58-slot
//!   `CharAttributeAccumulator` (character stats: Physique/Cunning/Spirit,
//!   life, energy, OA/DA, speeds, dodge, requirement reductions, ...).
//!   Numbers come from each `CharAttributeVal_*`/`CharAttributeMod_*`
//!   constructor (`Attribute.type` = type, `CharAttribute.valueTag` = field name).
//! - [`CombatType`] = `CombatAttributeType`: damage/defense/debuff
//!   kinds; names from the debug-name switch `CombatAttribute::GetTypeName`.
//! - [`FIELDS`](crate::catalog::FIELDS): every DB field name the
//!   offense/defense/retaliation/character/skill attribute classes load
//!   (provenance and invariants in [`crate::catalog`]); [`lookup`] finds the
//!   entry of a field, [`class_def`] the entry of a class.
use crate::catalog::FIELDS;
use std::sync::LazyLock;

/// `CharAttributeType` (constructor type ids).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum CharAttr {
    /// `characterStrength` (UI: Physique).
    Physique = 1,
    /// `characterDexterity` (UI: Cunning).
    Cunning = 2,
    /// `characterIntelligence` (UI: Spirit).
    Spirit = 3,
    Life = 4,
    /// `characterMana` (UI: Energy).
    Energy = 5,
    LifeRegen = 6,
    EnergyRegen = 7,
    EnergyReserve = 8,
    EnergyReserveReduction = 9,
    Constitution = 10,
    HealIncrease = 11,
    PercentHealIncrease = 12,
    TotalSpeed = 13,
    RunSpeed = 14,
    AttackSpeed = 15,
    CastSpeed = 16,
    BaseAttackSpeed = 17,
    RunSpeedCap = 18,
    AttackSpeedCap = 19,
    CastSpeedCap = 20,
    OffensiveAbility = 21,
    OffensiveAbilityModifier = 22,
    DefensiveAbility = 23,
    DefensiveAbilityModifier = 24,
    /// `characterDefensiveBlockRecoveryReduction` (% off shield recovery).
    BlockRecoveryReduction = 25,
    /// `characterEnergyAbsorptionPercent`.
    EnergyAbsorption = 26,
    /// `characterDodgePercent`.
    Dodge = 27,
    /// `characterDeflectProjectile`.
    DeflectProjectile = 28,
    WeaponStrengthReqReduction = 29,
    WeaponDexterityReqReduction = 30,
    WeaponIntelligenceReqReduction = 31,
    MeleeStrengthReqReduction = 32,
    MeleeDexterityReqReduction = 33,
    MeleeIntelligenceReqReduction = 34,
    HuntingStrengthReqReduction = 35,
    HuntingDexterityReqReduction = 36,
    HuntingIntelligenceReqReduction = 37,
    StaffStrengthReqReduction = 38,
    StaffDexterityReqReduction = 39,
    StaffIntelligenceReqReduction = 40,
    Weapon2HStrengthReqReduction = 41,
    Weapon2HDexterityReqReduction = 42,
    Weapon2HIntelligenceReqReduction = 43,
    ShieldStrengthReqReduction = 44,
    ShieldDexterityReqReduction = 45,
    ShieldIntelligenceReqReduction = 46,
    ArmorStrengthReqReduction = 47,
    ArmorDexterityReqReduction = 48,
    ArmorIntelligenceReqReduction = 49,
    JewelryStrengthReqReduction = 50,
    JewelryDexterityReqReduction = 51,
    JewelryIntelligenceReqReduction = 52,
    GlobalReqReduction = 53,
    LevelReqReduction = 54,
    IncreasedExperience = 55,
    IncreasedGold = 56,
    LightRadius = 57,
}

/// Number of slots in `CharAttributeAccumulator` (0x2B8 bytes = 3 x 58 f32).
pub const CHAR_ATTR_SLOTS: usize = 58;

impl CharAttr {
    /// Every type in id order: `ALL[id - 1]`. The length is the last id, so a
    /// variant added after it fails to compile until it is listed; the const
    /// check below fails the build on a gap or a misplaced entry.
    pub const ALL: [CharAttr; CharAttr::LightRadius as usize] = [
        Self::Physique,
        Self::Cunning,
        Self::Spirit,
        Self::Life,
        Self::Energy,
        Self::LifeRegen,
        Self::EnergyRegen,
        Self::EnergyReserve,
        Self::EnergyReserveReduction,
        Self::Constitution,
        Self::HealIncrease,
        Self::PercentHealIncrease,
        Self::TotalSpeed,
        Self::RunSpeed,
        Self::AttackSpeed,
        Self::CastSpeed,
        Self::BaseAttackSpeed,
        Self::RunSpeedCap,
        Self::AttackSpeedCap,
        Self::CastSpeedCap,
        Self::OffensiveAbility,
        Self::OffensiveAbilityModifier,
        Self::DefensiveAbility,
        Self::DefensiveAbilityModifier,
        Self::BlockRecoveryReduction,
        Self::EnergyAbsorption,
        Self::Dodge,
        Self::DeflectProjectile,
        Self::WeaponStrengthReqReduction,
        Self::WeaponDexterityReqReduction,
        Self::WeaponIntelligenceReqReduction,
        Self::MeleeStrengthReqReduction,
        Self::MeleeDexterityReqReduction,
        Self::MeleeIntelligenceReqReduction,
        Self::HuntingStrengthReqReduction,
        Self::HuntingDexterityReqReduction,
        Self::HuntingIntelligenceReqReduction,
        Self::StaffStrengthReqReduction,
        Self::StaffDexterityReqReduction,
        Self::StaffIntelligenceReqReduction,
        Self::Weapon2HStrengthReqReduction,
        Self::Weapon2HDexterityReqReduction,
        Self::Weapon2HIntelligenceReqReduction,
        Self::ShieldStrengthReqReduction,
        Self::ShieldDexterityReqReduction,
        Self::ShieldIntelligenceReqReduction,
        Self::ArmorStrengthReqReduction,
        Self::ArmorDexterityReqReduction,
        Self::ArmorIntelligenceReqReduction,
        Self::JewelryStrengthReqReduction,
        Self::JewelryDexterityReqReduction,
        Self::JewelryIntelligenceReqReduction,
        Self::GlobalReqReduction,
        Self::LevelReqReduction,
        Self::IncreasedExperience,
        Self::IncreasedGold,
        Self::LightRadius,
    ];

    pub fn from_id(id: u8) -> Option<CharAttr> {
        Self::ALL.get(usize::from(id).checked_sub(1)?).copied()
    }
}

const _: () = {
    let mut i = 0;
    while i < CharAttr::ALL.len() {
        assert!(
            CharAttr::ALL[i] as usize == i + 1,
            "CharAttr::ALL out of id order"
        );
        i += 1;
    }
};

/// `CombatAttributeType` (`CombatAttribute::GetTypeName` name switch + the
/// `GetType` virtual of every attribute class).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum CombatType {
    /// Weapon base damage before the physical/pierce split.
    PhysicalPierce = 1,
    Physical = 2,
    PierceRatio = 3,
    Pierce = 4,
    Cold = 5,
    Fire = 6,
    /// Acid / poison (DB fields `*Poison*`).
    Acid = 7,
    Lightning = 8,
    /// Vitality (DB fields `*Life*`).
    Vitality = 9,
    Chaos = 10,
    Aether = 11,
    ManaBurn = 12,
    Disruption = 13,
    PercentCurrentLife = 14,
    Bleeding = 15,
    TotalSpeed = 16,
    AttackSpeed = 17,
    CastSpeed = 18,
    RunSpeed = 19,
    LifeLeech = 20,
    ManaLeech = 21,
    OffensiveAbility = 22,
    DefensiveAbility = 23,
    OffensiveReduction = 24,
    /// Armor reduction debuff (subtracted from protection).
    DefensiveReduction = 25,
    Fumble = 26,
    ProjectileFumble = 27,
    TotalDamageReductionPercent = 28,
    TotalDamageReductionAbsolute = 29,
    PhysicalDamageReductionPercent = 30,
    ElementalDamageReductionPercent = 31,
    TotalResistanceReductionPercent = 32,
    TotalResistanceReductionAbsolute = 33,
    PhysicalResistanceReductionPercent = 34,
    PhysicalResistanceReductionAbsolute = 35,
    ElementalResistanceReductionPercent = 36,
    ElementalResistanceReductionAbsolute = 37,
    AbsorptionProtection = 38,
    Absorption = 39,
    Protection = 40,
    ArmorRating = 41,
    Stun = 42,
    Sleep = 43,
    Trap = 44,
    Freeze = 45,
    Petrify = 46,
    Immobilize = 47,
    Knockdown = 48,
    TakeHit = 49,
    Taunt = 50,
    Convert = 51,
    Fear = 52,
    Confusion = 53,
    CrowdControl = 54,
    BlockModifier = 55,
    BlockAmountModifier = 56,
    Reflect = 57,
    /// Fire+Cold+Lightning group.
    Elemental = 58,
    CritDamageModifier = 59,
    /// Offense: `offensiveTotalDamageModifier`; defense: `defensiveAllResistance`.
    TotalDamageModifier = 60,
    /// Offense: `offensiveDamageMult`; defense: `negativeDamageMult` (damage taken).
    DamageMultiplier = 61,
    ManaBurnRatio = 62,
    ReflectionResist = 63,
    AllMaxResist = 64,
    CrowdControlMaxResist = 65,
}

impl CombatType {
    /// Every type in id order: `ALL[id - 1]` (length and order checked as
    /// for [`CharAttr::ALL`]).
    pub const ALL: [CombatType; CombatType::CrowdControlMaxResist as usize] = [
        Self::PhysicalPierce,
        Self::Physical,
        Self::PierceRatio,
        Self::Pierce,
        Self::Cold,
        Self::Fire,
        Self::Acid,
        Self::Lightning,
        Self::Vitality,
        Self::Chaos,
        Self::Aether,
        Self::ManaBurn,
        Self::Disruption,
        Self::PercentCurrentLife,
        Self::Bleeding,
        Self::TotalSpeed,
        Self::AttackSpeed,
        Self::CastSpeed,
        Self::RunSpeed,
        Self::LifeLeech,
        Self::ManaLeech,
        Self::OffensiveAbility,
        Self::DefensiveAbility,
        Self::OffensiveReduction,
        Self::DefensiveReduction,
        Self::Fumble,
        Self::ProjectileFumble,
        Self::TotalDamageReductionPercent,
        Self::TotalDamageReductionAbsolute,
        Self::PhysicalDamageReductionPercent,
        Self::ElementalDamageReductionPercent,
        Self::TotalResistanceReductionPercent,
        Self::TotalResistanceReductionAbsolute,
        Self::PhysicalResistanceReductionPercent,
        Self::PhysicalResistanceReductionAbsolute,
        Self::ElementalResistanceReductionPercent,
        Self::ElementalResistanceReductionAbsolute,
        Self::AbsorptionProtection,
        Self::Absorption,
        Self::Protection,
        Self::ArmorRating,
        Self::Stun,
        Self::Sleep,
        Self::Trap,
        Self::Freeze,
        Self::Petrify,
        Self::Immobilize,
        Self::Knockdown,
        Self::TakeHit,
        Self::Taunt,
        Self::Convert,
        Self::Fear,
        Self::Confusion,
        Self::CrowdControl,
        Self::BlockModifier,
        Self::BlockAmountModifier,
        Self::Reflect,
        Self::Elemental,
        Self::CritDamageModifier,
        Self::TotalDamageModifier,
        Self::DamageMultiplier,
        Self::ManaBurnRatio,
        Self::ReflectionResist,
        Self::AllMaxResist,
        Self::CrowdControlMaxResist,
    ];

    pub fn from_id(id: u8) -> Option<CombatType> {
        Self::ALL.get(usize::from(id).checked_sub(1)?).copied()
    }

    /// The "elemental" test used by resistance reduction:
    /// `((t - 5) & ~3) == 0 && t != 7` -> Cold, Fire, Lightning (
    /// `CombatAttributeAbsDefense::Process`).
    pub fn is_elemental(self) -> bool {
        matches!(
            self,
            CombatType::Cold | CombatType::Fire | CombatType::Lightning
        )
    }

    /// Damage types that use `CombatAttributeAbsDamageElemental` /
    /// `CombatAttributeDurDamageElemental` (scaled by Spirit via the
    /// `magical*Equation`s): each class's AddDamageToAccumulator.
    pub fn scales_with_spirit(self) -> bool {
        matches!(
            self,
            CombatType::Cold
                | CombatType::Fire
                | CombatType::Lightning
                | CombatType::Acid
                | CombatType::Vitality
                | CombatType::Chaos
                | CombatType::Aether
        )
    }

    /// The ten resistances granted by `defensiveAllResistance`
    /// (`DefenseAttributeAbs_AllResistance::AddToAccumulator`, same set
    /// for `defensiveAllMaxResist`).
    pub const ALL_RESISTANCES: [CombatType; 10] = [
        CombatType::Physical,
        CombatType::Pierce,
        CombatType::Fire,
        CombatType::Cold,
        CombatType::Lightning,
        CombatType::Aether,
        CombatType::Chaos,
        CombatType::Vitality,
        CombatType::Acid,
        CombatType::Bleeding,
    ];
    /// Expansion of [`CombatType::Elemental`] (resistance, resistance modifier
    /// and `offensiveElemental*` damage, split 1/3 each).
    pub const ELEMENTAL: [CombatType; 3] =
        [CombatType::Cold, CombatType::Fire, CombatType::Lightning];
}

const _: () = {
    let mut i = 0;
    while i < CombatType::ALL.len() {
        assert!(
            CombatType::ALL[i] as usize == i + 1,
            "CombatType::ALL out of id order"
        );
        i += 1;
    }
};

/// Which attribute class family a DB field belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttrFamily {
    /// `offensiveXMin/Max` flat hit damage (`DamageAttributeAbs_*`).
    Offense,
    /// `offensiveBaseXMin/Max` (`DamageAttributeAbsBase_*`, weapon-like base).
    OffenseBase,
    /// `offensiveXModifier`: +% damage (`DamageAttributeAbsMod_*`).
    OffenseModifier,
    /// `offensiveSlowX{Min,Max,DurationMin,DurationMax}`: DoTs and debuffs.
    OffenseDuration,
    /// `offensiveSlowXModifier` / `...DurationModifier`.
    OffenseDurationModifier,
    /// `offensiveStun/Freeze/...Min/Max` crowd control.
    OffenseCrowdControl,
    /// Weapon base `offensivePhysicalMin/Max` + `offensivePierceRatioMin`.
    WeaponBase,
    Retaliation,
    RetaliationModifier,
    RetaliationDuration,
    RetaliationDurationModifier,
    RetaliationCrowdControl,
    /// `defensiveX`: resistance (%).
    Defense,
    /// `defensiveXModifier`: +% of the resistance.
    DefenseModifier,
    /// `defensiveXDuration`: DoT duration reduction (%).
    DefenseDuration,
    DefenseDurationModifier,
    /// `defensiveXMaxResist`: +cap.
    DefenseMaxResist,
    /// Block chance/amount modifiers, reflect, damage taken multiplier.
    DefenseMisc,
    /// `defensiveProtection` / `defensiveBonusProtection` (armor).
    Armor,
    /// `characterX`: flat character stat.
    CharacterValue,
    /// `characterXModifier`: +% character stat.
    CharacterModifier,
    /// `characterLifeMultModifier`.
    CharacterMultiplier,
    SkillValue,
    SkillModifier,
}

impl AttrFamily {
    /// Number of families (`SkillModifier` is the last variant).
    pub const COUNT: usize = AttrFamily::SkillModifier as usize + 1;
}

/// One attribute class and the DB fields it loads.
#[derive(Debug, Clone, Copy)]
pub struct FieldDef {
    pub family: AttrFamily,
    /// Engine class (`GAME::<class>`).
    pub class: &'static str,
    /// `CombatAttributeType` (offense/defense/retaliation/armor families),
    /// `CharAttributeType` (character families) or skill attribute type.
    pub ty: u8,
    /// Single-value field (defense, character, skill families).
    pub value: Option<&'static str>,
    pub min: Option<&'static str>,
    pub max: Option<&'static str>,
    pub chance: Option<&'static str>,
    pub modifier: Option<&'static str>,
    pub duration_min: Option<&'static str>,
    pub duration_max: Option<&'static str>,
    pub duration_modifier: Option<&'static str>,
    /// `offensiveManaBurnDamageRatio` / `offensivePierceRatioMin`.
    pub ratio: Option<&'static str>,
}

impl FieldDef {
    const fn new(family: AttrFamily, class: &'static str, ty: u8) -> Self {
        Self {
            family,
            class,
            ty,
            value: None,
            min: None,
            max: None,
            chance: None,
            modifier: None,
            duration_min: None,
            duration_max: None,
            duration_modifier: None,
            ratio: None,
        }
    }
    /// A single-value attribute ([`crate::catalog`] table constructors below).
    pub(crate) const fn value(
        family: AttrFamily,
        class: &'static str,
        ty: u8,
        value: &'static str,
    ) -> Self {
        Self {
            value: Some(value),
            ..Self::new(family, class, ty)
        }
    }
    pub(crate) const fn value_chance(
        family: AttrFamily,
        class: &'static str,
        ty: u8,
        value: &'static str,
        chance: &'static str,
    ) -> Self {
        Self {
            chance: Some(chance),
            ..Self::value(family, class, ty, value)
        }
    }
    pub(crate) const fn min_max(
        family: AttrFamily,
        class: &'static str,
        ty: u8,
        min: &'static str,
        max: &'static str,
    ) -> Self {
        Self {
            min: Some(min),
            max: Some(max),
            ..Self::new(family, class, ty)
        }
    }
    pub(crate) const fn range(
        family: AttrFamily,
        class: &'static str,
        ty: u8,
        min: &'static str,
        max: &'static str,
        chance: &'static str,
    ) -> Self {
        Self {
            chance: Some(chance),
            ..Self::min_max(family, class, ty, min, max)
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn duration(
        family: AttrFamily,
        class: &'static str,
        ty: u8,
        min: &'static str,
        max: &'static str,
        chance: &'static str,
        duration_min: &'static str,
        duration_max: &'static str,
    ) -> Self {
        Self {
            duration_min: Some(duration_min),
            duration_max: Some(duration_max),
            ..Self::range(family, class, ty, min, max, chance)
        }
    }
    pub(crate) const fn modifier(
        family: AttrFamily,
        class: &'static str,
        ty: u8,
        modifier: &'static str,
        chance: &'static str,
    ) -> Self {
        Self {
            modifier: Some(modifier),
            chance: Some(chance),
            ..Self::new(family, class, ty)
        }
    }
    pub(crate) const fn duration_modifier(
        family: AttrFamily,
        class: &'static str,
        ty: u8,
        modifier: &'static str,
        duration_modifier: &'static str,
        chance: &'static str,
    ) -> Self {
        Self {
            duration_modifier: Some(duration_modifier),
            ..Self::modifier(family, class, ty, modifier, chance)
        }
    }
    pub(crate) const fn with_ratio(self, ratio: &'static str) -> Self {
        Self {
            ratio: Some(ratio),
            ..self
        }
    }
}

impl FieldDef {
    pub fn combat_type(&self) -> Option<CombatType> {
        CombatType::from_id(self.ty)
    }
    pub fn char_attr(&self) -> Option<CharAttr> {
        CharAttr::from_id(self.ty)
    }
    pub fn fields(&self) -> impl Iterator<Item = &'static str> {
        [
            self.value,
            self.min,
            self.max,
            self.chance,
            self.modifier,
            self.duration_min,
            self.duration_max,
            self.duration_modifier,
            self.ratio,
        ]
        .into_iter()
        .flatten()
    }

    /// Stem of the attribute's global-chance tags: `<stem>Global` puts it in
    /// its store's global group, `<stem>XOR` in the "one of" list of that
    /// group (`GetLoadGlobalTag` / `GetLoadXorTag` of
    /// `DamageAttributeDur_Fire` / `DamageAttributeDurMod_Fire` → both
    /// `offensiveSlowFire…`, `DamageAttributeAbs_Fire` / `AbsMod_Fire` →
    /// `offensiveFire…`, `AbsMod_Elemental` → `offensiveElemental…`).
    pub fn global_stem(&self) -> Option<&'static str> {
        if let Some(m) = self.min {
            return m.strip_suffix("Min");
        }
        self.modifier.and_then(|m| m.strip_suffix("Modifier"))
    }

    /// Position of the class in the damage stores' load order
    /// ([`DAMAGE_LOAD_ORDER`]); unknown classes last.
    pub fn load_index(&self) -> usize {
        // (class, position) sorted by class; the stable sort keeps the first
        // position of a repeated class first.
        static INDEX: LazyLock<Vec<(&'static str, usize)>> = LazyLock::new(|| {
            let mut v: Vec<_> = DAMAGE_LOAD_ORDER
                .iter()
                .enumerate()
                .map(|(i, c)| (*c, i))
                .collect();
            v.sort_by_key(|e| e.0);
            v
        });
        let i = INDEX.partition_point(|e| e.0 < self.class);
        INDEX
            .get(i)
            .filter(|e| e.0 == self.class)
            .map_or(DAMAGE_LOAD_ORDER.len(), |e| e.1)
    }
}

/// The order `DamageAttributeStore_Equipment::Load` /
/// `DamageAttributeStore_Skill::Load` create their attribute
/// classes in (the skill store is the same order without the
/// `…Base` classes in front). Attributes reach a store's lists in this
/// order, which decides the global "one of" pick on the character sheet.
pub const DAMAGE_LOAD_ORDER: [&str; 104] = [
    "DamageAttribute_BasePhysical",
    "DamageAttributeAbsBase_Fire",
    "DamageAttributeAbsBase_Cold",
    "DamageAttributeAbsBase_Lightning",
    "DamageAttributeAbsBase_Poison",
    "DamageAttributeAbsBase_Life",
    "DamageAttributeAbsBase_Aether",
    "DamageAttributeAbsBase_Chaos",
    "DamageAttributeAbs_BonusPhysical",
    "DamageAttributeAbsMod_PierceRatio",
    "DamageAttributeAbs_Pierce",
    "DamageAttributeAbs_Fire",
    "DamageAttributeAbs_Cold",
    "DamageAttributeAbs_Lightning",
    "DamageAttributeAbs_Poison",
    "DamageAttributeAbs_Life",
    "DamageAttributeAbs_Aether",
    "DamageAttributeAbs_Chaos",
    "DamageAttributeAbs_ElementalDamage",
    "DamageAttributeAbs_PercentCurrentLife",
    "DamageAttributeDur_Physical",
    "DamageAttributeDur_Bleeding",
    "DamageAttributeDur_Fire",
    "DamageAttributeDur_Cold",
    "DamageAttributeDur_Lightning",
    "DamageAttributeDur_Poison",
    "DamageAttributeDur_Life",
    "DamageAttributeDur_Aether",
    "DamageAttributeDur_Chaos",
    "DamageAttributeDur_LifeLeach",
    "DamageAttributeDur_ManaLeach",
    "DamageAttributeAbs_ManaBurn",
    "DamageAttributeAbsMod_TotalDamageModifier",
    "DamageAttributeAbsMod_CritDamageModifier",
    "DamageAttributeAbsMod_DamageMultiplier",
    "DamageAttributeAbsMod_Physical",
    "DamageAttributeAbsMod_Pierce",
    "DamageAttributeAbsMod_Fire",
    "DamageAttributeAbsMod_Cold",
    "DamageAttributeAbsMod_Lightning",
    "DamageAttributeAbsMod_Poison",
    "DamageAttributeAbsMod_Life",
    "DamageAttributeAbsMod_Aether",
    "DamageAttributeAbsMod_Chaos",
    "DamageAttributeAbsMod_Elemental",
    "DamageAttributeDurMod_Physical",
    "DamageAttributeAbsMod_Bleeding",
    "DamageAttributeDurMod_Bleeding",
    "DamageAttributeDurMod_Fire",
    "DamageAttributeDurMod_Cold",
    "DamageAttributeDurMod_Lightning",
    "DamageAttributeDurMod_Poison",
    "DamageAttributeDurMod_Life",
    "DamageAttributeDurMod_Aether",
    "DamageAttributeDurMod_Chaos",
    "DamageAttributeDurMod_LifeLeach",
    "DamageAttributeDurMod_ManaLeach",
    "DamageAttributeAbsMod_ManaBurn",
    "DamageAttributeAbs_LifeLeech",
    "DamageAttributeAbs_Disruption",
    "DamageAttributeAbs_Taunt",
    "DamageAttributeReflex_Stun",
    "DamageAttributeReflex_Knockdown",
    "DamageAttributeReflex_Sleep",
    "DamageAttributeReflex_Freeze",
    "DamageAttributeAbs_Convert",
    "DamageAttributeAbs_Fear",
    "DamageAttributeAbs_Confusion",
    "DamageAttributeReflex_Trap",
    "DamageAttributeReflex_Petrify",
    "DamageAttributeAbsMod_Stun",
    "DamageAttributeAbsMod_Knockdown",
    "DamageAttributeAbsMod_Sleep",
    "DamageAttributeAbsMod_Trap",
    "DamageAttributeAbsMod_Freeze",
    "DamageAttributeAbsMod_Petrify",
    "DamageAttributeDur_TotalSpeed",
    "DamageAttributeDur_AttackSpeed",
    "DamageAttributeDur_SpellCastSpeed",
    "DamageAttributeDur_RunSpeed",
    "DamageAttributeDur_OffensiveAbility",
    "DamageAttributeDur_DefensiveAbility",
    "DamageAttributeDur_OffensiveReduction",
    "DamageAttributeDur_DefensiveReduction",
    "DamageAttributeDurMod_AttackSpeed",
    "DamageAttributeDurMod_SpellCastSpeed",
    "DamageAttributeDurMod_RunSpeed",
    "DamageAttributeDurMod_OffensiveAbility",
    "DamageAttributeDurMod_DefensiveAbility",
    "DamageAttributeDurMod_OffensiveReduction",
    "DamageAttributeDur_PhysicalReductionPercent",
    "DamageAttributeDur_ElementalReductionPercent",
    "DamageAttributeDur_TotalDamageReductionPercent",
    "DamageAttributeDur_TotalDamageReductionAbsolute",
    "DamageAttributeDur_TotalResistanceReductionPercent",
    "DamageAttributeDur_TotalResistanceReductionAbsolute",
    "DamageAttributeDur_PhysicalResistanceReductionPercent",
    "DamageAttributeDur_PhysicalResistanceReductionAbsolute",
    "DamageAttributeDur_ElementalResistanceReductionPercent",
    "DamageAttributeDur_ElementalResistanceReductionAbsolute",
    "DamageAttributeDur_Fumble",
    "DamageAttributeDur_ProjectileFumble",
    "DamageAttributeDur_DamageMultiplier",
    "DamageAttributeDurMod_DamageMultiplier",
];

/// All attribute definitions in a family: its slice of [`FIELDS`]
/// ([`crate::catalog::FAMILY_RANGES`]).
pub fn family(f: AttrFamily) -> std::slice::Iter<'static, FieldDef> {
    let (start, end) = crate::catalog::FAMILY_RANGES[f as usize];
    FIELDS[start..end].iter()
}

/// Finds the definition (and its family) that loads `field`: the first
/// [`FIELDS`] entry, in table order, with a field equal to `field` ignoring
/// ASCII case.
pub fn lookup(field: &str) -> Option<&'static FieldDef> {
    // Every (field, entry) pair keyed by the field's case-folded hash, sorted
    // by it: a lookup compares integers, then only the few equal-hash names.
    // The sort is stable, so equal fields keep table order and the first match
    // is the first entry in table order. (Profile builds call this for every
    // field of every record they add.)
    static INDEX: LazyLock<Vec<(u64, &'static str, &'static FieldDef)>> = LazyLock::new(|| {
        let mut v: Vec<_> = FIELDS
            .iter()
            .flat_map(|d| d.fields().map(move |f| (folded_hash(f), f, d)))
            .collect();
        v.sort_by_key(|e| e.0);
        v
    });
    let h = folded_hash(field);
    let i = INDEX.partition_point(|e| e.0 < h);
    INDEX[i..]
        .iter()
        .take_while(|e| e.0 == h)
        .find(|e| e.1.eq_ignore_ascii_case(field))
        .map(|e| e.2)
}

/// The definition of the engine class `class` (exact name): its first
/// [`FIELDS`] entry in table order. Only
/// `DefenseAttribute_BaseProtectionAbsorption` has two entries; this returns
/// its `defensiveBonusProtection` one (see [`crate::catalog`]).
pub fn class_def(class: &str) -> Option<&'static FieldDef> {
    // Entries sorted by class; the stable sort keeps table order within a class.
    static INDEX: LazyLock<Vec<&'static FieldDef>> = LazyLock::new(|| {
        let mut v: Vec<_> = FIELDS.iter().collect();
        v.sort_by_key(|d| d.class);
        v
    });
    let i = INDEX.partition_point(|d| d.class < class);
    INDEX.get(i).copied().filter(|d| d.class == class)
}

/// FNV-1a of `s` ASCII-lowercased: equal for names equal ignoring ASCII case.
/// An internal index key, no engine parity claim; FNV keeps the case fold
/// allocation-free on the lookup path.
fn folded_hash(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ b.to_ascii_lowercase() as u64).wrapping_mul(0x0000_0100_0000_01b3)
    })
}
