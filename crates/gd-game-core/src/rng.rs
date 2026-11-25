//! The game's random draws: one wyrand generator (`fastrand::Rng`) per thread,
//! for gameplay and presentation randomness.
//!
//! Not `fastrand`'s own thread-local generator: Bevy's task pool forks that one
//! (`async_executor::Executor::run`, `futures_lite::future::race`) a
//! timing-dependent number of times.
//!
//! Same wyrand sequence as `fastrand`'s free functions for a seed; unseeded, each
//! thread starts from a random seed. Seeded determinism holds only for the
//! headless single-threaded `TestGame`.
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

/// Restarts this thread's generator from `seed`.
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
