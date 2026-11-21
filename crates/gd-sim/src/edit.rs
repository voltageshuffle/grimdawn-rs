//! The engine edit box's editing behaviour (`editbox.tpl`, GrimDawn.exe class with vtable @0x140316028;
//! used by the vendor search box). Pure state: the caller feeds key presses / releases / elapsed time and
//! owns the text (`&mut String`), the clipboard and the width limit (`fits`).
//!
//! Key handler `sub_140194A60`, repeat tick `sub_140195270`, delete selection `sub_140195150`,
//! character filter `sub_140194990`:
//! * State: caret (+288), selection start / end (+292 / +296, start <= end) with a flag (+300).
//! * A key press with a selection and no Shift clears the selection first for Left / Right / Home / End.
//! * Left / Right (and every typed character, Backspace, Delete, Ctrl+V) arm the key repeat: the action
//!   runs at once, then after 220 ms, then every 30 ms; any key release stops it (the release event of
//!   any key, not only the repeating one). While repeating, the armed text is inserted again.
//! * Shift+Left / Right with no selection start one at the caret; the selection edge on the caret moves
//!   with it (grows on that side, shrinks when the caret is on the other edge). Without Shift the caret
//!   alone moves by one (a selection is already cleared).
//! * Home: caret 0; with Shift the selection becomes `[0, anchor]` (anchor = the caret, or the edge
//!   opposite the caret). End: caret to the end; with Shift the selection becomes `[caret, end]` (the
//!   engine drops the old anchor). Neither repeats.
//! * Backspace / Delete remove the selection if there is one, else the character before / after the caret.
//! * Ctrl+A selects everything (caret unchanged). Ctrl+C copies the selection (nothing without one).
//!   Ctrl+V deletes the selection and inserts the clipboard text unless it holds a character outside
//!   blank / alpha / digit / punct (or a tab).
//! * Inserting deletes the selection, clamps the caret into the text, inserts at the caret and moves it
//!   after the inserted text; the width limit then cuts the text (`sub_140194F80`).
//!
//! PORT: the maximum character count (+72) and the read-only flag (+76) are not modelled; the width
//! cut trims the end until `fits`; the gamepad input mode (caret forced to the end) is not ported.

/// A key the edit box reacts to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditKey {
    Left,
    Right,
    Home,
    End,
    Backspace,
    Delete,
    SelectAll,
    Copy,
    Paste,
    /// Typed characters.
    Text(String),
}

/// The engine's repeat delays, ms.
pub const REPEAT_FIRST_MS: f32 = 220.0;
pub const REPEAT_NEXT_MS: f32 = 30.0;

#[derive(Clone, Debug, Default)]
pub struct EditState {
    /// Caret position in characters.
    pub caret: usize,
    /// Selection `[start, end)` in characters.
    pub sel: Option<(usize, usize)>,
    repeat: Option<Repeat>,
}

#[derive(Clone, Debug)]
struct Repeat {
    key: Step,
    /// Text inserted per step (typed text or the pasted clipboard).
    text: String,
    delay: f32,
    elapsed: f32,
}

/// What one repeat step does (the armed keys; typed text and Ctrl+V insert).
#[derive(Clone, Copy, Debug)]
enum Step {
    Left,
    Right,
    Backspace,
    Delete,
    Insert,
}

/// `sub_140194990`: a character the box accepts (blank / alpha / digit / punct, no tab).
pub fn accepts(s: &str) -> bool {
    s.chars().all(|c| {
        c != '\t'
            && (c == ' '
                || c.is_alphanumeric()
                || c.is_ascii_punctuation()
                || (!c.is_ascii() && !c.is_control() && !c.is_whitespace()))
    })
}

fn byte(s: &str, idx: usize) -> usize {
    s.char_indices().nth(idx).map_or(s.len(), |(b, _)| b)
}

impl EditState {
    pub fn selection(&self) -> Option<(usize, usize)> {
        self.sel
    }

    /// The caret at the end of `text`, nothing selected, no repeat.
    pub fn to_end(&mut self, text: &str) {
        *self = Self {
            caret: text.chars().count(),
            ..Default::default()
        };
    }

    /// A key press (modifiers: `shift`). Returns text to put on the clipboard (Ctrl+C).
    pub fn press(
        &mut self,
        text: &mut String,
        key: EditKey,
        shift: bool,
        clipboard: &str,
        fits: &dyn Fn(&str) -> bool,
    ) -> Option<String> {
        let len = text.chars().count();
        self.caret = self.caret.min(len);
        if !shift
            && matches!(
                key,
                EditKey::Left | EditKey::Right | EditKey::Home | EditKey::End
            )
        {
            self.sel = None;
        }
        match key {
            EditKey::SelectAll => self.sel = Some((0, len)),
            EditKey::Copy => {
                return self
                    .sel
                    .map(|(a, b)| text.chars().skip(a).take(b - a).collect());
            }
            EditKey::Paste => {
                self.delete_selection(text);
                if !accepts(clipboard) {
                    return None;
                }
                self.arm(Step::Insert, clipboard.to_owned(), text, fits);
            }
            EditKey::Text(t) => {
                if !accepts(&t) {
                    return None;
                }
                self.arm(Step::Insert, t, text, fits);
            }
            EditKey::Home => {
                if shift {
                    let anchor = match self.sel {
                        Some((s, e)) => {
                            if self.caret == e {
                                s
                            } else {
                                e
                            }
                        }
                        None => self.caret,
                    };
                    self.sel = Some((0, anchor));
                }
                self.caret = 0;
            }
            EditKey::End => {
                if shift {
                    self.sel = Some((self.caret, len));
                }
                self.caret = len;
            }
            EditKey::Left | EditKey::Right => {
                if shift && self.sel.is_none() {
                    self.sel = Some((self.caret, self.caret));
                }
                let step = if key == EditKey::Left {
                    Step::Left
                } else {
                    Step::Right
                };
                self.arm(step, String::new(), text, fits);
            }
            EditKey::Backspace => self.arm(Step::Backspace, String::new(), text, fits),
            EditKey::Delete => self.arm(Step::Delete, String::new(), text, fits),
        }
        None
    }

    /// Any key release stops the repeat (`sub_140194A60`: release event).
    pub fn release(&mut self) {
        self.repeat = None;
    }

    /// Advance the repeat timer by `dt_ms`.
    pub fn tick(&mut self, text: &mut String, dt_ms: f32, fits: &dyn Fn(&str) -> bool) {
        let Some(mut r) = self.repeat.take() else {
            return;
        };
        r.elapsed += dt_ms;
        if r.elapsed >= r.delay {
            r.elapsed = 0.0;
            r.delay = REPEAT_NEXT_MS;
            self.step(text, r.key, &r.text, fits);
        }
        self.repeat = Some(r);
    }

    fn arm(&mut self, key: Step, ins: String, text: &mut String, fits: &dyn Fn(&str) -> bool) {
        self.step(text, key, &ins, fits);
        self.repeat = Some(Repeat {
            key,
            text: ins,
            delay: REPEAT_FIRST_MS,
            elapsed: 0.0,
        });
    }

    /// One repeat step (`sub_140195270`).
    fn step(&mut self, text: &mut String, key: Step, ins: &str, fits: &dyn Fn(&str) -> bool) {
        let len = text.chars().count();
        match key {
            Step::Backspace => {
                if self.sel.is_some() {
                    self.delete_selection(text);
                } else if self.caret > 0 && len > 0 {
                    let c = self.caret.min(len);
                    text.remove(byte(text, c - 1));
                    self.caret = c - 1;
                }
            }
            Step::Delete => {
                if self.sel.is_some() {
                    self.delete_selection(text);
                } else if self.caret < len {
                    text.remove(byte(text, self.caret));
                }
            }
            Step::Left if self.caret > 0 => {
                if let Some((s, e)) = &mut self.sel {
                    if *e == self.caret && *s != self.caret {
                        *e -= 1;
                    }
                    if *s == self.caret {
                        *s -= 1;
                    }
                }
                self.caret -= 1;
            }
            Step::Right if self.caret < len => {
                if let Some((s, e)) = &mut self.sel {
                    if *s == self.caret && *e != self.caret {
                        *s += 1;
                    }
                    if *e == self.caret {
                        *e += 1;
                    }
                }
                self.caret += 1;
            }
            Step::Insert => {
                self.delete_selection(text);
                self.caret = self.caret.min(text.chars().count());
                if ins.is_empty() {
                    return;
                }
                let at = byte(text, self.caret);
                text.insert_str(at, ins);
                self.caret += ins.chars().count();
                // The width cut (`sub_140194F80`).
                while !text.is_empty() && !fits(text) {
                    text.pop();
                }
                self.caret = self.caret.min(text.chars().count());
            }
            _ => {}
        }
    }

    /// `sub_140195150`: remove the selection, caret to its start.
    fn delete_selection(&mut self, text: &mut String) {
        if let Some((s, e)) = self.sel.take() {
            let (a, b) = (byte(text, s), byte(text, e.max(s)));
            text.replace_range(a..b, "");
            self.caret = s;
        }
    }
}
