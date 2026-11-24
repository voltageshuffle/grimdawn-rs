//! The HUD's notification widget (`records/ui/generic/notifications/notification_manager.dbr`, loaded by the HUD
//! loader `UIPlayerHud::Load` into `UIPlayerHud.notificationWidget`; load `NotificationWidget::Load`, update
//! `NotificationWidget::Update`, draw `NotificationWidget::Render` /
//! `NotificationWidget::RenderText`). The queue and its timing are gd-game's ([`gd_game::notify`]); this draws the one
//!shown.
//!
//! There: the window rect comes from the record's `windowDefault*` fields (stock: 500 × 0 px, centred,
//! 100 px above the screen's centre, `WindowDef`); the lines are drawn centred on the window's centre, the block of
//! `lines × line height` centred vertically on it, in `fontStyle` (`style_quest_notification`: Libertine 18 bold,
//! orange) with the style's colour times the fade alpha. `backgroundTop` / `backgroundCenter` / `backgroundBottom`
//! (three stretched bitmaps around the text) are drawn only when set; the stock record sets none. One string is one
//! line (`AddUINotification`'s single-string form; no wrapping).
use super::UiAssets;
use crate::fade::{Fader, fade_group, seal_fade_group};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::WindowDef;
use gd_game_core::data::GameData;

const MANAGER: &str = "records/ui/generic/notifications/notification_manager.dbr";

pub struct NotifyPlugin;

impl Plugin for NotifyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, show.in_set(super::UiPlaySet));
    }
}

#[derive(Component)]
struct NotifyRoot;

/// Draws the shown notification; built once per (text, screen) in a [`FadeGroup`], the fade alpha
/// moved in place ([`Fader`]), not by a rebuild per alpha step.
#[allow(clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    roots: Query<Entity, With<NotifyRoot>>,
    mut assets: UiAssets,
    mut fresh: gd_game_core::session::NewSession,
    mut drawn: Local<Option<(String, UVec2)>>,
    mut group: Local<Option<Entity>>,
    mut fader: Fader,
) {
    if fresh.begun() {
        *drawn = None;
        *group = None;
    }
    let shown = rt.as_ref().and_then(|r| r.notices.current());
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let key = shown.map(|(t, _)| (t.to_owned(), size));
    if *drawn != key {
        super::window::despawn_all(&mut commands, &roots);
        *drawn = key;
        *group = None;
        let (Some((text, alpha)), Some(rec)) = (shown, data.db.get(MANAGER)) else {
            return;
        };
        let Some(style) = rec.str("fontStyle").map(str::to_owned) else {
            return;
        };
        let s = assets.scale(size);
        let screen = size.as_vec2();
        let r = WindowDef::from_record(rec).screen_rect(screen.x, screen.y, s);
        let root = gd_ui_kit::spawn_root(&mut commands, NotifyRoot, 205);
        let mut ui = assets.ui(
            &mut commands,
            &data,
            root,
            s,
            1.0 / win.scale_factor(),
            screen,
        );
        // Built opaque; the group fades it.
        let g = fade_group(&mut ui, root, alpha);
        let lh = ui.line_height(&style);
        let lines: Vec<&str> = text.lines().collect();
        let cx = r.x + r.w * 0.5;
        let top = r.y + r.h * 0.5 - (lh * lines.len() as f32 * 0.5).floor();
        for (i, l) in lines.iter().enumerate() {
            let w = ui.text_width(&style, l);
            ui.text_line(
                &style,
                Vec2::new((cx - w * 0.5).round(), (top + lh * i as f32).round()),
                l,
                1.0,
            );
        }
        seal_fade_group(ui.commands, g);
        *group = Some(g);
    }
    if let (Some(g), Some((_, alpha))) = (*group, shown) {
        fader.set(g, alpha);
    }
}
