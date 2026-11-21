//! The combat rules ([`gd_sim::actor::Rules`]) at the session's difficulty, and the
//! `gameengine.dbr` values gameplay reads ([`GameEngineRules`], held by [`GameData::engine`]).
use crate::data::GameData;
use crate::session::SessionReady;
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_sim::actor::Rules;
use std::sync::Arc;

/// `records/game/gameengine.dbr` (`GameEngine::LoadFromDatabase` @0x1802c8750), parsed once per mounted
/// database ([`GameData::engine`]) instead of looked up per hit or cast. Defaults are the stock values.
pub struct GameEngineRules {
    /// The record itself, for the readers of a whole record (`ComboTable::from_engine`) and of a field
    /// chosen per use (the per-type damage fx paks).
    pub record: Option<Record>,
    /// `meleeRange` / `shortRange` / `moderateRange` / `longRange` / `maximumRange` / `bossRange`: the
    /// `distanceProfile` ranges ([`Self::profile_range`]).
    ranges: [f32; 6],
    /// `absoluteRunSpeedCapMax` (%).
    pub absolute_run_cap: f32,
    /// `alertDistance` (`gGameEngine` +1258): an enemy nearer than this is charged without the alert.
    pub alert_distance: f32,
    /// `meleeTargetDistance` (stock 2.4): the melee hit re-check's slack, the 3-D centre distance minus
    /// both radii (`Skill::IsTargetInRange` @0x180497a90 with `gGameEngine+1257`;
    /// `SkillActivated::ExecuteMeleeAttack` @0x180518320 re-checks it at the hit callback).
    pub melee_target_distance: f32,
    /// `meleeAutoTargetDistance` (stock 4.0): the cone fallback's reach over the radii when the primary
    /// target stepped out of melee reach (`ExecuteMeleeAttack` → `GameEngine::GetTargetsInCone`).
    pub melee_auto_target_distance: f32,
    /// `miniPetLimit` (0: none).
    pub mini_pet_limit: usize,
    /// `freezeDamageHandler` / `petrifyDamageHandler`: the icy look's handler records.
    pub freeze_handler: String,
    pub petrify_handler: String,
    /// `aetherCrystal`: the aether crystal item record.
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

    /// `longRange`: a ranged weapon's reach.
    pub fn long_range(&self) -> f32 {
        self.ranges[3]
    }
}

#[derive(Resource, Clone)]
pub struct CombatRules(pub Arc<Rules>);

/// The session's rules failed to load; stops [`load_rules`] retrying (and logging) every frame.
/// Removed with the session.
#[derive(Resource)]
pub struct RulesFailed;

pub struct RulesPlugin;

impl Plugin for RulesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            load_rules
                .run_if(resource_exists::<SessionReady>)
                .run_if(not(resource_exists::<CombatRules>))
                .run_if(not(resource_exists::<RulesFailed>)),
        );
    }
}

/// The rules, at the session's difficulty (game balance level).
fn load_rules(mut commands: Commands, data: Res<GameData>, mode: Res<crate::session::GameMode>) {
    match Rules::load(&data.db) {
        Ok(r) => {
            let difficulty = mode.0.difficulty;
            // The session's choice (Veteran / Ultimate Veteran).
            let veteran = mode.0.veteran() || mode.0.ultimate_veteran();
            commands.insert_resource(CombatRules(Arc::new(
                r.with_difficulty(difficulty)
                    .with_veteran(&data.db, veteran),
            )));
        }
        Err(e) => {
            error!("combat rules: {e:#}");
            commands.insert_resource(RulesFailed);
        }
    }
}
