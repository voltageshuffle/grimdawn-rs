//! The HUD's hot slot pick list (`hudSlotSelectWindow`): a right click on a
//! hotbar slot (keys 1–0 of the shown row, left / right mouse) lists what
//! may be bound to it (`gd_game::hotbar::choices`, rules and engine
//! references there), a pick sends [`BindSlot`], plays `skillSelectSound`
//! and closes the list; a left click outside it or Esc closes it. Drawn by
//! `slot_select` above the slot; the hovered pick's skill tooltip is shown
//! beside the list. The list takes the mouse over itself (`UiCapture`, the
//! wheel). `Grim Dawn.exe` `HotSlotButton::HandleMouseEvent` / `HotSlotPickList::HandleMouseEvent` /
//! `HotSlotPickList::Fill` (see `gd_game::hotbar`).
//!
//! Pick icons (the options' `GetBitmapNameUp`): a skill's button icon; a
//! potion's combo bitmap (`gd_sim::potions::potion_bitmap`), else the
//! record's `defaultHealthBitmap` / `defaultManaBitmap`; a scroll's
//! `bitmapButtonUp`. Names: the skill's `skillDisplayName`, the item's
//! `description`.
//! A left press on a hot slot (keys or mouse slots: `ActivateHotSlot`, and
//! `ActivatePrimarySlot` / `ActivateSecondarySlot` for the mouse slots, both
//! from the mouse) sends [`ActivateHotSlot`]: the option is used at once or the slot is armed.
//! A right press and one of the HUD windows being shown ([`DisarmingWindows`]) send [`DisarmHotSlot`];
//! Esc sends it through the shared dispatcher (`ui::esc_dispatcher`).
//! A scroll pick's rollover is the tooltip of the first carried item of its record
//! (`Grim Dawn.exe` `HotSlotButton::HandleMouseEvent` / `HotSlotPickList::HandleMouseEvent` / `HotSlotPickList::Fill`).
//! PORT: the slot's own press / hover look while the list is open
//! is not drawn. PORT: switching the shown hotbar row closes the list (its slot is no longer shown);
//! while the game menu / Options is up ([`gd_game_core::pause::GamePause::menu`]) the list shows no hover and
//! takes no wheel; while a modal box's blocker (`confirm.rs`, `Z_MODAL` and above) covers a hot slot,
//! the slot neither opens its list nor activates.
//! Hooks (hooks.rs): `GD_HOTBAR_PICK=<slot>` opens a slot's list (caster slot index:
//! 0–9 keys, 10 left, 12 right mouse), `GD_HOTBAR_HOVER=<n>` hovers its
//! n-th pick, `GD_HOTBAR_CHOOSE=<n>` picks it (after 3 s),
//! `GD_HOTBAR_PAGE=<n>` pages it forward n times.
use super::contains;
use super::slot_select::{Pick, Place};
use crate::state::inventory::UiCapture;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::Rect as URect;
use gd_game::hotbar::{
    ActivateHotSlot, ArmedSlot, BindSlot, DisarmHotSlot, OptionKind, option_kind,
};
use gd_game_core::casting::{Caster, SLOT_LEFT, SLOT_RIGHT, SLOT_ROW2};
use gd_game_core::data::GameData;

pub struct HotbarPickPlugin;

impl Plugin for HotbarPickPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PickView>()
            .init_resource::<PickLayout>()
            .init_resource::<OverHotSlot>()
            .add_systems(
                Update,
                (
                    clicks
                        .in_set(gd_ui_kit::UiInputSet)
                        .before(crate::state::casting::CastInputStage::Cast)
                        .before(crate::state::casting::CastInputStage::Cursor)
                        .before(crate::state::casting::CastInputStage::Armed),
                    draw.in_set(gd_ui_kit::UiPlaySet),
                    tooltip.in_set(gd_ui_kit::UiPlaySet),
                    disarm.in_set(gd_ui_kit::UiPlaySet),
                )
                    .chain(),
            );
    }
}

/// UI state: the slot whose list is open and its options (taken on open).
#[derive(Resource, Default, Clone, PartialEq)]
pub struct PickView {
    pub slot: Option<usize>,
    pub options: Vec<Option<String>>,
    /// The first shown column (`slot_select` paging).
    pub first: usize,
}

/// The cursor is over a hot slot button this frame (the HUD takes the mouse
/// events there: `casting::world_cursor` records no world point).
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq)]
pub struct OverHotSlot(pub bool);

/// The drawn list (physical px).
#[derive(Resource, Default, Clone, PartialEq)]
pub struct PickLayout {
    pub list: Option<URect>,
    pub picks: Vec<URect>,
    pub paged: super::slot_select::Paged,
    /// The hovered scroll pick's item record (its rollover is that item's tooltip, `tooltip.rs`).
    pub scroll: Option<String>,
}

/// The list's root; its hover tints are its direct children (`potions::live` skips them).
#[derive(Component)]
pub(super) struct PickRoot;

const RECORD_FIELD: &str = "hudSlotSelectWindow";

/// The HUD's hot slots that open the list: (caster slot, `HUD master`
/// field). The potion and evade buttons do not (module docs).
fn slots(row: usize) -> impl Iterator<Item = (usize, &'static str)> {
    (1..=10)
        .map(move |i| (i - 1 + row * SLOT_ROW2, super::hud::HOT_SLOTS[i - 1]))
        .chain([
            (SLOT_LEFT, "hudPriSkillButton"),
            (SLOT_RIGHT, "hudSecSkillButton"),
        ])
}

/// A hotbar option's icon (`GetBitmapNameUp`, module docs); `book` for the
/// potions' combo bitmaps.
pub fn option_icon(data: &GameData, rec: &str, book: &gd_sim::skills::SkillBook) -> Option<String> {
    match option_kind(&data.db, rec) {
        OptionKind::Scroll => data.db.get(rec).and_then(|r| {
            r.str("bitmapButtonUp")
                .or(r.str("bitmap"))
                .map(str::to_owned)
        }),
        OptionKind::HealthPotion | OptionKind::EnergyPotion => {
            let tree = gd_sim::potions::trees(&data.db)
                .into_iter()
                .find(|t| t.potion.eq_ignore_ascii_case(rec));
            let combo = tree
                .and_then(|t| gd_sim::potions::potion_bitmap(&data.db, book, &t))
                .map(|b| b.texture)
                .filter(|b| !b.is_empty());
            let key = if option_kind(&data.db, rec) == OptionKind::HealthPotion {
                "defaultHealthBitmap"
            } else {
                "defaultManaBitmap"
            };
            combo.or_else(|| {
                data.db
                    .get(gd_ui_kit::MASTER)
                    .and_then(|m| m.str(RECORD_FIELD).and_then(|r| data.db.get(r)))
                    .and_then(|w| w.str(key).map(str::to_owned))
            })
        }
        _ => super::potions::skill_icon(data, rec, 1),
    }
}

#[allow(clippy::too_many_arguments)]
fn clicks(
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    values: Res<super::HudValues>,
    layout: Res<PickLayout>,
    mut view: ResMut<PickView>,
    (mut capture, mut over_slot): (ResMut<UiCapture>, ResMut<OverHotSlot>),
    hud_layout: Res<super::hud::HudLayout>,
    (mut select_cache, mut fresh, mut last_row): (
        Local<Option<Option<String>>>,
        gd_game_core::session::NewSession,
        Local<Option<usize>>,
    ),
    players: Query<
        (
            &gd_game_core::player::Loadout,
            &gd_game_core::player::EffectiveSkills,
            &Caster,
        ),
        With<gd_game_core::player::Player>,
    >,
    inv: Res<gd_game_core::inventory::Inventory>,
    (mut out, mut sound, mut arm): (
        MessageWriter<BindSlot>,
        MessageWriter<gd_game_core::visuals::PlaySound>,
        (MessageWriter<ActivateHotSlot>, MessageWriter<DisarmHotSlot>),
    ),
    (mut wheel, pause, sounds, zs): (
        MessageReader<bevy::input::mouse::MouseWheel>,
        Res<gd_game_core::pause::GamePause>,
        Query<(&super::widgets::ButtonSound, &ChildOf, &InheritedVisibility)>,
        Query<&GlobalZIndex>,
    ),
    time: Res<Time>,
    (mut hook, hooks): (
        Local<Option<(Option<usize>, Option<usize>, Option<usize>)>>,
        Res<gd_game_core::hooks::Hooks>,
    ),
) {
    over_slot.0 = false;
    // The select sound is the session's HUD record's (a mod may replace it).
    if fresh.begun() {
        *select_cache = None;
        *last_row = None;
    }
    // The shown row switched: the open list's slot is hidden, so the list closes (module docs).
    if last_row
        .replace(values.hotbar_row)
        .is_some_and(|r| r != values.hotbar_row)
    {
        *view = PickView::default();
    }
    let Ok((loadout, effective, caster)) = players.single() else {
        return;
    };
    let open = |view: &mut PickView, slot: usize| {
        view.options =
            gd_game::hotbar::choices(&data.db, loadout, effective, caster, Some(&*inv), slot);
        view.slot = Some(slot);
        view.first = 0;
    };
    // Hooks (module docs).
    let hook = hook.get_or_insert_with(|| {
        (
            hooks.ui.hotbar_pick,
            hooks.ui.hotbar_choose,
            hooks.ui.hotbar_page,
        )
    });
    if let Some(slot) = hook.0.take() {
        open(&mut view, slot);
    }
    if view.slot.is_some()
        && layout.list.is_some()
        && let Some(n) = hook.2.take()
    {
        view.first = (0..n)
            .fold(layout.paged, |pg, _| super::slot_select::Paged {
                first: pg.step(true),
                ..pg
            })
            .first;
    }
    let mut picked: Option<usize> = None;
    if time.elapsed_secs() > 3.0
        && view.slot.is_some()
        && let Some(n) = hook.1.take()
    {
        picked = Some(n);
    }
    // A pick binds the option, plays `skillSelectSound` and closes the list;
    // a page arrow plays it too (`HotSlotPickList::HandleMouseEvent`).
    let select_sound = select_cache
        .get_or_insert_with(|| {
            data.db
                .get(gd_ui_kit::MASTER)
                .and_then(|m| m.str(RECORD_FIELD).and_then(|r| data.db.get(r)))
                .and_then(|w| w.str("skillSelectSound").map(str::to_owned))
        })
        .clone();
    let mut choose = |view: &mut PickView, n: usize| {
        if let (Some(slot), Some(option)) = (view.slot, view.options.get(n).cloned()) {
            out.write(BindSlot { slot, option });
            if let Some(pak) = select_sound.clone() {
                sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
            }
        }
        *view = PickView::default();
    };
    if let Some(n) = picked {
        choose(&mut view, n);
        return;
    }
    // The game menu / Options holds the mouse: no wheel, no press (module docs).
    if pause.menu {
        wheel.clear();
        return;
    }
    // Esc closes the list through the shared dispatcher (`ui::esc_dispatcher`).
    let Ok(win) = window.single() else { return };
    let Some(p) = super::cursor_px(&hooks, win) else {
        return;
    };
    let over_list = layout.list.is_some_and(|l| contains(&l, p));
    if over_list {
        capture.0 = true;
        wheel.clear();
    }
    if over_list
        && (buttons.just_pressed(MouseButton::Left) || buttons.just_pressed(MouseButton::Right))
    {
        let arrow = [(layout.paged.prev, false), (layout.paged.next, true)]
            .into_iter()
            .find(|(r, _)| r.is_some_and(|r| contains(&r, p)));
        if let Some((_, next)) = arrow {
            view.first = layout.paged.step(next);
            if let Some(pak) = select_sound {
                sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
            }
            return;
        }
        picked = layout.picks.iter().position(|r| contains(r, p));
        if picked.is_none() {
            return;
        }
    }
    if let Some(n) = picked {
        choose(&mut view, n);
        return;
    }
    if over_list {
        return;
    }
    // A right press on a hot slot opens its list; a left press elsewhere
    // closes it.
    // The slot rects are the built HUD's (`HudLayout`), not re-derived from the database each frame.
    let hit =
        slots(values.hotbar_row).find(|(_, f)| hud_layout.rect(f).is_some_and(|r| contains(&r, p)));
    // A modal box's blocker over the slot keeps it from the press (module docs).
    let blocked = sounds.iter().any(|(b, c, v)| {
        v.get()
            && contains(&b.rect, p)
            && zs
                .get(c.parent())
                .is_ok_and(|z| z.0 >= super::window::Z_MODAL)
    });
    over_slot.0 = hit.is_some() && !blocked;
    let hit = hit.filter(|_| !blocked);
    if let Some((slot, _)) = hit
        && buttons.pressed(MouseButton::Right)
    {
        // The press takes the button (no right-button skill underneath);
        // it drops the armed slot before the list opens.
        capture.0 = true;
        if buttons.just_pressed(MouseButton::Right) {
            arm.1.write(DisarmHotSlot);
            open(&mut view, slot);
        }
    } else if let Some((slot, _)) = hit
        && buttons.pressed(MouseButton::Left)
    {
        // A left press uses the slot's option from the button: at once, or
        // armed for a target (`gd_game::hotbar::ActivateHotSlot`).
        capture.0 = true;
        if buttons.just_pressed(MouseButton::Left) {
            *view = PickView::default();
            arm.0.write(ActivateHotSlot { slot });
        }
    } else if buttons.just_pressed(MouseButton::Left) && view.slot.is_some() {
        *view = PickView::default();
    }
}

/// The HUD windows whose showing drops the armed slot, `Grim Dawn.exe`
/// `UIPlayerHud::OnWindowEvent` (the HUD's window listener; event 1 = shown → `UIPlayerHud::DeactivateHotSlot` =
/// `PlayerHotSlotCtrl::DeactivateActiveSlot`, whose only other caller is the hot
/// slot button's mouse handler), by HUD member and master field:
/// the character window (`UIPlayerHud.characterWindow`, `hudCharacterWindow`: inventory and sheet),
/// quests (`UIPlayerHud.questWindow`), vendor (`UIPlayerHud.marketWindow`, `hudMarketWindow`), faction vendor
/// (`UIPlayerHud.factionMarketWindow`), the two `hudEnchanterWindow` windows (`UIPlayerHud.enchanterWindow` /
/// `UIPlayerHud.inventorWindow`: the blacksmith's crafting and the Inventor), skills (`UIPlayerHud.skillsWindow`), map
/// (`UIPlayerHud.mapWindow`, `hudMiniMapWindow`), party (`UIPlayerHud.partyWindow`), factions
/// (`UIPlayerHud.factionWindow`), caravan
/// (`UIPlayerHud.caravanWindow`: stash), shrine (`UIPlayerHud.shrineWindow` / `UIPlayerHud.shrineWindow2`,
/// `hudShrineWindow`), devotion (`UIPlayerHud.devotionWindow`), transmuter (`UIPlayerHud.transmuterWindow`: the
/// Illusionist), altar (`UIPlayerHud.altarWindow`), potions (`UIPlayerHud.potionsWindow`), item ascension
/// (`UIPlayerHud.ascensionWindow`), loot filter (`UIPlayerHud.lootFilterWindow`), the game menu
/// (`UIPlayerHud.gameMenuWindow`, `hudExitWindow`) and the options window
/// (`UIPlayerHud.optionsWindow`, built by the HUD constructor `UIPlayerHud::UIPlayerHud` with
/// `OptionsWindow::OptionsWindow`,
/// vtable `OptionsWindow::vftable`, whose open `OptionsWindow::Open` creates the options menu
/// `OptionsWindow::Load`). Not: chat (`UIPlayerHud.chatWindow`), achievements (`UIPlayerHud.achievementsWindow`),
/// and windows outside the listener (the conversation, trade, stack, hot slot pick list, pet menu). The riftgate map
/// is the map window's world-map mode: the riftgate widget is the map window's
/// member (`UIWorldMap.riftgateWidget`, map window constructor `UIWorldMap::UIWorldMap`), and its
/// `SetVisible` `UIWorldMap::Open` shows the riftgate widget, then the base
/// `UIWindow::SetVisible` tells the listener; so the map and the riftgate map are one
/// entry (switching between them while shown sends nothing). Each window's own
/// showing counts (`UIWindow::SetVisible` tells the listener only when the shown state
/// changes; another window opening while one is open disarms too).
#[derive(bevy::ecs::system::SystemParam)]
struct DisarmingWindows<'w> {
    panels: Res<'w, super::UiPanels>,
    map: Res<'w, crate::state::map::MapWindow>,
    rift: Res<'w, gd_game::riftgates::RiftgateMap>,
    journal: Res<'w, super::quests::QuestJournal>,
    potions: Res<'w, gd_game::cauldron::PotionWindow>,
    shrine: Res<'w, gd_game::devotion::ShrineWindow>,
    altar: Res<'w, crate::state::ascendant::AltarMenu>,
    trade: Res<'w, crate::state::trade::Trade>,
    loot: Res<'w, super::lootfilter::LootFilterView>,
    menu: Res<'w, super::gamemenu::GameMenu>,
    options: Res<'w, crate::state::options::OptionsWindow>,
}

impl DisarmingWindows<'_> {
    /// Which of them are shown (one entry per engine window; the trade
    /// windows by kind).
    fn shown(&self) -> [bool; 16] {
        use gd_game::trade::TradeOpen as T;
        let p = &self.panels;
        let t = self.trade.open.as_ref();
        let is = |f: fn(&T) -> bool| t.is_some_and(f);
        [
            p.character,
            p.skills,
            p.devotion,
            // One window (see above).
            self.map.open || self.rift.open,
            self.journal.open,
            self.potions.open,
            self.shrine.shrine.is_some(),
            self.altar.open,
            is(|t| matches!(t, T::Vendor { .. })),
            is(|t| matches!(t, T::Stash { .. })),
            is(|t| matches!(t, T::Crafter { .. } | T::Inventor { .. })),
            is(|t| matches!(t, T::Ascension { .. })),
            is(|t| matches!(t, T::Illusionist { .. })),
            self.loot.open,
            self.menu.open,
            self.options.open,
        ]
    }
}

/// A [`DisarmingWindows`] window being shown drops the armed slot (`gd_game::hotbar` module docs);
/// Esc drops it through the shared dispatcher (`ui::esc_dispatcher`).
fn disarm(
    armed: Res<ArmedSlot>,
    windows: DisarmingWindows,
    mut was: Local<[bool; 16]>,
    mut out: MessageWriter<DisarmHotSlot>,
) {
    let now = windows.shown();
    let shown = now.iter().zip(was.iter()).any(|(n, w)| *n && !*w);
    *was = now;
    if armed.0.is_some() && shown {
        out.write(DisarmHotSlot);
    }
}

/// Builds the list per (screen, slot, options, page); the hovered row's and page arrow's tints
/// ([`super::slot_select::PickHover`], [`super::slot_select::ArrowHover`]) are shown in place.
#[allow(clippy::too_many_arguments)]
fn draw(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    values: Res<super::HudValues>,
    view: Res<PickView>,
    roots: Query<Entity, With<PickRoot>>,
    mut layout: ResMut<PickLayout>,
    mut assets: gd_ui_kit::UiAssets,
    mut fresh: gd_game_core::session::NewSession,
    mut built: Local<Option<(UVec2, Option<usize>, Vec<Option<String>>, usize)>>,
    mut rows: Query<(&super::slot_select::PickHover, &ChildOf, &mut Visibility)>,
    mut arrows: Query<
        (&super::slot_select::ArrowHover, &mut Visibility),
        Without<super::slot_select::PickHover>,
    >,
    pause: Res<gd_game_core::pause::GamePause>,
) {
    if fresh.begun() {
        *built = None;
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    // Compared by reference: the options are cloned only when they changed.
    let same = built.as_ref().is_some_and(|(s, slot, options, first)| {
        *s == size && *slot == view.slot && *options == view.options && *first == view.first
    });
    if size.x != 0 && !same {
        *built = Some((size, view.slot, view.options.clone(), view.first));
        build(
            &mut commands,
            win,
            size,
            &data,
            &values,
            &view,
            &roots,
            &mut layout,
            &mut assets,
        );
        // The tints spawn hidden; the next frame shows the hovered ones.
        return;
    }
    // The game menu / Options holds the mouse (module docs).
    let p = super::cursor_px(&hooks, win).filter(|_| !pause.menu);
    let hover = hooks
        .ui
        .hotbar_hover
        .or_else(|| p.and_then(|p| layout.picks.iter().position(|r| contains(r, p))));
    let arrow = p
        .and_then(|p| {
            [(layout.paged.prev, false), (layout.paged.next, true)]
                .into_iter()
                .find(|(r, _)| r.is_some_and(|r| contains(&r, p)))
        })
        .map(|(_, n)| n);
    let shown = |on: bool| {
        if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        }
    };
    for (h, parent, mut vis) in &mut rows {
        if roots.contains(parent.parent()) {
            vis.set_if_neq(shown(hover == Some(h.0)));
        }
    }
    for (a, mut vis) in &mut arrows {
        vis.set_if_neq(shown(arrow == Some(a.0)));
    }
}

/// The list for the open slot ([`draw`]'s rebuild).
#[allow(clippy::too_many_arguments)]
fn build(
    commands: &mut Commands,
    win: &Window,
    size: UVec2,
    data: &GameData,
    values: &super::HudValues,
    view: &PickView,
    roots: &Query<Entity, With<PickRoot>>,
    layout: &mut PickLayout,
    assets: &mut gd_ui_kit::UiAssets,
) {
    super::window::despawn_all(commands, roots);
    *layout = PickLayout::default();
    let Some(slot) = view.slot else { return };
    let s = assets.scale(size);
    let book = &values.skills;
    let root = gd_ui_kit::spawn_root(commands, PickRoot, super::window::Z_MODAL);
    let mut ui = assets.ui(
        commands,
        data,
        root,
        s,
        1.0 / win.scale_factor(),
        size.as_vec2(),
    );
    let Some(sel) = ui
        .record(ui.hud_master())
        .and_then(|m| m.str(RECORD_FIELD).map(str::to_owned))
        .and_then(|r| ui.record(&r))
    else {
        return;
    };
    let Some(anchor) = slots(values.hotbar_row)
        .into_iter()
        .find(|(n, _)| *n == slot)
        .and_then(|(_, f)| super::hud::slot_rect(&ui, f))
    else {
        return;
    };
    let picks: Vec<Pick> = view
        .options
        .iter()
        .map(|o| match o {
            None => super::slot_select::empty_pick(&mut ui, &sel),
            Some(rec) => {
                let tag = match option_kind(&data.db, rec) {
                    OptionKind::Scroll => data
                        .db
                        .get(rec)
                        .and_then(|r| r.str("description").map(str::to_owned)),
                    _ => gd_sim::skills::display_record(&data.db, rec)
                        .and_then(|r| r.str("skillDisplayName").map(str::to_owned)),
                };
                Pick {
                    icon: option_icon(data, rec, book),
                    name: tag
                        .map(|t| ui.fonts.tag(&t).to_string())
                        .unwrap_or_default(),
                    selected: false,
                    look: Default::default(),
                }
            }
        })
        .collect();
    let (lr, rows, paged) = super::slot_select::draw_paged(
        &mut ui,
        &sel,
        &picks,
        Place::Above(anchor),
        None,
        Some(super::slot_select::Paging { first: view.first }),
    );
    // The list blocks the HUD buttons (and their sounds) behind it.
    ui.sound_blocker(lr);
    layout.list = Some(lr);
    layout.picks = rows;
    layout.paged = paged;
}

/// The hovered pick's skill tooltip via the shared [`super::tip::TipCanvas`]
/// ([`super::tip::TipOwner::HotbarPick`], the style's delay), not part of the rebuilt list; and the
/// hovered scroll pick's item record (`layout.scroll`, `tooltip.rs` shows its item tooltip,
/// `HotSlotOptionScroll::GetRolloverText`).
fn tooltip(
    mut fresh: gd_game_core::session::NewSession,
    hooks: Res<gd_game_core::hooks::Hooks>,
    window: Query<&Window, With<PrimaryWindow>>,
    (values, view, mut layout): (Res<super::HudValues>, Res<PickView>, ResMut<PickLayout>),
    data: Res<GameData>,
    mut canvas: super::tip::TipCanvas,
    mut shown: Local<Option<String>>,
) {
    if fresh.begun() {
        *shown = None;
    }
    let Ok(win) = window.single() else { return };
    let p = super::cursor_px(&hooks, win);
    let hover = hooks
        .ui
        .hotbar_hover
        .or_else(|| p.and_then(|p| layout.picks.iter().position(|r| contains(r, p))));
    let hovered = hover.and_then(|i| view.options.get(i).cloned().flatten());
    layout.scroll = hovered
        .clone()
        .filter(|o| option_kind(&data.db, o) == OptionKind::Scroll);
    let skill = hovered.filter(|o| option_kind(&data.db, o) != OptionKind::Scroll);
    if *shown == skill {
        return;
    }
    *shown = skill.clone();
    let Some(sk) = skill else {
        canvas.clear(super::tip::TipOwner::HotbarPick);
        return;
    };
    let ctx = gd_sim::skill_tooltip::SkillTipCtx {
        db: &data.db,
        trees: &data.skill_trees(),
        tags: &canvas.assets.fonts.tags,
        book: &values.skills,
        player: None,
        pet_bonuses: &[],
    };
    // The pick list's options use the simple text (`SkillPickList::BuildLines` sets
    // `HotSlotOptionSkill::SetSimpleText`).
    let lines = gd_sim::skill_tooltip::pick_list_lines(&ctx, &sk);
    if lines.is_empty() {
        canvas.clear(super::tip::TipOwner::HotbarPick);
        return;
    }
    let row = hover.and_then(|i| layout.picks.get(i)).copied();
    let list = layout.list;
    canvas.show(
        super::tip::TipOwner::HotbarPick,
        &lines,
        super::tip::Blank::DefaultLine,
        |w, h, screen, gap| {
            if let (Some(row), Some(list)) = (row, list) {
                (
                    (list.x + list.w + gap).min((screen.x - w).max(0.0)),
                    row.y.min((screen.y - h).max(0.0)),
                )
            } else {
                (0.0, 0.0)
            }
        },
    );
}
