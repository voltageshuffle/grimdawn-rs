/// The player's region (`Entity::GetRegion`); `None` without a story.
pub fn player_region(rt: Option<&crate::story::StoryRuntime>) -> Option<usize> {
    rt?.story.borrow().player.region
}
