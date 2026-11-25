//! The new-item glow of the bag grids (`newItemHighlight`, `Options::GetBool(67)`).
//!
//! Grim Dawn.exe, the inventory item grid's draw `BagWidget::Render` / rebuild `BagWidget::Rebuild` / mouse
//! handler `BagWidget_HandleMouseEvent`: an item that entered the bags by a pickup
//! (`Item::OnPickup` sets the flag the rebuild tests and clears: [`gd_game_core::inventory::Inventory::fresh`]) is
//! covered by a highlight over its cell with the colour of its drop class (`GameEngine::GetItemHighlightColor`),
//! alpha `colour.a * tri`, `tri` a triangle between `minNewItemAlpha` and the midpoint of min / max
//! (`maxNewItemAlpha`) with the period `newItemPeriod` s (`inventory_grid0.dbr`: 0.25, 0.75, 2 s); it ends as soon
//! as the cursor is over the item.
//! The highlight is drawn with the `"additive"` style (`ui/additive.rs`). `GameEngine::GetItemHighlightColor`
//! (Game.dll) reads the class map `GameEngine::LoadFromDatabase` fills with each class's
//! `<class>ItemHighlightColor` (`artifact` / `artifactFormula`: `<class>HighlightColor`) next to its `ItemColor`, so
//! the class is the plate's ([`super::character::rarity_key`]); a class without an entry gives (1, 1, 1, 0).
//! PORT: a bag window that is closed keeps the items glowing.
use super::additive::{AdditiveCache, AdditiveMaterial};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::Rect as URect;

#[derive(Component)]
pub struct NewItemGlow {
    pub key: (String, u32),
    pub rect: URect,
}

#[derive(Component)]
struct GlowColor(LinearRgba);

pub struct NewItemsPlugin;

impl Plugin for NewItemsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, animate_new_items.in_set(gd_ui_kit::UiPlaySet));
    }
}

/// `GameEngine::GetItemHighlightColor` of the bag item `key`.
fn highlight_color(
    data: &gd_game_core::data::GameData,
    inv: &gd_game_core::inventory::Inventory,
    key: &(String, u32),
) -> LinearRgba {
    let none = LinearRgba::new(1.0, 1.0, 1.0, 0.0);
    let Some(item) = inv
        .bags
        .iter()
        .flat_map(|b| &b.items)
        .find(|p| p.item.base_name == key.0 && p.item.seed == key.1)
        .map(|p| &p.item)
    else {
        return none;
    };
    let Some(class) = inv
        .info(item)
        .and_then(|i| super::character::rarity_key(i.resolved.display_color_index()))
    else {
        return none;
    };
    let Some(rec) = data.db.get("records/game/gameiteminfo.dbr") else {
        return none;
    };
    let field = if class.starts_with("artifact") {
        "HighlightColor"
    } else {
        "ItemHighlightColor"
    };
    let c = |k: &str| rec.f32(&format!("{class}{field}.{k}")).unwrap_or(0.0);
    Color::srgba(c("r"), c("g"), c("b"), c("a")).to_linear()
}

#[allow(clippy::too_many_arguments)]
fn animate_new_items(
    clock: Res<Time<Real>>,
    options: Res<gd_game_core::options::GameOptions>,
    data: Res<gd_game_core::data::GameData>,
    window: Query<&Window, With<PrimaryWindow>>,
    inv: Option<Res<gd_game_core::inventory::Inventory>>,
    glows: Query<(
        Entity,
        &NewItemGlow,
        Option<&GlowColor>,
        Option<&MaterialNode<AdditiveMaterial>>,
    )>,
    (mut commands, mut cache, mut mats): (
        Commands,
        ResMut<AdditiveCache>,
        ResMut<Assets<AdditiveMaterial>>,
    ),
    mut seen: MessageWriter<gd_game_core::inventory::NewItemSeen>,
    (pause, trade, confirm, stack): (
        Res<gd_game_core::pause::GamePause>,
        Res<crate::state::trade::Trade>,
        Res<super::apply::InvConfirm>,
        Res<super::stack::StackWindow>,
    ),
) {
    if glows.is_empty() {
        return;
    }
    let grid = data
        .db
        .get("records/ui/character/characterinventory/inventory_grid0.dbr");
    let f = |k: &str, d: f32| grid.as_ref().and_then(|g| g.f32(k)).unwrap_or(d);
    let (min, max, period) = (
        f("minNewItemAlpha", 0.25),
        f("maxNewItemAlpha", 0.75),
        f("newItemPeriod", 2.0),
    );
    let mid = (max - min) * 0.5 + min;
    let ph = if period > 0.0 {
        (clock.elapsed_secs() % period) / period
    } else {
        0.0
    };
    let mut v = min + (max - min) * ph;
    if v > mid {
        v = mid - (v - mid);
    }
    let on = options.flag("newItemHighlight") && period > 0.0;
    // Under the game menu, a modal question or the stack window the item is not hovered: its glow stays.
    let blocked = pause.menu
        || inv
            .as_deref()
            .is_some_and(|i| crate::apply::question_up(i, &trade, &confirm));
    let cursor = window
        .single()
        .ok()
        .and_then(super::cursor_px)
        .filter(|p| !blocked && !stack.covers(*p));
    let white = Handle::<Image>::default();
    for (e, g, color, mat) in &glows {
        let base = match color {
            Some(c) => c.0,
            None => {
                let c = inv
                    .as_deref()
                    .map_or(LinearRgba::NONE, |i| highlight_color(&data, i, &g.key));
                commands
                    .entity(e)
                    .try_remove::<BackgroundColor>()
                    .try_insert(GlowColor(c));
                c
            }
        };
        let a = if on { base.alpha * v } else { 0.0 };
        let h = cache.get(&mut mats, &white, base.with_alpha(a).into());
        if mat.is_none_or(|m| m.0 != h) {
            commands.entity(e).try_insert(MaterialNode(h));
        }
        if cursor.is_some_and(|p| super::contains(&g.rect, p)) {
            seen.write(gd_game_core::inventory::NewItemSeen(g.key.clone()));
        }
    }
}
