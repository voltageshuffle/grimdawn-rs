//! The local / world map window's state ([`MapWindow`]) and its legend table ([`LEGEND`]): the binary's
//! `localmap` module builds it, the window manager and `gd_ui::rollover` read it.
use bevy::prelude::*;
use gd_formats::ui::Rect as URect;
use gd_game::riftgates::RiftgateMap;
use gd_ui_kit::contains;

/// Legend buttons in the record's order (`iconbutton_party` is left out:
/// single player).
pub const LEGEND: [&str; 12] = [
    "hero",
    "mission",
    "riftgate",
    "npc",
    "shrine",
    "transmutenpc",
    "aoi",
    "caravan",
    "inventor",
    "vendor",
    "witch",
    "smith",
];

/// The map window opened with `M` (or showing the local page over a
/// riftgate's map): open, which page, the local map's zoom / pan (metres along
/// the screen's right / up) and its built layout.
#[derive(Resource)]
pub struct MapWindow {
    pub open: bool,
    /// The local (aerial) map shows; else the world map (`riftmap.rs`).
    pub local: bool,
    pub zoom: f32,
    pub zoom_to: f32,
    pub pan: Vec2,
    pub layout: LocalLayout,
    /// Per nugget type: ms left of the legend click's blink.
    pub blink: [f32; 21],
    /// The legend button under the mouse (index into [`LEGEND`]).
    pub legend_hover: Option<usize>,
    /// The window button under the mouse (0 close, 1 centre, 2 switch).
    pub button_hover: Option<usize>,
    /// A mouse drag of the map: (start px, pan at the start).
    pub drag: Option<(Vec2, Vec2)>,
}

#[derive(Default, Clone, PartialEq)]
pub struct LocalLayout {
    pub window: Option<URect>,
    pub pane: URect,
    pub close: Option<URect>,
    pub center: Option<URect>,
    pub switch: Option<URect>,
    pub zoom: [Option<URect>; 2],
    /// Legend buttons (index into [`LEGEND`], rect).
    pub legend: Vec<(usize, URect)>,
    /// Metres per px of the drawn map.
    pub mpp: f32,
}

impl Default for MapWindow {
    fn default() -> Self {
        Self {
            open: false,
            local: true,
            zoom: 65.0,
            zoom_to: 65.0,
            pan: Vec2::ZERO,
            layout: LocalLayout::default(),
            blink: [0.0; 21],
            legend_hover: None,
            button_hover: None,
            drag: None,
        }
    }
}

impl MapWindow {
    /// The world (riftgate) map page shows: opened at a riftgate or from `M`.
    pub fn world_shown(&self, rift: &RiftgateMap) -> bool {
        (rift.open || self.open) && !self.local
    }
    pub fn local_shown(&self, rift: &RiftgateMap) -> bool {
        (rift.open || self.open) && self.local
    }
    /// The local map window's rect contains `p` (physical px) while it shows.
    pub fn covers(&self, rift: &RiftgateMap, p: Vec2) -> bool {
        self.local_shown(rift) && self.layout.window.is_some_and(|r| contains(&r, p))
    }
    /// The hovered legend button and its rollover record (`ui/rollover.rs`).
    pub fn legend_rollover(&self, rift: &RiftgateMap) -> Option<(URect, String)> {
        let i = self.legend_hover.filter(|_| self.local_shown(rift))?;
        let r = self.layout.legend.iter().find(|(k, _)| *k == i)?.1;
        Some((
            r,
            format!("records/ui/mapaerial/iconbutton_{}rollover.dbr", LEGEND[i]),
        ))
    }

    /// The switch tab: the other page.
    pub fn switch(&mut self, rift: &RiftgateMap) {
        self.local = !self.local;
        if !rift.open {
            self.open = true;
        }
    }
}
