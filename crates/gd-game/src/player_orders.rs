//! Player orders: what the game's mouse and keyboard handlers decided, applied to the player.
//!
//! The input side (grimdawn `player.rs`: `click_to_move`, `wasd_move`, in [`PlayerInputSet`]) reads the buttons,
//! the cursor ray and the hovered object and writes [`PlayerOrder`]s; [`apply_orders`] applies them in order in
//! the same frame, before the walk advances (`MovementStage::Follow`). An order is one of the engine's controller
//! requests:
//! - the held button's repeat timer (`MovePath::repath`, sub_140021000 `a1+292`, [`command_wait`]);
//! - a walk to an object the player interacts with ([`PlayerOrder::Interact`], `MoveToNpc` / `MoveToFixedItem` /
//!   `MoveAndPickUpItem`), an attack on a foe (`SelectPrimaryAction` @0x180161d00): one
//!   use of the primary attack ([`PrimaryUse`]), repeated only while the button is held;
//! - a plain move ([`crate::movement::request_move`]): to the object's coordinates when there is one, else the
//!   ground point under the cursor. PORT: the engine moves to the object's coordinates only; a request the rules
//!   drop falls back to the ground point;
//!
//! One interaction target ([`InteractTarget`], a component of the player controller) replaces the engine's one
//! controller state (`MoveToFixedItem` / `MoveToNpc` / `MoveToItem`): [`apply_orders`] sets it for an
//! [`PlayerOrder::Interact`] (or the Interact key's [`crate::controller_target::UseObject`]) and walks beside the
//! object with a single path search; the object's own use system (chests, doors, shrines, riftgates, the altar,
//! the gravestone, NPCs, ground items) takes it once in reach, routing by the entity's component. Every new order
//! (Clear / Move / Attack / Interact, the keyboard's move and hop) drops it, as a new primary action replaces the
//! controller state (sub_140021000 `SelectPrimaryAction`); a started attack / cast / evade drops it too
//! (`follow_path`), and a target that left the world expires. The session's end despawns the player with it.
//!
//! A move requested mid-swing / mid-cast / mid-evade is remembered in the character's pending-action slot
//! ([`crate::character::QueuedAction`], last request wins) and runs at the clip's `AllowInterrupt` point or its
//! "End" (`follow_path`); the engine's one-slot action queue, `CharacterHandlerUpdate` @0x1801279f0.
//!
//! - the keyboard movement (`sub_14002BC00`): the controller direction ([`ControllerDirection`]), a turn
//!   toward a look-at point (`RequestRotateAction`) and a hop 1.25 m ahead. PORT: the engine reaches the hop with
//!   a short plain move; here a strict nav path.
use crate::character::Character;
use crate::combat::{Combatant, Dead};
use crate::data::GameData;
use crate::movement::{MovePath, MovementStage, apply_move_order};
use crate::nav::Nav;
use crate::player::{ControllerDirection, Player, PlayerSet};
use bevy::ecs::entity::Entities;
use bevy::prelude::*;

/// The input systems that write [`PlayerOrder`]s (the game's mouse and keyboard handlers).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerInputSet;

/// One request of the player's input to its controller (module doc).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub enum PlayerOrder {
    /// The held button's repeat timer runs for this many seconds.
    Elapse(f32),
    /// Drops the attack target and the pending interaction.
    Clear,
    /// Walks beside the object `0` and interacts with it once in reach (its own use system routes by component),
    /// dropping the attack target and the pending interaction. A non-interactable object (a described decoration)
    /// is only walked to.
    Interact(Entity),
    /// One use of the primary attack on `0` ([`PrimaryUse`]): walks into reach and swings / casts once. A new
    /// target stops the walk; the held button's repeat on the same one keeps it.
    Attack(Entity),
    /// A plain move to `to`, else to `ground`; `camera` is the camera position (the retry of a point above the
    /// player). Sets the repeat timer.
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
    /// The keyboard's hop to a point ahead (its height taken from the nav ground there).
    Hop(Vec3),
}

/// The wait before a held button (or the held Move key) issues its next command, in seconds.
/// sub_140021000: the timer `a1+292` is 10 ms when the command was taken (`HandleActionFromMouse` true) or the player
/// is moving (`ControllerAI::IsMoving`), else 50 ms.
pub fn command_wait(taken: bool, moving: bool) -> f32 {
    if taken || moving { 0.010 } else { 0.050 }
}

/// The player controller's pending interaction (module doc): the object walked to and used once in reach. One
/// target for every interactable kind; the use systems route by the entity's component and clear it on use.
#[derive(Component, Default)]
pub struct InteractTarget(pub Option<Entity>);

/// The click's attack order is one use of the primary attack: `SelectPrimaryAction`'s UseSkill controller state ends
/// once its skill was used, and only the held button's repeat (grimdawn `player.rs`, every 10 / 50 ms) asks again, so
/// a click attacks once and a held button keeps attacking. [`PlayerOrder::Attack`] arms it; the swing / cast that
/// starts on [`Combatant::target`] spends it (`combat::player_attack`, `casting::spell_attack_target`), and so does
/// any skill request that runs (a hot slot key, the right button: `casting::cast_requests`), as a new controller
/// state replaces the UseSkill one. The target stays the running swing's (its hit lands on it) and is dropped when the
/// action ends unarmed. A skill queued meanwhile goes before the next primary use (the action queue's pending slot,
/// [`crate::character::QueuedAction`]): between two held attacks.
#[derive(Component, Default)]
pub struct PrimaryUse(pub bool);

/// The use range of a fixed item (`ControllerPlayerState::DefaultRequestInteractableAction`
/// @0x180163080, `gd_game::doors` module doc): the 3D distance between the player's and the object's origins is at
/// most player radius + object radius (`actorRadius` × scale) + `usableRange` (0 when absent), 1 m more once the
/// walk to it has ended (`ControllerPlayerStateMoveToFixedItem::EndOfPathReached`, `path_end`).
pub fn usable_range(player_radius: f32, object_radius: f32, usable: f32, path_end: bool) -> f32 {
    player_radius + object_radius + usable + if path_end { 1.0 } else { 0.0 }
}

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

/// What an interact order walks to and uses (module doc): the walk goal's ring radius and its kind. Inserted at
/// spawn by every interactable object (chests, doors, devotion shrines, riftgates, portals, dynamic rifts, the
/// endless dungeon's gates and shrines, the gravestone, the ascendant altar; ground items and characters by the
/// observers below), so [`apply_orders`] reads one query. An entity without it is only walked to.
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

/// How an interact walk is planned: an NPC's failed path falls back to a straight move to it (`talk_input`'s PORT).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractKind {
    Fixed,
    Npc,
}

/// A ground item's [`Interactable`]: its record's item radius (`crate::loot_filter::item_radius`; gold: 0).
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

/// A non-player character's [`Interactable`] (NPCs are talked to; monsters and pets are attacked, never sent
/// here): its record's radius.
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
    let radius = crate::combat::get_radius(data.db.get(&ch.record), crate::combat::ACTOR_RADIUS);
    commands.entity(ev.entity).insert(Interactable {
        radius,
        kind: InteractKind::Npc,
    });
}

/// Routes an interact request (a click or the Interact key): drop the attack target and the pending interaction,
/// set the new target and walk beside it with one path search (module doc).
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
    // A new command supersedes the pending slot (last wins).
    ch.queued = None;
    let Ok((gt, it)) = objects.get(e) else {
        return;
    };
    let at = gt.translation();
    // A non-interactable object (a described decoration): walk to its coordinates only.
    let r = it.map_or(0.6, |i| i.radius);
    if it.is_some() {
        interact.0 = Some(e);
    }
    if let Some(p) = crate::chests::path_near(&nav.0, scratch, from, at, r) {
        path.points = p;
    } else if it.is_some_and(|i| i.kind == InteractKind::Npc) {
        // No nav path (e.g. a cell a new defense turret blocks): a straight move, not a talk from afar that
        // `run_conversations` closes at once (PORT, `DefaultRequestMoveAction`'s `FindPath` fallback).
        path.points = vec![at];
    }
}

/// Applies the frame's [`PlayerOrder`]s to the (living) player, in order.
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
    // A target that left the world (opened, taken, despawned) expires.
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
                // The held button's repeat on the same target keeps the walk to it.
                if cb.target != Some(e) {
                    path.points.clear();
                }
                cb.target = Some(e);
                primary.0 = true;
                interact.0 = None;
                // A new command supersedes a pending move (last wins); a queued skill stays and runs before the
                // attack ([`PrimaryUse`]), as the action queue drops a primary attack request while a skill waits
                // (`AttackAction::QueryActionPermission`, `casting::cast_requests`).
                ch.queued
                    .take_if(|q| matches!(q, crate::character::QueuedAction::Move(_)));
            }
            PlayerOrder::Move { to, ground, camera } => {
                interact.0 = None;
                let order = crate::character::MoveOrder { to, ground, camera };
                let busy = matches!(
                    ch.state,
                    crate::character::CharState::Attack
                        | crate::character::CharState::Cast
                        | crate::character::CharState::Evade
                        | crate::character::CharState::Pickup
                );
                if busy && !ch.can_act() {
                    // Remembered, not dropped: it runs at the clip's `AllowInterrupt`
                    // point or its "End" (`follow_path`, the action queue).
                    ch.queued = Some(crate::character::QueuedAction::Move(order));
                } else {
                    if busy {
                        // Recovery cancel: the cancelled remainder becomes the attack-delay
                        // (`AttackAction::QueryActionPermission` @0x180084380); the evade's
                        // components go with its state (`evade::evading`).
                        if matches!(
                            ch.state,
                            crate::character::CharState::Attack | crate::character::CharState::Cast
                        ) {
                            ch.attack_delay = ch.attack_delay.max(ch.remaining_ms());
                        }
                        ch.state = crate::character::CharState::Run;
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
                // A held direction moves the player: it drops the pending interaction (a release does not).
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
    // The Interact key's request (`gd_game::controller_target::UseObject`) routes as a click on the object.
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
