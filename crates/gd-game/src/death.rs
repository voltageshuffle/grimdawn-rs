//! The death penalty: dying costs experience (`experienceformulas.dbr` `deathPenaltyEquation` at the player's
//! level, clamped to `deathPenaltyMin/Max`), registered for redemption (`GameEngine::RegisterExperienceLoss`). A
//! gravestone (`records/ui/inworld/gravemarker.dbr`) marks the spot; using it gives back loss ×
//! `RedemptionMultiplier` and removes it; levels gained go through `progress::level_up`. Dying again replaces the
//! stone and the registered loss.
//!
//! `Player::CharacterIsDying`: experience never drops below the start of the current level, and the registered
//! loss is what was actually taken. The registered loss lives only in `GameEngine` (never saved), so after a
//! reload the engine's stone gives nothing back. PORT: the stone is not persisted either: it lasts the session.
//!
//! Softcore characters come back at the difficulty's current respawn point (`Player::GetClosestRespawnPoint`):
//! `PlayerStart` holds that point and `combat::dead_bodies` moves the body there.
//!
//! Deaths count into the save's play stats ([`crate::play_stats::PlayCounts`]). Hardcore
//! (`ControllerPlayer::CharacterIsDying`): the penalty and the stone happen as usual, the death count goes up, the
//! game saves at once and shows the Game Over dialog (`tagGameOver`); no respawn. There is no "dead" save flag:
//! the character list shows a hardcore character with a non-zero death count as dead.
use bevy::prelude::*;
use gd_game_core::combat::Dead;
use gd_game_core::data::GameData;
use gd_game_core::player::Player;
use gd_game_core::progress::PlayerProgress;
use gd_game_core::rules::CombatRules;
use gd_game_core::session::{LoadedCharacter, is_hardcore};

pub struct DeathPlugin;

impl Plugin for DeathPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                on_death
                    .run_if(resource_exists::<PlayerProgress>)
                    .run_if(resource_exists::<CombatRules>)
                    // A hardcore death saves on `Added<Dead>`; the file must carry this death's count/penalty.
                    .before(crate::save::SaveStage::Save),
                use_grave
                    .run_if(resource_exists::<PlayerProgress>)
                    .run_if(resource_exists::<CombatRules>)
                    .before(crate::player::rebuild_profile),
            )
                .chain()
                .after(gd_game_core::player::PlayerSet),
        );
        #[cfg(feature = "dev")]
        app.add_systems(
            Update,
            (test_die, test_god)
                .chain()
                .before(on_death)
                .after(gd_game_core::player::PlayerSet),
        );
    }
}

pub const GRAVE: &str = "records/ui/inworld/gravemarker.dbr";
const FORMULAS: &str = "records/game/experienceformulas.dbr";

#[derive(Component)]
pub struct Gravestone {
    /// Experience given back when used.
    pub redeem: u64,
}

/// The experience a death at `level` costs (before the level-start floor).
pub fn death_penalty(db: &gd_formats::vfs::Database, level: i32) -> u64 {
    let Some(r) = db.get(FORMULAS) else { return 0 };
    let l = level as f32;
    let v = r
        .str("deathPenaltyEquation")
        .and_then(|e| gd_rules::expr::Expr::parse(e).ok())
        .map(|e| {
            e.eval(&|n| {
                (n.eq_ignore_ascii_case("playerLevel")
                    || n.eq_ignore_ascii_case("currentPlayerLevel"))
                .then_some(l)
            })
        })
        .unwrap_or(0.0);
    let (lo, hi) = (
        r.f32("deathPenaltyMin").unwrap_or(0.0),
        r.f32("deathPenaltyMax").unwrap_or(f32::MAX),
    );
    v.clamp(lo, hi.max(lo)).max(0.0) as u64
}
/// The player died: the death penalty, and a gravestone where it fell holding the redeemable part (replacing an
/// earlier one).
#[allow(clippy::too_many_arguments)]
pub fn on_death(
    mut commands: Commands,
    died: Query<&Transform, (With<Player>, Added<Dead>)>,
    mut progress: ResMut<PlayerProgress>,
    rules: Res<CombatRules>,
    graves: Query<Entity, With<Gravestone>>,
    data: Res<GameData>,
    mut stats: ResMut<crate::play_stats::PlayCounts>,
    // Option: a new character has no `LoadedCharacter`.
    loaded: Option<Res<LoadedCharacter>>,
    choice: Res<gd_game_core::session::Choice>,
) {
    let Ok(tf) = died.single() else {
        return;
    };
    stats.deaths += 1;
    if is_hardcore(loaded.as_deref(), &choice) {
        info!("death: the hardcore character is dead");
    }
    let p = &mut progress.progress;
    let start = if p.level <= 1 {
        0
    } else {
        gd_sim::progress::Progress::threshold(&rules.0.levels, p.level - 1)
    };
    let loss = death_penalty(&data.db, p.level).min(p.experience.saturating_sub(start));
    p.experience -= loss;
    let mult = data
        .db
        .get(FORMULAS)
        .and_then(|r| r.f32("RedemptionMultiplier"))
        .unwrap_or(0.5);
    let redeem = (loss as f32 * mult) as u64;
    info!("death: lost {loss} experience ({redeem} recoverable at the gravestone)");
    for g in &graves {
        commands.entity(g).try_despawn();
    }
    // No gravestone in the Crucible (`CreateGravestone` only when `GameInfo::GetMode() != 1`).
    if redeem == 0 || choice.survival.is_some() {
        return;
    }
    let tf = Transform::from_translation(tf.translation);
    commands.spawn((
        tf,
        GlobalTransform::from(tf),
        Gravestone { redeem },
        crate::player_orders::Interactable::fixed(0.5),
        Name::new("gravestone"),
    ));
}

#[allow(clippy::too_many_arguments)]
pub fn use_grave(
    mut commands: Commands,
    graves: Query<(&GlobalTransform, &Gravestone)>,
    // The player's radius comes from `sheet`: a second `Combatant` fetch would conflict with it.
    mut players: Query<
        (
            &Transform,
            &mut crate::movement::MovePath,
            &mut crate::player_orders::InteractTarget,
        ),
        (With<Player>, Without<Dead>),
    >,
    mut sheet: crate::progress::LevelUpQuery,
    mut leveled: crate::progress::LevelUpOut,
    mut progress: ResMut<PlayerProgress>,
    rules: Res<CombatRules>,
    data: Res<GameData>,
) {
    let Ok((ptf, mut path, mut interact)) = players.single_mut() else {
        return;
    };
    let Ok(player_radius) = sheet.single().map(|(_, cb)| cb.radius) else {
        return;
    };
    let Some(e) = interact.0 else { return };
    let Ok((gt, g)) = graves.get(e) else {
        return;
    };
    let range = gd_game_core::combat::fixed_use_range(
        data.db.get(GRAVE),
        player_radius,
        path.points.is_empty(),
    );
    if ptf.translation.distance(gt.translation()) > range {
        return;
    }
    interact.0 = None;
    path.points.clear();
    let gained = progress.progress.add_experience(
        &rules.0.levels,
        g.redeem.min(u32::MAX as u64) as u32,
        0.0,
    );
    if gained > 0 {
        crate::progress::level_up(
            gained,
            &progress,
            &rules,
            &mut sheet,
            &mut leveled,
            &mut commands,
        );
    }
    info!(
        "death: recovered {} experience{}",
        g.redeem,
        if gained > 0 { " (level up)" } else { "" }
    );
    if let Some(fx) = data
        .db
        .get(GRAVE)
        .and_then(|r| r.str("GraveEffect").map(str::to_owned))
    {
        gd_game_core::visuals::spawn_fx(
            &mut commands,
            &data,
            &fx,
            Transform::from_translation(gt.translation()),
        );
    }
    commands.entity(e).try_despawn();
}

#[cfg(feature = "dev")]
pub fn test_god(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut players: Query<&mut gd_game_core::combat::Combatant, (With<Player>, Without<Dead>)>,
) {
    if let Some(pct) = hooks.gameplay.life_pct {
        for mut cb in &mut players {
            cb.life = cb.profile.sheet.life * pct * 0.01;
        }
        return;
    }
    if !hooks.gameplay.god {
        return;
    }
    for mut cb in &mut players {
        let max = cb.profile.sheet.life;
        if cb.life < max {
            cb.life = max;
        }
    }
}

#[cfg(feature = "dev")]
pub fn test_die(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut commands: Commands,
    time: Res<Time>,
    (mut t, mut fresh): (Local<Option<f32>>, gd_game_core::session::NewSession),
    players: Query<Entity, (With<Player>, Without<Dead>)>,
) {
    if fresh.begun() {
        *t = None;
    }
    let Some(secs) = hooks.gameplay.die else {
        return;
    };
    let Ok(e) = players.single() else { return };
    let left = t.get_or_insert(secs);
    if *left < 0.0 {
        return;
    }
    *left -= time.delta_secs();
    if *left < 0.0 {
        crate::combat::kill_me(&mut commands, e);
    }
}
