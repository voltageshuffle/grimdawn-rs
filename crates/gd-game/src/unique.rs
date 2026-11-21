//! Unique entity groups: the map's `Unique Entities` / `Inverse Unique Entities` instance groups place one
//! member (or all but one) per group, and the Chthonic rifts' `Any Entity` groups activate one rift.
//!
//! The choice, Game.dll `GameEngine::IsUniqueEntitySelected(entity, excludeArrivals)`
//! @0x1802e70c0: of the groups holding the entity's uid (map order,
//! `gd_formats::map::sector::InstanceGroups::containing`) the first of type `Unique Entities` or `Inverse
//! Unique Entities`, else the last one (no group: not selected). The engine keeps one chosen uid per group
//! *name* (GameEngine +6648): a known name answers "is it this uid"; else `SelectSingleEntityFromGroup`
//! @0x1802e7400 draws a random member (`RandomUniformLocked::IGenerate(0, n - 1)` on the engine's shared
//! RNG, `GetRandomGen`), leaving out with the flag the rift arrival points (+6720,
//! [`UniqueSelection::arrivals`]), and remembers it (nothing is remembered when no member is left). The
//! draw happens when the first member of the group loads.
//!
//! Lifetime: both sets are cleared only by `GameEngine::ExitPlayingMode` (and the constructor): the choice
//! holds for the whole play session, across level unloads and reloads; it is not saved, not per character
//! and not seeded, so the next session draws again (session state, reset by `session::end_session_state`).
//!
//! The loaders (all skipped in the editor) — [`Loader`]:
//! - `Proxy::InitializePools` @0x180414580 (`Proxy`, `ProxyAmbush`): the first group of type `Unified
//!   Entities` or `Unique Entities` ends the scan; in a `Unique Entities` group and not selected the proxy
//!   builds no pools: no monsters, no accessory chest (the treasure troves `chestproxy_treasuretrove_*`, sand
//!   sacrifices, trap chests). `Inverse Unique Entities` is not read for proxies.
//! - `ScriptEntity::Load` @0x1804752d0: in any `Unique Entities` group and not selected,
//!   `Actor::LoadLite(table, false)` (Engine @0x180029a30) skips `Entity::Load` @0x18007d6e0, so the entity
//!   has no `entityHooks`: its script (bounty targets, secret and bonus chests) never runs. Inverse: not read.
//! - `SetPiece::Load` (@0x18047b820): flags from all its groups (`Unified` 2, `Unique` 1, `Inverse` 4);
//!   (Unique and not selected) or (Inverse and selected): state 1, spent, no `placementChance` roll and no
//!   `Generate`: the barriers, Nemesis spawns (`setpiece_nemesis_*`), monster totems and one-shot chests
//!   (`d*_setpiece_oneshotchest_*`). So "Inverse" places every member but the chosen one.
//! - `DynamicTeleporter::Load` @0x180282770: a grouped rift asks with the flag set; unselected, state 1
//!   (`crate::riftgates::DynamicRift`).
//!
//! Other classes in these groups (a few `Destructible`, `Decoration`, `TerrainDecoration`, `AreaOfInterest`
//! in stock data) are not filtered by the engine. PORT: the draw uses the port's RNG ([`crate::rng`]).
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use gd_formats::map::Uid;
use gd_formats::map::sector::InstanceGroup;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub const UNIQUE: &str = "Unique Entities";
pub const INVERSE: &str = "Inverse Unique Entities";
pub const UNIFIED: &str = "Unified Entities";

/// The open map's instance groups (the map's own list, shared: set by the renderer when it opens a map,
/// with `save::MapLayout`).
#[derive(Resource, Clone, Default)]
pub struct InstanceGroups(pub Arc<gd_formats::map::sector::InstanceGroups>);

impl InstanceGroups {
    pub fn from_groups(groups: Vec<InstanceGroup>) -> Self {
        Self(Arc::new(gd_formats::map::sector::InstanceGroups::new(
            groups,
        )))
    }
}

/// Session state of the choice (module doc): the uid chosen per group name and the rift arrival points.
#[derive(Resource, Default)]
pub struct UniqueSelection {
    pub chosen: HashMap<String, Uid>,
    pub arrivals: HashSet<Uid>,
}

/// Which engine loader asks (module doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loader {
    Proxy,
    ScriptEntity,
    SetPiece,
}

impl UniqueSelection {
    /// `IsUniqueEntitySelected(uid, exclude_arrivals)` (module doc).
    pub fn is_selected(
        &mut self,
        groups: &gd_formats::map::sector::InstanceGroups,
        uid: &Uid,
        exclude_arrivals: bool,
    ) -> bool {
        let mut group = None;
        for g in groups.containing(uid) {
            group = Some(g);
            if g.kind == UNIQUE || g.kind == INVERSE {
                break;
            }
        }
        let Some(g) = group else { return false };
        if let Some(c) = self.chosen.get(&g.name) {
            return c == uid;
        }
        let free: Vec<Uid> = g
            .entries
            .iter()
            .map(|e| e.uid)
            .filter(|m| !(exclude_arrivals && self.arrivals.contains(m)))
            .collect();
        if free.is_empty() {
            return false;
        }
        let pick = free[crate::rng::usize(0..free.len())];
        debug!(
            "group {:?} ({}): {} members, chose {pick:?}",
            g.name,
            g.kind,
            g.entries.len()
        );
        self.chosen.insert(g.name.clone(), pick);
        pick == *uid
    }

    /// Whether `loader` loads the entity `uid` in full (module doc); ungrouped entities do.
    pub fn loads(
        &mut self,
        groups: &gd_formats::map::sector::InstanceGroups,
        uid: &Uid,
        loader: Loader,
    ) -> bool {
        match loader {
            Loader::Proxy => {
                let first = groups
                    .containing(uid)
                    .find(|g| g.kind == UNIFIED || g.kind == UNIQUE);
                !first.is_some_and(|g| g.kind == UNIQUE) || self.is_selected(groups, uid, false)
            }
            Loader::ScriptEntity => {
                !groups.containing(uid).any(|g| g.kind == UNIQUE)
                    || self.is_selected(groups, uid, false)
            }
            Loader::SetPiece => {
                let unique = groups.containing(uid).any(|g| g.kind == UNIQUE);
                let inverse = groups.containing(uid).any(|g| g.kind == INVERSE);
                if !unique && !inverse {
                    return true;
                }
                let selected = self.is_selected(groups, uid, false);
                !((unique && !selected) || (inverse && selected))
            }
        }
    }
}

/// The groups and the choice together, for the loaders.
#[derive(SystemParam)]
pub struct UniqueEntities<'w> {
    pub groups: Res<'w, InstanceGroups>,
    pub selection: ResMut<'w, UniqueSelection>,
}

impl UniqueEntities<'_> {
    /// [`UniqueSelection::loads`]; an entity without a uid loads.
    pub fn loads(&mut self, uid: Option<Uid>, loader: Loader) -> bool {
        let Some(uid) = uid else { return true };
        let groups = self.groups.0.clone();
        self.selection.loads(&groups, &uid, loader)
    }

    /// [`UniqueSelection::is_selected`].
    pub fn is_selected(&mut self, uid: &Uid, exclude_arrivals: bool) -> bool {
        let groups = self.groups.0.clone();
        self.selection.is_selected(&groups, uid, exclude_arrivals)
    }
}

pub struct UniquePlugin;

impl Plugin for UniquePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InstanceGroups>()
            .init_resource::<UniqueSelection>();
    }
}
