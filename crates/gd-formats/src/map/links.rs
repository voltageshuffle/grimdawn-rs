//! Which map regions are connected (`World::GetConnectedRegionSet` in Engine.dll).
//!
//! The links are not stored in the map file. `World::Load`
//! builds them (`World::BuildConnectivity`, per pair `Region::BuildRegionConnectivity`): region
//! B is a neighbour of A when on every axis (Y included)
//! `|cA - (cB + (oB - oA))| <= hB + hA + 2.0` (c the box centre, h the half extents, o the integer world offset:
//! boxes in world space that overlap or touch with 2 m of slack); symmetric. `GetConnectedRegionSet` adds the
//! region and its neighbours and expands the frontier one hop at a time up to a hop limit (50 in
//! `EndlessDungeon_Generator::PickExits`). PORT: the box is the map file's (the engine's `UpdateBoundingBox` may
//! replace it from a loaded level).
use super::RegionInfo;

/// The slack added to the box overlap test (m).
pub const SLACK: f32 = 2.0;

/// The neighbour lists of all regions (`Region.neighbours`): O(n^2) over the regions, about 4 million box tests
/// for the world map.
pub fn neighbours(regions: &[RegionInfo]) -> Vec<Vec<usize>> {
    let world = |r: &RegionInfo| -> ([f32; 3], [f32; 3]) {
        (
            [0, 1, 2].map(|i| r.bbox.center[i] + r.position[i] as f32),
            r.bbox.half,
        )
    };
    let boxes: Vec<_> = regions.iter().map(world).collect();
    let mut out = vec![Vec::new(); regions.len()];
    for a in 0..boxes.len() {
        for b in (a + 1)..boxes.len() {
            let ((ca, ha), (cb, hb)) = (boxes[a], boxes[b]);
            if (0..3).all(|i| (ca[i] - cb[i]).abs() <= ha[i] + hb[i] + SLACK) {
                out[a].push(b);
                out[b].push(a);
            }
        }
    }
    out
}

/// `GetConnectedRegionSet(region, set, depth)`: the region and the regions up to `depth` hops from it, in
/// discovery order.
pub fn connected_set(neighbours: &[Vec<usize>], region: usize, depth: usize) -> Vec<usize> {
    let mut seen = vec![false; neighbours.len()];
    let mut set = Vec::new();
    let mut frontier = Vec::new();
    if region < seen.len() {
        seen[region] = true;
        set.push(region);
        frontier.push(region);
    }
    for _ in 0..depth {
        let mut next = Vec::new();
        for r in frontier {
            for &n in &neighbours[r] {
                if !seen[n] {
                    seen[n] = true;
                    set.push(n);
                    next.push(n);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    set
}
