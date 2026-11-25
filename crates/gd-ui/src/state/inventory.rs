//! The character window's hit-area layout and the cursor-capture flag (the binary's `inventory.rs`
//! input reads them; `gd_ui::character` builds them).
use bevy::prelude::*;
use gd_formats::inventory::EquipSlot;
use gd_formats::ui::Rect as URect;
use gd_ui_kit::contains;

/// Hit areas of a character-sheet list (page II / III).
#[derive(Clone, Default)]
pub struct SheetList {
    pub tab: u8,
    pub view: URect,
    /// The shared scroll bar (`ui::window::Bar`): thumb drag, arrow hold-repeat, track pages.
    pub bar: Option<crate::window::Bar>,
    /// Largest scroll offset (UI units).
    pub max: f32,
    /// UI scale: the content is shifted up by `round_px(scroll × s)` px.
    pub s: f32,
    /// Section headers (field name, rect at scroll 0).
    pub headers: Vec<(String, URect)>,
    /// Row rollovers (rect at scroll 0, rollover record, stats key; see [`InvLayout::sheet_rollovers`]).
    pub rows: Vec<(URect, String, String)>,
}

/// Screen rectangles (physical px) of the open character window.
#[derive(Resource, Default, Clone)]
pub struct InvLayout {
    pub window: Option<URect>,
    /// (bag index, rect, cell size px).
    pub grids: Vec<(usize, URect, f32)>,
    pub slots: Vec<(EquipSlot, URect)>,
    /// The paper doll's view (`crate::doll`): a held item clicked on it goes to its slot.
    pub doll: Option<URect>,
    /// Attribute + buttons (0 Physique, 1 Cunning, 2 Spirit) while points remain.
    pub attr_buttons: Vec<(u8, URect)>,
    /// Extra-bag tab buttons (bag index).
    pub bag_tabs: Vec<(usize, URect)>,
    /// Character-sheet page buttons (page 1..=3).
    pub sheet_tabs: Vec<(u8, URect)>,
    /// Auto-sort buttons (bag index).
    pub sort_buttons: Vec<(usize, URect)>,
    /// The page II / III list.
    pub sheet_list: Option<SheetList>,
    /// The weapon-swap button (`gd_game::weapon_sets`).
    pub swap_button: Option<URect>,
    /// The move-reagents button (`reagentDepositButton`, `gd_game_core::stash::DepositReagents`).
    pub reagent_button: Option<URect>,
    /// The window's close button (`characterCloseButton`): closes the window, as the key.
    pub close_button: Option<URect>,
    /// Stats-pane widgets with a `RolloverLine2` rollover: (rect, rollover
    /// record, `HudValues::stats` key of the row's value).
    pub sheet_rollovers: Vec<(URect, String, String)>,
}

/// True while the cursor is over UI that took the click this frame.
#[derive(Resource, Default)]
pub struct UiCapture(pub bool);

impl SheetList {
    /// How far the content is scrolled up (physical px) at `scroll` (UI units): the list scrolls in place
    /// (`ui::character::scroll_sheet`), so the rects below are kept at scroll 0.
    pub fn shift(&self, scroll: f32) -> f32 {
        gd_formats::ui::round_px(scroll.clamp(0.0, self.max) * self.s)
    }

    /// The section header under `p` at `scroll` (only the part inside the view: a header scrolled out
    /// of it takes no click, as [`Self::rows_at`]).
    pub fn header_at(&self, scroll: f32, p: Vec2) -> Option<&String> {
        let dy = self.shift(scroll);
        let view = self.view;
        self.headers
            .iter()
            .find(|(_, r)| {
                let y0 = (r.y - dy).max(view.y);
                let y1 = (r.y - dy + r.h).min(view.y + view.h);
                y1 > y0 && contains(&URect::new(r.x, y0, r.w, y1 - y0), p)
            })
            .map(|(n, _)| n)
    }

    /// The rows fully inside the view at `scroll`, with their rects there.
    pub fn rows_at(&self, scroll: f32) -> impl Iterator<Item = (URect, &String, &String)> {
        let dy = self.shift(scroll);
        let view = self.view;
        self.rows.iter().filter_map(move |(r, roll, key)| {
            let r = URect::new(r.x, r.y - dy, r.w, r.h);
            (r.y >= view.y && r.y + r.h <= view.y + view.h).then_some((r, roll, key))
        })
    }
}

impl InvLayout {
    /// Every stats-row rollover: the page I widgets and the visible rows of the page II / III list at the
    /// current scroll (`scroll` = `UiPanels::sheet_scroll`).
    pub fn sheet_rows<'a>(
        &'a self,
        scroll: &[f32; 4],
    ) -> impl Iterator<Item = (URect, &'a String, &'a String)> {
        let list = self
            .sheet_list
            .as_ref()
            .map(|l| l.rows_at(scroll[l.tab as usize]));
        self.sheet_rollovers
            .iter()
            .map(|(r, a, b)| (*r, a, b))
            .chain(list.into_iter().flatten())
    }
}
