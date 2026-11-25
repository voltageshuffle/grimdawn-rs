//! `Skill_AttackPattern` (Fangs of Asterkarn bosses and nemeses): the skill marks spots on the ground in a
//! pattern around a centre; each spot goes off after its own delay by running the skill's secondaries there.
//!
//! Game.dll (placement rules: `gd_sim::pattern`):
//! - `ActivateNow`: the centre is the caster with `selfTarget`, else the target object, else the skill's
//!   point; the orientation stays the caster's. Each spot (`Pick…Points`) is put on the floor and sent to
//!   `SkillSpawnObject`.
//! - `SkillSpawnObject`: per spot the `groundEffect` entity is created and starts emitting, with a delay of
//!   `minDelay + r % (maxDelay − minDelay + 1)` ms (at least 1), r the Park–Miller stream of the spawn's
//!   seed, stepped once per spot.
//! - `Update`: at delay ≤ 0 the effect entity, if still in the world, runs `ActivateSecondarySkills` with
//!   no target at its coordinates, then removes itself.
//!
//! The nav's blocking requests (`NavManager::RequestAndBlock…`) are `gd_sim::pattern::NavBlock`, circles
//! read only while picking one activation's spots. PORT: the on-radius request's `needPath` / sector tests
//! are not made; the walkable-ground probe is the nav grid height.
use crate::nav::Nav;
use bevy::prelude::*;
use gd_game_core::data::GameData;
use gd_sim::pattern::{NavBlock, Params, Request};

pub struct PatternsPlugin;

impl Plugin for PatternsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Fire>().add_systems(
            Update,
            (fire, run_spots)
                .chain()
                .after(crate::casting::CastingSet)
                .before(gd_game_core::combat::CombatSet),
        );
    }
}

/// A `Skill_AttackPattern` cast went off.
#[derive(Message, Clone, Debug)]
pub struct Fire {
    pub caster: Entity,
    pub skill: String,
    pub level_index: usize,
    pub point: Vec3,
    pub target: Option<Entity>,
}

/// One marked spot (a `groundEffect` entity).
#[derive(Component, Clone, Debug)]
pub struct PatternSpot {
    pub caster: Entity,
    pub skill: String,
    pub level_index: usize,
    /// Seconds until it goes off.
    pub left: f32,
    fx: Option<Entity>,
}

fn fire(
    mut commands: Commands,
    mut msgs: MessageReader<Fire>,
    data: Res<GameData>,
    nav: Res<Nav>,
    at: Query<&Transform>,
) {
    for m in msgs.read() {
        let (Some(rec), Ok(ctf)) = (data.db.get(&m.skill), at.get(m.caster)) else {
            continue;
        };
        let centre = if rec.bool("selfTarget") {
            ctf.translation
        } else {
            m.target
                .and_then(|t| at.get(t).ok())
                .map_or(m.point, |t| t.translation)
        };
        let fwd = gd_game_core::character::facing_flat(ctf);
        let p = Params::from_record(rec, m.level_index + 1);
        let ground = |x: f32, z: f32, y: f32| nav.0.ground([x, y + 1.0, z]).unwrap_or(y);
        let mut block = NavBlock::default();
        let mut gnd = |x: f32, z: f32| nav.0.ground([x, centre.y + 1.0, z]);
        let mut rand = gd_game_core::rng::f32;
        let c = (centre.x, centre.z);
        let offsets = gd_sim::pattern::spots(
            &p,
            (fwd.x, fwd.z),
            |lo, hi| {
                if hi <= lo {
                    lo
                } else {
                    gd_game_core::rng::i32(lo..=hi)
                }
            },
            |r| {
                let got = match r {
                    Request::Near { x, z, spacing } => {
                        block.near_point((c.0 + x, c.1 + z), spacing, spacing, &mut rand, &mut gnd)
                    }
                    Request::OnRadius { radius, spacing } => {
                        block.random_on_radius(c, radius, spacing, &mut rand, &mut gnd)
                    }
                    Request::InRadius { radius, spacing } => {
                        block.random_in_radius(c, radius, spacing, &mut rand, &mut gnd)
                    }
                };
                got.map(|p| (p[0] - c.0, p[2] - c.1))
            },
        );
        let min = (rec.f32("minDelay").unwrap_or(0.0) * 1000.0) as i32;
        let max = (rec.f32("maxDelay").unwrap_or(0.0) * 1000.0) as i32;
        let span = if max - min == -1 {
            1
        } else {
            (max - min + 1) as u32
        };
        let mut rng = gd_rules::Rng(gd_game_core::rng::u32(1..0x7FFF_FFFF));
        rng.next();
        let fx = rec
            .str("groundEffect")
            .filter(|f| !f.is_empty())
            .map(str::to_owned);
        debug!(
            "pattern {}: {} spots ({:?})",
            m.skill.rsplit('/').next().unwrap_or(""),
            offsets.len(),
            p.pattern
        );
        for (x, z) in offsets {
            let (px, pz) = (centre.x + x, centre.z + z);
            let spot = Vec3::new(px, ground(px, pz, centre.y), pz);
            let delay = (min + (rng.next() % span.max(1)) as i32).max(1);
            let tf = Transform::from_translation(spot);
            let fx = fx
                .as_deref()
                .and_then(|f| crate::casting::spawn_fx_looping(&mut commands, &data, f, tf));
            commands.spawn((
                PatternSpot {
                    caster: m.caster,
                    skill: m.skill.clone(),
                    level_index: m.level_index,
                    left: delay as f32 / 1000.0,
                    fx,
                },
                tf,
            ));
        }
    }
}

fn run_spots(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &Transform, &mut PatternSpot)>,
    mut activations: MessageWriter<crate::secondary::Activate>,
) {
    let dt = time.delta_secs();
    for (e, tf, mut s) in &mut q {
        s.left -= dt;
        if s.left > 0.0 {
            continue;
        }
        activations.write(crate::secondary::Activate {
            caster: s.caster,
            skill: s.skill.clone(),
            targets: Vec::new(),
            point: tf.translation,
            mode: crate::secondary::Mode::Normal,
            level_index: s.level_index,
            replica: false,
        });
        if let Some(fx) = s.fx {
            commands.entity(fx).try_despawn();
        }
        commands.entity(e).try_despawn();
    }
}
