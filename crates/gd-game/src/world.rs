//! World objects that matter to gameplay: training dummies, breakables,
//! chests' identities, described decorations.
//!
//! A described decoration (the direction signs, `records/storyelements/signs/
//! sign_signpost_*.dbr`, the aether crystals, a cave mouth) is a `Decoration`
//! with a `description`: Engine.dll `Actor::Load` @0x180028c50 sets
//! `IsDescriptionVisible` (Actor+1120) to "`description` is not empty", and
//! Grim Dawn.exe sub_14002FF80 adds a `Decoration` to the hover candidates only
//! when that flag is set, so the cursor outlines it (object colour) and names it
//! (`Actor::GetGameDescription`: `SimpleStringFormat` = `{%s0}` of the
//! localized tag: the tag's text). The bare posts carry no description and stay
//! scenery. Such a decoration is not part of the level either
//! (`Decoration::GetIsPartOfLevel`): no physics body (`grimdawn::world`).
//! PORT: `Decoration`'s subclasses take the flag too; no stock record of one
//! has a description.
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

/// A training dummy: never moves or attacks, never dies. The Target Dummy (`trainingdummy.dbr`, the record whose
/// `description` is [`DUMMY_TAG`]; [`crate::creature::spawn_creature`]) and the test arena's dummies carry it.
/// No AI runs on it (`combat::monster_ai`, `cc_states`), and its life never drops below 1 (`combat::apply_hits`,
/// `afflictions::tick`): it takes every hit, DoT and number, then refills by its own record (the bio's high
/// regeneration and `targetdummy_lowhealthshield_01`, a 100 % heal with full absorption for 6 s at 15 % life).
/// PORT: the real dummy relies on that shield alone, so a burst from above 15 % to 0 within one frame (many
/// projectiles at once) killed it here; the floor rules that out. Its `causesAnger` 0 already keeps it out of
/// the monsters' and pets' sight (`combat::sight`, `pets::ai`) and the overhead bars (grimdawn `ui::overhead`).
#[derive(Component)]
pub struct Dummy;

/// The Target Dummy's `description` tag (`records/creatures/enemies/special/trainingdummy.dbr`).
pub const DUMMY_TAG: &str = "tagMiscTargetDummy";

/// A described decoration (module docs): what the hover names.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct Described {
    /// The record's `description` tag.
    pub tag: String,
}

impl Described {
    /// `Actor::IsDescriptionVisible` of a `Decoration` record (module docs).
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
    /// The record's `targetable`: `Destructible::IsTargetable`
    /// (`targetable` and not broken; a broken one is gone here) decides
    /// whether the cursor picks it as an enemy (hover, click; Grim Dawn.exe
    /// sub_1400318C0). Others only break by area damage.
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

/// A door (its nav obstacle stops projectiles while closed); the game's
/// doors carry it.
#[derive(Component, Default)]
pub struct DoorMarker;

/// Session memory: opened chests and what each proxy's accessory pool made
/// (record, position, yaw).
#[derive(Resource, Default)]
pub struct ChestStates {
    pub opened: HashSet<ChestKey>,
    pub accessories: HashMap<(usize, u32), Vec<(String, Vec3, f32)>>,
}
