//! The UI notification queue (`GameEngine::AddUINotification` / `TakeTopNotification`) and the timing of the
//! HUD widget that shows it (`NotificationWidget::Update`; drawn by `ui/notify.rs`).
//!
//! - the queue is FIFO; `AddUINotification(type, text, clear)` empties it first when `clear` is set. Every
//!   reversed error sender passes `clear` (`tagTransferStashError`, `tagAscensionAltarFull`,
//!   `tagShrineActivated` / `tagShrineHostNotActivated`); only `UIPlayerHud::ShowFormattedNotification` doesn't.
//!   The type is never read by the widget.
//! - the widget shows one notification at a time, taking the next while idle; the alpha is `t × 0.008` for the
//!   first 125 ms, 1 up to 2500 ms, then `1 − (t − 2500) × 0.002` down to 0 at 3000 ms, when it goes idle.
use std::collections::VecDeque;

/// One notification's display length (ms).
pub const SHOW_MS: f32 = 3000.0;
const FADE_IN_MS: f32 = 125.0;
const FADE_OUT_FROM_MS: f32 = 2500.0;

#[derive(Default, Debug)]
pub struct NotificationQueue {
    queue: VecDeque<String>,
    /// The one shown and its clock (ms).
    shown: Option<(String, f32)>,
}

impl NotificationQueue {
    /// `AddUINotification`: queues `text`, after emptying the queue when `clear` is set (the one on screen stays).
    pub fn push(&mut self, text: String, clear: bool) {
        if clear {
            self.queue.clear();
        }
        self.queue.push_back(text);
    }

    /// The widget's update: advances the shown one by `dt` ms, or takes the next one while idle.
    pub fn tick(&mut self, dt: f32) {
        match &mut self.shown {
            Some((_, t)) => {
                *t += dt;
                if *t > SHOW_MS {
                    self.shown = None;
                }
            }
            None => self.shown = self.queue.pop_front().map(|s| (s, 0.0)),
        }
    }

    pub fn current(&self) -> Option<(&str, f32)> {
        self.shown.as_ref().map(|(s, t)| (s.as_str(), alpha(*t)))
    }

    /// Everything not yet gone: the one shown, then the queue.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.shown
            .iter()
            .map(|(s, _)| s.as_str())
            .chain(self.queue.iter().map(String::as_str))
    }

    pub fn is_empty(&self) -> bool {
        self.shown.is_none() && self.queue.is_empty()
    }
}

fn alpha(t: f32) -> f32 {
    if t < FADE_IN_MS {
        t * 0.008
    } else if t <= FADE_OUT_FROM_MS {
        1.0
    } else {
        (1.0 - (t - FADE_OUT_FROM_MS) * 0.002).max(0.0)
    }
}
