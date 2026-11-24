//! Shared wiring of the pop-up windows (shrine, apply, ...): each window is a `build` system (rebuilds its
//! widget tree when its signature changes) chained before a `clicks` system (in [`super::UiInputSet`] and
//! [`WindowClicks`]), both in [`UiPlaySet`] and gated by the window's own `live` condition.
//!
//! PORT (performance): `live` is "open, or still built", so a closed, unbuilt window costs no system run; the
//! frame it closes still runs `build` once so it despawns its root.
//!
//! The shared scroll bar ([`scrollbar_live`] / [`Bar`] / [`bar_held`]): the journal's `UIScrollWindow` bar,
//! reused by every scrollable window; [`ScrollThumb`] pieces and the [`pane`] container move in place.
//!
//! The window manager's one open/close policy ([`WindowPolicy`], [`Windows`]): which windows opening one
//! closes (`InGameUI::HandleKeyAction` / `UIPlayerHud::CloseWindows`). Esc and focus are the one dispatcher's
//! (`super::esc_dispatcher`). PORT: windows sit in fixed z bands ([`Z_WINDOW`] … [`Z_OPTIONS`]) instead of
//! the engine's focus order.
use super::{UiInputSet, UiPlaySet};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::ScheduleSystem;
use bevy::prelude::*;

/// The "built signature" of a window builder (a `Local<Rebuild<Sig>>`): the builder computes its signature each
/// frame, rebuilds only when [`Rebuild::changed`] says so, and `NewSession` calls [`Rebuild::reset`].
pub struct Rebuild<S>(Option<S>);

impl<S> Default for Rebuild<S> {
    fn default() -> Self {
        Self(None)
    }
}

impl<S: PartialEq> Rebuild<S> {
    /// Whether `sig` differs from the last built one; if so, remembers it (the caller rebuilds now).
    pub fn changed(&mut self, sig: S) -> bool {
        if self.0.as_ref() == Some(&sig) {
            return false;
        }
        self.0 = Some(sig);
        true
    }

    /// As [`Self::changed`], but builds the signature only when `same` says the built one differs: no
    /// per-frame clone of the inputs.
    pub fn changed_by(&mut self, same: impl FnOnce(&S) -> bool, make: impl FnOnce() -> S) -> bool {
        if self.0.as_ref().is_some_and(same) {
            return false;
        }
        self.0 = Some(make());
        true
    }

    /// Forgets the built signature: the next `changed` is true.
    pub fn reset(&mut self) {
        self.0 = None;
    }
}

/// Despawns the old roots of a window that is about to be rebuilt (a second despawn in one frame is not an error).
pub fn despawn_all(
    commands: &mut Commands,
    roots: &Query<Entity, impl bevy::ecs::query::QueryFilter>,
) {
    for e in roots {
        commands.entity(e).try_despawn();
    }
}

/// The `clicks` systems of [`add_window`] windows: they see Escape before the shared dispatcher
/// (`super::esc_dispatcher`), so a modal box (the Game Over box) answers it instead of the game menu opening.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WindowClicks;

/// Adds `build` then `clicks` to `Update` while playing and `live`.
pub fn add_window<B, C, L>(
    app: &mut App,
    build: impl IntoScheduleConfigs<ScheduleSystem, B>,
    clicks: impl IntoScheduleConfigs<ScheduleSystem, C>,
    live: impl SystemCondition<L>,
) {
    app.add_systems(
        Update,
        (
            build.in_set(UiPlaySet),
            clicks.in_set(UiInputSet).in_set(WindowClicks),
        )
            .chain()
            .run_if(live),
    );
}

/// The shared window chrome's z bands (`super::spawn_root`): the play windows (altar, shrine, potions,
/// the NPC windows, the map, the riftgate map, the conversation) at [`Z_WINDOW`], the journal one below
/// (`Z_WINDOW - 1`), their confirmations / pick / stack boxes and the loot filter window at [`Z_MODAL`]
/// (a second modal layer `Z_MODAL + 1`), the factions / achievements band at [`Z_PANEL`] (their scroll
/// bars `Z_PANEL + 1`), the game menu at [`Z_MENU`] (its dim `Z_MENU - 10`) and the options window over it at [`Z_OPTIONS`].
pub const Z_WINDOW: i32 = 5;
/// The play windows' confirmations, pick / stack boxes and the loot filter window (see [`Z_WINDOW`]).
pub const Z_MODAL: i32 = 6;
/// The factions / achievements band (see [`Z_WINDOW`]).
pub const Z_PANEL: i32 = 30;
/// The game menu (see [`Z_WINDOW`]).
pub const Z_MENU: i32 = 160;
/// The options window, over the game menu (see [`Z_WINDOW`]).
pub const Z_OPTIONS: i32 = 165;

/// A window's close button (the shared chrome): the record's `Widget::Button` up bitmap at its
/// `bitmapPosition` from the window origin `o`, its press sound registered
/// (`Ui::record_path_button_sound`); returns its rect for the window's click layout. The close itself
/// is the window's (a click on the rect) or the shared Esc dispatcher's (`super::esc_dispatcher`).
pub fn close_button(ui: &mut super::Ui, path: &str, o: Vec2) -> Option<gd_formats::ui::Rect> {
    let Some(gd_formats::ui::Widget::Button {
        up: Some(up), x, y, ..
    }) = ui.widget(path)
    else {
        return None;
    };
    let (_, r) = ui.bitmap(&up, x, y, o)?;
    ui.record_path_button_sound(r, path);
    Some(r)
}

/// A clipped pane: a node over `r` whose child spans the screen shifted up
/// by `scroll`, so children keep screen coordinates; returns the child (the owner's live system
/// moves it by the scroll, [`ScrollThumb`] moves the bar's thumb).
pub fn pane(ui: &mut super::Ui, r: gd_formats::ui::Rect, scroll: f32) -> Entity {
    let clip = ui
        .commands
        .spawn(Node {
            overflow: Overflow::clip(),
            ..ui.node(r)
        })
        .id();
    ui.commands.entity(ui.root).add_child(clip);
    let inner = ui
        .commands
        .spawn(ui.node(gd_formats::ui::Rect::new(
            -r.x,
            -r.y - scroll,
            ui.screen.x,
            ui.screen.y,
        )))
        .id();
    ui.commands.entity(clip).add_child(inner);
    inner
}

/// A drawn scroll bar: its buttons, track span and thumb. The layout keeps the bar as built while the thumb moves
/// with the scroll in place ([`ScrollThumb`]), so a press places the thumb from the scroll ([`Bar::press`]).
#[derive(Clone, Copy, Default, PartialEq)]
pub struct Bar {
    pub up: gd_formats::ui::Rect,
    pub down: gd_formats::ui::Rect,
    pub track: (f32, f32),
    /// The thumb at scroll 0 (its length unrounded, as the drawn travel `track − length`).
    thumb: gd_formats::ui::Rect,
}

/// A held scroll bar part (`UIScrollWindow::HandleArrowInput` arrows, `UIScrollWindow::HandleScrollInput`
/// track and thumb, `UIScrollWindow::RepeatScroll` repeat): an arrow's step or a track
/// page's step and pages left (content px), or the thumb's grab offset; the
/// repeat delay and interval left (ms).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarHold {
    what: Held,
    delay: f32,
    interval: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Held {
    Arrow(f32),
    Page(f32, u32),
    Thumb(f32),
}

impl BarHold {
    fn new(what: Held) -> Self {
        Self {
            what,
            delay: 300.0,
            interval: 5.0,
        }
    }
}

impl Bar {
    /// The thumb's unrounded top at `scroll` of `max` (where [`draw_scrollbar`] draws it and the owner's live
    /// system moves it).
    fn thumb_top(&self, scroll: f32, max: f32) -> f32 {
        let f = if max > 0.0 {
            (scroll / max).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.thumb.y + (self.track.1 - self.track.0 - self.thumb.h) * f
    }

    /// A left press at `p`: the new scroll and the hold, if on the bar.
    /// An arrow scrolls 20 × s px at once; the track above / below the thumb
    /// pages by a fifth of the track (in thumb px, so `track / 5 × max /
    /// (track − thumb)` content px) at once, and while held as many more
    /// times as whole pages lie between the thumb and the press; the thumb grabs.
    pub fn press(&self, p: Vec2, s: f32, scroll: f32, max: f32) -> Option<(f32, BarHold)> {
        use gd_formats::ui::Rect as URect;
        let inside = |r: &URect| super::contains(r, p);
        let step = 20.0 * s;
        let arrow = if inside(&self.up) {
            Some(-step)
        } else if inside(&self.down) {
            Some(step)
        } else {
            None
        };
        if let Some(d) = arrow {
            return Some(((scroll + d).clamp(0.0, max), BarHold::new(Held::Arrow(d))));
        }
        // The thumb where it is drawn now (rounded as the draw rounds its ends).
        let top = self.thumb_top(scroll, max);
        let (y0, y1) = (top.round(), (top + self.thumb.h).round());
        let thumb = URect::new(self.thumb.x, y0, self.thumb.w, y1 - y0);
        if inside(&thumb) {
            return Some((scroll, BarHold::new(Held::Thumb(p.y - top))));
        }
        let (t0, t1) = self.track;
        if p.x < thumb.x || p.x >= thumb.x + thumb.w || p.y < t0 || p.y >= t1 {
            return None;
        }
        let track = t1 - t0;
        let page = (track / 5.0).trunc().max(1.0);
        let room = (track - self.thumb.h).max(1.0);
        let (d, gap) = if p.y < thumb.y {
            (-page, thumb.y - p.y)
        } else {
            (page, p.y - (thumb.y + thumb.h))
        };
        let d = d * max / room;
        Some((
            (scroll + d).clamp(0.0, max),
            BarHold::new(Held::Page(d, (gap / page) as u32)),
        ))
    }
}

/// A held bar part after `dt_ms` with the cursor at `y`: a held arrow or
/// track page repeats once the 300 ms delay is over, every 5 ms — whole
/// steps accumulated, the remainder kept (fps-independent, as `doll.rs`'s
/// hold-repeat; a page only its count of times); a held thumb follows the
/// mouse; returns the new scroll.
pub fn bar_held(
    bar: Option<Bar>,
    hold: &mut BarHold,
    y: f32,
    dt_ms: f32,
    scroll: f32,
    max: f32,
) -> f32 {
    let step = match &mut hold.what {
        Held::Arrow(d) => Some(*d),
        Held::Page(d, left) => (*left > 0).then_some(*d),
        Held::Thumb(grab) => {
            let Some(b) = bar else { return scroll };
            let room = (b.track.1 - b.track.0 - b.thumb.h).max(1.0);
            let v = ((y - *grab - b.track.0) / room * max).clamp(0.0, max);
            return if (v - scroll).abs() >= 0.5 { v } else { scroll };
        }
    };
    if hold.delay > 0.0 {
        hold.delay -= dt_ms;
    }
    if hold.delay > 0.0 {
        return scroll;
    }
    hold.interval -= dt_ms;
    if hold.interval > 0.0 {
        return scroll;
    }
    let Some(d) = step else { return scroll };
    let mut steps = 0u32;
    while hold.interval <= 0.0 {
        hold.interval += 5.0;
        if let Held::Page(_, left) = &mut hold.what {
            if *left == 0 {
                break;
            }
            *left -= 1;
        }
        steps += 1;
    }
    (scroll + d * steps as f32).clamp(0.0, max)
}

/// Which window's scroll state drives a [`ScrollThumb`]: each window's live system moves its own
/// (the journal's panes, the character sheet's lists, the potions description, the factions and
/// achievements lists, the crafting recipe list).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ScrollOwner {
    Journal,
    Sheet,
    Potions,
    Factions,
    Achievements,
    Craft,
}

/// A scroll bar thumb piece ([`scrollbar_live`]): the owner's live system moves it along the track.
#[derive(Component)]
pub struct ScrollThumb {
    pub owner: ScrollOwner,
    pub pane: usize,
    pub base_y: f32,
    pub round_add: f32,
    pub travel: f32,
    pub max: f32,
    pub inv: f32,
}

/// A vertical scroll bar (`scrollbar.tpl`: `upButton`, `downButton`, `scrollTrack`, `scrollBar` with first /
/// middle / last textures) along the pane's right edge `right` (the pane loses its width;
/// `UIScrollWindow::Load`), spanning `y .. y + len`, for `visible` px of `content` scrolled by `scroll`. The thumb
/// pieces carry [`ScrollThumb`], so the owner's live system moves them in place instead of a rebuild per
/// scroll step. PORT: thumb geometry (proportional, at least its two ends) and the buttons' looks are not
/// decompiled.
#[allow(clippy::too_many_arguments)]
pub fn scrollbar_live(
    ui: &mut super::Ui,
    rec: &str,
    right: f32,
    y: f32,
    len: f32,
    (visible, content, scroll): (f32, f32, f32),
    owner: ScrollOwner,
    pane: usize,
) -> Option<Bar> {
    draw_scrollbar(
        ui,
        rec,
        right,
        y,
        len,
        (visible, content, scroll),
        (owner, pane),
    )
}

fn draw_scrollbar(
    ui: &mut super::Ui,
    rec: &str,
    right: f32,
    y: f32,
    len: f32,
    (visible, content, scroll): (f32, f32, f32),
    live: (ScrollOwner, usize),
) -> Option<Bar> {
    use gd_formats::ui::Rect as URect;
    let r = ui.record(rec)?;
    let field = |k: &str| r.str(k).unwrap_or("").to_owned();
    let img = |ui: &mut super::Ui, rec: &str, k: &str| {
        ui.record(rec)
            .and_then(|b| b.str(k).map(str::to_owned))
            .and_then(|p| ui.image(&p))
    };
    let (up, down) = (
        img(ui, &field("upButton"), "bitmapNameUp"),
        img(ui, &field("downButton"), "bitmapNameUp"),
    );
    let w = up.as_ref().map_or(0.0, |(_, s)| ui.sc(s.x as f32));
    let x = right - w;
    let uh = up.as_ref().map_or(0.0, |(_, s)| ui.sc(s.y as f32));
    let dh = down.as_ref().map_or(0.0, |(_, s)| ui.sc(s.y as f32));
    let (t0, t1) = (y + uh, y + len - dh);
    let piece = |ui: &mut super::Ui, rec: &str, from: f32, to: f32| -> Option<[Entity; 3]> {
        let (Some(first), Some(mid), Some(last)) = (
            img(ui, rec, "firstEndTexture"),
            img(ui, rec, "middleTexture"),
            img(ui, rec, "lastEndTexture"),
        ) else {
            return None;
        };
        let (fh, lh) = (ui.sc(first.1.y as f32), ui.sc(last.1.y as f32));
        let pw = ui.sc(first.1.x as f32);
        let px = x + ((w - pw) * 0.5).round();
        Some([
            ui.quad_stretch(first.0, URect::new(px, from, pw, fh), Color::WHITE),
            ui.quad_stretch(
                mid.0,
                URect::new(px, from + fh, pw, (to - from - fh - lh).max(0.0)),
                Color::WHITE,
            ),
            ui.quad_stretch(last.0, URect::new(px, to - lh, pw, lh), Color::WHITE),
        ])
    };
    piece(ui, &field("scrollTrack"), t0, t1);
    let track = (t1 - t0).max(1.0);
    let thumb = (track * visible / content.max(1.0)).clamp(ui.sc(16.0), track);
    let max = (content - visible).max(1.0);
    let top = t0 + (track - thumb) * (scroll / max).clamp(0.0, 1.0);
    let pieces = piece(ui, &field("scrollBar"), top.round(), (top + thumb).round());
    if let Some([first, mid, last]) = pieces {
        let (owner, pane) = live;
        // Each piece's top at scroll 0 (`base_y`) plus the rounding the draw applies: the first and
        // middle round the thumb's top, the last rounds its bottom.
        let sb = field("scrollBar");
        let fh = img(ui, &sb, "firstEndTexture").map_or(0.0, |(_, s)| ui.sc(s.y as f32));
        let lh = img(ui, &sb, "lastEndTexture").map_or(0.0, |(_, s)| ui.sc(s.y as f32));
        let travel = track - thumb;
        for (e, base_y, round_add) in [(first, t0, 0.0), (mid, t0, fh), (last, t0 + thumb, -lh)] {
            ui.commands.entity(e).insert(ScrollThumb {
                owner,
                pane,
                base_y,
                round_add,
                travel,
                max,
                inv: ui.inv,
            });
        }
    }
    if let Some((h, _)) = up {
        ui.quad(h, URect::new(x, y, w, uh), None, Color::WHITE);
    }
    if let Some((h, s)) = down {
        ui.quad(
            h,
            URect::new(x, y + len - dh, ui.sc(s.x as f32), dh),
            None,
            Color::WHITE,
        );
    }
    Some(Bar {
        up: URect::new(x, y, w, uh),
        down: URect::new(x, y + len - dh, w, dh),
        track: (t0, t1),
        thumb: URect::new(x, t0, w, thumb),
    })
}

/// The windows the one open/close policy ([`WindowPolicy`]) manages. The first eight are the UI's own
/// (their open flags live in the UI resources); the rest are gameplay windows the policy only reads
/// (their openers are gd-game's; closing them goes through their requests).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Window {
    Character,
    Skills,
    Devotion,
    Journal,
    Map,
    Factions,
    Achievements,
    LootFilter,
    Altar,
    Shrine,
    Potions,
    Trade,
    Conversation,
    GameMenu,
}

impl Window {
    pub const COUNT: usize = 14;
    pub const ALL: [Window; Self::COUNT] = [
        Window::Character,
        Window::Skills,
        Window::Devotion,
        Window::Journal,
        Window::Map,
        Window::Factions,
        Window::Achievements,
        Window::LootFilter,
        Window::Altar,
        Window::Shrine,
        Window::Potions,
        Window::Trade,
        Window::Conversation,
        Window::GameMenu,
    ];
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|w| *w == self).unwrap_or(0)
    }
}

/// The window manager's single open/close policy (the engine's focus model: the factions key
/// `InGameUI::HandleKeyAction` case 52 and the openers' `UIPlayerHud::CloseWindows` / `UIPlayerHud::ClosePanels`):
/// opening a window closes the windows its row names. One table for every opener and watcher (the per-window copies
/// disagreed). Initialized by the factions plugin (the first user).
#[derive(Resource)]
pub struct WindowPolicy {
    closes: [u16; Window::COUNT],
}

impl Default for WindowPolicy {
    fn default() -> Self {
        use self::Window::*;
        let mask = |ws: &[Window]| ws.iter().fold(0u16, |m, w| m | (1 << w.index()));
        let mut closes = [0u16; Window::COUNT];
        // Character / Skills and the full-screen Devotion map replace each other (the HUD level-up
        // button already closes Devotion before opening Skills): a window opened under the map would
        // stay hidden yet take clicks.
        closes[Character.index()] = mask(&[Skills, Devotion, Factions, Achievements, LootFilter]);
        closes[Skills.index()] = mask(&[Character, Devotion, Factions, Achievements, LootFilter]);
        closes[Devotion.index()] = mask(&[Character, Skills, Factions, Achievements]);
        closes[Journal.index()] = mask(&[Factions, Achievements]);
        closes[Map.index()] = mask(&[Factions, Achievements, LootFilter]);
        closes[Factions.index()] = mask(&[
            Character,
            Skills,
            Devotion,
            Journal,
            Map,
            Achievements,
            LootFilter,
        ]);
        closes[Achievements.index()] = mask(&[
            Character, Skills, Devotion, Journal, Map, Factions, LootFilter,
        ]);
        // `UIPlayerHud::ClosePanels` (lootfilter.rs doc): the filter press closes factions / achievements too.
        closes[LootFilter.index()] = mask(&[
            Character,
            Skills,
            Devotion,
            Map,
            Altar,
            Shrine,
            Potions,
            Trade,
            Factions,
            Achievements,
        ]);
        // The gameplay windows close the factions / achievements windows.
        closes[Potions.index()] = mask(&[Factions, Achievements]);
        closes[Trade.index()] = mask(&[Factions, Achievements]);
        closes[Conversation.index()] = mask(&[Factions, Achievements]);
        closes[GameMenu.index()] = mask(&[LootFilter]);
        Self { closes }
    }
}

impl WindowPolicy {
    /// The windows opening `w` closes (an owned iterator: the mask is copied, no borrow of the policy).
    pub fn closes(&self, w: Window) -> impl Iterator<Item = Window> + use<> {
        let m = self.closes[w.index()];
        Window::ALL
            .into_iter()
            .filter(move |o| m & (1 << o.index()) != 0)
    }

    /// Whether a window that closes `w` opened between `was` and `now` (the watchers' reverse rule).
    pub fn opened_closer(
        &self,
        w: Window,
        was: &[bool; Window::COUNT],
        now: &[bool; Window::COUNT],
    ) -> bool {
        Window::ALL.into_iter().any(|o| {
            self.closes[o.index()] & (1 << w.index()) != 0 && now[o.index()] && !was[o.index()]
        })
    }
}

/// Every managed window's open state, writable for the UI's own: the openers' and watchers' one access
/// to the policy ([`WindowPolicy`]), and the Esc dispatcher's one close path ([`Self::close`]). Closing a
/// gameplay window goes through its request (the shrine's / potions' / conversation's cancel, the NPC
/// windows' `ServiceRequest::Close`, the riftgate map's `RiftgateRequest::Close`).
#[derive(bevy::ecs::system::SystemParam)]
pub struct Windows<'w> {
    pub panels: ResMut<'w, super::UiPanels>,
    pub journal: ResMut<'w, super::quests::QuestJournal>,
    pub map: ResMut<'w, crate::state::map::MapWindow>,
    pub factions: ResMut<'w, super::factions::FactionsView>,
    pub achievements: ResMut<'w, super::achievements::AchievementsView>,
    pub loot: ResMut<'w, super::lootfilter::LootFilterView>,
    pub menu: ResMut<'w, super::gamemenu::GameMenu>,
    pub altar: ResMut<'w, crate::state::ascendant::AltarMenu>,
    pub trade: Res<'w, crate::state::trade::Trade>,
    shrine: Res<'w, gd_game::devotion::ShrineWindow>,
    potions: Res<'w, gd_game::cauldron::PotionWindow>,
    conv: Res<'w, gd_game::conversation::Conversation>,
    rift: Res<'w, gd_game::riftgates::RiftgateMap>,
    policy: Res<'w, WindowPolicy>,
    out: (
        MessageWriter<'w, gd_game::devotion::ShrineRequest>,
        MessageWriter<'w, gd_game::cauldron::PotionRequest>,
        MessageWriter<'w, gd_game::trade::ServiceRequest>,
        MessageWriter<'w, gd_game::conversation::ConversationRequest>,
        MessageWriter<'w, gd_game::riftgates::RiftgateRequest>,
    ),
}

impl Windows<'_> {
    pub fn is_open(&self, w: Window) -> bool {
        match w {
            Window::Character => self.panels.character,
            Window::Skills => self.panels.skills,
            Window::Devotion => self.panels.devotion,
            Window::Journal => self.journal.open,
            // The riftgate map is the map window's world page: it is on the focus stack too, so Esc
            // closes it through `set_open(Map, false)`.
            Window::Map => self.map.open || self.rift.open,
            Window::Factions => self.factions.open,
            Window::Achievements => self.achievements.open,
            Window::LootFilter => self.loot.open,
            Window::Altar => self.altar.open,
            Window::Shrine => self.shrine.shrine.is_some(),
            Window::Potions => self.potions.open,
            Window::Trade => self.trade.open.is_some(),
            Window::Conversation => self.conv.view.is_some(),
            Window::GameMenu => self.menu.open,
        }
    }

    /// Every window's open state, in [`Window::ALL`] order (the watchers' snapshot).
    pub fn snapshot(&self) -> [bool; Window::COUNT] {
        std::array::from_fn(|i| self.is_open(Window::ALL[i]))
    }

    fn set_open(&mut self, w: Window, open: bool) {
        match w {
            Window::Character => self.panels.character = open,
            Window::Skills => self.panels.skills = open,
            Window::Devotion => self.panels.devotion = open,
            Window::Journal => self.journal.open = open,
            Window::Map => {
                self.map.open = open;
                // The riftgate map is the map window's world page: closing the map closes it too.
                if !open && self.rift.open {
                    self.out.4.write(gd_game::riftgates::RiftgateRequest::Close);
                }
            }
            Window::Factions => self.factions.open = open,
            Window::Achievements => self.achievements.open = open,
            Window::LootFilter => self.loot.open = open,
            Window::Altar => self.altar.open = open,
            Window::GameMenu => self.menu.open = open,
            // The gameplay windows only close (their openers are gd-game's).
            Window::Shrine if !open && self.shrine.shrine.is_some() => {
                self.out.0.write(gd_game::devotion::ShrineRequest::Cancel);
            }
            Window::Potions if !open && self.potions.open => {
                self.out.1.write(gd_game::cauldron::PotionRequest::Close);
            }
            Window::Trade if !open && self.trade.open.is_some() => {
                // `gd_game::trade::service_answers` resets the window on this request.
                self.out.2.write(gd_game::trade::ServiceRequest::Close);
            }
            Window::Conversation if !open && self.conv.view.is_some() => {
                self.out
                    .3
                    .write(gd_game::conversation::ConversationRequest::Close);
            }
            _ => {}
        }
    }

    /// Opens `w`, closing what the policy excludes (the one open/close rule).
    pub fn open(&mut self, w: Window) {
        for o in self.policy.closes(w) {
            self.set_open(o, false);
        }
        self.set_open(w, true);
    }

    /// The window's key / HUD button: open with the policy, or close.
    pub fn toggle(&mut self, w: Window) {
        if self.is_open(w) {
            self.set_open(w, false);
        } else {
            self.open(w);
        }
    }

    /// Closes `w` (the Esc dispatcher's one close path).
    pub fn close(&mut self, w: Window) {
        self.set_open(w, false);
    }
}

/// A `ButtonStaticText`'s state bitmaps on its quad ([`static_button_live`]): [`set_button_look`] swaps
/// them in place, so a hover never rebuilds the window.
#[derive(Component)]
pub struct ButtonLook {
    pub up: Handle<Image>,
    pub over: Option<Handle<Image>>,
    pub disabled: Option<Handle<Image>>,
}

/// [`super::widgets::static_button`] drawn in its up (or disabled) state with [`ButtonLook`] on the
/// quad: returns the rect and the quad entity. (The shared helper returns only the rect; a live hover
/// needs the entity to swap its bitmap.) The label and its disabled dim are as `static_button`'s.
pub fn static_button_live(
    ui: &mut super::Ui,
    rec: &gd_formats::arz::Record,
    o: Vec2,
    enabled: bool,
    label: super::widgets::Label,
) -> Option<(gd_formats::ui::Rect, Entity)> {
    use gd_formats::ui::{Rect as URect, TextAlign};
    let key = if !enabled && rec.str("bitmapNameDisabled").is_some() {
        "bitmapNameDisabled"
    } else {
        "bitmapNameUp"
    };
    let (img, size) = rec.str(key).map(str::to_owned).and_then(|p| ui.image(&p))?;
    let r = URect::new(
        o.x + ui.sc(rec.f32("bitmapPositionX").unwrap_or(0.0)),
        o.y + ui.sc(rec.f32("bitmapPositionY").unwrap_or(0.0)),
        ui.sc(size.x as f32),
        ui.sc(size.y as f32),
    );
    let e = ui.quad(img.clone(), r, None, Color::WHITE);
    let load = |ui: &mut super::Ui, k: &str| {
        rec.str(k)
            .map(str::to_owned)
            .and_then(|p| ui.image(&p))
            .map(|(h, _)| h)
    };
    let over = load(ui, "bitmapNameInFocus");
    let disabled = load(ui, "bitmapNameDisabled");
    ui.commands.entity(e).insert(ButtonLook {
        up: img,
        over,
        disabled,
    });
    let text = match label {
        super::widgets::Label::None => None,
        super::widgets::Label::Record => rec.str("textTag").map(|t| ui.fonts.tag(t).to_string()),
        super::widgets::Label::Text(t) => Some(t.to_owned()),
    };
    if let (Some(style), Some(t)) = (rec.str("style"), text) {
        let le = super::widgets::label(ui, r, style, TextAlign::Center, &t);
        if let (false, Some(le)) = (enabled, le) {
            ui.commands.entity(le).queue(|mut em: EntityWorldMut| {
                if let Some(mut i) = em.get_mut::<ImageNode>() {
                    i.color = Color::srgba(0.4, 0.4, 0.4, 1.0);
                }
                if let Some(mut t) = em.get_mut::<TextColor>() {
                    t.0 = t.0.mix(&Color::BLACK, 0.6);
                }
            });
        }
    }
    Some((r, e))
}

/// A [`ButtonLook`] quad's bitmap for the mouse state (writes only when it changed).
pub fn set_button_look(l: &ButtonLook, img: &mut ImageNode, enabled: bool, hovered: bool) {
    let want = match (enabled, hovered) {
        (false, _) if l.disabled.is_some() => l.disabled.as_ref(),
        (true, true) if l.over.is_some() => l.over.as_ref(),
        _ => Some(&l.up),
    };
    if let Some(h) = want
        && img.image != *h
    {
        img.image = h.clone();
    }
}
