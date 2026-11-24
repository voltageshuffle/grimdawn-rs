//! Combo charges (Savagery / "Onslaught" and the items granting
//! `skillComboChargeLevel`): one counter per character that skill hits raise
//! and the next cooldown skill spends for a damage bonus. Game.dll
//! (`SkillManager.comboChargeCount`, `.comboChargeTimeLeft` ms, `.comboChargeLockout` ms):
//! - Decay (`SkillManager::Update`): the time left counts down; at 0 with
//!   charges the count drops to 0. The lockout counts down while positive.
//! - Gain (`SkillManager::IncrementComboCharge`; a
//!   `Skill_WPAttack_BasicAttack` forwards to its pool parent): with a target
//!   and the lockout over, the skill's modifier-summed `skillComboChargeLevel`
//!   (0: nothing) and `skillComboChargeDuration` (ms) apply. Lockout =
//!   `skillComboChargeTime` (melee) or `skillComboChargeTimeRanged` ms / attack
//!   speed; count = `min(count + 1, max(count, level))`; the time left restarts.
//! - Use (`Skill::CollectComboMultiplier`): only a skill with a
//!   cooldown, not display parameters, item / granted auto-casts or the
//!   channelled classes (`SkillChanneled`, `Skill_AttackRadiusGrow`,
//!   `…RadiusSpin`, `Skill_AttackSpellDrain`, `…SpellCone`, `…SpellBeam`).
//!   Damage is scaled by (100 + `skillComboChargeMultipliers[min(count, len) −
//!   1]`) % after the % modifiers, once per use.
//! - Spend (`Character::SpendComboCharges`; pets with player
//!   scaling spend their master's): a roll × 100 above 100 −
//!   `SkillComboChargeSpendReduction` keeps the charges, else they drop to 0.
//! - Clear (`Character::ClearComboCharges`, players only): a weapon attach,
//!   the weapon swap, a teleport of the player.
//! - Look: one aura `skillComboChargeAura[count − 1]` (none at 0); reaching
//!   the gaining skill's level plays its `skillComboChargeSound`.
use crate::items::db::Fields;
use gd_formats::arz::Record;
use gd_rules::Rng;

/// `gameengine.dbr`'s combo charge fields (`GameEngine::LoadFromDatabase`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ComboTable {
    /// `skillComboChargeTime` × 1000 (ms).
    pub time_ms: i32,
    /// `skillComboChargeTimeRanged` × 1000 (ms).
    pub ranged_time_ms: i32,
    /// `skillComboChargeMultipliers` (% per count).
    pub multipliers: Vec<f32>,
    /// `skillComboChargeAura` (an effect per count).
    pub auras: Vec<String>,
}

impl ComboTable {
    pub fn from_engine(rec: &Record) -> Self {
        Self {
            time_ms: (rec.float("skillComboChargeTime") * 1000.0) as i32,
            ranged_time_ms: (rec.float("skillComboChargeTimeRanged") * 1000.0) as i32,
            multipliers: rec.raw_values("skillComboChargeMultipliers"),
            auras: rec
                .strs_iter("skillComboChargeAura")
                .map(|s| s.to_string())
                .collect(),
        }
    }

    /// `GetComboChargeMultiplier`: % bonus of `count` charges.
    pub fn multiplier(&self, count: u32) -> f32 {
        if count == 0 || self.multipliers.is_empty() {
            return 0.0;
        }
        self.multipliers[(count as usize - 1).min(self.multipliers.len() - 1)]
    }

    /// `GetComboChargeAuraName`: the aura shown with `count` charges.
    pub fn aura(&self, count: u32) -> Option<&str> {
        if count == 0 || self.auras.is_empty() {
            return None;
        }
        Some(self.auras[(count as usize - 1).min(self.auras.len() - 1)].as_str())
            .filter(|s| !s.is_empty())
    }
}

/// A skill's combo fields at a level (own record + modifiers):
/// (`skillComboChargeLevel`, `skillComboChargeDuration` ms).
pub fn skill_combo(records: &[(&Record, usize)]) -> (u32, f32) {
    let lv = |r: &Record, k: &str, li: usize| crate::stats::level_value(r, k, li).unwrap_or(0.0);
    let level: i32 = records
        .iter()
        .map(|(r, li)| lv(r, "skillComboChargeLevel", *li) as i32)
        .sum();
    let duration: f32 = records
        .iter()
        .map(|(r, li)| lv(r, "skillComboChargeDuration", *li) * 1000.0)
        .sum();
    (level.max(0) as u32, duration)
}

/// Whether a skill class can take the multiplier (not channelled:
/// `Skill::IsChanneled`).
pub fn class_takes_multiplier(class: &str) -> bool {
    !matches!(
        class,
        "SkillChanneled"
            | "Skill_AttackRadiusGrow"
            | "Skill_AttackRadiusSpin"
            | "Skill_AttackSpellDrain"
            | "Skill_AttackSpellCone"
            | "Skill_AttackSpellBeam"
    )
}

/// The character's combo charges (`SkillManager.comboChargeCount` .. `SkillManager.comboChargeLockout`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ComboCharges {
    pub count: u32,
    /// Milliseconds before the charges expire.
    pub remaining_ms: i32,
    /// Milliseconds before another charge can be gained.
    pub lockout_ms: i32,
}

impl ComboCharges {
    /// `SkillManager::Update`: true when the charges just expired.
    pub fn update(&mut self, ms: i32) -> bool {
        let mut expired = false;
        if self.remaining_ms > 0 {
            self.remaining_ms -= ms;
            if self.count != 0 && self.remaining_ms <= 0 {
                self.count = 0;
                expired = true;
            }
        }
        if self.lockout_ms > 0 {
            self.lockout_ms -= ms;
        }
        expired
    }

    /// `SkillManager::IncrementComboCharge` for a skill result with targets:
    /// the gaining skill's `level` / `duration_ms` ([`skill_combo`]); `melee`:
    /// the attack's `AttackInfo.isMelee` flag. True when the count was set.
    pub fn increment(
        &mut self,
        table: &ComboTable,
        level: u32,
        duration_ms: f32,
        melee: bool,
        attack_speed: f32,
    ) -> bool {
        if self.lockout_ms > 0 || level == 0 {
            return false;
        }
        let time = if melee {
            table.time_ms
        } else {
            table.ranged_time_ms
        };
        self.lockout_ms = (time as f32 / attack_speed) as i32;
        let cap = self.count.max(level);
        self.count = (self.count + 1).min(cap);
        self.remaining_ms = duration_ms as i32;
        true
    }

    /// `Character::SpendComboCharges`: true when the charges stay.
    pub fn spend(&mut self, reduction: f32, rng: &mut Rng) -> bool {
        rng.next();
        if rng.unit() * 100.0 > 100.0 - reduction {
            return true;
        }
        self.clear();
        false
    }

    /// `Character::ClearComboCharges`.
    pub fn clear(&mut self) {
        self.count = 0;
    }
}
