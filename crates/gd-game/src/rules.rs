//! Loads the session's [`CombatRules`] at its difficulty (the rules data is
//! [`gd_game_core::rules`]).
use bevy::prelude::*;
use gd_game_core::data::GameData;
use gd_game_core::rules::{CombatRules, RulesFailed};
use gd_sim::actor::Rules;
use std::sync::Arc;

pub struct RulesPlugin;

impl Plugin for RulesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            load_rules
                .run_if(resource_exists::<gd_game_core::session::SessionReady>)
                .run_if(not(resource_exists::<CombatRules>))
                .run_if(not(resource_exists::<RulesFailed>)),
        );
    }
}

/// The rules, at the session's difficulty (game balance level).
fn load_rules(
    mut commands: Commands,
    data: Res<GameData>,
    mode: Res<gd_game_core::session::GameMode>,
) {
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
