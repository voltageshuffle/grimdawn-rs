//! Unique entity groups: the map's `Unique Entities` / `Inverse Unique Entities` instance groups place one
//! member (or all but one) per group, and the Chthonic rifts' `Any Entity` groups activate one rift.
//!
//! The choice, `GameEngine::IsUniqueEntitySelected(entity, excludeArrivals)`: of the groups holding the
//! entity's uid (map order) the first of type `Unique Entities` or `Inverse Unique Entities`, else the last
//! one (no group: not selected). One chosen uid per group *name* (`GameEngine.uniqueSelections`); an unknown
//! name draws a random member (`SelectSingleEntityFromGroup`), leaving out with the flag the rift arrival
//! points ([`UniqueSelection::arrivals`]), and remembers it (nothing when no member is left). The draw
//! happens when the first member of the group loads.
//!
//! Lifetime: cleared only by `GameEngine::ExitPlayingMode`: the choice holds for the whole play session,
//! across level reloads; not saved, not seeded (reset by `session::end_session_state`).
//!
//! The loaders ([`Loader`]):
//! - `Proxy::InitializePools` (`Proxy`, `ProxyAmbush`): the first group of type `Unified Entities` or `Unique
//!   Entities` ends the scan; in a `Unique Entities` group and not selected the proxy builds no pools (no
//!   monsters, no accessory chest). Inverse groups are not read.
//! - `ScriptEntity::Load`: in any `Unique Entities` group and not selected, `Actor::LoadLite` skips
//!   `Entity::Load`, so the entity has no `entityHooks`. Inverse groups are not read.
//! - `SetPiece::Load`: (Unique and not selected) or (Inverse and selected): state 1, spent, no
//!   `placementChance` roll and no `Generate`. So "Inverse" places every member but the chosen one.
//! - `DynamicTeleporter::Load`: a grouped rift asks with the flag set; unselected, state 1
//!   (`crate::riftgates::DynamicRift`).
//!
//! Other classes in these groups are not filtered by the engine. PORT: the draw uses
//! [`gd_game_core::rng`].
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use gd_formats::collections::{HashMap, HashSet};
use gd_formats::map::Uid;
use std::sync::Arc;

pub const UNIQUE: &str = "Unique Entities";
pub const INVERSE: &str = "Inverse Unique Entities";

/// The open map's instance groups (set by `gd_world` when it opens a map).
#[derive(Resource, Clone, Default)]
pub struct InstanceGroups(pub Arc<gd_formats::map::sector::InstanceGroups>);

/// The uid chosen per group name and the rift arrival points.
#[derive(Resource, Default)]
pub struct UniqueSelection {
    pub chosen: HashMap<String, Uid>,
    pub arrivals: HashSet<Uid>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loader {
    Proxy,
    ScriptEntity,
    SetPiece,
}

impl UniqueSelection {
    /// `IsUniqueEntitySelected(uid, exclude_arrivals)`.
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
        let pick = free[gd_game_core::rng::usize(0..free.len())];
        debug!(
            "group {:?} ({}): {} members, chose {pick:?}",
            g.name,
            g.kind,
            g.entries.len()
        );
        self.chosen.insert(g.name.clone(), pick);
        pick == *uid
    }

    /// Whether `loader` loads the entity `uid` in full; ungrouped entities do.
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
                    .find(|g| g.kind == gd_sim::patrol::UNIFIED_ENTITIES || g.kind == UNIQUE);
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

#[derive(SystemParam)]
pub struct UniqueEntities<'w> {
    pub groups: Res<'w, InstanceGroups>,
    pub selection: ResMut<'w, UniqueSelection>,
}

impl UniqueEntities<'_> {
    /// An entity without a uid loads.
    pub fn loads(&mut self, uid: Option<Uid>, loader: Loader) -> bool {
        let Some(uid) = uid else { return true };
        self.selection.loads(&self.groups.0, &uid, loader)
    }

    pub fn is_selected(&mut self, uid: &Uid, exclude_arrivals: bool) -> bool {
        self.selection
            .is_selected(&self.groups.0, uid, exclude_arrivals)
    }
}

pub struct UniquePlugin;

impl Plugin for UniquePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InstanceGroups>()
            .init_resource::<UniqueSelection>();
    }
}
