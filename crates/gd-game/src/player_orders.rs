//! Player orders: what the game's mouse and keyboard handlers decided, applied to the player.
//!
//! The input side ([`PlayerInputSet`]) writes [`PlayerOrder`]s; [`apply_orders`] applies them in order the same
//! frame, before the walk advances. An order is one of the engine's controller requests:
//! - the held button's repeat timer (`InGameUI.commandWait`, [`command_wait`]);
//! - a walk to an object the player interacts with (`MoveToNpc` / `MoveToFixedItem` / `MoveAndPickUpItem`), an
//!   attack on a foe (`SelectPrimaryAction`): one use of the primary attack ([`PrimaryUse`]);
//! - a plain move: to the object's coordinates when there is one, else the ground point under the cursor.
//!   PORT: the engine moves to the object's coordinates only; a request the rules drop falls back to the ground
//!   point;
//! - the keyboard movement (`InGameUI::Update`): the controller direction, a turn toward a look-at point
//!   (`RequestRotateAction`) and a hop 1.25 m ahead. PORT: the engine reaches the hop with a short plain move;
//!   here a strict nav path.
//!
//! One interaction target ([`InteractTarget`]) replaces the engine's controller state (`MoveToFixedItem` /
//! `MoveToNpc` / `MoveToItem`); the object's own use system takes it once in reach. Every new order drops it, as
//! a new primary action replaces the controller state; a started attack / cast / evade drops it too, and a
//! target that left the world expires.
//!
//! A move requested mid-swing / mid-cast / mid-evade waits in the character's pending-action slot
//! ([`gd_game_core::character::QueuedAction`], last wins) until the clip's `AllowInterrupt` point or its "End"
//! (the engine's one-slot action queue, `CharacterHandlerUpdate`).
use crate::movement::{MovePath, MovementStage, apply_move_order};
use crate::nav::Nav;
use bevy::ecs::entity::Entities;
use bevy::prelude::*;
use gd_game_core::character::Character;
use gd_game_core::combat::{Combatant, Dead};
use gd_game_core::data::GameData;
use gd_game_core::player::{ControllerDirection, Player, PlayerSet};

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerInputSet;

#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub enum PlayerOrder {
    /// The held button's repeat timer runs for this many seconds.
    Elapse(f32),
    Clear,
    /// Walks beside the object `0` and interacts with it once in reach, dropping the attack target and the
    /// pending interaction. A non-interactable object is only walked to.
    Interact(Entity),
    /// One use of the primary attack on `0` ([`PrimaryUse`]): walks into reach and swings / casts once. A new
    /// target stops the walk; the held button's repeat on the same one keeps it.
    Attack(Entity),
    /// A plain move to `to`, else to `ground`; `camera` is for the retry of a point above the player. Sets the
    /// repeat timer.
    Move {
        to: Option<Vec3>,
        ground: Option<Vec3>,
        camera: Vec3,
    },
    /// The keyboard's controller direction and length; `aim` in Keyboard Only.
    Controller {
        dir: Vec3,
        length: f32,
        aim: Option<Vec3>,
    },
    /// Turns toward a look-at point (`RequestRotateAction`).
    Turn(Vec3),
    /// The keyboard's hop to a point ahead (its height from the nav ground there).
    Hop(Vec3),
}

/// The wait before a held button (or the held Move key) issues its next command, in seconds
/// (`InGameUI::HandleMouseEvent`): 10 ms when the command was taken or the player is moving, else 50 ms.
pub fn command_wait(taken: bool, moving: bool) -> f32 {
    if taken || moving { 0.010 } else { 0.050 }
}

/// The player controller's pending interaction; the use systems route by the entity's component and clear it
/// on use.
#[derive(Component, Default)]
pub struct InteractTarget(pub Option<Entity>);

/// One use of the primary attack: `SelectPrimaryAction`'s UseSkill controller state ends once its skill was
/// used, so a click attacks once and only the held button's repeat attacks again. [`PlayerOrder::Attack`] arms
/// it; the swing / cast that starts on [`Combatant::target`] spends it, and so does any skill request that runs,
/// as a new controller state replaces the UseSkill one. A repeat while the same skill's clip runs spends it
/// too without starting another (`AttackAction::QueryActionPermission` drops the running skill's own request);
/// only a repeat past the clip's `AllowInterrupt` chains the next use. The target stays the running swing's and
/// is dropped when the action ends unarmed. A skill queued meanwhile goes before the next primary use.
#[derive(Component, Default)]
pub struct PrimaryUse(pub bool);

pub struct PlayerOrdersPlugin;

impl Plugin for PlayerOrdersPlugin {
    fn build(&self, app: &mut App) {
        app.register_required_components::<Player, InteractTarget>()
            .register_required_components::<Player, PrimaryUse>()
            .add_observer(ground_item_interactable)
            .add_observer(character_interactable)
            .add_message::<PlayerOrder>()
            .add_systems(
                Update,
                apply_orders
                    .in_set(PlayerSet)
                    .after(PlayerInputSet)
                    .before(MovementStage::Follow),
            );
    }
}

/// What an interact order walks to and uses: the walk goal's ring radius and its kind. Every interactable
/// object inserts it at spawn (ground items and characters by the observers below). An entity without it is
/// only walked to.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Interactable {
    pub radius: f32,
    pub kind: InteractKind,
}

impl Interactable {
    pub const fn fixed(radius: f32) -> Self {
        Self {
            radius,
            kind: InteractKind::Fixed,
        }
    }
}

/// How an interact walk is planned: an NPC's failed path falls back to a straight move to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractKind {
    Fixed,
    Npc,
}

/// A ground item's [`Interactable`]: its record's item radius (gold: 0).
fn ground_item_interactable(
    ev: On<Insert<crate::loot::GroundItem>>,
    items: Query<&crate::loot::GroundItem>,
    data: Res<GameData>,
    mut commands: Commands,
) {
    let Ok(gi) = items.get(ev.entity) else {
        return;
    };
    let r = gi
        .item
        .as_ref()
        .and_then(|i| data.db.get(&i.base_name))
        .map_or(0.0, crate::loot_filter::item_radius);
    commands.entity(ev.entity).insert(Interactable::fixed(r));
}

/// A non-player character's [`Interactable`] (only NPCs are interacted with): its record's radius.
fn character_interactable(
    ev: On<Insert<Character>>,
    chars: Query<&Character>,
    data: Res<GameData>,
    mut commands: Commands,
) {
    let Ok(ch) = chars.get(ev.entity) else {
        return;
    };
    if ch.is_player {
        return;
    }
    let radius = gd_game_core::combat::get_radius(
        data.db.get(&ch.record),
        gd_game_core::combat::ACTOR_RADIUS,
    );
    commands.entity(ev.entity).insert(Interactable {
        radius,
        kind: InteractKind::Npc,
    });
}

/// Routes an interact request (a click or the Interact key).
#[allow(clippy::too_many_arguments)]
fn apply_interact(
    e: Entity,
    from: Vec3,
    path: &mut MovePath,
    cb: &mut Combatant,
    interact: &mut InteractTarget,
    ch: &mut Character,
    nav: &Nav,
    objects: &Query<(&GlobalTransform, Option<&Interactable>)>,
    scratch: &mut gd_sim::nav::PathScratch,
) {
    cb.target = None;
    interact.0 = None;
    ch.queued = None;
    let Ok((gt, it)) = objects.get(e) else {
        return;
    };
    let at = gt.translation();
    let r = it.map_or(0.6, |i| i.radius);
    if it.is_some() {
        interact.0 = Some(e);
    }
    if let Some(p) = crate::chests::path_near(&nav.0, scratch, from, at, r) {
        path.points = p;
    } else if it.is_some_and(|i| i.kind == InteractKind::Npc) {
        // No nav path: a straight move, not a talk from afar that `run_conversations` closes at once
        // (PORT, `DefaultRequestMoveAction`'s `FindPath` fallback).
        path.points = vec![at];
    }
}

#[allow(clippy::type_complexity)]
#[allow(clippy::too_many_arguments)]
pub fn apply_orders(
    mut orders: MessageReader<PlayerOrder>,
    mut used: MessageReader<crate::controller_target::UseObject>,
    nav: Res<Nav>,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    data: Res<GameData>,
    entities: &Entities,
    mut players: Query<
        (
            &Transform,
            &mut MovePath,
            &mut Combatant,
            &mut InteractTarget,
            &mut Character,
            &mut ControllerDirection,
            &mut PrimaryUse,
        ),
        (With<Player>, Without<Dead>),
    >,
    objects: Query<(&GlobalTransform, Option<&Interactable>)>,
) {
    let Ok((tf, mut path, mut cb, mut interact, mut ch, mut controller, mut primary)) =
        players.single_mut()
    else {
        orders.clear();
        used.clear();
        return;
    };
    let from = tf.translation;
    if interact.0.is_some_and(|e| !entities.contains(e)) {
        interact.0 = None;
    }
    let clear = |cb: &mut Combatant, interact: &mut InteractTarget, primary: &mut PrimaryUse| {
        cb.target = None;
        interact.0 = None;
        primary.0 = false;
    };
    for order in orders.read() {
        match *order {
            PlayerOrder::Elapse(dt) => path.repath -= dt,
            PlayerOrder::Clear => clear(&mut cb, &mut interact, &mut primary),
            PlayerOrder::Interact(e) => {
                primary.0 = false;
                apply_interact(
                    e,
                    from,
                    &mut path,
                    &mut cb,
                    &mut interact,
                    &mut ch,
                    &nav,
                    &objects,
                    &mut scratch,
                );
            }
            PlayerOrder::Attack(e) => {
                if cb.target != Some(e) {
                    path.points.clear();
                }
                cb.target = Some(e);
                primary.0 = true;
                interact.0 = None;
                // A queued skill stays and runs before the attack: the action queue drops a primary attack
                // request while a skill waits (`AttackAction::QueryActionPermission`).
                ch.queued
                    .take_if(|q| matches!(q, gd_game_core::character::QueuedAction::Move(_)));
            }
            PlayerOrder::Move { to, ground, camera } => {
                interact.0 = None;
                let order = gd_game_core::character::MoveOrder { to, ground, camera };
                let busy = matches!(
                    ch.state,
                    gd_game_core::character::CharState::Attack
                        | gd_game_core::character::CharState::Cast
                        | gd_game_core::character::CharState::Evade
                        | gd_game_core::character::CharState::Pickup
                );
                if busy && !ch.can_act() {
                    // Runs at the clip's `AllowInterrupt` point or its "End".
                    ch.queued = Some(gd_game_core::character::QueuedAction::Move(order));
                } else {
                    if busy {
                        // Recovery cancel: the cancelled remainder becomes the attack-delay
                        // (`AttackAction::QueryActionPermission`).
                        if matches!(
                            ch.state,
                            gd_game_core::character::CharState::Attack
                                | gd_game_core::character::CharState::Cast
                        ) {
                            ch.attack_delay = ch.attack_delay.max(ch.remaining_ms());
                        }
                        ch.state = gd_game_core::character::CharState::Run;
                    }
                    apply_move_order(
                        &mut path,
                        &nav.0,
                        &mut scratch,
                        &data,
                        &ch.record,
                        from,
                        order,
                    );
                }
            }
            PlayerOrder::Controller { dir, length, aim } => {
                // A held direction drops the pending interaction (a release does not).
                if length > 0.0 {
                    interact.0 = None;
                }
                let mut next = *controller;
                next.dir = dir;
                next.length = length;
                if let Some(aim) = aim {
                    next.aim = aim;
                }
                controller.set_if_neq(next);
            }
            PlayerOrder::Turn(point) => ch.turn.look_at(point),
            PlayerOrder::Hop(dest) => {
                interact.0 = None;
                let y = nav
                    .0
                    .ground([dest.x, dest.y + 0.5, dest.z])
                    .unwrap_or(dest.y);
                if let Some(p) = nav.0.find_path_strict(
                    &mut scratch,
                    from.to_array(),
                    [dest.x, y, dest.z],
                    crate::nav::PATH_BUDGET_LONG,
                ) {
                    path.points = p.into_iter().skip(1).map(Vec3::from).collect();
                }
            }
        }
    }
    for u in used.read() {
        primary.0 = false;
        apply_interact(
            u.0,
            from,
            &mut path,
            &mut cb,
            &mut interact,
            &mut ch,
            &nav,
            &objects,
            &mut scratch,
        );
    }
}
