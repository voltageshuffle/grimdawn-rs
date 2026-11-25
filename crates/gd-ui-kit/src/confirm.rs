//! The Yes / No (or Ok) confirmation box of the dialog manager (`prompt_manager.dbr`), centred on the
//! screen; each window only chooses the question, tracks which part is hovered, and reads the
//! returned rects.
//!
//! PORT: the box texture's top-right black square is taken as the slot of
//! `prompt_buttonwindowclose.dbr`; a click answers No (the engine's close handling was not decompiled).
use super::Ui;
use bevy::prelude::*;
use gd_formats::ui::{Rect as URect, TextAlign, TextBox};
use gd_game_core::data::GameData;

const PROMPT: &str = "records/ui/generic/promptbox/prompt_manager.dbr";
const PROMPT_TEXT: &str = "records/ui/generic/promptbox/prompt_text.dbr";
const CLOSE: &str = "records/ui/generic/promptbox/prompt_buttonwindowclose.dbr";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Yes,
    No,
    Close,
}

/// The hovered part and whether the left button is down on it (the close button's down bitmap).
pub type Look = (Option<Hit>, bool);

#[derive(Clone, Copy)]
pub struct ConfirmRects {
    pub yes: URect,
    pub no: URect,
    pub close: Option<URect>,
}

impl ConfirmRects {
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

    pub fn look(&self, cursor: Option<Vec2>, pressed: bool) -> Look {
        let hit = cursor.and_then(|p| self.hit(p));
        (hit, hit.is_some() && pressed)
    }
}

/// A box button's state bitmaps: up, `bitmapNameInFocus` hovered, `bitmapNameDown` pressed (close).
#[derive(Component)]
pub struct ConfirmLook {
    /// One box per root.
    pub root: Entity,
    pub hit: Hit,
    up: Handle<Image>,
    over: Option<Handle<Image>>,
    down: Option<Handle<Image>>,
}

/// Shows the box's [`Look`] in place (no rebuild), on the buttons of `root` only.
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

/// A box button (`ButtonStaticText`) with its top-left at `pos`, with [`ConfirmLook`] on its quad.
fn box_button(
    ui: &mut Ui,
    rec: &gd_formats::arz::Record,
    pos: Vec2,
    hit: Hit,
    look: Look,
) -> Option<URect> {
    let key = match look {
        (Some(h), true) if h == hit && rec.str("bitmapNameDown").is_some() => "bitmapNameDown",
        (Some(h), _) if h == hit && rec.str("bitmapNameInFocus").is_some() => "bitmapNameInFocus",
        _ => "bitmapNameUp",
    };
    let (img, size) = rec.str(key).map(str::to_owned).and_then(|p| ui.image(&p))?;
    let text = rec.str("textTag").map(|t| ui.fonts.tag(t).to_string());
    let tex_w = ui.sc(size.x as f32);
    let w = super::widgets::button_width(ui, rec, tex_w, text.as_deref());
    let r = URect::new(pos.x, pos.y, w, ui.sc(size.y as f32));
    let e = if w > tex_w {
        ui.quad_stretch(img.clone(), r, Color::WHITE)
    } else {
        ui.quad(img.clone(), r, None, Color::WHITE)
    };
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
    if let (Some(style), Some(t)) = (rec.str("style"), text) {
        super::widgets::label(ui, r, style, TextAlign::Center, &t);
    }
    // The close button's sound is its handler's ([`close_sound`]).
    if hit != Hit::Close {
        ui.record_button_sound(r, rec);
    }
    Some(r)
}

pub fn close_sound(data: &GameData) -> Option<String> {
    data.db.get(CLOSE)?.str("soundNameDown").map(str::to_owned)
}

/// The box's frame, question, close slot and buttons; returns the button rects (in order) and the
/// close rect. PORT: the stock box is fixed at its texture's size; here it grows to fit the
/// question and the button labels, and the button row is centred (the stock Ok sits 2 px left).
fn frame(
    ui: &mut Ui,
    text: &str,
    look: Look,
    buttons: &[(&str, Hit)],
) -> Option<(Vec<URect>, Option<URect>)> {
    let rec = ui.record(PROMPT)?;
    let (bg, sz) = rec
        .str("Background")
        .map(str::to_owned)
        .and_then(|p| ui.image(&p))?;
    let (w0, h0) = (ui.sc(sz.x as f32), ui.sc(sz.y as f32));
    struct Btn {
        rec: gd_formats::arz::Record,
        hit: Hit,
        w: f32,
        h: f32,
        y: f32,
    }
    let mut btns = Vec::new();
    for (field, hit) in buttons {
        let b = rec
            .str(field)
            .map(str::to_owned)
            .and_then(|p| ui.record(&p))?;
        let (_, bsz) = b
            .str("bitmapNameUp")
            .map(str::to_owned)
            .and_then(|p| ui.image(&p))?;
        let label = b.str("textTag").map(|t| ui.fonts.tag(t).to_string());
        let w = super::widgets::button_width(ui, &b, ui.sc(bsz.x as f32), label.as_deref());
        let y = ui.sc(b.f32("bitmapPositionY").unwrap_or(0.0));
        btns.push(Btn {
            rec: b,
            hit: *hit,
            w,
            h: ui.sc(bsz.y as f32),
            y,
        });
    }
    let tb = ui.record(PROMPT_TEXT).map(|t| TextBox::from_record(&t));
    let mut grow = 0.0;
    if let Some(tb) = &tb
        && let Some(style) = tb.style.as_deref()
    {
        let bw = ui.sc(tb.rect.w);
        let text_h = ui.wrap_lines(style, bw, text).len() as f32 * ui.line_height(style);
        grow = (text_h - ui.sc(tb.rect.h)).max(0.0);
    }
    // Grow the box width when the row exceeds the texture (stock side margin).
    let gap = ui.sc(4.0);
    let row_w: f32 =
        btns.iter().map(|b| b.w).sum::<f32>() + gap * btns.len().saturating_sub(1) as f32;
    let margin = ui.sc(21.0);
    let w = w0.max(row_w + 2.0 * margin);
    let h = h0 + grow;
    let o = Vec2::new(
        ((ui.screen.x - w) * 0.5).round(),
        ((ui.screen.y - h) * 0.5).round(),
    );
    ui.quad_stretch(bg, URect::new(o.x, o.y, w, h), Color::WHITE);
    // Block every button behind the question, full screen.
    ui.sound_blocker(URect::new(0.0, 0.0, ui.screen.x, ui.screen.y));
    // Close: keeps its record offset from the right edge as the box widens.
    let close = ui.record(CLOSE).and_then(|c| {
        let cx = ui.sc(c.f32("bitmapPositionX").unwrap_or(0.0));
        let cy = ui.sc(c.f32("bitmapPositionY").unwrap_or(0.0));
        box_button(
            ui,
            &c,
            Vec2::new(o.x + (w - w0) + cx, o.y + cy),
            Hit::Close,
            look,
        )
    });
    if let Some(tb) = &tb
        && let Some(style) = tb.style.as_deref()
    {
        let bw = ui.sc(tb.rect.w);
        let lh = ui.line_height(style);
        let mut y = o.y + ui.sc(tb.rect.y);
        for l in ui.wrap_lines(style, bw, text) {
            super::widgets::label(
                ui,
                URect::new(o.x + ui.sc(tb.rect.x), y, bw, lh),
                style,
                tb.align_x,
                &l,
            );
            y += lh;
        }
    }
    // Anchored to the stock bottom margin (pushed down by `grow`).
    let btn_h = btns.first().map(|b| b.h).unwrap_or(0.0);
    let bottom = btns.first().map(|b| h0 - b.y - b.h).unwrap_or(0.0);
    let by = o.y + h - bottom - btn_h;
    let mut x = o.x + (w - row_w) * 0.5;
    let mut rects = Vec::with_capacity(btns.len());
    for b in &btns {
        rects.push(box_button(ui, &b.rec, Vec2::new(x, by), b.hit, look)?);
        x += b.w + gap;
    }
    Some((rects, close))
}

/// The message box with its one Ok button ([`Look`]'s `Yes`): returns the Ok and close rects.
pub fn ok_box(ui: &mut Ui, tag: &str, look: Look) -> Option<(URect, Option<URect>)> {
    let text = ui.fonts.tag(tag).to_string();
    ok_text(ui, &text, look)
}

/// `text`: already localised and filled.
pub fn ok_text(ui: &mut Ui, text: &str, look: Look) -> Option<(URect, Option<URect>)> {
    let (rects, close) = frame(ui, text, look, &[("OkButton", Hit::Yes)])?;
    Some((rects.into_iter().next()?, close))
}

/// `tag` is the question's text tag; `None` when a record is missing.
pub fn confirm_box(ui: &mut Ui, tag: &str, look: Look) -> Option<ConfirmRects> {
    let text = ui.fonts.tag(tag).to_string();
    confirm_text(ui, &text, look)
}

/// `text`: already localised and filled.
pub fn confirm_text(ui: &mut Ui, text: &str, look: Look) -> Option<ConfirmRects> {
    let (rects, close) = frame(
        ui,
        text,
        look,
        &[("YesButton", Hit::Yes), ("NoButton", Hit::No)],
    )?;
    let mut it = rects.into_iter();
    Some(ConfirmRects {
        yes: it.next()?,
        no: it.next()?,
        close,
    })
}
