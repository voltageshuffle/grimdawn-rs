//! Player control: hold the left mouse button to move to the point under the
//! cursor along a nav path. The body is assembled by `character.rs` from the
//! loaded save's equipment (or the record's starting gear for a new
//! character); movement speed comes from the run animation's root motion, as
//! in the engine.
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_game::nav::Nav;
use gd_game::player_orders::PlayerOrder;
use gd_game_core::session::{LoadedCharacter, PlayerStart};
use gd_game_core::{
    combat::{Combatant, Dead},
    rules::CombatRules,
};
use gd_render::camera::CameraFocus;
use gd_render::character::{self, CharAssets};

pub use gd_game::movement::MovePath;
pub use gd_game_core::player::{Player, PlayerSet};

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<gd_game::movement::MouseCommand>()
            .add_systems(
                Update,
                (
                    spawn_player,
                    (click_to_move, wasd_move)
                        .chain()
                        .in_set(gd_game::player_orders::PlayerInputSet),
                )
                    .chain()
                    .in_set(PlayerSet)
                    .before(gd_game::movement::MovementStage::Follow),
            );
        #[cfg(feature = "dev")]
        app.add_systems(
            Update,
            walk_to_nearby
                .after(gd_game::player_orders::apply_orders)
                .after(spawn_player)
                .in_set(PlayerSet)
                .before(gd_game::movement::MovementStage::Follow),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_player(
    #[cfg(feature = "dev")] (hooks, mut scratch): (
        Res<gd_game_core::hooks::Hooks>,
        Local<gd_sim::nav::PathScratch>,
    ),
    mut commands: Commands,
    existing: Query<(), With<Player>>,
    start: Option<Res<PlayerStart>>,
    loaded: Option<Res<LoadedCharacter>>,
    nav: Res<Nav>,
    rules: Option<Res<CombatRules>>,
    mut cams: Query<&mut gd_render::camera::GameCamera>,
    mut assets: CharAssets,
    choice: Res<gd_game_core::session::Choice>,
    save_dir: Res<gd_game_core::session::SaveDir>,
) {
    if !existing.is_empty() {
        return;
    }
    let Some(rules) = rules else { return };
    let Some(sp) = start.map(|s| s.0) else { return };
    // Stream around the start point; wait until its walkable data is in.
    for mut c in &mut cams {
        c.target = sp;
    }
    let Some(y) = nav.0.nearest(sp.to_array(), 8.0).map(|c| nav.0.pos(c)[1]) else {
        return;
    };
    let gdc = loaded.as_ref().map(|ch| &*ch.0);
    let Some(spawn) =
        gd_game::player::spawn_data(&assets.data, &rules.0, gdc, choice.female, |skills| {
            crate::consumables::apply_account_unlocks(&choice, &save_dir, &assets.data.db, skills);
            // `GD_POTION_SELECT` (hooks.rs).
            #[cfg(feature = "dev")]
            crate::consumables::potion_select_hook(&assets.data.db, skills, &hooks);
            // `GD_LEARN` (hooks.rs): extra learned skills.
            #[cfg(feature = "dev")]
            for (rec, lv) in &hooks.gameplay.learn {
                skills.skills.retain(|(n, _)| !n.eq_ignore_ascii_case(rec));
                skills.skills.push((rec.clone(), *lv));
            }
            #[cfg(feature = "dev")]
            skills.touched();
        })
    else {
        error!("could not build the player's combat profile");
        return;
    };
    let gd_game_core::player::SpawnData {
        female,
        equipment,
        bundle: player,
    } = spawn;
    let profile = player.1.profile.clone();
    // `GD_WALK_TO` (hooks.rs) walks to its first point right after spawning.
    #[cfg(feature = "dev")]
    let path = {
        let mut path = MovePath::default();
        if let Some(&Vec2 { x, y: z }) = hooks.gameplay.walk_to.first()
            && let Some(p) = nav
                .0
                .find_path(&mut scratch, [sp.x, y, sp.z], [x, y, z], 500_000)
        {
            path.points = p.into_iter().skip(1).map(Vec3::from).collect();
        }
        path
    };
    #[cfg(not(feature = "dev"))]
    let path = MovePath::default();
    let tf = Transform::from_xyz(sp.x, y, sp.z);
    let Some(root) = character::spawn_player(&mut commands, &mut assets, tf, female, &equipment)
    else {
        error!("could not assemble the player character");
        return;
    };
    info!(
        "player spawned at {sp:?} (ground {y}), {} equipped items; life {} OA {} DA {} damage {:?}",
        equipment.len(),
        profile.sheet.life,
        profile.sheet.offensive_ability,
        profile.sheet.defensive_ability,
        profile.physical
    );
    commands
        .entity(root)
        .insert((player, CameraFocus, path, Name::new("player")));
}

/// `GD_WALK_TO` (hooks.rs), later half: the route's next point is walked to as soon as the player stands idle
/// within 60 m of it; a point within 3 m is passed. The route's last point is walked to once (e.g. teleported into
/// the Shattered Realm's treasure room, whose reward trigger is ~25 m from the portal's exit), the ones before it
/// every second until reached (a monster in the way stops a walk). Not while a teleport runs: the player is frozen
/// for the whole activity (`Character::IsTeleporting`) and `gd_game::doors::run_teleport` clears the path, so a walk
/// issued then would count as walked without happening.
#[cfg(feature = "dev")]
#[allow(clippy::too_many_arguments)]
fn walk_to_nearby(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut fresh: gd_game_core::session::NewSession,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    time: Res<Time>,
    // The route's next point, and the time of the last try (a point within 60 m can be in another, unconnected
    // area).
    mut state: Local<(usize, f32)>,
    nav: Res<Nav>,
    teleport: Res<gd_game::teleport::TeleportActivity>,
    mut players: Query<(&Transform, &mut MovePath), With<Player>>,
) {
    if fresh.begun() {
        *state = (0, f32::MIN);
    }
    let Some(&Vec2 { x, y: z }) = hooks.gameplay.walk_to.get(state.0) else {
        return;
    };
    // Frozen for the whole teleport activity (`gd_game::doors::run_teleport` clears the path until it ends).
    if teleport.active() {
        return;
    }
    let Ok((tf, mut path)) = players.single_mut() else {
        return;
    };
    let p = tf.translation;
    let d = Vec2::new(p.x - x, p.z - z).length();
    if d < 3.0 {
        state.0 += 1;
        return;
    }
    let now = time.elapsed_secs();
    if now - state.1 < 1.0 || !path.points.is_empty() || d >= 60.0 {
        return;
    }
    state.1 = now;
    if let Some(found) = nav
        .0
        .find_path(&mut scratch, p.to_array(), [x, p.y, z], 100_000)
    {
        info!("player: GD_WALK_TO walk to {x},{z} from {p:?}");
        if state.0 + 1 == hooks.gameplay.walk_to.len() {
            state.0 += 1;
        }
        path.points = found.into_iter().skip(1).map(Vec3::from).collect();
    }
}

/// First point where the cursor ray passes below the walkable ground.
pub fn pick_ground(ray: Ray3d, nav: &gd_sim::nav::NavWorld) -> Option<Vec3> {
    let mut prev = ray.origin;
    let mut t = 0.0;
    while t < 400.0 {
        t += 0.25;
        let p = ray.get_point(t);
        if let Some(g) = nav.ground(p.to_array())
            && p.y <= g
        {
            let (mut a, mut b) = (prev, p);
            for _ in 0..8 {
                let m = (a + b) * 0.5;
                if nav.ground(m.to_array()).is_some_and(|g| m.y <= g) {
                    b = m;
                } else {
                    a = m;
                }
            }
            return Some(b);
        }
        prev = p;
    }
    None
}

/// Hold the left button: walk to the cursor, attack the foe / breakable clicked, pick up the label clicked. With the
/// Move skill on a mouse button's slot (`gd_game::hotbar::move_slot_keeps_target`) that button only walks (a right
/// one too), to the object under the cursor or the ground point, and a left press keeps a breakable as its target
/// (the weapon attack breaks it) only with `breakingMoveTo`. A press with an armed Move slot follows the same rule,
/// and while the Move key is held (`crate::casting::MoveKeyHeld`, Classic movement) the player walks to the cursor
/// (the `GD_MOVE_KEY` hook holds it; `GD_HOVER` then stands in for the OS cursor).
/// A left press on an object the player interacts with (NPC, fixed item, item label) sends one
/// [`PlayerOrder::Interact`] (the object's entity), never a walk to the ground behind it, and the held button then
/// issues nothing more ([`gd_game::movement::MouseCommand`]); gd-game routes it by component and walks beside it.
/// Every ground move follows [`gd_game::movement::request_move`]. What the click means is sent as
/// [`PlayerOrder`]s, which `gd_game::player_orders` applies.
/// The foe / breakable clicked is the hover pick ([`gd_ui::state::hover::Hovered`], `combat::pick_enemy` — one pick
/// feeds the hover and the attack). A press attacks it once ([`gd_game::player_orders::PrimaryUse`]); while the
/// button is held, every repeat takes that pick again and re-issues the attack (`InGameUI::HandleMouseEvent`: the
/// pick runs on every mouse event while held; its wider held frustum is the stickiness), so the attacks chain, and a
/// skill key pressed meanwhile goes between two of them: a different pick switches the attack, and a target's death
/// re-acquires the cursor pick instead of falling through to a ground move.
#[allow(clippy::too_many_arguments)]
fn click_to_move(
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    // The main camera only (the glow pass has a second one).
    cams: Query<(&Camera, &GlobalTransform), With<gd_render::camera::GameCamera>>,
    time: Res<Time>,
    nav: Res<Nav>,
    players: Query<
        (
            &MovePath,
            &Combatant,
            Option<&gd_game_core::casting::Caster>,
        ),
        (With<Player>, Without<Dead>),
    >,
    mut orders: MessageWriter<PlayerOrder>,
    monsters: Query<
        (
            Entity,
            &GlobalTransform,
            &Combatant,
            Option<&gd_game_core::world::Breakable>,
            gd_game::combat::SideData,
        ),
        (
            Without<Player>,
            Without<Dead>,
            Without<gd_game::combat::Invincible>,
        ),
    >,
    items: Query<
        (Entity, &GlobalTransform),
        (
            With<crate::loot::GroundItem>,
            Without<crate::loot::Unpickable>,
        ),
    >,
    capture: Res<gd_ui::state::inventory::UiCapture>,
    keys: gd_ui::state::casting::ClickKeys,
    options: Res<gd_game_core::options::GameOptions>,
    hovered_label: Res<gd_ui::state::loot::HoveredLabel>,
    (move_held, armed): (
        Res<crate::casting::MoveKeyHeld>,
        Res<gd_game::hotbar::ArmedSlot>,
    ),
    (hovered, objects, mut gate): (
        Res<gd_ui::state::hover::Hovered>,
        Query<&GlobalTransform>,
        ResMut<gd_game::movement::MouseCommand>,
    ),
) {
    use gd_game::hotbar::{is_move_to, move_slot_keeps};
    // The frame's mouse event opens or closes the world command gate (`gd_game::movement::MouseCommand`,
    // `InGameUI::HandleMouseEvent`), for this system and the world clicks after it (`casting::cast_input`): a press
    // the UI took (`UiCapture`) leaves the held buttons without commands until they are released, also once the
    // cursor leaves the window or the window closed.
    let pressed_now = buttons.any_just_pressed([MouseButton::Left, MouseButton::Right]);
    gate.event(
        buttons.any_pressed([MouseButton::Left, MouseButton::Right]),
        pressed_now,
        capture.0,
    );
    if capture.0 {
        return;
    }
    // Stationary Attack held: attack in place (`casting::cast_input`), no moving.
    if keys.stationary() {
        return;
    }
    // Force Move held: the press skips the pick of whatever is under the cursor and walks to the ground point
    // (`casting::ClickKeys`, `InGameUI::HandleMouseEvent`).
    let force = keys.force_move();
    let Ok((path, cb, caster)) = players.single() else {
        return;
    };
    let move_slot =
        |slot: usize| caster.is_some_and(|c| c.slots[slot].as_deref().is_some_and(is_move_to));
    // A press with an armed Move slot is a Move press (`casting::armed_click` lets it through).
    let armed_move = armed.0.is_some_and(move_slot);
    // The right button walks only with the Move skill on it (else it is `casting::cast_input`'s).
    let right = !buttons.pressed(MouseButton::Left)
        && buttons.pressed(MouseButton::Right)
        && (move_slot(gd_game_core::casting::SLOT_RIGHT)
            || (armed_move && buttons.just_pressed(MouseButton::Right)));
    let (button, moving) = if right {
        (MouseButton::Right, true)
    } else {
        (
            MouseButton::Left,
            move_slot(gd_game_core::casting::SLOT_LEFT)
                || (armed_move && buttons.just_pressed(MouseButton::Left)),
        )
    };
    // The held button's repeat timer runs (applied as `PlayerOrder::Elapse`; read here as it will be).
    let dt = time.delta_secs();
    orders.write(PlayerOrder::Elapse(dt));
    let repath = path.repath - dt;
    // The held Move key (`casting::MoveKeyHeld`, Classic movement only): every event that is no press walks to the
    // cursor point, whatever is under it.
    let move_key = move_held.0 && !pressed_now && options.int("movementType") == 0;
    // The object under the cursor that a left press interacts with (`SelectPrimaryAction`'s Npc / FixedActor / Item
    // requests): an NPC, a fixed item (chest, door, shrine, riftgate), an item's label. The press is the whole
    // command (`gd_game::movement::MouseCommand`): the object's own system walks there (or acts at once in range) and
    // the held button issues nothing more, so no ground move under the cursor replaces that walk. A right press with
    // Move turns it into a plain walk to the object's coordinates (`move_slot_keeps_target`: the reset).
    let interactable = hovered
        .entity
        .filter(|_| !force)
        .filter(|_| {
            matches!(
                hovered.kind,
                gd_ui::state::hover::Kind::Npc | gd_ui::state::hover::Kind::Object
            )
        })
        .and_then(|e| objects.get(e).ok().map(|gt| (e, gt.translation())));
    let label = hovered_label.0.filter(|e| !force && items.contains(*e));
    let pressed = buttons.just_pressed(button);
    let interaction = pressed && !right && (label.is_some() || interactable.is_some());
    // The interaction press is itself a command (the walk to the object, the pickup below); it only ends the held
    // button's commands.
    if interaction {
        gate.interaction();
    }
    if !move_key && !interaction && (!buttons.pressed(button) || !gate.open()) {
        return;
    }
    if !move_key && repath > 0.0 && !pressed {
        return;
    }
    if move_key && repath > 0.0 {
        return;
    }
    let (Ok(win), Ok((cam, cam_tf))) = (window.single(), cams.single()) else {
        return;
    };
    let Some(cursor) = win.cursor_position() else {
        return;
    };
    let Ok(ray) = cam.viewport_to_world(cam_tf, cursor) else {
        return;
    };
    if move_key {
        orders.write(PlayerOrder::Clear);
    }
    let moving = moving || move_key;
    // The enemy the hover pick chose (`hover::pick` → `combat::pick_enemy`, one pick feeds the hover and the
    // attack): a character / targetable breakable the player's side may attack (`gd_game::combat::player_target`).
    // Force Move skips it (the press acts on the ground point).
    let hovered_enemy = || {
        hovered
            .entity
            .filter(|_| !force)
            .filter(|_| {
                matches!(
                    hovered.kind,
                    gd_ui::state::hover::Kind::Character | gd_ui::state::hover::Kind::Breakable
                )
            })
            .filter(|e| {
                monsters
                    .get(*e)
                    .ok()
                    .is_some_and(|m| gd_game::combat::player_target(m.3, &m.4))
            })
    };
    // With Move, a foe / breakable under the cursor that is not kept is walked to (its coordinates).
    let mut walk_to = None;
    if interaction && label.is_none() {
        // The walk to the object (`MoveToNpc` / `MoveToFixedItem`): one interact order, routed by component in
        // gd-game (`player_orders`), which walks beside it and uses it once in reach.
        if let Some((e, _)) = interactable {
            orders.write(PlayerOrder::Interact(e));
        }
        return;
    }
    if pressed {
        // Clicking an item's label walks over and picks it up (the interact order routes to `loot::pick_up`);
        // a right press with Move drops the object. The pick mode (`crate::loot::PickMode`,
        // `InGameUI::GetPickFilterMode`, with `lockItemPickup`): `items`.
        if !right && let Some(item) = label {
            orders.write(PlayerOrder::Interact(item));
            return;
        }
        orders.write(PlayerOrder::Clear);
        // Clicking a monster attacks it once (`gd_game::player_orders::PrimaryUse`).
        let mut target = hovered_enemy();
        if right && target.is_none() {
            walk_to = interactable.map(|o| o.1);
        }
        if moving && let Some(e) = target {
            let (_, gt, _, breakable, _) = monsters.get(e).expect("picked from the query");
            if !move_slot_keeps(right, breakable.is_some(), options.flag("breakingMoveTo")) {
                target = None;
                walk_to = Some(gt.translation());
            }
        }
        if let Some(e) = target {
            orders.write(PlayerOrder::Attack(e));
            return;
        }
    } else if !move_key {
        // The held button re-picks under the cursor every repeat (`InGameUI::HandleMouseEvent`: the pick runs on
        // every mouse event while held, `SetCombatEnemy` from it, the re-dispatch gated by the 10/50 ms timer — the
        // `repath` wait above). The hover pick is that one pick (its wider held frustum is the stickiness): a
        // different pick switches the attack; a dead target re-acquires the cursor pick, not a ground move.
        let mut target = hovered_enemy();
        if moving && let Some(e) = target {
            let (_, gt, _, breakable, _) = monsters.get(e).expect("picked from the query");
            if !move_slot_keeps(right, breakable.is_some(), options.flag("breakingMoveTo")) {
                target = None;
                walk_to = Some(gt.translation());
            }
        }
        match target {
            // Every repeat asks for the next use (the same target keeps its walk).
            Some(e) => {
                orders.write(PlayerOrder::Attack(e));
                return;
            }
            None => {
                if cb.target.is_some() {
                    // The cursor left the target: the combat enemy is cleared, the walk goes on.
                    orders.write(PlayerOrder::Clear);
                }
            }
        }
    } else if moving {
        walk_to = hovered_enemy()
            .and_then(|e| monsters.get(e).ok())
            .map(|m| m.1.translation());
    }
    // `SelectPrimaryAction`: with `movementType` above 0 (Keyboard / Keyboard Only) a left click never
    // walks to the cursor.
    if options.int("movementType") > 0 {
        return;
    }
    // The plain move (`gd_game::player_orders`): to the object's coordinates (`InGameUI::HandleMouseEvent`,
    // `InGameUI.cursorObjectCoords`; a breakable on its own nav obstacle gets the straight move point there), else
    // the ground point under the cursor.
    orders.write(PlayerOrder::Move {
        to: walk_to,
        ground: pick_ground(ray, &nav.0),
        camera: cam_tf.translation(),
    });
}

/// Keyboard movement (`movementType` 1 Keyboard, 2 Keyboard Only; `InGameUI::Update`):
/// the key actions 63 / 64 forward / back and 65 / 66 left / right (opposite keys cancel) give a direction relative
/// to the camera (its horizontal forward and right), normalised; while one is held a destination 1.25 m ahead is
/// issued every frame, so the player runs on. With a world mouse button held (and no running skill) Keyboard does
/// nothing, and with a skill running (a cast; a running channel refuses the turn) the keys only turn the player
/// (aiming). Every frame the direction and its length (1, or 1.414 for a diagonal; 0 and no direction with no key
/// held) become the controller direction, and in Keyboard Only a held direction is also the aim direction
/// ([`gd_game_core::player::ControllerDirection`]). Sent as [`PlayerOrder`]s (`gd_game::player_orders`).
#[allow(clippy::too_many_arguments)]
fn wasd_move(
    options: Res<gd_game_core::options::GameOptions>,
    bindings: Res<gd_ui_kit::keys::KeyBindings>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    capture: Res<gd_ui::state::inventory::UiCapture>,
    cams: Query<&Transform, (With<gd_render::camera::GameCamera>, Without<Player>)>,
    players: Query<
        (
            &Transform,
            &gd_game_core::character::Character,
            &gd_game_core::player::ControllerDirection,
            Has<gd_game_core::casting::Channel>,
        ),
        (With<Player>, Without<Dead>),
    >,
    mut orders: MessageWriter<PlayerOrder>,
) {
    let Ok((tf, ch, controller, channelling)) = players.single() else {
        return;
    };
    let mode = options.int("movementType");
    let held = |a: u32| bindings.pressed(a, &keys);
    let fwd =
        held(gd_ui_kit::keys::MOVE_FORWARD) as i32 - held(gd_ui_kit::keys::MOVE_BACKWARD) as i32;
    let side = held(gd_ui_kit::keys::MOVE_RIGHT) as i32 - held(gd_ui_kit::keys::MOVE_LEFT) as i32;
    let flat = |v: Vec3| Vec2::new(v.x, v.z).try_normalize().unwrap_or(Vec2::ZERO);
    let dir = cams
        .single()
        .ok()
        .filter(|_| mode > 0 && (fwd, side) != (0, 0))
        .and_then(|cam| {
            let (f, r) = (flat(*cam.forward()), flat(*cam.right()));
            (f * fwd as f32 + r * side as f32).try_normalize()
        });
    let Some(dir) = dir else {
        if controller.dir != Vec3::ZERO || controller.length != 0.0 {
            orders.write(PlayerOrder::Controller {
                dir: Vec3::ZERO,
                length: 0.0,
                aim: None,
            });
        }
        return;
    };
    let mouse =
        !capture.0 && (buttons.pressed(MouseButton::Left) || buttons.pressed(MouseButton::Right));
    let running = channelling || matches!(ch.state, gd_game_core::character::CharState::Cast);
    // Keyboard: a held mouse button owns the player unless a skill runs (the controller direction is left as it
    // was); in both modes a held button or a running skill makes the keys only turn.
    if mode == 1 && mouse && !running {
        return;
    }
    let dir3 = Vec3::new(dir.x, 0.0, dir.y);
    orders.write(PlayerOrder::Controller {
        dir: dir3,
        length: ((fwd * fwd + side * side) as f32).sqrt(),
        aim: (mode == 2).then_some(dir3),
    });
    let rotate_only = mouse || running;
    if rotate_only {
        // A running channel refuses the turn (`ControllerPlayerStateUseSkill::RequestRotateAction`,
        // a `SkillChanneled` that `IsRunning`); Keyboard Only steers it by the controller direction
        // (`gd_game::casting::channels`).
        // `RotateAction` (`RequestRotateAction`): a look-at point one step along the keys (`gd_game::turning`).
        if !channelling && dir3 != Vec3::ZERO {
            orders.write(PlayerOrder::Turn(tf.translation + dir3));
        }
        return;
    }
    orders.write(PlayerOrder::Hop(tf.translation + dir3 * 1.25));
}
