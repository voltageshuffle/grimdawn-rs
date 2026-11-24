//! The skill-style hover tooltip, in one place: the text-line box of `gd_sim::skill_tooltip` lines (skill buttons, devotion
//! stars, buff icons, hot slots, the potion and hot-slot pick lists), drawn by [`draw_wrapped`] and put on screen by [`TipCanvas`].
//! A window system only decides what is hovered and which lines it shows; it never builds the overlay itself.
//!
//! PORT: the game's tooltip window is drawn by the engine, not a database record; the box is a plain translucent panel
//! placed by each caller's rule. Record-styled rollovers (HUD buttons, sheet rows, shrine,
//! loot filter, ...) are a different window: `rollover.rs`; item tooltips: `tooltip.rs`.
//!
//! Delay, Grim Dawn.exe (the rollover window): setting a rollover object (`RolloverWindow::SetRollover`) restarts its
//! counter (`RolloverWindow.delayCounter`, zeroed by the layout `RolloverWindow::Layout`) only when the object differs
//! from the shown one; the window's update (`RolloverWindow::Tick`) adds the frame's ms, and its draw
//! (`RolloverWindow::Update`) shows it once the counter reaches the request style's `delay` (`RolloverStyleUIHelp`,
//! loader `RolloverStyleUIHelp::Load`, default 0), or at once while game time is paused. The style is the request's:
//! `style_hotslotbuttons` (600 ms) for a hot slot (`HotSlotButton::GetRollover`), else the request's default
//! `style_default` (150 ms, `RolloverRequest::RolloverRequest`) for skills, devotion stars and buff icons
//! ([`TipOwner::style`]). PORT: the object is told by the tooltip's first line (the skill or effect name), so a changed
//! text for the same object (cooldown state, time left) keeps its counter. While the game menu holds the game
//! (`gamemenu.rs`), the HUD's mouse handler (`UIPlayerHud::HandleMouseEvent`) tests nothing under the exit window and
//! hides the rollover: no tooltip shows and the counter starts over once the menu is gone. [`TipDelay`] is the one
//! counter: [`delay_tips`] drives the [`TipCanvas`] roots and the menu manager's box roots (`rollover.rs::menu_tip`,
//! the options window — [`TipDelay::menu`], which the game menu does not hide, its window being the menu's child);
//! `tooltip.rs` ticks its own [`TipDelay`] for the item box it draws itself.
use super::Ui;
use super::text::LoadedFont;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::{Rect as URect, TextBox};
use gd_game_core::data::GameData;

/// Width of the text column, in UI units.
const TIP_WIDTH: f32 = 300.0;

/// Which window's tooltip an overlay belongs to (one is shown per owner).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TipOwner {
    Skills,
    Devotion,
    Buff,
    Hotbar,
    /// The Ascendant Altar's row / box rollovers (`altar.rs`).
    Altar,
    /// The potions window's skill rollovers (`potions.rs`).
    Potions,
    /// The hot-slot pick list's rows (`hotbar_pick.rs`).
    HotbarPick,
}

/// The overlay root of an owner's tooltip (not part of any rebuilt window tree).
#[derive(Component)]
pub struct TipRoot(pub TipOwner);

impl TipOwner {
    /// The rollover style its request carries (module doc).
    fn style(self) -> &'static str {
        match self {
            TipOwner::Hotbar => "records/ui/styles/rollover/style_hotslotbuttons.dbr",
            TipOwner::Skills
            | TipOwner::Devotion
            | TipOwner::Buff
            | TipOwner::Altar
            | TipOwner::Potions
            | TipOwner::HotbarPick => "records/ui/styles/rollover/style_default.dbr",
        }
    }
}

/// A tooltip's show delay (module doc): its object, the ms it has been up and the style's delay.
/// [`Self::skill`] is hidden while the game menu holds the game; [`Self::menu`] (the menu manager's own
/// rollover box, `rollover.rs`) is not — the options window is the menu's child.
#[derive(Component)]
pub struct TipDelay {
    object: String,
    ms: f32,
    delay: f32,
    menu_hidden: bool,
}

impl TipDelay {
    /// A skill-style tip's delay: the game menu hides it and restarts its counter (module doc).
    pub fn skill(object: String, delay: f32) -> Self {
        Self {
            object,
            ms: 0.0,
            delay,
            menu_hidden: true,
        }
    }

    /// The menu manager's rollover box delay (500 ms, `MenuRolloverBox::Draw`): the game menu does not
    /// hide it. The object names what is shown; a changed object restarts the counter.
    pub fn menu(object: String, delay: f32) -> Self {
        Self {
            object,
            ms: 0.0,
            delay,
            menu_hidden: false,
        }
    }

    /// Seeds the counted ms (the menu box's one-shot timer: a new root keeps the manager's elapsed time).
    pub fn with_ms(mut self, ms: f32) -> Self {
        self.ms = ms;
        self
    }

    /// One frame of `dt` ms of the delay: whether the tip shows after it (the module doc's rules).
    /// For a tooltip system that draws its own box (`tooltip.rs`) and drives the counter itself.
    pub fn tick(&mut self, dt_ms: f32, pause: &gd_game_core::pause::GamePause) -> bool {
        matches!(advance(self, dt_ms, pause), Visibility::Inherited)
    }
}

impl Default for TipDelay {
    /// A skill-style counter with no object and no delay (the `Local` initial state).
    fn default() -> Self {
        Self::skill(String::new(), 0.0)
    }
}

/// Shows a tooltip once its delay has passed, or at once while the game is paused; hides the skill-style
/// ones while the game menu holds the game, their counter starting over (module doc). Drives every
/// [`TipDelay`] (the skill-style [`TipRoot`]s and the menu manager's box roots alike).
pub fn delay_tips(
    time: Res<Time<Real>>,
    pause: Res<gd_game_core::pause::GamePause>,
    mut roots: Query<(&mut TipDelay, &mut Visibility)>,
) {
    for (mut d, mut vis) in &mut roots {
        vis.set_if_neq(advance(&mut d, time.delta_secs() * 1000.0, &pause));
    }
}

/// One frame of `dt` ms of a tooltip's delay: its visibility after it.
fn advance(d: &mut TipDelay, dt: f32, pause: &gd_game_core::pause::GamePause) -> Visibility {
    if pause.menu && d.menu_hidden {
        d.ms = 0.0;
    } else {
        d.ms += dt;
    }
    shown(d, pause)
}

fn shown(d: &TipDelay, pause: &gd_game_core::pause::GamePause) -> Visibility {
    if (!pause.menu || !d.menu_hidden) && (d.ms >= d.delay || pause.paused()) {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    }
}

/// How tall a blank line is. `GameTextLines::AddBlankLine` adds an empty line of class 0; what that measures to differs
/// per window (PORT): the default style's line (skills, buffs, hot slots), or the last description line's (the devotion
/// map).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Blank {
    #[default]
    DefaultLine,
    DescriptionLine,
}

/// How wide the text column is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Wrap {
    /// [`TIP_WIDTH`] (PORT), the box always that wide.
    #[default]
    Fixed,
    /// `RolloverWindow::Layout` (the rollover window's layout, `RolloverRequest.wrapMode` = 1): lines wrap at the width
    /// of [`SAMPLE_TEXT`] in the description style (class 47; the request's style, `RolloverRequest.style`), and the
    /// box is as wide as the widest laid-out line (the request's own width, `RolloverRequest.width`, is only used by
    /// mode 3).
    Sample,
}

/// The rollover window's placement mode 4 (`RolloverWindow::Place`, `RolloverRequest.placement` = 4): centred below the
/// anchor rect (x = centre − w / 2, y = bottom), the centre clamped so the box stays on screen horizontally, and the
/// box moved up to end at the screen's bottom edge when it would pass it.
pub fn place_below(anchor: URect, w: f32, h: f32, screen: Vec2) -> (f32, f32) {
    let mut cx = anchor.x + anchor.w * 0.5;
    if cx - w * 0.5 < 0.0 {
        cx = w * 0.5;
    }
    if cx + w * 0.5 > screen.x {
        cx = screen.x - w * 0.5;
    }
    let mut y = anchor.y + anchor.h;
    if y + h > screen.y {
        y = screen.y - h;
    }
    (cx - w * 0.5, y)
}

/// The sample line a mode 1 request measures its wrap width with (`PlayerStatusIcons::GetRolloverText`,
/// `RolloverRequest.sampleText`).
pub const SAMPLE_TEXT: &str = "The quick brown fox jumped over the lazy dog";

/// Draws a tooltip box of text lines (blank lines are spacers, `icon` lines get the affinity icon before the text);
/// `place(w, h, screen, gap)` gives the box's top-left corner; [`Wrap`] sets the wrapping rule.
pub fn draw_wrapped(
    ui: &mut Ui,
    lines: &[gd_sim::skill_tooltip::TextLine],
    blank: Blank,
    wrap: Wrap,
    place: impl FnOnce(f32, f32, Vec2, f32) -> (f32, f32),
) {
    let s = ui.s;
    let pad = ui.sc(8.0);
    let col_w = match wrap {
        Wrap::Fixed => ui.sc(TIP_WIDTH),
        Wrap::Sample => {
            let path = ui
                .fonts
                .class_style(ui.data, gd_sim::skill_tooltip::class::SKILL_DESCRIPTION)
                .0;
            let style = path.as_ref().and_then(|p| ui.fonts.style(ui.data, p));
            let font = style
                .as_ref()
                .and_then(|st| ui.fonts.font(ui.data, &st.font, ui.font_assets));
            match (&style, font.as_deref()) {
                (Some(st), Some(LoadedFont::Bitmap { font, .. })) => {
                    gd_formats::ui::layout_text(
                        font,
                        st.scaled_size(s),
                        st.flags,
                        SAMPLE_TEXT,
                        (st.color0, st.color1),
                    )
                    .width as f32
                }
                _ => ui.sc(TIP_WIDTH),
            }
        }
    };
    let mut widest = 0.0f32;
    // (style path, text, height, icon, indent) per drawn row.
    type Row = (Option<std::sync::Arc<str>>, String, f32, Option<usize>, f32);
    let mut rows: Vec<Row> = Vec::new();
    let mut description_h = 0.0f32;
    let data = ui.data;
    let engine = data.db.get("records/game/gameengine.dbr");
    for l in lines {
        // `GameEngine::GetGameTextStyleIndent` (the pet bonus lines sit 10 in).
        let (style_path, indent) = ui.fonts.class_style(ui.data, l.class);
        let indent = indent * s;
        let style = style_path.as_ref().and_then(|p| ui.fonts.style(ui.data, p));
        let font = style
            .as_ref()
            .and_then(|st| ui.fonts.font(ui.data, &st.font, ui.font_assets));
        let px = style.as_ref().map_or(12, |st| st.scaled_size(s));
        let measure = |t: &str| -> (f32, f32) {
            match (&style, font.as_deref()) {
                (Some(st), Some(LoadedFont::Bitmap { font, .. })) => {
                    let lay =
                        gd_formats::ui::layout_text(font, px, st.flags, t, (st.color0, st.color1));
                    (lay.width as f32, lay.height as f32)
                }
                _ => (
                    gd_formats::text::strip_markup(t).chars().count() as f32 * px as f32 * 0.5,
                    px as f32 * 1.2,
                ),
            }
        };
        if l.text.is_empty() {
            // A blank spacer line (module doc of [`Blank`]).
            let h = match blank {
                Blank::DefaultLine => measure("A").1,
                Blank::DescriptionLine => description_h.max(measure("A").1),
            };
            rows.push((None, String::new(), h, None, 0.0));
            continue;
        }
        for (i, part) in super::text::wrap_by(&l.text, col_w - indent, |t| measure(t).0)
            .into_iter()
            .enumerate()
        {
            let (pw, ph) = measure(&part);
            widest = widest.max(pw + indent);
            let h = ph.max(px as f32 * 1.1);
            if l.class == gd_sim::skill_tooltip::class::SKILL_DESCRIPTION {
                description_h = h;
            }
            rows.push((
                style_path.clone(),
                part,
                h,
                if i == 0 { l.icon } else { None },
                indent,
            ));
        }
    }
    let total_h: f32 = rows.iter().map(|r| r.2).sum();
    let text_w = match wrap {
        Wrap::Fixed => col_w,
        Wrap::Sample if widest > 0.0 => widest,
        Wrap::Sample => col_w,
    };
    let (w, h) = (text_w + 2.0 * pad, total_h + 2.0 * pad);
    let (x, y) = place(w, h, ui.screen, ui.sc(6.0));
    ui.solid(URect::new(x, y, w, h), Color::srgba(0.0, 0.0, 0.0, 0.82));
    let mut cy = y + pad;
    for (style, text, rh, icon, indent) in rows {
        let mut tx = x + pad + indent;
        if let Some(a) = icon
            && let Some(path) =
                engine.and_then(|r| r.str(&format!("affinity{}Icon", a + 1)).map(str::to_owned))
            && let Some((_, r)) = ui.bitmap(&path, 0.0, 0.0, Vec2::new(tx, cy))
        {
            tx += r.w + ui.sc(3.0);
        }
        if let Some(st) = style {
            let tb = TextBox {
                style: None,
                rect: URect::new(0.0, 0.0, col_w / s, rh / s),
                align_x: gd_formats::ui::TextAlign::Start,
                align_y: gd_formats::ui::TextAlign::Start,
                tag: None,
                glow: None,
            };
            ui.text_with_style(&tb, &st, Vec2::new(tx, cy), &text);
        }
        cy += rh;
    }
}

/// Everything a tooltip system needs to put a tooltip on screen: the window, the data and font caches, and the overlay roots.
/// `clear` removes an owner's tooltip, `show` draws one (clearing the owner's old one first).
#[derive(SystemParam)]
pub struct TipCanvas<'w, 's> {
    commands: Commands<'w, 's>,
    window: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    pub data: Res<'w, GameData>,
    pub assets: super::UiAssets<'w>,
    roots: Query<'w, 's, (Entity, &'static TipRoot, Option<&'static TipDelay>)>,
    pause: Res<'w, gd_game_core::pause::GamePause>,
}

impl<'w, 's> TipCanvas<'w, 's> {
    /// The window's physical size, `None` when there is no window or it has no area.
    pub fn size(&self) -> Option<UVec2> {
        let win = self.window.single().ok()?;
        let size = UVec2::new(win.physical_width(), win.physical_height());
        (size.x > 0 && size.y > 0).then_some(size)
    }

    /// Removes `owner`'s tooltip.
    pub fn clear(&mut self, owner: TipOwner) {
        for (e, r, _) in &self.roots {
            if r.0 == owner {
                self.commands.entity(e).try_despawn();
            }
        }
    }

    /// Replaces `owner`'s tooltip with `lines` placed by `place` (see [`draw_wrapped`]).
    pub fn show(
        &mut self,
        owner: TipOwner,
        lines: &[gd_sim::skill_tooltip::TextLine],
        blank: Blank,
        place: impl FnOnce(f32, f32, Vec2, f32) -> (f32, f32),
    ) {
        self.show_wrapped(owner, lines, blank, Wrap::Fixed, place);
    }

    /// [`Self::show`] with a [`Wrap`] rule.
    pub fn show_wrapped(
        &mut self,
        owner: TipOwner,
        lines: &[gd_sim::skill_tooltip::TextLine],
        blank: Blank,
        wrap: Wrap,
        place: impl FnOnce(f32, f32, Vec2, f32) -> (f32, f32),
    ) {
        // The same object keeps its counter (module doc); the old root is still there until the commands apply.
        let object = lines.first().map(|l| l.text.clone()).unwrap_or_default();
        let ms = self
            .roots
            .iter()
            .find_map(|(_, r, d)| d.filter(|d| r.0 == owner && d.object == object))
            .map_or(0.0, |d| d.ms);
        self.clear(owner);
        let Ok(win) = self.window.single() else {
            return;
        };
        let Some(size) = self.size() else { return };
        let delay = self
            .data
            .db
            .get(owner.style())
            .and_then(|r| r.i32("delay"))
            .unwrap_or(0) as f32;
        let wait = TipDelay::skill(object, delay).with_ms(ms);
        let visibility = shown(&wait, &self.pause);
        let root = self
            .commands
            .spawn((
                TipRoot(owner),
                wait,
                visibility,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                GlobalZIndex(1000),
            ))
            .id();
        let mut ui = self.assets.ui(
            &mut self.commands,
            &self.data,
            root,
            self.assets.scale(size),
            1.0 / win.scale_factor(),
            Vec2::new(size.x as f32, size.y as f32),
        );
        draw_wrapped(&mut ui, lines, blank, wrap, place);
    }
}
