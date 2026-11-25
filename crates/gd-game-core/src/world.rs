//! World objects that matter to gameplay: training dummies, breakables,
//! chests' identities, described decorations.
//!
//! A described decoration (direction signs, aether crystals, a cave mouth) is
//! a `Decoration` with a `description`: `Actor::Load` sets
//! `IsDescriptionVisible` when `description` is not empty, and
//! `InGameUIActorCapture::CollectCandidates` makes only those hoverable
//! (outlined, named by the tag's text). Such a decoration is not part of the
//! level (`Decoration::GetIsPartOfLevel`): no physics body.
//! PORT: `Decoration`'s subclasses take the flag too; no stock record of one
//! has a description.
use bevy::prelude::*;
use gd_formats::collections::{HashMap, HashSet};

/// A training dummy: never moves, attacks or dies (the Target Dummy and the test arena's dummies). No AI runs on
/// it and its life never drops below 1. PORT: the real dummy relies on its 15 %-life shield alone, so a one-frame
/// burst from above 15 % could kill it; the floor rules that out.
#[derive(Component)]
pub struct Dummy;

/// `records/creatures/enemies/special/trainingdummy.dbr`'s `description`.
pub const DUMMY_TAG: &str = "tagMiscTargetDummy";

#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct Described {
    pub tag: String,
}

impl Described {
    /// `Actor::IsDescriptionVisible` of a `Decoration` record.
    pub fn from_record(rec: &gd_formats::arz::Record) -> Option<Self> {
        if rec.class != "Decoration" {
            return None;
        }
        let tag = rec.str("description").filter(|t| !t.is_empty())?;
        Some(Self {
            tag: tag.to_owned(),
        })
    }
}

/// (level, key) of objects broken this session.
#[derive(Resource, Default)]
pub struct Broken(pub HashSet<(usize, u32)>);

#[derive(Component)]
pub struct Breakable {
    pub record: String,
    pub key: (usize, u32),
    /// `Destructible::IsTargetable`: whether the cursor picks it as an enemy. Others only break by area damage.
    pub targetable: bool,
}

/// Which chest this is, for remembering it across level streaming.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChestKey {
    /// Placed in a level: (level, entity index).
    Level(usize, u32),
    /// From a proxy's accessory pool: (level, proxy entity index, n).
    Accessory(usize, u32, usize),
    /// A story object.
    Story(u32),
}

/// A door (its nav obstacle stops projectiles while closed).
#[derive(Component, Default)]
pub struct DoorMarker;

/// Session memory: opened chests and what each proxy's accessory pool made
/// (record, position, yaw).
#[derive(Resource, Default)]
pub struct ChestStates {
    pub opened: HashSet<ChestKey>,
    pub accessories: HashMap<(usize, u32), Vec<(String, Vec3, f32)>>,
}

/// A level's `AreaOfInterest`: an icon of the local map.
#[derive(Component)]
pub struct MapPoint {
    pub record: String,
}

/// `SoundPeriodic::Load` / `Update`: a `Decoration`'s periodic sound. Counts down from one period (or a random
/// draw); at 0 plays and re-arms, skipping the play while the previous one still sounds (PORT: its last voice
/// entity being alive stands for `pak.IsPlaying()`).
#[derive(Component, Clone, Debug)]
pub struct PeriodicSound {
    pub pak: String,
    period: f32,
    random: Option<(f32, f32)>,
    /// Seconds until the next play.
    pub left: f32,
    pub last: Option<Entity>,
}

impl PeriodicSound {
    pub fn from_record(rec: &gd_formats::arz::Record) -> Option<Self> {
        let pak = rec
            .str("periodicSound")
            .filter(|p| !p.is_empty())?
            .to_owned();
        let secs = |k: &str| (rec.f32(k).unwrap_or(0.0) * 1000.0) as i32 as f32 / 1000.0;
        let random = rec
            .bool("randomPeriod")
            .then(|| (secs("randomPeriodMin"), secs("randomPeriodMax")));
        let mut s = Self {
            pak,
            period: secs("soundPeriod"),
            random,
            left: 0.0,
            last: None,
        };
        s.arm();
        Some(s)
    }

    pub fn arm(&mut self) {
        self.left = match self.random {
            Some((lo, hi)) => {
                let (lo, hi) = (lo.min(hi), lo.max(hi));
                // `IGenerate(lo, hi)`: whole milliseconds, both ends inclusive.
                let (a, b) = ((lo * 1000.0) as u32, (hi * 1000.0) as u32);
                (a + fastrand::u32(..=(b - a))) as f32 / 1000.0
            }
            None => self.period,
        };
    }
}
