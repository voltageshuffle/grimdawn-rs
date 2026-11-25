//! The order in which `ItemEquipment::InitializeItem` creates an item's
//! attributes, store by store. Field names / class names only.
//!
//! `ItemEquipment::InitializeItem`: it loads the
//! stores in [`StoreKind`] order (character, skill, damage, retaliation,
//! defense, conversion), and each store's `Load` (e.g.
//! `DamageAttributeStore_Equipment::Load`) calls its class
//! creators (`Create_*`) in a fixed order. Each entry is one such creator
//! call; `jitter` says whether the store passed the 20% base jitter
//! (`0x41A00000`) to it.
//!
//! To extend or re-check the table, list the creator calls of each store
//! `Load` reached from `InitializeItem` and the float argument each one gets
//! (`docs/reverse-engineering.md`).
use super::resolve::{Creator, StoreKind};

pub static EQUIPMENT_ORDER: &[(StoreKind, Creator)] = &[
    (
        StoreKind::Char,
        Creator::WeaponOnly(&Creator::BaseAttackSpeed),
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterStrength",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterDexterity",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterIntelligence",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterLife",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterMana",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterStrengthModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterDexterityModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterIntelligenceModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterLifeModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterManaModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterLifeMultModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterOffensiveAbility",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterDefensiveAbility",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterOffensiveAbilityModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterDefensiveAbilityModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterLifeRegen",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterManaRegen",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterLifeRegenModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterManaRegenModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterConstitution",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterConstitutionModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterHealIncreasePercent",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterTotalSpeedModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterAttackSpeed",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterAttackSpeedModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterAttackSpeedMaxModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterSpellCastSpeed",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterSpellCastSpeedModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterSpellCastSpeedMaxModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterRunSpeed",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterRunSpeedModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterRunSpeedMaxModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterDefensiveBlockRecoveryReduction",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterEnergyAbsorptionPercent",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterDodgePercent",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterDeflectProjectile",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterIncreasedExperience",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterIncreasedGold",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterLightRadius",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterGlobalReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterWeaponStrengthReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterWeaponDexterityReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterWeaponIntelligenceReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterMeleeStrengthReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterMeleeDexterityReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterMeleeIntelligenceReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterHuntingStrengthReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterHuntingDexterityReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterHuntingIntelligenceReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterStaffStrengthReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterStaffDexterityReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterStaffIntelligenceReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterWeapon2HStrengthReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterWeapon2HDexterityReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterWeapon2HIntelligenceReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterShieldStrengthReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterShieldDexterityReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterShieldIntelligenceReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterArmorStrengthReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterArmorDexterityReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterArmorIntelligenceReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterJewelryStrengthReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterJewelryDexterityReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterJewelryIntelligenceReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterLevelReqReduction",
            jitter: false,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterManaLimitReserve",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterManaLimitReserveModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterManaLimitReserveReduction",
            jitter: true,
        },
    ),
    (
        StoreKind::Char,
        Creator::Char {
            field: "characterManaLimitReserveReductionModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Skill,
        Creator::Skill {
            field: "skillCooldownReduction",
        },
    ),
    (
        StoreKind::Skill,
        Creator::Skill {
            field: "skillManaCostReduction",
        },
    ),
    (
        StoreKind::Skill,
        Creator::Skill {
            field: "skillComboChargeSpendReduction",
        },
    ),
    (
        StoreKind::Skill,
        Creator::Skill {
            field: "skillProjectileSpeedModifier",
        },
    ),
    (
        StoreKind::Skill,
        Creator::Skill {
            field: "skillCooldownReductionModifier",
        },
    ),
    (
        StoreKind::Skill,
        Creator::Skill {
            field: "skillManaCostReductionModifier",
        },
    ),
    (
        StoreKind::Damage,
        Creator::NonWeaponOnly(&Creator::Class {
            class: "DamageAttribute_Physical",
            jitter: true,
        }),
    ),
    (
        StoreKind::Damage,
        Creator::WeaponOnly(&Creator::BasePhysical),
    ),
    (
        StoreKind::Damage,
        Creator::WeaponOnly(&Creator::Class {
            class: "DamageAttributeAbsBase_Fire",
            jitter: false,
        }),
    ),
    (
        StoreKind::Damage,
        Creator::WeaponOnly(&Creator::Class {
            class: "DamageAttributeAbsBase_Cold",
            jitter: false,
        }),
    ),
    (
        StoreKind::Damage,
        Creator::WeaponOnly(&Creator::Class {
            class: "DamageAttributeAbsBase_Lightning",
            jitter: false,
        }),
    ),
    (
        StoreKind::Damage,
        Creator::WeaponOnly(&Creator::Class {
            class: "DamageAttributeAbsBase_Poison",
            jitter: false,
        }),
    ),
    (
        StoreKind::Damage,
        Creator::WeaponOnly(&Creator::Class {
            class: "DamageAttributeAbsBase_Life",
            jitter: false,
        }),
    ),
    (
        StoreKind::Damage,
        Creator::WeaponOnly(&Creator::Class {
            class: "DamageAttributeAbsBase_Aether",
            jitter: false,
        }),
    ),
    (
        StoreKind::Damage,
        Creator::WeaponOnly(&Creator::Class {
            class: "DamageAttributeAbsBase_Chaos",
            jitter: false,
        }),
    ),
    (
        StoreKind::Damage,
        Creator::WeaponOnly(&Creator::Class {
            class: "DamageAttributeAbs_BonusPhysical",
            jitter: true,
        }),
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbsMod_PierceRatio",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbs_Pierce",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbs_Fire",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbs_Cold",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbs_Lightning",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbs_Poison",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbs_Life",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbs_Aether",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbs_Chaos",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbs_ElementalDamage",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbs_PercentCurrentLife",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_Physical",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_Bleeding",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_Fire",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_Cold",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_Lightning",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_Poison",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_Life",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_Aether",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_Chaos",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_LifeLeach",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_ManaLeach",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbs_ManaBurn",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbsMod_TotalDamageModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbsMod_CritDamageModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbsMod_DamageMultiplier",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbsMod_Physical",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbsMod_Pierce",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbsMod_Fire",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbsMod_Cold",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbsMod_Lightning",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbsMod_Poison",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbsMod_Life",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbsMod_Aether",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbsMod_Chaos",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbsMod_Elemental",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDurMod_Physical",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbsMod_Bleeding",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDurMod_Bleeding",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDurMod_Fire",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDurMod_Cold",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDurMod_Lightning",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDurMod_Poison",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDurMod_Life",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDurMod_Aether",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDurMod_Chaos",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDurMod_LifeLeach",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDurMod_ManaLeach",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbsMod_ManaBurn",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbs_LifeLeech",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbs_Disruption",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeReflex_Stun",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeReflex_Knockdown",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeReflex_Sleep",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeReflex_Freeze",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbs_Convert",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbs_Fear",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbs_Confusion",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeReflex_Petrify",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbsMod_Stun",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbsMod_Knockdown",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbsMod_Sleep",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbsMod_Freeze",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeAbsMod_Petrify",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_TotalSpeed",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_AttackSpeed",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_SpellCastSpeed",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_RunSpeed",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_OffensiveAbility",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_DefensiveAbility",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_OffensiveReduction",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_DefensiveReduction",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDurMod_AttackSpeed",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDurMod_SpellCastSpeed",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDurMod_RunSpeed",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDurMod_OffensiveAbility",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDurMod_DefensiveAbility",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDurMod_OffensiveReduction",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_PhysicalReductionPercent",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_ElementalReductionPercent",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_TotalDamageReductionPercent",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_TotalDamageReductionAbsolute",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_TotalResistanceReductionPercent",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_TotalResistanceReductionAbsolute",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_PhysicalResistanceReductionPercent",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_PhysicalResistanceReductionAbsolute",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_ElementalResistanceReductionPercent",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_ElementalResistanceReductionAbsolute",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_Fumble",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_ProjectileFumble",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDur_DamageMultiplier",
            jitter: true,
        },
    ),
    (
        StoreKind::Damage,
        Creator::Class {
            class: "DamageAttributeDurMod_DamageMultiplier",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbs_Physical",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbsMod_PierceRatio",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbs_Pierce",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbs_Fire",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbs_Cold",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbs_Lightning",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbs_Poison",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbs_Life",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbs_Aether",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbs_Chaos",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbs_ElementalDamage",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbs_PercentCurrentLife",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDur_Physical",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDur_Bleeding",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDur_Fire",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDur_Cold",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDur_Lightning",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDur_Poison",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDur_Life",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDur_Aether",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDur_Chaos",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDur_LifeLeach",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDur_ManaLeach",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbsMod_TotalDamageModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbsMod_DamageMultiplier",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbsMod_Physical",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbsMod_Pierce",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbsMod_Fire",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbsMod_Cold",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbsMod_Lightning",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbsMod_Poison",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbsMod_Life",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbsMod_Aether",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbsMod_Chaos",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbsMod_Elemental",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDurMod_Physical",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDurMod_Bleeding",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDurMod_Fire",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDurMod_Cold",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDurMod_Lightning",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDurMod_Poison",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDurMod_Life",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDurMod_Aether",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDurMod_Chaos",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDurMod_LifeLeach",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDurMod_ManaLeach",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDur_AttackSpeed",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDurMod_AttackSpeed",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDur_SpellCastSpeed",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDurMod_SpellCastSpeed",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDur_RunSpeed",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDurMod_RunSpeed",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDur_OffensiveAbility",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDurMod_OffensiveAbility",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDur_DefensiveAbility",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDurMod_DefensiveAbility",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDur_OffensiveReduction",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeDurMod_OffensiveReduction",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeReflex_Stun",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbsMod_Stun",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeReflex_Sleep",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbsMod_Sleep",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeReflex_Freeze",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbs_Convert",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbs_Fear",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbs_Confusion",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeReflex_Trap",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeReflex_Petrify",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeReflex_Knockdown",
            jitter: true,
        },
    ),
    (
        StoreKind::Retaliation,
        Creator::Class {
            class: "RetaliationAttributeAbsMod_Knockdown",
            jitter: true,
        },
    ),
    (StoreKind::Defense, Creator::Armor),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeMisc_BlockModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeMisc_BlockAmountModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbsMod_Protection",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbsMod_Absorption",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_Physical",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_Pierce",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_Fire",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_Cold",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_Lightning",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_Poison",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_Life",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_Aether",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_Chaos",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_ElementalResistance",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_Bleeding",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_LifeLeach",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_ManaLeach",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_ManaBurn",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_AllResistance",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbsMod_Physical",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbsMod_Pierce",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbsMod_Fire",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbsMod_Cold",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbsMod_Lightning",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbsMod_Poison",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbsMod_Life",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbsMod_Aether",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbsMod_Chaos",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbsMod_Elemental",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbsMod_Bleeding",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbsMod_LifeLeach",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbsMod_ManaLeach",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDur_Physical",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDur_Fire",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDur_Cold",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDur_Lightning",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDur_Poison",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDur_Life",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDur_Aether",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDur_Chaos",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDur_Bleeding",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDur_LifeLeach",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDur_ManaLeach",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDurMod_Physical",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDurMod_Fire",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDurMod_Cold",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDurMod_Lightning",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDurMod_Poison",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDurMod_Life",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDurMod_Aether",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDurMod_Chaos",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDurMod_Bleeding",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDurMod_LifeLeach",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDurMod_ManaLeach",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_Disruption",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_Stun",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbsMod_Stun",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_Freeze",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_Trap",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_Petrify",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_Sleep",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbsMod_Sleep",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_Knockdown",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbsMod_Knockdown",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_Taunt",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_Fear",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_Confusion",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_Convert",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_TotalSpeed",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_CrowdControl",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDefenseCap_Physical",
            jitter: false,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDefenseCap_Pierce",
            jitter: false,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDefenseCap_Fire",
            jitter: false,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDefenseCap_Cold",
            jitter: false,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDefenseCap_Lightning",
            jitter: false,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDefenseCap_Poison",
            jitter: false,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDefenseCap_Life",
            jitter: false,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDefenseCap_Aether",
            jitter: false,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDefenseCap_Chaos",
            jitter: false,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDefenseCap_Bleeding",
            jitter: false,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDefenseCap_LifeLeach",
            jitter: false,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDefenseCap_ManaLeach",
            jitter: false,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDefenseCap_Disruption",
            jitter: false,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDefenseCap_Trap",
            jitter: false,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDefenseCap_Petrify",
            jitter: false,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDefenseCap_Freeze",
            jitter: false,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDefenseCap_Stun",
            jitter: false,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDefenseCap_Sleep",
            jitter: false,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDefenseCap_All",
            jitter: false,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeDefenseCap_CrowdControl",
            jitter: false,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeMisc_Reflect",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeMisc_ReflectModifier",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_PercentCurrentLife",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeAbs_PercentReflectionResistance",
            jitter: true,
        },
    ),
    (
        StoreKind::Defense,
        Creator::Class {
            class: "DefenseAttributeMisc_NegativeDamageMultiplier",
            jitter: false,
        },
    ),
    (StoreKind::Conversion, Creator::Conversion),
];
