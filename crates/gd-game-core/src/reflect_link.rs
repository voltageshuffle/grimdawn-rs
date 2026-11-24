//! Reflect-link data: the cast message ([`StartLink`]), one link ([`Link`]),
//! the caster's link list ([`ReflectLinks`]) and the link constructor
//! ([`new_link`]). The systems that start, tick and end links are gd-game's
//! `reflect_link` (its module docs carry the full engine trace).
use bevy::prelude::*;

/// A reflect-link cast fired at `target` (`casting::fire_casts`).
#[derive(Message, Clone, Debug)]
pub struct StartLink {
    pub caster: Entity,
    pub target: Option<Entity>,
    pub skill: String,
    pub level_index: usize,
}

/// One link (`ActivateNow`'s list entry).
#[derive(Clone, Debug)]
pub struct Link {
    pub target: Entity,
    pub skill: String,
    /// Seconds left (`skillActiveDuration`).
    pub left: f32,
    /// `linkDelay` left: the range check starts after it.
    pub delay: f32,
    pub max_range: f32,
    /// `damageAbsorptionPercent`.
    pub absorb: f32,
    /// `damageAbsorptionReflectPercent`.
    pub reflect: f32,
}

/// The caster's links, oldest first.
#[derive(Component, Clone, Debug, Default)]
pub struct ReflectLinks(pub Vec<Link>);

impl Link {
    /// The `linkDelay` is over: the beam is in the world (`Update` state 2 on).
    pub fn shown(&self) -> bool {
        self.delay <= 0.0
    }
}

impl ReflectLinks {
    /// `GetReflectLinkDamageReduction`: the links' summed absorption.
    pub fn absorb(&self) -> f32 {
        self.0.iter().map(|l| l.absorb.max(0.0)).sum()
    }
}

/// A new link, if the target is within `maxRange` (PORT: centre to centre).
#[allow(clippy::too_many_arguments)]
pub fn new_link(
    rec: &gd_formats::arz::Record,
    skill: &str,
    li: usize,
    target: Entity,
    secs: f32,
    is_player: bool,
    from: Vec3,
    to: Vec3,
) -> Option<Link> {
    let max_range = rec.f32("maxRange").unwrap_or(0.0);
    if from.distance(to) > max_range {
        debug!("reflect link {skill}: target out of range");
        return None;
    }
    debug!("reflect link {skill} on {target} for {secs} s");
    Some(Link {
        target,
        skill: skill.to_owned(),
        left: secs,
        delay: if is_player {
            rec.f32("linkDelay").unwrap_or(0.0).max(0.0)
        } else {
            0.0
        },
        max_range,
        absorb: crate::casting::level_f(rec, "damageAbsorptionPercent", li),
        reflect: crate::casting::level_f(rec, "damageAbsorptionReflectPercent", li),
    })
}
