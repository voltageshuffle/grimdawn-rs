//! Attackable physics props (`PhysicsDecoration`): the reactive metal barrel
//! (`level art/setdress/barrel_metal01_reactive.dbr`) and the four wood pieces
//! (`level art/woodpiece1..4.dbr`) — the only 5 stock records of the class.
//!
//! Game.dll: `TakeAttack` @0x1803cb850 accepts an attack from any
//! `Entity`, switches the prop to physics simulation (vtable +272, arg 2) and
//! stores the direction of the attack's `PhysicsMotion` (`ParametersCombat`
//! +120, what `Skill::InitRagDollData` set: `crate::ragdoll`) — no life is
//! subtracted, the prop is not killed. `PhysicsSetup` @0x1803cb920 builds its
//! rigid body in `PhysicsSystem2` at the entity's coords (+ the region offset):
//! its collision box, the record's `physicsMass` (default 1),
//! `physicsFriction`, `physicsRestitution`. `PhysicsSync` @0x1803cbb10 applies
//! the stored direction × rand(0.4..0.6) as an impulse plus a random angular
//! impulse (rand(−1..1) × rand(0.4..0.6) × 0.025 on each axis), then zeroes it.
//! `UpdateSelf` @0x1803cb780: once hit, a 5000 ms countdown (the ctor's +333)
//! runs, then `GraphicsMeshInstance::BeginFade(255, 255, 255, 3 s)`; when the
//! fade completes the entity is removed and destroyed. The ctor calls
//! `SetCastsShadows(false)` (the record's `castsShadows` is overridden).
//!
//! The props come with their level ([`LevelPhysicsProps`], bucketed by
//! grimdawn `world.rs`) and go with it (`crate::levels::LevelMember`; no saved state; the engine recreates
//! them when the level reloads).
//!
//! PORT: the prop is a life-1 [`Combatant`] — the first hit "kills" it, which
//! tosses the whole mesh ([`PropTossed`]) with that hit's impulse (its stored
//! [`RagdollMotion`] direction, the velocity impulse / mass, the spin the
//! angular impulse over the inertia of the collision box); the class has no
//! `brokenMesh`, so unlike a `Destructible` it does not break into pieces, and
//! later hits find nothing. It blocks no pathing (the engine's prop is a
//! pushable crowd agent, not a static obstacle; the port does not simulate
//! crowd pushing — the prop stands until hit). The dressing
//! (`grimdawn::physics_decoration`) draws the prop and tumbles the tossed
//! mesh on the ragdolls' floor ([`crate::ragdoll::floor`]), then fades it.
use crate::combat::{Combatant, Dead};
use crate::creature::BodyCache;
use crate::data::GameData;
use crate::ragdoll::RagdollMotion;
use crate::rules::CombatRules;
use bevy::prelude::*;

pub struct PhysicsDecorationPlugin;

impl Plugin for PhysicsDecorationPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PropTossed>().add_systems(
            Update,
            (
                spawn_level_props.run_if(resource_exists::<CombatRules>),
                // The killing hit's motion is stored first.
                toss_objects
                    .in_set(PhysicsDecorationStage::Toss)
                    .after(crate::combat::CombatSet)
                    .after(crate::ragdoll::RagdollStage::Motion),
            ),
        );
    }
}

/// A stock physics prop, intact (a combat target; the first hit tosses it).
#[derive(Component)]
pub struct PhysicsDecoration {
    pub record: String,
    /// Its collision box's half extents (`Actor::GetCollisionBox`: the mesh
    /// box × the scale, at least 0.1 m; `gd_sim::drop_body::half_extents`).
    pub half: Vec3,
}

/// A loaded level's `PhysicsDecoration` objects (record, transform), on its
/// root until [`spawn_level_props`] spawns them.
#[derive(Component)]
pub struct LevelPhysicsProps {
    pub level: usize,
    pub props: Vec<(String, Transform)>,
}

/// A prop was hit: the dressing tosses its mesh from `transform` at `vel`
/// (m/s), spinning at `spin` (rad/s per world axis): `PhysicsSync` (module doc).
#[derive(Message, Clone, Debug)]
pub struct PropTossed {
    pub record: String,
    pub transform: Transform,
    pub vel: Vec3,
    pub spin: Vec3,
}

/// Spawns a prop (the `spawn_breakable` shape, without the pathing obstacle
/// and the breakable marker: module doc).
pub fn spawn_physics_decoration(
    commands: &mut Commands,
    data: &GameData,
    bodies: &mut BodyCache,
    rules: &CombatRules,
    tf: Transform,
    record: &str,
    level: usize,
) -> Option<Entity> {
    let rec = data.db.get(record)?;
    let msh = bodies.get(data, rec.str("mesh")?)?;
    let mut p =
        gd_sim::actor::monster(&data.db, &rules.0, record, 1, gd_sim::actor::Side::Monster)?;
    // Any hit activates the physics (TakeAttack subtracts no life).
    p.sheet.life = 1.0;
    p.sheet.defensive_ability = 0.0;
    p.sheet.dodge = 0.0;
    p.protection = 0.0;
    p.armor_regions.clear();
    let (lo, hi) = msh.bounding_box.unwrap_or(([0.0; 3], [0.0; 3]));
    let mesh_half = [0, 1, 2].map(|i| (hi[i] - lo[i]) * 0.5);
    let half = Vec3::from(gd_sim::drop_body::half_extents(
        mesh_half,
        tf.scale.max_element(),
    ));
    Some(
        commands
            .spawn((
                tf,
                GlobalTransform::from(tf),
                PhysicsDecoration {
                    record: record.to_owned(),
                    half,
                },
                crate::levels::LevelMember(level),
                Combatant {
                    bounds: gd_sim::bounds::Aabb::object(&msh, 1.0),
                    ..Combatant::new(
                        std::sync::Arc::new(p),
                        crate::combat::get_radius(Some(rec), 0.4),
                    )
                },
                Name::new(format!(
                    "physics prop {}",
                    record.rsplit('/').next().unwrap_or("")
                )),
            ))
            .id(),
    )
}

/// A level's props (module doc), once the combat rules exist.
fn spawn_level_props(
    mut commands: Commands,
    data: Res<GameData>,
    mut bodies: ResMut<BodyCache>,
    rules: Res<CombatRules>,
    levels: Query<(Entity, &LevelPhysicsProps)>,
) {
    for (root, lp) in &levels {
        commands.entity(root).try_remove::<LevelPhysicsProps>();
        for (record, tf) in &lp.props {
            spawn_physics_decoration(
                &mut commands,
                &data,
                &mut bodies,
                &rules,
                *tf,
                record,
                lp.level,
            );
        }
    }
}

/// The first hit tosses the prop with its impulse (module doc, `PhysicsSync`
/// @0x1803cbb10); the combat systems forget it.
fn toss_objects(
    mut commands: Commands,
    data: Res<GameData>,
    q: Query<
        (
            Entity,
            &Transform,
            &PhysicsDecoration,
            Option<&RagdollMotion>,
        ),
        Added<Dead>,
    >,
    mut tossed: MessageWriter<PropTossed>,
) {
    let r = |a: f32, b: f32| a + crate::rng::f32() * (b - a);
    for (e, tf, p, motion) in &q {
        let mass = data
            .db
            .get(&p.record)
            .and_then(|r| r.f32("physicsMass"))
            .filter(|m| *m > 0.0)
            .unwrap_or(1.0);
        let dir = motion.map_or(Vec3::ZERO, |m| m.dir);
        let vel = dir * r(0.4, 0.6) / mass;
        let k = r(0.4, 0.6) * 0.025;
        let impulse = Vec3::new(r(-1.0, 1.0), r(-1.0, 1.0), r(-1.0, 1.0)) * k;
        // The box's inertia about its axes: m (b² + c²) / 3 of the half extents.
        let h = p.half * p.half;
        let inertia = Vec3::new(h.y + h.z, h.x + h.z, h.x + h.y) * (mass / 3.0);
        tossed.write(PropTossed {
            record: p.record.clone(),
            transform: *tf,
            vel,
            spin: impulse / inertia.max(Vec3::splat(1e-4)),
        });
        commands.entity(e).try_despawn();
    }
}

/// Props tossing: what reacts to a toss orders after `Toss`.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PhysicsDecorationStage {
    Toss,
}
