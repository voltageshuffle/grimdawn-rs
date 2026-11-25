//! The hardcore Game Over box's state ([`GameOverBox`]): the binary's `death.rs` builds it, the Esc
//! dispatcher answers it.
use bevy::prelude::*;

/// The Game Over dialog's state.
#[derive(Resource, Default)]
pub struct GameOverBox {
    pub open: bool,
    /// The Ok and close rects as drawn (physical px).
    pub rects: Option<(gd_formats::ui::Rect, Option<gd_formats::ui::Rect>)>,
}

impl GameOverBox {
    /// The box is up (open, or its rects still drawn): a modal, so Esc answers it (`ui::esc_dispatcher`).
    pub fn is_open(&self) -> bool {
        self.open || self.rects.is_some()
    }
}
