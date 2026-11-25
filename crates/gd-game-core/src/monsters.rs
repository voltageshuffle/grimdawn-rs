//! The [`Monster`] component (record, level, proxy slot) and the [`FactionPack`] every creature's record
//! gives it. Spawning, AI and session state are gd-game's `monsters`.
use bevy::prelude::*;

#[derive(Component, Clone, Debug)]
pub struct Monster {
    pub record: String,
    pub level: i32,
    /// (level, proxy entity, index in its spawn list).
    pub slot: (usize, u32, usize),
}

/// A monster's faction pack (`FactionPack::Load`): its own faction (`myFaction`) and its value for each
/// of the 47 factions (keyed like [`gd_rules::faction::KEYS`]).
#[derive(Debug, Clone, PartialEq)]
pub struct FactionPack {
    pub mine: usize,
    pub values: Vec<f32>,
}

impl FactionPack {
    pub fn of(db: &gd_formats::vfs::Database, record: &str) -> Option<Self> {
        let pack = db
            .get(record)?
            .str("factions")
            .filter(|p| !p.is_empty())
            .and_then(|p| db.get(p))?;
        let mine = pack
            .str("myFaction")
            .and_then(crate::factions::index_of_name)?;
        Some(Self {
            mine,
            values: gd_rules::faction::KEYS
                .iter()
                .map(|k| pack.f32(k).unwrap_or(0.0))
                .collect(),
        })
    }

    /// [`Self::of`] sharing one `Arc` per `factions` record ([`Self::same`] uses `Arc::ptr_eq`).
    pub fn shared(data: &crate::data::GameData, record: &str) -> Option<std::sync::Arc<Self>> {
        let key = data
            .db
            .get(record)?
            .str("factions")
            .filter(|p| !p.is_empty())?
            .to_ascii_lowercase();
        data.packs()
            .lock()
            .entry(key)
            .or_insert_with(|| Self::of(&data.db, record).map(std::sync::Arc::new))
            .clone()
    }

    /// `FactionManager::IsFriend`: friends when the other's faction is its own; for a faction index
    /// above 46 (invisible) always; else its value for the other's faction decides when non-zero
    /// (friend from 0 up), else the other's value for its faction (friend unless below 0).
    pub fn is_friend(&self, other: &FactionPack) -> bool {
        let t = other.mine;
        if t == self.mine {
            return true;
        }
        if t <= 0x2E {
            let v = self.values.get(t).copied().unwrap_or(0.0);
            if v != 0.0 {
                return v >= 0.0;
            }
            if other.values.get(self.mine).copied().unwrap_or(0.0) < 0.0 {
                return false;
            }
        }
        true
    }
}

impl FactionPack {
    /// `FactionManager::IsFoe`: the asker's value for the other's faction decides when non-zero
    /// (foe below 0), else the other's value for the asker's faction.
    pub fn is_foe(&self, other: &FactionPack) -> bool {
        !self.is_friend(other)
    }

    /// `IsFoe` between two creatures' packs: one without a pack is nobody's foe (PORT: the engine's
    /// pack of a record without `factions` is not traced).
    pub fn foes(me: &Option<std::sync::Arc<Self>>, other: &Option<std::sync::Arc<Self>>) -> bool {
        match (me, other) {
            (Some(a), Some(b)) => a.is_foe(b),
            _ => false,
        }
    }

    /// Same pack contents.
    pub fn same(a: &Option<std::sync::Arc<Self>>, b: &Option<std::sync::Arc<Self>>) -> bool {
        match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b) || **a == **b,
            _ => false,
        }
    }
}
