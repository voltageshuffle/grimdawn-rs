//! The ascendant altar's data: the offering boxes, the session's altar state
//! and the formula / reagent record reads. The gameplay (use, activation,
//! Dread spawns) is `gd_game::ascendant`.
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

/// The formula reagents the altar takes: the exclusive (centre) formulas'
/// then the inclusive ones (`altarExclusiveRecipes`, `altarInclusiveRecipes`).
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

/// The altar's boxes: the centre (an exclusive formula's reagent) and five
/// boxes (inclusive formulas' reagents).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Offering {
    pub center: Option<String>,
    pub boxes: [Option<String>; 5],
}

impl Offering {
    /// Puts `reagent` in its box: an exclusive formula's in the free centre,
    /// an inclusive one's in the first free box. False when it is no
    /// formula's reagent or its box is taken.
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

    /// The placed reagents, the centre first (`gd_game::ascendant::activate_altar`'s order).
    pub fn placed(&self) -> Vec<String> {
        self.center
            .iter()
            .chain(self.boxes.iter().flatten())
            .cloned()
            .collect()
    }
}

/// The formulas the `placed` reagents install and their levels (the number of boxes holding the formula's reagent),
/// in first-placed order: the altar window's bonus lists and `gd_game::ascendant::activate_altar`.
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

/// The reagents one box of `reagent`'s formula takes (`reagentBaseQuantity`, at least 1;
/// `ItemArtifactFormula::GetReagentBaseQuantityForFormula`).
pub fn reagent_quantity(db: &gd_formats::vfs::Database, reagent: &str) -> u32 {
    formula_of(db, reagent)
        .and_then(|(f, _)| db.get(&f))
        .and_then(|f| f.i32("reagentBaseQuantity"))
        .unwrap_or(1)
        .max(1) as u32
}

/// The altar this session: whether it was activated, the reagents it was
/// activated with and the formulas installed (session state).
#[derive(Resource, Default)]
pub struct AltarState {
    pub activated: bool,
    /// The boxes as activated (`GameEngine::GetActivatedAltarReagents`, an
    /// `AltarOfferConfigInfo`: the centre and the five boxes; the altar
    /// window's show `AltarWindow::Show` fills its boxes from it).
    pub reagents: Offering,
    pub formulas: gd_sim::ascendant::Ascendant,
}
