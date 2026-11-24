//! The tether data of `SkillSecondary_Tether`: a caster's active beams.
use bevy::prelude::*;

/// One tether of a caster ([`Tethers`]).
#[derive(Clone, Debug)]
pub struct Tether {
    pub target: Entity,
    pub skill: String,
    pub level_index: usize,
    /// Seconds left, seconds to the next hit.
    pub left: f32,
    pub tick: f32,
    /// `tetherDelay` left: the beam shows and the hits start after it.
    pub delay: f32,
    pub max_range: f32,
    /// Seconds between hits (`timeBetweenAttacks` / spell cast speed).
    pub every: f32,
    /// `tetherThickness`: how far from the beam a foe is hit.
    pub thickness: f32,
    /// `hurtsTarget`: the tethered target is hit too.
    pub hurts_target: bool,
}

/// Active tethers of a caster.
#[derive(Component, Default)]
pub struct Tethers(pub Vec<Tether>);
