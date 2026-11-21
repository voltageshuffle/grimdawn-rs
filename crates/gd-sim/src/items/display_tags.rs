//! Attribute class → tooltip format tag. Game.dll: the string each
//! `<Class>::GetDisplayTag` returns (176 overrides, e.g.
//! `DamageAttributeReflex_Stun` → "DamageStun", `RetaliationAttributeReflex_Stun`
//! → "RetaliationStun") or, for the classes without one, the tag its
//! constructor stores (`CharAttributeMod_AttackSpeed::CharAttributeMod_AttackSpeed`
//! → "tagCharAttackSpeedModifier"). Generated; sorted by class name (byte
//! order, `tooltip::class_tag` binary-searches it); the tag texts themselves
//! come from the game's text files.
pub const DISPLAY_TAGS: &[(&str, &str)] = &[
    ("CharAttributeMod_AttackSpeed", "tagCharAttackSpeedModifier"),
    (
        "CharAttributeMod_AttackSpeedCap",
        "tagCharAttackSpeedMaxModifier",
    ),
    (
        "CharAttributeMod_Constitution",
        "tagCharConstitutionModifier",
    ),
    ("CharAttributeMod_Dexterity", "tagCharAttribute01Modifier"),
    (
        "CharAttributeMod_HealIncrease",
        "tagCharHealIncreaseModifier",
    ),
    (
        "CharAttributeMod_Intelligence",
        "tagCharAttribute03Modifier",
    ),
    ("CharAttributeMod_Life", "tagCharAttribute04Modifier"),
    (
        "CharAttributeMod_LifeMultiplier",
        "tagCharAttribute04MultModifier",
    ),
    ("CharAttributeMod_LifeRegen", "tagCharLifeRegenModifier"),
    ("CharAttributeMod_Mana", "tagCharAttribute05Modifier"),
    (
        "CharAttributeMod_ManaLimitReserve",
        "tagCharManaLimitReserveModifier",
    ),
    (
        "CharAttributeMod_ManaLimitReserveReduction",
        "tagCharManaLimitReserveReductionModifier",
    ),
    ("CharAttributeMod_ManaRegen", "tagCharManaRegenModifier"),
    (
        "CharAttributeMod_PercentHealIncrease",
        "tagCharPercentHealIncreaseModifier",
    ),
    ("CharAttributeMod_RunSpeed", "tagCharRunSpeedModifier"),
    ("CharAttributeMod_RunSpeedCap", "tagCharRunSpeedMaxModifier"),
    (
        "CharAttributeMod_SpellCastSpeed",
        "tagCharSpellCastSpeedModifier",
    ),
    (
        "CharAttributeMod_SpellCastSpeedCap",
        "tagCharSpellCastSpeedMaxModifier",
    ),
    ("CharAttributeMod_Strength", "tagCharAttribute02Modifier"),
    ("CharAttributeMod_TotalSpeed", "tagCharTotalSpeedModifier"),
    (
        "CharAttributeVal_ArmorDexterityReqReduction",
        "tagCharArmorDexterityReqReduction",
    ),
    (
        "CharAttributeVal_ArmorIntelligenceReqReduction",
        "tagCharArmorIntelligenceReqReduction",
    ),
    (
        "CharAttributeVal_ArmorStrengthReqReduction",
        "tagCharArmorStrengthReqReduction",
    ),
    ("CharAttributeVal_AttackSpeed", "tagCharAttackSpeed"),
    ("CharAttributeVal_BaseAttackSpeed", "tagAttackSpeed"),
    ("CharAttributeVal_Constitution", "tagCharConstitution"),
    (
        "CharAttributeVal_DefensiveAbility",
        "tagCharDefensiveAbility",
    ),
    (
        "CharAttributeVal_DefensiveAbilityModifier",
        "tagCharDefensiveAbilityModifier",
    ),
    (
        "CharAttributeVal_DefensiveBlockRecoveryReduction",
        "tagCharDefensiveBlockRecoveryReduction",
    ),
    (
        "CharAttributeVal_DeflectProjectile",
        "tagCharDeflectProjectiles",
    ),
    ("CharAttributeVal_Dexterity", "tagCharAttribute01"),
    ("CharAttributeVal_DodgePercent", "tagCharDodgePercent"),
    (
        "CharAttributeVal_EnergyAbsorptionPercent",
        "tagCharEnergyAbsorptionPercent",
    ),
    (
        "CharAttributeVal_GlobalReqReduction",
        "tagCharItemGlobalReduction",
    ),
    (
        "CharAttributeVal_HuntingDexterityReqReduction",
        "tagCharHuntingDexterityReqReduction",
    ),
    (
        "CharAttributeVal_HuntingIntelligenceReqReduction",
        "tagCharHuntingIntelligenceReqReduction",
    ),
    (
        "CharAttributeVal_HuntingStrengthReqReduction",
        "tagCharHuntingStrengthReqReduction",
    ),
    (
        "CharAttributeVal_IncreasedExperience",
        "tagCharIncreasedExperience",
    ),
    ("CharAttributeVal_IncreasedGold", "tagCharIncreasedGold"),
    ("CharAttributeVal_Intelligence", "tagCharAttribute03"),
    (
        "CharAttributeVal_JewelryDexterityReqReduction",
        "tagCharJewelryDexterityReqReduction",
    ),
    (
        "CharAttributeVal_JewelryIntelligenceReqReduction",
        "tagCharJewelryIntelligenceReqReduction",
    ),
    (
        "CharAttributeVal_JewelryStrengthReqReduction",
        "tagCharJewelryStrengthReqReduction",
    ),
    (
        "CharAttributeVal_LevelReqReduction",
        "tagCharLevelReqReduction",
    ),
    ("CharAttributeVal_Life", "tagCharAttribute04"),
    ("CharAttributeVal_LifeRegen", "tagCharLifeRegen"),
    ("CharAttributeVal_LightRadius", "tagCharLightRadius"),
    ("CharAttributeVal_Mana", "tagCharAttribute05"),
    (
        "CharAttributeVal_ManaLimitReserve",
        "tagCharManaLimitReserve",
    ),
    (
        "CharAttributeVal_ManaLimitReserveReduction",
        "tagCharManaLimitReserveReduction",
    ),
    ("CharAttributeVal_ManaRegen", "tagCharManaRegen"),
    (
        "CharAttributeVal_MeleeDexterityReqReduction",
        "tagCharMeleeDexterityReqReduction",
    ),
    (
        "CharAttributeVal_MeleeIntelligenceReqReduction",
        "tagCharMeleeIntelligenceReqReduction",
    ),
    (
        "CharAttributeVal_MeleeStrengthReqReduction",
        "tagCharMeleeStrengthReqReduction",
    ),
    (
        "CharAttributeVal_OffensiveAbility",
        "tagCharOffensiveAbility",
    ),
    (
        "CharAttributeVal_OffensiveAbilityModifier",
        "tagCharOffensiveAbilityModifier",
    ),
    ("CharAttributeVal_RunSpeed", "tagCharRunSpeed"),
    (
        "CharAttributeVal_ShieldDexterityReqReduction",
        "tagCharShieldDexterityReqReduction",
    ),
    (
        "CharAttributeVal_ShieldIntelligenceReqReduction",
        "tagCharShieldIntelligenceReqReduction",
    ),
    (
        "CharAttributeVal_ShieldStrengthReqReduction",
        "tagCharShieldStrengthReqReduction",
    ),
    ("CharAttributeVal_SpellCastSpeed", "tagCharSpellCastSpeed"),
    (
        "CharAttributeVal_StaffDexterityReqReduction",
        "tagCharStaffDexterityReqReduction",
    ),
    (
        "CharAttributeVal_StaffIntelligenceReqReduction",
        "tagCharStaffIntelligenceReqReduction",
    ),
    (
        "CharAttributeVal_StaffStrengthReqReduction",
        "tagCharStaffStrengthReqReduction",
    ),
    ("CharAttributeVal_Strength", "tagCharAttribute02"),
    (
        "CharAttributeVal_Weapon2HDexterityReqReduction",
        "tagCharWeapon2HDexterityReqReduction",
    ),
    (
        "CharAttributeVal_Weapon2HIntelligenceReqReduction",
        "tagCharWeapon2HIntelligenceReqReduction",
    ),
    (
        "CharAttributeVal_Weapon2HStrengthReqReduction",
        "tagCharWeapon2HStrengthReqReduction",
    ),
    (
        "CharAttributeVal_WeaponDexterityReqReduction",
        "tagCharWeaponDexterityReqReduction",
    ),
    (
        "CharAttributeVal_WeaponIntelligenceReqReduction",
        "tagCharWeaponIntelligenceReqReduction",
    ),
    (
        "CharAttributeVal_WeaponStrengthReqReduction",
        "tagCharWeaponStrengthReqReduction",
    ),
    ("DamageAttribute", "DamageRangeFormat"),
    ("DamageAttributeAbs", "DamageRangeFormat"),
    ("DamageAttributeAbsBase_Aether", "tagDamageBaseAether"),
    ("DamageAttributeAbsBase_Chaos", "tagDamageBaseChaos"),
    ("DamageAttributeAbsBase_Cold", "tagDamageBaseCold"),
    ("DamageAttributeAbsBase_Fire", "tagDamageBaseFire"),
    ("DamageAttributeAbsBase_Life", "tagDamageBaseVitality"),
    ("DamageAttributeAbsBase_Lightning", "tagDamageBaseLightning"),
    ("DamageAttributeAbsBase_Poison", "tagDamageBasePoison"),
    ("DamageAttributeAbsMod", "DamageRangeFormat"),
    ("DamageAttributeAbsMod_Aether", "DamageModifierAether"),
    ("DamageAttributeAbsMod_Bleeding", "DamageModifierBleeding"),
    ("DamageAttributeAbsMod_Chaos", "DamageModifierChaos"),
    ("DamageAttributeAbsMod_Cold", "DamageModifierCold"),
    (
        "DamageAttributeAbsMod_CritDamageModifier",
        "tagDamageModifierCritDamage",
    ),
    (
        "DamageAttributeAbsMod_DamageMultiplier",
        "tagDamageModifierDamageMult",
    ),
    ("DamageAttributeAbsMod_Elemental", "DamageModifierElemental"),
    ("DamageAttributeAbsMod_Fire", "DamageModifierFire"),
    ("DamageAttributeAbsMod_Freeze", "DamageModifierFreeze"),
    ("DamageAttributeAbsMod_Knockdown", "DamageModifierKnockdown"),
    ("DamageAttributeAbsMod_Life", "DamageModifierLife"),
    ("DamageAttributeAbsMod_Lightning", "DamageModifierLightning"),
    ("DamageAttributeAbsMod_ManaBurn", "DamageModifierManaBurn"),
    ("DamageAttributeAbsMod_Petrify", "DamageModifierPetrify"),
    ("DamageAttributeAbsMod_Physical", "DamageModifierPhysical"),
    ("DamageAttributeAbsMod_Pierce", "DamageModifierPierce"),
    (
        "DamageAttributeAbsMod_PierceRatio",
        "DamageModifierPierceRatio",
    ),
    ("DamageAttributeAbsMod_Poison", "DamageModifierPoison"),
    ("DamageAttributeAbsMod_Sleep", "tagDamageModifierSleep"),
    ("DamageAttributeAbsMod_Stun", "DamageModifierStun"),
    (
        "DamageAttributeAbsMod_TotalDamageModifier",
        "tagDamageModifierTotalDamage",
    ),
    ("DamageAttributeAbsMod_Trap", "DamageModifierTrap"),
    ("DamageAttributeAbs_Aether", "DamageAether"),
    ("DamageAttributeAbs_BonusPhysical", "DamageBonusPhysical"),
    ("DamageAttributeAbs_Chaos", "DamageChaos"),
    ("DamageAttributeAbs_Cold", "DamageCold"),
    ("DamageAttributeAbs_Confusion", "DamageConfusion"),
    ("DamageAttributeAbs_Convert", "DamageConvert"),
    ("DamageAttributeAbs_Disruption", "DamageDisruption"),
    ("DamageAttributeAbs_ElementalDamage", "DamageElemental"),
    ("DamageAttributeAbs_Fear", "DamageFear"),
    ("DamageAttributeAbs_Fire", "DamageFire"),
    ("DamageAttributeAbs_Life", "DamageLife"),
    ("DamageAttributeAbs_LifeLeech", "DamageLifeLeech"),
    ("DamageAttributeAbs_Lightning", "DamageLightning"),
    ("DamageAttributeAbs_ManaBurn", "DamageManaDrain"),
    (
        "DamageAttributeAbs_PercentCurrentLife",
        "DamagePercentCurrentLife",
    ),
    ("DamageAttributeAbs_Pierce", "DamagePierce"),
    ("DamageAttributeAbs_Poison", "DamagePoison"),
    ("DamageAttributeAbs_Taunt", "DamageTaunt"),
    ("DamageAttributeDur", "DamageRangeFormat"),
    ("DamageAttributeDurMod", "DamageRangeFormat"),
    (
        "DamageAttributeDurMod_Aether",
        "DamageDurationModifierAether",
    ),
    (
        "DamageAttributeDurMod_AttackSpeed",
        "DamageDurationModifierAttackSpeed",
    ),
    (
        "DamageAttributeDurMod_Bleeding",
        "DamageDurationModifierBleeding",
    ),
    ("DamageAttributeDurMod_Chaos", "DamageDurationModifierChaos"),
    ("DamageAttributeDurMod_Cold", "DamageDurationModifierCold"),
    (
        "DamageAttributeDurMod_DamageMultiplier",
        "DamageDurationModifierDamageMult",
    ),
    (
        "DamageAttributeDurMod_DefensiveAbility",
        "DamageDurationModifierDefensiveAbility",
    ),
    ("DamageAttributeDurMod_Fire", "DamageDurationModifierFire"),
    ("DamageAttributeDurMod_Life", "DamageDurationModifierLife"),
    (
        "DamageAttributeDurMod_LifeLeach",
        "DamageDurationModifierLifeLeach",
    ),
    (
        "DamageAttributeDurMod_Lightning",
        "DamageDurationModifierLightning",
    ),
    (
        "DamageAttributeDurMod_ManaLeach",
        "DamageDurationModifierManaLeach",
    ),
    (
        "DamageAttributeDurMod_OffensiveAbility",
        "DamageDurationModifierOffensiveAbility",
    ),
    (
        "DamageAttributeDurMod_OffensiveReduction",
        "DamageDurationModifierOffensiveReduction",
    ),
    (
        "DamageAttributeDurMod_Physical",
        "DamageDurationModifierPhysical",
    ),
    (
        "DamageAttributeDurMod_Poison",
        "DamageDurationModifierPoison",
    ),
    (
        "DamageAttributeDurMod_RunSpeed",
        "DamageDurationModifierRunSpeed",
    ),
    (
        "DamageAttributeDurMod_SpellCastSpeed",
        "DamageDurationModifierSpellCastSpeed",
    ),
    ("DamageAttributeDur_Aether", "DamageDurationAether"),
    (
        "DamageAttributeDur_AttackSpeed",
        "DamageDurationAttackSpeed",
    ),
    ("DamageAttributeDur_Bleeding", "DamageDurationBleeding"),
    ("DamageAttributeDur_Chaos", "DamageDurationChaos"),
    ("DamageAttributeDur_Cold", "DamageDurationCold"),
    (
        "DamageAttributeDur_DamageMultiplier",
        "DamageDurationDamageMult",
    ),
    (
        "DamageAttributeDur_DefensiveAbility",
        "DamageDurationDefensiveAbility",
    ),
    (
        "DamageAttributeDur_DefensiveReduction",
        "DamageDurationDefensiveReduction",
    ),
    (
        "DamageAttributeDur_ElementalReductionPercent",
        "DamageElementalReductionPercent",
    ),
    (
        "DamageAttributeDur_ElementalResistanceReductionAbsolute",
        "DamageElementalResistanceReductionAbsolute",
    ),
    (
        "DamageAttributeDur_ElementalResistanceReductionPercent",
        "DamageElementalResistanceReductionPercent",
    ),
    ("DamageAttributeDur_Fire", "DamageDurationFire"),
    ("DamageAttributeDur_Fumble", "DamageDurationFumble"),
    ("DamageAttributeDur_Life", "DamageDurationLife"),
    ("DamageAttributeDur_LifeLeach", "DamageDurationLifeLeach"),
    ("DamageAttributeDur_Lightning", "DamageDurationLightning"),
    ("DamageAttributeDur_ManaLeach", "DamageDurationManaLeach"),
    (
        "DamageAttributeDur_OffensiveAbility",
        "DamageDurationOffensiveAbility",
    ),
    (
        "DamageAttributeDur_OffensiveReduction",
        "DamageDurationOffensiveReduction",
    ),
    ("DamageAttributeDur_Physical", "DamageDurationPhysical"),
    (
        "DamageAttributeDur_PhysicalReductionPercent",
        "DamagePhysicalReductionPercent",
    ),
    (
        "DamageAttributeDur_PhysicalResistanceReductionAbsolute",
        "DamagePhysicalResistanceReductionAbsolute",
    ),
    (
        "DamageAttributeDur_PhysicalResistanceReductionPercent",
        "DamagePhysicalResistanceReductionPercent",
    ),
    ("DamageAttributeDur_Poison", "DamageDurationPoison"),
    (
        "DamageAttributeDur_ProjectileFumble",
        "DamageDurationProjectileFumble",
    ),
    ("DamageAttributeDur_RunSpeed", "DamageDurationRunSpeed"),
    (
        "DamageAttributeDur_SpellCastSpeed",
        "DamageDurationSpellCastSpeed",
    ),
    (
        "DamageAttributeDur_TotalDamageReductionAbsolute",
        "DamageTotalDamageReductionAbsolute",
    ),
    (
        "DamageAttributeDur_TotalDamageReductionPercent",
        "DamageTotalDamageReductionPercent",
    ),
    (
        "DamageAttributeDur_TotalResistanceReductionAbsolute",
        "DamageTotalResistanceReductionAbsolute",
    ),
    (
        "DamageAttributeDur_TotalResistanceReductionPercent",
        "DamageTotalResistanceReductionPercent",
    ),
    ("DamageAttributeDur_TotalSpeed", "DamageDurationTotalSpeed"),
    ("DamageAttributeInfluence", "DamageInfluenceRangeFormat"),
    ("DamageAttributeReflex", "DamageInfluenceRangeFormat"),
    ("DamageAttributeReflex_Freeze", "DamageFreeze"),
    ("DamageAttributeReflex_Knockdown", "DamageKnockdown"),
    ("DamageAttributeReflex_Petrify", "DamagePetrify"),
    ("DamageAttributeReflex_Sleep", "tagDamageSleep"),
    ("DamageAttributeReflex_Stun", "DamageStun"),
    ("DamageAttributeReflex_Trap", "DamageTrap"),
    ("DamageAttribute_BasePhysical", "Not Used"),
    ("DamageAttribute_Physical", "DamagePhysicalChance"),
    (
        "DefenseAttributeAbsMod_Absorption",
        "DefenseAbsorptionModifier",
    ),
    ("DefenseAttributeAbsMod_Aether", "DefenseAetherModifier"),
    ("DefenseAttributeAbsMod_Bleeding", "DefenseBleedingModifier"),
    ("DefenseAttributeAbsMod_Chaos", "DefenseChaosModifier"),
    ("DefenseAttributeAbsMod_Cold", "DefenseColdModifier"),
    (
        "DefenseAttributeAbsMod_Elemental",
        "DefenseElementalModifier",
    ),
    ("DefenseAttributeAbsMod_Fire", "DefenseFireModifier"),
    (
        "DefenseAttributeAbsMod_Knockdown",
        "DefenseKnockdownModifier",
    ),
    ("DefenseAttributeAbsMod_Life", "DefenseLifeModifier"),
    (
        "DefenseAttributeAbsMod_LifeLeach",
        "DefenseLifeLeachModifier",
    ),
    (
        "DefenseAttributeAbsMod_Lightning",
        "DefenseLightningModifier",
    ),
    (
        "DefenseAttributeAbsMod_ManaLeach",
        "DefenseManaLeachModifier",
    ),
    ("DefenseAttributeAbsMod_Physical", "DefensePhysicalModifier"),
    ("DefenseAttributeAbsMod_Pierce", "DefensePierceModifier"),
    ("DefenseAttributeAbsMod_Poison", "DefensePoisonModifier"),
    (
        "DefenseAttributeAbsMod_Protection",
        "DefenseProtectionModifier",
    ),
    ("DefenseAttributeAbsMod_Sleep", "tagDefenseSleepModifier"),
    ("DefenseAttributeAbsMod_Stun", "DefenseStunModifier"),
    ("DefenseAttributeAbs_Aether", "DefenseAether"),
    ("DefenseAttributeAbs_AllResistance", "DefenseAllResistance"),
    ("DefenseAttributeAbs_Bleeding", "DefenseBleeding"),
    ("DefenseAttributeAbs_Chaos", "DefenseChaos"),
    ("DefenseAttributeAbs_Cold", "DefenseCold"),
    ("DefenseAttributeAbs_Confusion", "DefenseConfusion"),
    ("DefenseAttributeAbs_Convert", "DefenseConvert"),
    (
        "DefenseAttributeAbs_CrowdControl",
        "DefenseCrowdControlNegative",
    ),
    ("DefenseAttributeAbs_Disruption", "DefenseDisruption"),
    (
        "DefenseAttributeAbs_ElementalResistance",
        "DefenseElementalResistance",
    ),
    ("DefenseAttributeAbs_Fear", "DefenseFear"),
    ("DefenseAttributeAbs_Fire", "DefenseFire"),
    ("DefenseAttributeAbs_Freeze", "DefenseFreezeNegative"),
    ("DefenseAttributeAbs_Knockdown", "DefenseKnockdownNegative"),
    ("DefenseAttributeAbs_Life", "DefenseLife"),
    ("DefenseAttributeAbs_LifeLeach", "DefenseLifeLeach"),
    ("DefenseAttributeAbs_Lightning", "DefenseLightning"),
    ("DefenseAttributeAbs_ManaBurn", "DefenseManaBurn"),
    ("DefenseAttributeAbs_ManaBurnRatio", "DefenseManaBurnRatio"),
    ("DefenseAttributeAbs_ManaLeach", "DefenseManaLeach"),
    (
        "DefenseAttributeAbs_PercentCurrentLife",
        "DefensePercentCurrentLife",
    ),
    (
        "DefenseAttributeAbs_PercentReflectionResistance",
        "DefenseReflectResist",
    ),
    ("DefenseAttributeAbs_Petrify", "DefensePetrifyNegative"),
    ("DefenseAttributeAbs_Physical", "DefensePhysical"),
    ("DefenseAttributeAbs_Pierce", "DefensePierce"),
    ("DefenseAttributeAbs_Poison", "DefensePoison"),
    ("DefenseAttributeAbs_Sleep", "tagDefenseSleep"),
    ("DefenseAttributeAbs_Stun", "DefenseStunNegative"),
    ("DefenseAttributeAbs_Taunt", "DefenseTaunt"),
    ("DefenseAttributeAbs_TotalSpeed", "tagTotalSpeedResistance"),
    ("DefenseAttributeAbs_Trap", "DefenseTrapNegative"),
    (
        "DefenseAttributeDefenseCap_Aether",
        "DefenseAetherMaxResist",
    ),
    ("DefenseAttributeDefenseCap_All", "DefenseAllMaxResist"),
    (
        "DefenseAttributeDefenseCap_Bleeding",
        "DefenseBleedingMaxResist",
    ),
    ("DefenseAttributeDefenseCap_Chaos", "DefenseChaosMaxResist"),
    ("DefenseAttributeDefenseCap_Cold", "DefenseColdMaxResist"),
    (
        "DefenseAttributeDefenseCap_CrowdControl",
        "DefenseCrowdControlMaxResist",
    ),
    (
        "DefenseAttributeDefenseCap_Disruption",
        "DefenseDisruptionMaxResist",
    ),
    ("DefenseAttributeDefenseCap_Fire", "DefenseFireMaxResist"),
    (
        "DefenseAttributeDefenseCap_Freeze",
        "DefenseFreezeMaxResist",
    ),
    ("DefenseAttributeDefenseCap_Life", "DefenseLifeMaxResist"),
    (
        "DefenseAttributeDefenseCap_LifeLeach",
        "DefenseLifeLeachMaxResist",
    ),
    (
        "DefenseAttributeDefenseCap_Lightning",
        "DefenseLightningMaxResist",
    ),
    (
        "DefenseAttributeDefenseCap_ManaLeach",
        "DefenseManaLeachMaxResist",
    ),
    (
        "DefenseAttributeDefenseCap_Petrify",
        "DefensePetrifyMaxResist",
    ),
    (
        "DefenseAttributeDefenseCap_Physical",
        "DefensePhysicalMaxResist",
    ),
    (
        "DefenseAttributeDefenseCap_Pierce",
        "DefensePierceMaxResist",
    ),
    (
        "DefenseAttributeDefenseCap_Poison",
        "DefensePoisonMaxResist",
    ),
    ("DefenseAttributeDefenseCap_Sleep", "DefenseSleepMaxResist"),
    ("DefenseAttributeDefenseCap_Stun", "DefenseStunMaxResist"),
    ("DefenseAttributeDefenseCap_Trap", "DefenseTrapMaxResist"),
    (
        "DefenseAttributeDurMod_Aether",
        "DefenseAetherDurationModifier",
    ),
    (
        "DefenseAttributeDurMod_Bleeding",
        "DefenseBleedingDurationModifier",
    ),
    (
        "DefenseAttributeDurMod_Chaos",
        "DefenseChaosDurationModifier",
    ),
    ("DefenseAttributeDurMod_Cold", "DefenseColdDurationModifier"),
    ("DefenseAttributeDurMod_Fire", "DefenseFireDurationModifier"),
    ("DefenseAttributeDurMod_Life", "DefenseLifeDurationModifier"),
    (
        "DefenseAttributeDurMod_LifeLeach",
        "DefenseLifeLeachDurationModifier",
    ),
    (
        "DefenseAttributeDurMod_Lightning",
        "DefenseLightningDurationModifier",
    ),
    (
        "DefenseAttributeDurMod_ManaLeach",
        "DefenseManaLeachDurationModifier",
    ),
    (
        "DefenseAttributeDurMod_Physical",
        "DefensePhysicalDurationModifier",
    ),
    (
        "DefenseAttributeDurMod_Poison",
        "DefensePoisonDurationModifier",
    ),
    ("DefenseAttributeDur_Aether", "DefenseAetherDuration"),
    ("DefenseAttributeDur_Bleeding", "DefenseBleedingDuration"),
    ("DefenseAttributeDur_Chaos", "DefenseChaosDuration"),
    ("DefenseAttributeDur_Cold", "DefenseColdDuration"),
    ("DefenseAttributeDur_Fire", "DefenseFireDuration"),
    ("DefenseAttributeDur_Life", "DefenseLifeDuration"),
    ("DefenseAttributeDur_LifeLeach", "DefenseLifeLeachDuration"),
    ("DefenseAttributeDur_Lightning", "DefenseLightningDuration"),
    ("DefenseAttributeDur_ManaLeach", "DefenseManaLeachDuration"),
    ("DefenseAttributeDur_Physical", "DefensePhysicalDuration"),
    ("DefenseAttributeDur_Poison", "DefensePoisonDuration"),
    (
        "DefenseAttributeMisc_BlockAmountModifier",
        "DefenseBlockAmountModifier",
    ),
    ("DefenseAttributeMisc_BlockModifier", "DefenseBlockModifier"),
    (
        "DefenseAttributeMisc_NegativeDamageMultiplier",
        "DefenseNegativeDamageMult",
    ),
    ("DefenseAttributeMisc_Reflect", "DefenseReflect"),
    (
        "DefenseAttributeMisc_ReflectModifier",
        "DefenseReflectModifier",
    ),
    (
        "RetaliationAttributeAbsMod_Aether",
        "RetaliationModifierAether",
    ),
    (
        "RetaliationAttributeAbsMod_Chaos",
        "RetaliationModifierChaos",
    ),
    ("RetaliationAttributeAbsMod_Cold", "RetaliationModifierCold"),
    (
        "RetaliationAttributeAbsMod_DamageMultiplier",
        "tagRetaliationModifierDamageMult",
    ),
    (
        "RetaliationAttributeAbsMod_Elemental",
        "RetaliationModifierElemental",
    ),
    ("RetaliationAttributeAbsMod_Fire", "RetaliationModifierFire"),
    (
        "RetaliationAttributeAbsMod_Knockdown",
        "RetaliationModifierKnockdown",
    ),
    ("RetaliationAttributeAbsMod_Life", "RetaliationModifierLife"),
    (
        "RetaliationAttributeAbsMod_Lightning",
        "RetaliationModifierLightning",
    ),
    (
        "RetaliationAttributeAbsMod_Physical",
        "RetaliationModifierPhysical",
    ),
    (
        "RetaliationAttributeAbsMod_Pierce",
        "RetaliationModifierPierce",
    ),
    (
        "RetaliationAttributeAbsMod_PierceRatio",
        "RetaliationModifierPierceRatio",
    ),
    (
        "RetaliationAttributeAbsMod_Poison",
        "RetaliationModifierPoison",
    ),
    (
        "RetaliationAttributeAbsMod_Sleep",
        "RetaliationModifierSleep",
    ),
    ("RetaliationAttributeAbsMod_Stun", "RetaliationModifierStun"),
    (
        "RetaliationAttributeAbsMod_TotalDamageModifier",
        "tagRetaliationModifierTotalDamage",
    ),
    ("RetaliationAttributeAbs_Aether", "RetaliationAether"),
    ("RetaliationAttributeAbs_Chaos", "RetaliationChaos"),
    ("RetaliationAttributeAbs_Cold", "RetaliationCold"),
    ("RetaliationAttributeAbs_Confusion", "RetaliationConfusion"),
    ("RetaliationAttributeAbs_Convert", "RetaliationConvert"),
    (
        "RetaliationAttributeAbs_ElementalDamage",
        "RetaliationElemental",
    ),
    ("RetaliationAttributeAbs_Fear", "RetaliationFear"),
    ("RetaliationAttributeAbs_Fire", "RetaliationFire"),
    ("RetaliationAttributeAbs_Life", "RetaliationLife"),
    ("RetaliationAttributeAbs_Lightning", "RetaliationLightning"),
    (
        "RetaliationAttributeAbs_PercentCurrentLife",
        "RetaliationPercentCurrentLife",
    ),
    ("RetaliationAttributeAbs_Physical", "RetaliationPhysical"),
    ("RetaliationAttributeAbs_Pierce", "RetaliationPierce"),
    ("RetaliationAttributeAbs_Poison", "RetaliationPoison"),
    (
        "RetaliationAttributeDurMod_Aether",
        "RetaliationDurationModifierAether",
    ),
    (
        "RetaliationAttributeDurMod_AttackSpeed",
        "RetaliationDurationModifierAttackSpeed",
    ),
    (
        "RetaliationAttributeDurMod_Bleeding",
        "RetaliationDurationModifierBleeding",
    ),
    (
        "RetaliationAttributeDurMod_Chaos",
        "RetaliationDurationModifierChaos",
    ),
    (
        "RetaliationAttributeDurMod_Cold",
        "RetaliationDurationModifierCold",
    ),
    (
        "RetaliationAttributeDurMod_DefensiveAbility",
        "RetaliationDurationModifierDefensiveAbility",
    ),
    (
        "RetaliationAttributeDurMod_Fire",
        "RetaliationDurationModifierFire",
    ),
    (
        "RetaliationAttributeDurMod_Life",
        "RetaliationDurationModifierLife",
    ),
    (
        "RetaliationAttributeDurMod_LifeLeach",
        "RetaliationDurationModifierLifeLeach",
    ),
    (
        "RetaliationAttributeDurMod_Lightning",
        "RetaliationDurationModifierLightning",
    ),
    (
        "RetaliationAttributeDurMod_ManaLeach",
        "RetaliationDurationModifierManaLeach",
    ),
    (
        "RetaliationAttributeDurMod_OffensiveAbility",
        "RetaliationDurationModifierOffensiveAbility",
    ),
    (
        "RetaliationAttributeDurMod_OffensiveReduction",
        "RetaliationDurationModifierOffensiveReduction",
    ),
    (
        "RetaliationAttributeDurMod_Physical",
        "RetaliationDurationModifierPhysical",
    ),
    (
        "RetaliationAttributeDurMod_Poison",
        "RetaliationDurationModifierPoison",
    ),
    (
        "RetaliationAttributeDurMod_RunSpeed",
        "RetaliationDurationModifierRunSpeed",
    ),
    (
        "RetaliationAttributeDurMod_SpellCastSpeed",
        "RetaliationDurationModifierSpellCastSpeed",
    ),
    (
        "RetaliationAttributeDur_Aether",
        "RetaliationDurationAether",
    ),
    (
        "RetaliationAttributeDur_AttackSpeed",
        "RetaliationDurationAttackSpeed",
    ),
    (
        "RetaliationAttributeDur_Bleeding",
        "RetaliationDurationBleeding",
    ),
    ("RetaliationAttributeDur_Chaos", "RetaliationDurationChaos"),
    ("RetaliationAttributeDur_Cold", "RetaliationDurationCold"),
    (
        "RetaliationAttributeDur_DefensiveAbility",
        "RetaliationDurationDefensiveAbility",
    ),
    ("RetaliationAttributeDur_Fire", "RetaliationDurationFire"),
    ("RetaliationAttributeDur_Life", "RetaliationDurationLife"),
    (
        "RetaliationAttributeDur_LifeLeach",
        "RetaliationDurationLifeLeach",
    ),
    (
        "RetaliationAttributeDur_Lightning",
        "RetaliationDurationLightning",
    ),
    (
        "RetaliationAttributeDur_ManaLeach",
        "RetaliationDurationManaLeach",
    ),
    (
        "RetaliationAttributeDur_OffensiveAbility",
        "RetaliationDurationOffensiveAbility",
    ),
    (
        "RetaliationAttributeDur_OffensiveReduction",
        "RetaliationDurationOffensiveReduction",
    ),
    (
        "RetaliationAttributeDur_Physical",
        "RetaliationDurationPhysical",
    ),
    (
        "RetaliationAttributeDur_Poison",
        "RetaliationDurationPoison",
    ),
    (
        "RetaliationAttributeDur_RunSpeed",
        "RetaliationDurationRunSpeed",
    ),
    (
        "RetaliationAttributeDur_SpellCastSpeed",
        "RetaliationDurationSpellCastSpeed",
    ),
    (
        "RetaliationAttributeInfluence",
        "DamageInfluenceRangeFormat",
    ),
    ("RetaliationAttributeReflex", "DamageInfluenceRangeFormat"),
    ("RetaliationAttributeReflex_Freeze", "RetaliationFreeze"),
    (
        "RetaliationAttributeReflex_Knockdown",
        "RetaliationKnockdown",
    ),
    ("RetaliationAttributeReflex_Petrify", "RetaliationPetrify"),
    ("RetaliationAttributeReflex_Sleep", "RetaliationSleep"),
    ("RetaliationAttributeReflex_Stun", "RetaliationStun"),
    ("RetaliationAttributeReflex_Trap", "RetaliationTrap"),
    (
        "SkillAttributeMod_CooldownReduction",
        "SkillCooldownReductionModifier",
    ),
    (
        "SkillAttributeMod_ManaCostReduction",
        "SkillManaCostReductionModifier",
    ),
    (
        "SkillAttributeVal_ComboChargeSpendReduction",
        "SkillComboChargeSpendReduction",
    ),
    (
        "SkillAttributeVal_CooldownReduction",
        "SkillCooldownReduction",
    ),
    (
        "SkillAttributeVal_ManaCostReduction",
        "SkillManaCostReduction",
    ),
    (
        "SkillAttributeVal_ProjectileSpeedModifier",
        "SkillProjectileSpeedModifier",
    ),
    ("Skill_DropProjectileTelekinesis", "H"),
    ("Skill_OrmenosChainLaser", "Beam"),
];
