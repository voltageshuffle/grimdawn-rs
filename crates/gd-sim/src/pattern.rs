//! `Skill_AttackPattern`'s spots (Fangs of Asterkarn bosses: Beronath's
//! lightning grids, Yurra's eruption lines, the summoning rings, …): where
//! around a centre the pattern puts its spots, as offsets (x, z) from the
//! centre. The nav requests the engine makes for them (blocking each spot
//! so the next keeps clear of it) are the caller's ([`Request`]), answered
//! by [`NavBlock`].
//!
//! (Game.dll):
//! - `ResolveEnum_Pattern`: `Grid` 0, `Lines` 1, `Ring` 2,
//!   `Circle` 3, anything else `Ring`.
//! - `Load`: `pattern`, `placementRadius` (R), `spawnRadius`,
//!   `spawnLocations` (a list, one per level: the count is the entry at
//!   min(level, len) − 1), `selfTarget`, `forceCenterPoint`, `randomPoints`,
//!   `alternateSpawn`, `minDelay` / `maxDelay` (s → whole ms),
//!   `groundEffect`.
//! - Spacing s = max(2 × `spawnRadius`, 2) (2 m for the even grid).
//! - `PickGridPoints`: random: the grid of step s over
//!   [−R, R]² (rows of z, x inside) inside the disk of R; `forceCenterPoint`
//!   first requests the centre (one fewer); then, until the count is met or
//!   no candidate is left, a uniform candidate (`IGenerate(0, left − 1)`,
//!   swap-removed) is requested near (s). Even: k = ⌈√count⌉, step 2R / k,
//!   every grid point of [−R, R]² (both ends) in a list from which the 2nd
//!   ones are taken (index 0, 2, 4, …; from 1 with `alternateSpawn`), each
//!   requested near (2 m); `forceCenterPoint` then moves the spot nearest the
//!   centre onto it and every other spot by the same offset.
//! - `PickLinePoints`: four arms at 0°, 90°, 180°, 270° about
//!   +Y from the centre's facing (45° on with `alternateSpawn`);
//!   `forceCenterPoint` adds the centre first (one fewer). Even: ⌊count / 4⌋
//!   per arm; random: arms 1–3 take `IGenerate(1, count − 4)` (at least 1, at
//!   most what is left), the 4th the rest. An arm of k spots starts at
//!   2 × `spawnRadius` from the centre and steps (R − that) / k. No nav
//!   requests.
//! - `PickRingPoints`: random: count × a random point on the
//!   circle of R (`RequestAndBlockRandomPointOnRadius`, s); even: count spots
//!   at angles i × 2π / count (+ half a step with `alternateSpawn`) on R,
//!   each requested near (s).
//! - `PickCirclePoints`: `forceCenterPoint` requests the centre
//!   first (one fewer); random: count × a random point in the disk of R
//!   (`RequestAndBlockRandomPointInRadius`, s); even: k = ⌈√count⌉, step
//!   2R / k, every grid point inside the disk requested near (s), however
//!   many that is.

/// `ResolveEnum_Pattern`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pattern {
    Grid,
    Lines,
    Ring,
    Circle,
}

impl Pattern {
    pub fn resolve(s: &str) -> Self {
        match s {
            "Grid" => Self::Grid,
            "Lines" => Self::Lines,
            "Circle" => Self::Circle,
            _ => Self::Ring,
        }
    }
}

/// A pattern record's placement fields (module doc).
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub pattern: Pattern,
    pub placement_radius: f32,
    pub spawn_radius: f32,
    /// `spawnLocations` at the skill's level.
    pub count: i32,
    pub random: bool,
    pub force_centre: bool,
    pub alternate: bool,
}

impl Params {
    pub fn from_record(rec: &gd_formats::arz::Record, level: usize) -> Self {
        let counts = rec.f32s("spawnLocations");
        let count = counts
            .get(level.max(1).min(counts.len().max(1)) - 1)
            .copied()
            .unwrap_or(0.0) as i32;
        Self {
            pattern: Pattern::resolve(rec.str("pattern").unwrap_or("")),
            placement_radius: rec.f32("placementRadius").unwrap_or(0.0),
            spawn_radius: rec.f32("spawnRadius").unwrap_or(0.0),
            count,
            random: rec.bool("randomPoints"),
            force_centre: rec.bool("forceCenterPoint"),
            alternate: rec.bool("alternateSpawn"),
        }
    }

    /// The spots' spacing (`max(2 × spawnRadius, 2)`).
    pub fn spacing(&self) -> f32 {
        (self.spawn_radius * 2.0).max(2.0)
    }
}

/// A nav request (`NavManager`, Engine.dll): offsets from the centre; the
/// answer is the blocked spot, if any.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Request {
    /// `RequestAndBlockNearPoint(at, spacing)`.
    Near { x: f32, z: f32, spacing: f32 },
    /// `RequestAndBlockRandomPointOnRadius(radius, spacing)`.
    OnRadius { radius: f32, spacing: f32 },
    /// `RequestAndBlockRandomPointInRadius(radius, spacing)`.
    InRadius { radius: f32, spacing: f32 },
}

/// The spots (x, z offsets from the centre) of `p`; `forward` the centre's
/// facing (x, z, unit), `rand(lo, hi)` the engine's inclusive
/// `IGenerate`, `request` the nav.
pub fn spots(
    p: &Params,
    forward: (f32, f32),
    mut rand: impl FnMut(i32, i32) -> i32,
    mut request: impl FnMut(Request) -> Option<(f32, f32)>,
) -> Vec<(f32, f32)> {
    let r = p.placement_radius;
    let s = p.spacing();
    let mut n = p.count.max(0);
    let mut out = Vec::new();
    if n == 0 {
        // An even grid of 0 spots would step by infinity and still yield one corner.
        return out;
    }
    let grid = |step: f32, disk: bool| {
        let mut c = Vec::new();
        // Bad data: a huge (or non-finite) `placementRadius` against the
        // spacing would enumerate unboundedly many candidates.
        let cells = (2.0 * r / step + 1.0).powi(2);
        if step <= 0.0 || cells.is_nan() || cells > MAX_GRID_CANDIDATES {
            return c;
        }
        let mut z = -r;
        while z <= r {
            let mut x = -r;
            while x <= r {
                if !disk || x * x + z * z <= r * r {
                    c.push((x, z));
                }
                x += step;
            }
            z += step;
        }
        c
    };
    match p.pattern {
        Pattern::Grid if p.random => {
            let mut cands = grid(s, true);
            if p.force_centre
                && let Some(c) = request(Request::Near {
                    x: 0.0,
                    z: 0.0,
                    spacing: s,
                })
            {
                out.push(c);
                n -= 1;
            }
            while n > 0 && !cands.is_empty() {
                let i = rand(0, cands.len() as i32 - 1).clamp(0, cands.len() as i32 - 1) as usize;
                let (x, z) = cands.swap_remove(i);
                if let Some(c) = request(Request::Near { x, z, spacing: s }) {
                    out.push(c);
                    n -= 1;
                }
            }
        }
        Pattern::Grid => {
            let k = (n as f32).sqrt().ceil();
            let cands = grid(2.0 * r / k, false);
            for (x, z) in cands.into_iter().skip(p.alternate as usize).step_by(2) {
                if let Some(c) = request(Request::Near { x, z, spacing: 2.0 }) {
                    out.push(c);
                }
            }
            if p.force_centre
                && let Some(i) = (0..out.len()).min_by(|a, b| len(out[*a]).total_cmp(&len(out[*b])))
            {
                let (dx, dz) = (-out[i].0, -out[i].1);
                for (j, o) in out.iter_mut().enumerate() {
                    *o = if j == i {
                        (0.0, 0.0)
                    } else {
                        (o.0 + dx, o.1 + dz)
                    };
                }
            }
        }
        Pattern::Lines => {
            if p.force_centre {
                out.push((0.0, 0.0));
                n -= 1;
            }
            let start = 2.0 * p.spawn_radius;
            let even = (n as f32 * 0.25) as i32;
            let mut angle: f32 = if p.alternate { 45.0 } else { 0.0 };
            for arm in 1..=4 {
                let k = if p.random {
                    let k = rand(1, (n as f32 - 4.0) as i32).max(1).min(n);
                    if arm == 4 { n } else { k }
                } else {
                    even
                };
                if p.random {
                    n -= k;
                }
                let (sin, cos) = angle.to_radians().sin_cos();
                let dir = (
                    forward.0 * cos + forward.1 * sin,
                    forward.1 * cos - forward.0 * sin,
                );
                angle += 90.0;
                let step = (r - start) / k as f32;
                for i in 0..k.max(0) {
                    let d = start + step * i as f32;
                    out.push((dir.0 * d, dir.1 * d));
                }
            }
        }
        Pattern::Ring if p.random => {
            for _ in 0..n {
                out.extend(request(Request::OnRadius {
                    radius: r,
                    spacing: s,
                }));
            }
        }
        Pattern::Ring => {
            let step = std::f32::consts::TAU / n.max(1) as f32;
            let half = if p.alternate { step * 0.5 } else { 0.0 };
            for j in 0..n {
                let a = j as f32 * step + half;
                out.extend(request(Request::Near {
                    x: a.cos() * r,
                    z: a.sin() * r,
                    spacing: s,
                }));
            }
        }
        Pattern::Circle => {
            if p.force_centre
                && let Some(c) = request(Request::Near {
                    x: 0.0,
                    z: 0.0,
                    spacing: s,
                })
            {
                out.push(c);
                n -= 1;
            }
            if p.random {
                for _ in 0..n {
                    out.extend(request(Request::InRadius {
                        radius: r,
                        spacing: s,
                    }));
                }
            } else {
                let k = (n as f32).sqrt().ceil();
                for (x, z) in grid(2.0 * r / k, true) {
                    out.extend(request(Request::Near { x, z, spacing: s }));
                }
            }
        }
    }
    out
}

fn len(o: (f32, f32)) -> f32 {
    (o.0 * o.0 + o.1 * o.1).sqrt()
}

/// `NavManager`'s blocked circles (Engine.dll, `NavManagerImpl.blockedPoints`): a transient
/// list of (x, z, r) read only while a skill picks its spots (never by
/// path-finding or movement); the pickers empty it when they finish
/// (`ClearRequestPointData`), so one `NavBlock` lives for one activation.
/// `ground(x, z)` is the nav probe (`NavManager::ProbeWalkableHeight`, flag 0x40000000): the
/// floor height where there is walkable ground; `rand()` in [0, 1) is the
/// engine's `rand() / 32767`. Engine: `NavManager::IsBlocked`,
/// `NavManager::BlockPoint`, `NavManager::RequestAndBlockNearPoint`,
/// `NavManager::RequestAndBlockRandomPointInRadius`,
/// `NavManager::RequestAndBlockRandomPointOnRadius`. PORT: the on-radius
/// request's `needPath` reachability test (`NavManager::HasPathTo`) and sector layer
/// test are not made (`PickRingPoints` passes `false, -1`).
#[derive(Clone, Debug, Default)]
pub struct NavBlock {
    circles: Vec<(f32, f32, f32)>,
}

/// Tries of the random requests (`RequestAndBlockRandomPoint…`).
const RANDOM_TRIES: usize = 100;

/// Grid candidate ceiling (bad data: a huge `placementRadius`); stock patterns
/// enumerate a few hundred.
const MAX_GRID_CANDIDATES: f32 = 1_048_576.0;

impl NavBlock {
    /// `IsBlocked`: the XZ distance to a circle is `<= r + r_circle`.
    pub fn is_blocked(&self, x: f32, z: f32, r: f32) -> bool {
        self.circles
            .iter()
            .any(|&(cx, cz, cr)| ((x - cx).powi(2) + (z - cz).powi(2)).sqrt() <= r + cr)
    }

    /// `BlockPoint`: refused when blocked, else the circle is appended.
    pub fn block_point(&mut self, x: f32, z: f32, r: f32) -> bool {
        if self.is_blocked(x, z, r) {
            return false;
        }
        self.circles.push((x, z, r));
        true
    }

    /// `ClearRequestPointData`.
    pub fn clear(&mut self) {
        self.circles.clear();
    }

    pub fn len(&self) -> usize {
        self.circles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.circles.is_empty()
    }

    /// `RequestAndBlockNearPoint(center, max_dist, out, r)`: rings at
    /// d = 0, step, 2 step ... while d < `max_dist` (step = 2r + 0.1),
    /// `int(2 pi d / step)` samples per ring (1 at d = 0) evenly spaced from
    /// a start angle `rand() * 2 pi` (drawn once); the first sample
    /// (cx + sin a d, cz + cos a d) with ground and not blocked for `r` is
    /// taken and blocked. With `max_dist = r` that is the centre or nothing.
    pub fn near_point(
        &mut self,
        center: (f32, f32),
        max_dist: f32,
        r: f32,
        rand: &mut impl FnMut() -> f32,
        ground: &mut impl FnMut(f32, f32) -> Option<f32>,
    ) -> Option<[f32; 3]> {
        let tau = std::f32::consts::TAU;
        let step = 2.0 * r + 0.1;
        let mut a = rand() * tau;
        // Bad data: a negative / non-finite radius gives a step <= 0 (or a
        // vanishing one) and a non-finite distance never ends the rings.
        if !(r >= 0.0 && r.is_finite() && max_dist.is_finite()) {
            return None;
        }
        let (mut ring, mut n) = (0.0f32, 1i32);
        while ring < max_dist {
            let da = tau / n.max(1) as f32;
            for _ in 0..n {
                let (x, z) = (center.0 + a.sin() * ring, center.1 + a.cos() * ring);
                if !self.is_blocked(x, z, r)
                    && let Some(y) = ground(x, z)
                {
                    self.circles.push((x, z, r));
                    return Some([x, y, z]);
                }
                a += da;
            }
            ring += step;
            n = (ring * tau / step) as i32;
        }
        None
    }

    /// `RequestAndBlockRandomPointInRadius`: up to 100 tries at a random
    /// angle (`rand() * 2 pi`) and distance (`rand() * R`, uniform in
    /// distance, not area), with ground and not blocked.
    pub fn random_in_radius(
        &mut self,
        center: (f32, f32),
        radius: f32,
        r: f32,
        rand: &mut impl FnMut() -> f32,
        ground: &mut impl FnMut(f32, f32) -> Option<f32>,
    ) -> Option<[f32; 3]> {
        for _ in 0..RANDOM_TRIES {
            let a = rand() * std::f32::consts::TAU;
            let d = rand() * radius;
            if let Some(p) =
                self.try_point(center.0 + a.cos() * d, center.1 + a.sin() * d, r, ground)
            {
                return Some(p);
            }
        }
        None
    }

    /// `RequestAndBlockRandomPointOnRadius(.., needPath = false, -1)`: up to
    /// 100 tries at a random angle on the ring of `radius`.
    pub fn random_on_radius(
        &mut self,
        center: (f32, f32),
        radius: f32,
        r: f32,
        rand: &mut impl FnMut() -> f32,
        ground: &mut impl FnMut(f32, f32) -> Option<f32>,
    ) -> Option<[f32; 3]> {
        for _ in 0..RANDOM_TRIES {
            let a = rand() * std::f32::consts::TAU;
            if let Some(p) = self.try_point(
                center.0 + a.cos() * radius,
                center.1 + a.sin() * radius,
                r,
                ground,
            ) {
                return Some(p);
            }
        }
        None
    }

    fn try_point(
        &mut self,
        x: f32,
        z: f32,
        r: f32,
        ground: &mut impl FnMut(f32, f32) -> Option<f32>,
    ) -> Option<[f32; 3]> {
        let y = ground(x, z)?;
        self.block_point(x, z, r).then_some([x, y, z])
    }
}

/// `NavManager::FillPointSet` (Engine.dll, the
/// non-random variant `Skill_SpawnPet::ActivateNow` asks for): one point per
/// radius in `radii`, in order, each by [`NavBlock::near_point`]. Any radius
/// that finds no place empties the whole set.
pub fn fill_point_set(
    center: (f32, f32),
    radii: &[f32],
    max_dist: f32,
    rand: impl FnMut() -> f32,
    ground: impl FnMut(f32, f32) -> Option<f32>,
) -> Vec<[f32; 3]> {
    fill_point_set_in(
        &mut NavBlock::default(),
        center,
        radii,
        max_dist,
        rand,
        ground,
    )
}

/// [`fill_point_set`] on the circles already blocked in `nav` (a caller's
/// `BlockPoint` first, as `Skill_TargetedSpawnPet::ActivateNow` blocks its
/// target's footprint). The engine's 7th argument (`a7`) picks
/// [`NavBlock::random_in_radius`] instead; no port caller asks for it.
pub fn fill_point_set_in(
    nav: &mut NavBlock,
    center: (f32, f32),
    radii: &[f32],
    max_dist: f32,
    mut rand: impl FnMut() -> f32,
    mut ground: impl FnMut(f32, f32) -> Option<f32>,
) -> Vec<[f32; 3]> {
    let mut out = Vec::new();
    for &r in radii {
        match nav.near_point(center, max_dist, r, &mut rand, &mut ground) {
            Some(p) => out.push(p),
            None => return Vec::new(),
        }
    }
    out
}
