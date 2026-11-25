//! The HUD notification flow. One way in: systems write a [`gd_game_core::story::Notify`]
//! message, and [`queue_notices`] (PostUpdate, after every writer of the frame) queues it
//! into [`gd_game_core::story::StoryRuntime::notices`] and runs the widget's update.
use bevy::prelude::*;

/// `GameEngine::AddUINotification`: the tag localised, the queue emptied first; then the
/// widget's update. Without a story (no session) the notices are dropped.
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
