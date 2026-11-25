//! Pet order input. The Pet Attack skill needs none of its own: on a hotbar
//! slot it is a `CastRequest` like any skill. Portraits and their stance menu: `ui::pets`.
//!
//! Selection (`InGameUI::TogglePetSelection`, `InGameUI::OrderSelectedPets`, `InGameUI::ClearPetSelection`,
//! `InGameUI::HandleMouseEvent`, `InGameUI::HandleKeyEvent` / `UIPlayerHud::HandleKeyAction`): the selected
//! pets are UI state ([`PetSelection`], in selection order). A pet of the player is toggled by a left click on
//! its portrait, by a left click on the pet in the world while the Target Pet key is held (action 38; such a
//! click does nothing else), and by Select Pet 1–5 (actions 44–48: the n-th portrait); Select All Pets (action
//! 49) clears the selection and toggles the first five portraits. With pets selected, the next world left click
//! orders each of them (one `PetCommand` per pet: the foe under the cursor is attacked, else they move to the
//! ground point), clears the selection and does nothing else; Esc clears it. A selected pet would get the
//! `gameengine.dbr` `petBannerName` banner and play its `petAcknowledgeSound` (no stock record sets either).
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_game::nav::Nav;
use gd_game::pets::{Pet, PetCommand, portrait_pets};
use gd_game_core::combat::{Combatant, Dead};
use gd_game_core::player::Player;
use gd_ui::state::inventory::UiCapture;
use gd_ui::state::pets::{PetInputSet, PetSelection};

pub struct PetInputPlugin;

impl Plugin for PetInputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PetSelection>()
            .add_plugins(gd_ui::pets::PetPortraitPlugin)
            .add_systems(
                Update,
                (
                    #[cfg(feature = "dev")]
                    stance_hook,
                    world_input
                        .in_set(PetInputSet)
                        .in_set(gd_ui_kit::UiInputSet)
                        .before(gd_ui::state::casting::CastInputStage::Cast),
                )
                    .before(gd_game::casting::CastingSet),
            );
    }
}

#[cfg(feature = "dev")]
fn stance_hook(
    hooks: Res<gd_game_core::hooks::Hooks>,
    players: Query<Entity, With<Player>>,
    pets: Query<(Entity, &Pet)>,
    mut hook: Local<Option<Option<gd_game::pets::Stance>>>,
    mut out: MessageWriter<gd_game::pets::SetPetStance>,
) {
    use gd_game::pets::{SetPetStance, Stance};
    let hooked = hook.get_or_insert_with(|| {
        hooks.ui.pet_stance.as_deref().and_then(|s| match s {
            "normal" => Some(Stance::Normal),
            "aggressive" => Some(Stance::Aggressive),
            "defensive" => Some(Stance::Defensive),
            _ => None,
        })
    });
    let Some(stance) = *hooked else { return };
    let Ok(owner) = players.single() else { return };
    let mine: Vec<Entity> = pets
        .iter()
        .filter(|(_, p)| p.owner == owner)
        .map(|(e, _)| e)
        .collect();
    if mine.is_empty() {
        return;
    }
    *hooked = None;
    info!("pets: stance {stance:?}");
    for pet in mine {
        out.write(SetPetStance { pet, stance });
    }
}

#[allow(clippy::too_many_arguments)]
pub fn world_input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    cams: Query<(&Camera, &GlobalTransform), With<gd_render::camera::GameCamera>>,
    nav: Res<Nav>,
    mut capture: ResMut<UiCapture>,
    mut selection: ResMut<PetSelection>,
    players: Query<Entity, With<Player>>,
    pets: Query<(Entity, &Pet, &GlobalTransform, &Combatant), Without<Dead>>,
    monsters: Query<
        (
            Entity,
            &GlobalTransform,
            &Combatant,
            gd_game::combat::SideData,
        ),
        Without<Dead>,
    >,
    mut out: MessageWriter<PetCommand>,
    mut swallow: Local<bool>,
    bindings: Res<gd_ui_kit::keys::KeyBindings>,
    #[cfg(feature = "dev")] (hooks, time, mut hook): (
        Res<gd_game_core::hooks::Hooks>,
        Res<Time>,
        Local<Option<Vec<(f32, Option<Vec2>)>>>,
    ),
) {
    let Ok(owner) = players.single() else { return };
    // What an order may attack: the characters the player's side calls foes.
    let foes = || {
        monsters
            .iter()
            .filter(|m| gd_game::combat::Side::Player.foe(m.3.side_ref()))
            .map(|m| (m.0, m.1, m.2))
    };
    // Gone or dead pets drop out.
    selection
        .0
        .retain(|e| pets.get(*e).is_ok_and(|(_, p, _, _)| p.owner == owner));
    // The click that gave an order (or toggled a pet) moves nobody while held.
    if *swallow {
        if buttons.pressed(MouseButton::Left) {
            capture.0 = true;
            return;
        }
        *swallow = false;
    }
    if keys.just_pressed(KeyCode::Escape) {
        selection.0.clear();
    }
    let order = || portrait_pets(owner, pets.iter().map(|(e, p, _, _)| (e, p)));
    if let Some(i) =
        (0..5).position(|i| bindings.just_pressed(gd_ui_kit::keys::PET_SELECT_FIRST + i, &keys))
    {
        if let Some(&pet) = order().get(i) {
            selection.toggle(pet);
        }
    } else if bindings.just_pressed(gd_ui_kit::keys::PET_SELECT_ALL, &keys) {
        selection.0.clear();
        for pet in order().into_iter().take(5) {
            selection.toggle(pet);
        }
    }
    #[cfg(feature = "dev")]
    let hook = hook.get_or_insert_with(|| hooks.ui.pet_order.clone());
    #[cfg(feature = "dev")]
    if let Some(i) = hook.iter().position(|(t, _)| time.elapsed_secs() >= *t) {
        let (_, at) = hook.remove(i);
        if selection.0.is_empty()
            && let Some(&pet) = order().first()
        {
            selection.toggle(pet);
        }
        let me_y = pets
            .iter()
            .find(|p| p.1.owner == owner)
            .map_or(0.0, |p| p.2.translation().y);
        let (target, point) = match at {
            None => {
                let me = pets
                    .get(selection.0.first().copied().unwrap_or(owner))
                    .map(|p| p.2.translation())
                    .unwrap_or_default();
                let near = foes().min_by(|a, b| {
                    a.1.translation()
                        .distance(me)
                        .total_cmp(&b.1.translation().distance(me))
                });
                (near.map(|m| m.0), near.map_or(me, |m| m.1.translation()))
            }
            // A click there: the monster under it, else the ground.
            Some(xz) => match foes().find(|m| m.1.translation().xz().distance(xz) < 1.5) {
                Some(m) => (Some(m.0), m.1.translation()),
                None => (
                    None,
                    nav.0
                        .nearest([xz.x, me_y + 2.0, xz.y], 4.0)
                        .map_or(Vec3::new(xz.x, me_y, xz.y), |c| Vec3::from(nav.0.pos(c))),
                ),
            },
        };
        info!("GD_PET_ORDER: target {target:?} point {point}");
        give_order(owner, &mut selection, target, point, &mut out);
        return;
    }
    let pet_key = bindings.pressed(gd_ui_kit::keys::TARGET_PET, &keys);
    if capture.0 || !buttons.just_pressed(MouseButton::Left) || (!pet_key && selection.0.is_empty())
    {
        return;
    }
    let (Ok(win), Ok((cam, cam_gt))) = (window.single(), cams.single()) else {
        return;
    };
    let Some(cursor) = win.cursor_position() else {
        return;
    };
    let Ok(ray) = cam.viewport_to_world(cam_gt, cursor) else {
        return;
    };
    capture.0 = true;
    *swallow = true;
    if pet_key {
        // The Target Pet key: toggle the own pet under the cursor.
        let mine = pets
            .iter()
            .filter(|(_, p, _, _)| p.owner == owner)
            .map(|(e, _, gt, cb)| (e, gt, cb));
        if let Some(pet) = crate::combat::pick_monster(ray, mine) {
            selection.toggle(pet);
        }
        return;
    }
    let target = crate::combat::pick_monster(ray, foes());
    let point = match target.and_then(|t| monsters.get(t).ok()) {
        Some((_, gt, ..)) => gt.translation(),
        None => match crate::player::pick_ground(ray, &nav.0) {
            Some(p) => p,
            None => return,
        },
    };
    give_order(owner, &mut selection, target, point, &mut out);
}

/// One order per selected pet (`InGameUI::OrderSelectedPets`), then the selection clears
/// (`InGameUI::ClearPetSelection`).
fn give_order(
    owner: Entity,
    selection: &mut PetSelection,
    target: Option<Entity>,
    point: Vec3,
    out: &mut MessageWriter<PetCommand>,
) {
    info!(
        "pets: order to {} selected pet(s), target {target:?}",
        selection.0.len()
    );
    for &pet in &selection.0 {
        out.write(PetCommand {
            owner,
            pet: Some(pet),
            target,
            point,
        });
    }
    selection.0.clear();
}
