//! Keyboard Only's controller targeting (`movementType` 2; the gamepad's too): with the cursor out of the aim,
//! every frame the game picks what the player faces and makes it the controller's cursor record and combat
//! enemy / ally. The game gathers the [`Candidate`]s; [`pick`] is the rule.
//!
//! `InGameUI::Update`, each frame when `InputMode == 0 && movementType == 2` and no window blocks it
//! (`Ui::IsAnyWindowOpen`; then the record stays): the record and the combat enemy / ally are cleared, then
//! `InGameUI::CollectEnemyCandidates` scans every entity within 20 m inside the camera frustum. `d` is the
//! horizontal distance; the cone test ([`in_cone`]) needs the cosine to the aim direction at least `cos 27°`
//! from 5 m on, at least 0 nearer.
//! - a foe (alive or of interest, targetable, not hiding) within 8 m joins the near list; one in the cone the
//!   enemy candidates, with targetable unbroken `Destructible`s in the cone;
//! - the nearest item of interest that passes the label filter within 5 m;
//! - the nearest friendly `Npc` within 5 m and its usable range (`Character::GetUsableRange`: both radii + 3 m);
//! - the nearest interactable fixed actor in the cone within 10 m and its usable range
//!   (`FixedActor::GetUsableRange`); a fixed actor nearer than the NPC drops the NPC, else the NPC drops it.
//!
//! A near foe that causes anger (`InGameUI::IsAngryFoe`) holds the combat flag 500 ms. The enemy is the
//! candidate nearest to the player (`InGameUI::PickNearestEnemy`). The first that applies is the [`Pick`]:
//! a container out of combat, a breakable enemy out of combat, an item out of combat, an NPC out of combat, a
//! fixed actor (out of combat, or always when not a shrine / blast container), the enemy.
//!
//! PORT: the game supplies the frustum test, the interactable fixed actors (`IsOfInterest`) and the label
//! filter.
//!
//! The Interact key (`tagUse`, [`interact_pick`]; `UIPlayerHud::InteractNearest`), in every movement mode:
//! of the entities within 10 m (`GetEntitiesInSphere`), in the query's order, an interactable `FixedActor`
//! within its usable range (3D) and any `Npc` (no range, no availability test) become the best when nearer
//! than the best so far (one best distance for both); a fixed actor found wins over the NPC. PORT: the query's
//! order is the game's candidate order.
use bevy::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// A foe character: `causes_anger` for the combat flag.
    Foe { causes_anger: bool },
    /// A targetable, unbroken `Destructible`.
    Breakable,
    /// A ground item of interest whose label passes the filter.
    Item,
    /// A friendly `Npc` available for conversations; `usable` = `GetUsableRange`.
    Npc { usable: f32 },
    /// An interactable fixed actor; `usable` = `GetUsableRange`.
    Fixed { kind: FixedKind, usable: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixedKind {
    /// `FixedItemContainer`: a chest.
    Container,
    /// `StaticShrine` / `FixedItemBlastContainer`: not picked in combat.
    Shrine,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub entity: Entity,
    pub pos: Vec3,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Pick {
    #[default]
    None,
    /// The record (its coordinates) and the combat enemy: a foe or a breakable.
    Enemy(Entity),
    /// The record and the combat ally.
    Npc(Entity),
    /// Highlighted only (the record stays empty).
    Fixed(Entity),
    /// The selected ground label (the record stays empty).
    Item(Entity),
}

impl Pick {
    pub fn entity(self) -> Option<Entity> {
        match self {
            Pick::None => None,
            Pick::Enemy(e) | Pick::Npc(e) | Pick::Fixed(e) | Pick::Item(e) => Some(e),
        }
    }
}

/// `GetEntitiesInSphere` radius.
pub const SCAN_RADIUS: f32 = 20.0;
/// How long the combat flag holds after the last frame with a near angry foe (`InGameUI.lastAngryTime`).
pub const COMBAT_HOLD: f32 = 0.5;

/// The time of the last frame with a near angry foe.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Targeting {
    pub angry_at: Option<f32>,
}

/// `InGameUI::CollectEnemyCandidates`'s cone test.
pub fn in_cone(cos: f32, d: f32) -> bool {
    cos >= if d >= 5.0 { 0.891_006_5 } else { -4.371_139e-8 }
}

/// The pick of this frame at `now` (s).
pub fn pick(
    me: Vec3,
    aim: Vec3,
    candidates: &[Candidate],
    state: &mut Targeting,
    now: f32,
) -> Pick {
    let aim = aim.with_y(0.0);
    let mut enemy: Option<(f32, Entity, bool)> = None;
    let mut item: Option<(f32, Entity)> = None;
    let (mut npc, mut npc_d): (Option<Entity>, f32) = (None, 5.0);
    let (mut fixed, mut fixed_d): (Option<(Entity, FixedKind)>, f32) = (None, 10.0);
    let mut angry = false;
    for c in candidates {
        let off = (c.pos - me).with_y(0.0);
        let d = off.length();
        if d > SCAN_RADIUS {
            continue;
        }
        let cos = if d > 0.0 { off.dot(aim) / d } else { 0.0 };
        let cone = in_cone(cos, d);
        let d3 = (c.pos - me).length_squared();
        let mut enemy_candidate = |breakable: bool| {
            if enemy.is_none_or(|b| d3 < b.0) {
                enemy = Some((d3, c.entity, breakable));
            }
        };
        match c.kind {
            Kind::Foe { causes_anger } => {
                angry |= d < 8.0 && causes_anger;
                if cone {
                    enemy_candidate(false);
                }
            }
            Kind::Breakable if cone => enemy_candidate(true),
            Kind::Breakable => {}
            Kind::Item => {
                if d < 5.0 && item.is_none_or(|b| d < b.0) {
                    item = Some((d, c.entity));
                }
            }
            Kind::Npc { usable } => {
                if d < npc_d && d < usable {
                    npc = Some(c.entity);
                    npc_d = d;
                }
            }
            Kind::Fixed { kind, usable } => {
                if d < fixed_d && d < usable && cone {
                    fixed = Some((c.entity, kind));
                    fixed_d = d;
                }
            }
        }
    }
    if fixed_d < npc_d {
        npc = None;
    } else if npc.is_some() {
        fixed = None;
    }
    if angry {
        state.angry_at = Some(now);
    }
    let combat = state.angry_at.is_some_and(|t| now - t < COMBAT_HOLD);
    if let Some((e, FixedKind::Container)) = fixed
        && !combat
    {
        return Pick::Fixed(e);
    }
    if let Some((_, e, true)) = enemy
        && !combat
    {
        return Pick::Enemy(e);
    }
    if let Some((_, e)) = item
        && !combat
    {
        return Pick::Item(e);
    }
    if let Some(e) = npc
        && !combat
    {
        return Pick::Npc(e);
    }
    if let Some((e, kind)) = fixed
        && (kind != FixedKind::Shrine || !combat)
    {
        return Pick::Fixed(e);
    }
    enemy.map_or(Pick::None, |(_, e, _)| Pick::Enemy(e))
}

/// Keyboard Only's pick for the controller's cursor record (`SetMouseRepeatData`): the enemy or NPC and its
/// coordinates, else an empty record.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct ControllerTarget {
    pub target: Option<Entity>,
    pub point: Option<Vec3>,
}

/// The Interact key's request: the object systems take it as a click on theirs.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct UseObject(pub Entity);

/// `GetEntitiesInSphere` radius (`UIPlayerHud::InteractNearest`).
pub const INTERACT_RADIUS: f32 = 10.0;

/// What the Interact key uses: of the `Fixed` and `Npc` candidates, in their order, the fixed actor within its
/// usable range nearer than the best so far, else the NPC nearer than it.
pub fn interact_pick(me: Vec3, candidates: &[Candidate]) -> Option<Entity> {
    let mut best = f32::INFINITY;
    let (mut fixed, mut npc) = (None, None);
    for c in candidates {
        let d = c.pos.distance(me);
        if d > INTERACT_RADIUS {
            continue;
        }
        match c.kind {
            Kind::Fixed { usable, .. } if d <= usable && d < best => {
                fixed = Some(c.entity);
                best = d;
            }
            Kind::Npc { .. } if d < best => {
                npc = Some(c.entity);
                best = d;
            }
            _ => {}
        }
    }
    fixed.or(npc)
}
