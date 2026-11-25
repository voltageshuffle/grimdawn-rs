//! Constitution: the pool behind accelerated out-of-combat life regeneration (`CharacterBio::Update`,
//! `CharacterBio::TakeBonus`, `GetConstitutionLimit`).
//! - Limit = `characterConstitution` through the accumulator (`CharacterSheet::constitution`; stock 300).
//!   Refilled to the limit on spawn / load / respawn (not saved).
//! - A limit change keeps the ratio `cur / limit` (1.0 when the old limit is ~0).
//! - Food adds `min(limit, cur + limit * bonusConstitutionPercent / 100)` then `min(limit, cur + bonusConstitutionPoints)`.
//! - Drain, per tick while life > 0, not in combat, no damage taken this tick, `cur > 0` and life < max life:
//!   `heal = acceleratedLifeRegenPercent * 0.01 * maxLife * dt` (stock 25), `heal = min(heal, cur)`,
//!   `cur -= 100 / maxLife * heal`, `life = min(life + heal, maxLife)` on top of the normal regen.
//! - PORT: out of combat is `combat_left`: set to `gameengine.dbr` `combatIdleTime` (stock 1.2 s) by the
//!   player's own attack on a non-breakable and by any life loss since the last tick, counted down per tick
//!   (`CombatManager::Update`'s countdown is not decompiled).
use crate::combat::AttackMade;
use bevy::prelude::*;
use gd_game_core::combat::{CombatSet, Combatant, Dead};
use gd_game_core::data::GameData;
use gd_game_core::player::{Loadout, Player, player_record};

/// The player's constitution (engine `CharacterBio.constitution`).
#[derive(Component, Debug, Clone, Copy)]
pub struct Constitution {
    pub current: f32,
    /// The limit `current` was last scaled to.
    limit_seen: f32,
    /// Seconds until the player counts as out of combat.
    combat_left: f32,
    /// Life at the end of the last tick (damage taken = a drop since).
    last_life: f32,
    /// Dead: refill to the limit on revival (the bio's re-init).
    restore: bool,
}

impl Constitution {
    pub fn full(limit: f32, life: f32) -> Self {
        Self {
            current: limit.max(0.0),
            limit_seen: limit,
            combat_left: 0.0,
            last_life: life,
            restore: false,
        }
    }

    /// Food: `CharacterBio::TakeBonus`.
    pub fn take_bonus(&mut self, limit: f32, percent: f32, points: f32) {
        let c = (self.current + limit * percent / 100.0).min(limit);
        self.current = (c + points).min(limit);
    }

    /// `CharacterBio.outOfCombat`.
    pub fn out_of_combat(&self) -> bool {
        self.combat_left <= 0.0
    }
}

pub struct ConstitutionPlugin;

impl Plugin for ConstitutionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (constitution_init, constitution_tick)
                .chain()
                .after(CombatSet)
                .run_if(resource_exists::<GameData>),
        );
    }
}

fn constitution_init(
    mut commands: Commands,
    q: Query<(Entity, &Combatant), (With<Player>, Without<Constitution>)>,
) {
    for (e, cb) in &q {
        commands
            .entity(e)
            .try_insert(Constitution::full(cb.profile.sheet.constitution, cb.life));
    }
}

/// `combatIdleTime` and the male / female records' `acceleratedLifeRegenPercent`.
type Rates = (f32, [f32; 2]);

fn rates(data: &GameData) -> Rates {
    let percent = |female| {
        data.db
            .get(player_record(female))
            .and_then(|r| r.f32("acceleratedLifeRegenPercent"))
            .unwrap_or(25.0)
    };
    let idle = data
        .db
        .get("records/game/gameengine.dbr")
        .and_then(|r| r.f32("combatIdleTime"))
        .unwrap_or(1.2);
    (idle, [percent(false), percent(true)])
}

#[allow(clippy::too_many_arguments)]
fn constitution_tick(
    time: Res<Time>,
    data: Res<GameData>,
    mut attacks: MessageReader<AttackMade>,
    breakables: Query<(), With<gd_game_core::world::Breakable>>,
    mut q: Query<
        (
            Entity,
            &mut Combatant,
            &mut Constitution,
            &Loadout,
            Has<Dead>,
        ),
        With<Player>,
    >,
    (mut cached, mut fresh): (Local<Option<Rates>>, gd_game_core::session::NewSession),
    mut attacked: Local<Vec<Entity>>,
) {
    // A new session may run another mod's records.
    if fresh.begun() {
        *cached = None;
    }
    let (idle, percents) = *cached.get_or_insert_with(|| rates(&data));
    let dt = time.delta_secs();
    attacked.clear();
    attacked.extend(
        attacks
            .read()
            .filter(|a| !breakables.contains(a.target))
            .map(|a| a.attacker),
    );
    for (e, mut cb, mut c, loadout, dead) in &mut q {
        if dead {
            c.restore = true;
            continue;
        }
        let sheet = &cb.profile.sheet;
        let (limit, max_life) = (sheet.constitution, sheet.life);
        // CharacterBio::Update: revival re-inits to the limit.
        if c.restore {
            c.current = limit;
            c.restore = false;
        } else if (limit - c.limit_seen).abs() > 1e-4 {
            let ratio = if c.limit_seen <= 1e-4 {
                1.0
            } else {
                c.current / c.limit_seen
            };
            c.current = limit * ratio;
        }
        c.limit_seen = limit;
        let damaged = cb.life < c.last_life;
        if damaged || attacked.contains(&e) {
            c.combat_left = idle;
        } else {
            c.combat_left = (c.combat_left - dt).max(0.0);
        }
        let percent = percents[loadout.female as usize];
        if cb.life > 0.0 && c.out_of_combat() && !damaged && c.current > 0.0 && max_life > cb.life {
            let heal = (percent * 0.01 * max_life * dt).min(c.current);
            c.current -= (100.0 / max_life) * heal;
            cb.life = (cb.life + heal).min(max_life);
        }
        c.last_life = cb.life;
    }
}
