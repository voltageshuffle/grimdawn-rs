//! The Yes / No confirmation box of the dialog manager (`prompt_manager.dbr`), centred on the screen: the loot filter's Defaults
//! question, the inventory's reagent / sort questions (`apply.rs`), the game menu's Exit / Quit question (`gamemenu.rs`), the
//! Inventor's dismantle warning and the vendor / caravan windows' boxes (`ui/trade.rs`, `crate::trade::Prompt`) are this one box (or
//! its Ok variant); each window only chooses the question, tracks which part is
//! hovered, and reads the returned rects.
//!
//! PORT: the box texture has a black square in its top-right corner: the slot of the window close button. `prompt_manager.dbr` has
//! no close field (only Yes / Ok / No), but its folder holds `prompt_buttonwindowclose.dbr`, which the engine's dialog window must
//! draw there; a click answers the box No and plays `soundNameDown` (how the engine answers a close button of the dialog manager
//! was not decompiled). The Yes / No buttons show `bitmapNameInFocus` while hovered.
//!
//! The box's looks can move in place: every button quad carries [`ConfirmLook`] (its state bitmaps), and
//! [`apply_look`] swaps them per frame — a window whose box is its own rebuild group (the loot filter's
//! Defaults question, the reclaim mode's message) keeps the hover / press out of its signature.
use super::Ui;
use crate::data::GameData;
use bevy::prelude::*;
use gd_formats::ui::{Rect as URect, TextAlign, TextBox};

const PROMPT: &str = "records/ui/generic/promptbox/prompt_manager.dbr";
const PROMPT_TEXT: &str = "records/ui/generic/promptbox/prompt_text.dbr";
const CLOSE: &str = "records/ui/generic/promptbox/prompt_buttonwindowclose.dbr";

/// A part of the box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Yes,
    No,
    Close,
}

/// What the box shows of the mouse: the hovered part and whether the left button is down on it (the close button's down bitmap).
pub type Look = (Option<Hit>, bool);

/// The box's parts (physical px) as drawn.
#[derive(Clone, Copy)]
pub struct ConfirmRects {
    pub yes: URect,
    pub no: URect,
    pub close: Option<URect>,
}

impl ConfirmRects {
    /// The part under `p`.
    pub fn hit(&self, p: Vec2) -> Option<Hit> {
        if super::contains(&self.yes, p) {
            Some(Hit::Yes)
        } else if super::contains(&self.no, p) {
            Some(Hit::No)
        } else if self.close.is_some_and(|r| super::contains(&r, p)) {
            Some(Hit::Close)
        } else {
            None
        }
    }

    /// The [`Look`] for the cursor (`pressed`: the left button is down).
    pub fn look(&self, cursor: Option<Vec2>, pressed: bool) -> Look {
        let hit = cursor.and_then(|p| self.hit(p));
        (hit, hit.is_some() && pressed)
    }
}

/// A box button's state bitmaps on its quad ([`apply_look`] swaps them in place): the up bitmap as drawn,
/// `bitmapNameInFocus` hovered, `bitmapNameDown` hovered and pressed (the close button).
#[derive(Component)]
pub struct ConfirmLook {
    /// The box root it belongs to (one box per root; the game menu draws one per look layer).
    pub root: Entity,
    pub hit: Hit,
    up: Handle<Image>,
    over: Option<Handle<Image>>,
    down: Option<Handle<Image>>,
}

/// Shows the box's [`Look`] in place (no rebuild): the hovered part's focus bitmap, the close button's
/// down bitmap while pressed. Only the buttons of `root` (the caller's box).
pub fn apply_look(root: Entity, look: Look, looks: &mut Query<(&ConfirmLook, &mut ImageNode)>) {
    for (l, mut img) in looks.iter_mut() {
        if l.root != root {
            continue;
        }
        let want = match look {
            (Some(h), true) if h == l.hit && l.down.is_some() => l.down.as_ref(),
            (Some(h), _) if h == l.hit && l.over.is_some() => l.over.as_ref(),
            _ => Some(&l.up),
        };
        if let Some(h) = want
            && img.image != *h
        {
            img.image = h.clone();
        }
    }
}

/// A box button (`ButtonStaticText`): its bitmap by the look (as `widgets::static_button`, always
/// enabled), the label centred, its record sound — and [`ConfirmLook`] on the quad for [`apply_look`].
/// (The box needs the quad entity to swap its bitmap in place; `static_button` returns only the rect.)
fn box_button(
    ui: &mut Ui,
    rec: &gd_formats::arz::Record,
    o: Vec2,
    hit: Hit,
    look: Look,
) -> Option<URect> {
    let key = match look {
        (Some(h), true) if h == hit && rec.str("bitmapNameDown").is_some() => "bitmapNameDown",
        (Some(h), _) if h == hit && rec.str("bitmapNameInFocus").is_some() => "bitmapNameInFocus",
        _ => "bitmapNameUp",
    };
    let (img, size) = rec.str(key).map(str::to_owned).and_then(|p| ui.image(&p))?;
    let r = URect::new(
        o.x + ui.sc(rec.f32("bitmapPositionX").unwrap_or(0.0)),
        o.y + ui.sc(rec.f32("bitmapPositionY").unwrap_or(0.0)),
        ui.sc(size.x as f32),
        ui.sc(size.y as f32),
    );
    let e = ui.quad(img.clone(), r, None, Color::WHITE);
    let load = |ui: &mut Ui, k: &str| {
        rec.str(k)
            .map(str::to_owned)
            .and_then(|p| ui.image(&p))
            .map(|(h, _)| h)
    };
    let over = load(ui, "bitmapNameInFocus");
    let down = load(ui, "bitmapNameDown");
    ui.commands.entity(e).insert(ConfirmLook {
        root: ui.root,
        hit,
        up: img,
        over,
        down,
    });
    if let (Some(style), Some(tag)) = (rec.str("style"), rec.str("textTag")) {
        let t = ui.fonts.tag(tag).to_string();
        super::trade::label(ui, r, style, TextAlign::Center, &t);
    }
    // Yes / No sound on the press; the close button's sound is its handler's ([`close_sound`]).
    if hit != Hit::Close {
        ui.record_button_sound(r, rec);
    }
    Some(r)
}

/// The close button's press sound (`soundNameDown` of its record).
pub fn close_sound(data: &GameData) -> Option<String> {
    data.db.get(CLOSE)?.str("soundNameDown").map(str::to_owned)
}

/// The window close button in the box's top-right slot, its bitmap by the [`Look`] (up / `bitmapNameInFocus` / `bitmapNameDown`).
fn close_slot(ui: &mut Ui, o: Vec2, look: Look) -> Option<URect> {
    let b = ui.record(CLOSE)?;
    box_button(ui, &b, o, Hit::Close, look)
}

/// The box's frame, `text` and close slot: its record, its origin and the close rect.
fn frame(
    ui: &mut Ui,
    text: &str,
    look: Look,
) -> Option<(gd_formats::arz::Record, Vec2, Option<URect>)> {
    let rec = ui.record(PROMPT)?;
    let (bg, sz) = rec
        .str("Background")
        .map(str::to_owned)
        .and_then(|p| ui.image(&p))?;
    let (w, h) = (ui.sc(sz.x as f32), ui.sc(sz.y as f32));
    let o = Vec2::new(
        ((ui.screen.x - w) * 0.5).round(),
        ((ui.screen.y - h) * 0.5).round(),
    );
    ui.quad(bg, URect::new(o.x, o.y, w, h), None, Color::WHITE);
    // The question takes every click while up: block the sounds (and `frontmost` presses) of every
    // button behind it, full screen, not only under the box.
    ui.sound_blocker(URect::new(0.0, 0.0, ui.screen.x, ui.screen.y));
    let close = close_slot(ui, o, look);
    if let Some(t) = ui.record(PROMPT_TEXT) {
        let tb = TextBox::from_record(&t);
        if let Some(style) = tb.style.as_deref() {
            let bw = ui.sc(tb.rect.w);
            let lh = ui.line_height(style);
            let mut y = o.y + ui.sc(tb.rect.y);
            for l in ui.wrap_lines(style, bw, text) {
                super::trade::label(
                    ui,
                    URect::new(o.x + ui.sc(tb.rect.x), y, bw, lh),
                    style,
                    tb.align_x,
                    &l,
                );
                y += lh;
            }
        }
    }
    Some((rec, o, close))
}

/// Draws the message box with its one Ok button (the `OkButton` of `prompt_manager.dbr`; [`Look`]'s `Yes` is the
/// Ok button): returns the Ok and close rects.
pub fn ok_box(ui: &mut Ui, tag: &str, look: Look) -> Option<(URect, Option<URect>)> {
    let text = ui.fonts.tag(tag).to_string();
    ok_text(ui, &text, look)
}

/// [`ok_box`] showing `text` (a tag already localised and filled).
pub fn ok_text(ui: &mut Ui, text: &str, look: Look) -> Option<(URect, Option<URect>)> {
    let (rec, o, close) = frame(ui, text, look)?;
    let b = rec
        .str("OkButton")
        .map(str::to_owned)
        .and_then(|p| ui.record(&p))?;
    let ok = box_button(ui, &b, o, Hit::Yes, look)?;
    Some((ok, close))
}

/// Draws the box: `tag` is the question's text tag; returns the rects of its parts (None when a record is missing).
pub fn confirm_box(ui: &mut Ui, tag: &str, look: Look) -> Option<ConfirmRects> {
    let text = ui.fonts.tag(tag).to_string();
    confirm_text(ui, &text, look)
}

/// [`confirm_box`] asking `text` (a tag already localised and filled).
pub fn confirm_text(ui: &mut Ui, text: &str, look: Look) -> Option<ConfirmRects> {
    let (rec, o, close) = frame(ui, text, look)?;
    let mut rects = [None, None];
    for (i, (k, hit)) in [("YesButton", Hit::Yes), ("NoButton", Hit::No)]
        .into_iter()
        .enumerate()
    {
        let b = rec.str(k).map(str::to_owned).and_then(|p| ui.record(&p))?;
        // Yes / No sound on the press (`ui/widgets.rs`); the close button's sound is its handler's ([`close_sound`]).
        rects[i] = box_button(ui, &b, o, hit, look);
    }
    Some(ConfirmRects {
        yes: rects[0]?,
        no: rects[1]?,
        close,
    })
}
