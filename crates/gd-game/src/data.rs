//! Mounts the install as the [`GameData`] resource at startup (the data
//! access itself is [`gd_game_core::data`]).
use bevy::prelude::*;
use gd_game_core::data::GameData;
use std::path::PathBuf;

/// Mounts the install at `root` as the [`GameData`] resource. The binary resolves `root` once
/// (`grimdawn`'s `main`: `--game-dir`, then the executable's and the working folder, see
/// [`gd_game_core::data::find_install`]); everything after reads it back as [`GameData::root`].
pub struct DataPlugin {
    pub root: PathBuf,
}

impl Plugin for DataPlugin {
    fn build(&self, app: &mut App) {
        let root = &self.root;
        let t = std::time::Instant::now();
        let data = GameData::open(root).expect("open game data");
        // `GD_SURVIVAL`: a Crucible process (hooks are inserted before the plugins).
        if app
            .world()
            .get_resource::<gd_game_core::hooks::Hooks>()
            .is_some_and(|h| h.session.survival.is_some())
            && let Err(e) = data.set_mod(Some(gd_formats::vfs::SURVIVAL_MOD))
        {
            error!("GD_SURVIVAL: the Crucible does not mount: {e:#}");
        }
        info!(
            "mounted {} files, {} records from {} in {:?}",
            data.vfs.paths().count(),
            data.db.len(),
            root.display(),
            t.elapsed()
        );
        app.insert_resource(data);
    }
}
