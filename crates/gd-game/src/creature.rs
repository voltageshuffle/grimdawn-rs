//! The creature spawning systems (the creature data is [`gd_game_core::creature`]).
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use gd_game_core::character::{Character, character_for};
use gd_game_core::creature::Slot;
use gd_game_core::creature::{
    BodyCache, BodyRequest, CreatureLook, creature_look, creature_mesh, record_scale,
};
use gd_game_core::data::GameData;
use std::sync::Arc;

/// Files the meshes [`BodyCache::ready`] parsed in the background.
pub fn collect_body_loads(mut bodies: ResMut<BodyCache>) {
    use bevy::tasks::{block_on, futures_lite::future};
    let bodies = &mut *bodies;
    let mut i = 0;
    while i < bodies.loading.len() {
        let Some((key, msh)) = block_on(future::poll_once(&mut bodies.loading[i])) else {
            i += 1;
            continue;
        };
        drop(bodies.loading.swap_remove(i));
        bodies.in_flight.remove(&key);
        // A spawn that could not wait may have parsed it meanwhile: keep that one.
        if bodies.cache.get(&key).is_none() {
            bodies.cache.put(&key, msh);
        }
    }
}

/// Spawns a creature of `record` (`variant`, see [`creature_look`]) at `transform`, scaled by
/// `placed` (a level placement's scale) else by the record's.
pub fn spawn_creature(
    commands: &mut Commands,
    data: &GameData,
    bodies: &mut BodyCache,
    transform: Transform,
    record: &str,
    variant: u32,
    placed: Option<f32>,
) -> Option<(Entity, CreatureLook)> {
    let rec = data.db.get(record)?;
    let look = creature_look(&data.db, record, variant)?;
    let body = bodies.get(data, &look.mesh)?;
    let scale = placed.unwrap_or_else(|| record_scale(rec));
    let tf = transform.with_scale(Vec3::splat(scale));
    let e = commands
        .spawn((tf, GlobalTransform::from(tf), Name::new(record.to_owned())))
        .id();
    let (right, left) = look.weapons(&data.db);
    let character: Character = character_for(
        data,
        record,
        rec,
        body,
        right,
        left,
        false,
        scale,
        e.to_bits(),
    );
    let gear = crate::combat_sounds::Gear {
        right: look.items.get(&Slot::RightHand).cloned(),
        left: look.items.get(&Slot::LeftHand).cloned(),
        chest: look.items.get(&Slot::Chest).cloned(),
    };
    commands.entity(e).insert((
        character,
        BodyRequest {
            record: record.to_owned(),
            look: look.clone(),
        },
        gear,
    ));
    if rec.bool("hiddenFromCombat") {
        commands.entity(e).insert(crate::combat::HiddenFromCombat);
    }
    if rec.bool("invincible") {
        commands.entity(e).insert(crate::combat::Invincible);
    }
    if rec.str("description") == Some(gd_game_core::world::DUMMY_TAG) {
        commands.entity(e).insert(gd_game_core::world::Dummy);
    }
    if let Some(r) = look
        .items
        .get(&Slot::RightHand)
        .and_then(|w| gd_game_core::casting::ranged_weapon(data, w))
    {
        commands.entity(e).insert(r);
    }
    Some((e, look))
}

#[derive(SystemParam)]
pub struct Spawner<'w> {
    pub data: Res<'w, GameData>,
    pub bodies: ResMut<'w, BodyCache>,
}

/// How many monsters a deferred spawner spawns a frame at most.
pub const SPAWNS_PER_FRAME: usize = 8;

/// How long a deferred spawner works out new records' clip lists a frame.
pub const CLIPS_BUDGET: std::time::Duration = std::time::Duration::from_millis(3);

/// Each record's clip paths by variant ([`gd_game_core::character::creature_clips`]).
pub type ClipLists =
    gd_formats::collections::HashMap<String, gd_formats::collections::HashMap<u32, Arc<[String]>>>;

/// Whether a creature of `record` / `variant` spawns without loading on the game thread (each
/// `ready` starts its background load otherwise). Past `deadline` a record whose clip list is not
/// known yet waits for a later frame.
pub fn ready_to_spawn(
    s: &mut Spawner,
    anims: &mut gd_game_core::character::AnimCache,
    lists: &mut ClipLists,
    record: &str,
    variant: u32,
    deadline: std::time::Instant,
) -> bool {
    let mut ready = true;
    if let Some(mesh) = creature_mesh(&s.data.db, record, variant) {
        ready &= s.bodies.ready(&s.data, &mesh);
    }
    let paths = match lists.get(record).and_then(|v| v.get(&variant)) {
        Some(p) => p.clone(),
        None if std::time::Instant::now() >= deadline => return false,
        None => {
            let p: Arc<[String]> =
                gd_game_core::character::creature_clips(&s.data, record, variant).into();
            lists
                .entry(record.to_owned())
                .or_default()
                .insert(variant, p.clone());
            p
        }
    };
    for p in paths.iter() {
        ready &= anims.ready(&s.data, p);
    }
    ready
}

pub fn spawn(
    commands: &mut Commands,
    s: &mut Spawner,
    transform: Transform,
    record: &str,
    variant: u32,
) -> Option<Entity> {
    spawn_placed(commands, s, transform, record, variant, None)
}

/// [`spawn`] at a level placement's scale `placed`.
pub fn spawn_placed(
    commands: &mut Commands,
    s: &mut Spawner,
    transform: Transform,
    record: &str,
    variant: u32,
    placed: Option<f32>,
) -> Option<Entity> {
    spawn_creature(
        commands,
        &s.data,
        &mut s.bodies,
        transform,
        record,
        variant,
        placed,
    )
    .map(|x| x.0)
}
