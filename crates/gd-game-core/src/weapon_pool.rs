//! Weapon-pool data of a character: the learned skill-modifier links and
//! usable pool skills ([`SkillLinks`]) and the charge state of the charged
//! pool skills ([`PoolCharges`]: Cadence's finale, Righteous Fervor's
//! scaling). The systems that build and run them are `gd_game::weapon_pool`.
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_formats::collections::HashMap;
use gd_rules::Rng;
use gd_sim::equip::WeaponType;

/// Learned modifiers and usable pool skills of a character.
#[derive(Component, Default, Debug, Clone)]
pub struct SkillLinks {
    /// Lower-case skill → its learned modifiers (record, level index): the
    /// `Skill_Modifier` family (transmuters, projectile modifiers…), whose
    /// fields add to the skill (`Skill.modifiers` list, class check).
    pub mods: HashMap<String, Vec<(String, usize)>>,
    /// Lower-case skill → its learned `SkillSecondary_*` skills
    /// (`Skill.secondarySkills` list, `Skill::GetSecondarySkills`): separate effects, not damage
    /// (`secondary.rs`); the sheet's skill rollovers add their damage.
    pub secondaries: HashMap<String, Vec<(String, usize)>>,
    /// Lower-case skill → the gear-granted secondaries (`Skill.itemSecondarySkills`,
    /// `AddToItemSecondarySkills`): they act like the learned ones
    /// (`ActivateSecondarySkills` runs both lists) but the
    /// rollovers leave them out (`gd_sim::skills` docs).
    pub item_secondaries: HashMap<String, Vec<(String, usize)>>,
    /// Lower-case skill → its gear modifiers that are off (`Skill_Modifier::IsSkillEnabled`:
    /// exclusive to a learned transmuter, `gd_game::weapon_pool::build_links`): not in [`Self::mods`], so they add
    /// nothing and have no target effect; only the warm-up starts their paks (`casting::warmup`).
    pub off_mods: HashMap<String, Vec<(String, usize)>>,
    /// Usable pool skills: (record, level index, chance weight).
    pub pool: Vec<(String, usize, u32)>,
    /// The worn weapons' types (right, left hand).
    pub hands: (Option<WeaponType>, Option<WeaponType>),
    /// Lower-case skill → the modifier replacing its weapon requirements
    /// (`gd_sim::weapon_pool::weapon_list_override`).
    pub weapon_override: HashMap<String, String>,
}

impl SkillLinks {
    /// A monster's (or any record skill list's) links, as
    /// `SkillManager::LoadSkills` builds them by calling
    /// `CreateAndLoadSkill` for each `skillNameN` in order:
    /// a `SkillSecondary_*` joins the last ordinary skill's `Skill.secondarySkills`
    /// list ([`Self::secondaries`]); a `Skill_Modifier` (and subclasses)
    /// joins the `Skill.modifiers` list ([`Self::mods`]) of its `subSkillName`
    /// (`Skill_Modifier::Load`) when that names a skill
    /// already created (`FindSkillId`: an earlier entry), else of the last
    /// ordinary skill (`gd_sim::skills::tree_bases`); a `Skill_Mastery`
    /// clears the last skill. Each is keyed by that skill and its
    /// `buffSkillName` (wrappers cast as it). Stock data: the giants' Cleaving
    /// Strikes modifier (`giant_cleavingstrikesmod`, a 45 deg / 5 target cone).
    pub fn of_list(db: &gd_formats::vfs::Database, skills: &[(String, u32)]) -> Self {
        let names: Vec<String> = skills.iter().map(|s| s.0.clone()).collect();
        let mut links = Self::default();
        for (i, ((sk, level), base)) in skills
            .iter()
            .zip(gd_sim::skills::tree_bases(db, &names))
            .enumerate()
        {
            let (Some(base), Some(r)) = (base, db.get(sk)) else {
                continue;
            };
            // `FindSkillId` only finds skills created before this one.
            if !names[..i].iter().any(|n| n.eq_ignore_ascii_case(&base)) {
                continue;
            }
            let secondary = gd_sim::skills::is_secondary_class(&r.class);
            let mut keys = vec![base.to_ascii_lowercase()];
            if let Some(b) = db
                .get(&base)
                .and_then(|br| gd_sim::skills::profile_name(br).map(str::to_ascii_lowercase))
            {
                keys.push(b);
            }
            let map = if secondary {
                &mut links.secondaries
            } else {
                &mut links.mods
            };
            for k in keys {
                map.entry(k)
                    .or_default()
                    .push((sk.clone(), (*level).max(1) as usize - 1));
            }
        }
        links
    }

    /// Whether the worn weapons allow `skill` (`Skill::SetAvailability`:
    /// `QualifyingWeapon` and `QualifyingHandState`).
    pub fn weapons_allow(&self, db: &gd_formats::vfs::Database, rec: &Record, skill: &str) -> bool {
        let over = self
            .weapon_override
            .get(&skill.to_ascii_lowercase())
            .and_then(|m| db.get(m));
        gd_sim::weapon_pool::qualifies(rec, over, self.hands.0, self.hands.1)
    }

    pub fn mods_of(&self, skill: &str) -> &[(String, usize)] {
        self.mods
            .get(&skill.to_ascii_lowercase())
            .map_or(&[], |v| v.as_slice())
    }

    /// [`Self::off_mods`] of `skill`.
    pub fn off_mods_of(&self, skill: &str) -> &[(String, usize)] {
        self.off_mods
            .get(&skill.to_ascii_lowercase())
            .map_or(&[], |v| v.as_slice())
    }

    /// Every secondary of `skill` (lower-case key): the learned ones, then
    /// the gear's (`Skill::ActivateSecondarySkills` order).
    pub fn all_secondaries_of(&self, skill: &str) -> impl Iterator<Item = &(String, usize)> {
        self.secondaries
            .get(skill)
            .into_iter()
            .flatten()
            .chain(self.item_secondaries.get(skill).into_iter().flatten())
    }

    /// The skills (lower-case keys) with any secondary.
    pub fn secondary_parents(&self) -> impl Iterator<Item = &String> {
        self.secondaries.keys().chain(
            self.item_secondaries
                .keys()
                .filter(|k| !self.secondaries.contains_key(*k)),
        )
    }

    /// Rolls the pool for one weapon-pool attack: the pool skill to fire
    /// instead, if any.
    pub fn roll(&self, rng: &mut Rng) -> Option<(String, usize)> {
        let weights: Vec<u32> = self.pool.iter().map(|p| p.2).collect();
        let i = gd_sim::weapon_pool::select(&weights, rng)?;
        Some((self.pool[i].0.clone(), self.pool[i].1))
    }
}

/// Is this a weapon-pool skill whose attacks roll the pool?
pub fn is_pool_parent(rec: &Record) -> bool {
    rec.class.starts_with("Skill_WeaponPool")
}

/// Charge state of the charged pool skills (lower-case skill → charge).
#[derive(Component, Default, Debug)]
pub struct PoolCharges(pub HashMap<String, Charge>);

#[derive(Default, Debug, Clone, Copy)]
pub struct Charge {
    pub count: u32,
    /// Seconds before another charge can be gained.
    pub lockout: f32,
    /// Seconds before the charges expire (Cadence) / one is lost (Fervor).
    pub decay: f32,
}

/// What a weapon-pool skill's attack does ([`PoolCharges::step`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PoolStep {
    /// Cadence's finale: the skill's own hit, no pool roll.
    Finale,
    /// A pool attack; `parent_damage`: the skill's damage applies.
    Attack { parent_damage: bool },
}

/// A skill field's value at a level index (0 without one).
pub fn lv(rec: &Record, k: &str, i: usize) -> f32 {
    gd_sim::stats::level_value(rec, k, i).unwrap_or(0.0)
}

impl PoolCharges {
    /// Whether Cadence's next attack is the finale: full charges and the
    /// lockout over (`Skill_WeaponPool_ChargedFinale::StartAction`).
    pub fn finale_ready(&self, rec: &Record, skill: &str, li: usize) -> bool {
        let level = lv(rec, "skillChargeLevel", li).max(1.0) as u32;
        rec.class == "Skill_WeaponPool_ChargedFinale"
            && self
                .0
                .get(&skill.to_ascii_lowercase())
                .is_some_and(|c| c.count >= level && c.lockout <= 0.0)
    }

    /// The hit of one attack of the pool skill `rec` (`has_target`:
    /// something to hit; `finale`: the attack started as Cadence's finale,
    /// [`Self::finale_ready`] at its start).
    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        rec: &Record,
        skill: &str,
        li: usize,
        attack_speed: f32,
        has_target: bool,
        ranged: bool,
        finale_start: bool,
    ) -> PoolStep {
        let finale = rec.class == "Skill_WeaponPool_ChargedFinale";
        if !finale && rec.class != "Skill_WeaponPool_ChargedScaling" {
            return PoolStep::Attack {
                parent_damage: true,
            };
        }
        let level = lv(rec, "skillChargeLevel", li).max(1.0) as u32;
        let c = self.0.entry(skill.to_ascii_lowercase()).or_default();
        if finale && finale_start {
            if rec.bool("spendChargesOnMiss") {
                *c = Charge::default();
            }
            return PoolStep::Finale;
        }
        if has_target && c.lockout <= 0.0 && (c.count < level || !finale) {
            c.count = (c.count + 1).min(level);
            let t = if finale && ranged {
                lv(rec, "skillChargeTimeRanged", li)
            } else {
                lv(rec, "skillChargeTime", li)
            };
            c.lockout = t / attack_speed.max(0.1);
            c.decay = lv(rec, "skillChargeDuration", c.count as usize - 1);
        }
        PoolStep::Attack {
            parent_damage: !finale,
        }
    }

    /// Fervor's damage scale for `skill` (1 for other skills).
    pub fn scale(&self, rec: &Record, skill: &str) -> f32 {
        if rec.class != "Skill_WeaponPool_ChargedScaling" {
            return 1.0;
        }
        let n = self
            .0
            .get(&skill.to_ascii_lowercase())
            .map_or(0, |c| c.count);
        if n == 0 {
            return 0.0;
        }
        rec.f32s_iter("skillChargeMultipliers")
            .nth(n as usize - 1)
            .or_else(|| rec.f32s_iter("skillChargeMultipliers").last())
            .map(|m| m * 0.01)
            .unwrap_or(1.0)
    }
}
