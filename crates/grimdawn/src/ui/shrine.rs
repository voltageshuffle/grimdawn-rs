//! The devotion shrine window (`records/ui/shrine/shrine_table.dbr`, the
//! HUD master table's `hudShrineWindow`): the "Ruined Shrine" offering
//! window and, for a shrine with a monster proxy, the "Desecrated Shrine"
//! window. It shows [`gd_game::devotion::ShrineWindow`] and sends
//! [`ShrineRequest`]s; the rules (what is taken, when it closes) are in
//! `gd_game::devotion`.
//!
//! Layout, `Grim Dawn.exe` (offering window: loader 0x1401d2640,
//! update 0x1401d1da0, draw 0x1401d16e0, button 0x1401d3900; proxy window:
//! loader 0x1401d4030, buttons 0x1401d45c0):
//! - the record's window rect (346 × 715, `Left` / `Center`); everything is
//!   placed from its top-left corner;
//! - draw order: background (`shrineRuinedBitmap` / `shrineCorruptedBitmap`,
//!   at its `bitmapPosition`), title (`shrineTitle` "Ruined Shrine" /
//!   `shrineCorruptedTitle` "Desecrated Shrine"), info box (`shrineInfo`,
//!   wrapped and centred, `tagShrineWarning` / `tagShrineConfirmProxy`),
//!   the three offering boxes (`shrineBox1..3`, the offering's item in each),
//!   a hole cover (`shrineCover1..3`) over each slot without an offering,
//!   then the shrine button, close button and cancel button;
//! - the shrine button reads "Offer" (its `textTag`) and is disabled until
//!   every offering slot is filled; on the proxy window it reads
//!   `tagShrineButton02` ("Start"); cancel and close hide the window.
//!
//! The shrine button has no rollover: the window reads no
//! `shrineButtonRollover` (the record's field; the name is not even
//! a string in `Grim Dawn.exe`, the loaders 0x1401d2640 / 0x1401d4030 read
//! only the fields above and `shrineSound`).
//!
//! Offering boxes: each is an `enchanteritembox.tpl` item box (class ctor
//! 0x1401107b0 on the base 0x1401ad920, loader 0x1401add00, draw 0x1401ae4d0,
//! update 0x1401af080). The window's update (0x1401d1da0) gives every filled
//! slot the shrine's own display item (`StaticShrine` +432..434) and sets the
//! box's "have" flag (+97) from the player's count (inventory and stash,
//! `gd_game::devotion::offering_slots`); the box update raises the "fails
//! requirements" flag (+96) for an item the player lacks. Draw: the
//! box rect (`itemX/Y`, `itemX/YSize`) is first filled with the box's
//! `highlightTexture` (default `ui/character/itembackground.tex`) tinted
//! `failsRequirementsColor` while the item is missing, else
//! `backgroundShadeColor` (alpha 0 in the shrine boxes: nothing); then the item's
//! rarity plate (its `gameiteminfo` class colour) unless `disableItemBackground`,
//! then its icon.
//!
//! Sounds: the buttons are a push group (0x14012ab70): a left press
//! on an enabled button plays its `soundNameDown` and the window acts at once
//! (listener 0x1401d24e0, event 0); a disabled button does nothing. Offer
//! (0x1401d3900) then plays `shrineSound` too, as it sends the cleanse; the
//! proxy window's Start (0x1401d45c0) plays nothing more. Both unpositioned.
//! The rarity plate fills the item graphic's rect, the icon's own (plate draw 0x1401effb0 and icon draw
//! 0x1401efe20 both use the graphic's +108 rect, centred on its position, the whole texture stretched into it).
use super::contains;
use super::window::Rebuild;
use crate::data::GameData;
use crate::inventory::{Inventory, UiCapture};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::save::item::Item;
use gd_formats::ui::{Rect as URect, TextBox, WindowDef};
use gd_game::devotion::{Shrine, ShrineRequest, ShrineWindow, offering_slots};

pub const RECORD: &str = "records/ui/shrine/shrine_table.dbr";

pub struct ShrineUiPlugin;

impl Plugin for ShrineUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShrineLayout>();
        super::window::add_window(
            app,
            build,
            clicks,
            |open: Res<ShrineWindow>, layout: Res<ShrineLayout>| {
                open.shrine.is_some() || layout.window.is_some()
            },
        );
        app.add_systems(
            Update,
            live.after(build)
                .in_set(super::UiPlaySet)
                .run_if(|l: Res<ShrineLayout>| l.window.is_some()),
        );
    }
}

/// A clickable part of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Accept,
    Cancel,
    Close,
}

/// The built window (physical px). Empty while no shrine window is open.
#[derive(Resource, Default, Clone, PartialEq)]
pub struct ShrineLayout {
    pub window: Option<URect>,
    /// Buttons (enabled ones only).
    pub buttons: Vec<(URect, Part)>,
    pub hover: Option<Part>,
    /// The offering boxes and their items.
    pub boxes: Vec<(URect, String)>,
    /// The offering under the cursor (its tooltip, `tooltip.rs`).
    pub hover_item: Option<Item>,
}

#[derive(Component)]
struct ShrineRoot;

/// A button quad: its part and whether it is enabled (hover look swapped in place by [`live`]).
#[derive(Component)]
struct ShrineButton(Part, bool);

#[allow(clippy::too_many_arguments)]
fn build(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    open: Res<ShrineWindow>,
    shrines: Query<&Shrine>,
    inv: Res<Inventory>,
    stash: Option<Res<gd_game::stash::Stash>>,
    roots: Query<Entity, With<ShrineRoot>>,
    mut layout: ResMut<ShrineLayout>,
    mut fresh: gd_game::session::NewSession,
    mut built: Local<Rebuild<(u64, UVec3, [Option<bool>; 3])>>,
    // The offering slots and the (window, inventory) versions they were read at.
    mut slots_at: Local<(Option<(u64, u64)>, [Option<(String, bool)>; 3])>,
    mut assets: crate::ui::UiAssets,
) {
    if fresh.begun() {
        built.reset();
        *slots_at = Default::default();
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let shrine = open.shrine.and_then(|e| shrines.get(e).ok());
    let key = (open.version, inv.version);
    if slots_at.0 != Some(key) || stash.as_ref().is_some_and(|s| s.is_changed()) {
        slots_at.0 = Some(key);
        slots_at.1 = match shrine {
            Some(sh) if !open.proxy => offering_slots(sh, &inv, stash.as_deref()),
            _ => [None, None, None],
        };
    }
    let slots = &slots_at.1;
    let sig = (
        open.version,
        // The UI scale too (an applied UI Scale rebuilds).
        size.extend(assets.scale(size).to_bits()),
        slots.each_ref().map(|s| s.as_ref().map(|s| s.1)),
    );
    if size.x == 0 || !built.changed(sig) {
        return;
    }
    super::window::despawn_all(&mut commands, &roots);
    *layout = ShrineLayout::default();
    let (Some(_), Some(rec)) = (shrine, data.db.get(RECORD)) else {
        return;
    };
    let s = assets.scale(size);
    let root = crate::ui::spawn_root(&mut commands, ShrineRoot, super::window::Z_WINDOW);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        s,
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    let field = |k: &str| rec.str(k).unwrap_or("").to_string();
    let r = WindowDef::from_record(rec).screen_rect(ui.screen.x, ui.screen.y, s);
    let o = Vec2::new(r.x, r.y);
    // The window takes its clicks, so no HUD button under it plays a sound (as the altar does).
    ui.sound_blocker(r);
    let proxy = open.proxy;
    // Background.
    if let Some(bg) = ui.record(&field(if proxy {
        "shrineCorruptedBitmap"
    } else {
        "shrineRuinedBitmap"
    })) {
        let f = |k: &str| bg.f32(k).unwrap_or(0.0);
        if let Some(path) = bg.str("bitmapName") {
            ui.bitmap(path, f("bitmapPositionX"), f("bitmapPositionY"), o);
        }
    }
    // Title.
    if let Some(t) = ui.record(&field(if proxy {
        "shrineCorruptedTitle"
    } else {
        "shrineTitle"
    })) {
        let tb = TextBox::from_record(&t);
        let text = tb
            .tag
            .as_deref()
            .map(|t| ui.fonts.tag(t).to_string())
            .unwrap_or_default();
        ui.text(&tb, o, &text);
    }
    // Info: wrapped in the box, centred both ways.
    if let Some(t) = ui.record(&field("shrineInfo")) {
        let tb = TextBox::from_record(&t);
        let text = ui
            .fonts
            .tag(if proxy {
                "tagShrineConfirmProxy"
            } else {
                "tagShrineWarning"
            })
            .to_string();
        if let Some(style) = tb.style.as_deref() {
            let (bx, by, bw, bh) = (
                o.x + ui.sc(tb.rect.x),
                o.y + ui.sc(tb.rect.y),
                ui.sc(tb.rect.w),
                ui.sc(tb.rect.h),
            );
            let lines = ui.wrap_lines(style, bw, &text);
            let lh = ui.line_height(style);
            let mut y = (by + (bh - lines.len() as f32 * lh) * 0.5).round();
            for l in &lines {
                super::trade::label(&mut ui, URect::new(bx, y, bw, lh), style, tb.align_x, l);
                y += lh;
            }
        }
    }
    // Offering boxes, then covers over the empty slots.
    if !proxy {
        for (i, slot) in slots.iter().enumerate() {
            let n = i + 1;
            let Some((item, have)) = slot else {
                if let Some(c) = ui.record(&field(&format!("shrineCover{n}"))) {
                    let f = |k: &str| c.f32(k).unwrap_or(0.0);
                    if let Some(path) = c.str("bitmapName") {
                        ui.bitmap(path, f("bitmapPositionX"), f("bitmapPositionY"), o);
                    }
                }
                continue;
            };
            let Some(b) = ui.record(&field(&format!("shrineBox{n}"))) else {
                continue;
            };
            let f = |k: &str| b.f32(k).unwrap_or(0.0);
            let br = URect::new(
                o.x + ui.sc(f("itemX")),
                o.y + ui.sc(f("itemY")),
                ui.sc(f("itemXSize")),
                ui.sc(f("itemYSize")),
            );
            // The box's background: `highlightTexture` tinted by the missing look (module doc).
            if !have {
                let tex = b
                    .str("highlightTexture")
                    .unwrap_or("ui/character/itembackground.tex")
                    .to_owned();
                if let Some((h, _)) = ui.image(&tex) {
                    ui.quad_stretch(
                        h,
                        br,
                        Color::srgba(
                            f("failsRequirementsColorRed"),
                            f("failsRequirementsColorGreen"),
                            f("failsRequirementsColorBlue"),
                            f("failsRequirementsColorAlpha"),
                        ),
                    );
                }
            }
            let plate = !b.bool("disableItemBackground");
            let it = Item {
                base_name: item.clone(),
                stack_count: 1,
                var1: 1,
                ..Default::default()
            };
            if plate {
                rarity_plate(&mut ui, &inv, &it, br);
            }
            super::character::item_icon(&mut ui, &inv, &it, br);
            layout.boxes.push((br, item.clone()));
        }
    }
    // Buttons.
    let ready = proxy || slots.iter().flatten().all(|(_, have)| *have);
    if let Some(b) = ui.record(&field("shrineButton")) {
        let text = if proxy {
            Some("tagShrineButton02")
        } else {
            b.str("textTag")
        }
        .map(|t| ui.fonts.tag(t).to_string());
        let label = text
            .as_deref()
            .map_or(super::widgets::Label::None, super::widgets::Label::Text);
        if let Some(br) = live_button(&mut ui, &b, o, Part::Accept, ready, label) {
            layout.buttons.push((br, Part::Accept));
        }
    }
    if let Some(b) = ui.record(&field("closeButton"))
        && let Some(br) = live_button(
            &mut ui,
            &b,
            o,
            Part::Close,
            true,
            super::widgets::Label::None,
        )
    {
        layout.buttons.push((br, Part::Close));
    }
    if let Some(b) = ui.record(&field("cancelButton")) {
        let text = b.str("textTag").map(|t| ui.fonts.tag(t).to_string());
        let label = text
            .as_deref()
            .map_or(super::widgets::Label::None, super::widgets::Label::Text);
        if let Some(br) = live_button(&mut ui, &b, o, Part::Cancel, true, label) {
            layout.buttons.push((br, Part::Cancel));
        }
    }
    layout.window = Some(r);
}

/// A button drawn for [`live`] hover; its rect when enabled.
fn live_button(
    ui: &mut super::Ui,
    b: &gd_formats::arz::Record,
    o: Vec2,
    part: Part,
    enabled: bool,
    label: super::widgets::Label,
) -> Option<URect> {
    let (br, e) = super::window::static_button_live(ui, b, o, enabled, label)?;
    ui.commands.entity(e).insert(ShrineButton(part, enabled));
    enabled.then_some(br)
}

/// Hover in place (no rebuild): the button bitmaps, [`ShrineLayout::hover`] and the hovered offering's item.
fn live(
    hooks: Res<crate::hooks::Hooks>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut layout: ResMut<ShrineLayout>,
    pause: Res<gd_game::pause::GamePause>,
    mut buttons: Query<(&ShrineButton, &super::window::ButtonLook, &mut ImageNode)>,
) {
    let Ok(win) = window.single() else { return };
    // The game menu / Options over the window takes the pointer.
    let p = super::cursor_px(&hooks, win).filter(|_| !pause.menu);
    let hover = p.and_then(|p| {
        layout
            .buttons
            .iter()
            .find(|(r, _)| contains(r, p))
            .map(|b| b.1)
    });
    let hover_box = p.and_then(|p| layout.boxes.iter().position(|(r, _)| contains(r, p)));
    for (b, l, mut img) in &mut buttons {
        super::window::set_button_look(l, &mut img, b.1, hover == Some(b.0));
    }
    if layout.hover != hover {
        layout.hover = hover;
    }
    let rec = hover_box
        .and_then(|i| layout.boxes.get(i))
        .map(|b| b.1.as_str());
    if layout.hover_item.as_ref().map(|i| i.base_name.as_str()) != rec {
        let item = rec.map(|rec| Item {
            base_name: rec.to_owned(),
            stack_count: 1,
            var1: 1,
            ..Default::default()
        });
        layout.hover_item = item;
    }
}

/// Clicks on the window: the shrine button accepts, cancel / close cancel (Esc through the shared
/// dispatcher, `ui::esc_dispatcher`); the window takes the click (no walking). `GD_SHRINE_ACCEPT` (hooks.rs)
/// presses the shrine button once a window has been open that long.
#[allow(clippy::too_many_arguments)]
pub fn clicks(
    hooks: Res<crate::hooks::Hooks>,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    layout: Res<ShrineLayout>,
    open: Res<ShrineWindow>,
    time: Res<Time>,
    mut capture: ResMut<UiCapture>,
    mut out: MessageWriter<ShrineRequest>,
    mut fresh: gd_game::session::NewSession,
    mut opened: Local<Option<(u64, f32)>>,
    data: Res<GameData>,
    mut sound: MessageWriter<gd_game::visuals::PlaySound>,
) {
    if fresh.begun() {
        *opened = Default::default();
    }
    let Some(wr) = layout.window else {
        *opened = None;
        return;
    };
    // `GD_SHRINE_ACCEPT`: press the shrine button (when enabled) once per window.
    if let Some(secs) = hooks.ui.shrine_accept {
        let since = opened.get_or_insert((open.version, time.elapsed_secs()));
        if since.0 != open.version {
            *since = (open.version, time.elapsed_secs());
        } else if since.1 >= 0.0 && time.elapsed_secs() - since.1 >= secs {
            since.1 = -1.0;
            info!(
                "shrine test: pressing the shrine button ({})",
                if layout.buttons.iter().any(|b| b.1 == Part::Accept) {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            if layout.buttons.iter().any(|b| b.1 == Part::Accept) {
                out.write(ShrineRequest::Accept);
            }
        }
    }
    // Esc closes the window through the shared dispatcher (`ui::esc_dispatcher`).
    let Ok(win) = window.single() else { return };
    let Some(p) = super::cursor_px(&hooks, win) else {
        return;
    };
    let hit = layout
        .buttons
        .iter()
        .find(|(r, _)| contains(r, p))
        .map(|b| b.1);
    if !contains(&wr, p) && hit.is_none() {
        return;
    }
    capture.0 = true;
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    if let Some(part) = hit
        && let Some(rec) = data.db.get(RECORD)
    {
        for pak in press_sounds(rec, part, open.proxy, |b| {
            data.db
                .get(b)
                .and_then(|r| r.str("soundNameDown").map(str::to_owned))
        }) {
            sound.write(gd_game::visuals::PlaySound { pak, at: None });
        }
    }
    match hit {
        Some(Part::Accept) => {
            out.write(ShrineRequest::Accept);
        }
        Some(Part::Cancel | Part::Close) => {
            out.write(ShrineRequest::Cancel);
        }
        None => {}
    }
}

/// The item's rarity plate (`gameiteminfo.dbr` `<class>ItemBackground` tinted `<class>ItemColor`) over the
/// rect `character::item_icon` draws the icon in (module doc).
pub(crate) fn rarity_plate(ui: &mut super::Ui, inv: &Inventory, item: &Item, r: URect) {
    if let Some(info) = inv.info(item)
        && let Some(key) = super::character::rarity_key(info.resolved.display_color_index())
        && let Some(rec) = ui.data.db.get("records/game/gameiteminfo.dbr")
        && let Some((_, icon)) = ui.image(&info.resolved.bitmap)
    {
        let c = |k: &str| rec.f32(&format!("{key}ItemColor.{k}")).unwrap_or(0.0);
        let color = Color::srgba(c("r"), c("g"), c("b"), c("a"));
        let tex = rec
            .str(&format!("{key}ItemBackground"))
            .unwrap_or("ui/character/itembackground.tex")
            .to_owned();
        let (w, h) = (ui.sc(icon.x as f32), ui.sc(icon.y as f32));
        let d = URect::new(
            r.x + ((r.w - w) * 0.5).trunc(),
            r.y + ((r.h - h) * 0.5).trunc(),
            w,
            h,
        );
        if c("a") > 0.0
            && let Some((t, _)) = ui.image(&tex)
        {
            ui.quad_stretch(t, d, color);
        }
    }
}

/// The sounds of a left press on `part` (module doc): the button record's `soundNameDown` (looked up by `down`),
/// then, for Offer on the offering window, `shrineSound`.
fn press_sounds(
    rec: &gd_formats::arz::Record,
    part: Part,
    proxy: bool,
    down: impl Fn(&str) -> Option<String>,
) -> Vec<String> {
    let field = match part {
        Part::Accept => "shrineButton",
        Part::Cancel => "cancelButton",
        Part::Close => "closeButton",
    };
    let mut out: Vec<String> = rec.str(field).and_then(&down).into_iter().collect();
    if part == Part::Accept
        && !proxy
        && let Some(s) = rec.str("shrineSound").filter(|s| !s.is_empty())
    {
        out.push(s.to_owned());
    }
    out
}
