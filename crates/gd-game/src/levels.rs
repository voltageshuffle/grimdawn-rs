//! The level-streaming despawn systems. Streaming never despawns in `Update`: unloaded levels' entities get
//! [`Unload`] and [`despawn_unloaded`] despawns them in `Last`, so inserts other systems queued for them that
//! frame still apply.
use bevy::prelude::*;
use gd_game_core::levels::{LevelMember, Levels, Unload};

/// `Last`: despawns the [`Unload`]ed entities.
pub fn despawn_unloaded(mut commands: Commands, q: Query<Entity, With<Unload>>) {
    for e in &q {
        commands.entity(e).try_despawn();
    }
}

/// Marks the [`LevelMember`]s of unloaded levels [`Unload`]: all of them when the loaded levels changed, else
/// only those added since the last run.
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

/// `Last`: the loose world entities that appeared this frame take the level they stand in. `Last`, so the
/// parenting queued with a spawn has been applied: an effect that follows its owner is the owner's.
#[allow(clippy::type_complexity)]
pub fn place_loose(
    mut commands: Commands,
    levels: Res<Levels>,
    // Option: no map is open (headless tests, the menu): nothing is placed.
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
