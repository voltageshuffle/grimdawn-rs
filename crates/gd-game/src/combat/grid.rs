//! Spatial indices for the systems that scan every combatant (PORT: stands in for the engine's proximity grid, it
//! only narrows the candidates). Each reader builds its own lazily, once per frame, from its own query: positions
//! and query order change between systems. Candidates come in query order and callers keep their engine tests, so
//! the scan finds what the full one did, in the same order (first-found picks, `min_by` ties, RNG draws).

use bevy::prelude::*;
use gd_sim::bounds::Aabb;
use gd_sim::grid::Grid;

/// The grid's cell side (m).
pub const CELL: f32 = 4.0;

/// `range` widened past the float rounding of the callers' tests (`d² ≤ r²`, `meets_sphere`, a ray's entry).
pub fn pad(range: f32) -> f32 {
    let r = range.abs();
    r + r * 1.0e-4 + 1.0e-3
}

pub fn xz(p: Vec3) -> [f32; 2] {
    [p.x, p.z]
}

/// The world boxes of the bodies a system scans (in its query's order), indexed by their centres.
#[derive(Default)]
pub struct BodyGrid {
    grid: Grid,
    boxes: Vec<Aabb>,
    reach: f32,
}

impl BodyGrid {
    pub fn new(boxes: Vec<Aabb>) -> Self {
        let mut g = Self::default();
        g.rebuild(boxes);
        g
    }

    /// Replaces the bodies and re-indexes them, reusing both allocations.
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

    pub fn body(&self, i: usize) -> &Aabb {
        &self.boxes[i]
    }

    /// Fills `out` with the bodies (indices, ascending) whose box may come within `range` of `at` horizontally
    /// (box half extents included).
    pub fn near(&self, at: Vec3, range: f32, out: &mut Vec<usize>) {
        self.grid.near(xz(at), pad(range.abs() + self.reach), out);
    }
}
