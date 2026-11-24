//! The HUD notification flow: the queue data and the widget's presentation timing are
//! [`gd_game_core::notify`] (`NotificationWidget::Update` / `Render`; the widget itself is `ui/notify.rs`).
//!
//! The queue lives in [`gd_game_core::story::StoryRuntime::notices`]. One way in: gameplay, UI and input systems write a
//! [`gd_game_core::story::Notify`] message, and [`queue_notices`] (PostUpdate: after every writer of the frame's Update,
//! whichever plugin it is in) queues it through `StoryRuntime::notify` and then runs the widget's update. The UI
//! only reads [`gd_game_core::notify::NotificationQueue::current`] (a notice starts at alpha 0, so the frame between is not seen).
use bevy::prelude::*;

/// The frame's [`gd_game_core::story::Notify`] messages into the queue (`GameEngine::AddUINotification`: the tag
/// localised, the queue emptied first), then the widget's update. Registered with the message
/// (`story::StoryEffectsPlugin`). Without a story (no session) the notices are dropped: no HUD shows them.
pub fn queue_notices(
    mut events: MessageReader<gd_game_core::story::Notify>,
    // Option: the story exists only in a session.
    rt: Option<NonSendMut<gd_game_core::story::StoryRuntime>>,
    time: Res<Time>,
) {
    let Some(mut rt) = rt else {
        events.clear();
        return;
    };
    for gd_game_core::story::Notify(tag) in events.read() {
        rt.notify(tag);
    }
    rt.notices.tick(time.delta_secs() * 1000.0);
}
