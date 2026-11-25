//! Fog of war (map exploration), per difficulty in `levels_world001.map/<Difficulty>/map.fow` (format and reveal
//! rule: `gd_formats::save::fow`). `World::AddFOWVisibility` reveals only once the player has moved at least 1 m
//! (x or z) since the last call, for every region touching a 30 m (`kVisibleRange`) box around the player, in the
//! region's local coordinates. A region's grid is created fully fogged on first use (`FogOfWar::Initialize`, the
//! region's bbox width/depth) and kept for the next save.
//!
//! Each region has two grids revealed together (`FogOfWar::Reveal`): the saved one ([`FogOfWar::grids`]) and a
//! session one ([`FogOfWar::session`], never saved) the local map greys by.
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
    /// Region level name (lowercase) -> this session's grid.
    pub session: HashMap<String, FogGrid>,
    pub last: Option<Vec2>,
    /// Reveals that changed a cell of either grid, counted: the local map redraws the box around [`Self::last`]
    /// when it moves.
    pub revision: u64,
    /// Reveals that changed a cell of the saved grids, counted: the minimap redraws by it.
    pub saved_revision: u64,
    /// The open map's region names lowercased, by region index.
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

    /// `World::IsInFog`-style query for a world position (true when the region has no grid yet).
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

/// `World::AddFOWVisibility` around the living player.
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
    // Loaded under the save guard: an unreadable file is set aside / blocks, never rewritten from the empty runtime.
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
