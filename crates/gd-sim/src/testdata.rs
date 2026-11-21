//! gd-sim's data-test fixtures: the install's database and archives, opened
//! once per test binary. Missing data panics unless `GD_SKIP_DATA_TESTS=1`
//! (then None and the test skips): see [`gd_formats::testdata`].
pub use gd_formats::testdata::*;
use gd_formats::vfs::{Database, Vfs};
use std::sync::{Arc, OnceLock};

/// The installed game's merged database (opened once).
pub fn db() -> Option<Arc<Database>> {
    static DB: OnceLock<Arc<Database>> = OnceLock::new();
    let dir = game()?;
    Some(
        DB.get_or_init(|| Arc::new(Database::open(&dir).expect("open the game database")))
            .clone(),
    )
}

/// The installed game's merged archives (opened once).
pub fn vfs() -> Option<Arc<Vfs>> {
    static VFS: OnceLock<Arc<Vfs>> = OnceLock::new();
    let dir = game()?;
    Some(
        VFS.get_or_init(|| Arc::new(Vfs::open(&dir).expect("open the game archives")))
            .clone(),
    )
}
