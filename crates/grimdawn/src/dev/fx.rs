//! Effect checks: `GD_FX` (hooks.rs) places effects in a row next to the spawn point (a
//! `FxPak` record as a skill spawns it: sound, shake, mesh and particles); `GD_FX_GRID=n` repeats each as an
//! n×n grid (performance checks). With `GD_FPS` the effect counters are logged every second. Registered by
//! `dev::DevPlugin` (not with `GD_NO_FX`).
use bevy::prelude::*;
use gd_game_core::data::GameData;
use gd_game_core::hooks::Hooks;
use gd_render::fx::{Effect, EffectSpec, FxStats, FxView, simulated, spawn_effect_record};

pub fn plugin(app: &mut App) {
    app.add_systems(PostStartup, scene)
        .add_systems(Last, log_stats.run_if(|hooks: Res<Hooks>| hooks.render.fps));
}

fn scene(
    hooks: Res<Hooks>,
    mut commands: Commands,
    world: Option<Res<crate::world::StreamedMap>>,
    data: Res<GameData>,
) {
    let Some(list) = &hooks.render.fx else {
        return;
    };
    let grid = hooks.render.fx_grid;
    let base = world.and_then(|w| w.spawn_point()).unwrap_or(Vec3::ZERO);
    for (i, p) in list.split(';').filter(|s| !s.is_empty()).enumerate() {
        for gx in 0..grid {
            for gz in 0..grid {
                let off = Vec3::new(
                    i as f32 * 4.0 - 4.0 + gx as f32 * 3.0,
                    0.0,
                    3.0 + gz as f32 * 3.0,
                );
                let t = Transform::from_translation(base + off);
                if data.db.get(p).is_some_and(|r| r.class == "FxPak") {
                    gd_game_core::visuals::spawn_fx(&mut commands, &data, p, t);
                } else if p.ends_with(".dbr") {
                    spawn_effect_record(&mut commands, p, t);
                } else {
                    commands.spawn((EffectSpec::pfx(p), t, Visibility::default()));
                }
            }
        }
    }
}

/// Every second: the effects, how many are simulated, their particles and the mean update time.
fn log_stats(
    time: Res<Time>,
    stats: Res<FxStats>,
    mut acc: Local<(f32, f32, u32)>,
    fx: Query<(
        &Effect,
        &GlobalTransform,
        Option<&InheritedVisibility>,
        Has<FxView>,
    )>,
    cams: Query<&gd_render::camera::GameCamera>,
) {
    acc.0 += time.delta_secs();
    acc.1 += stats.update_ms;
    acc.2 += 1;
    if acc.0 < 1.0 {
        return;
    }
    let focus = cams.iter().next().map(|c| c.target);
    let (mut active, mut particles) = (0, 0);
    for (effect, g, inherited, other_view) in &fx {
        if simulated(g.translation(), inherited, other_view, focus) {
            active += 1;
        }
        particles += effect
            .emitters
            .iter()
            .map(|em| em.state.particles.len())
            .sum::<usize>();
    }
    info!(
        "fx: {} effects, {active} simulated, {particles} particles, update {:.2} ms/frame",
        fx.iter().len(),
        acc.1 / acc.2 as f32
    );
    *acc = (0.0, 0.0, 0);
}
