//! The UI notification queue (`GameEngine::AddUINotification` / `TakeTopNotification`, Game.dll 0x1802d46a0 /
//! 0x1802d45d0) and the presentation timing of the HUD's notification widget that shows it (`Grim Dawn.exe`, the
//! widget at UIPlayerHud+318424, update sub_1401FF0A0, draw sub_1401FEEA0; the widget itself is `ui/notify.rs`).
//!
//! - the queue is FIFO (`TakeTopNotification` takes the front); `AddUINotification(type, text, clear)` empties it
//!   first when `clear` is set. Every reversed error sender passes `clear`: `tagTransferStashError` (bag / stash
//!   transfers, sub_14012F830 / sub_1401BC820 / BagWidget handlers / sub_14025E340), `tagAscensionAltarFull`
//!   (sub_140119CB0), `StaticShrine::RequestToUse`'s `tagShrineActivated` / `tagShrineHostNotActivated`; the only
//!   sender without it found is sub_14022F7C0 (a localised text with arguments). The type is stored but never read
//!   by the widget.
//! - the widget shows one notification at a time: while idle it takes the next one and restarts its clock; the
//!   alpha is `t × 0.008` for the first 125 ms, 1 up to 2500 ms, then `1 − (t − 2500) × 0.002` down to 0 at 3000 ms,
//!   when the widget goes idle again (the next one is taken on the following update).
//!
//! The queue lives in [`crate::story::StoryRuntime::notices`]. One way in: gameplay, UI and input systems write a
//! [`crate::story::Notify`] message, and [`queue_notices`] (PostUpdate: after every writer of the frame's Update,
//! whichever plugin it is in) queues it through `StoryRuntime::notify` and then runs the widget's update. The UI
//! only reads [`NotificationQueue::current`] (a notice starts at alpha 0, so the frame between is not seen).
use bevy::prelude::*;
use std::collections::VecDeque;

/// One notification's display length (ms) and its fades (sub_1401FF0A0).
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

    /// The notification on screen and its alpha.
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

/// sub_1401FF0A0's alpha at `t` ms.
fn alpha(t: f32) -> f32 {
    if t < FADE_IN_MS {
        t * 0.008
    } else if t <= FADE_OUT_FROM_MS {
        1.0
    } else {
        (1.0 - (t - FADE_OUT_FROM_MS) * 0.002).max(0.0)
    }
}

/// The frame's [`crate::story::Notify`] messages into the queue (`GameEngine::AddUINotification`: the tag
/// localised, the queue emptied first), then the widget's update. Registered with the message
/// (`story::StoryEffectsPlugin`). Without a story (no session) the notices are dropped: no HUD shows them.
pub fn queue_notices(
    mut events: MessageReader<crate::story::Notify>,
    // Option: the story exists only in a session.
    rt: Option<NonSendMut<crate::story::StoryRuntime>>,
    time: Res<Time>,
) {
    let Some(mut rt) = rt else {
        events.clear();
        return;
    };
    for crate::story::Notify(tag) in events.read() {
        rt.notify(tag);
    }
    rt.notices.tick(time.delta_secs() * 1000.0);
}
