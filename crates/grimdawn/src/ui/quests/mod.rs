//! The quest view collector: builds `gd_ui::quests::QuestView` from the story's quest log in the
//! streamed map's repository order. The journal / tracker / lore windows are `gd_ui::quests`.
mod collect;
pub use collect::collect;
