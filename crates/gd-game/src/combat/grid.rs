//! Spatial indices for the systems that scan every combatant (`gd_sim::grid::Grid`; PORT: stands in for the engine's
//! proximity grid, it only narrows the candidates). Each reader builds its own once per frame, from its own query
//! at its own run, lazily (on the first query that needs it): positions and query order change between systems
//! (charges, leaps and the monster AI move bodies; inserted components reorder archetypes), so a shared index built
//! earlier in the frame could miss a body or reorder the scan. The candidates come in the index order (the
//! query's) and the callers keep their own engine tests, so the scan finds what the full one did, in the same order
//! (first-found picks, `min_by` ties, RNG draws over the target list are unchanged).

use bevy::prelude::*;
use gd_sim::bounds::Aabb;
use gd_sim::grid::Grid;

/// The grid's cell side (m): about a body and a melee reach; the radius queries (explosions, auras, sight) cover a
/// few cells.
pub const CELL: f32 = 4.0;

/// `range` widened past the float rounding of the callers' tests (`d² ≤ r²`, a box's `meets_sphere`, a ray's
/// entry): a body those find lies within it on both horizontal axes.
pub fn pad(range: f32) -> f32 {
    let r = range.abs();
    r + r * 1.0e-4 + 1.0e-3
}

/// The horizontal point of a position.
pub fn xz(p: Vec3) -> [f32; 2] {
    [p.x, p.z]
}

/// The world boxes of the bodies a system scans ([`Combatant::body_box`] under their transform, in its query's
/// order), indexed by their centres. A box test (`meets_sphere`, `ray_entry`) can only pass within its half extent
/// of the centre: [`BodyGrid::near`] adds the largest one.
#[derive(Default)]
pub struct BodyGrid {
    grid: Grid,
    boxes: Vec<Aabb>,
    /// The largest horizontal half extent.
    reach: f32,
}

impl BodyGrid {
    pub fn new(boxes: Vec<Aabb>) -> Self {
        let mut g = Self::default();
        g.rebuild(boxes);
        g
    }

    /// Replaces the bodies and re-indexes them ([`Self::new`]'s result for the same boxes), reusing both
    /// allocations: a per-frame reader keeps its grid in a `Local` and rebuilds it in place.
    pub fn rebuild(&mut self, boxes: impl IntoIterator<Item = Aabb>) {
        self.boxes.clear();
        self.boxes.extend(boxes);
        self.reach = self
            .boxes
            .iter()
            .map(|b| b.half[0].max(b.half[2]))
            .fold(0.0, f32::max);
        self.grid
            .rebuild(CELL, self.boxes.iter().map(|b| [b.center[0], b.center[2]]));
    }

    /// The world box of body `i`.
    pub fn body(&self, i: usize) -> &Aabb {
        &self.boxes[i]
    }

    /// Fills `out` with the bodies (indices, ascending) whose box may come within `range` of `at` horizontally: a
    /// superset of those a sphere of radius `range` at `at` meets, or a ray from `at` enters before `range`.
    pub fn near(&self, at: Vec3, range: f32, out: &mut Vec<usize>) {
        self.grid.near(xz(at), pad(range.abs() + self.reach), out);
    }
}
