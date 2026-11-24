//! The Crucible / Shattered Realm HUD's view state ([`SurvivalView`], [`PaneRollovers`]) and the compass
//! shift helpers: the binary's `survival.rs` fills them, `gd_ui`'s tracker / challenge / rollover read them.
use bevy::prelude::*;
use gd_formats::ui::Rect as URect;

/// What the pane shows, from the story's survival state.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct SurvivalView {
    /// A Crucible session (the pane shows).
    pub active: bool,
    /// The objective window shows and the quest tracker hides: a Crucible session, or the player is in the
    /// Shattered Realm (`UIObjectiveWindow`, `UIPlayerHud::Update`: in the dungeon, outside mode 1).
    pub objective_window: bool,
    pub wave: u32,
    pub score: u32,
    pub tribute: u32,
    /// Bonus timer seconds (`ms / 1000`, the text's resolution) and whether it runs.
    pub timer_secs: i32,
    pub warning: bool,
    pub multiplier: u32,
    /// Objective localisation tags (`Game.AddObjective`).
    pub objectives: Vec<String>,
    /// The event counters (see `Survival::tribute_gains`).
    pub counters: [u32; 4],
}

/// A rollover area of a pane (hover tests `UISurvivalHud::HitTestRollover` / `UIEndlessHud::HitTestRollover`): the
/// first area under the cursor shows its `RolloverLine2` record (`field` names the table field for the `GD_HUD_HOVER`
/// hook).
pub struct Zone {
    pub field: &'static str,
    pub rect: URect,
    pub record: String,
    /// A rollover with its own lines (style, text, wrapped) instead of a `RolloverLine2` record (mutator icons).
    pub lines: Option<Vec<(String, String, bool)>>,
}

/// The panes' rollover areas in hit-test order, set when a pane is built (`rollover.rs::hud_rollover` reads them).
#[derive(Resource, Default)]
pub struct PaneRollovers {
    pub crucible: Vec<Zone>,
    pub endless: Vec<Zone>,
    pub mutators: Vec<Zone>,
    /// The challenge-area window's name (`challenge.rs`).
    pub challenge: Vec<Zone>,
}

/// The compass mode the panes shift for (`gd_game::compass`; a new character's before the player is in).
pub fn compass_mode(c: Option<&gd_game::compass::CompassMode>) -> u8 {
    c.map_or(gd_game::compass::NEW_CHARACTER_MODE, |c| c.0)
}

/// The HUD shift for compass `mode` (`UIPlayerHud::ToggleCompass`, leftward; module doc): mode 2 none, mode 4 `15 +
/// (int)(246·s)` (`UIPlayerHud.minimapWidth` is the minimap widget's width, set by `UIMinimap::SetSize`), else (mode 3;
/// modes 0 / 1 leave the shift as it was, the port's 3) the minimap's 160 × s side plus the 20 px margin, unrounded.
pub fn hud_shift(s: f32, mode: u8) -> Vec2 {
    match mode {
        2 => Vec2::ZERO,
        4 => Vec2::new(-15.0 - (246.0 * s).trunc(), 0.0),
        _ => Vec2::new(-(160.0 * s + 20.0), 0.0),
    }
}
