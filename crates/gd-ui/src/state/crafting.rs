//! The crafter's recipe list ([`CraftList`]) and its category groups ([`GROUPS`]): built from the
//! learned blueprints (`gd_sim::items::crafting`), drawn by `gd_ui::trade`, filtered by the search box.
use bevy::prelude::*;
use gd_sim::items::crafting::Blueprint;

pub const WINDOW: &str = "records/ui/inventor/inventor_mastertable.dbr";

pub struct Recipe {
    pub bp: Blueprint,
    /// Name tag (the blueprint's `description`).
    pub name_tag: String,
    /// The made item's name tag (`itemNameTag` / `description`), shown in the
    /// list.
    pub item_tag: String,
    /// Whether it makes a relic (`ItemArtifact`; cyan in the list).
    pub artifact: bool,
    pub tab: usize,
    /// [`GROUPS`] index.
    pub group: usize,
    /// The product item's search lines ([`CraftList::fill_search_lines`]).
    pub search_lines: Vec<String>,
}

/// The list's groups: (tab, header tag), in the window's order (exe `CraftList::BuildGroups`): each group names the
/// item-type ranks it takes (`Item::GetSortRank`) and, for weapon groups, the weapon types (`Item::GetWeaponType`);
/// relic groups by tier; see [`group_of`].
pub const GROUPS: [(usize, &str); 26] = [
    (0, "tagCraftingRelicsTier01"),
    (0, "tagCraftingRelicsTier02"),
    (0, "tagCraftingRelicsTier03"),
    (0, "tagCraftingRunes"),
    (1, "tagCraftingAxes"),
    (1, "tagCraftingBlunt"),
    (1, "tagCraftingSwords"),
    (1, "tagCraftingImplements"),
    (1, "tagCraftingTwoHandedMelee"),
    (1, "tagCraftingSpears"),
    (2, "tagCraftingOneHanded"),
    (2, "tagCraftingTwoHanded"),
    (2, "tagCraftingStaves"),
    (3, "tagCraftingBelts"),
    (3, "tagCraftingHelms"),
    (3, "tagCraftingTorso"),
    (3, "tagCraftingShoulders"),
    (3, "tagCraftingLegs"),
    (3, "tagCraftingHands"),
    (3, "tagCraftingFeet"),
    (3, "tagCraftingOffhands"),
    (3, "tagCraftingShields"),
    (4, "tagCraftingComponents"),
    (4, "tagCraftingAccessories"),
    (4, "tagCraftingConsumables"),
    (4, "tagCraftingMaterials"),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Row {
    Header(usize),
    Recipe(usize),
}

/// The open crafter's recipes.
#[derive(Resource, Default)]
pub struct CraftList {
    pub recipes: Vec<Recipe>,
    pub enhancement: Option<String>,
    /// The crafter's name and profession tags.
    pub name_tag: String,
    pub title_tag: String,
    /// `crafterBitmap` (shown in the craft button's circle).
    pub bitmap: Option<String>,
    /// Whether [`Recipe::search_lines`] are filled ([`CraftList::fill_search_lines`]).
    pub search_lines_ready: bool,
}

impl CraftList {
    /// `localize`: tag → text (the list is sorted by the shown names).
    pub fn build(
        data: &gd_game_core::data::GameData,
        crafter: &gd_formats::arz::Record,
        learned: &[&str],
        localize: &dyn Fn(&str) -> String,
    ) -> Self {
        let window = data
            .db
            .get(WINDOW)
            .and_then(|w| w.str("enchanterArtifactTab").and_then(|t| data.db.get(t)));
        let defaults = window
            .as_ref()
            .map(|w| w.strs("craftingDefaultRecipes"))
            .unwrap_or_default();
        // CraftList::build (exe `CraftList::Build`): own recipes, awakened recipes (gameascendant.dbr
        // `awakenedRecipes`) when the crafter has `awakenedRecipes`, learned blueprints, window defaults.
        let awakened_rec = data.db.get(gd_sim::ascendant::GAME_ASCENDANT);
        let awakened = awakened_rec
            .as_ref()
            .map(|a| a.strs("awakenedRecipes"))
            .unwrap_or_default();
        let mut recipes = Vec::new();
        for r in
            gd_sim::items::crafting::recipes_with_awakened(crafter, &awakened, &defaults, learned)
        {
            // exe `CraftList::LoadCrafter`: an invalid formula is not listed.
            let Some(rec) = data.db.get(&r) else { continue };
            if !gd_sim::items::crafting::formula_valid(&*data.db, rec) {
                continue;
            }
            let Some(bp) = Blueprint::load(&r, rec) else {
                continue;
            };
            let group = group_of(data, &bp.display);
            let shown = data.db.get(&bp.display);
            let item_tag = shown
                .as_ref()
                .and_then(|d| {
                    d.str("itemNameTag")
                        .or_else(|| d.str("description"))
                        .map(str::to_string)
                })
                .unwrap_or_default();
            let artifact = shown.as_ref().is_some_and(|d| d.class == "ItemArtifact");
            recipes.push(Recipe {
                name_tag: rec.str("description").unwrap_or("").to_string(),
                item_tag,
                artifact,
                bp,
                tab: GROUPS[group].0,
                group,
                search_lines: Vec::new(),
            });
        }
        let tag = |k: &str| crafter.str(k).unwrap_or("").to_string();
        // Within a group: by shown name.
        recipes.sort_by_cached_key(|r| {
            (
                r.group,
                localize(if r.item_tag.is_empty() {
                    &r.name_tag
                } else {
                    &r.item_tag
                }),
            )
        });
        Self {
            recipes,
            enhancement: crafter
                .str("enhancementTable")
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            name_tag: tag("description"),
            title_tag: tag("professionTag"),
            bitmap: crafter.str("crafterBitmap").map(str::to_string),
            search_lines_ready: false,
        }
    }

    /// The list rows of `tab`: each non-empty group's header, then its recipes.
    /// With a search query, only the recipes whose search lines match
    /// ([`gd_sim::items::market::search_matches`]) show, and a group left empty
    /// hides its header (`CraftList::FilterRows`).
    pub fn rows(&self, tab: usize, query: &str) -> Vec<Row> {
        let mut out = Vec::new();
        for (g, (t, _)) in GROUPS.iter().enumerate() {
            if *t != tab {
                continue;
            }
            let header = out.len();
            out.extend(
                (0..self.recipes.len())
                    .filter(|i| self.shows(*i, g, query))
                    .map(Row::Recipe),
            );
            if out.len() > header {
                out.insert(header, Row::Header(g));
            }
        }
        out
    }

    /// `rows(tab, query).len()` without building the rows.
    pub fn row_count(&self, tab: usize, query: &str) -> usize {
        GROUPS
            .iter()
            .enumerate()
            .filter(|(_, (t, _))| *t == tab)
            .map(|(g, _)| {
                let n = (0..self.recipes.len())
                    .filter(|i| self.shows(*i, g, query))
                    .count();
                if n == 0 { 0 } else { n + 1 }
            })
            .sum()
    }

    fn shows(&self, i: usize, group: usize, query: &str) -> bool {
        let r = &self.recipes[i];
        r.group == group
            && (query.is_empty() || gd_sim::items::market::search_matches(&r.search_lines, query))
    }

    /// Fills every recipe's search lines from the product item's search text
    /// (`gd_sim::items::tooltip_item::item_search_text`).
    pub fn fill_search_lines(&mut self, ctx: &gd_sim::items::tooltip_item::TipCtx) {
        for r in &mut self.recipes {
            let item = gd_formats::save::item::Item {
                base_name: r.bp.display.clone(),
                stack_count: r.bp.quantity,
                ..Default::default()
            };
            r.search_lines = gd_sim::items::tooltip_item::item_search_text(ctx, &item)
                .iter()
                .map(|l| gd_sim::items::market::search_line(&l.text))
                .collect();
        }
        self.search_lines_ready = true;
    }
}

/// The [`GROUPS`] entry of what a blueprint makes (exe `CraftList::BuildGroups`):
/// relics (rank 11) by tier (`artifactClassification` Lesser /
/// Greater / Divine), runes = augments (21); weapons (rank 9) by weapon
/// type: axe, mace, sword, dagger + scepter, the four two-handed melee
/// classes (spear included), `WeaponHunting_Spear`, one- and two-handed
/// ranged, staff; armour by rank: waist, head, chest (+ clothing,
/// vestments), shoulders, legs, hands (+ bracelets, rank 7), feet;
/// off-hands and shields (rank 8, weapon type 12 / 11); components (16:
/// relics, charms), accessories (amulet 2, ring 6, medal 15), consumables
/// (`OneShot*` 17, attribute 26 / devotion 24 resets), materials
/// (`QuestItem`, 19). A class in no group (none among the blueprints'
/// items) is listed under Materials here.
fn group_of(data: &gd_game_core::data::GameData, display: &str) -> usize {
    let rec = data.db.get(display);
    let class = rec.map_or("", |r| r.class.as_str());
    let group = |tag: &str| {
        GROUPS
            .iter()
            .position(|(_, t)| *t == tag)
            .unwrap_or(GROUPS.len() - 1)
    };
    match class {
        "WeaponMelee_Axe" => return group("tagCraftingAxes"),
        "WeaponMelee_Mace" => return group("tagCraftingBlunt"),
        "WeaponMelee_Sword" => return group("tagCraftingSwords"),
        "WeaponMelee_Dagger" | "WeaponMelee_Scepter" => return group("tagCraftingImplements"),
        "WeaponMelee_Axe2h"
        | "WeaponMelee_Mace2h"
        | "WeaponMelee_Sword2h"
        | "WeaponMelee_Spear2h" => return group("tagCraftingTwoHandedMelee"),
        "WeaponHunting_Spear" => return group("tagCraftingSpears"),
        "WeaponHunting_Ranged1h" => return group("tagCraftingOneHanded"),
        "WeaponHunting_Ranged2h" => return group("tagCraftingTwoHanded"),
        "WeaponMagical_Staff" => return group("tagCraftingStaves"),
        "WeaponArmor_Offhand" => return group("tagCraftingOffhands"),
        "WeaponArmor_Shield" => return group("tagCraftingShields"),
        _ => {}
    }
    match gd_formats::inventory::sort_rank(class) {
        11 => match rec.and_then(|r| r.str("artifactClassification")) {
            Some("Greater") => group("tagCraftingRelicsTier02"),
            Some("Divine") => group("tagCraftingRelicsTier03"),
            _ => group("tagCraftingRelicsTier01"),
        },
        21 => group("tagCraftingRunes"),
        13 => group("tagCraftingBelts"),
        1 => group("tagCraftingHelms"),
        3 => group("tagCraftingTorso"),
        14 => group("tagCraftingShoulders"),
        4 => group("tagCraftingLegs"),
        7 => group("tagCraftingHands"),
        5 => group("tagCraftingFeet"),
        16 => group("tagCraftingComponents"),
        2 | 6 | 15 => group("tagCraftingAccessories"),
        17 | 26 | 24 => group("tagCraftingConsumables"),
        _ => group("tagCraftingMaterials"),
    }
}
