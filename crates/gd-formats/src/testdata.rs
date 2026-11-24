//! Test support: where the data tests find the game, and their rule for missing data.
//! Runtime code never uses it (the game finds its install itself: `grimdawn`'s `main`).
//!
//! The install is `GD_ROOT` ([`game_dir`]; there is no default) and is the only input from outside
//! the repo. Data tests get their inputs through [`require`] (or [`game`]): a missing file, or an
//! unset `GD_ROOT`, **panics** with what is missing, so a moved or renamed file fails the test
//! instead of passing it silently; with `GD_SKIP_DATA_TESTS=1` it returns None and the test skips
//! (a machine without the game). Files a test writes go in a [`TempDir`].
use std::path::{Path, PathBuf};

/// The installed game: `GD_ROOT`, None when unset.
pub fn game_dir() -> Option<PathBuf> {
    std::env::var_os("GD_ROOT").map(PathBuf::from)
}

/// True when `GD_SKIP_DATA_TESTS=1`: data tests skip instead of failing on
/// missing data.
pub fn skip_data_tests() -> bool {
    std::env::var_os("GD_SKIP_DATA_TESTS").is_some_and(|v| v == "1")
}

/// `path` when it exists; else None under `GD_SKIP_DATA_TESTS=1`, otherwise a
/// panic naming it.
pub fn require(path: impl AsRef<Path>) -> Option<PathBuf> {
    let path = path.as_ref();
    if path.exists() {
        return Some(path.to_owned());
    }
    if skip_data_tests() {
        return None;
    }
    panic!(
        "game data not found at {}; set GD_SKIP_DATA_TESTS=1 to skip",
        path.display()
    )
}

/// The game install ([`game_dir`]) when its database is there (see [`require`]). An unset `GD_ROOT`
/// panics like a missing file (None under `GD_SKIP_DATA_TESTS=1`).
pub fn game() -> Option<PathBuf> {
    let Some(dir) = game_dir() else {
        if skip_data_tests() {
            return None;
        }
        panic!("GD_ROOT is not set: set GD_ROOT to the Grim Dawn install or GD_SKIP_DATA_TESTS=1");
    };
    require(dir.join("database"))?;
    Some(dir)
}

/// A fresh folder under the system temp dir that a test owns: removed with
/// everything in it when the guard drops, also when the test panics. Only
/// folders made by [`TempDir::new`] in this run are ever removed.
pub struct TempDir(PathBuf);

impl TempDir {
    /// Creates `<tmp>/<prefix>-<pid>-<n>` (a new, empty folder).
    pub fn new(prefix: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "{prefix}-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("create {}: {e}", path.display()),
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}
