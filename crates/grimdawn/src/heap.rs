//! Returning freed memory to the system. glibc's allocator keeps what the program frees inside its arenas, so
//! after a level unloads or a session ends the resident set would stay at its peak. After such a free event
//! (fewer loaded levels, lingering levels let go, a new `SessionEpoch`) and [`SETTLE`] of real time with no level
//! building or spawning (`StreamedMap::busy`), a background task runs `malloc_trim(0)`. While streaming is busy
//! the trim waits: the trim takes each arena's lock while the spawns allocate.
//! PORT: the engine's own allocator is not ported. Other allocators / platforms: nothing to do.
use bevy::prelude::*;

/// How long after a free event, with streaming idle, the trim runs.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const SETTLE: std::time::Duration = std::time::Duration::from_secs(10);

pub struct HeapPlugin;

impl Plugin for HeapPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        app.add_systems(
            Last,
            trim_after_free.after(gd_game::levels::despawn_unloaded),
        );
        #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
        let _ = app;
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[derive(Default)]
struct Trim {
    levels: usize,
    /// `Lingering::released` last frame.
    released: u64,
    /// When the pending trim is due (real time since startup).
    due: Option<std::time::Duration>,
    running: Option<bevy::tasks::Task<()>>,
}

/// Starts a trim [`SETTLE`] after a level unloaded, lingering levels were let go or a session ended, once streaming
/// is idle and unless the last one still runs.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn trim_after_free(
    levels: Res<gd_game_core::levels::Levels>,
    lingering: Res<gd_world::Lingering>,
    streamed: Option<Res<gd_world::StreamedMap>>,
    epoch: Res<gd_game_core::session::SessionEpoch>,
    time: Res<Time<Real>>,
    mut trim: Local<Trim>,
) {
    let now = time.elapsed();
    let loaded = levels.loaded.len();
    let released = lingering.released();
    if loaded < trim.levels || released != trim.released || epoch.is_changed() {
        trim.due = Some(now + SETTLE);
    }
    (trim.levels, trim.released) = (loaded, released);
    if trim.due.is_some() && streamed.is_some_and(|s| s.busy()) {
        trim.due = Some(now + SETTLE);
        return;
    }
    if trim.due.is_none_or(|d| now < d) || trim.running.as_ref().is_some_and(|t| !t.is_finished()) {
        return;
    }
    trim.due = None;
    trim.running = Some(bevy::tasks::AsyncComputeTaskPool::get().spawn(async {
        // SAFETY: `malloc_trim` only releases free pages of glibc's arenas; any thread may call it.
        unsafe { libc::malloc_trim(0) };
    }));
}
