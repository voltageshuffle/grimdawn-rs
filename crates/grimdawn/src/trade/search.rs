//! The trade windows' search box ([`TradeSearch`]; matching `gd_sim::items::market::search_market`): the
//! vendor window's `marketSearchBox`, the caravan window's `searchBox` and the crafting panel's `searchBox`
//! (all `editbox.tpl`, an `EditBox`). A left click on it presses the clear button first
//! (`MarketWindow::HandleMouseEvent` / `MarketWindow::ClearSearch`) and focuses it (`EditBox::Focus`); a click
//! elsewhere unfocuses it (`EditBox::Unfocus`). While focused it takes the keyboard: printable characters
//! (`EditBox::HandleKey` / `EditBox::RejectsText`), Backspace, Enter (unfocus); Esc unfocuses it instead of closing
//! the window (`MarketWindow::HandleEscape`). The text is cut to the box's `Width` − 10 px (`EditBox::ClipText`). The
//! clear button shows while the text is not empty; closing the window empties it (`MarketWindow::SetVisible`). Caret,
//! selection, Delete, key repeat and Ctrl+A/C/V are `gd_sim::edit`. PORT: the clipboard is process-local
//! ([`Clipboard`]); a click focuses with the caret at the end. The search lines are `Item::GenerateSearchText`'s.
//! The caravan window searches the shown stash's pages the same way (`CaravanWindow::FilterStash`; the query
//! survives the personal / transfer toggle). PORT: the transfer view's Materials / Components plates are not
//! searched. The crafting panel filters its recipe list instead (`CraftList::FilterRows`).
use super::*;
use gd_ui::state::trade::{Clipboard, Panel, Trade, TradeSearch, search_box_record};

/// The focused trade-window search box takes the keyboard (module doc): typed
/// text, Backspace, Enter / Esc unfocus; the game's keys see nothing
/// (`ButtonInput` reset) while it is focused. The text goes when the window
/// has no search box; the crafting panel keeps it while another tab shows.
#[allow(clippy::too_many_arguments)]
pub(super) fn search_input(
    #[cfg(feature = "dev")] (hooks, mut hooked): (Res<gd_game_core::hooks::Hooks>, Local<bool>),
    mut trade: ResMut<Trade>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    data: Res<gd_game_core::data::GameData>,
    fonts: ResMut<gd_ui_kit::text::FontCache>,
    mut font_assets: ResMut<Assets<Font>>,
    time: Res<Time>,
    mut clipboard: ResMut<Clipboard>,
    mut mods: Local<(bool, bool)>,
    pause: Res<gd_game_core::pause::GamePause>,
) {
    use bevy::input::keyboard::Key;
    use gd_sim::edit::EditKey;
    let has_box = trade.open.as_ref().is_some_and(|o| {
        search_box_record(o).is_some()
            && (!matches!(o, TradeOpen::Crafter { .. }) || trade.panel == Panel::Craft)
    });
    #[cfg(feature = "dev")]
    if !*hooked
        && has_box
        && let Some(t) = hooks.items.vendor_search.clone()
    {
        *hooked = true;
        trade.search.edit.to_end(&t);
        trade.search.text = t;
        trade.search.focused = true;
        trade.version += 1;
    }
    if !has_box {
        if matches!(trade.open, Some(TradeOpen::Crafter { .. })) {
            // The crafting panel's box is hidden on the other tabs: the query stays, the focus goes.
            if trade.search.focused {
                trade.search.focused = false;
                trade.version += 1;
            }
        } else if !trade.search.text.is_empty()
            || trade.search.focused
            || !trade.search.hits.is_empty()
        {
            trade.search = TradeSearch::default();
        }
        typed.clear();
        return;
    }
    // The game menu / Options (`GamePause::menu`) takes the keyboard: the box loses its focus (the click
    // that would unfocus it is the menu's).
    if pause.menu && trade.search.focused {
        trade.search.focused = false;
        trade.version += 1;
    }
    if !trade.search.focused {
        // Modifier releases still count while unfocused (a Ctrl let go here must not stick).
        for ev in typed.read() {
            use bevy::input::keyboard::KeyCode as K;
            let down = ev.state.is_pressed();
            match ev.key_code {
                K::ShiftLeft | K::ShiftRight => mods.0 = down,
                K::ControlLeft | K::ControlRight => mods.1 = down,
                _ => {}
            }
        }
        return;
    }
    let (mut fonts, Some(rec)) = (
        fonts,
        trade
            .open
            .as_ref()
            .and_then(search_box_record)
            .and_then(|r| data.db.get(r)),
    ) else {
        return;
    };
    let style = rec.str("Style").unwrap_or_default().to_owned();
    let max = rec.f32("Width").unwrap_or(0.0) - 10.0;
    let mut text = trade.search.text.clone();
    let mut edit = trade.search.edit.clone();
    let mut focused = true;
    let measure = std::cell::RefCell::new((&mut fonts, &mut font_assets));
    let fits = |t: &str| {
        let mut m = measure.borrow_mut();
        let (f, a) = &mut *m;
        f.text_width(&data, a, &style, 1.0, t) <= max
    };
    edit.tick(&mut text, time.delta_secs() * 1000.0, &fits);
    for ev in typed.read() {
        use bevy::input::keyboard::KeyCode as K;
        if !ev.state.is_pressed() {
            if matches!(ev.key_code, K::ShiftLeft | K::ShiftRight) {
                mods.0 = false;
            }
            if matches!(ev.key_code, K::ControlLeft | K::ControlRight) {
                mods.1 = false;
            }
            edit.release();
            continue;
        }
        match ev.key_code {
            K::ShiftLeft | K::ShiftRight => mods.0 = true,
            K::ControlLeft | K::ControlRight => mods.1 = true,
            _ => {}
        }
        if ev.repeat {
            continue;
        }
        let (shift, ctrl) = *mods;
        let key = match (&ev.logical_key, ev.key_code) {
            (_, K::KeyA) if ctrl => EditKey::SelectAll,
            (_, K::KeyC) if ctrl => EditKey::Copy,
            (_, K::KeyV) if ctrl => EditKey::Paste,
            _ if ctrl => continue,
            (Key::Character(c), _) => {
                EditKey::Text(c.chars().filter(|ch| !ch.is_control()).collect())
            }
            (Key::Space, _) => EditKey::Text(" ".into()),
            (Key::Backspace, _) => EditKey::Backspace,
            (Key::Delete, _) => EditKey::Delete,
            (Key::ArrowLeft, _) => EditKey::Left,
            (Key::ArrowRight, _) => EditKey::Right,
            (Key::Home, _) => EditKey::Home,
            (Key::End, _) => EditKey::End,
            (Key::Enter | Key::Escape, _) => {
                focused = false;
                continue;
            }
            _ => continue,
        };
        if let Some(c) = edit.press(&mut text, key, shift, &clipboard.0, &fits) {
            clipboard.0 = c;
        }
    }
    if text != trade.search.text
        || !focused
        || edit.caret != trade.search.edit.caret
        || edit.selection() != trade.search.edit.selection()
    {
        trade.search.text = text;
        trade.search.edit = edit;
        trade.search.focused = focused;
        trade.version += 1;
    } else {
        trade.search.edit = edit;
    }
    keys.reset_all();
}

/// Finds the search query's items in the open market or stash (module doc)
/// when the query or the stock changes; fills the crafting list's recipe
/// search lines once (`crafting.rs` filters its rows with the query itself).
#[allow(clippy::too_many_arguments)]
pub(super) fn search_hits(
    mut trade: ResMut<Trade>,
    markets: Res<Markets>,
    stash: Option<Res<Stash>>,
    mut craft: Option<ResMut<gd_ui::state::crafting::CraftList>>,
    data: Res<gd_game_core::data::GameData>,
    fonts: Res<gd_ui_kit::text::FontCache>,
    inv: Option<Res<Inventory>>,
    progress: Option<Res<PlayerProgress>>,
    hud: Res<gd_ui::HudValues>,
    rules: Option<Res<gd_game_core::rules::CombatRules>>,
    players: Query<&gd_game_core::combat::Combatant, With<Player>>,
) {
    if !trade.is_changed()
        && !markets.is_changed()
        && !stash.as_ref().is_some_and(|s| s.is_changed())
        && !craft.as_ref().is_some_and(|c| c.is_changed())
    {
        return;
    }
    // The cache key of the window's stock and whether it changed: the
    // market's, the shown stash's pages, or the crafting list (lines once).
    let crafter = matches!(trade.open, Some(TradeOpen::Crafter { .. }));
    let (key, changed) = match &trade.open {
        Some(TradeOpen::Vendor { key, .. }) => (key.clone(), markets.is_changed()),
        Some(TradeOpen::Stash { .. }) => (
            format!("stash:{}", trade.transfer),
            stash.as_ref().is_some_and(|s| s.is_changed()),
        ),
        Some(TradeOpen::Crafter { .. }) => {
            if craft.as_ref().is_some_and(|c| c.search_lines_ready) {
                return;
            }
            (String::new(), true)
        }
        _ => return,
    };
    let query = trade.search.query();
    let searched = Some((query.clone(), key));
    if !crafter && trade.search.searched == searched && !changed {
        return;
    }
    let (fonts, Some(inv)) = (fonts, inv) else {
        return;
    };
    let idb = &data.item_db();
    let player = players.single().ok();
    let sheet = player.map(|c| &c.profile.sheet);
    let have = gd_sim::items::tooltip::Have {
        level: progress.as_ref().map_or(1.0, |p| p.progress.level as f32),
        physique: sheet.map_or(0.0, |s| s.physique),
        cunning: sheet.map_or(0.0, |s| s.cunning),
        spirit: sheet.map_or(0.0, |s| s.spirit),
        reduction: player.map_or_else(Default::default, |c| c.profile.req_reduction),
    };
    // Set counts read what is worn (the Loadout source), not a detached slot's item.
    let equipped = inv.worn();
    let skills = gd_sim::skill_tooltip::SkillTipCtx {
        db: &data.db,
        trees: &data.skill_trees(),
        tags: &fonts.tags,
        book: &hud.skills,
        player: rules
            .as_ref()
            .zip(player)
            .map(|(r, c)| (&*r.0, &*c.profile)),
        pet_bonuses: &[],
    };
    let ctx = gd_sim::items::tooltip_item::TipCtx {
        db: idb,
        tags: &fonts.tags,
        have,
        equipped: &equipped,
        suppress_directions: true,
        formula_known: None,
        skills: Some(&skills),
        details: false,
        details_option: false,
        dps_delta: None,
        dps_offhand: None,
        weapon_anim_rate: None,
    };
    if crafter {
        if let Some(craft) = craft.as_mut() {
            craft.fill_search_lines(&ctx);
        }
        return;
    }
    let pages = match &trade.open {
        Some(TradeOpen::Vendor { key, .. }) => {
            markets.0.get(key).map(Vec::as_slice).unwrap_or_default()
        }
        Some(TradeOpen::Stash { .. }) => stash
            .as_ref()
            .map(|s| s.view(trade.transfer))
            .unwrap_or_default(),
        _ => return,
    };
    let hits = gd_sim::items::market::search_market(pages, &query, |item| {
        gd_sim::items::tooltip_item::item_search_text(&ctx, item)
            .iter()
            .map(|l| gd_sim::items::market::search_line(&l.text))
            .collect()
    });
    let search = &mut trade.search;
    search.searched = searched;
    if search.hits != hits {
        search.hits = hits;
        trade.version += 1;
    }
}
