//! PORT: the `maxFPS` frame limiter (the engine has no frame-rate cap option; `maxFPS` 0, the default,
//! is uncapped). A `Last` system sleeps out the rest of the frame's budget (1 / `maxFPS`): the deadline
//! advances by exactly one budget per frame, so the frame period holds the cap without drift, also with
//! vsync off. A deadline already past means the frame overran its budget (a hitch, loading) — the sleep
//! is skipped and the cadence re-phases from now, so no catch-up burst follows.
//!
//! Unfocused behaviour is untouched: `inactiveUpdateRate` pauses virtual time and sound
//! (`gd_game_core::pause`, `gd_audio::mixer`), not frame delivery — the cap applies focused or not.
//! The pacing phase is a `Local`, not session state: it self-corrects against `Instant::now()` every
//! frame, so a session change needs no reset.
use bevy::prelude::*;
use gd_game_core::options::GameOptions;
use std::time::{Duration, Instant};

pub struct FpsLimitPlugin;

impl Plugin for FpsLimitPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Last, pace);
    }
}

/// Sleeps until this frame's deadline (the previous one plus the budget), then schedules the next.
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
