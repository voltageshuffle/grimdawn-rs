//! `Skill_CharonGeysers`: a `Skill_AttackProjectileAreaEffect` that goes off not at its target
//! but at the `CharonGeyserMarker` objects placed around the boss's arena.
//!
//! `ActivateNow`: with a projectile launcher, the markers within 45 m of the caster;
//! `numGeysersToFire` (default 0) < 0 fires them all, else min(n, found): with `findClosest` the
//! n nearest, else n drawn without replacement (`rand() % left`). Each launches the skill's area
//! effect at the marker (`ClearHitIteration` between them, so each area hits on its own) for
//! `skillActiveDuration`, radius `projectileExplosionRadius`.
//!
//! The markers ([`GeyserMarker`]) stream in with their level; outside the editor they draw
//! nothing. PORT: the random draw uses the port's RNG.
use bevy::prelude::*;
use gd_game_core::casting::Casting;
use gd_game_core::data::GameData;

pub struct GeysersPlugin;

impl Plugin for GeysersPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Fire>().add_systems(
            Update,
            fire.after(crate::casting::CastingSet)
                .before(gd_game_core::combat::CombatSet),
        );
    }
}

/// `ActivateNow`'s search radius around the caster (m).
pub const SEARCH_RADIUS: f32 = 45.0;

#[derive(Component, Clone, Debug)]
pub struct GeyserMarker;

/// A `Skill_CharonGeysers` cast went off.
#[derive(Message, Clone)]
pub struct Fire {
    pub caster: Entity,
    pub cast: Casting,
    /// `skillActiveDuration` (s) and `projectileExplosionRadius` (m).
    pub duration: f32,
    pub radius: f32,
}

/// Which of `found` (distance to the caster, position) fire: all for a negative `n`, else the
/// `n` nearest (`closest`) or `n` drawn at random.
pub fn pick(
    mut found: Vec<(f32, Vec3)>,
    n: i32,
    closest: bool,
    mut draw: impl FnMut(usize) -> usize,
) -> Vec<Vec3> {
    if n < 0 {
        return found.into_iter().map(|f| f.1).collect();
    }
    let n = (n as usize).min(found.len());
    if closest {
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        found.truncate(n);
        return found.into_iter().map(|f| f.1).collect();
    }
    (0..n).map(|_| found.remove(draw(found.len())).1).collect()
}

fn fire(
    mut commands: Commands,
    mut msgs: MessageReader<Fire>,
    data: Res<GameData>,
    casters: Query<&Transform>,
    markers: Query<&Transform, With<GeyserMarker>>,
) {
    for m in msgs.read() {
        let (Ok(at), Some(rec)) = (casters.get(m.caster), data.db.get(&m.cast.skill)) else {
            continue;
        };
        let found: Vec<(f32, Vec3)> = markers
            .iter()
            .map(|t| (t.translation.distance(at.translation), t.translation))
            .filter(|(d, _)| *d <= SEARCH_RADIUS)
            .collect();
        let n = rec.i32("numGeysersToFire").unwrap_or(0);
        let points = pick(found, n, rec.bool("findClosest"), |len| {
            gd_game_core::rng::u32(..len as u32) as usize
        });
        debug!(
            "geysers {}: {} of the markers",
            m.cast.skill.rsplit('/').next().unwrap_or(""),
            points.len()
        );
        for p in points {
            let cast = Casting {
                point: p,
                target: None,
                ..m.cast.clone()
            };
            crate::casting::spawn_area(
                &mut commands,
                &data,
                rec,
                cast,
                m.caster,
                m.duration,
                m.radius,
            );
        }
    }
}
