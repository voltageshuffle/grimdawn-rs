//! Returning freed memory to the system. glibc's allocator keeps what the program frees inside its arenas (one per
//! thread that allocates; only free space at an arena's top goes back), so after an unloaded level's assets go (its
//! models, textures, effects and sounds go with their last holder, `gd_game_core::weak_cache`; its models and terrain
//! layers only once they stop lingering, `gd_world::Lingering`) or a session ends, the resident set would stay at its
//! peak. After such a free event (fewer loaded levels, lingering levels let go, a new `SessionEpoch`) and `SETTLE` of
//! real time with no level building or spawning (`StreamedMap::busy`), a background task runs `malloc_trim(0)`, which
//! hands the free pages of every arena back. While streaming is busy the trim waits: the pages would be faulted in
//! again by the loads, and the trim takes each arena's lock while the spawns allocate.
//! PORT: the engine's own allocator is not ported; this only keeps the port's resident memory near what it uses.
//! Other allocators / platforms: nothing to do.
use bevy::prelude::*;

/// How long after a free event, with streaming idle, the trim runs: the despawned entities' assets are freed over
/// the next frames, and a level often unloads while the next ones stream in.
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

/// [`trim_after_free`]'s state.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[derive(Default)]
struct Trim {
    /// The levels loaded last frame.
    levels: usize,
    /// `Lingering::released` last frame.
    released: u64,
    /// When the pending trim is due (real time since startup).
    due: Option<std::time::Duration>,
    /// The trim running in the background.
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
    // Streaming: the settle starts over once it is idle.
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
