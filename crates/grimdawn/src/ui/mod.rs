//! The UI that stays with the binary: the windows coupled to the audio mixer, the world streaming or
//! the menu scene (`credits`, `survival`, `options`, `localmap`, `riftmap`, `cinematic`, `view`,
//! `mainmenu`, `quicktip`), plus the quest-view collector (it reads the streamed map). The movable
//! window clusters, the rebuild hub, the Esc dispatcher and the shared presentation state are `gd_ui`;
//! this module registers the staying plugins, fills [`gd_ui::HudValues`] before the rebuild chain
//! (`view::collect`) and resets the staying state on Exit to Main Menu.
pub mod cinematic;
pub mod credits;
pub mod localmap;
pub mod mainmenu;
pub mod options;
pub mod quests;
pub mod quicktip;
pub mod riftmap;
pub mod survival;
pub mod view;

use bevy::prelude::*;

// The shared kit, imported privately so the staying window modules keep their `super::` paths.
use gd_formats::ui::{Rect as URect, Widget};
use gd_ui_kit::{LiveText, TextAt, Ui, UiAssets, UiInputSet, contains, cursor_px, set_node_rect};
use gd_ui_kit::{additive, keys, text, widgets};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            survival::SurvivalUiPlugin,
            credits::CreditsPlugin,
            options::OptionsUiPlugin,
            mainmenu::MainMenuUiPlugin,
            cinematic::CinematicPlugin,
            quicktip::QuickTipPlugin,
        ))
        // The HUD view collector fills gd_ui's HudValues / HudVitals before the rebuild chain.
        .add_systems(
            Update,
            view::collect
                .in_set(gd_ui_kit::HudSet)
                .before(gd_ui::HudBuildSet),
        )
        // The quest view collector fills gd_ui's QuestView before the quest window chain.
        .add_systems(
            Update,
            quests::collect
                .in_set(gd_ui_kit::UiPlaySet)
                .before(gd_ui::quests::QuestSet),
        )
        // The crafting list scrolls in place after the rebuild and the NPC window clicks.
        .add_systems(
            Update,
            crate::crafting::craft_scroll
                .after(gd_ui_kit::HudSet)
                .after(gd_ui::state::trade::TradeInputSet)
                .in_set(gd_ui_kit::UiPlaySet),
        );
    }
}

/// Resets the staying UI state a session leaves, after `gd_ui::reset_session` (`exit/mod.rs`).
pub fn reset_session(world: &mut World) {
    gd_ui::reset_session(world);
    world.insert_resource(localmap::MapTiles::default());
    world.insert_resource(gd_ui::state::survival::SurvivalView::default());
    world.insert_resource(survival::EndlessView::default());
    world.insert_resource(survival::GlowState::default());
    world.insert_resource(gd_ui::state::survival::PaneRollovers::default());
    world.insert_resource(survival::GlowOverlay::default());
    world.insert_resource(survival::SunderedOverlay::default());
    world.insert_resource(survival::MutatorView::default());
    // Keeps the once-per-process main-menu tip flag (`QuickTip::menu_tip_done`).
    if let Some(mut tip) = world.get_resource_mut::<quicktip::QuickTip>() {
        tip.end_session();
    } else {
        world.insert_resource(quicktip::QuickTip::default());
    }
    world.insert_resource(cinematic::CinematicQueue::default());
    world.insert_resource(options::OptionsLayout::default());
    world.insert_resource(credits::Credits::default());
}
