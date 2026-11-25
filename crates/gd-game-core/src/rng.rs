//! The game's random draws: one wyrand generator (`fastrand::Rng`) per
//! thread. gd-game's gameplay draws from it, and so does presentation
//! randomness (gd-render's water, chaos beams, lightning and FX meshes;
//! grimdawn's breakables, cast decals, gibs and the menu's arena pick).
//!
//! Not `fastrand`'s own thread-local generator: Bevy's task pool draws from
//! that one (`async_executor::Executor::run` and `futures_lite::future::race`
//! call `fastrand::Rng::new()`, which forks it) whenever a scope's tasks
//! outlive their first poll on the calling thread: a timing-dependent number of
//! draws that shifted a seeded headless game's dice (`tests/game/determinism.rs`).
//!
//! The functions mirror `fastrand`'s free functions, so draws are the same
//! wyrand sequence for a seed. Unseeded, each thread starts from a random seed.
//! Seeded determinism is promised only for the headless `TestGame`: no render,
//! and a single-threaded executor, so the drawing systems run on one thread and
//! none of the presentation draws above happen there.
use std::cell::Cell;
use std::ops::RangeBounds;

thread_local! {
    static RNG: Cell<fastrand::Rng> = Cell::new(fastrand::Rng::new());
}

fn with<T>(f: impl FnOnce(&mut fastrand::Rng) -> T) -> T {
    RNG.with(|c| {
        let mut r = c.replace(fastrand::Rng::with_seed(0));
        let out = f(&mut r);
        c.set(r);
        out
    })
}

/// Restarts this thread's generator from `seed` (`TestGame`'s seed).
pub fn seed(seed: u64) {
    with(|r| r.seed(seed));
}

pub fn f32() -> f32 {
    with(|r| r.f32())
}

pub fn u8(range: impl RangeBounds<u8>) -> u8 {
    with(|r| r.u8(range))
}

pub fn u32(range: impl RangeBounds<u32>) -> u32 {
    with(|r| r.u32(range))
}

pub fn i32(range: impl RangeBounds<i32>) -> i32 {
    with(|r| r.i32(range))
}

pub fn usize(range: impl RangeBounds<usize>) -> usize {
    with(|r| r.usize(range))
}

pub fn shuffle<T>(slice: &mut [T]) {
    with(|r| r.shuffle(slice));
}
