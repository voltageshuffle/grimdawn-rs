//! PORT: the `maxFPS` frame limiter (the engine has no frame-rate cap option; 0, the default, is
//! uncapped). A `Last` system sleeps out the rest of the frame's budget (1 / `maxFPS`); the deadline
//! advances by one budget per frame. A deadline already past skips the sleep and re-phases from now,
//! so no catch-up burst follows. The cap applies focused or not. Pacing sleeps only the bulk of the
//! remaining budget (minus a 2ms margin) and spin-loops the rest: Windows `std::thread::sleep`
//! quantises to the ~15.6ms system timer tick, which alone would undershoot e.g. a 60 FPS cap to
//! ~50 FPS.
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
    let margin = Duration::from_millis(2);
    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let left = deadline - now;
        if left > margin {
            std::thread::sleep(left.saturating_sub(margin));
        } else {
            std::hint::spin_loop();
        }
    }
    *next = Some(deadline + budget);
}
