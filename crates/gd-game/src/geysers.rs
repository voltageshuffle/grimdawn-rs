//! `Skill_CharonGeysers` (the Aetherial Abomination's and Ellena's geysers,
//! the clone's fire geysers, the Messenger's magma, the Malmouth enclave's
//! aether): a `Skill_AttackProjectileAreaEffect` that does not go off at its
//! target but at the `CharonGeyserMarker` objects the level designers placed
//! around the boss's arena.
//!
//! Game.dll:
//! - `LoadResources`: `numGeysersToFire` (int, default 0) and
//!   `findClosest` (default false) on top of `Skill::LoadResources`.
//! - `ActivateNow`: the skill's projectile launcher (none:
//!   nothing happens); `World::GetEntitiesInSphere` of 45 m around the
//!   caster, keeping the `CharonGeyserMarker`s (RTTI); `numGeysersToFire`
//!   < 0 fires them all, else min(n, found): with `findClosest` the n nearest
//!   to the caster (sorted, `Game::FindClosestEntities`), else n drawn at random without
//!   replacement (`rand() % left`, `Game::DrawWithoutReplacement`). Each one launches the
//!   skill's area-effect projectile at the marker's coordinates
//!   (`ClearHitIteration` between them, so each area hits on its own): an
//!   ordinary `ProjectileAreaEffect` for `skillActiveDuration`, radius
//!   `projectileExplosionRadius` (`casting::spawn_area`).
//! - So `numGeysersToFire` missing (0) fires none, and a boss away from its
//!   markers fires none.
//!
//! The markers are level entities (`CharonGeyserMarker` class records, e.g.
//! `ellena_geysers_marker.dbr`), streamed in with their level as
//! [`GeyserMarker`]s (`monsters::populate`). In the game the marker
//! draws nothing: its constructor (an `Actor`) calls
//! `SetVisibility(false)` unless `Engine::IsEditorMode` (Engine.dll, `Engine.editorMode`),
//! and `Load` only forces the editor's default mesh
//! (`System/Meshes/Sphere.msh`) before `Actor::Load` (the record's `mesh`,
//! `aethercrystals01_summon.msh`, and `baseTexture` are the editor's look), so
//! the dressing has nothing to draw. PORT: the draw for the random pick uses
//! the port's RNG.
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

/// A `CharonGeyserMarker` level object.
#[derive(Component, Clone, Debug)]
pub struct GeyserMarker;

/// A `Skill_CharonGeysers` cast went off (`casting::fire_casts`).
#[derive(Message, Clone)]
pub struct Fire {
    pub caster: Entity,
    pub cast: Casting,
    /// `skillActiveDuration` (s) and `projectileExplosionRadius` (m).
    pub duration: f32,
    pub radius: f32,
}

/// Which of `found` (distance to the caster, position) fire: all for a
/// negative `n`, else the `n` nearest (`closest`) or `n` drawn at random.
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
