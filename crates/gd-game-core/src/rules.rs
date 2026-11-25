//! The combat rules ([`gd_sim::actor::Rules`]) at the session's difficulty, and the
//! `gameengine.dbr` values gameplay reads ([`GameEngineRules`], held by [`GameData::engine`](crate::data::GameData::engine)).
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_sim::actor::Rules;
use std::sync::Arc;

/// `records/game/gameengine.dbr` (`GameEngine::LoadFromDatabase`). Defaults are the stock values.
pub struct GameEngineRules {
    /// For readers of the whole record or of a field chosen per use.
    pub record: Option<Record>,
    /// The `distanceProfile` ranges, in `PROFILES` order.
    ranges: [f32; 6],
    /// `absoluteRunSpeedCapMax` (%).
    pub absolute_run_cap: f32,
    /// `alertDistance`: an enemy nearer than this is charged without the alert.
    pub alert_distance: f32,
    /// `meleeTargetDistance`: the melee hit re-check's slack over both radii (`Skill::IsTargetInRange`,
    /// re-checked at the hit callback by `SkillActivated::ExecuteMeleeAttack`).
    pub melee_target_distance: f32,
    /// `meleeAutoTargetDistance`: the cone fallback's reach over the radii when the primary target
    /// left melee reach (`ExecuteMeleeAttack` → `GameEngine::GetTargetsInCone`).
    pub melee_auto_target_distance: f32,
    /// `miniPetLimit` (0: none).
    pub mini_pet_limit: usize,
    /// `freezeDamageHandler` / `petrifyDamageHandler`: the icy look's handler records.
    pub freeze_handler: String,
    pub petrify_handler: String,
    pub aether_crystal: Option<String>,
}

const PROFILES: [(&str, &str, f32); 6] = [
    ("Melee", "meleeRange", crate::combat::MELEE_RANGE),
    ("Short", "shortRange", 4.75),
    ("Moderate", "moderateRange", 9.0),
    ("Long", "longRange", 15.0),
    ("Maximum", "maximumRange", 18.0),
    ("Boss", "bossRange", 32.0),
];

impl GameEngineRules {
    pub fn load(db: &gd_formats::vfs::Database) -> Self {
        let record = db.get_owned("records/game/gameengine.dbr");
        let r = record.as_ref();
        let f = |k: &str, d: f32| r.and_then(|r| r.f32(k)).unwrap_or(d);
        let s = |k: &str, d: &str| r.and_then(|r| r.str(k)).unwrap_or(d).to_owned();
        Self {
            ranges: PROFILES.map(|(_, k, d)| f(k, d)),
            absolute_run_cap: f("absoluteRunSpeedCapMax", 350.0),
            alert_distance: f("alertDistance", 6.0),
            melee_target_distance: f("meleeTargetDistance", 2.4),
            melee_auto_target_distance: f("meleeAutoTargetDistance", 4.0),
            mini_pet_limit: r.and_then(|r| r.i32("miniPetLimit")).unwrap_or(0).max(0) as usize,
            freeze_handler: s(
                "freezeDamageHandler",
                "records/fx/damagedefault/dmgspecial_freeze_handler.dbr",
            ),
            petrify_handler: s(
                "petrifyDamageHandler",
                "records/fx/damagedefault/dmgspecial_petrify_handler.dbr",
            ),
            aether_crystal: r.and_then(|r| r.str("aetherCrystal")).map(str::to_owned),
            record,
        }
    }

    /// A `distanceProfile`'s range; an unknown profile is melee's.
    pub fn profile_range(&self, profile: &str) -> f32 {
        let i = PROFILES.iter().position(|p| p.0 == profile).unwrap_or(0);
        self.ranges[i]
    }

    /// A ranged weapon's reach.
    pub fn long_range(&self) -> f32 {
        self.ranges[3]
    }
}

#[derive(Resource, Clone)]
pub struct CombatRules(pub Arc<Rules>);

/// The session's rules failed to load; stops `gd_game::rules::load_rules` retrying every frame.
#[derive(Resource)]
pub struct RulesFailed;
