//! Reflect links (`Skill_AttackSpellReflectLink`: the Necromancer's Mark of
//! Torment, `playerclass08/soultransfer.dbr`, and Blackheart's monster copy):
//! the caster binds itself to a foe; while the link lasts, damage the caster
//! takes is cut and part of it goes on to the linked foe.
//!
//! Game.dll:
//! - `Load`: `linkName` (the beam), `maxRange`, `emitPointName`
//!   (default `FXCentered`), `linkDelay` (s → ms), `linkLimit` (default 1),
//!   `startSound`, `endSound`, `loopSound`, `startEffect`, `endEffect`; the
//!   profile gives `damageAbsorptionPercent` (in the `SkillProfile`),
//!   `damageAbsorptionReflectPercent` (per level) and `skillActiveDuration`
//!   (`GetMaxDuration`).
//! - `ActivateNow` (Ashes of Malmouth loaded): the target must
//!   be a living character. A link to it already: its time is reset to the
//!   full duration, the cooldown starts and the energy is paid. Else, with
//!   `linkLimit` links, the oldest is ended; the caster's emit point must be
//!   within `maxRange` of the target's (else nothing happens: no cooldown,
//!   no cost); a link is made (beam, start / end effects, loop sound, the
//!   duration, `linkDelay` only for the main player), a `CombatFilter` added
//!   to the caster, the cooldown started and the energy paid.
//! - `Update`: every link's time runs down; at 0, or when its
//!   target is gone or dead, it ends. After the start (the start effect) it
//!   waits out the delay, then shows the beam and loops its sound; from then
//!   on it ends when caster and target are `maxRange` apart or more. Ending:
//!   the end sound, the effects stop, the beam goes, the filter is removed.
//! - Damage the caster takes (`CombatManager::TakeAttack`,
//!   `SkillManager::GetReflectLinkDamageReduction` /
//!   `SkillManager::ReflectLinkDamage`): the link's filter
//!   (`ModifyDamage`) cuts every damage application by
//!   `damageAbsorptionPercent` per link; after the defenses a copy of the
//!   attack scaled by the links' summed `damageAbsorptionPercent` is, for
//!   attacks other than retaliation and reflection, sent through
//!   `Skill_AttackSpellReflectLink::ReflectLinkDamage`: per link, scaled by
//!   `damageAbsorptionReflectPercent` (debuffs by at most the player's reflect
//!   cap), a style-6 attack of the caster on the link's target
//!   (`Character::AttackTarget`). So at level 1 the caster takes 85 % and the
//!   target gets 15 % × 200 % = 30 % of the defended damage, through its own
//!   defenses (`gd_sim::actor::attack_linked`, `link_reflection`, `reflect`;
//!   `combat::apply_hits`).
//!
//! - DoT ticks (`DurationDamageManager::ExecuteDamage` →
//!   `CombatManager::ApplyDamage`, which runs every filter) are
//!   cut too, but only `TakeAttack` passes damage on (`afflictions::tick`).
//!
//! PORT: the link starts when the cast fires; an out-of-range target
//! (feet to aim point, checked before the cast pays,
//! `casting::try_cast`) is refused unpaid like the engine's; the reflected
//! attack is a [`crate::combat::CounterHit`] (combat numbers) plus a
//! [`crate::combat::LinkedAttack`] (the caster's `AttackEnemy(Crit)` autocast
//! triggers, as `AttackTarget` runs them); a dead
//! caster's links end; the range is measured centre to centre (not emit
//! point to emit point); the beam and effects are not drawn; the sounds: `crate::combat_sounds::held_loops`.
use bevy::prelude::*;
use gd_game_core::combat::Dead;
use gd_game_core::data::GameData;
use gd_game_core::reflect_link::{ReflectLinks, StartLink, new_link};

pub struct ReflectLinkPlugin;

impl Plugin for ReflectLinkPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<StartLink>().add_systems(
            Update,
            (start_links, update_links)
                .chain()
                .after(crate::casting::CastingSet)
                .before(gd_game_core::combat::CombatSet),
        );
    }
}

fn start_links(
    mut commands: Commands,
    mut events: MessageReader<StartLink>,
    data: Res<GameData>,
    mut casters: Query<(
        &GlobalTransform,
        Option<&mut ReflectLinks>,
        Has<gd_game_core::player::Player>,
    )>,
    targets: Query<&GlobalTransform, (With<gd_game_core::combat::Combatant>, Without<Dead>)>,
) {
    for ev in events.read() {
        let Some(rec) = data.db.get(&ev.skill) else {
            continue;
        };
        let Some(target) = ev.target else {
            debug!("reflect link {}: no target", ev.skill);
            continue;
        };
        let Ok(tgt) = targets.get(target) else {
            continue;
        };
        let Ok((gt, links, is_player)) = casters.get_mut(ev.caster) else {
            continue;
        };
        let li = ev.level_index;
        let secs = gd_game_core::casting::level_f(rec, "skillActiveDuration", li);
        let mut links = match links {
            Some(l) => l,
            None => {
                let mut l = ReflectLinks::default();
                if let Some(link) = new_link(
                    rec,
                    &ev.skill,
                    li,
                    target,
                    secs,
                    is_player,
                    gt.translation(),
                    tgt.translation(),
                ) {
                    l.0.push(link);
                    commands.entity(ev.caster).try_insert(l);
                }
                continue;
            }
        };
        if let Some(l) = links
            .0
            .iter_mut()
            .find(|l| l.target == target && l.skill == ev.skill)
        {
            l.left = secs;
            continue;
        }
        let limit = rec.i32("linkLimit").unwrap_or(1).max(0) as usize;
        if limit > 0
            && links.0.iter().filter(|l| l.skill == ev.skill).count() >= limit
            && let Some(i) = links.0.iter().position(|l| l.skill == ev.skill)
        {
            links.0.remove(i);
        }
        if let Some(link) = new_link(
            rec,
            &ev.skill,
            li,
            target,
            secs,
            is_player,
            gt.translation(),
            tgt.translation(),
        ) {
            links.0.push(link);
        }
    }
}

fn update_links(
    mut commands: Commands,
    time: Res<Time>,
    mut casters: Query<(Entity, &GlobalTransform, &mut ReflectLinks, Has<Dead>)>,
    targets: Query<&GlobalTransform, (With<gd_game_core::combat::Combatant>, Without<Dead>)>,
) {
    let dt = time.delta_secs();
    for (e, gt, mut links, dead) in &mut casters {
        links.0.retain_mut(|l| {
            l.left -= dt;
            let Ok(t) = targets.get(l.target) else {
                return false;
            };
            if l.left <= 0.0 || dead {
                return false;
            }
            if l.delay > 0.0 {
                l.delay -= dt;
                return true;
            }
            gt.translation().distance(t.translation()) < l.max_range
        });
        if links.0.is_empty() {
            commands.entity(e).try_remove::<ReflectLinks>();
        }
    }
}
