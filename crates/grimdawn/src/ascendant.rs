//! The Ascendant Altar's presence in the world and the state of its window (`gd_game::ascendant` holds the use and
//! activation rules, `gd_sim::ascendant` the formulas, `ui::altar` draws the window).
//!
//! The altar (`gd_game::ascendant::Altar`, level object) is spawned by gd-game and dressed here once its model is
//! built (it loads in the background, `CharAssets::model_ready`; [`dress_altars`]). A click (or the Interact key on
//! it) is the player's interact order (`gd_game::player_orders`),
//! which walks there; in reach gd-game answers with `AltarOpen`, which opens the altar window at that altar ([`AltarMenu`]). The window's offering in
//! progress is window state, as in the engine (the boxes are the window's item boxes until the activate button
//! sends `ControllerCharacter::SendAltarOfferCmd`, Grim Dawn.exe `AltarWindow::OnActivate`): the window sends that
//! offer as `gd_game::ascendant::AltarOffer`. The Altar Window key opens the window anywhere as a view of the boxes
//! as activated (`altar_key`).
//!
//! The Dread (`superBossSpawn`, Ultimate Veteran list): every kill by the
//! player's side counts (unless the player holds `superBossDisableToken`
//! and no super-boss formula is installed); at
//! `ascendant::super_boss_kills` a `super_boss_chance` % roll spawns it 20 m
//! away (`FindSuperBossSpawnPoint(…, 20)`), hating the player.
//! `GameEngine::OnCreatureDeath`: kills are not counted while the last
//! spawned Dread is alive; every kill at or past the count rolls (a failed
//! roll keeps the count); the count restarts only when one spawns.
//!
//! Hooks (hooks.rs): `GD_USE_ALTAR` uses the nearest altar within 40 m;
//! `GD_ALTAR_OFFER` fills the boxes in order and activates (reagents given
//! if missing, `ui::altar`); `GD_UI_OPEN=altar` opens the window as a view (`ui::altar`); `GD_DREAD_KILLS`
//! overrides the kill count needed.
use bevy::prelude::*;
use gd_ui::state::ascendant::AltarMenu;
use gd_game::ascendant::{Altar, AltarOpen, AscendantStage};
use gd_game_core::player::Player;
use gd_render::character::CharAssets;
use gd_render::model::Ready;

pub struct AscendantPlugin;

impl Plugin for AscendantPlugin {
    fn build(&self, app: &mut App) {
        app.register_required_components::<Altar, Visibility>()
            .init_resource::<AltarMenu>()
            .add_systems(
                Update,
                (
                    dress_altars.before(altar_looks),
                    altar_looks
                        .after(gd_game_core::player::PlayerSet)
                        .before(AscendantStage::Use),
                    (open_at_altar, close_far, altar_key)
                        .chain()
                        .after(AscendantStage::Use)
                        .before(AscendantStage::Altar),
                ),
            );
    }
}

/// The altar's shown state (its looping effect).
#[derive(Component, Default)]
struct AltarLooks {
    shown: Option<bool>,
    fx: Option<Entity>,
}



/// A new altar gets its model and its shown state once its model is built (the dressing seam; [`AltarLooks`]
/// marks a dressed one).
fn dress_altars(
    mut commands: Commands,
    new: Query<(Entity, &Altar), Without<AltarLooks>>,
    mut a: CharAssets,
) {
    for (e, altar) in &new {
        let db = a.data.db.clone();
        let model = match db
            .get(&altar.record)
            .and_then(|r| r.str("mesh"))
            .map(|m| a.model_ready(m))
        {
            Some(Ready::Loading) => continue,
            Some(Ready::Done(model)) => Some(model),
            Some(Ready::Failed) | None => None,
        };
        commands.entity(e).try_insert(AltarLooks::default());
        if let Some(model) = model {
            gd_render::model::spawn_model(&mut commands, e, &model);
        }
    }
}

fn altar_looks(
    mut commands: Commands,
    altar: Res<gd_game_core::ascendant::AltarState>,
    mut altars: Query<(Entity, &Altar, &mut AltarLooks)>,
    data: Res<gd_game_core::data::GameData>,
) {
    for (e, a, mut looks) in &mut altars {
        if looks.shown == Some(altar.activated) {
            continue;
        }
        looks.shown = Some(altar.activated);
        let Some(rec) = data.db.get(&a.record) else {
            continue;
        };
        if let Some(fx) = looks.fx.take() {
            commands.entity(fx).try_despawn();
        }
        if altar.activated
            && let Some(r) = rec
                .str("dormantToActivatedEffect")
                .filter(|r| !r.is_empty())
        {
            let once = gd_render::fx::spawn_effect_record(&mut commands, r, Transform::IDENTITY);
            commands.entity(e).add_child(once);
        }
        let key = if altar.activated {
            "activatedLoopingEffect"
        } else {
            "dormantLoopingEffect"
        };
        if let Some(r) = rec.str(key).filter(|r| !r.is_empty()) {
            let fx = gd_render::fx::spawn_effect_record(&mut commands, r, Transform::IDENTITY);
            commands.entity(e).add_child(fx);
            looks.fx = Some(fx);
        }
    }
}

/// gd-game's answer to a use of the dormant altar (`gd_game::ascendant::use_altar`): the window opens at the altar
/// with empty boxes.
fn open_at_altar(mut opened: MessageReader<AltarOpen>, mut menu: ResMut<AltarMenu>) {
    if let Some(o) = opened.read().last() {
        menu.show(Some(o.0), Default::default());
    }
}

/// The window opened at an altar hides once the player is farther from it than [`gd_game::ascendant::altar_reach`]
/// (Grim Dawn.exe `AltarWindow::CloseIfFar`; the reach is a PORT stand-in, see there).
fn close_far(
    mut menu: ResMut<AltarMenu>,
    altars: Query<(&GlobalTransform, &Altar)>,
    players: Query<(&Transform, &gd_game_core::combat::Combatant), With<Player>>,
    data: Res<gd_game_core::data::GameData>,
) {
    let Some(e) = menu.altar.filter(|_| menu.open) else {
        return;
    };
    let Ok((tf, pcb)) = players.single() else {
        return;
    };
    let far = altars.get(e).map_or(true, |(gt, a)| {
        tf.translation.distance(gt.translation())
            > gd_game::ascendant::altar_reach(&data.db, a, pcb.radius)
    });
    if far {
        menu.open = false;
    }
}

/// The Altar Window key (action 61, no default key) toggles the offering menu anywhere. `Grim Dawn.exe`
/// HUD key action `UIPlayerHud::HandleKeyAction` case 61 → `UIPlayerHud::ToggleAltarWindow`: when
/// `MainPlayerCanUseAscendantAltar` and the third expansion is loaded, the other windows close
/// (`UIPlayerHud::CloseWindows`) and the altar window (`UIPlayerHud.altarWindow`) flips its visibility; no altar
/// nearby is needed. The window's show (`AltarWindow::Show`) fills the boxes from
/// `GameEngine::GetActivatedAltarReagents` ([`gd_game_core::ascendant::AltarState::reagents`]: empty while dormant) and,
/// with no altar (`AltarWindow.altarId` = 0), disables the activate button (`AltarWindow.activateButton`) and the six
/// boxes (`AltarWindow.offerBoxes`): the key opens a view of the active offering, never an offering. PORT: the
/// expansions are always loaded.
fn altar_key(
    (keys, bindings): (Res<ButtonInput<KeyCode>>, Res<gd_ui_kit::keys::KeyBindings>),
    (hooks, data, altar): (
        Res<gd_game_core::hooks::Hooks>,
        Res<gd_game_core::data::GameData>,
        Res<gd_game_core::ascendant::AltarState>,
    ),
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    (dialog, rift_map): (
        Res<gd_game::conversation::Conversation>,
        Res<gd_game::riftgates::RiftgateMap>,
    ),
    mut menu: ResMut<AltarMenu>,
    mut panels: ResMut<gd_ui::UiPanels>,
    (mut journal, mut map, mut factions): (
        ResMut<gd_ui::quests::QuestJournal>,
        ResMut<gd_ui::state::map::MapWindow>,
        ResMut<gd_ui::factions::FactionsView>,
    ),
) {
    if !bindings.just_pressed(gd_ui_kit::keys::ALTAR_WINDOW, &keys)
        || dialog.view.is_some()
        || rift_map.open
        || !gd_game::ascendant::may_use_altar(&hooks, &data.db, rt.as_deref())
    {
        return;
    }
    if menu.open {
        menu.open = false;
        return;
    }
    panels.character = false;
    panels.skills = false;
    panels.devotion = false;
    journal.open = false;
    map.open = false;
    factions.open = false;
    menu.show(None, altar.reagents.clone());
}
