//! Movement along nav paths: the player walks its [`MovePath`] at the run animation's root-motion speed (as in the
//! engine), or charges at the Charge animation's root-motion speed toward a skill's target.
//!
//! A plain move request (a click or a held button on the ground, [`move_request`]) follows
//! `ControllerPlayerState::DefaultRequestMoveAction`:
//! * a point less than 1 m (0.5 m while the player is already moving, `CharacterMovementManager.moving`) from the
//!   player, horizontally, is ignored: the current move goes on;
//! * `Player::FindPath` (→ `CharacterMovementManager::FindPath`) asks for a full path
//!   (no partial result). It is refused (result 2) when the path is at least 15 m long and longer than
//!   `maxMoveToRatio` (the player record, 2.5 in `malepc01.dbr` / `femalepc01.dbr`; `Player::Load` →
//!   `Player.maxMoveToRatio`) times
//!   the straight 3D distance, and (result 3) when its first leg points away from the target by more than the
//!   minimum dot (−1 in Classic movement: never; 0 with `movementType` above 0, `Player::FindPath`);
//! * a refused or failed path (2 or 4) to a point above the player is retried once at the point moved back along the
//!   camera ray to the player's height (the cursor over a ledge or a wall above);
//! * else the player walks to `NavManager::FindStraightMovePoint` (the farthest point the straight line toward the
//!   target reaches, [`crate::nav::straight_move_point`]), when that is more than 2.5 cm away and pathable;
//! * nothing at all: the request is dropped (the player keeps what it did).
//!
//! The held button (EXE `InGameUI::HandleMouseEvent`): a press that issues an interaction request (an
//! NPC, an item, a fixed actor: `SelectPrimaryAction` clears `ControllerPlayer::SetNewMouseCommand`) is the whole
//! command: `HandleActionFromMouse` issues nothing more until the button is released ([`MouseCommand`]), so the
//! walk to the object is not replaced by moves to the ground under the cursor. A press the UI takes (a window,
//! a HUD button) leaves no command pending (`InGameUI.commandPending` cleared; only a world press or the held Move
//! key sets it), so
//! the button held after it, also once the cursor leaves the window or the window closed, issues nothing either.
//!
//! A request while the player walks a plain move ([`request_move`], `ControllerPlayerStateMoveTo`): the
//! state keeps the horizontal direction from the player to its path's end, taken when it began (`OnBegin`,
//! `ControllerPlayerStateMoveTo.startDirection`). `RequestMoveAction`: when the direction from the player to the new
//! point has a dot above 0.9999998 with it, the point only becomes the pending destination
//! (`ControllerPlayerStateMoveTo.pendingDestination`), no path is searched; else the
//! request is a plain one. `OnUpdate`: once the player is within 2.5 m (3D) of the move's end
//! (`CharacterMovementManager.target`, the `MoveTo` target), the pending point is taken and requested as a plain move
//! ([`pending_moves`]). So a held button over a cursor that stays ahead of a straight run searches no path until
//! the run nears its end. `EndOfPathReached` ends the state (`Idle`): a pending point not taken by then is dropped.
//!
//! The crowd (Engine.dll crowd update `Crowd::Update`; the rules and parameters are in `gd_sim::crowd`):
//! every character is a crowd agent ([`CrowdAgent`], given by [`crowd_begin`], which also marks where each one
//! starts the frame). The agents' groups and masks decide who meets whom: the player only hostile monsters, a
//! hostile monster everyone, pets and allied monsters only hostile monsters, NPCs hostile monsters and each other;
//! a `forceCollision` monster (or a Boss / Quest / SuperBoss) is never pushed, a `forceNoCollision` one is met by
//! no one, and a dashing character (the warm-ups of path charges, leaps, blinks, weapon charges and evades set
//! `Character.crowdFlags`, the "no collision" flag 0x800; the player's ChargeToUseSkill) is neither pushed nor steered.
//! - The player's planning ([`follow_path`], flag 4 of its agent): with an agent that moved last frame, a velocity is
//!   sampled against the hostile monsters within max(8 r, r + the largest radius) that stand between it and its
//!   target (`gd_sim::crowd::avoid`); the player keeps its speed (flag 0x40) and only turns. Its step then fails
//!   the move (reason 2, `ControllerPlayerStateMoveTo::PathFailed`: Idle) when it touches a neighbour
//!   that stands on the move's end, or touches one within its radius of the end: a run onto a monster ends at it.
//! - A moving monster's or pet's separation ([`crowd_begin`], weight 1.1, `Monster::CrowdAgentCreated`): from where
//!   the agents stand when the frame begins, the push away from the other monsters and pets it meets
//!   (`gd_sim::crowd::separation`) is added to its desired velocity before it moves, and the sum is clamped to its
//!   speed (`combat::advance_steered`, the integration): its AI and the crowd agree on one displacement per frame.
//! - The occupied target for monsters and pets ([`crowd_update`], the same rule as the player's, after their AI
//!   moved them): the step is undone and the path fails (reason 2). A steered step is put back on the mesh
//!   ([`onto_nav`]).
//! - A knocked-down character (`crate::ragdoll::KnockedDown`) is out of the crowd while down
//!   (`Character::ExecuteKnockdown` → `NavManager::RemoveObject`; back at `ReleaseKnockdown`).
//! - The collision pass ([`crowd_update`], `gd_sim::crowd::depenetration`): each overlapping pair that
//!   `CrowdAgentDepenetrate` lets push moves the agent by (r_a + r_b − d) / d × 0.35, averaged; a moving agent is
//!   pushed by every neighbour unless that one moves too with a higher priority (the player has 2, the rest 1), a
//!   still one only by a still neighbour of higher priority. So monsters do not shove each other while standing,
//!   a running monster or pet slides off the ones it meets, a running player off hostile monsters, and a standing
//!   player shoves the standing monsters it overlaps out. A monster playing its Spawn animation is not pushed. All
//!   pushes come from the same positions and apply together.
//! - The projection ([`onto_nav`]): an agent moved off the walkable mesh is put at the nearest point on it (only
//!   its height when it is over the mesh); one whose record has `allowedOffNavmesh` stays where it is pushed.
//! - The stuck check ([`crowd_update`], `gd_sim::crowd::Stuck`): an agent whose run carried it less than 1.1 m/s
//!   for over 0.4 s (held back by others) fails its move (reason 3).
//! - A failed path: the player's move ends (`ControllerPlayerStateMoveTo::PathFailed`: Idle); a
//!   monster's or pet's [`CrowdAgent::take_path_failed`] runs its state's `PathFailed` at its next AI update
//!   (`combat::monster_ai`, `pets::ai`, `melee`).
//! - A spinning player (`Skill_AttackRadiusSpin`) has mask 8: only the group-8 monsters push it or are avoided.
//!
//! [`crowd_update`] finds each agent's neighbours through a `gd_sim::grid::Grid` (cell `gd_sim::crowd::grid_cell`)
//! built per pass from that pass's positions; the candidates are filtered with the engine's tests and come by index,
//! so the result and its order are the brute-force scan's.
//!
//! The radius is `Actor::GetRadius` ([`gd_game_core::combat::Combatant::radius`], `gd_game_core::combat::get_radius`).
//! PORT: the crowd's corridor (Detour path corridor, `movePosition` each frame, the visibility shortcut of flag
//! 0x400) is the port's string-pulled [`MovePath`] (taut already); the smooth steering of flag 0x200 is set for no
//! character. A steered monster or pet arrives at a path corner once it is within one frame's step of it (then it
//! takes the corner exactly). The player's avoidance takes its neighbours' last velocities as their desired ones.
//! "Moving" is the run / walk / charge state (with a path for monsters and pets; the separation reads the
//! state the frame begins with); the neighbour query's height window is 2 m. The off-mesh flag of the leap / evade
//! action states (19 / 21) is moot: the dashing characters are not pushed.
//! Every mover's speed is its move clip's root motion at the caller speed it plays at (`combat::run_speed`,
//! `combat::charge_move`, `combat::pace_speed`; the slot's own motion before the clip plays).
//!
//! The frame step: [`MovementPlugin`] caps the gameplay delta at 100 ms and integrates each frame in one step, no
//! substeps. A long step cannot carry a character through a wall: every mover walks straight legs between
//! waypoints the nav mesh gave it (paths, `nav::straight_move_point`, charge / leap legs; a corner reached within
//! the step is taken and the rest of the step goes on along the next leg), each step's end is grounded on the
//! mesh (an off-mesh point ends a leap or charge), crowd pushes are put back on the mesh ([`onto_nav`]), and
//! projectiles test the whole segment they flew. What a long step changes is only how coarse the crowd's
//! reactions are (≤ 0.1 s × the speed per step), which the cap bounds.
use crate::nav::Nav;
use bevy::prelude::*;
use bevy::time::Virtual;
use gd_game_core::character::{CharState, Character, MoveOrder, QueuedAction};
use gd_game_core::player::{Player, PlayerSet};
use std::time::Duration;

pub struct MovementPlugin;

impl Plugin for MovementPlugin {
    fn build(&self, app: &mut App) {
        // A hitch integrates at most 100 ms of gameplay: Bevy's default 250 ms cap
        // teleports a character up to ~2 m and resolves crowds against stale positions.
        // PORT: the engine's own clamp was not located (its time helpers show none);
        // the loop callbacks a long step spans are all emitted (`gd_sim::anim::advance`).
        if let Some(mut time) = app.world_mut().get_resource_mut::<Time<Virtual>>() {
            time.set_max_delta(Duration::from_secs_f32(0.1));
        }
        app.add_systems(
            Update,
            (
                pending_moves.before(MovementStage::Follow),
                follow_path.in_set(MovementStage::Follow),
            )
                .in_set(PlayerSet),
        )
        .add_systems(
            Update,
            (
                crowd_begin
                    .run_if(resource_exists::<gd_game_core::data::GameData>)
                    .before(PlayerSet),
                crowd_update.after(gd_game_core::combat::CombatSet),
            ),
        );
    }
}

/// No actions (stunned, asleep, knocked down, frozen, petrified). Fear / confusion mark it too as the
/// AI-exclusion flag (`combat::cc_states` drives those monsters' own move; they are not [`Rooted`]).
#[derive(Component)]
pub struct Stunned;

/// No movement (trapped, immobilized; the action-blocking stun states insert it alongside [`Stunned`] —
/// fear / confusion's AI-exclusion mark does not).
#[derive(Component)]
pub struct Rooted;

#[derive(Component, Default)]
pub struct MovePath {
    pub points: Vec<Vec3>,
    /// Re-path cooldown while the button is held.
    pub repath: f32,
    /// The plain move these points walk (`ControllerPlayerStateMoveTo`, module docs); valid while the path still
    /// ends at its `dest` (another mover replacing the points ends it).
    plain: Option<PlainMove>,
}

#[derive(Clone, Copy, Debug)]
struct PlainMove {
    /// Horizontal unit direction from the player to `dest` when the move began
    /// (`ControllerPlayerStateMoveTo.startDirection`).
    dir: Vec3,
    dest: Vec3,
    /// The pending destination (`ControllerPlayerStateMoveTo.pendingDestination`) and the rules it is requested with.
    pending: Option<(Vec3, MoveRules)>,
}

/// The inputs of a plain move request besides its points ([`move_request`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoveRules {
    /// `maxMoveToRatio` of the player record.
    pub max_ratio: f32,
    /// The first leg's minimum dot: −1 in Classic movement, 0 with `movementType` above 0.
    pub min_dot: f32,
    /// The camera position, for the retry of a point above the player.
    pub camera: Option<Vec3>,
}

impl MovePath {
    fn plain_mut(&mut self) -> Option<&mut PlainMove> {
        let last = *self.points.last()?;
        self.plain.as_mut().filter(|p| p.dest == last)
    }
}

/// `ControllerPlayerStateMoveTo::RequestMoveAction` (module docs): while a plain move runs and the
/// direction to `to` is the one it began with, `to` becomes its pending destination; else
/// [`default_move_request`]. True when the request was taken (the held button's 10 ms repeat).
pub fn request_move(
    path: &mut MovePath,
    nav: &gd_sim::nav::NavWorld,
    scratch: &mut gd_sim::nav::PathScratch,
    from: Vec3,
    to: Vec3,
    rules: MoveRules,
) -> bool {
    let dir = (to - from).with_y(0.0).normalize_or_zero();
    if let Some(plain) = path.plain_mut()
        && dir.dot(plain.dir) > 0.999_999_8
    {
        plain.pending = Some((to, rules));
        return true;
    }
    default_move_request(path, nav, scratch, from, to, rules)
}

/// `ControllerPlayerState::DefaultRequestMoveAction` for a plain move: [`move_request`]'s path becomes the walk
/// (a new `MoveTo` state, its direction taken from the player to the path's end). False when dropped.
pub fn default_move_request(
    path: &mut MovePath,
    nav: &gd_sim::nav::NavWorld,
    scratch: &mut gd_sim::nav::PathScratch,
    from: Vec3,
    to: Vec3,
    rules: MoveRules,
) -> bool {
    let moving = !path.points.is_empty();
    let Some(points) = move_request(
        nav,
        scratch,
        from,
        to,
        moving,
        rules.max_ratio,
        rules.min_dot,
        rules.camera,
    ) else {
        return false;
    };
    path.plain = points.last().map(|&dest| PlainMove {
        dir: (dest - from).with_y(0.0).normalize_or_zero(),
        dest,
        pending: None,
    });
    path.points = points;
    true
}

/// Applies a plain move order ([`crate::player_orders::PlayerOrder::Move`], queued
/// while busy as [`MoveOrder`]): to the object's coordinates when there is one, else
/// the ground point; sets the held button's repeat timer. True when a request was
/// taken. The one place the order's rules are built (`apply_orders`, `follow_path`).
pub fn apply_move_order(
    path: &mut MovePath,
    nav: &gd_sim::nav::NavWorld,
    scratch: &mut gd_sim::nav::PathScratch,
    data: &gd_game_core::data::GameData,
    record: &str,
    from: Vec3,
    order: MoveOrder,
) -> bool {
    let moving_now = !path.points.is_empty();
    let rules = MoveRules {
        max_ratio: data
            .db
            .get(record)
            .and_then(|r| r.f32("maxMoveToRatio"))
            .unwrap_or(MAX_MOVE_TO_RATIO),
        min_dot: -1.0,
        camera: Some(order.camera),
    };
    let taken = order
        .to
        .is_some_and(|to| request_move(path, nav, scratch, from, to, rules))
        || order
            .ground
            .is_some_and(|to| request_move(path, nav, scratch, from, to, rules));
    path.repath = crate::player_orders::command_wait(taken, moving_now);
    taken
}

/// `ControllerPlayerStateMoveTo::OnUpdate` (module docs): within 2.5 m of the move's end, its
/// pending destination is requested. PORT: its camera-ray retry uses the camera position of the held request.
pub fn pending_moves(
    nav: Res<Nav>,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    mut players: Query<(&Transform, &mut MovePath), With<Player>>,
) {
    for (tf, mut path) in &mut players {
        let from = tf.translation;
        let Some(plain) = path.plain_mut() else {
            continue;
        };
        if plain.pending.is_none() || plain.dest.distance(from) >= 2.5 {
            continue;
        }
        let (to, rules) = plain.pending.take().expect("checked");
        default_move_request(&mut path, &nav.0, &mut scratch, from, to, rules);
    }
}

/// `ControllerPlayerStateMoveToUseSkill`: the pending cast of a
/// skill whose target is out of reach. The player walks to the target
/// (`ControllerAI::MoveTo` with the skill) and `EndOfPathReached` /
/// `PathFailed` start `UseSkill` with the clicked point. The state ends
/// (`Idle`) when the target dies (`OnUpdate`), and a move
/// request, stun or a new valid skill request replaces it
/// (`RequestMoveAction`, `BeginStun`, `RequestSkillAction`).
/// `ControllerPlayerStateChargeToUseSkill` (same vtable: it shares
/// `EndOfPathReached` / `PathFailed` / `CloseEnoughToUseSkill` and ends the
/// same way in `OnUpdate`) differs in `OnBegin` only:
/// `ControllerAI::MoveTo` with animation type 7 (Charge) and speed factor 1.0
/// (unit speed, as `Pace::Charge`), the `charge` flag here.
///
/// The move is a `MoveToAction` carrying the skill: `MoveToAction::Execute`
/// calls the skill's `WarmUpStart` and, when
/// it returns true, sets the warm-up run speed flag (`Character.warmUpRunSpeed`,
/// `ForceSpeedUpdate`); `Finish` calls `WarmUpEnd` with the
/// target and clears the flag. Only weapon skills warm up
/// (`Skill_AttackWeapon::WarmUpStart`, see
/// `casting::WarmUp`); the others' `WarmUpStart` is an empty stub.
#[derive(Component)]
pub struct MoveToUseSkill {
    /// ChargeToUseSkill rather than MoveToUseSkill.
    pub charge: bool,
    /// The skill's warm-up started with the move (energy paid, cooldown
    /// running): its run speed bonus and the absolute cap, both %.
    pub warm_up: Option<(f32, f32)>,
    pub skill: String,
    pub point: Vec3,
    pub target: Option<Entity>,
    /// The end of the path set for this walk (another mover changes it).
    pub dest: Option<Vec3>,
}

impl MoveToUseSkill {
    /// The walk is a charge ([`Self::charge`]): unit speed, Charge animation.
    pub fn charging(&self) -> bool {
        self.charge
    }
}

/// A hostile monster as the player's crowd neighbour (avoidance and the occupied-target check).
struct Neighbour {
    pos: Vec3,
    radius: f32,
    vel: Vec3,
}

/// The player walks its [`MovePath`]. Held in place ([`Rooted`]: trapped, immobilized, every [`Stunned`]) the
/// move is over: `ControllerPlayerState::DefaultBeginTrapAction` sets the "Trapped" state
/// (so the `MoveTo` state is left; `ControllerPlayerStateMoveToUseSkill::BeginTrap` runs its
/// `ControllerPlayerState::PreTrap` first, then the same), whose `OnBegin` is a `TakeTrapAction`
/// (`Character::ExecuteTrap`: action state 13) and whose `EndTrap` goes to "Idle", not back to the move. PORT:
/// the pending skill move is dropped whole (its state's `PreTrap` was not decompiled); the trapped pose is Idle.
/// A move queued while busy ([`gd_game_core::character::QueuedAction`]) runs here once the action allows it: at the
/// clip's `AllowInterrupt` point (the recovery cancel, its remainder carried as the attack-delay) or when the
/// action ends; an evade's recovery is cancelable once its dash (`StopJump`) is over.
#[allow(clippy::type_complexity)]
pub fn follow_path(
    mut commands: Commands,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    time: Res<Time>,
    nav: Res<Nav>,
    data: Res<gd_game_core::data::GameData>,
    mut players: Query<
        (
            Entity,
            &mut Transform,
            &mut MovePath,
            &mut Character,
            &mut crate::player_orders::InteractTarget,
            Option<&MoveToUseSkill>,
            Option<&gd_game_core::combat::Combatant>,
            Option<&mut CrowdAgent>,
            Has<Rooted>,
        ),
        With<Player>,
    >,
    monsters: Query<
        (
            &Transform,
            &Character,
            &gd_game_core::combat::Combatant,
            Option<&CrowdAgent>,
        ),
        (
            With<gd_game_core::monsters::Monster>,
            Without<Player>,
            Without<gd_game_core::combat::Dead>,
            Without<crate::combat::Allied>,
        ),
    >,
) {
    let dt = time.delta_secs();
    for (e, mut tf, mut path, mut ch, mut interact, pending, cb, mut agent, rooted) in &mut players
    {
        if rooted && ch.state != CharState::Death {
            if !path.points.is_empty() {
                path.points.clear();
            }
            if pending.is_some() {
                commands
                    .entity(e)
                    .try_remove::<(MoveToUseSkill, gd_game_core::casting::WarmPaid)>();
            }
            if matches!(ch.state, CharState::Run | CharState::Charge) {
                ch.state = CharState::Idle;
            }
            continue;
        }
        if ch.state == CharState::Death {
            continue;
        }
        // A started attack / cast / channel / evade / stun supersedes the pending interaction (the controller leaves
        // `MoveToFixedItem` / `MoveToNpc`); a new order drops it too (`apply_orders`).
        if matches!(
            ch.state,
            CharState::Attack
                | CharState::Cast
                | CharState::Channel
                | CharState::Channel2
                | CharState::Spin
                | CharState::Evade
                | CharState::Stunned
        ) {
            interact.0 = None;
        }
        let busy = matches!(
            ch.state,
            CharState::Attack
                | CharState::Cast
                | CharState::Evade
                | CharState::Pickup
                | CharState::KnockedDown
                | CharState::GettingUp
        );
        // A queued move runs when the action allows it: free, or the recovery
        // cancelable at the clip's `AllowInterrupt` point (an evade: its dash over) —
        // `AttackAction::QueryActionPermission` permission 4; the cancelled
        // remainder becomes the attack-delay so the cadence is kept.
        if busy && !ch.can_act() {
            continue;
        }
        if let Some(QueuedAction::Move(order)) =
            ch.queued.take_if(|q| matches!(q, QueuedAction::Move(_)))
        {
            if matches!(ch.state, CharState::Attack | CharState::Cast) {
                ch.attack_delay = ch.attack_delay.max(ch.remaining_ms());
            }
            // The evade's components go with its state (`evade::evading`).
            if busy {
                ch.state = CharState::Run;
            }
            apply_move_order(
                &mut path,
                &nav.0,
                &mut scratch,
                &data,
                &ch.record,
                tf.translation,
                order,
            );
        } else if busy {
            continue;
        }
        // ChargeToUseSkill (`OnBegin`): `ControllerAI::MoveTo` with the Charge animation at speed factor 1,
        // a warmed-up charge at the warmed-up run speed; the Charge clip's root motion carries the player, a
        // character without the slot does not move (`combat::charge_move`, the charges' one rule).
        let charging = pending.is_some_and(|m| m.charging());
        // The movers' one speed rule (`combat::run_speed`): the move clip's root motion.
        let (speed, charge_state) = if charging {
            let warm = pending.and_then(|m| m.warm_up);
            let (speed, state, rate) = crate::combat::charge_move(&ch, CharState::Charge, warm);
            if state != CharState::Idle {
                ch.move_rate = Some(rate);
            }
            (speed, state)
        } else {
            (crate::combat::run_speed(&ch), CharState::Run)
        };
        if let Some(a) = agent.as_mut() {
            a.speed = speed;
        }
        let agent = agent.as_deref();
        let start = tf.translation;
        let end = path.points.last().copied();
        let r = cb.map_or(0.5, |cb| cb.radius);
        // The crowd neighbours: hostile monsters (the player's mask) within the planning range; none while charging
        // (no collision).
        let neighbours: Vec<Neighbour> = if charging || path.points.is_empty() {
            Vec::new()
        } else {
            // Spinning, the player's mask is 8: only the group-8 (`forceCollision`, boss) monsters.
            let spinning = agent.as_ref().is_some_and(|a| a.spinning);
            let all = monsters
                .iter()
                .filter(|(.., a)| !a.is_some_and(|a| a.ghost))
                .filter(|(.., a)| !spinning || a.is_some_and(|a| a.forced))
                .map(|(t, _, b, a)| Neighbour {
                    pos: t.translation,
                    radius: b.radius,
                    vel: a.map_or(Vec3::ZERO, |a| a.vel),
                });
            let all: Vec<_> = all.collect();
            let max_r = all.iter().map(|n| n.radius).fold(r, f32::max);
            let range = gd_sim::crowd::plan_range(r, max_r);
            all.into_iter()
                .filter(|n| {
                    (n.pos.y - start.y).abs() <= gd_sim::crowd::HEIGHT * 0.5
                        && n.pos.distance_squared(start) < range * range
                })
                .collect()
        };
        let mut budget = speed * dt;
        let mut moving = false;
        // The avoidance (flag 4 of the player's agent, the planning pass): with an agent that moved last frame
        // and a path corner beyond this frame's step, the velocity sampled against the neighbours between the
        // player and its target; the player keeps its speed (flag 0x40) and only turns.
        let avoided = (|| {
            let target = *path.points.last()?;
            let corner = path.points[0];
            if neighbours.is_empty()
                || agent.is_none_or(|a| a.vel.length_squared() == 0.0)
                || budget <= 0.0
            {
                return None;
            }
            let to = (corner - start).with_y(0.0);
            let d = to.length();
            if d <= budget {
                return None;
            }
            let reach = target.distance(start) - r - gd_sim::crowd::ARRIVE_DIST;
            let obstacles: Vec<_> = neighbours
                .iter()
                .filter(|n| {
                    n.pos.distance(start) - r - n.radius < reach
                        && target.distance(n.pos) - gd_sim::crowd::ARRIVE_DIST > n.radius
                })
                .map(|n| gd_sim::crowd::Obstacle {
                    pos: xz(n.pos),
                    radius: n.radius,
                    vel: xz(n.vel),
                    // PORT: the neighbour's desired velocity is its last velocity (monsters plan after the player).
                    dvel: xz(n.vel),
                })
                .collect();
            if obstacles.is_empty() {
                return None;
            }
            let dvel = to / d * speed;
            let vel = agent.map_or(Vec3::ZERO, |a| a.vel.with_y(0.0));
            let v = gd_sim::crowd::avoid(xz(start), r, speed, xz(vel), xz(dvel), &obstacles);
            let v = Vec3::new(v[0], 0.0, v[1]);
            if v.length_squared() <= 0.0001 {
                return Some((start, Vec3::ZERO));
            }
            let dir = v.normalize();
            let to = start + dir * budget;
            onto_nav(&nav.0, to, agent.is_some_and(|a| a.off_mesh)).map(|to| (to, dir))
        })();
        if let Some((to, dir)) = avoided {
            tf.translation = to;
            // The corner is passed once this frame's step reaches it.
            if (path.points[0] - to).with_y(0.0).length() <= budget {
                path.points.remove(0);
            }
            if dir != Vec3::ZERO {
                ch.turn.moved(dir * speed);
            }
            moving = true;
            budget = 0.0;
        }
        while budget > 0.0 && !path.points.is_empty() {
            let target = path.points[0];
            let to = Vec3::new(
                target.x - tf.translation.x,
                0.0,
                target.z - tf.translation.z,
            );
            let d = to.length();
            if d <= budget {
                tf.translation.x = target.x;
                tf.translation.z = target.z;
                path.points.remove(0);
                budget -= d;
            } else {
                tf.translation += to / d * budget;
                budget = 0.0;
            }
            if d > 1e-3 {
                // `CrowdAgentMoved`: the character turns toward its velocity (`crate::turning`).
                ch.turn.moved(to / d * speed);
            }
            moving = true;
        }
        // The occupied target (the integration, reason 2): a step onto a neighbour that stands at the move's end,
        // or one that touches a neighbour within the player's radius of the end, fails the path; the player does
        // not move (`ControllerPlayerStateMoveTo::PathFailed`: Idle).
        if moving
            && let Some(end) = end
            && gd_sim::crowd::occupied_target(
                tf.translation.to_array(),
                r,
                end.to_array(),
                neighbours.iter().map(|n| (n.pos.to_array(), n.radius)),
            )
        {
            tf.translation = start;
            path.points.clear();
            moving = false;
        }
        if let Some(g) = nav
            .0
            .ground([tf.translation.x, tf.translation.y + 0.5, tf.translation.z])
        {
            tf.translation.y = g;
        }
        // The move's state. Without a move only a move state goes back to Idle: another action's state (a held
        // channel or whirlwind, a flinch, a scripted clip) is its requester's, left at its own "End" or release. A
        // move that starts replaces it, and a channel's skill stops with it (`MoveToAction::Execute`:
        // `SkillManager::StopCurrentSkill`).
        let moves = moving || !path.points.is_empty();
        let want = match (moves, charging) {
            (false, _) => CharState::Idle,
            (true, false) => CharState::Run,
            (true, true) => charge_state,
        };
        let in_move = matches!(
            ch.state,
            CharState::Idle | CharState::Run | CharState::Walk | CharState::Charge
        );
        if ch.state != want && (moves || in_move) {
            if matches!(
                ch.state,
                CharState::Channel | CharState::Channel2 | CharState::Spin
            ) {
                commands
                    .entity(e)
                    .try_remove::<gd_game_core::casting::Channel>();
            }
            ch.state = want;
        }
    }
}

/// `maxMoveToRatio` of the player records (`records/creatures/pc/{male,female}pc01.dbr`), used when the record has
/// none.
pub const MAX_MOVE_TO_RATIO: f32 = 2.5;

/// `CharacterMovementManager::FindPath` checks the ratio only from this path length on (its `a10` is false for
/// `Player::FindPath`).
const RATIO_MIN_LENGTH: f32 = 15.0;

/// The rules a plain move request follows (module docs): the path to walk, or `None` when the request is dropped.
/// `moving`: the player walks already (the smaller dead zone). `min_dot`: −1 in Classic movement, 0 with
/// `movementType` above 0. `camera`: the camera position, for the retry of a point above the player.
#[allow(clippy::too_many_arguments)]
pub fn move_request(
    nav: &gd_sim::nav::NavWorld,
    scratch: &mut gd_sim::nav::PathScratch,
    from: Vec3,
    to: Vec3,
    moving: bool,
    max_ratio: f32,
    min_dot: f32,
    camera: Option<Vec3>,
) -> Option<Vec<Vec3>> {
    let dead = if moving { 0.5 } else { 1.0 };
    if (to - from).with_y(0.0).length_squared() < dead * dead {
        return None;
    }
    let mut find = |to: Vec3| -> Result<Vec<Vec3>, u8> {
        if to.distance_squared(from) <= 0.025 * 0.025 {
            return Err(0);
        }
        let p = nav
            .find_path_strict(
                scratch,
                from.to_array(),
                to.to_array(),
                crate::nav::PATH_BUDGET_LONG,
            )
            .ok_or(4u8)?;
        let mut pts: Vec<Vec3> = p.into_iter().skip(1).map(Vec3::from).collect();
        // Detour's path ends at the target itself; the cell path at its cell's centre.
        if let Some(last) = pts.last_mut()
            && (*last - to).with_y(0.0).length() < 0.5
            && nav.ground([to.x, to.y + 0.5, to.z]).is_some()
        {
            *last = Vec3::new(to.x, last.y, to.z);
        }
        let len: f32 = std::iter::once(from)
            .chain(pts.iter().copied())
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| w[0].distance(w[1]))
            .sum();
        let straight = (to - from).length();
        if let Some(first) = pts.first() {
            let (a, b) = ((*first - from).with_y(0.0), (to - from).with_y(0.0));
            if a.normalize_or_zero().dot(b.normalize_or_zero()) < min_dot {
                return Err(3);
            }
        }
        if max_ratio > 0.0
            && len >= RATIO_MIN_LENGTH
            && straight > 0.0
            && len / straight > max_ratio
        {
            return Err(2);
        }
        Ok(pts)
    };
    match find(to) {
        Ok(p) => return Some(p),
        Err(0) => return None,
        Err(2 | 4) if from.y < to.y => {
            if let Some(cam) = camera {
                let dir = (to - cam).normalize_or_zero();
                if dir.y.abs() > 1e-4 {
                    // Back along the camera ray until the player's height.
                    let back = to + dir * ((from.y - to.y) / dir.y);
                    if let Ok(p) = find(back) {
                        return Some(p);
                    }
                }
            }
        }
        Err(_) => {}
    }
    find(crate::nav::straight_move_point(nav, from, to)).ok()
}

/// The crowd agent follows a path (agent state 2). PORT: the run / walk / charge state.
fn crowd_moving(ch: &Character) -> bool {
    matches!(
        ch.state,
        CharState::Run | CharState::Walk | CharState::Charge
    )
}

fn xz(v: Vec3) -> gd_sim::crowd::V2 {
    [v.x, v.z]
}

/// Where the crowd puts an agent moved to `to` (`Crowd::Update`'s end, the nav interface's
/// `NavInterface::FindNearestPoly` / `NavInterface::FindNearestPoint`):
/// over the mesh (`findNearestPoly` with the default extents) only its height is taken; else the nearest point on
/// the mesh within 10 m horizontally; with neither the agent stays at `to`, off the mesh (PORT: it keeps its spot,
/// None). An agent allowed off the mesh (flag 0x20, [`CrowdAgent::off_mesh`]) is not projected. PORT: the mesh is
/// the open walkable cells: the nearest one in rings of 0.25 m within a step's height ([`gd_sim::nav::CLIMB`]),
/// `to` clamped into it.
fn onto_nav(nav: &gd_sim::nav::NavWorld, to: Vec3, off_mesh: bool) -> Option<Vec3> {
    if off_mesh {
        return Some(to);
    }
    let open = |p: Vec3| {
        nav.locate([p.x, p.y + 0.5, p.z])
            .filter(|&c| !nav.blocked(nav.pos(c)))
            .filter(|&c| (nav.pos(c)[1] - to.y).abs() <= gd_sim::nav::CLIMB)
    };
    if open(to).is_some() {
        return Some(to);
    }
    const STEP: f32 = 0.25;
    for ring in 1..=(10.0 / STEP) as i32 {
        let mut best: Option<(f32, Vec3)> = None;
        for i in -ring..=ring {
            for (dx, dz) in [(i, -ring), (i, ring), (-ring, i), (ring, i)] {
                let q = to + Vec3::new(dx as f32 * STEP, 0.0, dz as f32 * STEP);
                let Some(c) = open(q) else { continue };
                let (centre, half) = (Vec3::from(nav.pos(c)), nav.cell_size(c) * 0.5 - 0.001);
                let p = Vec3::new(
                    to.x.clamp(centre.x - half, centre.x + half),
                    centre.y,
                    to.z.clamp(centre.z - half, centre.z + half),
                );
                let d = (p - to).with_y(0.0).length_squared();
                if best.is_none_or(|(b, _)| d < b) {
                    best = Some((d, p));
                }
            }
        }
        if let Some((_, p)) = best {
            return Some(p);
        }
    }
    None
}

/// A character's crowd agent (module docs): the crowd's own per-agent state.
#[derive(Component, Debug, Clone, Default)]
pub struct CrowdAgent {
    /// Where the agent stood when the frame began (`CrowdAgent.frameStart`).
    pub start: Vec3,
    /// Its velocity over the last frame (m/s).
    pub vel: Vec3,
    /// A `forceCollision` monster (or a Boss / Quest / SuperBoss): group 8 too, never pushed
    /// (`Monster.forceCollision`).
    pub forced: bool,
    /// A `forceNoCollision` monster: group 0 (`Monster.forceNoCollision`).
    pub ghost: bool,
    /// Its top speed this frame (`CrowdAgentParams.maxSpeed`; set by the movers: the player's path following,
    /// `combat::advance_steered`; 0 otherwise).
    pub speed: f32,
    /// The separation velocity [`crowd_begin`] computed for this frame (zero: none), added to the desired
    /// velocity before the agent moves (`combat::advance_steered`).
    pub(crate) separation: Vec3,
    /// This frame's move was steered (not along the path exactly): the crowd puts it back on the mesh.
    pub(crate) steered: bool,
    /// The record's `allowedOffNavmesh` (`Character::Load` → `Character.allowedOffNavmesh`, `GetAllowedOffNavmesh`):
    /// flag 0x20, the crowd
    /// never puts it back on the mesh.
    pub off_mesh: bool,
    /// Spinning (`Skill_AttackRadiusSpin`, a [`gd_game_core::casting::Channel`] of that class): `Character.spinning`, mask 8
    /// (`gd_sim::crowd::group::SPIN_MASK`). Set every frame by [`crowd_begin`].
    pub spinning: bool,
    /// The stuck check (`CrowdAgent.stuck`).
    stuck: gd_sim::crowd::Stuck,
    /// The crowd failed the agent's path (state 6, reasons 2 and 3) and its controller's `PathFailed` is due
    /// ([`CrowdAgent::take_path_failed`]).
    failed: bool,
}

impl CrowdAgent {
    /// `Character::CrowdAgentError`: the crowd failed the path (the agent's move stops) and
    /// the current controller state's `PathFailed` runs; true once per failure. The monster and pet
    /// AIs take it at their next update (the crowd runs after them).
    pub fn take_path_failed(&mut self) -> bool {
        std::mem::take(&mut self.failed)
    }
}

/// Gives every character its [`CrowdAgent`], marks where each one starts the frame, and computes the moving
/// monsters' and pets' separation from those positions (module docs; `Crowd::Update`'s planning pass: the
/// neighbours within max(8 r, r + the largest radius) that carry a separation weight too).
#[allow(clippy::type_complexity)]
pub fn crowd_begin(
    mut commands: Commands,
    data: Res<gd_game_core::data::GameData>,
    mut agents: Query<
        (
            (
                Entity,
                &Transform,
                &Character,
                Option<&mut CrowdAgent>,
                Option<&gd_game_core::monsters::Monster>,
                Option<&gd_game_core::casting::Channel>,
            ),
            (
                Option<&gd_game_core::combat::Combatant>,
                Has<Player>,
                Has<crate::pets::Pet>,
                Has<crate::combat::Allied>,
                Has<gd_game_core::combat::Dead>,
                Has<crate::ragdoll::KnockedDown>,
            ),
            (
                Has<gd_game_core::casting::PathCharge>,
                Has<gd_game_core::casting::Leap>,
                Has<gd_game_core::casting::Charge>,
                Has<crate::evade::Evading>,
                Option<&MoveToUseSkill>,
            ),
        ),
        With<Character>,
    >,
) {
    use gd_sim::crowd;
    // The agents with a separation weight (monsters and pets) as the crowd sees them.
    let mut seps: Vec<(Entity, crowd::Body)> = Vec::new();
    let mut max_r = 0.0f32;
    for (
        (e, tf, ch, agent, monster, channel),
        (cb, player, pet, allied, dead, down),
        (pc, leap, charge, evade, mts),
    ) in &mut agents
    {
        let spinning = channel.is_some_and(|c| {
            data.db
                .get(&c.skill)
                .is_some_and(|r| r.class == "Skill_AttackRadiusSpin")
        });
        if let Some(mut a) = agent {
            a.start = tf.translation;
            a.speed = 0.0;
            a.spinning = spinning;
            a.steered = false;
            a.separation = Vec3::ZERO;
            let Some(cb) = cb.filter(|_| !dead && !down) else {
                continue;
            };
            max_r = max_r.max(cb.radius);
            if monster.is_some() || pet {
                // "No collision" (flag 0x800): the dashes are not steered.
                let dash =
                    pc || leap || charge || evade || mts.is_some_and(MoveToUseSkill::charging);
                let (group, mask) = crowd_group(player, true, allied || pet, &a);
                seps.push((
                    e,
                    crowd::Body {
                        pos: xz(tf.translation),
                        y: tf.translation.y,
                        radius: cb.radius,
                        group,
                        mask,
                        priority: 1,
                        moving: crowd_moving(ch),
                        collides: !dash,
                        pushable: true,
                        dir: [0.0; 2],
                    },
                ));
            }
            continue;
        }
        // `Monster::Load`: `forceCollision` (or classification 3..5), `forceNoCollision`.
        let rec = monster.and_then(|m| data.db.get(&m.record));
        let (forced, ghost) = rec.map_or((false, false), |r| {
            let boss = matches!(
                r.str("monsterClassification"),
                Some("Boss" | "Quest" | "SuperBoss")
            );
            (r.bool("forceCollision") || boss, r.bool("forceNoCollision"))
        });
        let off_mesh = data
            .db
            .get(&ch.record)
            .is_some_and(|r| r.bool("allowedOffNavmesh"));
        commands.entity(e).try_insert(CrowdAgent {
            start: tf.translation,
            forced,
            ghost,
            off_mesh,
            spinning,
            ..default()
        });
    }
    if !seps.iter().any(|s| s.1.moving && s.1.collides) {
        return;
    }
    let grid = gd_sim::grid::Grid::new(crowd::grid_cell(max_r), seps.iter().map(|s| s.1.pos));
    let mut near: Vec<usize> = Vec::new();
    let mut out: Vec<(Entity, Vec3)> = Vec::new();
    for (i, (e, me)) in seps.iter().enumerate() {
        if !me.moving || !me.collides {
            continue;
        }
        let range = crowd::plan_range(me.radius, max_r);
        grid.near(me.pos, range, &mut near);
        let others = near
            .iter()
            .filter(|&&j| j != i && me.meets(&seps[j].1, range))
            .map(|&j| (seps[j].1.pos, seps[j].1.radius));
        if let Some(s) =
            crowd::separation(me.pos, me.radius, range, crowd::MONSTER_SEPARATION, others)
        {
            out.push((*e, Vec3::new(s[0], 0.0, s[1])));
        }
    }
    for (e, s) in out {
        if let Ok(((_, _, _, Some(mut a), _, _), ..)) = agents.get_mut(e) {
            a.separation = s;
        }
    }
}

/// A character's crowd group and mask (module docs, `gd_sim::crowd::group`).
fn crowd_group(player: bool, monster: bool, allied: bool, agent: &CrowdAgent) -> (u32, u32) {
    use gd_sim::crowd::group;
    let (mut g, mut m) = if player {
        group::PLAYER
    } else if monster && (!allied || agent.forced) {
        group::HOSTILE
    } else if monster {
        group::FRIENDLY
    } else {
        group::OTHER
    };
    if monster && agent.forced {
        g |= group::FORCED;
    }
    if monster && agent.ghost {
        g = 0;
    }
    // Spinning: mask 8, which the `Monster` override rewrites.
    if agent.spinning && !monster {
        m = group::SPIN_MASK;
    }
    (g, m)
}

/// [`crowd_update`]'s per-frame scratch: the allocations are reused, the contents rebuilt every frame.
#[derive(Default)]
pub struct CrowdScratch {
    infos: Vec<Info>,
    /// The neighbour candidates of a query (`gd_sim::grid`: by index, the engine's order).
    near: Vec<usize>,
    blocked: Vec<usize>,
    bodies: Vec<gd_sim::crowd::Body>,
    grid: gd_sim::grid::Grid,
}

/// One crowd agent's snapshot for [`crowd_update`].
struct Info {
    e: Entity,
    start: Vec3,
    body: gd_sim::crowd::Body,
    /// Its move this frame was steered (`combat::advance_steered`): put back on the mesh.
    steered: bool,
    off_mesh: bool,
    /// A monster's or pet's path end while it follows one (the player's own checks are `follow_path`'s).
    target: Option<Vec3>,
    /// Where its AI's move put it.
    pre: Vec3,
    failed: bool,
}

/// The crowd update after every mover has moved (module docs): the monsters' and pets' occupied targets, the
/// steered steps put back on the mesh, the collision pass, the stuck checks and each agent's velocity.
#[allow(clippy::type_complexity)]
pub fn crowd_update(
    time: Res<Time>,
    nav: Res<Nav>,
    mut scratch: Local<CrowdScratch>,
    mut agents: Query<
        (
            (
                Entity,
                &mut Transform,
                &mut Character,
                &gd_game_core::combat::Combatant,
                &mut CrowdAgent,
            ),
            (
                Has<Player>,
                Has<gd_game_core::monsters::Monster>,
                Has<crate::pets::Pet>,
                Has<crate::combat::Allied>,
                Option<&crate::combat::MonsterAi>,
                Option<&crate::pets::Pet>,
            ),
            (
                Has<gd_game_core::casting::PathCharge>,
                Has<gd_game_core::casting::Leap>,
                Has<gd_game_core::casting::Charge>,
                Has<crate::evade::Evading>,
                Option<&MoveToUseSkill>,
                Option<&mut MovePath>,
            ),
        ),
        (
            Without<gd_game_core::combat::Dead>,
            Without<crate::ragdoll::KnockedDown>,
        ),
    >,
) {
    use gd_sim::crowd;
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let s = &mut *scratch;
    s.infos.clear();
    for (
        (e, tf, ch, cb, agent),
        (player, monster, pet, allied, ai, pet_c),
        (pc, leap, charge, evade, mts, _),
    ) in &agents
    {
        // "No collision" (`Character.crowdFlags`, flag 0x800): the dashes' warm-ups (path charge, leap, blink, weapon
        // charge, evade) and the player's ChargeToUseSkill.
        let dash = pc || leap || charge || evade || mts.is_some_and(MoveToUseSkill::charging);
        let (group, mask) = crowd_group(player, monster || pet, allied || pet, agent);
        let moving = crowd_moving(ch);
        // `Monster::CrowdAgentDepenetrate`: never a `forceCollision` monster, nor one whose Spawn
        // animation (`EnableSpawnAnimation`, `Character.spawnAnimation`) still plays (the current entry's type 19,
        // the Spawn
        // animation type; the flag clears once it is something else).
        let spawning =
            ch.state == CharState::Scripted && ch.scripted == gd_sim::anim::AnimType::Spawn;
        s.infos.push(Info {
            e,
            start: agent.start,
            body: crowd::Body {
                pos: xz(tf.translation),
                y: tf.translation.y,
                radius: cb.radius,
                group,
                mask,
                priority: if player { crowd::PLAYER_PRIORITY } else { 1 },
                moving,
                collides: !dash,
                pushable: !((monster && agent.forced) || ((monster || pet) && spawning)),
                dir: {
                    let v = agent.vel.with_y(0.0).normalize_or_zero();
                    [v.x, v.z]
                },
            },
            steered: agent.steered,
            off_mesh: agent.off_mesh,
            target: if player || !moving {
                None
            } else {
                ai.and_then(|a| a.path_target())
                    .or_else(|| pet_c.and_then(|p| p.path_target()))
            },
            pre: tf.translation,
            failed: false,
        });
    }
    let max_r = s.infos.iter().map(|i| i.body.radius).fold(0.0, f32::max);
    let cell = crowd::grid_cell(max_r);
    // The occupied target (the integration, reason 2): a monster's or pet's step that touches a neighbour standing on
    // its path's end, or that touches one within its radius of the end, fails the path; it does not move. Its combat
    // target counts like any neighbour: the attack points are the skill range + both radii (`GetTargetDistance`)
    // from the enemy, the same `GetRadius` as the crowd's.
    if s.infos
        .iter()
        .any(|a| a.target.is_some() && a.body.collides)
    {
        s.grid.rebuild(cell, s.infos.iter().map(|i| i.body.pos));
        for (i, a) in s.infos.iter().enumerate() {
            let Some(t) = a.target.filter(|_| a.body.collides) else {
                continue;
            };
            let range = crowd::plan_range(a.body.radius, max_r);
            let me = [a.body.pos[0], a.body.y, a.body.pos[1]];
            s.grid.near(a.body.pos, range, &mut s.near);
            let others = s
                .near
                .iter()
                .map(|&j| (j, &s.infos[j]))
                .filter(|&(j, b)| j != i && a.body.meets(&b.body, range))
                .map(|(_, b)| ([b.body.pos[0], b.body.y, b.body.pos[1]], b.body.radius));
            if crowd::occupied_target(me, a.body.radius, t.to_array(), others) {
                s.blocked.push(i);
            }
        }
    }
    for i in s.blocked.drain(..) {
        let a = &mut s.infos[i];
        debug!("crowd: {} occupied target at {:.1?}", a.e, a.start);
        a.failed = true;
        a.target = None;
        a.body.moving = false;
        a.body.pos = xz(a.start);
        a.body.y = a.start.y;
        if let Ok((mut c, ..)) = agents.get_mut(a.e) {
            c.1.translation = a.start;
        }
    }
    // A steered step may leave the walkable mesh: put it back (the integration's projection).
    for a in s.infos.iter_mut().filter(|a| a.steered && !a.failed) {
        let at = Vec3::new(a.body.pos[0], a.body.y, a.body.pos[1]);
        if let Some(to) = onto_nav(&nav.0, at, a.off_mesh)
            && to != at
        {
            a.body.pos = xz(to);
            if let Ok((mut c, ..)) = agents.get_mut(a.e) {
                c.1.translation.x = to.x;
                c.1.translation.z = to.z;
            }
        }
    }
    // The collision pass.
    s.bodies.clear();
    s.bodies.extend(s.infos.iter().map(|i| i.body));
    for (info, push) in s.infos.iter().zip(crowd::depenetration(&s.bodies)) {
        if push == [0.0; 2] {
            continue;
        }
        let to = Vec3::new(
            info.body.pos[0] + push[0],
            info.body.y,
            info.body.pos[1] + push[1],
        );
        if let Some(to) = onto_nav(&nav.0, to, info.off_mesh)
            && let Ok((mut c, ..)) = agents.get_mut(info.e)
        {
            c.1.translation = to;
        }
    }
    // The query's order is the same as in the first pass: `s.infos` is index-aligned.
    for (((e, mut tf, ch, _, mut agent), (player, ..), (.., path)), info) in
        agents.iter_mut().zip(&s.infos)
    {
        debug_assert_eq!(e, info.e);
        if (tf.translation.x != agent.start.x || tf.translation.z != agent.start.z)
            && let Some(g) =
                nav.0
                    .ground([tf.translation.x, tf.translation.y + 0.5, tf.translation.z])
        {
            tf.translation.y = g;
        }
        agent.vel = (tf.translation - agent.start) / dt;
        if info.failed {
            agent.failed = true;
            continue;
        }
        // The stuck check (reason 3). The player's: `ControllerPlayerStateMoveTo::PathFailed` → Idle.
        // A monster's or pet's top speed is the one its mover gave (`combat::advance_steered`), else how far its
        // move carried it this frame.
        let (moving, top) = if player {
            (
                crowd_moving(&ch) && path.as_ref().is_some_and(|p| !p.points.is_empty()),
                agent.speed,
            )
        } else if agent.speed > 0.0 {
            (info.target.is_some(), agent.speed)
        } else {
            (info.target.is_some(), (info.pre - info.start).length() / dt)
        };
        if !moving {
            continue;
        }
        let speed = agent.vel.length();
        if agent.stuck.update(top, speed, dt * 1000.0) {
            if player {
                if let Some(mut p) = path {
                    p.points.clear();
                }
            } else {
                debug!("crowd: {e} stuck at {:.1?}", tf.translation);
                agent.failed = true;
            }
        }
    }
}

/// The world mouse command gate (module docs, `ControllerPlayer::HandleActionFromMouse` with
/// `NewMouseCommand`, EXE `InGameUI::HandleMouseEvent` with the pending command `InGameUI.commandPending`):
/// after a press that started an
/// interaction, or a press the UI took, the held buttons issue no more commands (a move, an attack, a held skill)
/// until both are released. One for both buttons, as the engine's flags: grimdawn's `click_to_move` updates it each
/// frame, the world-click systems after it read it.
#[derive(bevy::prelude::Resource, Default, Debug, Clone, Copy)]
pub struct MouseCommand {
    consumed: bool,
}

impl MouseCommand {
    /// The frame's mouse event, before anything acts on it: `held` a world button (left / right) is down,
    /// `pressed` one went down now, `ui` the UI took the press. With neither button held the gate opens; a world
    /// press opens it; a press the UI took closes it (`InGameUI::HandleMouseEvent`: the UI's handler,
    /// `UIWindow::HandleMouseEvent`, takes the press and `InGameUI.commandPending` is cleared; the held events
    /// after it, with the cursor over the world, only set `CommandRepeated` and dispatch nothing, as
    /// `InGameUI.commandPending` is set again only by a world press or the held Move key).
    pub fn event(&mut self, held: bool, pressed: bool, ui: bool) {
        if !held {
            self.consumed = false;
        } else if pressed {
            self.consumed = ui;
        }
    }

    /// The press picked an object it interacts with (NPC, item, fixed actor: `SelectPrimaryAction` clears
    /// `NewMouseCommand`): the press is the whole command.
    pub fn interaction(&mut self) {
        self.consumed = true;
    }

    /// The held buttons may issue a command.
    pub fn open(&self) -> bool {
        !self.consumed
    }
}

/// Path following: what steers a path orders before `Follow`.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MovementStage {
    Follow,
}
