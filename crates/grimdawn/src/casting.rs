//! Skill casting, presentation side: player input becomes requests
//! (`CastRequest`, `UsePotion`, a channel's held key and aim), and
//! projectile models and effect decals get their meshes. The casting rules
//! are `gd_game::casting`.
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
pub use gd_game::casting::*;
use gd_game::combat::MonsterAi;
use gd_game::hotbar::{ArmedSlot, CursorOverWorld, UseArmedSlot};
use gd_game::nav::Nav;
pub use gd_game_core::casting::*;
use gd_game_core::combat::{Combatant, Dead};
use gd_game_core::player::Player;
use gd_game_core::visuals::{PendingDecal, ProjectileMesh, ProjectileModelOf};
use gd_render::fx::{DecalOwner, DropDecal, GroundTilt};
use gd_ui::state::casting::{CastInputStage, ClickKeys};
use gd_ui::state::inventory::UiCapture;

pub struct CastingViewPlugin;

impl Plugin for CastingViewPlugin {
    fn build(&self, app: &mut App) {
        app.register_required_components::<ProjectileMesh, Visibility>()
            .register_required_components::<PendingDecal, Visibility>()
            .register_required_components::<Projectile, Visibility>()
            // The potion keys before `potions` reads them: the R / E press is drunk
            // the same frame (`HotSlotOptionPotion::Activate` → `UsePotionOfType`).
            .add_systems(
                Update,
                potion_input
                    .after(gd_game_core::inventory::InventorySet)
                    .before(potions),
            );
        // Bevy 0.20 releases the keyboard on focus loss but not the mouse buttons; after the input
        // systems, which clear the previous frame's just-flags.
        app.add_systems(
            PreUpdate,
            release_mouse_on_focus_loss.after(bevy::input::InputSystems),
        );
        // The Move-key hook stands in for the OS keyboard: before the input readers.
        #[cfg(feature = "dev")]
        app.add_systems(
            Update,
            move_key_hook.before(gd_game::player_orders::PlayerInputSet),
        );
        app.add_systems(
            Update,
            (
                hot_slot_keys,
                // After `click_to_move`, which updates the world command gate it reads.
                cast_input
                    .in_set(CastInputStage::Cast)
                    .after(gd_game::player_orders::PlayerInputSet),
                channel_input,
            )
                .chain()
                .after(gd_game_core::inventory::InventorySet)
                // The cursor record is taken before the requests that read it.
                .after(gd_game::hotbar::HotbarCursorSet)
                // Before the gd-game hotbar chain: its `hot_slot_keys` reads the
                // `HotSlotKey` these write, `use_options` the option `CastRequest`s,
                // so a key press / right-click option acts the same frame.
                .before(gd_game::hotbar::HotbarSet)
                .before(cast_requests)
                .before(channels),
        )
        // Before the click moves the player (and so before `cast_input`);
        // a pending pet order takes the click first (`InGameUI::HandleMouseEvent`).
        .add_systems(
            Update,
            armed_click
                .in_set(CastInputStage::Armed)
                .after(gd_game_core::inventory::InventorySet)
                .after(gd_ui::state::pets::PetInputSet)
                .before(gd_game_core::player::PlayerSet),
        )
        // The cursor record before the press that may use it (armed_click,
        // gd-game's hot slot keys and evade read it after PlayerSet).
        .add_systems(
            Update,
            world_cursor
                .in_set(CastInputStage::Cursor)
                .after(gd_game_core::inventory::InventorySet)
                .before(armed_click),
        )
        // The hotbar's cursor readers take this frame's `CursorOverWorld` /
        // `WorldMouse` (they moved out of the chain that ran after PlayerSet).
        .configure_sets(Update, gd_game::hotbar::HotbarCursorSet.after(world_cursor))
        .init_resource::<DropDecals>()
        .init_resource::<DecalTextureLoads>()
        .init_resource::<MoveKeyHeld>()
        .add_systems(Update, (fx_decals, (drop_decals, ring_alpha).chain()))
        // After every Update spawner of a projectile model (casting, late procs, monster skills) and the
        // frame's flight step, before this frame's propagation and bounds: a model is dressed and placed the
        // frame its projectile spawns and draws where the projectile is this frame.
        .add_systems(
            PostUpdate,
            projectile_meshes
                .before(bevy::transform::TransformSystems::Propagate)
                .before(bevy::camera::visibility::VisibilitySystems::CalculateBounds)
                .in_set(gd_render::model::ModelSpawnSet),
        );
    }
}

/// Bevy 0.20 synthesizes key releases on focus loss (`bevy_input`'s keyboard system) but no mouse ones:
/// a button held across Alt-Tab stays `pressed` until it is clicked again (phantom casting / walking).
/// Release every mouse button when a window loses focus; the held-button consumers ([`armed_click`]'s
/// swallow, [`cast_input`], `crate::player`'s walk) see the buttons let go as on a real release.
fn release_mouse_on_focus_loss(
    mut focus: MessageReader<bevy::window::WindowFocused>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
) {
    if focus.read().any(|f| !f.focused) {
        buttons.release_all();
    }
}

/// Potion keys (scan codes 19/18 = actions 30/29, named by `KeyBindings::GetActionByName`):
/// R health, E energy; not while the UI took the input, nor while the riftgate map, a conversation or the
/// altar menu takes the keys (as the hot slot keys).
fn potion_input(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<gd_ui_kit::keys::KeyBindings>,
    capture: Res<UiCapture>,
    (rift_map, altar, dialog): (
        Res<gd_game::riftgates::RiftgateMap>,
        Res<gd_ui::state::ascendant::AltarMenu>,
        Res<gd_game::conversation::Conversation>,
    ),
    mut out: MessageWriter<UsePotion>,
) {
    if capture.0 || rift_map.open || dialog.view.is_some() || altar.open {
        return;
    }
    if bindings.just_pressed(gd_ui_kit::keys::DRINK_HEALTH, &keys) {
        out.write(UsePotion::Health);
    } else if bindings.just_pressed(gd_ui_kit::keys::DRINK_ENERGY, &keys) {
        out.write(UsePotion::Energy);
    }
}

/// What a cursor press may aim at: foes, targetable breakables and one's
/// own pets (the engine's combat enemy / ally, `InGameUIActorCapture::CollectCandidates`; the player
/// is never one).
type Pickable<'w, 's> = (
    Query<
        'w,
        's,
        (
            Entity,
            &'static GlobalTransform,
            &'static Combatant,
            Option<&'static gd_game_core::world::Breakable>,
            gd_game::combat::SideData,
        ),
        (
            Without<Dead>,
            Without<Player>,
            Without<gd_game::combat::Invincible>,
        ),
    >,
    Query<
        'w,
        's,
        (
            Entity,
            &'static GlobalTransform,
            &'static Combatant,
            &'static gd_game::pets::Pet,
            gd_game::combat::SideData,
        ),
        (Without<Dead>, Without<Player>, Without<MonsterAi>),
    >,
);

/// The character a cursor press aims at (a foe, a targetable breakable or one of `owner`'s pets) and its position.
/// The foe is the hover pick ([`gd_ui::state::hover::Hovered`], `combat::pick_enemy`: the touched enemy nearest to the
/// player, held stickiness, target lock), the outlined one, as for the left click (`crate::player`'s
/// `click_to_move`): one pick feeds the hover and every attack (`InGameUI::HandleMouseEvent`). Else one's own
/// pet: the
/// hovered one, or the one under the ray (the hover outlines one's pets only with the Target Pet key).
fn under_cursor(
    hovered: &gd_ui::state::hover::Hovered,
    ray: Ray3d,
    (monsters, pets): &Pickable,
    owner: Entity,
) -> Option<(Entity, Vec3)> {
    use gd_ui::state::hover::Kind;
    let foe = hovered
        .entity
        .filter(|_| matches!(hovered.kind, Kind::Character | Kind::Breakable))
        .and_then(|e| monsters.get(e).ok())
        .filter(|m| gd_game::combat::player_target(m.3, &m.4))
        .map(|m| (m.0, m.1.translation()));
    if foe.is_some() {
        return foe;
    }
    // One's own pets, unless a monster's convert made one a foe (then it is a foe above).
    let mine = || {
        pets.iter()
            .filter(|p| p.3.owner == owner && !gd_game::combat::Side::Player.foe(p.4.side_ref()))
    };
    let e = hovered
        .entity
        .filter(|e| mine().any(|p| p.0 == *e))
        .or_else(|| crate::combat::pick_monster(ray, mine().map(|p| (p.0, p.1, p.2))))?;
    pets.get(e).ok().map(|p| (e, p.1.translation()))
}

/// The world mouse handler's per-frame part (`gd_game::hotbar` module docs: one input-device event a frame, plus the
/// presses): with the cursor over the world (the UI did not take the mouse, not over a hot slot button), a
/// `WorldMouse` per left / right press and one for the frame (whether a left / right button is held; the new mouse
/// command), and the controller's cursor record (`gd_game::hotbar::WorldCursor`): [`CursorOverWorld`] with the
/// character under the cursor and its coordinates, else the ground point (none when the ray hits no ground: the
/// record stays). Keyboard Only leaves the record to the controller targeting (`crate::controller_target`;
/// `InGameUI::HandleMouseEvent`).
#[allow(clippy::too_many_arguments)]
pub fn world_cursor(
    mut clicked: MessageReader<bevy::input::mouse::MouseButtonInput>,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    cams: Query<(&Camera, &GlobalTransform), With<gd_render::camera::GameCamera>>,
    nav: Res<Nav>,
    (capture, over_slot): (Res<UiCapture>, Res<gd_ui::hotbar_pick::OverHotSlot>),
    players: Query<Entity, (With<Player>, Without<Dead>)>,
    pickable: Pickable,
    (mut out, mut mouse): (
        MessageWriter<CursorOverWorld>,
        MessageWriter<gd_game::hotbar::WorldMouse>,
    ),
    options: Res<gd_game_core::options::GameOptions>,
    (hovered, label, controller, armed): (
        Res<gd_ui::state::hover::Hovered>,
        Res<gd_ui::state::loot::HoveredLabel>,
        Res<crate::controller_target::ControllerPick>,
        Res<ArmedSlot>,
    ),
) {
    let presses: Vec<MouseButton> = clicked
        .read()
        .filter(|e| {
            e.state.is_pressed() && matches!(e.button, MouseButton::Left | MouseButton::Right)
        })
        .map(|e| e.button)
        .collect();
    if capture.0 || over_slot.0 {
        return;
    }
    let held = buttons.any_pressed([MouseButton::Left, MouseButton::Right]);
    // A left press's primary action on an item (its label), an NPC or a fixed actor (`SelectPrimaryAction`'s
    // requests, `gd_game::hotbar` module docs); in Keyboard Only it acts on the cursor record (a picked NPC) or
    // the selected label.
    let on_object = label.0.is_some()
        || if options.int("movementType") == 2 {
            matches!(controller.0, gd_game::controller_target::Pick::Npc(_))
        } else {
            hovered.entity.is_some()
                && matches!(
                    hovered.kind,
                    gd_ui::state::hover::Kind::Object | gd_ui::state::hover::Kind::Npc
                )
        };
    for b in presses {
        mouse.write(gd_game::hotbar::WorldMouse {
            press: true,
            held: true,
            object: b == MouseButton::Left && armed.0.is_none() && on_object,
        });
    }
    mouse.write(gd_game::hotbar::WorldMouse {
        press: false,
        held,
        object: false,
    });
    if options.int("movementType") == 2 {
        return;
    }
    let Ok(me) = players.single() else { return };
    let (Ok(win), Ok((cam, cam_gt))) = (window.single(), cams.single()) else {
        return;
    };
    let Some(ray) = win
        .cursor_position()
        .and_then(|c| cam.viewport_to_world(cam_gt, c).ok())
    else {
        return;
    };
    let rec = match under_cursor(&hovered, ray, &pickable, me) {
        Some((e, at)) => Some(CursorOverWorld {
            target: Some(e),
            point: at,
        }),
        None => crate::player::pick_ground(ray, &nav.0).map(|point| CursorOverWorld {
            target: None,
            point,
        }),
    };
    if let Some(rec) = rec {
        out.write(rec);
    }
}

/// With a hot slot armed (`gd_game::hotbar::ArmedSlot`), the next world
/// press, left or right, uses it at the character under the cursor or the
/// ground point ([`UseArmedSlot`], `InGameUI::HandleMouseEvent`); the press is the armed
/// slot's: no move, no mouse skill, until the button is let go.
/// In `InGameUI::HandleMouseEvent` a press runs the armed slot (`ActivateSuperSkill` first,
/// with the Move key held too: a press takes the press path), the event after it
/// finds the slot gone (`InGameUI.armedSlotUsed` set) and clears the pending command
/// (`InGameUI.commandPending`), which only a press sets again; but with the Move key held
/// ([`MoveKeyHeld`], Classic movement) every event that is no press sets it, so
/// the walk goes on while the button is still held. PORT: the engine's one
/// event without the walk is not kept.
#[allow(clippy::too_many_arguments)]
pub fn armed_click(
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    cams: Query<(&Camera, &GlobalTransform), With<gd_render::camera::GameCamera>>,
    nav: Res<Nav>,
    mut capture: ResMut<UiCapture>,
    armed: Res<ArmedSlot>,
    players: Query<(Entity, &Transform, &Caster), (With<Player>, Without<Dead>)>,
    pickable: Pickable,
    mut out: MessageWriter<UseArmedSlot>,
    mut swallow: Local<bool>,
    (options, record, move_held, hovered): (
        Res<gd_game_core::options::GameOptions>,
        Res<gd_game::hotbar::WorldCursor>,
        Res<MoveKeyHeld>,
        Res<gd_ui::state::hover::Hovered>,
    ),
) {
    let held = buttons.any_pressed([MouseButton::Left, MouseButton::Right]);
    if *swallow {
        match armed_hold(held, move_held.0 && options.int("movementType") == 0) {
            ArmedHold::Capture => {
                capture.0 = true;
                return;
            }
            ArmedHold::Walk => return,
            ArmedHold::Done => *swallow = false,
        }
    }
    if capture.0
        || armed.0.is_none()
        || !buttons.any_just_pressed([MouseButton::Left, MouseButton::Right])
    {
        return;
    }
    let Ok((me, tf, caster)) = players.single() else {
        return;
    };
    // An armed Move slot: its request fails (`gd_game::hotbar::MOVE_TO`) and `ActivateSuperSkill`
    // (the armed slot taken and cleared either way) does not take the press, so it goes on as a press with the Move
    // rule (`gd_game::hotbar::move_slot_keeps_target`, the walk in `crate::player`'s `click_to_move`).
    let moving = armed
        .0
        .and_then(|s| caster.slots[s].as_deref())
        .is_some_and(gd_game::hotbar::is_move_to);
    let (Ok(win), Ok((cam, cam_gt))) = (window.single(), cams.single()) else {
        return;
    };
    let Some(ray) = win
        .cursor_position()
        .and_then(|c| cam.viewport_to_world(cam_gt, c).ok())
    else {
        return;
    };
    let (target, point) = match under_cursor(&hovered, ray, &pickable, me) {
        // Keyboard Only: the controller's cursor record (`cast_input`).
        _ if options.int("movementType") == 2 => {
            (record.target, record.point.unwrap_or(tf.translation))
        }
        Some((e, at)) => (Some(e), at),
        None => (
            None,
            crate::player::pick_ground(ray, &nav.0).unwrap_or(tf.translation),
        ),
    };
    out.write(UseArmedSlot { target, point });
    if !moving {
        capture.0 = true;
        *swallow = true;
    }
}

/// What a held button does after an armed slot's press ([`armed_click`]).
#[derive(Debug, PartialEq, Eq)]
enum ArmedHold {
    /// Still the armed slot's press: nothing else takes it.
    Capture,
    /// The held Move key walks meanwhile.
    Walk,
    /// Let go.
    Done,
}

fn armed_hold(held: bool, move_key: bool) -> ArmedHold {
    match (held, move_key) {
        (false, _) => ArmedHold::Done,
        (true, false) => ArmedHold::Capture,
        (true, true) => ArmedHold::Walk,
    }
}

/// The Move key (51, `tagMove`) or a hot slot key bound to the Move skill is held: the player walks to the cursor
/// (`crate::player`'s `click_to_move`). `Grim Dawn.exe`: the key handler `InGameUI::HandleKeyEvent` sets
/// `InGameUI.moveKeyHeld` on the key's press (a hot slot key: when its slot in the shown row holds
/// `GetDefaultSkillId(3)`, the row kept in `InGameUI.shownHotbarRow`) and clears it on the release; with it
/// set, every mouse event that is no press (the per-frame move event too) is a held Move command
/// (`InGameUI::HandleMouseEvent`: `SetCommandRepeated`, command pending) and, in Classic movement only,
/// `HandleActionFromMouse` gets `a2 = 1`, so `SelectPrimaryAction` skips every object and requests a plain move to
/// the cursor point (the object's coordinates when one is under the cursor). The hot slot key's own request fails
/// (`gd_game::hotbar::MOVE_TO`). PORT: the held state is recomputed every frame (not while the UI takes the keys).
#[derive(Resource, Default)]
pub struct MoveKeyHeld(pub bool);

/// `GD_MOVE_KEY=<secs>`: holds the Move key ([`MoveKeyHeld`]) for that many seconds of play, so the
/// held-key walk (`crate::player::click_to_move`) runs without a keyboard; `GD_HOVER=x,y` aims it.
#[cfg(feature = "dev")]
fn move_key_hook(
    hooks: Res<gd_game_core::hooks::Hooks>,
    time: Res<Time>,
    players: Query<(), With<Player>>,
    mut move_held: ResMut<MoveKeyHeld>,
    mut left: Local<Option<f32>>,
) {
    if players.is_empty() {
        return;
    }
    let t = left.get_or_insert(hooks.gameplay.move_key.unwrap_or(-1.0));
    if *t > 0.0 {
        *t -= time.delta_secs();
        move_held.0 = true;
    }
}

/// Hotbar keys 1–0 (of the row shown, `ui::hud::HotbarRow`) become `gd_game::hotbar::HotSlotKey`
/// presses and releases (gd-game requests the slot at the cursor record and
/// repeats a held key); a release goes to the slot its press used. Presses
/// are not taken while the UI has the input (as [`cast_input`]).
#[allow(clippy::too_many_arguments)]
fn hot_slot_keys(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<gd_ui_kit::keys::KeyBindings>,
    capture: Res<UiCapture>,
    rift_map: Res<gd_game::riftgates::RiftgateMap>,
    altar: Res<gd_ui::state::ascendant::AltarMenu>,
    dialog: Res<gd_game::conversation::Conversation>,
    mut pressed: Local<[Option<usize>; 10]>,
    mut out: MessageWriter<gd_game::hotbar::HotSlotKey>,
    (mut move_held, casters, row): (
        ResMut<MoveKeyHeld>,
        Query<&Caster, With<Player>>,
        Res<gd_ui::hud::HotbarRow>,
    ),
) {
    for i in 0..10 {
        if bindings.just_released(gd_ui_kit::keys::HOT_SLOT_FIRST + i as u32, &keys)
            && let Some(slot) = pressed[i].take()
        {
            out.write(gd_game::hotbar::HotSlotKey { slot, down: false });
        }
    }
    let blocked = capture.0 || rift_map.open || dialog.view.is_some() || altar.open;
    // The Move key, or a held hot slot key whose slot holds the Move skill (`MoveKeyHeld`).
    let held = !blocked
        && (bindings.pressed(gd_ui_kit::keys::MOVE, &keys)
            || casters.single().is_ok_and(|c| {
                pressed.iter().flatten().any(|&s| {
                    c.slots[s]
                        .as_deref()
                        .is_some_and(gd_game::hotbar::is_move_to)
                })
            }));
    if move_held.0 != held {
        move_held.0 = held;
    }
    if blocked {
        return;
    }
    let row2 = row.0 == 1;
    for i in 0..10 {
        if bindings.just_pressed(gd_ui_kit::keys::HOT_SLOT_FIRST + i as u32, &keys) {
            let slot = if row2 { SLOT_ROW2 + i } else { i };
            if let Some(old) = pressed[i].replace(slot) {
                out.write(gd_game::hotbar::HotSlotKey {
                    slot: old,
                    down: false,
                });
            }
            out.write(gd_game::hotbar::HotSlotKey { slot, down: true });
        }
    }
}

/// The mouse buttons become [`CastRequest`]s at the character under the
/// cursor (a foe or one's own pet: `gd_game_core::casting::CastRequest` aims by
/// the skill), else the ground point. While a slot is armed the mouse
/// buttons are its ([`armed_click`]).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn cast_input(
    hooks: Res<gd_game_core::hooks::Hooks>,
    keys: ClickKeys,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    cams: Query<(&Camera, &GlobalTransform), With<gd_render::camera::GameCamera>>,
    nav: Res<Nav>,
    capture: Res<UiCapture>,
    players: Query<
        (Entity, &Transform, &Caster),
        (
            With<Player>,
            Without<Dead>,
            Without<gd_game::movement::Stunned>,
        ),
    >,
    pickable: Pickable,
    rift_map: Res<gd_game::riftgates::RiftgateMap>,
    altar: Res<gd_ui::state::ascendant::AltarMenu>,
    dialog: Res<gd_game::conversation::Conversation>,
    armed: Res<ArmedSlot>,
    mut out: MessageWriter<CastRequest>,
    mut script: Local<Option<(std::time::Instant, Vec<(f32, usize)>)>>,
    (options, record, move_held, gate, hovered): (
        Res<gd_game_core::options::GameOptions>,
        Res<gd_game::hotbar::WorldCursor>,
        Res<MoveKeyHeld>,
        Res<gd_game::movement::MouseCommand>,
        Res<gd_ui::state::hover::Hovered>,
    ),
) {
    // The riftgate map and conversations take the keys while open.
    if capture.0 || rift_map.open || dialog.view.is_some() || altar.open {
        return;
    }
    // A held Move key owns the held buttons (only a press acts as one: [`MoveKeyHeld`]), in Classic movement.
    if move_held.0
        && options.int("movementType") == 0
        && !buttons.any_just_pressed([MouseButton::Left, MouseButton::Right])
    {
        return;
    }
    let mut slot = None;
    // The mouse buttons cast only while the world command gate is open (`gd_game::movement::MouseCommand`, set by
    // `crate::player`'s `click_to_move` earlier this frame; `InGameUI::HandleMouseEvent`): not after a press
    // the UI took
    // (a right press on a window held while the cursor leaves it), nor after an interaction press.
    let mouse = armed.0.is_none() && gate.open();
    if mouse && buttons.pressed(MouseButton::Right) {
        slot = Some(SLOT_RIGHT);
    }
    // Stationary Attack ([`ClickKeys`]) + left button: the left skill at the cursor without moving.
    // In the keyboard movement modes (`movementType` above 0) a left click always uses the primary skill at the cursor,
    // also on empty ground (`SelectPrimaryAction`).
    let force = mouse
        && (keys.stationary() || options.int("movementType") > 0)
        && buttons.pressed(MouseButton::Left);
    if slot.is_none() && force {
        slot = Some(SLOT_LEFT);
    }
    let Ok((me, tf, caster)) = players.single() else {
        return;
    };
    // Test hook: `GD_CAST=slot@secs` casts a hotbar slot at the cursor
    // (only consulted once the player exists, so no entry is lost).
    let tested = slot.is_none();
    let Some(slot) = slot.or_else(|| test_cast(&mut script, &hooks)) else {
        return;
    };
    // The Move skill on the right button only walks (`crate::player`'s `click_to_move`;
    // `InGameUI::HandleMouseEvent` turns the press into the primary action,
    // `gd_game::hotbar::move_slot_keeps_target`).
    if caster.slots[slot]
        .as_deref()
        .is_none_or(|s| slot == SLOT_RIGHT && gd_game::hotbar::is_move_to(s))
    {
        return;
    }
    // Target: the monster under the cursor, else the ground point.
    let (Ok(win), Ok((cam, cam_gt))) = (window.single(), cams.single()) else {
        return;
    };
    let cursor = win
        .cursor_position()
        .unwrap_or(Vec2::new(win.width() * 0.5, win.height() * 0.4));
    let Ok(ray) = cam.viewport_to_world(cam_gt, cursor) else {
        return;
    };
    // Breakables only when targetable (`gd_game_core::world::Breakable`).
    // Force Move ([`ClickKeys`]): the press skips the pick, so it aims at the ground point.
    let mut hit = under_cursor(&hovered, ray, &pickable, me).filter(|_| !keys.force_move());
    // The test hook aims at the nearest monster within 20 m.
    if tested {
        hit = pickable
            .0
            .iter()
            .filter(|m| m.3.is_none_or(|b| b.targetable))
            .map(|m| (m.0, m.1.translation()))
            .filter(|(_, at)| at.distance(tf.translation) < 20.0)
            .min_by(|a, b| {
                a.1.distance(tf.translation)
                    .total_cmp(&b.1.distance(tf.translation))
            })
            .or(hit);
    }
    let mut target = hit.map(|h| h.0);
    let mut point = match hit {
        Some((_, at)) => at,
        None => crate::player::pick_ground(ray, &nav.0)
            .unwrap_or(tf.translation + gd_game_core::character::facing(tf) * 5.0),
    };
    // The Move skill on the left button (Shift / the keyboard modes): only a breakable stays the target, with
    // `breakingMoveTo` (`gd_game::hotbar::move_slot_keeps_target`); gd-game attacks with the default weapon attack.
    // In Keyboard Only the record below replaces the target anyway (`InGameUI::HandleMouseEvent` reads it
    // back after the reset).
    if !tested
        && slot == SLOT_LEFT
        && caster.slots[slot]
            .as_deref()
            .is_some_and(gd_game::hotbar::is_move_to)
        && !gd_game::hotbar::move_slot_keeps(
            false,
            target.is_some_and(|t| matches!(pickable.0.get(t), Ok((_, _, _, Some(_), _)))),
            options.flag("breakingMoveTo"),
        )
    {
        target = None;
    }
    // Keyboard Only (`movementType` 2): the cursor does not aim; the press uses the controller's cursor record (the
    // controller targeting's pick, `crate::controller_target`; `InGameUI::HandleMouseEvent` reads
    // `GetMouseRepeatData` back),
    // and gd-game's `keyboard_aim` turns a request without a target into one along the facing / movement direction.
    if !tested && options.int("movementType") == 2 {
        target = record.target;
        point = record.point.unwrap_or(tf.translation);
    }
    // Test hook `GD_CAST_AT=x,z`: hook casts aim at that ground point.
    if tested && let Some(Vec2 { x, y: z }) = hooks.gameplay.cast_at {
        target = None;
        point = Vec3::new(
            x,
            nav.0
                .ground([x, tf.translation.y + 2.0, z])
                .unwrap_or(tf.translation.y),
            z,
        );
    }
    out.write(CastRequest {
        slot,
        target,
        point,
        force,
        scripted: tested,
    });
}

/// A channel runs while its slot's key / button is held, aimed at the
/// cursor's ground point (the keyboard modes' aim: `gd_game::casting::channels`).
fn channel_input(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<gd_ui_kit::keys::KeyBindings>,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    cams: Query<(&Camera, &GlobalTransform), With<gd_render::camera::GameCamera>>,
    nav: Res<Nav>,
    mut players: Query<&mut Channel, With<Player>>,
) {
    let Ok(mut ch) = players.single_mut() else {
        return;
    };
    if ch.scripted {
        return;
    }
    ch.input_held = match ch.slot {
        SLOT_LEFT => buttons.pressed(MouseButton::Left),
        SLOT_RIGHT => buttons.pressed(MouseButton::Right),
        s => {
            let i = if s >= SLOT_ROW2 { s - SLOT_ROW2 } else { s };
            i < 10 && bindings.pressed(gd_ui_kit::keys::HOT_SLOT_FIRST + i as u32, &keys)
        }
    };
    let (Ok(win), Ok((cam, cgt))) = (window.single(), cams.single()) else {
        ch.input_aim = None;
        return;
    };
    ch.input_aim = win
        .cursor_position()
        .and_then(|c| cam.viewport_to_world(cgt, c).ok())
        .and_then(|ray| crate::player::pick_ground(ray, &nav.0));
}

/// Dresses a projectile's model (`ProjectileMesh`, its record's `glowTexture` replacing the mesh's glow) and keeps
/// it on its projectile; the model goes with it (the meteors vanish on impact; their impact effect leaves the
/// ground decal). The reglowed materials are the model materials' variants (`MaterialAssets::reglow`).
fn projectile_meshes(
    mut commands: Commands,
    mut models: Query<
        (
            Entity,
            &mut Transform,
            &ProjectileModelOf,
            Option<&ProjectileMesh>,
        ),
        Without<Projectile>,
    >,
    projectiles: Query<&Transform, With<Projectile>>,
    mut assets: gd_render::character::CharAssets,
) {
    for (e, mut tf, of, new) in &mut models {
        let Ok(p) = projectiles.get(of.0) else {
            commands.entity(e).try_despawn();
            continue;
        };
        tf.translation = p.translation;
        tf.rotation = p.rotation;
        let Some(m) = new else { continue };
        // The model and its glow texture load in the background: `ProjectileMesh` stays until they are in (or the
        // model cannot load).
        let model = match assets.model_ready(&m.0) {
            gd_render::model::Ready::Loading => continue,
            gd_render::model::Ready::Failed => {
                commands.entity(e).remove::<ProjectileMesh>();
                continue;
            }
            gd_render::model::Ready::Done(model) => model,
        };
        if let Some(glow) = &m.2 {
            let data = assets.data.clone();
            if !assets
                .textures
                .ready(&data, glow, false, &mut assets.images)
            {
                continue;
            }
        }
        commands.entity(e).remove::<ProjectileMesh>();
        match &m.2 {
            Some(glow) => {
                gd_render::model::spawn_model_with(&mut commands, e, &model, |_, def| {
                    assets.mats.reglow(
                        &assets.data,
                        def,
                        glow,
                        &mut assets.textures,
                        &mut assets.images,
                    )
                });
            }
            None => gd_render::model::spawn_model(&mut commands, e, &model),
        }
        tf.scale = Vec3::splat(m.1);
    }
}

/// Places effect decals (`decal.rs`: the effect's `decal` pick, frame,
/// dynamic geometry, the level decals' material; fading in
/// `decal::fade_decals`). The surface: the terrain under the effect and,
/// unless `terrainOnly`, the static meshes in its box (`DecalReceivers`).
/// `Decal::Load` / `UpdateSelf`: `life`
/// (s, a missing field reads 0) ≤ 0 never removes the decal. An owned decal
/// follows that (then goes with its effect); PORT: a one-shot effect's decal
/// without a `life` gets 3 s (the engine keeps it and its effect forever). PORT: a decal whose textures are not
/// loaded yet waits ([`ColdDecal`]) while they decode in the background ([`DecalTextureLoads`]) and is placed on a
/// later frame, instead of decoding them on the spot.
#[allow(clippy::too_many_arguments)]
fn fx_decals(
    mut commands: Commands,
    coll: Res<crate::collision::WorldCollision>,
    recv: Res<gd_render::decal::DecalReceivers>,
    new: Query<(Entity, &PendingDecal, &Transform, Option<&DecalOwner>)>,
    cold: Query<(Entity, &ColdDecal)>,
    mut assets: gd_render::character::CharAssets,
    mut decal_mats: ResMut<Assets<gd_render::decal::DecalMaterial>>,
    mut loads: ResMut<DecalTextureLoads>,
    options: Res<gd_game_core::options::GameOptions>,
) {
    use bevy::tasks::block_on;
    use bevy::tasks::futures_lite::future;
    let a = &mut assets;
    loads
        .0
        .retain(|key, t| match block_on(future::poll_once(t)) {
            Some(img) => {
                a.textures.put(key, false, img, &mut a.images);
                false
            }
            None => true,
        });
    for (e, d) in &cold {
        if !decal_textures_ready(&d.dec, &mut assets, &mut loads) {
            continue;
        }
        commands.entity(e).try_despawn();
        place_fx_decal(
            &mut commands,
            &coll,
            &recv,
            &mut assets,
            &mut decal_mats,
            &d.dec,
            d.at,
            d.rot,
            d.owner,
        );
    }
    for (e, d, tf, owner) in &new {
        commands.entity(e).try_despawn();
        // `detailObjects` gates every decal.
        if !options.flag("detailObjects") {
            continue;
        }
        let owner = owner.map(|o| o.0);
        let Some(fx) = assets.data.db.get(&d.0) else {
            continue;
        };
        // A `Decal` record itself (a `Lightning2` strike decal, `lightning.rs`); else `EffectEntity::Load`:
        // `decalChance` %, one of the `decal` list.
        let (pick, decal_rotate) = if fx.class == "Decal" {
            (d.0.clone(), false)
        } else {
            let chance = fx.i32("decalChance").filter(|&c| c != 0).unwrap_or(100);
            let list = fx.strs("decal");
            if list.is_empty() || gd_game_core::rng::i32(0..=100) > chance {
                continue;
            }
            (
                list[gd_game_core::rng::usize(..list.len())].to_owned(),
                fx.i32("decalRotate").unwrap_or(0) != 0,
            )
        };
        let Some(rec) = assets.data.db.get(&pick) else {
            continue;
        };
        let Some(mut dec) = gd_render::decal::DecalRecord::from_record(rec) else {
            continue;
        };
        // Engine.dll `Lightning2::Generate`: the strike decal's `Decal.minNormalAngle` (`Decal::Load`'s
        // π/2 − `maxAngle`, whose sine `CreateGeometry` takes as the faces' least normal y) is set to −π/2, so
        // it keeps every mesh face.
        if fx.class == "Decal" {
            dec.min_normal_y = (-std::f32::consts::FRAC_PI_2).sin();
        }
        if dec.timing.life_ms <= 0 && owner.is_none() {
            dec.timing.life_ms = 3000;
        }
        // The frame: the effect's coords, yawed at random with `decalRotate`.
        let rot = if decal_rotate {
            Quat::from_rotation_y(gd_game_core::rng::f32() * std::f32::consts::TAU)
        } else {
            tf.rotation
        };
        let c = tf.translation;
        debug!(
            "fx decal {pick} at {c} (terrain {:?})",
            coll.terrain_ground(c)
        );
        if !decal_textures_ready(&dec, &mut assets, &mut loads) {
            commands.spawn(ColdDecal {
                dec,
                at: c,
                rot,
                owner,
            });
            continue;
        }
        place_fx_decal(
            &mut commands,
            &coll,
            &recv,
            &mut assets,
            &mut decal_mats,
            &dec,
            c,
            rot,
            owner,
        );
    }
}

/// An effect decal [`fx_decals`] resolved (the picked record, its frame and owner) whose textures are still
/// decoding in the background ([`DecalTextureLoads`]).
#[derive(Component)]
struct ColdDecal {
    dec: gd_render::decal::DecalRecord,
    at: Vec3,
    rot: Quat,
    owner: Option<Entity>,
}

/// Effect decal textures decoding in the background (`gd_render::textures::decode`), by normalized path; filed into
/// `Textures` as they land ([`fx_decals`]).
#[derive(Resource, Default)]
struct DecalTextureLoads(
    gd_formats::collections::HashMap<String, bevy::tasks::Task<Option<Image>>>,
);

/// Whether the textures `decal::spawn` takes for `dec` (texture, bump, glow; UNORM) are loaded (or failed); the
/// ones that are not start decoding in the background.
fn decal_textures_ready(
    dec: &gd_render::decal::DecalRecord,
    assets: &mut gd_render::character::CharAssets,
    loads: &mut DecalTextureLoads,
) -> bool {
    let mut ready = true;
    for t in std::iter::once(dec.texture.as_str())
        .chain(dec.bump.as_deref())
        .chain(dec.glow.as_deref())
    {
        if assets.textures.cached(t, false, &mut assets.images) {
            continue;
        }
        ready = false;
        let key = gd_formats::vfs::normalized(t);
        if loads.0.contains_key(key.as_ref()) {
            continue;
        }
        let key = key.into_owned();
        let data = (*assets.data).clone();
        let k = key.clone();
        let task = bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
            gd_render::textures::decode(&data, &k, false)
                .inspect_err(|e| warn!("texture {k}: {e:#}"))
                .ok()
        });
        loads.0.insert(key, task);
    }
    ready
}

/// Places the effect decal `dec` at `c` with local Z `rot * Z` ([`ground_patch`]), fading with `owner`.
#[allow(clippy::too_many_arguments)]
fn place_fx_decal(
    commands: &mut Commands,
    coll: &crate::collision::WorldCollision,
    recv: &gd_render::decal::DecalReceivers,
    assets: &mut gd_render::character::CharAssets,
    decal_mats: &mut Assets<gd_render::decal::DecalMaterial>,
    dec: &gd_render::decal::DecalRecord,
    c: Vec3,
    rot: Quat,
    owner: Option<Entity>,
) {
    let Some(mesh) = ground_patch(coll, recv, dec, c, rot, true) else {
        return;
    };
    let mesh = assets.meshes.add(mesh);
    let a = assets;
    for layer in gd_render::decal::spawn(
        commands,
        dec,
        mesh,
        &a.mats.defaults,
        &a.data,
        &mut a.textures,
        &mut a.images,
        decal_mats,
        Some(owner),
    ) {
        commands.entity(layer).insert(Name::new("fx decal"));
    }
}

/// The decal mesh of `dec` centred at `c` with local Z `rot * Z`: the terrain
/// under it and (unless `terrainOnly`) the static meshes in its box
/// (`decal.rs`; `dynamic`: the 0.7 box of a `Decal.dynamic` decal).
fn ground_patch(
    coll: &crate::collision::WorldCollision,
    recv: &gd_render::decal::DecalReceivers,
    dec: &gd_render::decal::DecalRecord,
    c: Vec3,
    rot: Quat,
    dynamic: bool,
) -> Option<Mesh> {
    let (w, h) = (dec.width, dec.height);
    let ext = gd_render::decal::extent(w, h, dynamic);
    let mut patch = gd_render::decal::DecalPatch::default();
    if let Some((origin, tw, th, heights)) = coll.terrain_at(c) {
        let height = |x: i32, z: i32| {
            (x >= 0 && z >= 0 && (x as u32) < tw && (z as u32) < th)
                .then(|| heights[(z as u32 * tw + x as u32) as usize])
        };
        patch.gather(
            &gd_render::decal::Surface::Terrain {
                origin,
                height: &height,
            },
            c,
            ext,
        );
    }
    if !dec.terrain_only {
        recv.gather(&mut patch, c, ext, dec, dynamic);
    }
    patch.build(c, rot * Vec3::Z, w, h)
}

/// The placed particle decals, oldest first, whether a decal came in since the ring's alphas were set, and the
/// shared quad mesh per decal size (`f32::to_bits` of width, height).
#[derive(Resource, Default)]
pub(crate) struct DropDecals(
    std::collections::VecDeque<Vec<Entity>>,
    bool,
    gd_formats::collections::HashMap<(u32, u32), Handle<Mesh>>,
);

/// `World::PlaceDecalOnGround` → `PutOnFloor` →
/// `World::PlaceDecal` (only while the option is on,
/// `detailObjects` in `GameOptions`) → `DecalSet::AddDecal`: the region's
/// set keeps 256 decals in a ring (the next one replaces the oldest), no
/// life. The decal is a flat quad in the particle's coords at the floor
/// point (`decal::quad`, `DecalSet::RenderPass`), not projected onto
/// the ground; each decal's alpha is its rank in the ring ([`ring_alpha`]),
/// and a body's contact decal grows in (`decal::GrowIn` on the `DropDecal`).
/// Particles' decals tilt to the terrain normal ([`GroundTilt`]), a body's
/// contact decal to the contact normal (`monsters.rs`). PORT: one ring for
/// the whole world instead of one per region; a decal goes with the level it lies in as the region's set does
/// (`gd_game::monsters::LevelProp`) and leaves the ring then; `Emitter::UpdateParticles`
/// also drops the decal where `Terrain::IsPointVisible` fails (not ported).
/// Blood pools are engine-identical (blend, size, ring alpha, 8
/// particle + 5 gib decals per kill): the solid-blob look is overlapping
/// patchy decals, engine behavior.
#[allow(clippy::too_many_arguments)]
fn drop_decals(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut commands: Commands,
    nav: Res<gd_game::nav::Nav>,
    coll: Res<crate::collision::WorldCollision>,
    new: Query<(
        Entity,
        &DropDecal,
        &Transform,
        Option<&gd_render::decal::GrowIn>,
        Has<GroundTilt>,
    )>,
    options: Res<gd_game_core::options::GameOptions>,
    mut placed: ResMut<DropDecals>,
    mut assets: gd_render::character::CharAssets,
    mut decal_mats: ResMut<Assets<gd_render::decal::DecalMaterial>>,
    mut records: Local<
        gd_formats::collections::HashMap<String, Option<gd_render::decal::DecalRecord>>,
    >,
    mut mod_changed: MessageReader<gd_render::model::ModChanged>,
    (levels, layout, live): (
        Res<gd_game_core::levels::Levels>,
        Option<Res<gd_game_core::save::MapLayout>>,
        Query<()>,
    ),
) {
    const RING: usize = 256;
    // The parsed decal records are the mounted mod's (`model::drop_on_mod_change`).
    if mod_changed.read().next().is_some() {
        records.clear();
    }
    // The decals whose level unloaded are out of the ring.
    if !new.is_empty() {
        placed.0.retain(|d| d.iter().any(|e| live.contains(*e)));
    }
    for (e, d, tf, grow, tilt) in &new {
        commands.entity(e).try_despawn();
        if !options.flag("detailObjects") || hooks.render.no_decals {
            continue;
        }
        if !records.contains_key(&*d.0) {
            let rec = assets
                .data
                .db
                .get(&d.0)
                .and_then(gd_render::decal::DecalRecord::from_record);
            records.insert(d.0.to_string(), rec);
        }
        let Some(dec) = &records[&*d.0] else {
            continue;
        };
        // `PutOnFloor`: the ground under the particle.
        let mut c = tf.translation;
        let floor = nav
            .0
            .ground((c + Vec3::Y).to_array())
            .into_iter()
            .chain(coll.terrain_ground(c))
            .fold(None::<f32>, |a, y| {
                Some(a.map_or(y, |a| {
                    if (y - c.y).abs() < (a - c.y).abs() {
                        y
                    } else {
                        a
                    }
                }))
            });
        let Some(y) = floor else { continue };
        c.y = y;
        // The quad about its origin, placed (and grown) by the transform; one mesh per size.
        let mesh = placed
            .2
            .entry((dec.width.to_bits(), dec.height.to_bits()))
            .or_insert_with(|| {
                assets.meshes.add(gd_render::decal::quad(
                    Vec3::ZERO,
                    Quat::IDENTITY,
                    dec.width,
                    dec.height,
                    0.01,
                ))
            })
            .clone();
        let rot = if tilt {
            Quat::from_rotation_arc(Vec3::Y, coll.terrain_normal(c).unwrap_or(Vec3::Y))
                * tf.rotation
        } else {
            tf.rotation
        };
        let at = Transform::from_translation(c)
            .with_rotation(rot)
            .with_scale(Vec3::splat(grow.map_or(1.0, |g| g.0)));
        let a = &mut assets;
        let layers = gd_render::decal::spawn(
            &mut commands,
            dec,
            mesh,
            &a.mats.defaults,
            &a.data,
            &mut a.textures,
            &mut a.images,
            &mut decal_mats,
            None,
        );
        let level = layout.as_ref().and_then(|m| levels.level_at(&m.0, c));
        for l in &layers {
            let mut ec = commands.entity(*l);
            ec.insert((Name::new("particle decal"), at));
            if let Some(g) = grow {
                ec.insert(*g);
            }
            if let Some(level) = level {
                ec.insert(gd_game::monsters::LevelProp(level));
            }
        }
        debug!(
            "particle decal {} at {c} ({} x {}, grow {})",
            d.0,
            dec.width,
            dec.height,
            grow.is_some()
        );
        placed.0.push_back(layers);
        placed.1 = true;
        while placed.0.len() > RING {
            for l in placed.0.pop_front().unwrap_or_default() {
                if let Ok(mut ec) = commands.get_entity(l) {
                    ec.despawn();
                }
            }
        }
    }
}

/// `DecalSet::Update`'s per-decal alpha after decals came in
/// ([`gd_render::decal::ring_alpha`]).
fn ring_alpha(
    mut placed: ResMut<DropDecals>,
    layers: Query<&MeshMaterial3d<gd_render::decal::DecalMaterial>>,
    mut mats: ResMut<Assets<gd_render::decal::DecalMaterial>>,
) {
    if !placed.1 {
        return;
    }
    placed.1 = false;
    let n = placed.0.len();
    for (k, decal) in placed.0.iter().enumerate() {
        let a = gd_render::decal::ring_alpha(n, k);
        for m in decal.iter().filter_map(|e| layers.get(*e).ok()) {
            if let Some(mut mat) = mats.get_mut(&m.0) {
                gd_render::decal::set_layer_alpha(&mut mat, a);
            }
        }
    }
}
