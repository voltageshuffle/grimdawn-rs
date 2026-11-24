//! A uniform grid over horizontal points (x, z) for neighbour queries, built once per frame from a point list
//! (PORT: stands in for the engine's proximity grid; it only narrows the candidates, the callers keep the engine's
//! distance tests). The points are kept as (cell, index) sorted, so a cell's points form one run and so do a
//! column's (one x, the z ascending): a query does one binary search per column it covers.
//!
//! [`Grid::near`] lists, by ascending index (the order the points were given in), every point whose cell meets the
//! square of half side |range| around a position. That is a superset of the points with |dx| ≤ |range| and
//! |dz| ≤ |range|, exactly in float arithmetic (rounding is monotone, so `(x − range) / cell` never lands past
//! a point within range), hence of any test `d² < range²` / `d ≤ range` over computed differences: filtering the
//! candidates with the caller's own test gives the brute-force result, in the brute-force order.

/// A horizontal point (x, z).
pub type V2 = [f32; 2];

/// The grid (module docs).
#[derive(Clone, Debug, Default)]
pub struct Grid {
    cell: f32,
    /// (cell, index), sorted.
    cells: Vec<((i32, i32), usize)>,
}

impl Grid {
    /// A grid of `cell`-wide squares (`cell` > 0) over `points`, indexed in iteration order.
    pub fn new(cell: f32, points: impl IntoIterator<Item = V2>) -> Self {
        let mut g = Grid {
            cell,
            cells: Vec::new(),
        };
        g.rebuild(cell, points);
        g
    }

    /// Replaces the points ([`Self::new`]'s build for the same input), reusing the cell list's allocation:
    /// a per-frame reader keeps its grid and rebuilds it in place.
    pub fn rebuild(&mut self, cell: f32, points: impl IntoIterator<Item = V2>) {
        debug_assert!(cell > 0.0, "grid cell {cell}");
        self.cell = cell;
        self.cells.clear();
        self.cells.extend(
            points
                .into_iter()
                .enumerate()
                .map(|(i, p)| (Self::key(cell, p), i)),
        );
        self.cells.sort_unstable();
    }

    fn col(cell: f32, x: f32) -> i32 {
        // (`as` saturates, and NaN goes to 0.)
        (x / cell).floor() as i32
    }

    fn key(cell: f32, p: V2) -> (i32, i32) {
        (Self::col(cell, p[0]), Self::col(cell, p[1]))
    }

    /// Fills `out` with the candidate neighbours of `pos` within `range` (module docs), by ascending index.
    pub fn near(&self, pos: V2, range: f32, out: &mut Vec<usize>) {
        out.clear();
        let (Some(first), Some(last)) = (self.cells.first(), self.cells.last()) else {
            return;
        };
        let range = range.abs();
        // Only the columns that hold points: a huge range does not walk empty ones.
        let x0 = Self::col(self.cell, pos[0] - range).max(first.0.0);
        let x1 = Self::col(self.cell, pos[0] + range).min(last.0.0);
        let (z0, z1) = (
            Self::col(self.cell, pos[1] - range),
            Self::col(self.cell, pos[1] + range),
        );
        for x in x0..=x1 {
            let from = self.cells.partition_point(|c| c.0 < (x, z0));
            out.extend(
                self.cells[from..]
                    .iter()
                    .take_while(|c| c.0 <= (x, z1))
                    .map(|c| c.1),
            );
        }
        out.sort_unstable();
    }
}
