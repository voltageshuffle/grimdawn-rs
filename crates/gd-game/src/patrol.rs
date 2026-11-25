//! Patrol points and patrolling monsters.
//!
//! Level data (map chunk 0x11, instance groups): "Patrol Points" groups list the uids of `PatrolPoint` level
//! objects in walking order; a proxy (or a placed monster) is in a "ProxyPatrollers" (resp. "Patrollers") group
//! whose child groups are "Patrol Points" groups. A monster attached to its controller takes the patrol points of
//! those child groups (`ControllerMonster::PostParentAttached`); `Proxy:LinkPatrolPointGroup` edits the groups at
//! run time and re-points the proxy's live monsters.
//!
//! The patrol itself is the controller's "Patrol" state, with the other idle-time states: see [`PatrolState`].
use crate::nav::Nav;
use bevy::prelude::*;
use gd_formats::map::Uid;
pub use gd_sim::patrol::story_proxy_uid;
use std::cell::RefCell;

pub struct PatrolPlugin;

impl Plugin for PatrolPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PatrolGroups>().add_systems(
            Update,
            (
                link_patrol_groups,
                call_for_followers.after(gd_game_core::combat::CombatSet),
            ),
        );
    }
}

/// A `PatrolPoint` level object (`PatrolPoint::Load`).
#[derive(Component)]
pub struct PatrolPoint {
    pub uid: Uid,
    pub radius: f32,
    pub run: bool,
}

#[derive(Resource, Default)]
pub struct PatrolGroups {
    pub groups: gd_sim::patrol::PatrolGroups,
    pub built: bool,
}

/// `Proxy:LinkPatrolPointGroup`: the group bookkeeping, then the proxy's spawned monsters get the points (the
/// current point resets to 0).
fn link_patrol_groups(
    mut events: MessageReader<gd_game_core::story::StoryEffect>,
    mut groups: ResMut<PatrolGroups>,
    mut monsters: Query<(
        &gd_game_core::story::StoryProxyChild,
        &mut crate::combat::MonsterAi,
    )>,
) {
    for ev in events.read() {
        let gd_story::story::state::Effect::LinkPatrolPointGroup { proxy, group } = &ev.0 else {
            continue;
        };
        let Some(points) = groups.groups.link(story_proxy_uid(*proxy), group) else {
            warn!("LinkPatrolPointGroup() - '{group}' is not a Patrol Points group");
            continue;
        };
        for (child, mut ai) in &mut monsters {
            if child.0 == *proxy {
                ai.patrol.replace(points.clone());
            }
        }
    }
}

#[derive(Clone, Copy)]
pub struct PointInfo {
    pub pos: Vec3,
    pub radius: f32,
    pub run: bool,
}

pub type Points<'w, 's> = Query<'w, 's, (&'static PatrolPoint, &'static Transform)>;

fn lookup(points: &Points, uid: Uid) -> Option<PointInfo> {
    points
        .iter()
        .find(|(p, _)| p.uid == uid)
        .map(|(p, t)| PointInfo {
            pos: t.translation,
            radius: p.radius,
            run: p.run,
        })
}

/// `ControllerMonster::Load` settings of the idle-time states (defaults: `NeverRoam`, `NeverLead`, 20 %,
/// 2000..5000 ms patrol idle, 3000..10000 ms before roaming, 4 followers, 0 for every distance).
#[derive(Clone, Debug)]
pub struct Settings {
    /// `RoamBehavior` is `Roam` (else `NeverRoam`).
    pub roam: bool,
    /// `ChanceToIdleOnPatrol`, `Min/MaxPatrolIdleTime` (ms).
    pub idle_chance: u32,
    pub idle_ms: (i32, i32),
    /// `Min/MaxTimeBeforeRoam` (ms); a monster with patrol points waits 0.
    pub roam_wait_ms: (i32, i32),
    /// `RoamDistance`, `MinRoamDistance`.
    pub roam_dist: f32,
    pub min_roam: f32,
    /// `WanderDistance`, `MinWanderDistance`.
    pub wander_dist: f32,
    pub min_wander: f32,
    /// `TeleportToLeaderDistance` (0: never).
    pub teleport: f32,
    /// `LeaderBehavior`, `LeaderDistance`, `MaxFollowers`.
    pub leader: LeaderBehavior,
    pub leader_dist: f32,
    pub max_followers: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            roam: false,
            idle_chance: 20,
            idle_ms: (2000, 5000),
            roam_wait_ms: (3000, 10000),
            roam_dist: 0.0,
            min_roam: 0.0,
            wander_dist: 0.0,
            min_wander: 0.0,
            teleport: 0.0,
            leader: LeaderBehavior::Never,
            leader_dist: 0.0,
            max_followers: 4,
        }
    }
}

/// `LeaderBehavior`: when the monster calls the monsters near it to follow it
/// (`ControllerMonsterState::CallForFollowers`). No record of the game sets it; the leaders in the data are
/// summoners, whose minions `JoinMe` them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LeaderBehavior {
    #[default]
    Never,
    OnPatrol,
    WhenEnemyIsSeen,
    OnDamage,
}

impl LeaderBehavior {
    pub fn parse(s: &str) -> Self {
        match s {
            "LeadOnPatrol" => Self::OnPatrol,
            "LeadWhenEnemyIsSeen" => Self::WhenEnemyIsSeen,
            "LeadOnDamage" => Self::OnDamage,
            _ => Self::Never,
        }
    }
}

/// The controller's idle-time states (`ControllerMonster::RegisterStates`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// "Idle": stands, waits for the roam timer.
    #[default]
    Idle,
    Patrol,
    /// "Return": walks (runs) back to the spawn point.
    Return,
    /// "Roam": walks to a random spot around the spawn point.
    Roam,
    /// "Wander": walks to a random spot around the leader.
    Wander,
    /// "WanderPause": stands between two wanders.
    WanderPause,
    /// "DefendLeader": stands next to the leader.
    DefendLeader,
    /// "FollowLeader": runs to the spot next to the leader.
    FollowLeader,
}

/// A monster's leader as the AI sees it this frame (a dead leader whose corpse is still there is `alive: false`).
#[derive(Clone, Copy, Debug)]
pub struct Leader {
    pub entity: Entity,
    pub pos: Vec3,
    pub radius: f32,
    pub alive: bool,
}

pub struct Cx<'a, 'w, 's> {
    pub id: Entity,
    pub slots: &'a crate::melee::Slots<'a>,
    pub me: Vec3,
    pub radius: f32,
    /// `MaxYViewDistance`.
    pub view_y: f32,
    /// `None` when the monster has no leader or the object is gone.
    pub leader: Option<Leader>,
    /// `Character::LostSlot` was sent since the last update.
    pub lost: bool,
    pub nav: &'a Nav,
    pub points: &'a Points<'w, 's>,
    /// Shared with the melee states.
    pub scratch: &'a RefCell<gd_sim::nav::PathScratch>,
}

/// The controller's patrol data and idle-time state machine (`ControllerMonsterState{Idle,Patrol,Return,Roam,
/// Wander,WanderPause,DefendLeader,FollowLeader}`). A transition runs the new state's `OnBegin` at once.
///
/// * Idle: a monster whose leader is alive goes to "DefendLeader"; else its timer is
///   `MinTimeBeforeRoam`..`MaxTimeBeforeRoam` ms, or 0 with patrol points. When `RoamBehavior` is `Roam` and the
///   timer ran out, a monster with a leader goes to "Wander", one with patrol points to "Patrol", else to "Roam".
///   A failed start re-enters Idle at once and runs again at the next update.
/// * Roam: a random spot ([`pick_spot`]) between twice its radius and `RoamDistance` from the spawn point, at
///   least `MinRoamDistance` from where it stands, walked; there (or when the path failed) `rand() % 100 < 20`
///   roams again, else Idle.
/// * Wander: the same around the leader (radii sum .. `WanderDistance`, `MinWanderDistance`); at the end of the
///   path "WanderPause" stands 3..9999 ms, then wanders again. No leader object: Idle. Farther from the leader
///   than `WanderDistance` (surface to surface): "Return", which runs to the leader.
/// * DefendLeader: stands; a leader gone or dead: Idle. Farther than `TeleportToLeaderDistance` (0: never):
///   `TeleportToLeader`. Every 300..699 ms it asks for its slot next to the leader and runs there
///   ("FollowLeader") when it is more than 2.5 cm away; after 3000..9999 ms it wanders.
/// * FollowLeader: runs to the slot when it is farther than the monster's radius and reachable, else
///   "DefendLeader"; arriving or failing: "DefendLeader"; the leader dying: Idle (via "Return"). Teleports as
///   DefendLeader does. Losing its slot (`LostSlot`) sends it to "DefendLeader" at its next update. The slot is
///   the leader's defence slot (`RequestDefenseSlot`); with none free the monster stays.
/// * Return: a monster with a leader goes "FollowLeader"; one at its spawn point (`AlreadyThere` 0.025) is Idle;
///   else it runs to the spawn point, Idle at the end; with no way there it makes its spot the spawn point.
/// * Patrol (`OnBegin`): the closest loaded patrol point (3-D) becomes the current one; none, or no way there:
///   "Return". Having started a leg, `LeadOnPatrol` calls its followers. `MoveToCurrentPatrolPoint`: from the
///   current point on, the first loaded point more than 1 m away that the monster can path to (a point with a
///   `radius` is entered at a random spot inside it); the current index is not advanced by the skipped points.
///   `shouldRun` picks run speed, else a walk. Reaching it: with `ChanceToIdleOnPatrol` % the monster stands for
///   `MinPatrolIdleTime`..`MaxPatrolIdleTime` ms, then moves on to `(current + 1) % count`. While patrolling the
///   monster's home (for `InPursuitRange`) is where it stands; in any other state it is the spawn point (or the
///   leader's spot).
///
/// Fighting is the monster AI's: it leaves these states when it gets an enemy, goes back ("Return") afterwards
/// and is Idle again.
#[derive(Clone, Debug, Default)]
pub struct PatrolState {
    /// `ControllerMonster::GetPatrolPoints`.
    pub points: Vec<Uid>,
    pub settings: Settings,
    /// `GetCurrentPatrolPoint`.
    pub current: usize,
    pub mode: Mode,
    /// The current patrol leg is a run.
    pub run: bool,
    /// Standing at a patrol point (seconds).
    pub wait: f32,
    /// `Monster::GetLeader` (`JoinMe`).
    pub leader: Option<Entity>,
    /// `GetFollowersJoined`: `CallForFollowers` joins, never reset.
    pub followers_joined: u32,
    /// `CallForFollowers` is due ([`call_for_followers`]).
    pub call: bool,
    /// `GetHomePosition`.
    pub leader_pos: Option<Vec3>,
    /// Idle's timer, WanderPause's timer (seconds).
    timer: f32,
    /// DefendLeader's slot-check and wander timers (seconds).
    defend: (f32, f32),
    /// `OnBegin` of the first state (Idle) ran.
    started: bool,
}

/// `ControllerMonster::FindRandomPoint`: up to 5 tries at a random spot `lo..hi` from `center` (uniform in
/// distance, not in area), on the ground, within `max_dy` of the monster's height (0: any), farther than
/// `min_move` from it and reachable. Returns the path there.
#[allow(clippy::too_many_arguments)]
pub fn pick_spot(
    me: Vec3,
    center: Vec3,
    lo: f32,
    hi: f32,
    min_move: f32,
    max_dy: f32,
    nav: &Nav,
    scratch: &mut gd_sim::nav::PathScratch,
) -> Option<Vec<Vec3>> {
    for _ in 0..5 {
        let r = gd_game_core::rng::f32() * (hi - lo) + lo;
        let a = gd_game_core::rng::f32() * std::f32::consts::TAU;
        let q = center + Vec3::new(a.sin() * r, 2.0, a.cos() * r);
        let Some(c) = nav.0.nearest(q.to_array(), 1.5) else {
            continue;
        };
        let p = Vec3::from(nav.0.pos(c));
        if (max_dy == 0.0 || (p.y - me.y).abs() < max_dy)
            && p.distance(me) > min_move
            && let Some(found) = nav.0.find_path(
                scratch,
                me.to_array(),
                p.to_array(),
                crate::nav::PATH_BUDGET,
            )
        {
            return Some(found.into_iter().skip(1).map(Vec3::from).collect());
        }
    }
    None
}

/// `rand() % n + base` (ms) as seconds.
fn rand_ms(base: i32, n: i32) -> f32 {
    (base + gd_game_core::rng::i32(0..n)) as f32 / 1000.0
}

impl PatrolState {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            ..Default::default()
        }
    }

    /// The Patrol state is running (`UsePathPositionAsHome`).
    pub fn active(&self) -> bool {
        self.mode == Mode::Patrol
    }

    /// The monster walks (a `WalkAction`) rather than runs in this state.
    pub fn walking(&self) -> bool {
        match self.mode {
            Mode::Patrol => !self.run,
            Mode::Roam | Mode::Wander => true,
            _ => false,
        }
    }

    /// `ReplacePatrolPoints`: new points, current point 0 (the state goes on).
    pub fn replace(&mut self, points: Vec<Uid>) {
        self.points = points;
        self.current = 0;
    }

    /// The monster left its state for a fight, a quest command or a teleport; the caller sets the path home.
    pub fn interrupt(&mut self) {
        self.mode = Mode::Return;
        self.wait = 0.0;
    }

    /// `Monster::JoinMe`: the monster follows `leader` (its idle states react from the next time it is Idle).
    pub fn join(&mut self, leader: Entity) {
        self.leader = Some(leader);
    }

    /// `RequestDefenseSlot`: the spot next to the leader, None without a free slot.
    fn slot(cx: &Cx, l: &Leader) -> Option<Vec3> {
        cx.slots.defense(l.entity, cx.id, cx.nav)
    }

    /// `ControllerMonsterStateIdle::OnBegin`.
    fn enter_idle(&mut self, cx: &Cx, path: &mut Vec<Vec3>) {
        self.mode = Mode::Idle;
        path.clear();
        if cx.leader.is_some_and(|l| l.alive) {
            return self.enter_defend(cx, path);
        }
        self.timer = if self.points.is_empty() {
            let (lo, hi) = self.settings.roam_wait_ms;
            (gd_game_core::rng::f32() * (hi - lo) as f32 + lo as f32).max(0.0) as i32 as f32
                / 1000.0
        } else {
            0.0
        };
    }

    /// `ControllerMonsterStateDefendLeader::OnBegin`.
    fn enter_defend(&mut self, cx: &Cx, path: &mut Vec<Vec3>) {
        if cx.leader.is_none_or(|l| !l.alive) {
            return self.enter_idle(cx, path);
        }
        self.mode = Mode::DefendLeader;
        path.clear();
        self.defend = (rand_ms(300, 400), rand_ms(3000, 7000));
    }

    /// `ControllerMonsterStateFollowLeader::OnBegin`.
    fn enter_follow(&mut self, cx: &Cx, path: &mut Vec<Vec3>) {
        let Some(l) = cx.leader.filter(|l| l.alive) else {
            return self.enter_idle(cx, path);
        };
        let Some(slot) = Self::slot(cx, &l) else {
            return self.enter_defend(cx, path);
        };
        if slot.distance(cx.me) > cx.radius
            && let Some(found) = cx.nav.0.find_path(
                &mut cx.scratch.borrow_mut(),
                cx.me.to_array(),
                slot.to_array(),
                crate::nav::PATH_BUDGET,
            )
        {
            *path = found.into_iter().skip(1).map(Vec3::from).collect();
            self.mode = Mode::FollowLeader;
            return;
        }
        self.enter_defend(cx, path);
    }

    /// `ControllerMonsterStateReturn::OnBegin` (the non-fighting part).
    pub fn enter_return(&mut self, cx: &Cx, home: &mut Vec3, path: &mut Vec<Vec3>) {
        self.wait = 0.0;
        if self.leader.is_some() {
            return self.enter_follow(cx, path);
        }
        if cx.me.distance(*home) <= 0.025 {
            return self.enter_idle(cx, path);
        }
        match cx.nav.0.find_path(
            &mut cx.scratch.borrow_mut(),
            cx.me.to_array(),
            home.to_array(),
            crate::nav::PATH_BUDGET,
        ) {
            Some(p) => {
                *path = p.into_iter().skip(1).map(Vec3::from).collect();
                self.mode = Mode::Return;
            }
            None => {
                // "Monster %s can't return home.": its spot becomes home.
                *home = cx.me;
                self.enter_idle(cx, path);
            }
        }
    }

    /// `ControllerMonsterStateRoam::StartRoaming`.
    fn start_roaming(&mut self, cx: &Cx, home: Vec3, path: &mut Vec<Vec3>) -> bool {
        let s = &self.settings;
        match pick_spot(
            cx.me,
            home,
            cx.radius * 2.0,
            s.roam_dist,
            s.min_roam,
            cx.view_y,
            cx.nav,
            &mut cx.scratch.borrow_mut(),
        ) {
            Some(p) => {
                debug!("roam: monster at {:.0?} -> {:.0?}", cx.me, p.last());
                *path = p;
                self.mode = Mode::Roam;
                true
            }
            None => false,
        }
    }

    /// `ControllerMonsterStateWander::OnBegin`.
    fn enter_wander(&mut self, cx: &Cx, path: &mut Vec<Vec3>) {
        let Some(l) = cx.leader else {
            return self.enter_idle(cx, path);
        };
        let s = &self.settings;
        match pick_spot(
            cx.me,
            l.pos,
            cx.radius + l.radius,
            s.wander_dist,
            s.min_wander,
            cx.view_y,
            cx.nav,
            &mut cx.scratch.borrow_mut(),
        ) {
            Some(p) => {
                debug!(
                    "wander: monster at {:.0?} -> {:.0?} around its leader",
                    cx.me,
                    p.last()
                );
                *path = p;
                self.mode = Mode::Wander;
            }
            None => self.enter_idle(cx, path),
        }
    }

    /// `ControllerMonsterStateWander::CheckLeaderAndWanderDistance`: true when
    /// it left the state.
    fn check_wander(&mut self, cx: &Cx, home: &mut Vec3, path: &mut Vec<Vec3>) -> bool {
        let Some(l) = cx.leader else {
            self.enter_idle(cx, path);
            return true;
        };
        if l.pos.distance(cx.me) - (cx.radius + l.radius) <= self.settings.wander_dist {
            return false;
        }
        self.enter_return(cx, home, path);
        true
    }

    /// `ControllerMonsterStatePatrol::MoveToCurrentPatrolPoint`: sets `path`
    /// toward the first usable point; false when none is.
    fn move_to_current(
        &mut self,
        me: Vec3,
        path: &mut Vec<Vec3>,
        nav: &Nav,
        scratch: &RefCell<gd_sim::nav::PathScratch>,
        points: &Points,
    ) -> bool {
        let n = self.points.len();
        if self.current >= n {
            return false;
        }
        let mut k = self.current;
        loop {
            if let Some(p) = lookup(points, self.points[k % n]).filter(|p| p.pos.distance(me) > 1.0)
            {
                let mut target = p.pos;
                if p.radius > 0.01 {
                    // `NavManager::FindRandomPointInRadius`.
                    for _ in 0..8 {
                        let a = gd_game_core::rng::f32() * std::f32::consts::TAU;
                        let r = p.radius * gd_game_core::rng::f32().sqrt();
                        let q = p.pos + Vec3::new(a.cos() * r, 2.0, a.sin() * r);
                        if let Some(c) = nav.0.nearest(q.to_array(), 1.5) {
                            target = Vec3::from(nav.0.pos(c));
                            break;
                        }
                    }
                }
                // `Character::CanMoveTo`: a path exists.
                if let Some(found) = nav.0.find_path(
                    &mut scratch.borrow_mut(),
                    me.to_array(),
                    target.to_array(),
                    crate::nav::PATH_BUDGET,
                ) {
                    *path = found.into_iter().skip(1).map(Vec3::from).collect();
                    self.run = p.run;
                    return true;
                }
            }
            k += 1;
            if k % n == self.current {
                return false;
            }
        }
    }

    /// `ControllerMonsterStatePatrol::OnBegin`.
    fn enter_patrol(&mut self, cx: &Cx, home: &mut Vec3, path: &mut Vec<Vec3>) {
        // `GetClosest`: loaded points only.
        let closest = self
            .points
            .iter()
            .enumerate()
            .filter_map(|(i, &u)| lookup(cx.points, u).map(|p| (i, p.pos.distance_squared(cx.me))))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|c| c.0);
        if let Some(c) = closest {
            self.current = c;
            if self.move_to_current(cx.me, path, cx.nav, cx.scratch, cx.points) {
                self.mode = Mode::Patrol;
                debug!(
                    "patrol: monster at {:.0?} starts at point {c} of {} (run {})",
                    cx.me,
                    self.points.len(),
                    self.run
                );
                if self.settings.leader == LeaderBehavior::OnPatrol {
                    self.call = true;
                }
                return;
            }
        }
        self.enter_return(cx, home, path);
    }

    /// `MoveToNextPatrolPoint`.
    fn next(&mut self, cx: &Cx, home: &mut Vec3, path: &mut Vec<Vec3>) {
        self.current = (self.current + 1) % self.points.len().max(1);
        debug!("patrol: monster at {:.0?} -> point {}", cx.me, self.current);
        if !self.move_to_current(cx.me, path, cx.nav, cx.scratch, cx.points) {
            self.enter_return(cx, home, path);
        }
    }

    /// Runs the state for a monster that is not fighting. `path` is its current walk (empty at the end of a path);
    /// `home` the spawn point. Returns where `TeleportToLeader` puts the monster.
    pub fn update(
        &mut self,
        dt: f32,
        cx: &Cx,
        home: &mut Vec3,
        path: &mut Vec<Vec3>,
    ) -> Option<Vec3> {
        if !self.started {
            self.started = true;
            self.enter_idle(cx, path);
        }
        match self.mode {
            Mode::Idle => {
                // `StartedRoaming`.
                if !self.settings.roam {
                    return None;
                }
                self.timer -= dt;
                if self.timer >= 0.0 {
                    return None;
                }
                if self.leader.is_some() {
                    self.enter_wander(cx, path);
                } else if self.points.is_empty() {
                    if !self.start_roaming(cx, *home, path) {
                        self.enter_idle(cx, path);
                    }
                } else {
                    self.enter_patrol(cx, home, path);
                }
            }
            Mode::Patrol => {
                if self.wait > 0.0 {
                    self.wait -= dt;
                    if self.wait <= 0.0 {
                        self.wait = 0.0;
                        self.next(cx, home, path);
                    }
                } else if path.is_empty() {
                    // `EndOfPathReached`.
                    let (lo, hi) = self.settings.idle_ms;
                    let wait_ms = if (gd_game_core::rng::u32(0..100)) >= self.settings.idle_chance {
                        0
                    } else if (hi - lo) as u32 > 0x7FFD || hi < lo {
                        lo
                    } else {
                        lo + gd_game_core::rng::i32(0..=(hi - lo))
                    };
                    if wait_ms != 0 {
                        self.wait = wait_ms as f32 / 1000.0;
                    } else {
                        self.next(cx, home, path);
                    }
                }
            }
            Mode::Return => {
                // `EndOfPathReached`.
                if path.is_empty() {
                    self.enter_idle(cx, path);
                }
            }
            Mode::Roam => {
                // `EndOfPathReached`.
                if path.is_empty()
                    && (gd_game_core::rng::u32(0..100) >= 20
                        || !self.start_roaming(cx, *home, path))
                {
                    self.enter_idle(cx, path);
                }
            }
            Mode::Wander => {
                if !self.check_wander(cx, home, path) && path.is_empty() {
                    self.mode = Mode::WanderPause;
                    // `rand() % -9997 + 3`.
                    self.timer = rand_ms(3, 9997);
                }
            }
            Mode::WanderPause => {
                if !self.check_wander(cx, home, path) {
                    self.timer -= dt;
                    if self.timer < 0.0 {
                        self.enter_wander(cx, path);
                    }
                }
            }
            Mode::DefendLeader | Mode::FollowLeader => {
                // `ControllerMonsterStateFollowLeader::LostSlot`: "DefendLeader" before any other check;
                // DefendLeader has no handler.
                if self.mode == Mode::FollowLeader && cx.lost {
                    self.enter_defend(cx, path);
                    return None;
                }
                let Some(l) = cx.leader.filter(|l| l.alive) else {
                    self.enter_idle(cx, path);
                    return None;
                };
                if self.settings.teleport != 0.0 && l.pos.distance(cx.me) > self.settings.teleport {
                    // `TeleportToLeader`: onto its slot, then Idle.
                    if let Some(at) = Self::slot(cx, &l) {
                        let cx = Cx { me: at, ..*cx };
                        self.enter_idle(&cx, path);
                        return Some(at);
                    }
                }
                if self.mode == Mode::FollowLeader {
                    if path.is_empty() {
                        // `EndOfPathReached`.
                        self.enter_defend(cx, path);
                    }
                    return None;
                }
                self.defend.0 -= dt;
                if self.defend.0 < 0.0 {
                    if Self::slot(cx, &l).is_some_and(|s| s.distance_squared(cx.me) > 0.000_625) {
                        self.enter_follow(cx, path);
                        return None;
                    }
                    self.defend.0 = rand_ms(300, 400);
                }
                self.defend.1 -= dt;
                if self.defend.1 < 0.0 {
                    self.enter_wander(cx, path);
                }
            }
        }
        None
    }
}

/// `ControllerMonsterState::CallForFollowers`: the monster calls the living friends (leaderless or led alike)
/// within `LeaderDistance` (and `MaxYViewDistance` in height), nearest-first, to join it until `MaxFollowers`
/// calls have been made over its lifetime (each call counts). A monster without a faction pack counts as a
/// friend.
fn call_for_followers(
    mut q: Query<
        (Entity, &Transform, &mut crate::combat::MonsterAi),
        Without<gd_game_core::combat::Dead>,
    >,
) {
    let calls: Vec<(Entity, Vec3)> = q
        .iter()
        .filter(|(_, _, ai)| ai.patrol.call)
        .map(|(e, tf, _)| (e, tf.translation))
        .collect();
    for (leader, pos) in calls {
        let Ok((_, _, mut lai)) = q.get_mut(leader) else {
            continue;
        };
        lai.patrol.call = false;
        let (dist, max, mut joined) = (
            lai.patrol.settings.leader_dist,
            lai.patrol.settings.max_followers,
            lai.patrol.followers_joined,
        );
        let view_y = lai.view_y();
        let lfac = lai.faction.clone();
        let near: Vec<Entity> = q
            .iter()
            .filter(|(e, tf, ai)| {
                *e != leader
                    && tf.translation.distance(pos) <= dist
                    && (view_y == 0.0 || (tf.translation.y - pos.y).abs() <= view_y)
                    && match (&lfac, &ai.faction) {
                        (Some(a), Some(b)) => a.is_friend(b),
                        _ => true,
                    }
            })
            .map(|(e, _, _)| e)
            .collect();
        for e in near {
            if joined >= max {
                break;
            }
            if let Ok((_, _, mut ai)) = q.get_mut(e) {
                ai.patrol.join(leader);
            }
            joined += 1;
            debug!("follower {e} joins leader {leader}");
        }
        if let Ok((_, _, mut lai)) = q.get_mut(leader) {
            lai.patrol.followers_joined = joined;
        }
    }
}
