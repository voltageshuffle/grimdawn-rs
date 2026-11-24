//! The crowd's per-agent rules: who meets whom, who pushes whom, the monsters' separation steering and the
//! player's velocity sampling. Engine.dll crowd update `Crowd::Update` (`crowd.cpp`), the agents'
//! parameters from Game.dll `Character::CrowdAgentCreated` / `CrowdAgentUpdate` and the
//! `Player` / `Monster` overrides (`Player::CrowdAgentCreated`, `Monster::CrowdAgentCreated` /
//! `Monster::CrowdAgentUpdate`); the ECS side is
//! `gd_game::movement`. Positions and velocities here are horizontal (x, z).
//!
//! Collision groups (`CrowdAgentParams.collisionGroup`) and masks (`CrowdAgentParams.collisionMask`): an agent's
//! neighbours are the agents whose group meets its mask,
//! found in a sphere around it (`Crowd::QueryNeighbours`: 3D distance below the range, within half the agent's
//! height up or
//! down). The player is group 1, mask 2; a monster hostile to the player group 2, mask 7; any other monster (pets,
//! allies) group 1, mask 2; other characters (NPCs) group 4, mask 6. A monster with `forceCollision`, or a Boss /
//! Quest / SuperBoss (`Monster::Load`: classification 3..5), is hostile-grouped and adds group 8; one
//! with `forceNoCollision` has group 0 (no one meets it). Priority (`CrowdAgentParams.priority`): 2 for the player,
//! 1 for every other one.
//! A spinning character (`Skill_AttackRadiusSpin::StartAction` sets `Character.spinning`, the
//! "selective pathing push-through", until `StopSpinning`) has mask 8 (`CrowdAgentUpdate`): it meets only the group-8
//! monsters. The `Monster` override rewrites the mask afterwards, so this only matters for the player.
use crate::grid::Grid;
use std::f32::consts::PI;

/// A horizontal vector (x, z).
pub type V2 = [f32; 2];

/// The collision groups and masks (module docs).
pub mod group {
    pub const PLAYER: (u32, u32) = (1, 2);
    pub const HOSTILE: (u32, u32) = (2, 7);
    pub const FRIENDLY: (u32, u32) = (1, 2);
    pub const OTHER: (u32, u32) = (4, 6);
    /// Added to the group of a `forceCollision` monster.
    pub const FORCED: u32 = 8;
    /// The mask of a spinning non-monster (module docs).
    pub const SPIN_MASK: u32 = 8;
}

/// The player's crowd priority; every other character has 1.
pub const PLAYER_PRIORITY: i32 = 2;
/// The separation weight of a monster (`Monster::CrowdAgentCreated`, `CrowdAgentParams.separationWeight`); 0 for
/// other characters.
pub const MONSTER_SEPARATION: f32 = 1.1;
/// `CrowdAgentParams.arriveDistance`: the agent arrives within this distance of its target; the player's avoidance
/// also ignores a
/// neighbour standing at the target by this margin.
pub const ARRIVE_DIST: f32 = 0.025;
/// `CrowdAgentParams.stuckSpeed` / `stuckTime`: a moving agent slower than this (m/s) for more than [`STUCK_MS`]
/// fails its path (reason 3).
pub const STUCK_SPEED: f32 = 1.1;
pub const STUCK_MS: f32 = 400.0;
/// `CrowdAgentParams.height` stands in for `Actor::GetHeight` in the neighbour query (PORT: 2 m for everyone).
pub const HEIGHT: f32 = 2.0;

/// The neighbour range of a moving agent's planning (`max(8 r, r + the largest radius)`).
pub fn plan_range(r: f32, max_r: f32) -> f32 {
    (8.0 * r).max(r + max_r)
}

/// The neighbour range of a still agent's collision pass (`max(4 r, r + the largest radius)`).
pub fn collide_range(r: f32, max_r: f32) -> f32 {
    (4.0 * r).max(r + max_r)
}

/// The cell of the crowd's neighbour grid ([`crate::grid::Grid`], PORT: the engine's proximity grid): 2 × the
/// largest radius, at least 0.5 m.
pub fn grid_cell(max_r: f32) -> f32 {
    (2.0 * max_r).max(0.5)
}

/// `Character::CrowdAgentDepenetrate` (called with (own state, own priority, the neighbour's
/// state, its priority)): whether an agent (`moving`, `priority`) is pushed by an overlapping neighbour. A moving agent
/// is pushed unless the neighbour moves too with a higher priority; a still one only by a still neighbour of higher
/// priority (a running player slides off a standing monster, a standing player shoves the standing monsters out).
/// (`Monster::CrowdAgentDepenetrate`: never for a `forceCollision` one, nor while its Spawn animation plays.)
pub fn depenetrated_by(
    moving: bool,
    priority: i32,
    other_moving: bool,
    other_priority: i32,
) -> bool {
    if moving {
        !(other_moving && other_priority > priority)
    } else {
        !other_moving && other_priority > priority
    }
}

/// One agent of the collision pass.
#[derive(Clone, Copy, Debug)]
pub struct Body {
    pub pos: V2,
    pub y: f32,
    pub radius: f32,
    pub group: u32,
    pub mask: u32,
    pub priority: i32,
    /// Crowd state 2 (following a path).
    pub moving: bool,
    /// Collision flag without "no collision" (flags & 0x801 == 1): the agent is depenetrated at all.
    pub collides: bool,
    /// `CrowdAgentDepenetrate` may say yes (false for a `forceCollision` monster and one playing its Spawn
    /// animation).
    pub pushable: bool,
    /// The movement direction (`CrowdAgent.moveDir`), which parts coincident agents.
    pub dir: V2,
}

impl Body {
    /// Whether `other` is in this agent's neighbour query of range `range`.
    pub fn meets(&self, other: &Body, range: f32) -> bool {
        let (dx, dz, dy) = (
            other.pos[0] - self.pos[0],
            other.pos[1] - self.pos[1],
            other.y - self.y,
        );
        self.mask & other.group != 0
            && dy.abs() <= HEIGHT * 0.5
            && dx * dx + dy * dy + dz * dz < range * range
    }
}

/// The collision pass (`Crowd::Update`, the last loop): every colliding agent's push away from the overlapping
/// neighbours that may push it, each (r_a + r_b − d) / d × 0.35 of the separation vector, averaged over them.
/// Coincident agents (d ≤ 1 mm) part by 1 mm along the agent's direction (x negated for the earlier agent, z for
/// the later one: the engine orders by address, PORT: by index). A still agent only takes a push of at least
/// √0.001 m; all pushes are computed from the same positions and applied together.
pub fn depenetration(bodies: &[Body]) -> Vec<V2> {
    let max_r = bodies.iter().map(|b| b.radius).fold(0.0, f32::max);
    let mut out = vec![[0.0; 2]; bodies.len()];
    let grid = Grid::new(grid_cell(max_r), bodies.iter().map(|b| b.pos));
    // (|radius|: negative radii, bad data, still overlap by |r_a + r_b|.)
    let reach = bodies.iter().map(|b| b.radius.abs()).fold(0.0, f32::max);
    let mut near: Vec<usize> = Vec::new();
    for (i, a) in bodies.iter().enumerate() {
        if !a.collides || !a.pushable {
            continue;
        }
        let range = if a.moving {
            plan_range(a.radius, max_r)
        } else {
            collide_range(a.radius, max_r)
        };
        let (mut sum, mut n) = ([0.0f32; 2], 0.0f32);
        // Only overlapping neighbours count (d < |r_a + r_b| ≤ |r_a| + the largest |radius|). By index, the
        // engine's order (as the agents were added).
        grid.near(a.pos, a.radius.abs() + reach, &mut near);
        for &j in &near {
            let b = &bodies[j];
            if i == j || !a.meets(b, range) {
                continue;
            }
            if !depenetrated_by(a.moving, a.priority, b.moving, b.priority) {
                continue;
            }
            let (mut dx, mut dz) = (a.pos[0] - b.pos[0], a.pos[1] - b.pos[1]);
            let rs = a.radius + b.radius;
            let d2 = dx * dx + dz * dz;
            if d2 >= rs * rs {
                continue;
            }
            let d = d2.sqrt();
            let pen = if d <= 0.001 {
                (dx, dz) = (a.dir[0], a.dir[1]);
                if i >= j {
                    dz = -dz;
                } else {
                    dx = -dx;
                }
                0.001
            } else {
                (rs - d) / d * 0.35
            };
            sum[0] += pen * dx;
            sum[1] += pen * dz;
            n += 1.0;
        }
        if n > 0.0001 {
            sum = [sum[0] / n, sum[1] / n];
        }
        if a.moving || sum[0] * sum[0] + sum[1] * sum[1] >= 0.001 {
            out[i] = sum;
        }
    }
    out
}

/// A moving monster's separation (the planning pass, for agents with a separation weight and no "no collision"
/// flag): over the neighbours that have a separation weight too and are apart (d² − r² − r_n² > 0.0001), each adds
/// (me − it) × (1 − (s / range)²) × weight / s with s = √(d² − r² − r_n²); the average is added to the desired
/// velocity, which is then clamped to the agent's speed. `others` are (position, radius); None without such a
/// neighbour.
pub fn separation(
    pos: V2,
    r: f32,
    range: f32,
    weight: f32,
    others: impl Iterator<Item = (V2, f32)>,
) -> Option<V2> {
    // Bad data: all-zero radii give a zero range, and `s / range` would be inf.
    if range.is_nan() || range <= 0.0 {
        return None;
    }
    let (mut sum, mut n) = ([0.0f32; 2], 0.0f32);
    for (p, rn) in others {
        let (dx, dz) = (pos[0] - p[0], pos[1] - p[1]);
        let s2 = dx * dx + dz * dz - r * r - rn * rn;
        if s2 > 0.0001 {
            n += 1.0;
            let s = s2.sqrt();
            let w = (1.0 - (s / range) * (s / range)) * weight / s;
            sum[0] += dx * w;
            sum[1] += dz * w;
        }
    }
    (n > 0.0).then(|| [sum[0] / n, sum[1] / n])
}

/// `v` scaled down to at most `max` long.
pub fn clamp_len(v: V2, max: f32) -> V2 {
    let l2 = v[0] * v[0] + v[1] * v[1];
    if l2 > max * max {
        // (The engine scales by max² / l², not max / l: a clamped velocity comes out shorter.)
        let k = max * max / l2;
        [v[0] * k, v[1] * k]
    } else {
        v
    }
}

/// The stuck check (the crowd update's end, `CrowdAgentParams.stuckSpeed` / `stuckTime` copied to
/// `CrowdAgent.stuckSpeed` / `stuckTime`, the count in `CrowdAgent.stuckCount`): a moving agent whose
/// top speed is above [`STUCK_SPEED`] and that moved slower than it (its displacement over the update, pushes
/// included) for more than [`STUCK_MS`] fails its path (reason 3), and the count starts again.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stuck {
    ms: f32,
}

impl Stuck {
    /// One update of a moving agent with top speed `max_speed` that moved at `speed` over `dt_ms`: true when its
    /// path fails.
    pub fn update(&mut self, max_speed: f32, speed: f32, dt_ms: f32) -> bool {
        if STUCK_SPEED >= max_speed {
            return false;
        }
        if speed >= STUCK_SPEED {
            self.ms = 0.0;
            return false;
        }
        self.ms += dt_ms;
        if self.ms > STUCK_MS {
            self.ms = 0.0;
            return true;
        }
        false
    }
}

/// The integration's occupied-target test (`Crowd::Update`, failure reason 2): an agent whose step ends at
/// `new` (radius `r`) toward `target` fails its path when the step overlaps a neighbour (horizontal distance at most
/// the radii's sum) that stands on the target (the target within the neighbour's radius and its half height) or
/// when the agent itself is then within its radius of the target (and its half height). `neighbours` are
/// (position, radius) of the agents it meets; the height window is [`HEIGHT`].
pub fn occupied_target(
    new: [f32; 3],
    r: f32,
    target: [f32; 3],
    mut neighbours: impl Iterator<Item = ([f32; 3], f32)>,
) -> bool {
    let flat = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
    let on =
        |p: [f32; 3], rad: f32| flat(target, p) <= rad && (target[1] - p[1]).abs() <= HEIGHT * 0.5;
    neighbours.any(|(p, rn)| flat(new, p) <= r + rn && (on(p, rn) || on(new, r)))
}

/// A neighbour of the player's avoidance.
#[derive(Clone, Copy, Debug)]
pub struct Obstacle {
    pub pos: V2,
    pub radius: f32,
    /// Its velocity (speed × direction).
    pub vel: V2,
    /// Its desired velocity.
    pub dvel: V2,
}

/// The avoidance's velocity sampling (`ObstacleAvoidanceQuery::SampleVelocityAdaptive` with the sample score
/// `ObstacleAvoidanceQuery::ProcessSample`; Detour's adaptive
/// sampling with its own weights): for an agent at `pos` with radius `r`, top speed `vmax`, velocity `vel` and
/// desired velocity `dvel`, the best velocity against `obstacles`. 22 samples (the centre and three rings of 7 at
/// radii 1, ⅔, ⅓, the middle ring turned by π/7, the pattern turned to `dvel`) around `dvel` × 0.25 with radius
/// 0.75 `vmax`, 3 passes in all (the loop counter starts at 3), each after the first around the
/// previous best at half the radius; samples faster than `vmax` + 1 mm are
/// skipped. Score: 10 |dvel − v| / vmax + 0.75 |vel − v| / vmax + 0.75 × side + 2.5 / (0.1 + 0.4 t), with `side`
/// the average of clamp(min(dp·vab × 0.5 + 0.5, np·vab × 2), 0, 1) and `t` the earliest impact of the relative
/// velocity vab = 2 v − vel − its velocity (2.5 s at most; an overlap counts half its exit time).
pub fn avoid(pos: V2, r: f32, vmax: f32, vel: V2, dvel: V2, obstacles: &[Obstacle]) -> V2 {
    // Each obstacle's direction from the agent and the side to pass it on.
    let sides: Vec<(V2, V2)> = obstacles
        .iter()
        .map(|o| {
            let d = [o.pos[0] - pos[0], o.pos[1] - pos[1]];
            let l = (d[0] * d[0] + d[1] * d[1]).sqrt();
            let dp = if l > 0.0 {
                [d[0] / l, d[1] / l]
            } else {
                [0.0, 0.0]
            };
            let dv = [o.dvel[0] - dvel[0], o.dvel[1] - dvel[1]];
            let np = if dp[1] * dv[0] - dp[0] * dv[1] >= 0.01 {
                [dp[1], -dp[0]]
            } else {
                [-dp[1], dp[0]]
            };
            (dp, np)
        })
        .collect();
    let inv_vmax = 1.0 / vmax;
    let score = |v: V2| {
        let (mut side, mut tmin) = (0.0f32, 2.5f32);
        for (o, (dp, np)) in obstacles.iter().zip(&sides) {
            let vab = [
                2.0 * v[0] - vel[0] - o.vel[0],
                2.0 * v[1] - vel[1] - o.vel[1],
            ];
            side += (dp[0] * vab[0] * 0.5 + dp[1] * vab[1] * 0.5 + 0.5)
                .min(np[0] * vab[0] * 2.0 + np[1] * vab[1] * 2.0)
                .clamp(0.0, 1.0);
            let a = vab[0] * vab[0] + vab[1] * vab[1];
            if a < 0.0001 {
                continue;
            }
            let s = [o.pos[0] - pos[0], o.pos[1] - pos[1]];
            let b = vab[0] * s[0] + vab[1] * s[1];
            let rs = r + o.radius;
            let c = s[0] * s[0] + s[1] * s[1] - rs * rs;
            let disc = b * b - c * a;
            if disc < 0.0 {
                continue;
            }
            let rd = disc.sqrt();
            let (mut t0, t1) = ((b - rd) / a, (b + rd) / a);
            if t0 < 0.0 && t1 > 0.0 {
                t0 = -t0 * 0.5;
            }
            if t0 >= 0.0 {
                tmin = tmin.min(t0);
            }
        }
        if !obstacles.is_empty() {
            side /= obstacles.len() as f32;
        }
        let dist = |a: V2, b: V2| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
        dist(dvel, v) * inv_vmax * 10.0
            + dist(vel, v) * inv_vmax * 0.75
            + side * 0.75
            + 2.5 / (tmin * 0.4 + 0.1)
    };
    let base = dvel[1].atan2(dvel[0]);
    let mut pattern = [[0.0f32; 2]; 22];
    for ring in 0..3 {
        let rad = (3 - ring) as f32 / 3.0;
        let mut a = base + (ring & 1) as f32 * (PI / 7.0);
        for k in 0..7 {
            pattern[1 + ring * 7 + k] = [a.cos() * rad, a.sin() * rad];
            a += 2.0 * PI / 7.0;
        }
    }
    let mut res = [dvel[0] * 0.25, dvel[1] * 0.25];
    let mut cr = vmax * 0.75;
    let lim = (vmax + 0.001) * (vmax + 0.001);
    for _ in 0..3 {
        let (mut best, mut best_score) = ([0.0f32; 2], f32::MAX);
        for p in &pattern {
            let v = [res[0] + p[0] * cr, res[1] + p[1] * cr];
            if v[0] * v[0] + v[1] * v[1] > lim {
                continue;
            }
            let s = score(v);
            if s < best_score {
                best_score = s;
                best = v;
            }
        }
        res = best;
        cr *= 0.5;
    }
    res
}
