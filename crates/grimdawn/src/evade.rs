//! Evade input: Space becomes a `gd_game::evade::EvadeRequest`.
use bevy::prelude::*;
pub use gd_game::evade::*;
use gd_game_core::player::Player;

pub struct EvadeInputPlugin;

impl Plugin for EvadeInputPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            evade_input
                .after(gd_game_core::player::PlayerSet)
                .before(start_evade),
        );
        #[cfg(feature = "dev")]
        app.add_systems(
            Update,
            evade_hook
                .after(gd_game_core::player::PlayerSet)
                .before(start_evade),
        );
    }
}

type Ready<'w, 's> = Query<
    'w,
    's,
    (),
    (
        With<Player>,
        Without<Evading>,
        Without<gd_game_core::combat::Dead>,
        Without<gd_game::movement::Stunned>,
    ),
>;

fn evade_input(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<gd_ui_kit::keys::KeyBindings>,
    players: Ready,
    mut out: MessageWriter<EvadeRequest>,
) {
    if !players.is_empty() && bindings.just_pressed(gd_ui_kit::keys::EVADE, &keys) {
        out.write(EvadeRequest { scripted: false });
    }
}

#[cfg(feature = "dev")]
fn evade_hook(
    hooks: Res<gd_game_core::hooks::Hooks>,
    time: Res<Time>,
    players: Ready,
    mut left: Local<Option<f32>>,
    mut out: MessageWriter<EvadeRequest>,
) {
    if players.is_empty() {
        return;
    }
    let t = left.get_or_insert(hooks.gameplay.evade.unwrap_or(-1.0));
    if *t > 0.0 {
        *t -= time.delta_secs();
        if *t <= 0.0 {
            out.write(EvadeRequest { scripted: true });
        }
    }
}
