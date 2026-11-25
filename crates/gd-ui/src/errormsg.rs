//! The red "That skill is not ready" text (`tagSkillNotReady`), the one thing the `errorMessages` option gates.
//!
//! Grim Dawn.exe `InGameUI` draw `InGameUI::Render`: with the option on and the player's
//! message flag raised by `HotSlotOptionSkill::PlayCooldownVox` (a hot slot skill pressed while it
//! cools down, `Player.cooldownVoxFlag`; the flag lasts 2000 ms, `Player` ctor), the tag is drawn in
//! `style_skillcooldowntext.dbr`, red (1, 0, 0), centred on the viewport's centre x and 300 px above its centre.
//! A new press restarts the 2 s. PORT: the 300 px are screen pixels, unscaled.
use super::UiAssets;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_game_core::data::GameData;

const STYLE: &str = "records/ui/styles/text/style_skillcooldowntext.dbr";
/// How long the text stays (ms).
const MS: f32 = 2000.0;

#[derive(Resource, Default)]
pub struct ErrorMessage {
    left_ms: f32,
}

#[derive(Component)]
struct ErrorRoot;

pub struct ErrorMessagePlugin;

impl Plugin for ErrorMessagePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ErrorMessage>()
            .add_systems(Update, show_error.in_set(gd_ui_kit::UiPlaySet));
    }
}

#[allow(clippy::too_many_arguments)]
fn show_error(
    mut fresh: gd_game_core::session::NewSession,
    mut commands: Commands,
    mut requests: MessageReader<gd_game_core::casting::SkillNotReady>,
    mut state: ResMut<ErrorMessage>,
    options: Res<gd_game_core::options::GameOptions>,
    clock: Res<Time<Real>>,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    roots: Query<Entity, With<ErrorRoot>>,
    mut shown: Local<Option<(UVec2, bool)>>,
    mut assets: UiAssets,
    #[cfg(feature = "dev")] (hooks, mut fired): (Res<gd_game_core::hooks::Hooks>, Local<bool>),
) {
    if fresh.begun() {
        *state = ErrorMessage::default();
        *shown = None;
    }
    let pressed = requests.read().count() > 0;
    // `GD_NOT_READY` (hooks.rs): raised once at that time.
    #[cfg(feature = "dev")]
    let pressed = hooks.gameplay.not_ready.is_some_and(|t| {
        let hit = !*fired && clock.elapsed_secs() >= t;
        *fired |= hit;
        hit
    }) || pressed;
    if pressed && options.flag("errorMessages") {
        state.left_ms = MS;
    }
    let on = state.left_ms > 0.0;
    state.left_ms = (state.left_ms - clock.delta_secs() * 1000.0).max(0.0);
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if *shown == Some((size, on)) {
        return;
    }
    *shown = Some((size, on));
    super::window::despawn_all(&mut commands, &roots);
    if !on {
        return;
    }
    let root = gd_ui_kit::spawn_root(&mut commands, ErrorRoot, 2);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        assets.scale(size),
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    // `{^r}` colour codes are the style's: the text is red whole.
    let text = gd_formats::text::strip_markup(ui.fonts.tag("tagSkillNotReady"));
    let (w, h) = (ui.text_width(STYLE, &text), ui.line_height(STYLE));
    let at = Vec2::new(
        size.x as f32 * 0.5 - w * 0.5,
        size.y as f32 * 0.5 - 300.0 - h * 0.5,
    );
    let e = ui.text_line_tinted(STYLE, at, &text, Color::srgb(1.0, 0.0, 0.0));
    debug!("errormsg: {text:?} at {at:?} -> {e:?}");
}
