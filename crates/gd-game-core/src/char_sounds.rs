//! The weapon-swipe predicate over a cast's kind.

/// Whether a swipe callback of the animation slot `anim` reaches a weapon
/// skill's `SwipeAction` (`SkillActivatedWeapon` and its kin; the spells' is
/// only their swipe sound, `gd_game::combat_sounds::swipe_action`): the default
/// attack (the Attack or an `AttTurn*` slot) or a cast of a weapon skill (`cast`, e.g. a form's
/// claws in their special animation).
/// PORT: the cast's kind stands in for the skill class's `SwipeAction`.
pub fn weapon_swipe(anim: gd_sim::anim::AnimType, cast: Option<&crate::casting::Casting>) -> bool {
    gd_sim::anim::is_attack(anim) || cast.is_some_and(|c| c.kind == crate::casting::Kind::Weapon)
}
