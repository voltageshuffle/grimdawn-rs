//! PORT: the `maxFPS` frame limiter (the engine has no frame-rate cap option; 0, the default, is
//! uncapped). A `Last` system sleeps out the rest of the frame's budget (1 / `maxFPS`); the deadline
//! advances by one budget per frame. A deadline already past skips the sleep and re-phases from now,
//! so no catch-up burst follows. The cap applies focused or not.
use bevy::prelude::*;
use gd_game_core::options::GameOptions;
use std::time::{Duration, Instant};

pub struct FpsLimitPlugin;

impl Plugin for FpsLimitPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Last, pace);
    }
}

fn pace(options: Res<GameOptions>, mut next: Local<Option<Instant>>) {
    let fps = options.int("maxFPS");
    if fps <= 0 {
        *next = None;
        return;
    }
    let budget = Duration::from_secs_f64(1.0 / fps as f64);
    let now = Instant::now();
    let deadline = match *next {
        Some(t) if t > now => t,
        _ => now + budget,
    };
    std::thread::sleep(deadline.saturating_duration_since(now));
    *next = Some(deadline + budget);
}
