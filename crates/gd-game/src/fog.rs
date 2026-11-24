//! Fog of war (map exploration), per difficulty in
//! `levels_world001.map/<Difficulty>/map.fow` (format and reveal rule:
//! `gd_formats::save::fow`). The game reveals around the player through
//! `World::AddFOWVisibility` (Engine.dll): only when the player has
//! moved at least 1 m (x or z) since the last call, for every region touching a
//! box of `kVisibleRange` (30 m) around the player, in the region's local
//! coordinates. Each region's grid is created fully fogged on first use
//! (`FogOfWar::Initialize` with the region's bbox width/depth, 2 * half extents)
//! and the map's `FOWManager` keeps every region it knows for the next save.
//!
//! The data is exposed as [`FogOfWar`] for a minimap: [`FogOfWar::is_in_fog`]
//! for a world position, or the per-region [`FogGrid`]s.
//! [`reveal`] reveals around the player (by map region of the open map,
//! [`gd_game_core::save::MapLayout`]). Engine.dll `World::AddFOWVisibility` /
//! `FogOfWar::Reveal` / `Region::GetFogOfWar`: each region has two
//! grids revealed together, the saved one (`Region.fogOfWar`, [`FogOfWar::grids`])
//! and a session one (`Region.sessionFogOfWar`, [`FogOfWar::session`]: fully fogged on first use,
//! never saved), which the local map greys by (grimdawn `ui::localmap::raster`).
use bevy::prelude::*;
use gd_formats::collections::HashMap;
use gd_formats::save::fow::{FogGrid, Fow, FowRegion};

/// Explored areas of the session's difficulty.
#[derive(Resource, Default)]
pub struct FogOfWar {
    pub difficulty: usize,
    /// Region level name (lowercase) -> grid.
    pub grids: HashMap<String, FogGrid>,
    /// Region names in file order (kept for the save).
    pub order: Vec<String>,
    /// Region level name (lowercase) -> this session's grid (module doc).
    pub session: HashMap<String, FogGrid>,
    pub last: Option<Vec2>,
    /// The reveals that changed a cell of either grid, counted: what the local map draws by (it greys by the session
    /// grid). The maps draw the box around [`Self::last`] again when their count moves by one
    /// (`grimdawn/src/ui/localmap`).
    pub revision: u64,
    /// The reveals that changed a cell of the saved grids, counted: what the minimap draws by (it never shows the
    /// session grid, so walking explored ground leaves it alone).
    pub saved_revision: u64,
    /// The open map's region names lowercased, by region index: the grid keys, built once.
    keys: std::sync::OnceLock<Vec<String>>,
}

impl FogOfWar {
    pub fn dir_name(difficulty: usize) -> &'static str {
        ["Normal", "Elite", "Ultimate"][difficulty.min(2)]
    }

    pub fn from_fow(difficulty: usize, fow: &Fow) -> Self {
        let mut s = Self {
            difficulty,
            ..Default::default()
        };
        for r in &fow.regions {
            match FogGrid::parse(&r.data) {
                Ok(g) => {
                    let k = r.name.to_ascii_lowercase();
                    s.order.push(r.name.clone());
                    s.grids.insert(k, g);
                }
                Err(e) => warn!("fog: region {}: {e:#}", r.name),
            }
        }
        s
    }

    /// The `map.fow` content for the save.
    pub fn to_fow(&self) -> Fow {
        let regions = self
            .order
            .iter()
            .filter_map(|n| {
                self.grids.get(&n.to_ascii_lowercase()).map(|g| FowRegion {
                    name: n.clone(),
                    data: g.encode(),
                })
            })
            .collect();
        Fow {
            version: 1,
            regions,
        }
    }

    /// The grid key (lowercase level name) of every region of `map`, by region index.
    fn keys(&self, map: &gd_formats::map::MapFile) -> &[String] {
        let keys = self.keys.get_or_init(|| {
            map.regions
                .iter()
                .map(|r| r.name.to_ascii_lowercase())
                .collect()
        });
        debug_assert_eq!(keys.len(), map.regions.len(), "fog: one map per session");
        keys
    }

    /// `World::IsInFog`-style query for a world position (true when the region
    /// has no grid yet).
    pub fn is_in_fog(&self, map: &gd_formats::map::MapFile, p: Vec3) -> bool {
        for (r, key) in map.regions.iter().zip(self.keys(map)) {
            let (lo, hi) = bounds(r);
            if p.x >= lo.x && p.x <= hi.x && p.z >= lo.z && p.z <= hi.z {
                let o = gd_game_core::levels::region_origin(r);
                return self
                    .grids
                    .get(key)
                    .is_none_or(|g| g.is_in_fog(p.x - o.x, p.z - o.z));
            }
        }
        true
    }

    pub fn reveal_at(&mut self, map: &gd_formats::map::MapFile, p: Vec3) {
        // `GetRegionsInBox`: the regions touching the 30 m box.
        let m = gd_formats::save::fow::VISIBLE_RANGE;
        self.keys(map);
        let Self {
            grids,
            order,
            session,
            revision,
            saved_revision,
            keys,
            ..
        } = self;
        let keys = keys.get().expect("built above");
        let (mut changed, mut saved_changed) = (false, false);
        for (r, key) in map.regions.iter().zip(keys) {
            let (lo, hi) = bounds(r);
            if p.x + m < lo.x || p.x - m > hi.x || p.z + m < lo.z || p.z - m > hi.z {
                continue;
            }
            let o = gd_game_core::levels::region_origin(r);
            let new = || FogGrid::new(r.bbox.half[0] * 2.0, r.bbox.half[2] * 2.0);
            if !grids.contains_key(key) {
                order.push(r.name.clone());
                grids.insert(key.clone(), new());
            }
            if !session.contains_key(key) {
                session.insert(key.clone(), new());
            }
            let saved = grids.get_mut(key).unwrap().reveal(p.x - o.x, p.z - o.z);
            let sess = session.get_mut(key).unwrap().reveal(p.x - o.x, p.z - o.z);
            changed |= saved | sess;
            saved_changed |= saved;
        }
        if changed {
            *revision += 1;
        }
        if saved_changed {
            *saved_revision += 1;
        }
    }
}

/// `World::AddFOWVisibility` around the living player, once it has moved at least 1 m (x or z) since the last
/// reveal (module doc).
pub fn reveal(
    mut fog: ResMut<FogOfWar>,
    map: Res<gd_game_core::save::MapLayout>,
    players: Query<
        &Transform,
        (
            With<gd_game_core::player::Player>,
            Without<gd_game_core::combat::Dead>,
        ),
    >,
) {
    let Ok(p) = players.single() else {
        return;
    };
    let xz = Vec2::new(p.translation.x, p.translation.z);
    if fog
        .last
        .is_some_and(|l| (l.x - xz.x).abs() < 1.0 && (l.y - xz.y).abs() < 1.0)
    {
        return;
    }
    fog.last = Some(xz);
    fog.reveal_at(&map.0, p.translation);
}

pub fn init_fog(
    mut commands: Commands,
    // Option: `fog` is the existence check (this system creates it); `loaded` is absent for a new character.
    fog: Option<Res<FogOfWar>>,
    travel: Res<crate::riftgates::TravelState>,
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
    choice: Res<gd_game_core::session::Choice>,
    save_dir: Res<gd_game_core::session::SaveDir>,
    mut shared: ResMut<gd_game_core::stash::SharedFiles>,
) {
    if fog.is_some() {
        return;
    }
    let d = travel.difficulty.min(2);
    let mut s = FogOfWar {
        difficulty: d,
        ..Default::default()
    };
    // The map file loads under the save guard (`stash::read_guarded`): unreadable → set aside / block,
    // never rewritten from the empty runtime (`save.rs`).
    if let Some(c) = &loaded {
        let rel = std::path::Path::new(&choice.map_folder())
            .join(FogOfWar::dir_name(d))
            .join("map.fow");
        if let Some(path) = choice.save_file(&rel) {
            let target = choice
                .player_folder(&save_dir, &c.0.header.name, c.0.info.is_in_main_quest != 0)
                .join(&rel);
            if let Some(f) =
                gd_game_core::stash::read_guarded(&mut shared, &path, &target, Fow::parse)
            {
                s = FogOfWar::from_fow(d, &f);
            }
        }
    }
    info!("fog: {} regions explored", s.grids.len());
    commands.insert_resource(s);
}

pub fn bounds(r: &gd_formats::map::RegionInfo) -> (Vec3, Vec3) {
    let c = gd_game_core::levels::region_origin(r) + Vec3::from(r.bbox.center);
    let h = Vec3::from(r.bbox.half);
    (c - h, c + h)
}

/// Loads the session's fog of war and reveals around the player.
pub struct FogPlugin;

impl Plugin for FogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                init_fog
                    .after(gd_game_core::player::PlayerSet)
                    .run_if(resource_exists::<crate::riftgates::TravelState>)
                    .run_if(not(resource_exists::<FogOfWar>)),
                reveal
                    .after(init_fog)
                    .after(gd_game_core::player::PlayerSet)
                    .run_if(resource_exists::<FogOfWar>)
                    .run_if(resource_exists::<gd_game_core::save::MapLayout>),
            ),
        );
    }
}
