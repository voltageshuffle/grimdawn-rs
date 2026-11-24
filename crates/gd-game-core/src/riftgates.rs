//! The player's region read from the story runtime.

/// The player's region (the story's `player.region`), which the engine's
/// per-entity sector reads use (`Entity::GetRegion`); `None` without a story.
pub fn player_region(rt: Option<&crate::story::StoryRuntime>) -> Option<usize> {
    rt?.story.borrow().player.region
}
