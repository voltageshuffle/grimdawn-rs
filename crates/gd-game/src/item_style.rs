//! Item styles, gameplay side (rules: `gd_sim::items::item_style`). A port addition (Grim Raynbow-like): the real
//! game has no such option.
//!
//! [`ItemStyles`] holds the rules for the whole install, not per character: they live in the port's own
//! settings folder next to `options.txt` (`<data dir>/Settings/itemstyles.txt`, [`path`]), never in the
//! character file, so saves stay the real game's. The file is read once at start-up ([`ItemStyles::load`],
//! called by the loot filter window's plugin); a missing file is no rules (the stock look), a bad line is
//! skipped with a warning, and a file that cannot be read (permissions, invalid UTF-8) is no rules with a warning
//! and is never written over. Both problems also go to [`ItemStyles::notice`], which the Item Styles page shows
//! (a release build has no log); a successful save clears it, a failed one sets it. The loot filter window sends
//! [`ItemStyleRequest`]s; [`apply_requests`] changes the rules, bumps [`ItemStyles::generation`] and writes the
//! file back (keeping the file's comment lines and its skipped lines as comments, `gd_sim::items::item_style`).
//!
//! Readers resolve a style when they build a label or tooltip (both rebuild on a signature that holds the
//! generation), never per frame: the ground labels (`grimdawn::loot`), the item tooltip's name line
//! (`gd_ui::tooltip`). The hide flag is the loot filter's ([`ItemStyles::hides`], in
//! `crate::loot_filter::LootFilter::passes`): the stock filter decides first, a style can only hide more,
//! and never while "Show Items (No Filter)" is held (`Mode::ShowAll`).
use bevy::prelude::*;
use gd_formats::save::item::Item;
use gd_game_core::inventory::Inventory;
use gd_sim::items::item_style::{Name, Rule, Style, StyleRules, StyledItem};
use std::path::PathBuf;

/// The rules and their change count (module doc).
#[derive(Resource, Default, Debug)]
pub struct ItemStyles {
    pub rules: StyleRules,
    /// Bumped by every change: the label / tooltip / window rebuild signatures hold it.
    pub generation: u32,
    /// Where edits are written; `None` (tests) writes nothing.
    pub file: Option<PathBuf>,
    /// The file's load or save problem for the player (module doc); changes only with `generation`.
    pub notice: Option<String>,
}

/// The rules' file in the port's settings folder (module doc).
pub fn path() -> PathBuf {
    gd_game_core::session::data_dir().join("Settings/itemstyles.txt")
}

impl ItemStyles {
    /// Reads `file` (module doc); edits go back to it. A missing file is no rules; a file that cannot be read
    /// (permissions, invalid UTF-8) is no rules too, with a warning, and is never written over.
    pub fn load(file: PathBuf) -> Self {
        let (rules, notice) = match std::fs::read_to_string(&file) {
            Ok(text) => {
                let (rules, warnings) = StyleRules::parse(&text);
                for w in &warnings {
                    warn!("item styles: {}: {w}", file.display());
                }
                info!(
                    "item styles: {} rules from {}",
                    rules.rules.len(),
                    file.display()
                );
                let notice = match warnings.len() {
                    0 => None,
                    1 => Some("1 line in itemstyles.txt skipped".to_owned()),
                    n => Some(format!("{n} lines in itemstyles.txt skipped")),
                };
                (rules, notice)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (StyleRules::default(), None),
            Err(e) => {
                warn!(
                    "item styles: cannot read {} ({e}): no rules, and edits are not saved",
                    file.display()
                );
                return Self {
                    notice: Some(
                        "itemstyles.txt could not be read: edits are not saved".to_owned(),
                    ),
                    ..default()
                };
            }
        };
        Self {
            rules,
            generation: 0,
            file: Some(file),
            notice,
        }
    }

    /// The style of the first matching rule.
    pub fn style(&self, item: &StyledItem) -> Option<&Style> {
        self.rules.find(item)
    }

    /// Whether the first rule matching `item` hides its label (no rule hides: no lookup at all).
    pub fn hides(&self, inv: &Inventory, data: &gd_game_core::data::GameData, item: &Item) -> bool {
        if !self.rules.any_hide() {
            return false;
        }
        let Some(info) = inv.info(item) else {
            return false;
        };
        let tags = data.tags();
        let styled = StyledItem::new(
            &info.resolved,
            data.db.get(&item.base_name),
            Name::Tags(&tags),
        );
        self.style(&styled).is_some_and(|s| s.hide)
    }
}

/// The loot filter window's edits of the rules (module doc).
#[derive(Message, Clone, Debug)]
pub enum ItemStyleRequest {
    /// A new rule (no conditions, no style) at the end.
    Add,
    Remove(usize),
    /// Swaps rule `i` with the one above (`true`) or below.
    Move(usize, bool),
    /// Replaces rule `i`.
    Set(usize, Rule),
    /// Replaces every rule with the built-in preset (`StyleRules::preset`).
    Preset,
    Clear,
}

/// Applies the window's requests, then bumps the generation and writes the file once.
pub fn apply_requests(
    mut requests: MessageReader<ItemStyleRequest>,
    mut styles: ResMut<ItemStyles>,
) {
    let mut changed = false;
    for r in requests.read() {
        let rules = &mut styles.rules.rules;
        let n = rules.len();
        changed |= match r {
            ItemStyleRequest::Add => {
                rules.push(Rule::default());
                true
            }
            ItemStyleRequest::Remove(i) if *i < n => {
                rules.remove(*i);
                true
            }
            ItemStyleRequest::Move(i, true) if *i > 0 && *i < n => {
                rules.swap(*i, i - 1);
                true
            }
            ItemStyleRequest::Move(i, false) if i + 1 < n => {
                rules.swap(*i, i + 1);
                true
            }
            ItemStyleRequest::Set(i, rule) if *i < n => {
                rules[*i] = rule.clone();
                true
            }
            ItemStyleRequest::Preset => {
                // The user's comment lines stay.
                let comments = std::mem::take(&mut styles.rules.comments);
                styles.rules = StyleRules::preset();
                styles.rules.comments = comments;
                true
            }
            ItemStyleRequest::Clear => {
                rules.clear();
                true
            }
            _ => false,
        };
    }
    if !changed {
        return;
    }
    styles.generation = styles.generation.wrapping_add(1);
    let saved = styles.file.as_ref().map(|file| {
        gd_game_core::session::try_write_user_file("item styles", file, styles.rules.write())
    });
    if let Some(saved) = saved {
        styles.notice = (!saved).then(|| "itemstyles.txt could not be saved".to_owned());
    }
}
