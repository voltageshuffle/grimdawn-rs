//! `SlotManager` (Game.dll): the spots around a character that attackers and
//! followers are handed so they spread out instead of piling on one point.
//!
//! Every `Character` owns two: the attack manager (`numAttackSlots`, default
//! 4, mode 0) that `RequestAttackSlot` / `GetMoveToPoint` ask for melee
//! attackers, and the defence manager (`numDefenseSlots`, default 4, mode 1)
//! `RequestDefenseSlot` asks for followers (`Character::Character`,
//! `Character::Load`). A manager is a list of rings around its
//! owner; ring 0 is made by `SetNumSlots` and holds that many slots, ring
//! `i` holds `ring 0's count * (i + 1)` slots (`AddNewRing`). Ring `i` lies at
//! `(i + 1) * spacing` from the owner (`CalculateSlotPosition`), slot `k` at
//! angle `k * 2pi / n` (odd rings shifted by half a step), offset
//! `(cos * d, 0, sin * d)`; positions are recalculated when the spacing or the
//! owner's spot changed (`CalculateSlotPositions`).
//!
//! `SlotManager::RequestSlot`: per ring in order it refreshes the positions,
//! frees the slots held by the requester or by gone / dead characters
//! (`CleanupRing`) and looks for a free slot (`SearchRingForFreeSlot`); free
//! slots are tried nearest to the requester first and the first on the path
//! mesh that the requester `CanMoveTo` (unless `skip_reach`) is taken
//! (`GetPrioritizedSlot`). With every ring full a defence manager (mode 1)
//! with fewer than 3 rings adds a ring and searches it; else
//! `SearchRingForOverrideSlot` offers the occupied slots where the requester
//! is nearer than the occupant (distance squared less radius squared, floored
//! at 0), nearest first, and the winner takes the slot from its occupant, who
//! is told `LostSlot`. No slot found: no position (the game hands back a
//! default `WorldVec3`, which has no region).
//! `ReleaseSlot` frees the id's slots in every ring.

/// What a request needs from the world (the game reads the object manager,
/// the nav manager and the characters).
pub trait SlotEnv {
    /// The character `id` if it is alive: its position and radius
    /// (`CleanupRing`'s `IsAlive` and `GetRadius`); None: gone or dead.
    fn body(&self, id: u64) -> Option<([f32; 3], f32)>;
    /// `NavManager::IsPointOnPathMesh`.
    fn on_path_mesh(&self, p: [f32; 3]) -> bool;
    /// `Character::CanMoveTo` for the requester.
    fn can_move_to(&self, p: [f32; 3]) -> bool;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotMode {
    /// Attack slots: the rings are those of `SetNumSlots` only.
    Attack,
    /// Defence slots: up to 3 rings.
    Defense,
}

#[derive(Clone, Debug)]
struct Slot {
    /// The character holding it (0: free).
    id: u64,
    pos: [f32; 3],
}

#[derive(Clone, Debug)]
struct Ring {
    slots: Vec<Slot>,
    /// The owner's spot the positions were calculated for.
    center: [f32; 3],
    /// The spacing they were calculated for.
    spacing: f32,
}

#[derive(Clone, Debug)]
pub struct SlotManager {
    mode: SlotMode,
    rings: Vec<Ring>,
}

/// A request's result: the spot and the character that lost its slot to it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Granted {
    pub pos: [f32; 3],
    pub lost: Option<u64>,
}

fn dist_sq(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

impl SlotManager {
    pub fn new(mode: SlotMode) -> Self {
        Self {
            mode,
            rings: Vec::new(),
        }
    }

    /// `SetNumSlots`: a single ring of `n` free slots.
    pub fn set_num_slots(&mut self, n: usize) {
        self.rings = vec![Ring {
            slots: vec![
                Slot {
                    id: 0,
                    pos: [0.0; 3]
                };
                n
            ],
            center: [0.0; 3],
            spacing: 0.0,
        }];
    }

    /// `ReleaseSlot`.
    pub fn release(&mut self, id: u64) {
        for r in &mut self.rings {
            for s in &mut r.slots {
                if s.id == id {
                    s.id = 0;
                }
            }
        }
    }

    /// The spot `id` holds, if any (what the game reads back from the rings).
    pub fn held(&self, id: u64) -> Option<[f32; 3]> {
        self.rings
            .iter()
            .flat_map(|r| &r.slots)
            .find(|s| s.id == id)
            .map(|s| s.pos)
    }

    pub fn rings(&self) -> usize {
        self.rings.len()
    }

    /// `CalculateSlotPosition`.
    fn slot_position(&self, owner: [f32; 3], ring: usize, k: usize, spacing: f32) -> [f32; 3] {
        let n = self.rings[ring].slots.len() as f32;
        let step = std::f32::consts::TAU / n;
        let mut a = k as f32 * step;
        if ring & 1 != 0 {
            a += step * 0.5;
        }
        let d = (ring + 1) as f32 * spacing;
        [owner[0] + a.cos() * d, owner[1], owner[2] + a.sin() * d]
    }

    /// `CalculateSlotPositions`.
    fn calc_positions(&mut self, owner: [f32; 3], ring: usize, spacing: f32) {
        if self.rings[ring].spacing != spacing || self.rings[ring].center != owner {
            self.rings[ring].spacing = spacing;
            self.rings[ring].center = owner;
            for k in 0..self.rings[ring].slots.len() {
                let p = self.slot_position(owner, ring, k, spacing);
                self.rings[ring].slots[k].pos = p;
            }
        }
    }

    /// `CleanupRing`.
    fn cleanup(&mut self, ring: usize, requester: u64, env: &dyn SlotEnv) {
        for s in &mut self.rings[ring].slots {
            if s.id != 0 && (s.id == requester || env.body(s.id).is_none()) {
                s.id = 0;
            }
        }
    }

    /// `GetPrioritizedSlot`: the nearest candidate on the path mesh (and
    /// reachable unless `skip_reach`) is taken.
    fn prioritized(
        &mut self,
        ring: usize,
        mut cands: Vec<(usize, f32)>,
        requester: u64,
        skip_reach: bool,
        env: &dyn SlotEnv,
    ) -> Option<Granted> {
        cands.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        for (k, _) in cands {
            let pos = self.rings[ring].slots[k].pos;
            if env.on_path_mesh(pos) && (skip_reach || env.can_move_to(pos)) {
                let prev = std::mem::replace(&mut self.rings[ring].slots[k].id, requester);
                return Some(Granted {
                    pos,
                    lost: (prev != 0).then_some(prev),
                });
            }
        }
        None
    }

    /// `SearchRingForFreeSlot`.
    fn search_free(
        &mut self,
        ring: usize,
        req_pos: [f32; 3],
        requester: u64,
        skip_reach: bool,
        env: &dyn SlotEnv,
    ) -> Option<Granted> {
        let cands = self.rings[ring]
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.id == 0)
            .map(|(k, s)| (k, dist_sq(s.pos, req_pos)))
            .collect();
        self.prioritized(ring, cands, requester, skip_reach, env)
    }

    /// `SearchRingForOverrideSlot`.
    fn search_override(
        &mut self,
        ring: usize,
        req_pos: [f32; 3],
        req_radius: f32,
        requester: u64,
        skip_reach: bool,
        env: &dyn SlotEnv,
    ) -> Option<Granted> {
        let mut cands = Vec::new();
        for (k, s) in self.rings[ring].slots.iter().enumerate() {
            if s.id == 0 {
                continue;
            }
            let Some((opos, orad)) = env.body(s.id) else {
                continue;
            };
            let mine = dist_sq(s.pos, req_pos);
            let theirs = dist_sq(s.pos, opos);
            if (mine - req_radius * req_radius).max(0.0) < (theirs - orad * orad).max(0.0) {
                cands.push((k, mine));
            }
        }
        self.prioritized(ring, cands, requester, skip_reach, env)
    }

    /// `RequestSlot` (`owner`: the manager's character spot; `spacing`:
    /// `GetTargetDistance` for attack slots, both radii + 0.5 for defence
    /// slots). The requester is alive and `spacing` is non-zero, else none.
    #[allow(clippy::too_many_arguments)]
    pub fn request(
        &mut self,
        owner: [f32; 3],
        requester: u64,
        req_pos: [f32; 3],
        req_radius: f32,
        spacing: f32,
        skip_reach: bool,
        env: &dyn SlotEnv,
    ) -> Option<Granted> {
        if spacing == 0.0 || self.rings.is_empty() || env.body(requester).is_none() {
            return None;
        }
        for i in 0..self.rings.len() {
            self.calc_positions(owner, i, spacing);
            self.cleanup(i, requester, env);
            if let Some(g) = self.search_free(i, req_pos, requester, skip_reach, env) {
                return Some(g);
            }
        }
        if self.mode == SlotMode::Defense && self.rings.len() < 3 {
            // `AddNewRing`: ring 0's count * (rings + 1) slots.
            let n = self.rings[0].slots.len() * (self.rings.len() + 1);
            self.rings.push(Ring {
                slots: vec![
                    Slot {
                        id: 0,
                        pos: [0.0; 3]
                    };
                    n
                ],
                center: [0.0; 3],
                spacing: 0.0,
            });
            let i = self.rings.len() - 1;
            self.calc_positions(owner, i, spacing);
            if let Some(g) = self.search_free(i, req_pos, requester, skip_reach, env) {
                return Some(g);
            }
        }
        for i in 0..self.rings.len() {
            if let Some(g) =
                self.search_override(i, req_pos, req_radius, requester, skip_reach, env)
            {
                return Some(g);
            }
        }
        None
    }
}
