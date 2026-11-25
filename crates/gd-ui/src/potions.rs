//! The potions window ("Cauldron", `records/ui/cauldron/potions_table.dbr`,
//! the HUD master table's `hudPotionsWindow`): per potion (tabs), its three
//! containers, its two modifier slots and the potion's description; a click
//! on a slot opens the list of the potion's unlocked modifiers. It shows
//! [`PotionWindow`] and the player's skill book (`HudValues::skills`) and
//! sends [`PotionRequest`]s; the rules are in `gd_game::cauldron` and
//! `gd_sim::potions`. Clicking a cauldron (`NpcCauldron`) is the shared interact order
//! (`gd_game::player_orders`, the hover pick): gd-game walks beside it, and in reach the window opens.
//!
//! Layout, `Grim Dawn.exe` (loader `PotionsWindow::Load`, open
//! `PotionsWindow::Open`, click `PotionsWindow::OnButtonPress`, update `PotionsWindow::Update`, HUD
//! placement in `UIPlayerHud::Update`):
//! - the record's window (346 × 785, `Left` / `Center`) sits left of the
//!   inventory window, the two centred together:
//!   `x = W/2 − (inventory w + window w)/2`, the inventory at `x + w`;
//! - `potionsBaseBitmap`, `potionsWindowTitle` (`tagCauldron`),
//!   `potionsCloseButton`; the tab buttons `potions1Button` /
//!   `potions2Button` (health / energy, the shown one pressed) with their
//!   `potionBitmapName` at `skillOffset`; the page's container buttons
//!   `potionsContainer1..3` (one per container, the chosen one pressed, its
//!   skill icon at level max(1, unlock level) at `skillOffset`) and modifier
//!   buttons `potionsModifier1..2` (the slot's skill icon), disabled while
//!   no container is chosen; the description in `potionsModifiersBox` inside
//!   `potionsModifiersScrollWindow`;
//! - a button's skill icon (`SkillButton::GetIcon`): the skill's
//!   `skillUpAlternateBitmapName`, else `skillUpBitmapName`, of that level;
//! - the modifier list (`skillSelect`, a `SlotSelectWindow`) opens at the
//!   clicked slot's bottom-left corner, listing the potion's unlocked
//!   modifiers (no `selectedBorder`: the window loads it into `PotionsWindow.selectedBorder`
//!   (loader `PotionsWindow::Load`, ctor `PotionsWindow::PotionsWindow`) and no method of its vtable reads it).
//!
//! - the tab buttons: the record's `potionBitmapName` until the window
//!   opens; on open (`PotionsWindow::Open`) both potions run their `Update`
//!   (`UpdatePotionBitmap`) and each tab is set to its potion skill, so it
//!   shows the potion's combo bitmap (`gd_sim::potions::potion_bitmap`), else
//!   the skill's icon at level 1 (`SkillButton::GetIcon`); the tabs are not
//!   refreshed while the window stays open.
//!
//! - the modifier list: a `SlotSelectWindow` (`slot_select`); a click
//!   plays `skillSelectSound` and closes the list;
//! - the description scrolls in `potionsModifiersScrollWindow` (the
//!   journal's scroll window class, so the shared `window::scrollbar_live` / `Bar`): the
//!   wheel over the scroll window scrolls int(120 × 0.2 × s) px a notch
//!   (`UIScrollWindow::HandleScrollInput`); arrows 20 × s px, repeating after 300 ms. Scrolling and hovering move the
//!   pane, the bar thumb and the button frames in place ([`live`]); the skill rollovers go through
//!   `tip::TipCanvas` with the style's delay ([`tooltip`]), not into the window build.
//!
//! PORT: a potion whose choice matches no combo shows its skill icon (the
//! engine keeps the last matched bitmap); a locked container's icon is grey;
//! the picks' selected (0.4, 0.6, 1, 0.15) / disabled looks were not traced; the
//! description is the potion's, container's and slotted modifiers' tooltip
//! lines (its text source not decompiled) and returns to the top on a page
//! switch; a click on the scroll track pages (`window::Bar::press`); a secondary-skill modifier (Stoneskin, ...) takes its icon
//! from its buff record (`display_record`). Sounds: [`press_sound`].
//! Hooks (hooks.rs): `GD_UI_OPEN=potions` (`GD_UI_TAB=2`: the energy page); `GD_POTION_SLOT` opens a slot's list,
//! `GD_POTION_HOVER` shows a skill's tooltip, `GD_POTION_SCROLL` scrolls the description, `GD_POTION_CONTAINER`
//! chooses a container of the page, `GD_USE_CAULDRON` walks to the nearest cauldron and opens it (each as a click would).
use super::Ui;
use super::contains;
use super::window::Rebuild;
use crate::state::inventory::UiCapture;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::{Rect as URect, TextBox, WindowDef};
use gd_game::cauldron::{PotionRequest, PotionWindow};
use gd_game_core::data::GameData;
use gd_sim::potions::PotionTree;
use std::borrow::Cow;

pub const RECORD: &str = "records/ui/cauldron/potions_table.dbr";

pub struct PotionsUiPlugin;

impl Plugin for PotionsUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PotionsLayout>()
            .init_resource::<PotionsView>()
            .add_systems(
                Update,
                click_cauldron.in_set(gd_ui_kit::UiInputSet).after(clicks),
            );
        #[cfg(feature = "dev")]
        app.add_systems(
            Update,
            test_hooks.before(build).in_set(gd_ui_kit::UiPlaySet),
        );
        // Live: open, or still built (the closing frame despawns the root).
        super::window::add_window(
            app,
            build,
            clicks,
            |open: Res<PotionWindow>, layout: Res<PotionsLayout>| {
                open.open || layout.window.is_some()
            },
        );
        app.add_systems(
            Update,
            (live, tooltip).in_set(gd_ui_kit::UiPlaySet).after(build),
        );
    }
}

/// A clickable part of the window.
#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Close,
    Tab(usize),
    Container(String),
    Slot(usize),
    /// A modifier of the open list.
    Pick(String),
}

/// UI-only state: the slot whose modifier list is open, the description's
/// scroll (px).
#[derive(Resource, Default, Clone, PartialEq)]
pub struct PotionsView {
    pub list: Option<usize>,
    pub scroll: f32,
}

/// The built window (physical px). Empty while it is closed.
#[derive(Resource, Default, Clone, PartialEq)]
pub struct PotionsLayout {
    pub window: Option<URect>,
    pub list: Option<URect>,
    pub parts: Vec<(URect, Part)>,
    /// The description's scroll window (`potionsModifiersScrollWindow`) and
    /// how far it can scroll.
    pub desc: Option<(URect, f32)>,
    /// Its scroll bar.
    pub(super) bar: Option<super::window::Bar>,
    /// The modifier buttons are enabled (a container of the page is chosen).
    pub slots_enabled: bool,
}

#[derive(Component)]
struct PotionsRoot;

/// A button frame's up / over bitmaps ([`live`] swaps them by the hover; the down frame is static per
/// build, `enabled` gates the over look).
#[derive(Component)]
struct FrameLook {
    part: Part,
    up: Handle<Image>,
    over: Handle<Image>,
    down: bool,
    enabled: bool,
}

/// The description pane's inner node ([`live`] scrolls it by `PotionsView::scroll`).
#[derive(Component)]
struct DescScrolled {
    clip_y: f32,
    inv: f32,
}

/// The hovered part: the `GD_POTION_HOVER` hook, else the topmost part under the cursor (borrowed from the
/// layout: no copy per frame).
fn hovered<'a>(
    hooks: &gd_game_core::hooks::Hooks,
    win: &Window,
    layout: &'a PotionsLayout,
) -> Option<Cow<'a, Part>> {
    hooks
        .ui
        .potion_hover
        .clone()
        .map(|s| Cow::Owned(Part::Pick(s)))
        .or_else(|| {
            super::cursor_px(hooks, win).and_then(|p| {
                layout
                    .parts
                    .iter()
                    .rev()
                    .find(|(r, _)| contains(r, p))
                    .map(|(_, h)| Cow::Borrowed(h))
            })
        })
}

/// The potions window's rect (see the module docs).
pub fn window_rect(data: &GameData, screen: Vec2, s: f32) -> Option<URect> {
    let rec = data.db.get(RECORD)?;
    let mut r = WindowDef::from_record(rec).screen_rect(screen.x, screen.y, s);
    let inv_w = data.db.get(super::character::MASTER).map_or(0.0, |cm| {
        WindowDef::from_record(cm)
            .screen_rect(screen.x, screen.y, s)
            .w
    });
    r.x = ((screen.x - inv_w - r.w) * 0.5).max(0.0).round();
    Some(r)
}

/// A skill's button icon at `level` (1-based; `SkillButton::GetIcon`).
pub(super) fn skill_icon(data: &GameData, skill: &str, level: u32) -> Option<String> {
    let rec = gd_sim::skills::display_record(&data.db, skill)?;
    let i = level.max(1) as usize - 1;
    let pick = |k: &str| {
        let v = rec.strs(k);
        v.get(i)
            .or(v.last())
            .filter(|p| !p.is_empty())
            .map(|p| p.to_string())
    };
    pick("skillUpAlternateBitmapName").or_else(|| pick("skillUpBitmapName"))
}

fn field_pos(r: &gd_formats::arz::Record, k: &str) -> f32 {
    r.f32(k).unwrap_or(0.0)
}

/// A `potionbutton.tpl` button: its frame (`frame` bitmap field), then
/// `icon` at `skillOffset`; the rect of the frame (or of the icon).
fn potion_button(
    ui: &mut Ui,
    b: &gd_formats::arz::Record,
    o: Vec2,
    frame: Option<&str>,
    icon: Option<&str>,
    gray: bool,
    look: Option<(Part, bool, bool)>,
) -> URect {
    let at = Vec2::new(
        o.x + ui.sc(field_pos(b, "bitmapPositionX")),
        o.y + ui.sc(field_pos(b, "bitmapPositionY")),
    );
    // The frame size defines the button (InFocus exists on every one).
    let size_of = |ui: &mut Ui, k: &str| {
        b.str(k)
            .map(str::to_owned)
            .and_then(|p| ui.image(&p))
            .map(|(_, s)| s)
    };
    let size = size_of(ui, "bitmapNameInFocus")
        .or_else(|| size_of(ui, "bitmapNameUp"))
        .unwrap_or(UVec2::new(64, 64));
    let r = URect::new(at.x, at.y, ui.sc(size.x as f32), ui.sc(size.y as f32));
    let tint = if gray {
        Color::srgba(0.45, 0.45, 0.45, 1.0)
    } else {
        Color::WHITE
    };
    if let Some((img, sz)) = frame
        .and_then(|k| b.str(k))
        .map(str::to_owned)
        .and_then(|p| ui.image(&p))
    {
        let e = ui.quad(
            img,
            URect::new(r.x, r.y, ui.sc(sz.x as f32), ui.sc(sz.y as f32)),
            None,
            tint,
        );
        if let Some((part, down, enabled)) = look {
            let h = |ui: &mut Ui, k: &str| {
                b.str(k)
                    .map(str::to_owned)
                    .and_then(|p| ui.image(&p))
                    .map(|(h, _)| h)
            };
            if let (Some(up), Some(over)) = (h(ui, "bitmapNameUp"), h(ui, "bitmapNameInFocus")) {
                ui.commands.entity(e).insert(FrameLook {
                    part,
                    up,
                    over,
                    down,
                    enabled,
                });
            }
        }
    }
    if let Some(path) = icon {
        let img = if gray {
            ui.image_gray(path)
        } else {
            ui.image(path)
        };
        if let Some((img, sz)) = img {
            let ir = URect::new(
                at.x + ui.sc(field_pos(b, "skillOffsetX")),
                at.y + ui.sc(field_pos(b, "skillOffsetY")),
                ui.sc(sz.x as f32),
                ui.sc(sz.y as f32),
            );
            ui.quad(img, ir, None, Color::WHITE);
        }
    }
    r
}

/// The module doc's hooks.
#[cfg(feature = "dev")]
fn test_hooks(
    hooks: Res<gd_game_core::hooks::Hooks>,
    players: Query<(), With<gd_game_core::player::Player>>,
    data: Res<GameData>,
    window: Res<PotionWindow>,
    mut view: ResMut<PotionsView>,
    mut out: MessageWriter<PotionRequest>,
    mut done: Local<u8>,
) {
    if players.is_empty() || *done >= 2 {
        return;
    }
    if *done == 0 {
        *done = 1;
        if hooks.ui.open.as_deref() == Some("potions") {
            out.write(PotionRequest::Open(None));
            if hooks.ui.tab == Some(2) {
                out.write(PotionRequest::Page(1));
            }
        } else {
            *done = 2;
        }
    } else if window.open {
        *done = 2;
        if let Some(i) = hooks.ui.potion_container
            && let Some(c) = gd_sim::potions::trees(&data.db)
                .get(window.page)
                .and_then(|t| t.containers.get(i).cloned())
        {
            out.write(PotionRequest::Container(c));
        }
        view.list = hooks.ui.potion_slot.filter(|s| *s < 2);
        view.scroll = hooks.ui.potion_scroll;
    }
}

#[allow(clippy::too_many_arguments)]
fn build(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    open: Res<PotionWindow>,
    values: Res<super::HudValues>,
    mut view: ResMut<PotionsView>,
    roots: Query<Entity, With<PotionsRoot>>,
    mut layout: ResMut<PotionsLayout>,
    mut trees: Local<Option<Vec<PotionTree>>>,
    mut fresh: gd_game_core::session::NewSession,
    mut built: Local<Rebuild<(u64, u64, UVec2, Option<usize>, gd_sim::skills::SkillBook)>>,
    // The tab icons set when the window opened (module doc).
    mut tab_icons: Local<Option<Vec<Option<String>>>>,
    mut assets: gd_ui_kit::UiAssets,
) {
    if fresh.begun() {
        built.reset();
        *tab_icons = None;
        // The ended session's data (a session may mount another mod).
        *trees = None;
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if !open.open && (view.list.is_some() || view.scroll != 0.0 || tab_icons.is_some()) {
        view.list = None;
        view.scroll = 0.0;
        *tab_icons = None;
    }
    // Hover and scroll are not in the signature: `live` moves the frames, the pane and the thumb,
    // `tooltip` shows the hovered skill's rollover. The font generation is: a language swap redraws the texts.
    let list = view.list;
    let generation = assets.fonts.generation;
    if size.x == 0
        || !built.changed_by(
            |b| {
                b.0 == open.version
                    && b.1 == generation
                    && b.2 == size
                    && b.3 == list
                    && b.4 == values.skills
            },
            || (open.version, generation, size, list, values.skills.clone()),
        )
    {
        return;
    }
    super::window::despawn_all(&mut commands, &roots);
    *layout = PotionsLayout::default();
    let Some(rec) = data.db.get(RECORD).filter(|_| open.open) else {
        return;
    };
    let trees = trees.get_or_insert_with(|| gd_sim::potions::trees(&data.db));
    let Some(tree) = trees.get(open.page) else {
        return;
    };
    let book = &values.skills;
    let tab_icons = tab_icons.get_or_insert_with(|| {
        trees
            .iter()
            .map(|t| {
                gd_sim::potions::potion_bitmap(&data.db, book, t)
                    .map(|b| b.texture)
                    .filter(|b| !b.is_empty())
                    .or_else(|| skill_icon(&data, &t.potion, 1))
            })
            .collect()
    });
    let s = assets.scale(size);
    // Description and tooltip lines (before `Ui` borrows the assets.fonts).
    let ctx = gd_sim::skill_tooltip::SkillTipCtx {
        db: &data.db,
        trees: &data.skill_trees(),
        tags: &assets.fonts.tags,
        book,
        player: None,
        pet_bonuses: &[],
    };
    let slots = gd_sim::potions::slots(book, tree);
    let mut desc = gd_sim::skill_tooltip::skill_lines(&ctx, &tree.potion, None);
    for sk in tree
        .containers
        .iter()
        .filter(|c| book.level(c) > 0)
        .chain(slots.iter().flatten())
    {
        desc.push(gd_sim::skill_tooltip::TextLine {
            class: 0,
            text: String::new(),
            icon: None,
        });
        desc.extend(gd_sim::skill_tooltip::skill_lines(&ctx, sk, None));
    }
    let root = gd_ui_kit::spawn_root(&mut commands, PotionsRoot, super::window::Z_WINDOW);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        s,
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    let field = |k: &str| rec.str(k).unwrap_or("").to_string();
    let Some(r) = window_rect(&data, ui.screen, s) else {
        return;
    };
    let o = Vec2::new(r.x, r.y);
    // Background, title.
    if let Some(bg) = ui.record(&field("potionsBaseBitmap"))
        && let Some(path) = bg.str("bitmapName")
    {
        ui.bitmap(
            path,
            field_pos(&bg, "bitmapPositionX"),
            field_pos(&bg, "bitmapPositionY"),
            o,
        );
    }
    if let Some(t) = ui.record(&field("potionsWindowTitle")) {
        let tb = TextBox::from_record(&t);
        let text = tb
            .tag
            .as_deref()
            .map(|t| ui.fonts.tag(t).to_string())
            .unwrap_or_default();
        ui.text(&tb, o, &text);
    }
    // Tabs.
    for (i, key) in ["potions1Button", "potions2Button"].into_iter().enumerate() {
        let Some(b) = ui.record(&field(key)) else {
            continue;
        };
        let frame = if open.page == i {
            "bitmapNameDown"
        } else {
            "bitmapNameUp"
        };
        let icon = tab_icons
            .get(i)
            .cloned()
            .flatten()
            .or_else(|| b.str("potionBitmapName").map(str::to_owned));
        let br = potion_button(
            &mut ui,
            &b,
            o,
            Some(frame),
            icon.as_deref(),
            false,
            Some((Part::Tab(i), open.page == i, true)),
        );
        layout.parts.push((br, Part::Tab(i)));
    }
    // Containers.
    for (i, c) in tree.containers.iter().enumerate().take(3) {
        let Some(b) = ui.record(&field(&format!("potionsContainer{}", i + 1))) else {
            continue;
        };
        let tier = book
            .unlock
            .get(&c.to_ascii_lowercase())
            .copied()
            .unwrap_or(0);
        let chosen = book.level(c) > 0;
        let frame = if chosen {
            "bitmapNameDown"
        } else {
            "bitmapNameUp"
        };
        let icon = skill_icon(&data, c, tier.max(1))
            .or_else(|| b.str("potionBitmapName").map(str::to_owned));
        let br = potion_button(
            &mut ui,
            &b,
            o,
            Some(frame),
            icon.as_deref(),
            tier == 0,
            Some((Part::Container(c.clone()), chosen, true)),
        );
        layout.parts.push((br, Part::Container(c.clone())));
    }
    // Modifier slots.
    let enabled = gd_sim::potions::slots_enabled(book, tree);
    layout.slots_enabled = enabled;
    let mut slot_rects = [None; 2];
    for (i, slot) in slots.iter().enumerate() {
        let Some(b) = ui.record(&field(&format!("potionsModifier{}", i + 1))) else {
            continue;
        };
        let frame = if view.list == Some(i) {
            "bitmapNameDown"
        } else {
            "bitmapNameUp"
        };
        let icon = slot.as_deref().and_then(|sk| skill_icon(&data, sk, 1));
        let br = potion_button(
            &mut ui,
            &b,
            o,
            Some(frame),
            icon.as_deref(),
            !enabled,
            Some((Part::Slot(i), view.list == Some(i), enabled)),
        );
        slot_rects[i] = Some(br);
        if enabled || slot.is_some() {
            layout.parts.push((br, Part::Slot(i)));
        }
    }
    // Description: the potions' text box in its scroll window
    // (loader `PotionsWindow::Load`: the box is the window's only child,
    // `UIScrollWindow::SetChild`), the same `UIScrollWindow` as the journal's panes
    // (`UIScrollWindow::Load`, `UIScrollWindow::HandleArrowInput`), so the same
    // scroll bar (`verticalScrollbar`, always shown here) and arrows.
    if let (Some(sw), Some(bx)) = (
        ui.record(&field("potionsModifiersScrollWindow")),
        ui.record(&field("potionsModifiersBox")),
    ) {
        let tb = TextBox::from_record(&bx);
        let num = |k: &str| field_pos(&sw, k);
        let (top, bottom) = (num("topGuardBand"), num("bottomGuardBand"));
        let window = URect::new(
            o.x + ui.sc(num("positionX")),
            o.y + ui.sc(num("positionY")),
            ui.sc(num("width")),
            ui.sc(num("height")),
        );
        let clip = URect::new(
            window.x,
            window.y + ui.sc(top),
            window.w,
            ui.sc(num("height") - top - bottom),
        );
        let (x, y0) = (window.x + ui.sc(tb.rect.x), clip.y + ui.sc(tb.rect.y));
        let w = ui.sc(tb.rect.w);
        let default_style = tb.style.clone().unwrap_or_default();
        let blank = ui.line_height(&default_style);
        let outer = ui.root;
        let inner = super::window::pane(&mut ui, clip, view.scroll);
        ui.commands.entity(inner).insert(DescScrolled {
            clip_y: clip.y,
            inv: ui.inv,
        });
        ui.root = inner;
        let mut y = y0;
        for l in &desc {
            if l.text.is_empty() {
                y += blank;
                continue;
            }
            let style = gd_sim::skill_tooltip::style_path(&data.db, l.class)
                .unwrap_or_else(|| default_style.clone());
            let lh = ui.line_height(&style);
            for part in ui.wrap_lines(&style, w, &l.text) {
                // The pane clips the rows that cannot show; `live` scrolls it.
                ui.text_line(&style, Vec2::new(x, y), &part, 1.0);
                y += lh;
            }
        }
        ui.root = outer;
        let content = y - clip.y;
        let max = (content - clip.h).max(0.0);
        if view.scroll > max {
            view.scroll = max;
        }
        layout.desc = Some((window, max));
        if content > clip.h || sw.bool("alwaysShowScroll") {
            let len = ui.sc(num("height") + num("scrollbarSizeAdjust"));
            let top = window.y + ui.sc(num("scrollbarPositionAdjust"));
            layout.bar = super::window::scrollbar_live(
                &mut ui,
                sw.str("verticalScrollbar").unwrap_or(""),
                clip.x + clip.w,
                top,
                len,
                (clip.h, content, view.scroll),
                super::window::ScrollOwner::Potions,
                0,
            );
        }
    }
    // Close button.
    if let Some(b) = ui.record(&field("potionsCloseButton"))
        && let Some((img, sz)) = b
            .str("bitmapNameUp")
            .map(str::to_owned)
            .and_then(|p| ui.image(&p))
    {
        let br = URect::new(
            o.x + ui.sc(field_pos(&b, "bitmapPositionX")),
            o.y + ui.sc(field_pos(&b, "bitmapPositionY")),
            ui.sc(sz.x as f32),
            ui.sc(sz.y as f32),
        );
        let e = ui.quad(img, br, None, Color::WHITE);
        let h = |ui: &mut Ui, k: &str| {
            b.str(k)
                .map(str::to_owned)
                .and_then(|p| ui.image(&p))
                .map(|(h, _)| h)
        };
        if let (Some(up), Some(over)) =
            (h(&mut ui, "bitmapNameUp"), h(&mut ui, "bitmapNameInFocus"))
        {
            ui.commands.entity(e).insert(FrameLook {
                part: Part::Close,
                up,
                over,
                down: false,
                enabled: true,
            });
        }
        layout.parts.push((br, Part::Close));
    }
    layout.window = Some(r);
    // The open slot's modifier list.
    if let Some(slot) = view.list.filter(|_| enabled)
        && let Some(anchor) = slot_rects[slot]
        && let Some(sel) = ui.record(&field("skillSelect"))
    {
        // The list (`slot_select`): the potion's unlocked modifiers (the window's
        // `selectedBorder` is never drawn, module docs).
        let list = gd_sim::potions::unlocked_modifiers(book, tree);
        let picks: Vec<super::slot_select::Pick> = list
            .iter()
            .map(|sk| super::slot_select::Pick {
                icon: skill_icon(&data, sk, 1),
                name: gd_sim::skills::display_record(&data.db, sk)
                    .and_then(|r| r.str("skillDisplayName").map(str::to_owned))
                    .map(|t| ui.fonts.tag(&t).to_string())
                    .unwrap_or_default(),
                selected: slots.iter().flatten().any(|s| s.eq_ignore_ascii_case(sk)),
                look: Default::default(),
            })
            .collect();
        let (lr, rows) = super::slot_select::draw_live(
            &mut ui,
            &sel,
            &picks,
            super::slot_select::Place::Below(anchor),
            None,
        );
        for (row, sk) in rows.into_iter().zip(&list) {
            layout.parts.push((row, Part::Pick((*sk).clone())));
        }
        layout.list = Some(lr);
    }
}

/// The window's hover looks in place (no rebuild): the button frames' up / over bitmaps, the open
/// list's hovered row tint, the description pane and its scroll bar thumb.
#[allow(clippy::too_many_arguments)]
fn live(
    hooks: Res<gd_game_core::hooks::Hooks>,
    window: Query<&Window, With<PrimaryWindow>>,
    open: Res<PotionWindow>,
    view: Res<PotionsView>,
    layout: Res<PotionsLayout>,
    mut frames: Query<(&FrameLook, &mut ImageNode)>,
    mut hovers: Query<(&super::slot_select::PickHover, &ChildOf, &mut Visibility)>,
    picks: Query<(), With<super::hotbar_pick::PickRoot>>,
    mut descs: Query<(&DescScrolled, &mut Node), Without<super::window::ScrollThumb>>,
    mut thumbs: Query<(&super::window::ScrollThumb, &mut Node), Without<DescScrolled>>,
    pause: Res<gd_game_core::pause::GamePause>,
) {
    if !open.open {
        return;
    }
    let Ok(win) = window.single() else { return };
    // The game menu / Options holds the mouse: no hover looks under it.
    let hover = hovered(&hooks, win, &layout).filter(|_| !pause.menu);
    for (f, mut img) in &mut frames {
        if f.down {
            continue;
        }
        let want = if f.enabled && hover.as_deref() == Some(&f.part) {
            &f.over
        } else {
            &f.up
        };
        if img.image != *want {
            img.image = want.clone();
        }
    }
    let pick = match hover.as_deref() {
        Some(Part::Pick(_)) => layout
            .parts
            .iter()
            .filter(|(_, p)| matches!(p, Part::Pick(_)))
            .position(|(_, p)| Some(p) == hover.as_deref()),
        _ => None,
    };
    for (h, parent, mut vis) in &mut hovers {
        // The hot slot list's rows are `hotbar_pick::draw`'s.
        if picks.contains(parent.parent()) {
            continue;
        }
        vis.set_if_neq(if pick == Some(h.0) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    for (d, mut node) in &mut descs {
        let top = Val::Px(-(d.clip_y + view.scroll) * d.inv);
        if node.top != top {
            node.top = top;
        }
    }
    for (t, mut node) in &mut thumbs {
        if t.owner != super::window::ScrollOwner::Potions {
            continue;
        }
        let off = t.travel * (view.scroll / t.max).clamp(0.0, 1.0);
        let top = Val::Px(((t.base_y + off).round() + t.round_add) * t.inv);
        if node.top != top {
            node.top = top;
        }
    }
}

/// The hovered skill's rollover, right of the window (or of the open list), through the shared
/// `TipCanvas` with the style's delay — not drawn into the window build.
#[allow(clippy::too_many_arguments)]
fn tooltip(
    hooks: Res<gd_game_core::hooks::Hooks>,
    window: Query<&Window, With<PrimaryWindow>>,
    open: Res<PotionWindow>,
    values: Res<super::HudValues>,
    layout: Res<PotionsLayout>,
    mut canvas: super::tip::TipCanvas,
    mut trees: Local<Option<Vec<PotionTree>>>,
    mut shown: Local<Option<(Option<Part>, u64, gd_sim::skills::SkillBook)>>,
    mut fresh: gd_game_core::session::NewSession,
) {
    if fresh.begun() {
        *shown = None;
        // The ended session's data (a session may mount another mod).
        *trees = None;
    }
    let Ok(win) = window.single() else { return };
    if !open.open {
        if shown.is_some() {
            *shown = None;
            canvas.clear(super::tip::TipOwner::Potions);
        }
        return;
    }
    let hover = hovered(&hooks, win, &layout);
    let same = shown.as_ref().is_some_and(|(h, v, book)| {
        h.as_ref() == hover.as_deref() && *v == open.version && *book == values.skills
    });
    if same {
        return;
    }
    *shown = Some((
        hover.as_deref().cloned(),
        open.version,
        values.skills.clone(),
    ));
    canvas.clear(super::tip::TipOwner::Potions);
    let skill = match hover.as_deref() {
        Some(Part::Container(sk) | Part::Pick(sk)) => Some(sk.clone()),
        Some(Part::Slot(i)) => {
            let trees = trees.get_or_insert_with(|| gd_sim::potions::trees(&canvas.data.db));
            trees
                .get(open.page)
                .and_then(|t| gd_sim::potions::slots(&values.skills, t).get(*i).cloned())
                .flatten()
        }
        _ => None,
    };
    let Some(skill) = skill else { return };
    let ctx = gd_sim::skill_tooltip::SkillTipCtx {
        db: &canvas.data.db,
        trees: &canvas.data.skill_trees(),
        tags: &canvas.assets.fonts.tags,
        book: &values.skills,
        player: None,
        pet_bonuses: &[],
    };
    let lines = gd_sim::skill_tooltip::skill_lines(&ctx, &skill, None);
    if lines.is_empty() {
        return;
    }
    let Some(wr) = layout.window else { return };
    let anchor = hover
        .as_deref()
        .and_then(|h| layout.parts.iter().find(|(_, p)| p == h))
        .map_or(wr, |(a, _)| *a);
    let right = layout
        .list
        .map_or(wr.x + wr.w, |l| (wr.x + wr.w).max(l.x + l.w));
    canvas.show(
        super::tip::TipOwner::Potions,
        &lines,
        super::tip::Blank::DefaultLine,
        |w, h, screen, gap| {
            (
                (right + gap).min((screen.x - w).max(0.0)),
                anchor.y.min((screen.y - h).max(0.0)),
            )
        },
    );
}

/// Clicks on the window (it takes them: no walking): tabs, containers,
/// slots (open / close their list), list picks, the close button. Esc closes the list, else the
/// window, through the shared dispatcher (`ui::esc_dispatcher`).
#[allow(clippy::too_many_arguments)]
pub fn clicks(
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    hooks: Res<gd_game_core::hooks::Hooks>,
    scale: Res<gd_ui_kit::state::settings::UiScaleOption>,
    layout: Res<PotionsLayout>,
    open: Res<PotionWindow>,
    values: Res<super::HudValues>,
    mut view: ResMut<PotionsView>,
    mut capture: ResMut<UiCapture>,
    mut out: MessageWriter<PotionRequest>,
    data: Res<GameData>,
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    time: Res<Time<Real>>,
    mut held: Local<Option<super::window::BarHold>>,
    pause: Res<gd_game_core::pause::GamePause>,
) {
    let Some(wr) = layout.window.filter(|_| open.open) else {
        wheel.clear();
        *held = None;
        return;
    };
    // The game menu / Options holds the mouse: no wheel, no press, no held bar.
    if pause.menu {
        wheel.clear();
        *held = None;
        return;
    }
    // Esc closes the list, else the window, through the shared dispatcher (`ui::esc_dispatcher`).
    let Ok(win) = window.single() else { return };
    let Some(p) = super::cursor_px(&hooks, win) else {
        return;
    };
    let max = layout.desc.map_or(0.0, |(_, m)| m);
    // A held scroll bar arrow repeats, a held thumb drags (`window::bar_held`).
    if !buttons.pressed(MouseButton::Left) {
        *held = None;
    }
    if let Some(hold) = held.as_mut() {
        let v = super::window::bar_held(
            layout.bar,
            hold,
            p.y,
            time.delta_secs() * 1000.0,
            view.scroll,
            max,
        );
        if v != view.scroll {
            view.scroll = v;
        }
        capture.0 = true;
        wheel.clear();
        return;
    }
    let hit = layout
        .parts
        .iter()
        .rev()
        .find(|(r, _)| contains(r, p))
        .map(|(_, h)| h.clone());
    if !contains(&wr, p) && !layout.list.is_some_and(|l| contains(&l, p)) {
        wheel.clear();
        if buttons.just_pressed(MouseButton::Left) {
            view.list = None;
        }
        return;
    }
    capture.0 = true;
    // The wheel over the scroll window scrolls the description: int(delta ×
    // 0.2 × s) px with Windows' 120 per notch (scroll bar mouse
    // handler `UIScrollWindow::HandleScrollInput`, events 17 / 18, passed on by the scroll window's
    // `UIScrollWindow::HandleMouseEvent` while the cursor is inside it).
    let s = scale.factor(win.physical_width(), win.physical_height());
    for w in wheel.read() {
        if !layout.desc.is_some_and(|(r, _)| contains(&r, p))
            || layout.list.is_some_and(|l| contains(&l, p))
        {
            continue;
        }
        let step = match w.unit {
            bevy::input::mouse::MouseScrollUnit::Line => (w.y * 120.0 * 0.2 * s).trunc(),
            bevy::input::mouse::MouseScrollUnit::Pixel => w.y,
        };
        let v = (view.scroll - step).clamp(0.0, max);
        if v != view.scroll {
            view.scroll = v;
        }
    }
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    if layout.list.is_none_or(|l| !contains(&l, p))
        && let Some((v, hold)) = layout.bar.and_then(|b| b.press(p, s, view.scroll, max))
    {
        view.scroll = v;
        *held = Some(hold);
        return;
    }
    // A press plays the button's `soundNameDown` when it goes down ([`press_sound`]).
    let button = press_sound(
        hit.as_ref(),
        open.page,
        |c| values.skills.level(c) > 0,
        layout.slots_enabled,
    );
    // A pick plays the list's `skillSelectSound` (`SlotSelectWindow::HandleMouseEvent`).
    let (button, key) = match (&hit, button) {
        (Some(Part::Pick(_)), _) => (Some("skillSelect".to_owned()), "skillSelectSound"),
        (_, b) => (b, "soundNameDown"),
    };
    if let Some(pak) = button.and_then(|f| {
        data.db
            .get(RECORD)?
            .str(&f)
            .and_then(|r| data.db.get(r))
            .and_then(|b| b.str(key).map(str::to_owned))
    }) {
        sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
    }
    match hit {
        Some(Part::Close) => {
            out.write(PotionRequest::Close);
        }
        Some(Part::Tab(i)) => {
            view.list = None;
            // PORT: a page switch scrolls the description back to the top.
            if open.page != i {
                view.scroll = 0.0;
            }
            out.write(PotionRequest::Page(i));
        }
        Some(Part::Container(c)) => {
            view.list = None;
            if values.skills.level(&c) == 0 {
                out.write(PotionRequest::Container(c));
            }
        }
        Some(Part::Slot(i)) => view.list = if view.list == Some(i) { None } else { Some(i) },
        Some(Part::Pick(skill)) => {
            if let Some(slot) = view.list.take() {
                out.write(PotionRequest::Modifier { skill, slot });
            }
        }
        None => view.list = None,
    }
}

/// The button record whose `soundNameDown` a left press on `hit` plays. `Grim Dawn.exe`: the tabs
/// (`PotionsWindow.tabGroup`) and each page's containers are radio groups (`UIRadioButtonGroup::HandleMouseEvent`): a
/// press on an enabled button that is up sets it down with its `soundNameDown` (the group's down one goes up with its
/// `soundNameUp`: no stock potion button has one); a press on the down one does nothing. The close button
/// (`PotionsWindow.closeButton`) and the modifier buttons are push groups (`UIPushButtonGroup::HandleMouseEvent`): a
/// press on an enabled button sets it down with the sound (the release over it plays `soundNameUp`, none in stock). The
/// window acts on the press (listener `PotionsWindow::OnButtonPress`, event 0). The update `PotionsWindow::Update`
/// disables the modifier buttons while no container of the page is down; containers are never disabled.
fn press_sound(
    hit: Option<&Part>,
    page: usize,
    chosen: impl Fn(&str) -> bool,
    slots_enabled: bool,
) -> Option<String> {
    match hit? {
        Part::Close => Some("potionsCloseButton".to_owned()),
        Part::Tab(i) if *i != page => Some(format!("potions{}Button", i + 1)),
        // Every container has the same sound.
        Part::Container(c) if !chosen(c) => Some("potionsContainer1".to_owned()),
        Part::Slot(i) if slots_enabled => Some(format!("potionsModifier{}", i + 1)),
        _ => None,
    }
}

/// Clicking a cauldron is the shared interact order ([`gd_game::player_orders::PlayerOrder::Interact`],
/// from the hover pick [`crate::state::hover::Hovered`]): gd-game walks beside it and opens the window in
/// use range (`gd_game::cauldron::reach_cauldron`, `NpcCauldron::OnPlayerInteract`). The Interact key's
/// `UseObject` routes through the same order (`apply_orders`), so it opens the window too.
#[allow(clippy::too_many_arguments)]
fn click_cauldron(
    hooks: Res<gd_game_core::hooks::Hooks>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: crate::state::casting::ClickKeys,
    capture: Res<UiCapture>,
    data: Res<GameData>,
    hovered: Res<crate::state::hover::Hovered>,
    npcs: Query<
        (Entity, &GlobalTransform, &gd_render::character::Character),
        Without<gd_game_core::player::Player>,
    >,
    players: Query<&Transform, With<gd_game_core::player::Player>>,
    open: Res<PotionWindow>,
    mut orders: MessageWriter<gd_game::player_orders::PlayerOrder>,
    mut tested: Local<bool>,
) {
    let Ok(tf) = players.single() else {
        return;
    };
    let is_cauldron = |c: &gd_render::character::Character| {
        data.db
            .get(&c.record)
            .is_some_and(gd_game::cauldron::is_cauldron)
    };
    // `GD_USE_CAULDRON`: the nearest within 40 m, as if clicked.
    if !*tested && hooks.gameplay.use_cauldron {
        let near = npcs
            .iter()
            .filter(|(_, gt, c)| gt.translation().distance(tf.translation) < 40.0 && is_cauldron(c))
            .min_by(|a, b| {
                a.1.translation()
                    .distance(tf.translation)
                    .total_cmp(&b.1.translation().distance(tf.translation))
            });
        if let Some((e, gt, _)) = near {
            *tested = true;
            info!(
                "GD_USE_CAULDRON: walking to the cauldron at {}",
                gt.translation()
            );
            orders.write(gd_game::player_orders::PlayerOrder::Interact(e));
            return;
        }
    }
    if capture.0 || open.open || keys.skips_objects() || !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    // The hover pick (one pick feeds the hover and the world clicks): a cauldron under the cursor.
    let Some(e) = hovered.entity else { return };
    if let Ok((_, _, c)) = npcs.get(e)
        && is_cauldron(c)
    {
        orders.write(gd_game::player_orders::PlayerOrder::Interact(e));
    }
}
