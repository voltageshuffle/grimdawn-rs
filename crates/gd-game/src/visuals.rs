//! The presentation-request systems: the sounds a removed [`StopSound`]
//! plays and the `GD_SHOT_ON` feed (the request data is
//! [`gd_game_core::visuals`]).
use bevy::prelude::*;
use gd_game_core::visuals::{PlaySound, PlaySoundOn, StopSound, notify};

pub struct VisualsPlugin;

impl Plugin for VisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PlaySound>()
            .add_message::<gd_game_core::visuals::CameraShake>()
            .add_message::<PlaySoundOn>()
            .add_message::<gd_game_core::visuals::PointDisturbance>()
            .add_message::<gd_game_core::visuals::LineDisturbance>()
            .add_observer(stop_sounds)
            .add_systems(
                Update,
                shot_feed.run_if(|h: Res<gd_game_core::hooks::Hooks>| h.shot.on.is_some()),
            );
    }
}

fn stop_sounds(
    ev: On<Remove<StopSound>>,
    q: Query<&StopSound>,
    at: Query<&Transform>,
    mut out: MessageWriter<PlaySound>,
) {
    if let Ok(s) = q.get(ev.entity)
        && let Ok(tf) = at.get(s.on)
    {
        out.write(PlaySound {
            pak: s.pak.clone(),
            at: Some(tf.translation),
        });
    }
}

/// `GD_SHOT_ON` feed from the shared messages: every sound played (`PlaySound` / `PlaySoundOn`, the `SoundPak`
/// record) and every proc cast (`ProcCast`, the skill record), whichever system sent them. Casts, FX starts and the
/// other hook sites feed [`notify`] directly.
fn shot_feed(
    mut commands: Commands,
    mut sounds: MessageReader<PlaySound>,
    mut tracked: MessageReader<PlaySoundOn>,
    mut procs: MessageReader<gd_game_core::casting::ProcCast>,
) {
    for s in sounds.read() {
        notify(&mut commands, &s.pak);
    }
    for s in tracked.read() {
        notify(&mut commands, &s.pak);
    }
    for p in procs.read() {
        notify(&mut commands, &p.cast.skill);
    }
}
