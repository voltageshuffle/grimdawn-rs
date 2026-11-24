//! The world (riftgate) map's icons and window state ([`RiftMapView`]): the binary's `riftmap.rs` builds
//! it, `gd_ui::rollover` reads its buttons.
use bevy::prelude::*;
use gd_formats::map::Uid;
use gd_formats::ui::Rect as URect;
use gd_game::riftgates::Rift;
use gd_ui_kit::contains;

/// What an icon is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum IconKind {
    Gate(Uid),
    /// The personal portal (its object id).
    Portal(u32),
    Undiscovered,
    Shrine,
}

/// One map icon: its point (record units), bitmaps (normal, current,
/// hover) and name.
#[derive(Clone, Debug, PartialEq)]
pub struct Icon {
    pub at: Vec2,
    pub images: [String; 3],
    pub name: String,
    pub kind: IconKind,
    /// Its zone's point (where the map centres for it).
    pub zone: Vec2,
    /// The gate in use with the player at it (the "current" bitmap).
    pub current: bool,
}

/// The built window (physical px). Icon rects are in content px (relative to
/// the map content's origin, see [`RiftMapView::content_origin`]).
#[derive(Default, Clone, PartialEq)]
pub struct RiftLayout {
    pub window: Option<URect>,
    pub map: URect,
    pub close: Option<URect>,
    /// The map window's `centerButton`.
    pub center: Option<URect>,
    /// The map window's `switchMapButton` (to the local map).
    pub switch: Option<URect>,
    /// Section bounds (scaled px): min and size.
    pub bounds: (Vec2, Vec2),
    pub icons: Vec<(URect, Icon)>,
}

/// Window state: pan (px), drag, pressed icon, and the built layout.
#[derive(Resource, Default)]
pub struct RiftMapView {
    pub pan: Vec2,
    pub drag: Option<Vec2>,
    pub pressed: Option<usize>,
    pub layout: RiftLayout,
    /// The map was centred for this opening.
    pub centred: bool,
}

impl RiftMapView {
    /// The window button at a physical-px point: 0 close, 1 centre, 2 switch.
    pub fn button_at(&self, p: Vec2) -> Option<usize> {
        let over = |r: &URect| contains(r, p);
        [self.layout.close, self.layout.center, self.layout.switch]
            .iter()
            .position(|r| r.as_ref().is_some_and(over))
    }
}

impl Icon {
    /// The travel destination of a gate / portal icon.
    pub fn rift(&self) -> Option<Rift> {
        match self.kind {
            IconKind::Gate(u) => Some(Rift::Gate(u)),
            IconKind::Portal(id) => Some(Rift::Portal(id)),
            _ => None,
        }
    }
}

impl RiftMapView {
    /// Top-left of the map content in the map view (px): content point `q`
    /// (= `s·p − bounds min`) is at `map.xy + origin + q`.
    pub fn content_origin(&self) -> Vec2 {
        let (min, size) = self.layout.bounds;
        let centre = min + size * 0.5;
        Vec2::new(self.layout.map.w, self.layout.map.h) * 0.5 - (centre - min) - self.pan
    }

    pub fn clamp(&mut self) {
        let size = self.layout.bounds.1;
        let half = ((size - Vec2::new(self.layout.map.w, self.layout.map.h)) * 0.5).max(Vec2::ZERO);
        self.pan = self.pan.clamp(-half, half);
    }

    /// The icon under a screen point.
    pub fn icon_at(&self, p: Vec2) -> Option<usize> {
        let m = &self.layout.map;
        if !contains(m, p) {
            return None;
        }
        let q = p - Vec2::new(m.x, m.y) - self.content_origin();
        self.layout
            .icons
            .iter()
            .rposition(|(r, i)| i.kind != IconKind::Undiscovered && contains(r, q))
    }
}
