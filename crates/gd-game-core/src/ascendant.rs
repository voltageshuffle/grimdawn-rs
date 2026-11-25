//! Ascendant altar data: offering boxes, session altar state, formula / reagent reads.
use bevy::prelude::*;

/// Reagent → (formula record, exclusive).
pub fn formula_of(db: &gd_formats::vfs::Database, reagent: &str) -> Option<(String, bool)> {
    let g = db.get(gd_sim::ascendant::GAME_ASCENDANT)?;
    for (field, excl) in [
        ("altarExclusiveRecipes", true),
        ("altarInclusiveRecipes", false),
    ] {
        for f in g.strs(field) {
            if db
                .get(f)
                .and_then(|r| {
                    r.str("reagentBaseBaseName")
                        .map(|n| n.eq_ignore_ascii_case(reagent))
                })
                .unwrap_or(false)
            {
                return Some((f.to_string(), excl));
            }
        }
    }
    None
}

/// Exclusive (centre) formulas' reagents first, then inclusive ones.
pub fn formula_reagents(db: &gd_formats::vfs::Database) -> Vec<String> {
    let Some(g) = db.get(gd_sim::ascendant::GAME_ASCENDANT) else {
        return Vec::new();
    };
    g.strs("altarExclusiveRecipes")
        .into_iter()
        .chain(g.strs("altarInclusiveRecipes"))
        .filter_map(|f| {
            db.get(f)
                .and_then(|r| r.str("reagentBaseBaseName").map(str::to_owned))
        })
        .collect()
}

/// The centre takes an exclusive formula's reagent, the five boxes inclusive ones.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Offering {
    pub center: Option<String>,
    pub boxes: [Option<String>; 5],
}

impl Offering {
    /// Exclusive reagents go in the free centre, inclusive in the first free box.
    /// False when not a formula reagent or its box is taken.
    pub fn place(&mut self, db: &gd_formats::vfs::Database, reagent: &str) -> bool {
        let Some((_, excl)) = formula_of(db, reagent) else {
            return false;
        };
        if excl {
            if self.center.is_none() {
                self.center = Some(reagent.to_owned());
                return true;
            }
        } else if let Some(b) = self.boxes.iter_mut().find(|b| b.is_none()) {
            *b = Some(reagent.to_owned());
            return true;
        }
        info!("ascendant: no free box for {reagent}");
        false
    }

    /// Centre first.
    pub fn placed(&self) -> Vec<String> {
        self.center
            .iter()
            .chain(self.boxes.iter().flatten())
            .cloned()
            .collect()
    }
}

/// Each formula with its level (boxes holding its reagent), in first-placed order.
pub fn formula_levels(db: &gd_formats::vfs::Database, placed: &[String]) -> Vec<(String, u32)> {
    let mut levels: Vec<(String, u32)> = Vec::new();
    for r in placed {
        let Some((f, _)) = formula_of(db, r) else {
            continue;
        };
        match levels.iter_mut().find(|l| l.0 == f) {
            Some(l) => l.1 += 1,
            None => levels.push((f, 1)),
        }
    }
    levels
}

/// Reagents per box (`reagentBaseQuantity`, min 1; `ItemArtifactFormula::GetReagentBaseQuantityForFormula`).
pub fn reagent_quantity(db: &gd_formats::vfs::Database, reagent: &str) -> u32 {
    formula_of(db, reagent)
        .and_then(|(f, _)| db.get(&f))
        .and_then(|f| f.i32("reagentBaseQuantity"))
        .unwrap_or(1)
        .max(1) as u32
}

#[derive(Resource, Default)]
pub struct AltarState {
    pub activated: bool,
    /// The boxes as activated (`GameEngine::GetActivatedAltarReagents`).
    pub reagents: Offering,
    pub formulas: gd_sim::ascendant::Ascendant,
}
