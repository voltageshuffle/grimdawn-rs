//! Widgets more than one window draws from a record.
//!
//! `ButtonStaticText` records (the game menu's and the confirmation box's buttons, the shrine window's): the bitmap at
//! the record's `bitmapPosition` from the window origin by state (up, `bitmapNameInFocus` hovered, `bitmapNameDisabled`
//! disabled), the label centred in its `style`. The main menu's `menubutton` is another class (own text font, offset
//! and disabled tint, `MenuButton::Draw`): `mainmenu.rs`.
//!
//! Button sounds ([`ButtonSound`], [`play_button_sounds`]), Grim Dawn.exe:
//! - in-game buttons (`UIButton`, loader `UIButton::Load`) have `soundNameDown` and `soundNameUp` only: going down on a
//!   press plays `soundNameDown` (`UIButton::Press` / `UIButton::SetDown`); no hover sound;
//! - menu buttons (`menubutton` / `menucheckbutton`, loaders `MenuButton::Load` / `MenuCheckButton::Load`) have
//!   `soundNameOver` and `soundNameDown`: the mouse handler (`MenuButton::HandleMouseEvent`) plays `soundNameOver` when
//!   the cursor enters an enabled button and `soundNameDown` on a left press on it; a disabled one plays neither. The
//!   options pages' check boxes are built in code (`MenuCheckBox::MenuCheckBox`) with the menu constants'
//!   `gemButtonRolloverSound` (none in stock) and `gemButtonClickSound`;
//! - sounds play while the game is paused (`SoundPak::EnablePauseHack`).
//!
//! The button classes' one activation rule ([`button_pressed`]): a left press activates, in game
//! and menu alike (`UIButton::Press` / `MenuButton::HandleMouseEvent`; the level-up indicator's release is the
//! engine's own exception).
//!
//! A window registers each button that sounds while it builds (`Ui::button_sound`, `Ui::record_button_sound`; a window
//! whose own click handler plays the sound registers nothing) and a modal window its rect (`Ui::sound_blocker`; the
//! game menu and the options window the whole screen, so the HUD under them stays silent). PORT: the
//! engine routes the mouse through the widget tree; here the frontmost root ([`RootZ`]: its `GlobalZIndex`,
//! then a HUD group root's local `ZIndex`) with a registered rect under the cursor takes the press and
//! the hover, a window's buttons before its blocker.
use super::Ui;
use bevy::prelude::*;
use gd_formats::ui::{Rect as URect, TextAlign};

/// A button's sounds (or, with none, a window rect that keeps the mouse from the windows behind it), registered while
/// its window builds: a hidden node under the window's root, so it lives and dies with it.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct ButtonSound {
    pub rect: URect,
    /// `soundNameOver`: the cursor enters the button.
    pub over: Option<String>,
    /// `soundNameDown`: a left press on it.
    pub down: Option<String>,
}

impl Ui<'_, '_, '_> {
    /// Registers a sounding button at `rect` (physical px); a disabled button registers nothing.
    pub fn button_sound(&mut self, rect: URect, over: Option<&str>, down: Option<&str>) {
        let s = |v: Option<&str>| v.filter(|s| !s.is_empty()).map(str::to_owned);
        self.spawn_sound(ButtonSound {
            rect,
            over: s(over),
            down: s(down),
        });
    }

    /// [`Self::button_sound`] with a button record's `soundNameOver` / `soundNameDown`.
    pub fn record_button_sound(&mut self, rect: URect, rec: &gd_formats::arz::Record) {
        self.button_sound(rect, rec.str("soundNameOver"), rec.str("soundNameDown"));
    }

    /// [`Self::record_button_sound`] with the button record at path `rec` (nothing when it is missing).
    pub fn record_path_button_sound(&mut self, rect: URect, rec: &str) {
        if let Some(b) = self.record(rec) {
            self.record_button_sound(rect, &b);
        }
    }

    /// A modal window's rect: the buttons of the windows behind it get no press or hover there.
    pub fn sound_blocker(&mut self, rect: URect) {
        self.spawn_sound(ButtonSound {
            rect,
            over: None,
            down: None,
        });
    }

    fn spawn_sound(&mut self, b: ButtonSound) {
        // A probe (rects only, no root) registers nothing.
        if self.root == Entity::PLACEHOLDER {
            return;
        }
        self.commands.spawn((
            b,
            Node {
                display: Display::None,
                ..default()
            },
            ChildOf(self.root),
        ));
    }
}

/// The sounds this frame: the button under the cursor (`hovered`, the frontmost) plays `over` when it was not the
/// hovered one last frame (`last`, its rect) and `down` on a left press.
pub fn button_sounds(
    hovered: Option<&ButtonSound>,
    last: &mut Option<URect>,
    pressed: bool,
) -> Vec<String> {
    let mut out = Vec::new();
    let rect = hovered.map(|b| b.rect);
    if rect != *last
        && let Some(o) = hovered.and_then(|b| b.over.clone())
    {
        out.push(o);
    }
    *last = rect;
    if pressed && let Some(d) = hovered.and_then(|b| b.down.clone()) {
        out.push(d);
    }
    out
}

/// The roots' stacking keys read by [`frontmost`]: overlays carry a `GlobalZIndex`, the HUD groups
/// (`ui::Group`, all at global 0) a local `ZIndex` in their stacking order.
pub type RootZ<'w, 's> = Query<'w, 's, (Option<&'static GlobalZIndex>, Option<&'static ZIndex>)>;

/// The frontmost registered button / blocker under `p`: the highest root `GlobalZIndex`, then the
/// HUD groups' local `ZIndex` (a Character / Skills / Devotion window over the HUD group), and within
/// one window its sounding buttons before its blocker — the one that takes the press and the hover
/// (a modal window's blocker silences and blocks the buttons behind it). Shared by
/// [`play_button_sounds`] and the HUD's button system (`hud.rs`).
pub fn frontmost<'a>(
    p: Vec2,
    buttons: &'a Query<'_, '_, (&ButtonSound, &ChildOf, &InheritedVisibility)>,
    roots: &RootZ,
) -> Option<&'a ButtonSound> {
    buttons
        .iter()
        .filter(|(b, _, v)| v.get() && super::contains(&b.rect, p))
        .max_by_key(|(b, c, _)| {
            let (g, l) = roots.get(c.parent()).map_or((0, 0), |(g, l)| {
                (g.map_or(0, |g| g.0), l.map_or(0, |l| l.0))
            });
            // In one window its buttons are in front of its blocker.
            (g, l, b.over.is_some() || b.down.is_some())
        })
        .map(|(b, ..)| b)
}

/// Plays the registered buttons' hover and press sounds (module doc). Runs before the windows that take the mouse.
pub fn play_button_sounds(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    buttons: Query<(&ButtonSound, &ChildOf, &InheritedVisibility)>,
    roots: RootZ,
    mut last: Local<Option<URect>>,
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
) {
    let p = window.single().ok().and_then(super::cursor_px);
    let hovered = p.and_then(|p| frontmost(p, &buttons, &roots));
    for pak in button_sounds(hovered, &mut last, mouse.just_pressed(MouseButton::Left)) {
        sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
    }
}

/// The button classes' one activation rule: a left PRESS activates. Grim Dawn.exe: `UIButton` (`UIButton::Press`) sets
/// the down flag and plays `soundNameDown` on a press, and the window listeners act on event 0 (the character window
/// `CharacterWindow::OnButtonEvent` `if (a2 != 0) return;`, the HUD toolbar `UIPlayerHud::OnToolbarButton`); the menu's
/// `menubutton` mouse handler `MenuButton::HandleMouseEvent` is the same: mouse event 1 (left press) dispatches
/// listener event 0, event 9 (release) only event 1 / 2 (the up look, inside / outside). The HUD level-up indicator is
/// the engine's own release exception (`UIPlayerHud::OnToolbarButton` event 1). A release never activates a button.
pub fn button_pressed(buttons: &ButtonInput<MouseButton>) -> bool {
    buttons.just_pressed(MouseButton::Left)
}

/// The edit boxes' shared key routing over [`gd_sim::edit`] (the stack amount and the devotion
/// search): Ctrl+A / C / V and the caret / delete keys. `None`: a key the shared routing
/// does not take — the caller applies its own text and Enter rules (Esc is the dispatcher's,
/// `gd_ui::esc_dispatcher`).
pub fn edit_key(
    logical: &bevy::input::keyboard::Key,
    code: KeyCode,
    ctrl: bool,
) -> Option<gd_sim::edit::EditKey> {
    use bevy::input::keyboard::{Key, KeyCode as K};
    use gd_sim::edit::EditKey;
    Some(match (logical, code) {
        (_, K::KeyA) if ctrl => EditKey::SelectAll,
        (_, K::KeyC) if ctrl => EditKey::Copy,
        (_, K::KeyV) if ctrl => EditKey::Paste,
        _ if ctrl => return None,
        (Key::Backspace, _) => EditKey::Backspace,
        (Key::Delete, _) => EditKey::Delete,
        (Key::ArrowLeft, _) => EditKey::Left,
        (Key::ArrowRight, _) => EditKey::Right,
        (Key::Home, _) => EditKey::Home,
        (Key::End, _) => EditKey::End,
        _ => return None,
    })
}

/// What a [`static_button`] writes on itself.
#[derive(Clone, Copy)]
pub enum Label<'a> {
    None,
    /// The record's own `textTag`.
    Record,
    /// This text (already localised).
    Text(&'a str),
}

/// A text in `r` (pixels), drawn with a style from the text styles.
pub fn label(ui: &mut Ui, r: URect, style: &str, align: TextAlign, text: &str) -> Option<Entity> {
    let tb = gd_formats::ui::TextBox {
        style: None,
        rect: gd_formats::ui::Rect::new(r.x / ui.s, r.y / ui.s, r.w / ui.s, r.h / ui.s),
        align_x: align,
        align_y: TextAlign::Center,
        tag: None,
        glow: None,
    };
    ui.text_with_style(&tb, style, Vec2::ZERO, text)
}

/// The auto-sized width of a `ButtonStaticText`: its texture width, grown to fit the label plus
/// `2 * sc(10)` padding when the label is wider (never shrunk below the texture). PORT: the stock
/// button is fixed at its bitmap's pixel width and a long localised label overflows it; here the
/// button grows so the label always fits (UX over parity). The caller stretches the bitmap when the
/// width grows ([`static_button`]); the height stays the texture's.
pub fn button_width(
    ui: &mut Ui,
    rec: &gd_formats::arz::Record,
    tex_w: f32,
    text: Option<&str>,
) -> f32 {
    let lw = match (rec.str("style"), text) {
        (Some(style), Some(t)) => ui.text_width(style, t),
        _ => 0.0,
    };
    tex_w.max(lw + 2.0 * ui.sc(10.0))
}

/// Draws a `ButtonStaticText` record at its `bitmapPosition` from `o`; returns its rect (physical px).
/// The width auto-grows to fit the label ([`button_width`]), stretching the bitmap; the left edge stays
/// at `bitmapPosition` so a caller that centres on a point can re-centre from the returned rect.
/// PORT: a disabled button's label is dimmed like the trade windows' buttons.
pub fn static_button(
    ui: &mut Ui,
    rec: &gd_formats::arz::Record,
    o: Vec2,
    enabled: bool,
    hovered: bool,
    label: Label,
) -> Option<URect> {
    let key = match (enabled, hovered) {
        (false, _) if rec.str("bitmapNameDisabled").is_some() => "bitmapNameDisabled",
        (true, true) if rec.str("bitmapNameInFocus").is_some() => "bitmapNameInFocus",
        _ => "bitmapNameUp",
    };
    let (img, size) = rec.str(key).map(str::to_owned).and_then(|p| ui.image(&p))?;
    let text = match label {
        Label::None => None,
        Label::Record => rec.str("textTag").map(|t| ui.fonts.tag(t).to_string()),
        Label::Text(t) => Some(t.to_owned()),
    };
    let tex_w = ui.sc(size.x as f32);
    let w = button_width(ui, rec, tex_w, text.as_deref());
    let r = URect::new(
        o.x + ui.sc(rec.f32("bitmapPositionX").unwrap_or(0.0)),
        o.y + ui.sc(rec.f32("bitmapPositionY").unwrap_or(0.0)),
        w,
        ui.sc(size.y as f32),
    );
    if w > tex_w {
        ui.quad_stretch(img, r, Color::WHITE);
    } else {
        ui.quad(img, r, None, Color::WHITE);
    }
    if let (Some(style), Some(t)) = (rec.str("style"), text) {
        let e = self::label(ui, r, style, TextAlign::Center, &t);
        if let (false, Some(e)) = (enabled, e) {
            ui.commands.entity(e).queue(|mut em: EntityWorldMut| {
                if let Some(mut i) = em.get_mut::<ImageNode>() {
                    i.color = Color::srgba(0.4, 0.4, 0.4, 1.0);
                }
                if let Some(mut t) = em.get_mut::<TextColor>() {
                    t.0 = t.0.mix(&Color::BLACK, 0.6);
                }
            });
        }
    }
    Some(r)
}
