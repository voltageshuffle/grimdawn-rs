//! Breakable objects (`Destructible`: crates, barrels, bone piles, urns,
//! breakable doors and walls, the story's levers). They are combat targets with the record's
//! `maxLifeEquation` (`charLevel` = player level) or `maxLife`, and block
//! pathing while intact unless `passableWhen` is "Intact" or "Always"
//! (`Destructible::InitialUpdate` → `SetBlocked(true)` unless
//! `PassabilityFromString` gives Intact/Always; "Never" is unrecognised there, so
//! it blocks like ""): `SetBlocked` → `Entity::SetDynamicObstacle(true)`
//! (Engine.dll) takes, with `useBoundingBoxesForDynamicObstacles`,
//! the mesh hit boxes (`Destructible::CreatePathObstacles`), else
//! `NavManager::AddDynamicObstacle(Entity, 0)`: for an `Actor`, a
//! cylinder of radius max(`GetExtents` = `actorRadius`, 0.5) × |scale vector|
//! (sqrt(3) × scale for a uniform scale) and height `GetHeight` = `actorHeight`
//! × scale y, which the tile cache (`NavMesh::AddObstacleCylinder`) makes h = max(height, 3)
//! from 0.5 h below the origin, 1.5 h tall. So a wooden door (`actorRadius`
//! 0.8) blocks 1.39 m around its centre, its whole doorway. Broken, they vanish at once: the
//! `brokenMesh` pieces (one root bone each) fly apart, lie for `rubbleLife`
//! seconds and fade over `rubbleFade`; `breakEffects` play; with
//! `lootChance` % they drop their `lootTable` and gold like a container
//! (`gd_sim::items::loot`). Broken objects stay broken for the session. A
//! story object's breakable (`crate::story_world`) runs its `onBroken` hook and
//! goes with its story object; a level's goes with its level (`gd_game_core::levels::LevelMember`).
//!
//! `Destructible::TakeAttack`: no defensive ability, dodge or
//! armour; the hit's damage values are summed raw off the life.
//! Loot at the container level (`Destructible::InitialUpdate`
//! runs the same loot generator `LootContainer::GetItemLevel` as chests). The broken
//! pieces' launch, physics and fade are the dressing's ([`Broke`]; grimdawn
//! `breakables.rs`).
use crate::nav::Nav;
use bevy::prelude::*;
use gd_game_core::combat::{Combatant, Dead};
use gd_game_core::creature::BodyCache;
use gd_game_core::data::GameData;
use gd_game_core::rules::CombatRules;
use gd_game_core::world::{Breakable, Broken};
use gd_rules::Rng;

pub struct BreakablesPlugin;

impl Plugin for BreakablesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Broken>()
            .add_message::<Broke>()
            .add_systems(
                Update,
                (
                    break_objects.in_set(BreakableStage::Break),
                    crate::levels::cull_level_members,
                    test_break,
                ),
            );
    }
}

/// A breakable broke (the game throws its broken pieces and plays its
/// break effects): its record and transform, `Destructible::BreakApart`'s direction and force, and the
/// story object it is (`crate::story_world`: its `onBroken` hook). A killing blow's break
/// (`Destructible::TakeAttack`) passes the direction from its attacker (any caster; the player when it is
/// gone) to the object, 0.5 m up its centre, straight up without one, and force 1; a script's
/// `Destructible:Destroy(dir, force, bool)` passes its own.
#[derive(Message, Clone, Debug)]
pub struct Broke {
    pub record: String,
    pub transform: Transform,
    pub dir: Vec3,
    pub force: f32,
    pub story: Option<u32>,
}

/// Spawns a breakable (intact, or already broken this session).
#[allow(clippy::too_many_arguments)]
pub fn spawn_breakable(
    commands: &mut Commands,
    data: &GameData,
    bodies: &mut BodyCache,
    rules: &CombatRules,
    broken: &Broken,
    tf: Transform,
    record: &str,
    key: (usize, u32),
    player_level: i32,
) -> Option<Entity> {
    if broken.0.contains(&key) {
        return None;
    }
    let rec = data.db.get(record)?;
    let msh = bodies.get(data, rec.str("mesh")?)?;
    let e = commands
        .spawn((
            tf,
            GlobalTransform::from(tf),
            Name::new(format!(
                "breakable {}",
                record.rsplit('/').next().unwrap_or("")
            )),
        ))
        .id();
    // (`usize::MAX`: an on-death drop or script object stays; a story object's leaves `crate::story_world`'s way.)
    if key.0 != usize::MAX {
        commands
            .entity(e)
            .insert(gd_game_core::levels::LevelMember(key.0));
    }
    let life = rec
        .str("maxLifeEquation")
        .and_then(|eq| gd_rules::expr::Expr::parse(eq).ok())
        .map(|e| {
            e.eval(&|n| {
                n.eq_ignore_ascii_case("charLevel")
                    .then_some(player_level as f32)
            })
        })
        .or(rec.f32("maxLife"))
        .or(rec.f32("life"))
        .unwrap_or(1.0)
        .max(1.0);
    // Only an intact blocking one is an obstacle (module doc).
    let passable = rec
        .str("passableWhen")
        .is_some_and(|p| p.eq_ignore_ascii_case("Intact") || p.eq_ignore_ascii_case("Always"));
    let o = if passable {
        Vec::new()
    } else if rec.bool("useBoundingBoxesForDynamicObstacles") {
        crate::obstacles::hit_box_obstacles(&msh, &tf)
    } else {
        // `NavManager::AddDynamicObstacle(Entity)` / `NavMesh::AddObstacleCylinder` (module doc).
        let radius = rec
            .f32("actorRadius")
            .unwrap_or(gd_game_core::combat::ACTOR_RADIUS)
            .max(0.5)
            * tf.scale.length();
        let h = (rec.f32("actorHeight").unwrap_or(0.0) * tf.scale.y).max(3.0);
        vec![gd_sim::nav::Obstacle::Cylinder {
            base: (tf.translation - Vec3::Y * (0.5 * h)).to_array(),
            radius,
            height: 1.5 * h,
        }]
    };
    if !o.is_empty() {
        commands.entity(e).insert(crate::nav::NavObstacle(o));
    }
    if let Some(mut p) =
        gd_sim::actor::monster(&data.db, &rules.0, record, 1, gd_sim::actor::Side::Monster)
    {
        p.sheet.life = life;
        p.sheet.defensive_ability = 0.0;
        p.sheet.dodge = 0.0;
        p.protection = 0.0;
        p.armor_regions.clear();
        commands.entity(e).insert((
            Breakable {
                record: record.to_owned(),
                key,
                targetable: rec.bool("targetable"),
            },
            Combatant {
                bounds: gd_sim::bounds::Aabb::object(&msh, 1.0),
                ..Combatant::new(
                    std::sync::Arc::new(p),
                    gd_game_core::combat::get_radius(Some(rec), gd_game_core::combat::ACTOR_RADIUS),
                )
            },
        ));
    }
    Some(e)
}

#[allow(clippy::too_many_arguments)]
pub fn break_objects(
    mut commands: Commands,
    q: Query<
        (
            Entity,
            &Transform,
            &Breakable,
            Option<&crate::combat::KilledBy>,
            Option<&gd_game_core::story::StoryActor>,
        ),
        Added<Dead>,
    >,
    places: Query<&Transform, Without<Breakable>>,
    mut broken: ResMut<Broken>,
    // Option: level 1 when absent.
    progress: Option<Res<gd_game_core::progress::PlayerProgress>>,
    world: Res<gd_game_core::levels::Levels>,
    nav: Res<Nav>,
    data: Res<GameData>,
    mut broke: MessageWriter<Broke>,
    players: Query<&Transform, With<gd_game_core::player::Player>>,
    mut loot: crate::loot::ContainerLoot,
    mut bodies: ResMut<BodyCache>,
) {
    let player = players.single().ok().map(|p| p.translation);
    for (e, tf, b, by, story) in &q {
        // The killing blow's attacker (any caster: `combat::Side::hits`),
        // else the player.
        let dir = by
            .and_then(|k| places.get(k.0).ok())
            .map(|t| t.translation)
            .or(player)
            .and_then(|a| (tf.translation + Vec3::Y * 0.5 - a).try_normalize())
            .unwrap_or(Vec3::Y);
        broken.0.insert(b.key);
        let Some(rec) = data.db.get(&b.record) else {
            continue;
        };
        // Swap to the broken mesh: a fresh entity, so the combat systems
        // forget the old one.
        commands.entity(e).try_despawn();
        broke.write(Broke {
            record: b.record.clone(),
            transform: *tf,
            dir,
            force: 1.0,
            story: story.map(|a| a.0),
        });
        let mut rng = Rng(gd_game_core::rng::u32(1..0x7FFF_FFFF));
        let player_level = progress.as_ref().map_or(1, |p| p.progress.level);
        let difficulty = loot.session.mode().difficulty;
        let at = tf.translation;
        let limits = world.level_limits(Some(b.key.0), at, difficulty);
        let level = gd_sim::items::loot::container_level(rec, player_level, difficulty, limits, 0);
        let idb = &data.item_db();
        let drops = if rng.unit() * 100.0 < rec.f32("lootChance").unwrap_or(0.0) {
            loot.drops(
                idb,
                &b.record,
                level,
                player_level as u32,
                Some(at),
                &mut rng,
            )
        } else {
            Vec::new()
        };
        let gold = rec
            .str("goldGenerator")
            .map(|g| {
                gd_sim::progress::gold_drop(
                    &data.db,
                    g,
                    rec.f32("goldGeneratorChance").unwrap_or(0.0),
                    level as i32,
                    &mut rng,
                )
            })
            .unwrap_or_default();
        debug!(
            "broke {} at {at:?} (level {level}): {} items, gold {gold:?}",
            b.record.rsplit('/').next().unwrap_or(""),
            drops.len()
        );
        // `Destructible::GetLootDropCoords` / `GetLootDropRadius` (`crate::loot` doc).
        let body = rec.str("mesh").and_then(|m| bodies.get(&data, m));
        let drop = crate::loot::fixed_drop_point(&nav, at, tf.rotation, body.as_deref());
        crate::loot::drop_loot_by(
            &mut commands,
            idb,
            drop,
            drops.iter().map(|d| d.to_item()),
            &gold,
            crate::loot::Dropper::all_at_once(
                rec.f32("actorRadius")
                    .unwrap_or(gd_game_core::combat::ACTOR_RADIUS),
            ),
        );
    }
}

/// `GD_BREAK` (hooks.rs).
pub fn test_break(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut commands: Commands,
    time: Res<Time>,
    mut timer: Local<f32>,
    mut players: Query<
        (
            &Transform,
            &mut Combatant,
            &mut crate::player_orders::PrimaryUse,
        ),
        (With<gd_game_core::player::Player>, Without<Breakable>),
    >,
    q: Query<(Entity, &GlobalTransform), (With<Breakable>, With<Combatant>, Without<Dead>)>,
) {
    let Some(mode) = hooks.gameplay.break_mode.clone() else {
        return;
    };
    let Ok((tf, mut cb, mut primary)) = players.single_mut() else {
        return;
    };
    let nearest = q
        .iter()
        .map(|(e, gt)| (e, gt.translation().distance(tf.translation)))
        .filter(|(_, d)| *d < 20.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(e, _)| e);
    if mode == "now" {
        *timer -= time.delta_secs();
        if *timer <= 0.0
            && let Some(e) = nearest
        {
            *timer = 3.0;
            crate::combat::kill_me(&mut commands, e);
        }
    } else if cb.target.is_none() {
        // Attacked as with the button held (`player_orders::PrimaryUse`).
        cb.target = nearest;
        primary.0 = nearest.is_some();
    }
}

/// Breakables breaking: what reacts to a break orders after `Break`.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BreakableStage {
    Break,
}
