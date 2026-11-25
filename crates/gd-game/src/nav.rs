//! The walkable world: nav layers of the loaded levels (the game streams
//! them in with the levels; tests build flat ground with [`flat_layers`]).
use bevy::prelude::*;
use gd_sim::nav::{NavLayer, Obstacle, TILE};

/// Walkable cells of the loaded levels.
#[derive(Resource, Default)]
pub struct Nav(pub gd_sim::nav::NavWorld);

/// The node budget of a character's path search (melee approach, AI moves).
pub const PATH_BUDGET: usize = 20_000;
/// The node budget of a long path search (player orders, return-home walks).
pub const PATH_BUDGET_LONG: usize = 60_000;

/// Flat walkable ground at height `center.y` covering `center` ± `half`
/// (x, z): 0.5 m cells, every side connected.
pub fn flat_layers(center: Vec3, half: f32) -> Vec<NavLayer> {
    let t0 = ((center.x - half) / TILE).floor() as i32;
    let t0z = ((center.z - half) / TILE).floor() as i32;
    let n = (2.0 * half / TILE).ceil() as i32 + 1;
    let (cs, cells) = (0.5f32, (TILE / 0.5) as u32);
    let mut layers = Vec::new();
    for tx in t0..t0 + n {
        for tz in t0z..t0z + n {
            let count = (cells * cells) as usize;
            layers.push(NavLayer {
                tile: (tx, tz),
                bmin: [tx as f32 * TILE, center.y, tz as f32 * TILE],
                cs,
                ch: 0.1,
                w: cells,
                h: cells,
                heights: vec![0; count],
                areas: vec![1; count],
                cons: vec![0x0f; count],
            });
        }
    }
    layers
}

/// Where a teleport (`GameEngine::InstallTeleportActivity`: dungeon entrances, riftgates, portals, the Shattered
/// Realm's doors and gates) puts the player: the caller passes the destination's world position cast to
/// integers (`(int)` per axis, toward zero: `DungeonEntrance::RequestToUse`, `InitiatePlayerTeleport`); the activity
/// (init `TeleportActivity::Initialize`, placement `TeleportLocalClientActivity::PlacePlayer`) makes it region-local,
/// `WorldVec3::TranslateToFloor(0)` →
/// `World::PutOnFloor` drops it onto the first floor a ray from 2.5 m above meets (terrain, grids, water, pathable
/// actors; [`NavWorld::floor_below`] here) and `World::SetCoords` puts the player there, unrotated. No offset, no
/// search for room around it. `None` while the nav there is not loaded. PORT: a point with no walkable cell
/// under it (the engine stands the player on any mesh) takes the nearest one within 2.5 m, as the port's movement
/// needs a nav cell.
///
/// [`NavWorld::floor_below`]: gd_sim::nav::NavWorld::floor_below
pub fn teleport_landing(nav: &gd_sim::nav::NavWorld, dest: Vec3) -> Option<Vec3> {
    let t = Vec3::new(
        dest.x as i32 as f32,
        dest.y as i32 as f32,
        dest.z as i32 as f32,
    );
    if let Some(c) = nav.floor_below(t.to_array(), 2.5) {
        return Some(Vec3::new(t.x, nav.pos(c)[1], t.z));
    }
    nav.nearest((t + Vec3::Y * 2.5).to_array(), 2.5)
        .map(|c| Vec3::from(nav.pos(c)))
}

/// `NavManager::FindStraightMovePointOnSlopes(from, to)`: the farthest point toward `to` that the straight line from
/// `from` reaches over walkable ground. A step a dynamic obstacle covers (a closed door) ends it too: the skills'
/// drop and spawn points call it with its nav probe (`NavManager::ProbeWalkableHeight`), which misses the polygons
/// the tile cache carved around the obstacle, and a plain move or an evade checks its point with
/// `NavManager::FindStraightMovePoint`'s raycast over the same mesh (PORT: the keyboard aim's call, without the
/// probe, stops there too). PORT: the nav ground is walked in 0.25 m steps, stopping at the first step without
/// ground, 1.5 m above / below the last or on a cell whose centre (or the step itself) an obstacle covers
/// ([`NavWorld::set_obstacles`]); `from` itself when the first step fails.
///
/// [`NavWorld::set_obstacles`]: gd_sim::nav::NavWorld::set_obstacles
pub fn straight_move_point(nav: &gd_sim::nav::NavWorld, from: Vec3, to: Vec3) -> Vec3 {
    let flat = (to - from).with_y(0.0);
    let dist = flat.length();
    if dist < 1e-4 {
        return from;
    }
    let mut at = from;
    let steps = (dist / 0.25).ceil() as i32;
    for i in 1..=steps {
        let p = from + flat * (i as f32 / steps as f32);
        match nav.ground([p.x, at.y + 1.0, p.z]) {
            Some(g) if (g - at.y).abs() < 1.5 => {
                let p = Vec3::new(p.x, g, p.z);
                let cell = nav.locate([p.x, g + 0.5, p.z]).map(|c| nav.pos(c));
                if cell.is_none_or(|c| nav.blocked(c) || nav.blocked(p.to_array())) {
                    break;
                }
                at = p;
            }
            _ => break,
        }
    }
    at
}

/// World-space obstacles of this entity: it blocks pathing while it exists. Sources: closed doors and
/// gates and blocking set-piece objects (their mesh hit boxes, like `FixedItem::CreatePathObstacle`),
/// `NavBlocker` entities (cylinders: `radius`, `height`; e.g. the repairable bridges'
/// blockers).
#[derive(Component, Clone)]
pub struct NavObstacle(pub Vec<Obstacle>);

pub fn sync_obstacles(
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
    mut nav: ResMut<Nav>,
    added: Query<(Entity, &NavObstacle), Changed<NavObstacle>>,
    mut removed: RemovedComponents<NavObstacle>,
) {
    for e in removed.read() {
        nav.0.set_obstacles(e.to_bits(), Vec::new());
    }
    // `GD_NO_OBSTACLES=1` (debug): ignore them.
    #[cfg(feature = "dev")]
    if hooks.gameplay.no_obstacles {
        return;
    }
    for (e, o) in &added {
        nav.0.set_obstacles(e.to_bits(), o.0.clone());
    }
}

/// Works out the loaded layers' connectivity ([`NavWorld::settle`]) for at most 0.5 ms a frame until it is known, so
/// a strict path search toward an unreachable goal fails at once.
///
/// [`NavWorld::settle`]: gd_sim::nav::NavWorld::settle
pub fn settle_connectivity(mut nav: ResMut<Nav>) {
    if !nav.0.settled() {
        nav.bypass_change_detection()
            .0
            .settle(std::time::Duration::from_micros(500));
    }
}
