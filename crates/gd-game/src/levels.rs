//! The level-streaming despawn systems (the loaded-levels data is
//! [`gd_game_core::levels`]).
//!
//! Streaming never despawns in `Update`: an unloaded level's root, the
//! monsters / NPCs / props of unloaded levels ([`crate::monsters::cull_monsters`]),
//! the chests, breakables, doors and physics props of unloaded levels ([`LevelMember`],
//! [`cull_level_members`]) and story actors whose objects left the world get [`Unload`], and
//! [`despawn_unloaded`] despawns them in `Last`, so inserts other systems
//! queued for them that frame still apply.
use bevy::prelude::*;
use gd_game_core::levels::{LevelMember, Levels, Unload};

/// `Last`: despawns the [`Unload`]ed entities, after every command of the
/// frame's `Update` / `PostUpdate` systems has been applied.
pub fn despawn_unloaded(mut commands: Commands, q: Query<Entity, With<Unload>>) {
    for e in &q {
        commands.entity(e).try_despawn();
    }
}

/// Marks the [`LevelMember`]s of unloaded levels [`Unload`]: all of them when the loaded levels changed, else only
/// the members added since the last run (spawned for a level already gone).
pub fn cull_level_members(
    mut commands: Commands,
    world: Res<Levels>,
    all: Query<(Entity, &LevelMember), Without<Unload>>,
    added: Query<(Entity, &LevelMember), (Added<LevelMember>, Without<Unload>)>,
) {
    let mut cull = |(e, m): (Entity, &LevelMember)| {
        if !world.loaded.contains_key(&m.0) {
            commands.entity(e).try_insert(Unload);
        }
    };
    if world.is_changed() {
        all.iter().for_each(&mut cull);
    } else {
        added.iter().for_each(&mut cull);
    }
}

/// `Last`: the loose world entities that appeared this frame take the level they stand in (module doc): top-level
/// ground items, effects, containers and exit gates that are no level's yet and no story actor (the story streams
/// those, `crate::story_world`). `Last`, so the parenting queued with a spawn has been applied: an effect that follows
/// its owner as a child is the owner's.
#[allow(clippy::type_complexity)]
pub fn place_loose(
    mut commands: Commands,
    levels: Res<Levels>,
    // Option: no map is open (headless tests, the menu): nothing is placed, and what appeared meanwhile stays
    // unplaced (a run condition would place it late, once a map opens).
    layout: Option<Res<gd_game_core::save::MapLayout>>,
    new: Query<
        (Entity, &Transform),
        (
            Without<ChildOf>,
            Without<crate::monsters::LevelProp>,
            Without<gd_game_core::story::StoryActor>,
            Or<(
                Added<crate::loot::GroundItem>,
                Added<gd_game_core::visuals::EffectSpec>,
                Added<gd_game_core::world::Breakable>,
                Added<crate::chests::Chest>,
                Added<crate::endless::EndlessGate>,
            )>,
        ),
    >,
) {
    let Some(layout) = layout else { return };
    for (e, tf) in &new {
        if let Some(level) = levels.level_at(&layout.0, tf.translation) {
            commands
                .entity(e)
                .try_insert(crate::monsters::LevelProp(level));
        }
    }
}
