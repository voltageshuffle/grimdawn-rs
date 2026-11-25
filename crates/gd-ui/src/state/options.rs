//! The options window's state ([`OptionsWindow`], its tab and edited draft) and input set: the binary's
//! `options` module builds and edits it, the Esc dispatcher closes it.
use bevy::prelude::*;
use gd_game_core::options::GameOptions;
use gd_ui_kit::keys::KeyBindings;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Gameplay,
    Interface,
    HealthBars,
    KeyBindings,
    Audio,
    Video,
    Network,
}

/// The window's state: open, tab, the edited draft, the bindings' scroll.
#[derive(Resource, Default, Clone, PartialEq)]
pub struct OptionsWindow {
    pub open: bool,
    pub tab: Tab,
    pub draft: Option<GameOptions>,
    pub scroll: usize,
    /// The slider being dragged (option name).
    pub drag: Option<&'static str>,
    /// The key-binding list's thumb being dragged: the press's offset from the thumb's top (physical px).
    pub key_grab: Option<f32>,
    /// The open dropdown (its control's index in the page).
    pub dropdown: Option<usize>,
    /// The open list's first row shown (the binary's `options::DROP_ROWS` at a time).
    pub drop_top: usize,
    /// The edited key bindings, and the cell waiting for a key (action, 0
    /// primary / 1 secondary).
    pub keys: Option<KeyBindings>,
    pub rebind: Option<(u32, usize)>,
    /// Bumped whenever the keys draft is replaced or bound: the key list's
    /// layout version in the rebuild signature (the draft's values are live).
    pub keys_ver: u64,
}

/// The options window's keyboard / mouse input.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OptionsInputSet;

impl Tab {
    pub const ALL: [Tab; 7] = [
        Tab::Gameplay,
        Tab::Interface,
        Tab::HealthBars,
        Tab::KeyBindings,
        Tab::Audio,
        Tab::Video,
        Tab::Network,
    ];

    /// `GD_OPTIONS_OPEN`'s tab names.
    #[cfg(feature = "dev")]
    pub fn parse(s: &str) -> Option<Tab> {
        Some(match s {
            "gameplay" => Tab::Gameplay,
            "interface" => Tab::Interface,
            "healthbars" => Tab::HealthBars,
            "keybindings" => Tab::KeyBindings,
            "audio" => Tab::Audio,
            "video" => Tab::Video,
            "network" => Tab::Network,
            _ => return None,
        })
    }
}

impl OptionsWindow {
    /// The shared Esc dispatcher's close (`ui::esc_dispatcher`): the waiting rebind cell first,
    /// then an open list, then the window itself. Whether something closed.
    pub fn escape(&mut self) -> bool {
        if !self.open {
            return false;
        }
        if self.rebind.take().is_some() || self.dropdown.take().is_some() {
            return true;
        }
        self.open = false;
        true
    }
}
