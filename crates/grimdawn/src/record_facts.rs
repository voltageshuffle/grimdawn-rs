//! Per-record facts the mouse hover (`crate::hover`) and the controller targeting (`crate::controller_target`)
//! share: one cache, resolved on first use, cleared when the mod data changes.

use crate::data::GameData;
use crate::hover::Interact;
use crate::ui::text::FontCache;
use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;

const ENGINE: &str = "records/game/gameengine.dbr";

pub struct RecordFactsPlugin;

impl Plugin for RecordFactsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RecordFacts>().add_systems(
            PreUpdate,
            clear_facts.run_if(on_message::<gd_render::model::ModChanged>),
        );
    }
}

/// The engine's outline colours and the per-record facts.
#[derive(Resource, Default)]
pub struct RecordFacts {
    /// Object, friend, enemy outline colours (`GameEngine::LoadFromDatabase`).
    colours: Option<[Vec3; 3]>,
    records: HashMap<String, RecordFact>,
}

pub struct RecordFact {
    /// `Actor::GetRadius` (`gd_game::combat::get_radius`; the record's `scale` counts).
    pub radius: f32,
    /// Record `Class` starts with `Npc`.
    pub npc: bool,
    /// Record `Class` is `NpcMerchant`.
    pub merchant: bool,
    /// An `Npc` subclass (always `IsOfInterest`).
    pub npc_subclass: bool,
    /// Record `Class` is `FixedItemBlastContainer`.
    pub blast: bool,
    /// `FixedActor::GetUsableRange`'s record term (`gd_game::combat::fixed_usable_range`).
    pub usable: f32,
    pub invincible: bool,
    /// Record `interactIconOverride`.
    pub icon: Option<Interact>,
    /// `monsterClassification` index (Common 0 … SuperBoss 5).
    pub classification: u8,
    /// The hover name (`Npc::GetRolloverDescription` / `Monster::GetGameDescription`).
    pub name: Arc<str>,
    /// `Monster::GetRaceText`.
    pub race: Arc<str>,
}

fn clear_facts(mut facts: ResMut<RecordFacts>) {
    *facts = RecordFacts::default();
}

/// `GameEngine::ResolveMonsterClassificationEnum`.
fn classification(s: &str) -> u8 {
    match s {
        "Champion" => 1,
        "Hero" => 2,
        "Boss" => 3,
        "Quest" => 4,
        "SuperBoss" => 5,
        _ => 0,
    }
}

/// A record's `interactIconOverride`.
pub fn icon_override(v: &str) -> Option<Interact> {
    match v {
        "Dialog" => Some(Interact::Dialog),
        "Attack" => Some(Interact::Attack),
        _ => None,
    }
}

impl RecordFacts {
    /// Object, friend and enemy outline colours.
    pub fn colours(&mut self, data: &GameData) -> [Vec3; 3] {
        *self.colours.get_or_insert_with(|| {
            let r = data.db.get(ENGINE);
            let c = |k: &str, d: [f32; 3]| {
                let f = |ch: &str, d: f32| r.and_then(|r| r.f32(&format!("{k}.{ch}"))).unwrap_or(d);
                Vec3::new(f("R", d[0]), f("G", d[1]), f("B", d[2]))
            };
            [
                c("objectOutlineColor", [0.5, 0.5, 1.0]),
                c("friendOutlineColor", [0.0, 1.0, 0.0]),
                c("enemyOutlineColor", [1.0, 0.0, 0.0]),
            ]
        })
    }

    /// The record's facts, resolved on first use and kept.
    pub fn of(&mut self, data: &GameData, fonts: &FontCache, record: &str) -> &RecordFact {
        if !self.records.contains_key(record) {
            let f = resolve(data, fonts, record);
            self.records.insert(record.to_owned(), f);
        }
        &self.records[record]
    }
}

fn resolve(data: &GameData, fonts: &FontCache, record: &str) -> RecordFact {
    let r = data.db.get(record);
    let s = |k: &str| r.and_then(|r| r.str(k)).unwrap_or("");
    let tag = |t: &str| fonts.tag(t).to_owned();
    let class = r.map_or("", |r| r.class.as_str());
    let npc = class.starts_with("Npc");
    let cls = classification(s("monsterClassification"));
    let engine = data.db.get(ENGINE);
    let name = if npc {
        // `Npc::GetRolloverDescription`.
        let prof = s("professionTag");
        let base = tag(s("description"));
        if prof.is_empty() {
            base
        } else {
            format!("{base} - {}", tag(prof))
        }
    } else {
        // `Monster::GetGameDescription`: the classification's colour code first.
        let key = [
            "commonMonsterColor",
            "championMonsterColor",
            "heroMonsterColor",
            "bossMonsterColor",
            "questMonsterColor",
            "bossMonsterColor",
        ][cls as usize];
        let code = engine.and_then(|e| e.str(key)).unwrap_or_default();
        format!("{code}{}", tag(s("description")))
    };
    // `Monster::GetRaceText` (`GameEngine::GetMonsterRaceTagSingular`:
    // `RaceNNN` → `tagRaceNNN`, 1 ≤ NNN ≤ `numMonsterRaces`).
    let races = engine.and_then(|e| e.i32("numMonsterRaces")).unwrap_or(18);
    let race = r
        .map(|r| r.strs("characterRacialProfile"))
        .unwrap_or_default()
        .iter()
        .filter(|x| !x.is_empty())
        .map(|x| {
            let known = x
                .strip_prefix("Race")
                .and_then(|n| n.parse::<i32>().ok())
                .is_some_and(|n| (1..=races).contains(&n));
            tag(&if known {
                format!("tag{x}")
            } else {
                "noTagUnknownRace".to_owned()
            })
        })
        .collect::<Vec<_>>()
        .join(" - ");
    RecordFact {
        radius: gd_game::combat::get_radius(r, gd_game::combat::ACTOR_RADIUS),
        npc,
        merchant: class == "NpcMerchant",
        npc_subclass: npc && class != "Npc",
        blast: class == "FixedItemBlastContainer",
        usable: gd_game::combat::fixed_usable_range(r),
        invincible: r.is_some_and(|r| r.bool("invincible")),
        icon: icon_override(s("interactIconOverride")),
        classification: cls,
        name: name.into(),
        race: race.into(),
    }
}
