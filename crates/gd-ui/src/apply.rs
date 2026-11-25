//! The component / augment / transmuter apply mode's look (rules: `gd_game::inventory`
//! `start_apply` / `apply_click` / `answer_apply`; transmuters `gd_game_core::illusion::transmute_check`:
//! `CursorHandlerTransmute` is the same mode, its highlights `CursorHandlerTransmute::GetInventoryHighlights` as below,
//! its questions the `tagTransmute…` ones in the same box).
//!
//! Highlights (`CursorHandlerRelicCombine::GetInventoryHighlights`,
//! `CursorHandlerEnchant::GetInventoryHighlights`): the
//! selected component's cell goes in the second list, the item under the
//! cursor in the first when it can take it (`CanRelicBeUsedOn` /
//! `CanBeUsedOn`), else the third; the grid draws those lists blue, green
//! and red at 0.5 alpha as for the item-move preview (`inventory.rs`
//! `held_overlay`). PORT: every valid target (bags, the shown stash /
//! transfer page and equipped slots, from `Inventory::apply_targets`, the
//! click's own rule) is drawn green with a bright 2 px border (so it shows on
//! rarity backgrounds), not only the hovered one (the decompiled handler
//! lists only the hovered item); an invalid hovered item is red.
//!
//! Cancel (callers, `gd_game_core::inventory::Inventory::cancel_apply`):
//! Esc while no confirmation is up (the shared `ui::esc_dispatcher`;
//! `CursorHandlerEnchant::Escape`, which returns "consumed"), closing the character window (`inventory.rs`) and
//! hiding the stash window ([`stash_closed`], whatever the component's
//! place). A right click is no cancel: on a bag, slot or stash cell it is the
//! mode's click (`ApplyRequest::Click`). A click on the world, either button,
//! ends the mode and does nothing else ([`world_click`]).
//!
//! `GD_APPLY_YES` (hooks.rs): Yes once the confirmation has been up that long
//! (with `GD_INV_CLICKS`, which also clicks the stash window).
//!
//! The confirmation (`DialogManager::AddDialog` with the tag from
//! `apply_check`) is the generic Yes / No box (`gd_ui_kit::confirm::confirm_box`), built in its base look
//! with its hover / press look moved in place ([`box_live`]); it takes every click while up; Esc and the
//! close button answer No (PORT).
//!
//! The drop key (action 32: D, B with keyboard movement) does what a world click does, with no
//! mouse-over-world test and whatever window is open ([`drop_key`]; rules `gd_game::item_drop`).
//!
//! The same box asks the tonics' and faction writs' questions (`tagDevotionResetConfirmation`,
//! `tagAttributeResetConfirmation`, `tagFactionWarrantConfirmation`, `tagFactionMandateConfirmation`)
//! (`gd_game::consumables::use_question`; Yes, No, Esc and the close button send
//! `consumables::UseAnswer`).
//!
//! A world click with an item on the cursor drops it ([`world_click`],
//! `gd_game::item_drop`); for a soulbound / untradeable item the same box
//! asks `tagDestroySoulbound` / `tagDestroyUntradeable` (`Inventory::destroying`;
//! Yes, No, Esc and the close button send `item_drop::DestroyAnswer`).
//!
//! The same box asks the move-reagents button's `tagMoveReagentsConfirm`
//! ([`InvConfirm`], `inventory.rs`); Yes sends `gd_game_core::stash::DepositReagents`.
//! And the auto-sort buttons' `tagCaravanSort` (the window
//! listener `CharacterWindow::OnButtonEvent`: without the option "sortConfirmBypass" a
//! press opens `DialogManager::AddDialog` type 31 with it, remembering the
//! sack (`CaravanWindow.pendingSortSack`); with it the sack is sorted at once); Yes sorts that bag
//! ([`gd_game_core::inventory::SortBag`]).
use super::contains;
use super::window::Rebuild;
use crate::state::inventory::{InvLayout, UiCapture};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::Rect as URect;
use gd_game_core::data::GameData;
use gd_game_core::inventory::ApplyRequest;
use gd_game_core::inventory::Inventory;
use gd_game_core::inventory::ItemLoc;

pub struct ApplyUiPlugin;

impl Plugin for ApplyUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ApplyView>()
            .init_resource::<InvConfirm>()
            .add_systems(
                Update,
                (
                    highlights.after(build).in_set(gd_ui_kit::UiPlaySet).run_if(
                        |inv: Option<Res<Inventory>>, old: Query<(), With<ApplyHighlight>>| {
                            inv.is_some_and(|i| i.applying.is_some()) || !old.is_empty()
                        },
                    ),
                    stash_closed
                        .in_set(gd_ui_kit::UiInputSet)
                        .run_if(resource_exists::<Inventory>),
                    drop_key
                        .in_set(gd_ui_kit::UiPlaySet)
                        .run_if(resource_exists::<Inventory>),
                    world_click
                        .in_set(gd_ui_kit::UiPlaySet)
                        .after(gd_ui_kit::UiInputSet)
                        .after(crate::state::trade::TradeInputSet)
                        .before(crate::state::casting::CastInputStage::Armed)
                        .before(crate::state::casting::CastInputStage::Cast)
                        .before(gd_game_core::player::PlayerSet)
                        .run_if(resource_exists::<Inventory>),
                ),
            );
        // Live: an item is being applied, or a confirmation is up / still built.
        super::window::add_window(
            app,
            build,
            clicks,
            |inv: Option<Res<Inventory>>, reagents: Res<InvConfirm>, view: Res<ApplyView>| {
                inv.is_some_and(|i| {
                    i.applying.is_some()
                        || i.destroying.is_some()
                        || gd_game::consumables::use_question(&i).is_some()
                }) || *reagents != InvConfirm::None
                    || view.yes_no.is_some()
            },
        );
        // The box's hover / press look moves in place (no rebuild on a hover).
        app.add_systems(
            Update,
            box_live
                .run_if(|view: Res<ApplyView>| view.yes_no.is_some())
                .after(build)
                .in_set(gd_ui_kit::UiPlaySet),
        );
    }
}

/// The confirmation's Yes / No rects (physical px).
#[derive(Resource, Default)]
pub struct ApplyView {
    yes_no: Option<super::confirm::ConfirmRects>,
}

/// The character window's question up (module doc).
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq)]
pub enum InvConfirm {
    #[default]
    None,
    /// `tagMoveReagentsConfirm`.
    Reagents,
    /// `tagCaravanSort` for this bag.
    Sort(usize),
}

impl InvConfirm {
    fn tag(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Reagents => Some("tagMoveReagentsConfirm"),
            Self::Sort(_) => Some("tagCaravanSort"),
        }
    }
}

#[derive(Component)]
struct ApplyRoot;

#[derive(Component)]
struct ApplyHighlight;

/// What a highlight marks (module doc): the component's own cell (blue), a valid target (green, bright
/// border), the invalid item under the cursor (red).
#[derive(Clone, Copy, PartialEq)]
enum Mark {
    Source,
    Valid,
    Invalid,
}

/// The box's answer this frame, the same for every question: a left press on Yes / No or the close
/// button (its sound). Esc answers No through the shared dispatcher (`ui::esc_dispatcher`).
fn answer(
    buttons: &ButtonInput<MouseButton>,
    cursor: Option<Vec2>,
    rects: Option<super::confirm::ConfirmRects>,
    data: &GameData,
    sound: &mut MessageWriter<gd_game_core::visuals::PlaySound>,
) -> Option<bool> {
    use super::confirm::Hit;
    if !super::widgets::button_pressed(buttons) {
        return None;
    }
    match rects?.hit(cursor?)? {
        Hit::Yes => Some(true),
        Hit::No => Some(false),
        Hit::Close => {
            if let Some(pak) = super::confirm::close_sound(data) {
                sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
            }
            Some(false)
        }
    }
}

fn pending(inv: &Inventory) -> Option<&'static str> {
    inv.applying.as_ref()?.confirm.map(|c| c.1)
}

/// Builds the box in its base look when the question or the layout changes; [`box_live`] shows the hover.
#[allow(clippy::too_many_arguments)]
fn build(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    inv: Res<Inventory>,
    mut view: ResMut<ApplyView>,
    roots: Query<Entity, With<ApplyRoot>>,
    mut fresh: gd_game_core::session::NewSession,
    mut built: Local<Rebuild<(Option<&'static str>, UVec3)>>,
    reagents: Res<InvConfirm>,
    mut assets: gd_ui_kit::UiAssets,
) {
    if fresh.begun() {
        built.reset();
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let sig = (
        pending(&inv)
            .or(reagents.tag())
            .or(inv.destroying)
            .or(gd_game::consumables::use_question(&inv)),
        // The UI scale too (an applied UI Scale rebuilds).
        size.extend(assets.scale(size).to_bits()),
    );
    if size.x == 0 || !built.changed(sig) {
        return;
    }
    super::window::despawn_all(&mut commands, &roots);
    view.yes_no = None;
    let Some(tag) = sig.0 else { return };
    let s = assets.scale(size);
    let root = gd_ui_kit::spawn_root(
        &mut commands,
        (ApplyRoot, super::hud::KeepsShown),
        super::window::Z_MODAL,
    );
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        s,
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    view.yes_no = super::confirm::confirm_box(&mut ui, tag, (None, false));
}

/// The box's hover / press look in place ([`super::confirm::apply_look`]).
fn box_live(
    window: Query<&Window, With<PrimaryWindow>>,
    view: Res<ApplyView>,
    mouse: Res<ButtonInput<MouseButton>>,
    roots: Query<Entity, With<ApplyRoot>>,
    mut looks: Query<(&super::confirm::ConfirmLook, &mut ImageNode)>,
) {
    let (Ok(win), Some(rects), Ok(root)) = (window.single(), view.yes_no, roots.single()) else {
        return;
    };
    let look = rects.look(super::cursor_px(win), mouse.pressed(MouseButton::Left));
    super::confirm::apply_look(root, look, &mut looks);
}

/// The stash window hiding ends the mode (the caravan window's
/// show / hide `CaravanWindow::SetVisible` in Grim Dawn.exe: on hide it calls the cursor
/// handler's `Cancel` when its `CursorHandler.cancelOnWindowClose` allows, true for both apply handlers).
fn stash_closed(
    trade: Res<crate::state::trade::Trade>,
    inv: Res<Inventory>,
    mut was_open: Local<bool>,
    mut fresh: gd_game_core::session::NewSession,
    mut requests: MessageWriter<ApplyRequest>,
) {
    if fresh.begun() {
        *was_open = false;
    }
    let open = matches!(trade.open, Some(gd_game::trade::TradeOpen::Stash { .. }));
    if *was_open && !open && inv.applying.is_some() {
        requests.write(ApplyRequest::Cancel);
    }
    *was_open = open;
}

/// Key action 32 `DropItem` down sends [`gd_game::item_drop::DropKey`]
/// (`CursorHandlerMgr_OnKeyAction`: key down only, no world-hover test, any window
/// open; the cursor handler's `ActivateWorld` is gd-game's).
fn drop_key(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<gd_ui_kit::keys::KeyBindings>,
    mut out: MessageWriter<gd_game::item_drop::DropKey>,
) {
    if bindings.just_pressed(gd_ui_kit::keys::DROP_ITEM, &keys) {
        out.write(gd_game::item_drop::DropKey);
    }
}

/// A press no window took ends the mode (Grim Dawn.exe: the cursor
/// handler manager's mouse handler `CursorHandlerMgr::HandleMouseEvent` gives a left or right press
/// with a handler up to its `ActivateWorld` and reports it consumed;
/// `CursorHandlerRelicCombine::ActivateWorld`, shared by
/// `CursorHandlerEnchant`, clears the item id and the highlight, so
/// `IsComplete` ends the handler). Runs after every window's input
/// (`UiCapture`) and before the world's (pet orders aside): the press is
/// taken, and so is holding that button (PORT: the port walks while the
/// button is held, the engine starts a walk from the press only).
/// With an item on the cursor the same press goes to
/// `CursorHandlerItemMove::ActivateWorld`: it drops the item
/// ([`gd_game::item_drop::DropHeld`]; rules and the destroy question of a
/// soulbound / untradeable item there, answered in [`clicks`]).
///
/// `GD_INV_CLICKS` (hooks.rs): a click outside the inventory and stash
/// windows is such a press.
#[allow(clippy::too_many_arguments)]
fn world_click(
    buttons: Res<ButtonInput<MouseButton>>,
    inv: Res<Inventory>,
    mut capture: ResMut<UiCapture>,
    mut held: Local<Option<MouseButton>>,
    mut fresh: gd_game_core::session::NewSession,
    mut requests: MessageWriter<ApplyRequest>,
    mut drops: MessageWriter<gd_game::item_drop::DropHeld>,
    #[cfg(feature = "dev")] (hooks, time, layout, trade_layout, mut script): (
        Res<gd_game_core::hooks::Hooks>,
        Res<Time>,
        Res<InvLayout>,
        Res<crate::state::trade::TradeLayout>,
        Local<Option<Vec<gd_game_core::hooks::Click>>>,
    ),
) {
    if fresh.begun() {
        *held = None;
    }
    #[cfg(feature = "dev")]
    let scripted = gd_game_core::hooks::take_due(
        script.get_or_insert_with(|| hooks.items.inv_clicks.clone()),
        time.elapsed_secs(),
    )
    .filter(|c| {
        ![layout.window, trade_layout.window]
            .iter()
            .flatten()
            .any(|r| contains(r, c.at))
    })
    .map(|c| {
        if c.right {
            MouseButton::Right
        } else {
            MouseButton::Left
        }
    });
    #[cfg(not(feature = "dev"))]
    let scripted: Option<MouseButton> = None;
    if let Some(b) = *held {
        if buttons.pressed(b) {
            capture.0 = true;
            return;
        }
        *held = None;
    }
    // The destroy question takes every click while up (`clicks`).
    let dropping = inv.held.is_some() && inv.destroying.is_none();
    if (capture.0 && scripted.is_none())
        || inv.destroying.is_some()
        || gd_game::consumables::use_question(&inv).is_some()
        || !(dropping || inv.applying.as_ref().is_some_and(|a| a.confirm.is_none()))
    {
        return;
    }
    let Some(b) = scripted.or_else(|| {
        [MouseButton::Left, MouseButton::Right]
            .into_iter()
            .find(|b| buttons.just_pressed(*b))
    }) else {
        return;
    };
    if dropping {
        info!("a world click drops the held item");
        drops.write(gd_game::item_drop::DropHeld);
    } else {
        info!("apply: a world click ends the mode");
        requests.write(ApplyRequest::Cancel);
    }
    capture.0 = true;
    *held = Some(b);
}

#[allow(clippy::too_many_arguments)]
fn clicks(
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    view: Res<ApplyView>,
    inv: Res<Inventory>,
    mut requests: MessageWriter<gd_game_core::inventory::ApplyRequest>,
    mut capture: ResMut<UiCapture>,
    data: Res<GameData>,
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
    (mut reagents, mut deposit, mut sort, mut destroy, mut reset): (
        ResMut<InvConfirm>,
        MessageWriter<gd_game_core::stash::DepositReagents>,
        MessageWriter<gd_game_core::inventory::SortBag>,
        MessageWriter<gd_game::item_drop::DestroyAnswer>,
        MessageWriter<gd_game::consumables::UseAnswer>,
    ),
    #[cfg(feature = "dev")] (hooks, time, mut asked): (
        Res<gd_game_core::hooks::Hooks>,
        Res<Time>,
        Local<Option<f32>>,
    ),
) {
    let cursor = window.single().ok().and_then(super::cursor_px);
    // The move-reagents / auto-sort question (module doc).
    if *reagents != InvConfirm::None {
        capture.0 = true;
        let answer = answer(&buttons, cursor, view.yes_no, &data, &mut sound);
        if let Some(yes) = answer {
            let q = std::mem::take(&mut *reagents);
            match q {
                InvConfirm::Reagents if yes => {
                    deposit.write(gd_game_core::stash::DepositReagents);
                }
                InvConfirm::Sort(b) if yes => {
                    sort.write(gd_game_core::inventory::SortBag(b));
                }
                _ => {}
            }
        }
        return;
    }
    // The held item's destroy question (`gd_game::item_drop`) or an item use's (tonic / writ): Yes / No, Esc and
    // the close button No.
    let asking = gd_game::consumables::use_question(&inv).is_some();
    if inv.destroying.is_some() || asking {
        capture.0 = true;
        let answer = answer(&buttons, cursor, view.yes_no, &data, &mut sound);
        if let Some(yes) = answer {
            if inv.destroying.is_some() {
                destroy.write(gd_game::item_drop::DestroyAnswer(yes));
            } else {
                reset.write(gd_game::consumables::UseAnswer(yes));
            }
        }
        return;
    }
    // Escape ends the mode through the shared dispatcher (`ui::esc_dispatcher`;
    // `CursorHandlerEnchant::Escape`: refused while the confirmation is up, else it calls the
    // handler's `CursorHandler::Finish` virtual (taken as its end) and consumes the key).
    if pending(&inv).is_none() {
        #[cfg(feature = "dev")]
        {
            *asked = None;
        }
        return;
    }
    capture.0 = true;
    // `GD_APPLY_YES` (hooks.rs).
    #[cfg(feature = "dev")]
    {
        let since = *asked.get_or_insert(time.elapsed_secs());
        if hooks
            .items
            .apply_yes
            .is_some_and(|s| time.elapsed_secs() - since >= s)
        {
            info!("apply: scripted Yes to {:?}", pending(&inv));
            *asked = None;
            requests.write(ApplyRequest::Answer(true));
            return;
        }
    }
    let answer = answer(&buttons, cursor, view.yes_no, &data, &mut sound);
    if let Some(yes) = answer {
        #[cfg(feature = "dev")]
        {
            *asked = None;
        }
        requests.write(ApplyRequest::Answer(yes));
    }
}

/// The mode's highlights (module doc): what to mark is worked out each frame, the nodes are respawned only
/// when that changes (the hovered item, the targets, the windows' layout).
#[allow(clippy::too_many_arguments)]
fn highlights(
    mut commands: Commands,
    old: Query<Entity, With<ApplyHighlight>>,
    inv: Res<Inventory>,
    reqs: Query<&gd_game_core::inventory::Requirements, With<gd_game_core::player::Player>>,
    window: Query<&Window, With<PrimaryWindow>>,
    layout: Res<InvLayout>,
    panels: Res<super::UiPanels>,
    (trade, trade_layout, stash): (
        Res<crate::state::trade::Trade>,
        Res<crate::state::trade::TradeLayout>,
        // Option: created with the player (`gd_game::stash::init_stash`).
        Option<Res<gd_game_core::stash::Stash>>,
    ),
    (shared, choice, save_dir): (
        Res<gd_game_core::stash::SharedFiles>,
        Res<gd_game_core::session::Choice>,
        Res<gd_game_core::session::SaveDir>,
    ),
    mut shown: Local<(Vec<(URect, Mark)>, f32)>,
) {
    let mut marks: Vec<(URect, Mark)> = Vec::new();
    let k = window.single().map_or(1.0, |w| 1.0 / w.scale_factor());
    mark(
        &mut marks,
        (
            &*inv,
            &reqs.single().copied().unwrap_or_default(),
            shared.accepts(&choice, &save_dir, gd_formats::save::Shared::Transfer),
        ),
        window.single().ok().and_then(super::cursor_px),
        &layout,
        &panels,
        (&*trade, &*trade_layout, stash.as_deref()),
    );
    // The nodes on screen are these marks' (a new session despawned them: drawn again).
    if shown.0 == marks && shown.1 == k && old.is_empty() == marks.is_empty() {
        return;
    }
    for e in &old {
        commands.entity(e).try_despawn();
    }
    // PORT: valid targets get the engine's 0.5 green plus a bright border, so
    // they stay visible on the rarity backgrounds.
    let edge = Color::srgba(0.4, 1.0, 0.4, 1.0);
    for (r, m) in &marks {
        let (c, border) = match m {
            Mark::Source => (Color::srgba(0.0, 0.0, 1.0, 0.5), None),
            Mark::Valid => (Color::srgba(0.0, 1.0, 0.0, 0.5), Some(edge)),
            Mark::Invalid => (Color::srgba(1.0, 0.0, 0.0, 0.5), None),
        };
        let mut e = commands.spawn((
            ApplyHighlight,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(r.x * k),
                top: Val::Px(r.y * k),
                width: Val::Px(r.w * k),
                height: Val::Px(r.h * k),
                border: UiRect::all(Val::Px(if border.is_some() { 2.0 } else { 0.0 })),
                ..default()
            },
            BackgroundColor(c),
            GlobalZIndex(90),
        ));
        if let Some(b) = border {
            e.insert(BorderColor::all(b));
        }
    }
    *shown = (marks, k);
}

/// Lists the cells and slots to mark (physical px), in draw order; nothing while no item is being applied or
/// its confirmation is up.
fn mark(
    marks: &mut Vec<(URect, Mark)>,
    (inv, req, transfer_open): (&Inventory, &gd_game_core::inventory::Requirements, bool),
    cursor: Option<Vec2>,
    layout: &InvLayout,
    panels: &super::UiPanels,
    (trade, trade_layout, stash): (
        &crate::state::trade::Trade,
        &crate::state::trade::TradeLayout,
        Option<&gd_game_core::stash::Stash>,
    ),
) {
    let Some(a) = inv.applying.as_ref().filter(|a| a.confirm.is_none()) else {
        return;
    };
    let valid = inv.apply_targets(stash, req, transfer_open);
    // The shown grids: the bags (character window) and the stash / transfer page.
    let mut grids: Vec<(
        URect,
        f32,
        &gd_formats::inventory::Sack<gd_formats::save::item::Item>,
        Box<dyn Fn(u32, u32) -> ItemLoc>,
    )> = Vec::new();
    if panels.character {
        for (b, r, cell) in &layout.grids {
            if let Some(bag) = inv.bags.get(*b) {
                let b = *b;
                grids.push((
                    *r,
                    *cell,
                    bag,
                    Box::new(move |x, y| ItemLoc::Bag { bag: b, x, y }),
                ));
            }
        }
    }
    if let Some(st) = stash
        && matches!(trade.open, Some(gd_game::trade::TradeOpen::Stash { .. }))
        && let Some((r, cell)) = trade_layout.grid
    {
        let pages = st.view(trade.transfer);
        let tab = trade.tab.min(pages.len().saturating_sub(1));
        if let Some(page) = pages.get(tab) {
            let transfer = trade.transfer;
            grids.push((
                r,
                cell,
                page,
                Box::new(move |x, y| ItemLoc::Stash {
                    transfer,
                    page: tab,
                    x,
                    y,
                }),
            ));
        }
    }
    for (r, cell, sack, loc) in &grids {
        let rect = |q: &gd_formats::inventory::Placed<gd_formats::save::item::Item>| {
            URect::new(
                r.x + q.x as f32 * cell,
                r.y + q.y as f32 * cell,
                q.size.w as f32 * cell,
                q.size.h as f32 * cell,
            )
        };
        for q in &sack.items {
            let l = loc(q.x, q.y);
            if l == a.src {
                marks.push((rect(q), Mark::Source));
            } else if valid.contains(&l) {
                marks.push((rect(q), Mark::Valid));
            }
        }
        let Some(p) = cursor.filter(|p| contains(r, *p)) else {
            continue;
        };
        let Some(q) = sack.item_at(((p.x - r.x) / cell) as u32, ((p.y - r.y) / cell) as u32) else {
            continue;
        };
        let l = loc(q.x, q.y);
        if l != a.src && !valid.contains(&l) {
            marks.push((rect(q), Mark::Invalid));
        }
    }
    if panels.character {
        for &(slot, r) in &layout.slots {
            let i = gd_game_core::inventory::slot_index(slot);
            if valid.contains(&ItemLoc::Equip(i)) {
                marks.push((r, Mark::Valid));
            } else if inv.equipped.get(i).is_some_and(|e| e.is_some())
                && cursor.is_some_and(|p| contains(&r, p))
            {
                marks.push((r, Mark::Invalid));
            }
        }
    }
}

/// True while a modal Yes/No (or Ok) question is up over the inventory: the apply confirmation and the
/// reagent / sort question (`ui/apply.rs`), the destroy-item and tonic/writ questions (`item_drop`,
/// `gd_game::consumables::use_question`), the trade prompt and the dismantle warning. The windows under it
/// take no click, wheel, hover or key while it is up.
pub fn question_up(
    inv: &Inventory,
    trade: &crate::state::trade::Trade,
    confirm: &InvConfirm,
) -> bool {
    pending(inv).is_some()
        || *confirm != InvConfirm::None
        || inv.destroying.is_some()
        || gd_game::consumables::use_question(inv).is_some()
        || trade.prompt.is_some()
        || trade.confirm
}
