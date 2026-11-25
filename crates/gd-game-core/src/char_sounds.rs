//! The weapon-swipe predicate over a cast's kind.

/// Whether a swipe callback of `anim` reaches a weapon skill's `SwipeAction`: the default
/// attack (Attack or an `AttTurn*` slot) or a weapon-skill cast (e.g. a form's claws).
/// PORT: the cast's kind stands in for the skill class's `SwipeAction`.
pub fn weapon_swipe(anim: gd_sim::anim::AnimType, cast: Option<&crate::casting::Casting>) -> bool {
    gd_sim::anim::is_attack(anim) || cast.is_some_and(|c| c.kind == crate::casting::Kind::Weapon)
}
